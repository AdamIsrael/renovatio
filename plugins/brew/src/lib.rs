use renovatio::{Plugin, PluginProgress, execute};

use serde::{Deserialize, Serialize};

// brew outdated --json
// {
//   "formulae": [
//     {
//       "name": "k9s",
//       "installed_versions": [
//         "0.50.15"
//       ],
//       "current_version": "0.50.16",
//       "pinned": false,
//       "pinned_version": null
//     }
//   ],
//   "casks": []
// }

#[derive(Clone, Deserialize, Serialize, Debug)]
pub struct Formulae {
    pub name: String,
    pub installed_versions: Vec<String>,
    pub current_version: String,

    #[serde(default)]
    pub pinned: bool,

    #[serde(default)]
    pub pinned_version: Option<String>,
}

impl Default for Formulae {
    fn default() -> Self {
        Self::new()
    }
}
impl Formulae {
    pub fn new() -> Formulae {
        Formulae {
            name: "".to_string(),
            installed_versions: Vec::new(),
            current_version: "".to_string(),
            pinned: false,
            pinned_version: None,
        }
    }
}
#[derive(Clone, Deserialize, Serialize, Debug)]
pub struct Outdated {
    pub formulae: Vec<Formulae>,
    pub casks: Vec<Formulae>,
}

impl Default for Outdated {
    fn default() -> Self {
        Self::new()
    }
}

impl Outdated {
    pub fn new() -> Outdated {
        Outdated {
            formulae: Vec::new(),
            casks: Vec::new(),
        }
    }
}

// Implementation of brew
pub struct Brew;

impl Plugin for Brew {
    fn name(&self) -> &str {
        "brew"
    }

    fn description(&self) -> &str {
        "Update and upgrade formulae and casks via brew."
    }

    fn version(&self) -> &str {
        // TODO: implement versioning
        "0.1.0"
    }

    /// Uupd conflicts with all other plugins
    fn conflicts(&self, plugin_name: &str) -> bool {
        plugin_name == "uupd"
    }

    /// Run uupd
    extern "Rust" fn update(&self, tx: flume::Sender<PluginProgress>) -> bool {
        let mut pgrss = PluginProgress::new(self.name());

        // run a `brew update`
        pgrss.status = "Updating brew...".to_string();
        pgrss.progress = 5;
        let _ = tx.send(pgrss.clone());

        let (mut stdout, mut stderr, mut success) = update();
        if success != 0 {
            pgrss.status = "Failed to update brew".to_string();
            pgrss.stderr = Some(stderr);
            let _ = tx.send(pgrss.clone());
            return false;
        }

        // Get a list of outdated packages
        pgrss.status = "Getting outdated packages...".to_string();
        pgrss.stdout = Some(stdout.clone());
        pgrss.progress = 10;
        let _ = tx.send(pgrss.clone());

        (stdout, stderr, success) = get_outdated();
        if success != 0 {
            pgrss.status = "Failed to get outdated brew".to_string();
            pgrss.stderr = Some(stderr);
            let _ = tx.send(pgrss.clone());
            return false;
        }
        let outdated: Outdated = serde_json::from_str(&stdout).unwrap();

        // calculate the total progress based on the number of outdated formulae and casks
        let total_progress = outdated.formulae.len() + outdated.casks.len();

        println!("total_progress: {}", total_progress);
        if total_progress > 0 {
            // Figure out how much each step should progress, minus the first two hard-coded steps
            let step_progress = 100 / total_progress;

            // Upgrade each formulae
            for formulae in outdated.formulae {
                pgrss.status = format!("Upgrading formulae {}...", formulae.name);
                pgrss.stdout = None;
                pgrss.stderr = None;
                let _ = tx.send(pgrss.clone());

                (stdout, stderr, success) = upgrade_formulae(&formulae.name);
                if success != 0 {
                    pgrss.status = format!("Failed to upgrade formulae {}", formulae.name);
                    pgrss.stderr = Some(stderr.clone());
                    let _ = tx.send(pgrss.clone());
                    // Continue updating
                }

                pgrss.progress += step_progress as u32;
                pgrss.stdout = Some(stdout.clone());
                if !stderr.is_empty() {
                    pgrss.stderr = Some(stderr.clone());
                }
                let _ = tx.send(pgrss.clone());
            }

            // Upgrade each cask
            for cask in outdated.casks {
                pgrss.status = format!("Upgrading cask {}...", cask.name);
                pgrss.stdout = None;
                pgrss.stderr = None;
                let _ = tx.send(pgrss.clone());

                (stdout, stderr, success) = upgrade_cask(&cask.name);
                if success != 0 {
                    pgrss.status = format!("Failed to upgrade cask {}", cask.name);
                    pgrss.stderr = Some(stderr.clone());
                    let _ = tx.send(pgrss.clone());
                    // Continue updating
                }

                pgrss.stdout = Some(stdout.clone());
                if !stderr.is_empty() {
                    pgrss.stderr = Some(stderr.clone());
                }
                pgrss.progress += step_progress as u32;
                let _ = tx.send(pgrss.clone());
            }
        }

        // Done!
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
    Box::into_raw(Box::new(Brew))
}

fn get_outdated() -> (String, String, i32) {
    let cmd = "brew outdated --json";
    execute(cmd)
}

fn update() -> (String, String, i32) {
    // run a `brew update`
    execute("brew update")
}

fn upgrade_formulae(formula: &str) -> (String, String, i32) {
    // run a `brew upgrade <formula> --dry-run`
    execute(&format!("brew upgrade {}", formula))
}

fn upgrade_cask(cask: &str) -> (String, String, i32) {
    // run a `brew upgrade --cask <formula> --dry-run`
    execute(&format!("brew upgrade --cask {}", cask))
}
