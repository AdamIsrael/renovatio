use renovatio::{Plugin, PluginProgress, execute_args};

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

    #[serde(default)]
    pub installed_versions: Vec<String>,

    #[serde(default)]
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
        env!("CARGO_PKG_VERSION")
    }

    /// uupd also updates brew.
    fn conflicts(&self, plugin_name: &str) -> bool {
        plugin_name == "uupd"
    }

    /// Update brew, then upgrade outdated formulae and casks
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
        let outdated = match parse_outdated(&stdout) {
            Ok(outdated) => outdated,
            Err(error) => {
                pgrss.status = "Failed to read outdated brew packages".to_string();
                pgrss.stderr = Some(error);
                let _ = tx.send(pgrss.clone());
                return false;
            }
        };

        // calculate the total progress based on the number of outdated formulae and casks
        let total_progress = outdated.formulae.len() + outdated.casks.len();

        println!("total_progress: {}", total_progress);
        // Figure out how much each step should progress, minus the first two hard-coded steps
        if let Some(step_progress) = 100usize.checked_div(total_progress) {
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
renovatio::export_plugin!(Brew);

/// Parse `brew outdated --json` (the v2 format: `formulae` and `casks`).
fn parse_outdated(stdout: &str) -> Result<Outdated, String> {
    serde_json::from_str(stdout)
        .map_err(|error| format!("Unexpected brew outdated output: {error}"))
}

fn get_outdated() -> (String, String, i32) {
    execute_args("brew", &["outdated", "--json"])
}

fn update() -> (String, String, i32) {
    // run a `brew update`
    execute_args("brew", &["update"])
}

fn upgrade_formulae(formula: &str) -> (String, String, i32) {
    // run a `brew upgrade <formula> --dry-run`
    execute_args("brew", &["upgrade", formula])
}

fn upgrade_cask(cask: &str) -> (String, String, i32) {
    // run a `brew upgrade --cask <formula> --dry-run`
    execute_args("brew", &["upgrade", "--cask", cask])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_outdated_reads_formulae_and_casks() {
        let stdout = r#"{
          "formulae": [
            {
              "name": "k9s",
              "installed_versions": ["0.50.15"],
              "current_version": "0.50.16",
              "pinned": false,
              "pinned_version": null
            }
          ],
          "casks": [
            {
              "name": "font-fira-code",
              "installed_versions": ["6.2"],
              "current_version": "6.3"
            }
          ]
        }"#;
        let outdated = parse_outdated(stdout).unwrap();

        assert_eq!(outdated.formulae.len(), 1);
        assert_eq!(outdated.formulae[0].name, "k9s");
        assert_eq!(outdated.formulae[0].installed_versions, vec!["0.50.15"]);
        assert_eq!(outdated.formulae[0].current_version, "0.50.16");

        // Casks have no pinned fields.
        assert_eq!(outdated.casks.len(), 1);
        assert_eq!(outdated.casks[0].name, "font-fira-code");
        assert!(!outdated.casks[0].pinned);
    }

    #[test]
    fn parse_outdated_with_nothing_outdated() {
        // Captured from `brew outdated --json` with everything up to date.
        let outdated = parse_outdated("{\n  \"formulae\": [],\n  \"casks\": []\n}\n").unwrap();
        assert!(outdated.formulae.is_empty() && outdated.casks.is_empty());
    }

    #[test]
    fn parse_outdated_rejects_unexpected_output() {
        // e.g. the v1 format (a bare array), or a warning instead of JSON
        assert!(parse_outdated("[]").is_err());
        assert!(parse_outdated("Warning: something went wrong").is_err());
    }
}
