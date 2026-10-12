use renovatio::{Plugin, PluginProgress, execute_args};

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
        env!("CARGO_PKG_VERSION")
    }

    /// uupd also updates flatpaks.
    fn conflicts(&self, plugin_name: &str) -> bool {
        plugin_name == "uupd"
    }

    /// Upgrade user and system flatpaks
    extern "Rust" fn update(&self, tx: flume::Sender<PluginProgress>) -> bool {
        let mut pgrss = PluginProgress::new(self.name());

        // Find the system and user flatpaks needing update
        let (system, user) = match (list_updates(true), list_updates(false)) {
            (Ok(system), Ok(user)) => (system, user),
            (Err(error), _) | (_, Err(error)) => {
                pgrss.status = "Failed to list flatpak updates".to_string();
                pgrss.stderr = Some(error);
                let _ = tx.send(pgrss);
                return false;
            }
        };

        // calculate the total progress based on the number of outdated flatpaks
        let total_progress = system.len() + user.len();

        // Figure out how much each step should progress
        let step_progress = 100usize.checked_div(total_progress).unwrap_or(100);

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
renovatio::export_plugin!(Flatpak);

/// The applications with updates in the system or user installation.
fn list_updates(system: bool) -> Result<Vec<String>, String> {
    let flag = if system { "--system" } else { "--user" };
    let (stdout, stderr, rc) = execute_args(
        "flatpak",
        &["remote-ls", "--updates", "--columns=application", flag],
    );
    if rc != 0 {
        return Err(stderr);
    }
    Ok(parse_updates(&stdout))
}

/// Parse `flatpak remote-ls --updates --columns=application`: one application
/// ID per line. flatpak only prints its "Application ID" header to a terminal,
/// but skip it if it's there.
fn parse_updates(stdout: &str) -> Vec<String> {
    stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && *line != "Application ID")
        .map(str::to_string)
        .collect()
}

fn upgrade_flatpak(name: &str) -> (String, String, i32) {
    execute_args("flatpak", &["update", "--noninteractive", "-y", name])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_updates_keeps_every_line_of_piped_output() {
        // Captured from `flatpak remote-ls --updates --columns=application | cat`:
        // no header, so the first line is an update too.
        let stdout = "org.mozilla.firefox\ncom.github.tchx84.Flatseal\n";
        assert_eq!(
            parse_updates(stdout),
            vec!["org.mozilla.firefox", "com.github.tchx84.Flatseal"]
        );
    }

    #[test]
    fn parse_updates_skips_header_and_blank_lines() {
        let stdout = "Application ID\norg.gnome.Calculator\n\n  \n";
        assert_eq!(parse_updates(stdout), vec!["org.gnome.Calculator"]);
    }

    #[test]
    fn parse_updates_of_nothing_is_empty() {
        assert!(parse_updates("").is_empty());
    }
}
