use renovatio::{Plugin, PluginProgress, execute};

pub struct Flatpak;
// Implementation of flatpak

impl Plugin for Flatpak {
    fn name(&self) -> &str {
        "flatpak"
    }

    fn description(&self) -> &str {
        "Update user and system flatpaks."
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

        // Find the system flatpaks needing update
        let system = list_updates(true);
        let user = list_updates(false);

        // calculate the total progress based on the number of outdated flatpaks
        let total_progress = system.len() + user.len();

        // Figure out how much each step should progress
        let mut step_progress = 100;
        if total_progress > 0 {
            step_progress = 100 / total_progress;
        }

        let mut pgrss_clone = pgrss.clone();
        let mut upgrade = |flatpak: &str, installation: &str| {
            pgrss_clone.status = format!("Upgrading {} flatpak {}...", installation, flatpak);
            pgrss_clone.stdout = None;
            pgrss_clone.stderr = None;
            let _ = tx.send(pgrss_clone.clone());

            let (stdout, stderr, success) = upgrade_flatpak(flatpak);

            if success != 0 {
                pgrss_clone.status =
                    format!("Failed to upgrade {} flatpak {}", installation, flatpak);
                pgrss_clone.stderr = Some(stderr.clone());
                let _ = tx.send(pgrss_clone.clone());
                // Continue updating
            }

            pgrss_clone.progress += step_progress as u32;
            pgrss_clone.stdout = Some(stdout.clone());
            if !stderr.is_empty() {
                pgrss_clone.stderr = Some(stderr.clone());
            }
            let _ = tx.send(pgrss_clone.clone());
        };

        pgrss.status = "Upgrading flatpaks...".to_string();
        pgrss.progress = 0;
        pgrss.stdout = None;
        pgrss.stderr = None;
        let _ = tx.send(pgrss.clone());

        for flatpak in system {
            upgrade(&flatpak, "system");
        }

        for flatpak in user {
            upgrade(&flatpak, "user");
        }

        pgrss.progress = 100;
        pgrss.stdout = None;
        pgrss.stderr = None;

        pgrss.status = "Upgrade completed!".to_string();
        let _ = tx.send(pgrss.clone());

        true
    }
}

// Export a function to create an instance of the plugin
#[unsafe(no_mangle)]
pub fn create_plugin() -> *mut dyn Plugin {
    Box::into_raw(Box::new(Flatpak))
}

fn list_updates(system: bool) -> Vec<String> {
    let mut updates = Vec::new();
    let mut flag = "--system";
    if !system {
        flag = "--user";
    }
    let (stdout, _stderr, success) =
        execute(format!("flatpak remote-ls --updates --columns=application {}", flag).as_str());
    if success != 0 {
        return updates;
    }

    for line in stdout.lines().skip(1) {
        updates.push(line.to_string());
    }

    updates
}

fn upgrade_flatpak(name: &str) -> (String, String, i32) {
    execute(format!("flatpak update --noninteractive -y {}", name).as_str())
}
