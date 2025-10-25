use renovatio::{Plugin, PluginProgress, execute};

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Root {
    pub api_version: String,
    pub kind: String,
    pub metadata: Metadata,
    pub spec: Spec,
    pub status: Status,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Metadata {
    pub name: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Spec {
    pub boot_order: String,
    pub image: Image,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Image {
    pub image: String,
    pub signature: String,
    pub transport: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub booted: Booted,
    pub rollback: Rollback,
    pub rollback_queued: bool,
    pub staged: Option<Staged>,
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Booted {
    pub cached_update: Option<CachedUpdate>,
    pub image: Image3,
    pub incompatible: bool,
    pub ostree: Ostree,
    pub pinned: bool,
    pub soft_reboot_capable: bool,
    pub store: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CachedUpdate {
    pub architecture: String,
    pub image: Image2,
    pub image_digest: String,
    pub timestamp: String,
    pub version: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Image2 {
    pub image: String,
    pub signature: String,
    pub transport: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Image3 {
    pub architecture: String,
    pub image: Image4,
    pub image_digest: String,
    pub timestamp: String,
    pub version: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Image4 {
    pub image: String,
    pub signature: String,
    pub transport: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ostree {
    pub checksum: String,
    pub deploy_serial: i64,
    pub stateroot: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rollback {
    pub cached_update: Value,
    pub image: Value,
    pub incompatible: bool,
    pub ostree: Ostree2,
    pub pinned: bool,
    pub soft_reboot_capable: bool,
    pub store: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ostree2 {
    pub checksum: String,
    pub deploy_serial: i64,
    pub stateroot: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Staged {
    pub cached_update: Value,
    pub image: Image5,
    pub incompatible: bool,
    pub ostree: Ostree3,
    pub pinned: bool,
    pub soft_reboot_capable: bool,
    pub store: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Image5 {
    pub architecture: String,
    pub image: Image6,
    pub image_digest: String,
    pub timestamp: String,
    pub version: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Image6 {
    pub image: String,
    pub signature: String,
    pub transport: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ostree3 {
    pub checksum: String,
    pub deploy_serial: i64,
    pub stateroot: String,
}

// Implementation of uupd
pub struct Bootc;

impl Plugin for Bootc {
    fn name(&self) -> &str {
        "bootc"
    }

    fn description(&self) -> &str {
        "Update the OS via bootc."
    }

    fn version(&self) -> &str {
        // TODO: implement versioning
        "0.1.0"
    }

    /// Uupd conflicts with all other plugins
    fn conflicts(&self, _plugin_name: &str) -> bool {
        true
    }

    /// Run uupd
    extern "Rust" fn update(&self, tx: flume::Sender<PluginProgress>) -> bool {
        let mut pgrss = PluginProgress::new(self.name());
        pgrss.progress = 0;
        pgrss.pulse = true;
        pgrss.status = "Checking for updates...".to_string();
        let _ = tx.send(pgrss.clone());

        // Check the status to see if there's an update available
        if let Some(status) = get_status() {
            // Check to see if there's an update
            if status.status.booted.cached_update.is_some() {
                // There is an update cached, so we need to reboot

                let new_version = status.status.booted.cached_update.unwrap().version;

                // There's a cached update, so a) update the progress and b) run the upgrade
                pgrss.status = format!("OS upgrade to {} pending reboot", new_version);
                pgrss.progress = 100;
                pgrss.pulse = false;
                pgrss.reboot_required = true;

                let _ = tx.send(pgrss.clone());
            } else {
                // There's a cached update, so a) update the progress and b) run the upgrade
                pgrss.status = "Checking for updates...".to_string();
                let _ = tx.send(pgrss.clone());

                let (stdout, stderr, rc) = upgrade();
                if rc == 0 {
                    pgrss.stdout = Some(stdout.clone());
                } else {
                    pgrss.stderr = Some(stderr.clone());
                }

                // When the upgrade is complete, signal that we're done.
                pgrss.progress = 100;
                pgrss.pulse = false;

                if stdout.contains("No changes in") {
                    pgrss.status = "No updates available".to_string();
                } else {
                    pgrss.reboot_required = true;
                    pgrss.status = "OS upgrade pending reboot".to_string();
                }

                let _ = tx.send(pgrss.clone());
            }
        } else {
            println!("No status available");
        }

        true
    }
}

// Export a function to create an instance of the plugin
#[unsafe(no_mangle)]
pub fn create_plugin() -> *mut dyn Plugin {
    Box::into_raw(Box::new(Bootc))
}

fn get_status() -> Option<Root> {
    // execute `bootc status --json`
    let (stdout, _stderr, rc) = execute("pkexec bootc status --json");

    if rc != 0 {
        return None;
    }

    // deserialize the json to a Root structure
    let root: Root = serde_json::from_str(&stdout).expect("Failed to deserialize JSON");

    Some(root)
}

/// Run the `bootc upgrade` command
fn upgrade() -> (String, String, i32) {
    execute("pkexec bootc upgrade")
}
