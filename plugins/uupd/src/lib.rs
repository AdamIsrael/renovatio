use renovatio::{Plugin, PluginProgress};

use serde::{Deserialize, Serialize};

use std::io::{BufRead, BufReader, Read};
use std::os::unix::process::ExitStatusExt;
use std::process::{Command, Stdio};
use std::thread;

#[derive(Clone, Deserialize, Serialize, Debug)]
pub struct UupdProgress {
    #[serde(default)]
    pub level: String,

    #[serde(default)]
    pub msg: String,

    #[serde(default)]
    pub title: String,

    #[serde(default)]
    pub description: String,

    #[serde(default)]
    pub previous_overall: u32,

    #[serde(default)]
    pub progress: u32,

    #[serde(default)]
    pub total: u32,

    #[serde(default)]
    pub step_progress: f32,

    #[serde(default)]
    pub overall: u32,
}

// Implementation of uupd
pub struct Uupd;

impl Plugin for Uupd {
    fn name(&self) -> &str {
        "uupd"
    }

    fn description(&self) -> &str {
        "uupd updates bootc, rpm-ostree, flatpak, brew, and distrobox."
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
        pgrss.status = "Starting uupd...".to_string();
        let _ = tx.send(pgrss.clone());

        // uupd reports its progress as one JSON object per line on stdout.
        let mut child = match Command::new("pkexec")
            .args(["uupd", "--json"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(child) => child,
            Err(error) => {
                pgrss.pulse = false;
                pgrss.status = format!("Failed to run uupd: {error}");
                let _ = tx.send(pgrss);
                return false;
            }
        };

        // Both are piped above, so they're always present.
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();

        // Read stderr on its own thread so neither pipe can fill up and stall uupd.
        let (last_error, stderr) = thread::scope(|scope| {
            let stderr_reader = scope.spawn(|| {
                let mut text = String::new();
                let _ = BufReader::new(stderr).read_to_string(&mut text);
                text
            });

            let mut previous_overall = 0;
            let mut last_error = None;
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                println!("Received data: {}", line);

                // Skip anything that isn't a uupd JSON message, e.g. pkexec output.
                let Ok(p) = serde_json::from_str::<UupdProgress>(&line) else {
                    continue;
                };
                if p.level.eq_ignore_ascii_case("error") && !p.msg.is_empty() {
                    last_error = Some(p.msg.clone());
                }

                let (progress, status) = convert(&p, previous_overall);
                previous_overall = p.overall;

                let mut pgrss = PluginProgress::new(self.name());
                pgrss.progress = progress;
                pgrss.status = status;
                let _ = tx.send(pgrss);
            }

            (last_error, stderr_reader.join().unwrap_or_default())
        });

        let rc = match child.wait() {
            Ok(status) => status
                .code()
                .or_else(|| status.signal().map(|signal| 128 + signal))
                .unwrap_or(1),
            Err(_) => 1,
        };
        let (success, status) = outcome(rc, last_error.as_deref(), &stderr);

        pgrss.pulse = false;
        pgrss.status = status;
        pgrss.stderr = (!stderr.trim().is_empty()).then_some(stderr);
        if success {
            pgrss.progress = 100;
        }
        let _ = tx.send(pgrss);

        success
    }
}

/// Convert one uupd message into a progress percentage and status line.
///
/// uupd only sometimes reports a detailed `step_progress` (e.g. while
/// downloading the image); otherwise use the overall progress of the
/// previous message, since `overall` is reported before a step runs.
fn convert(p: &UupdProgress, previous_overall: u32) -> (u32, String) {
    let progress = if p.step_progress > 0.0 {
        p.step_progress as u32
    } else {
        previous_overall
    };

    let step = if (p.progress + 1) < p.total {
        p.progress + 1
    } else {
        p.progress
    };

    let status = if p.progress == 100 || (step == 0 && p.total == 0) {
        "Finishing update...".to_string()
    } else {
        format!(
            "{} {} - {} (step {}/{})...",
            p.msg,
            p.title,
            p.description,
            step,
            p.total + 1
        )
    };

    (progress.min(100), status)
}

/// Decide the result of a run from uupd's exit code, the last `error`-level
/// JSON message, and its stderr.
fn outcome(rc: i32, last_error: Option<&str>, stderr: &str) -> (bool, String) {
    if rc == 0 {
        return (true, "Update complete.".to_string());
    }

    let reason = match rc {
        // pkexec: the authorization dialog was dismissed, or authorization failed
        126 | 127 => {
            return (
                false,
                "uupd cancelled: authorization was not granted".to_string(),
            );
        }
        _ => last_error.map(str::to_string).or_else(|| {
            stderr
                .lines()
                .map(str::trim)
                .rfind(|line| !line.is_empty())
                .map(str::to_string)
        }),
    };

    match reason {
        Some(reason) => (false, format!("uupd failed: {reason}")),
        None => (false, format!("uupd failed (exit code {rc})")),
    }
}

