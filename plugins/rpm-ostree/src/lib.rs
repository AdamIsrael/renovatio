use renovatio::{Plugin, PluginProgress, execute};

// Implementation of rpm-ostree
pub struct RpmOstree;

impl Plugin for RpmOstree {
    fn name(&self) -> &str {
        "rpm-ostree"
    }

    fn description(&self) -> &str {
        "Update the OS via rpm-ostree."
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
        pgrss.pulse = true;
        // TODO: need to figure out how to use the _pulse_ on the ProgressBar

        // Figure out how much each step should progress
        let total_progress = 2;
        let step_progress = 100 / total_progress;

        pgrss.status = "Checking for updates ...".to_string();
        pgrss.stdout = None;
        pgrss.stderr = None;
        let _ = tx.send(pgrss.clone());

        let (mut stdout, mut stderr, mut success) = download();
        // TODO: need to change execute to return the exit code. And then only fail
        // if the exit code indicates an error (like no network) rather than no update available.
        if success != 0 {
            pgrss.status = "Failed to check/download updates".to_string();
            pgrss.stderr = Some(stderr.clone());
            let _ = tx.send(pgrss.clone());
            // if we failed to download, we can't continue
            return false;
        }
        println!("{}", stdout);

        // We've downloaded the update, so let's install it.
        pgrss.status = "Installing OS update ...".to_string();
        pgrss.progress = step_progress as u32;
        pgrss.stdout = None;
        pgrss.stderr = None;
        let _ = tx.send(pgrss.clone());

        (stdout, stderr, success) = upgrade();
        if success != 0 {
            pgrss.status = "Failed to install OS update...".to_string();
            pgrss.stderr = Some(stderr.clone());
            let _ = tx.send(pgrss.clone());
            // if we failed to download, we can't continue
            return false;
        }
        println!("{}", stdout);

        // Done!
        pgrss.progress = 100;
        pgrss.stdout = None;
        pgrss.stderr = None;

        pgrss.status = "Upgrade complete!".to_string();
        let _ = tx.send(pgrss.clone());

        true
    }
}

// Export a function to create an instance of the plugin
#[unsafe(no_mangle)]
pub fn create_plugin() -> *mut dyn Plugin {
    Box::into_raw(Box::new(RpmOstree))
}

fn download() -> (String, String, i32) {
    // run `rpm-ostree upgrade --download-only`
    execute("rpm-ostree upgrade --download-only")
}

fn upgrade() -> (String, String, i32) {
    // run `rpm-ostree upgrade`
    execute("rpm-ostree upgrade")
}
