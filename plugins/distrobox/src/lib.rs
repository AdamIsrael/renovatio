use renovatio::{Plugin, PluginProgress, execute};

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

        // List the distroboxes
        let distroboxes = list();

        // calculate the total progress based on the number of outdated formulae and casks
        let total_progress = distroboxes.len();

        // Figure out how much each step should progress
        let mut step_progress = 100;
        if total_progress > 0 {
            step_progress = 100 / total_progress;
        }

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
#[unsafe(no_mangle)]
pub fn create_plugin() -> *mut dyn Plugin {
    Box::into_raw(Box::new(Distrobox))
}

pub fn list() -> Vec<String> {
    let mut boxes = Vec::new();
    let (stdout, _stderr, success) = execute("distrobox list --no-color");
    if success != 0 {
        return boxes;
    }

    for line in stdout.lines().skip(1) {
        let cols = line
            .split('|')
            .map(|s| s.trim().to_string())
            .collect::<Vec<String>>();
        boxes.push(cols[1].to_string());
    }
    boxes
}

pub fn upgrade(name: &str) -> (String, String, i32) {
    execute(format!("distrobox upgrade {}", name).as_str())
}
