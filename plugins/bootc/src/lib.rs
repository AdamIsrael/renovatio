use renovatio::{Plugin, PluginProgress, execute_args};

/// `rpm-ostree status --pending-exit-77` exits with this when a deployment is staged.
const PENDING_DEPLOYMENT: i32 = 77;

// Implementation of bootc
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

    /// uupd and rpm-ostree also update the OS.
    fn conflicts(&self, plugin_name: &str) -> bool {
        matches!(plugin_name, "uupd" | "rpm-ostree")
    }

    /// Run `bootc upgrade`, then check whether a reboot is needed to apply it.
    extern "Rust" fn update(&self, tx: flume::Sender<PluginProgress>) -> bool {
        let mut pgrss = PluginProgress::new(self.name());
        pgrss.pulse = true;
        pgrss.status = "Checking for and downloading OS updates...".to_string();
        let _ = tx.send(pgrss.clone());

        // bootc needs root for both `status` and `upgrade`, so only run the
        // upgrade with pkexec (one password prompt). Whether a deployment is
        // staged can be checked without root via rpm-ostree.
        let upgrade = execute_args("pkexec", &["bootc", "upgrade"]);
        let pending = (upgrade.2 == 0)
            .then(|| execute_args("rpm-ostree", &["status", "--pending-exit-77"]).2);

        let outcome = outcome(&upgrade.0, &upgrade.1, upgrade.2, pending);

        pgrss.pulse = false;
        pgrss.status = outcome.status;
        pgrss.reboot_required = outcome.reboot_required;
        pgrss.stdout = (!upgrade.0.trim().is_empty()).then_some(upgrade.0);
        pgrss.stderr = (!upgrade.1.trim().is_empty()).then_some(upgrade.1);
        if outcome.success {
            pgrss.progress = 100;
        }
        let _ = tx.send(pgrss);

        outcome.success
    }
}

// Export a function to create an instance of the plugin
renovatio::export_plugin!(Bootc);

#[derive(Debug, PartialEq)]
struct Outcome {
    success: bool,
    reboot_required: bool,
    status: String,
}

/// Decide the result of an update from `bootc upgrade`'s output and exit code
/// and, if the upgrade succeeded, the exit code of `rpm-ostree status --pending-exit-77`.
fn outcome(stdout: &str, stderr: &str, upgrade_rc: i32, pending_rc: Option<i32>) -> Outcome {
    if upgrade_rc != 0 {
        let status = match upgrade_rc {
            // pkexec: the authorization dialog was dismissed, or authorization failed
            126 | 127 => "OS update cancelled: authorization was not granted".to_string(),
            _ => match last_line(stderr) {
                Some(error) => format!("OS update failed: {error}"),
                None => format!("OS update failed (exit code {upgrade_rc})"),
            },
        };
        return Outcome {
            success: false,
            reboot_required: false,
            status,
        };
    }

    // Prefer rpm-ostree's answer; if it isn't usable, fall back to bootc's
    // "No changes in ..." message for "nothing new was staged".
    let staged = match pending_rc {
        Some(PENDING_DEPLOYMENT) => true,
        Some(0) => false,
        _ => !stdout.contains("No changes in"),
    };

    Outcome {
        success: true,
        reboot_required: staged,
        status: if staged {
            "OS update staged; reboot to apply it".to_string()
        } else {
            "OS is up to date".to_string()
        },
    }
}

fn last_line(text: &str) -> Option<&str> {
    text.lines().map(str::trim).rfind(|line| !line.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn staged_update_requires_reboot() {
        let outcome = outcome("Queued for next boot: ...\n", "", 0, Some(77));
        assert!(outcome.success);
        assert!(outcome.reboot_required);
        assert_eq!(outcome.status, "OS update staged; reboot to apply it");
    }

    #[test]
    fn no_pending_deployment_is_up_to_date() {
        let outcome = outcome("No changes in: ghcr.io/example/os:latest\n", "", 0, Some(0));
        assert!(outcome.success);
        assert!(!outcome.reboot_required);
        assert_eq!(outcome.status, "OS is up to date");
    }

    #[test]
    fn falls_back_to_bootc_output_without_rpm_ostree() {
        // e.g. rpm-ostree not installed: sh reports 127
        assert!(!outcome("No changes in: img\n", "", 0, Some(127)).reboot_required);
        assert!(outcome("Queued for next boot: img\n", "", 0, Some(127)).reboot_required);
    }

    #[test]
    fn failed_upgrade_is_a_failure_not_a_pending_reboot() {
        let outcome = outcome(
            "",
            "Fetching image\nerror: Upgrading: Pulling: connection refused\n",
            1,
            None,
        );
        assert!(!outcome.success);
        assert!(!outcome.reboot_required);
        assert_eq!(
            outcome.status,
            "OS update failed: error: Upgrading: Pulling: connection refused"
        );
    }

    #[test]
    fn cancelled_authorization_is_reported() {
        for rc in [126, 127] {
            let outcome = outcome("", "", rc, None);
            assert!(!outcome.success);
            assert_eq!(
                outcome.status,
                "OS update cancelled: authorization was not granted"
            );
        }
    }

    #[test]
    fn failure_without_stderr_reports_exit_code() {
        assert_eq!(
            outcome("", "\n", 2, None).status,
            "OS update failed (exit code 2)"
        );
    }
}
