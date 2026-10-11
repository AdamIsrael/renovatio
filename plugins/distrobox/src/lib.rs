use renovatio::{Plugin, PluginProgress, execute_args};

// Implementation of distrobox
pub struct Distrobox;

impl Plugin for Distrobox {
    fn name(&self) -> &str {
        "distrobox"
    }

    fn description(&self) -> &str {
        "Update distrobox containers."
    }

    fn version(&self) -> &str {
        env!("CARGO_PKG_VERSION")
    }

    /// uupd also updates distroboxes.
    fn conflicts(&self, plugin_name: &str) -> bool {
        plugin_name == "uupd"
    }

    /// Upgrade each distrobox
    extern "Rust" fn update(&self, tx: flume::Sender<PluginProgress>) -> bool {
        let mut pgrss = PluginProgress::new(self.name());

        // List the distroboxes
        let distroboxes = list();

        // calculate the total progress based on the number of outdated formulae and casks
        let total_progress = distroboxes.len();

        // Figure out how much each step should progress
        let step_progress = 100usize.checked_div(total_progress).unwrap_or(100);

        for distrobox in distroboxes {
            pgrss.status = format!("Upgrading distrobox {}...", distrobox);
            pgrss.stdout = None;
            pgrss.stderr = None;
            let _ = tx.send(pgrss.clone());

            let (stdout, stderr, success) = upgrade(&distrobox);
            if success != 0 {
                pgrss.status = format!("Failed to upgrade distrobox {}", distrobox);
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
renovatio::export_plugin!(Distrobox);

pub fn list() -> Vec<String> {
    let (stdout, _stderr, success) = execute_args("distrobox", &["list", "--no-color"]);
    if success != 0 {
        return Vec::new();
    }
    parse_list(&stdout)
}

/// Parse the container names out of `distrobox list --no-color`:
///
/// ```text
/// ID           | NAME      | STATUS        | IMAGE
/// 1a2b3c4d5e6f | fedora    | Up 2 hours    | registry.fedoraproject.org/fedora-toolbox:44
/// ```
///
/// Skips the header and any line without a NAME column (blank lines, warnings).
fn parse_list(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| line.split('|').nth(1).map(str::trim))
        .filter(|name| !name.is_empty() && *name != "NAME")
        .map(str::to_string)
        .collect()
}

pub fn upgrade(name: &str) -> (String, String, i32) {
    execute_args("distrobox", &["upgrade", name])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_list_returns_container_names() {
        let output = "ID           | NAME      | STATUS        | IMAGE
1a2b3c4d5e6f | fedora    | Up 2 hours    | registry.fedoraproject.org/fedora-toolbox:44
6f5e4d3c2b1a | ubuntu    | Exited (0)    | quay.io/toolbx/ubuntu-toolbox:24.04
";
        assert_eq!(parse_list(output), vec!["fedora", "ubuntu"]);
    }

    #[test]
    fn parse_list_skips_lines_without_a_name_column() {
        // Each of these used to panic on cols[1].
        let output = "ID           | NAME      | STATUS        | IMAGE

Warning: something unexpected
1a2b3c4d5e6f | fedora    | Up 2 hours    | img
abc |  | x | y
";
        assert_eq!(parse_list(output), vec!["fedora"]);
    }

    #[test]
    fn parse_list_handles_no_containers() {
        assert!(parse_list("").is_empty());
        assert!(parse_list("ID | NAME | STATUS | IMAGE\n").is_empty());
    }
}