// Export a function to create an instance of the plugin
renovatio::export_plugin!(Uupd);

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> UupdProgress {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn non_json_lines_do_not_parse() {
        assert!(
            serde_json::from_str::<UupdProgress>("Error executing command as another user")
                .is_err()
        );
    }

    #[test]
    fn partial_json_lines_parse_with_defaults() {
        let p = parse(r#"{"overall": 40}"#);
        assert_eq!(p.overall, 40);
        assert_eq!(p.msg, "");
    }

    #[test]
    fn convert_prefers_step_progress() {
        let p = parse(
            r#"{"level":"INFO","msg":"Updating","title":"System","description":"Pulling image","progress":0,"total":4,"step_progress":42.5,"overall":10}"#,
        );
        let (progress, status) = convert(&p, 5);
        assert_eq!(progress, 42);
        assert_eq!(status, "Updating System - Pulling image (step 1/5)...");
    }

    #[test]
    fn convert_falls_back_to_previous_overall() {
        let p =
            parse(r#"{"msg":"Updating","title":"Flatpak","progress":2,"total":4,"overall":60}"#);
        assert_eq!(convert(&p, 40).0, 40);
    }

    #[test]
    fn convert_clamps_progress() {
        let p = parse(r#"{"step_progress":250.0}"#);
        assert_eq!(convert(&p, 0).0, 100);
    }

    #[test]
    fn success_is_reported() {
        assert_eq!(outcome(0, None, ""), (true, "Update complete.".to_string()));
    }

    #[test]
    fn failure_prefers_uupd_error_message() {
        let (success, status) = outcome(1, Some("Failed to update flatpaks"), "warning: x\n");
        assert!(!success);
        assert_eq!(status, "uupd failed: Failed to update flatpaks");
    }

    #[test]
    fn failure_falls_back_to_stderr_then_exit_code() {
        assert_eq!(
            outcome(1, None, "first\nconnection refused\n").1,
            "uupd failed: connection refused"
        );
        assert_eq!(outcome(3, None, "").1, "uupd failed (exit code 3)");
    }

    /// Put a fake `pkexec` in `dir` that ignores its arguments, prints
    /// `stdout` and `stderr`, and exits with `code`.
    fn fake_pkexec(dir: &std::path::Path, stdout: &str, stderr: &str, code: i32) {
        use std::os::unix::fs::PermissionsExt;

        let path = dir.join("pkexec");
        let script =
            format!("#!/bin/sh\nprintf '%s' '{stdout}'\nprintf '%s' '{stderr}' >&2\nexit {code}\n");
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn run_update() -> (bool, Vec<PluginProgress>) {
        let (tx, rx) = flume::unbounded();
        let success = Uupd.update(tx);
        (success, rx.iter().collect())
    }

    // A single test, because it changes PATH for the whole process.
    #[test]
    fn update_runs_uupd_and_reports_result() {
        let dir = std::env::temp_dir().join(format!("renovatio-uupd-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // SAFETY: no other test in this crate reads or writes the environment.
        unsafe { std::env::set_var("PATH", &dir) };

        // Success, with a non-JSON line mixed in (previously a panic).
        fake_pkexec(
            &dir,
            "polkit says hello\n{\"level\":\"INFO\",\"msg\":\"Updating\",\"title\":\"System\",\"progress\":0,\"total\":2,\"overall\":0}\n{\"level\":\"INFO\",\"msg\":\"Updating\",\"title\":\"Flatpak\",\"progress\":1,\"total\":2,\"overall\":50}\n",
            "",
            0,
        );
        let (success, messages) = run_update();
        assert!(success);
        assert!(
            messages
                .iter()
                .any(|m| m.status.starts_with("Updating System"))
        );
        let last = messages.last().unwrap();
        assert_eq!(last.progress, 100);
        assert_eq!(last.status, "Update complete.");

        // Failure: uupd's error-level message and exit code are reported.
        fake_pkexec(
            &dir,
            "{\"level\":\"ERROR\",\"msg\":\"Failed to update brew\"}\n",
            "",
            1,
        );
        let (success, messages) = run_update();
        assert!(!success);
        let last = messages.last().unwrap();
        assert_ne!(last.progress, 100);
        assert_eq!(last.status, "uupd failed: Failed to update brew");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn cancelled_authorization_is_reported() {
        for rc in [126, 127] {
            assert_eq!(
                outcome(rc, None, ""),
                (
                    false,
                    "uupd cancelled: authorization was not granted".to_string()
                )
            );
        }
    }
}
