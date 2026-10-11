use renovatio::{Plugin, PluginProgress};

use std::io::{BufRead, BufReader, Read};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;

// Implementation of rustup
pub struct Rustup;

impl Plugin for Rustup {
    fn name(&self) -> &str {
        "rustup"
    }

    fn description(&self) -> &str {
        "Update Rust toolchains via rustup."
    }

    fn version(&self) -> &str {
        env!("CARGO_PKG_VERSION")
    }

    /// rustup manages its own toolchains, which no other plugin touches.
    fn conflicts(&self, _plugin_name: &str) -> bool {
        false
    }

    /// Run `rustup update`
    extern "Rust" fn update(&self, tx: flume::Sender<PluginProgress>) -> bool {
        let mut pgrss = PluginProgress::new(self.name());
        pgrss.pulse = true;
        pgrss.status = "Updating Rust toolchains...".to_string();
        let _ = tx.send(pgrss.clone());

        let Some(rustup) = find_rustup() else {
            pgrss.pulse = false;
            pgrss.status =
                "rustup not found. Install it from https://rustup.rs or disable this plugin."
                    .to_string();
            let _ = tx.send(pgrss);
            return false;
        };

        let mut child = match Command::new(&rustup)
            .arg("update")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(child) => child,
            Err(error) => {
                pgrss.pulse = false;
                pgrss.status = format!("Failed to run {}: {error}", rustup.display());
                let _ = tx.send(pgrss);
                return false;
            }
        };

        // Both are piped above, so they're always present.
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();

        // rustup reports progress as `info:` lines on stderr and prints a
        // per-toolchain summary on stdout. Read both concurrently so neither
        // pipe can fill up and stall rustup.
        let (summary, messages) = thread::scope(|scope| {
            let progress = scope.spawn(|| {
                let mut messages = Vec::new();
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    if let Some(info) = line.strip_prefix("info: ") {
                        let mut progress = PluginProgress::new(self.name());
                        progress.pulse = true;
                        progress.status = format!("rustup: {info}");
                        let _ = tx.send(progress);
                    } else if !line.trim().is_empty() {
                        messages.push(line);
                    }
                }
                messages
            });

            let mut summary = String::new();
            let _ = BufReader::new(stdout).read_to_string(&mut summary);
            (summary, progress.join().unwrap_or_default())
        });

        let success = matches!(child.wait(), Ok(status) if status.success());

        pgrss.pulse = false;
        pgrss.stdout = (!summary.trim().is_empty()).then(|| summary.clone());
        pgrss.stderr = (!messages.is_empty()).then(|| messages.join("\n"));

        if success {
            pgrss.progress = 100;
            pgrss.status = summarize(&summary);
        } else {
            pgrss.status = match messages.iter().find_map(|m| m.strip_prefix("error: ")) {
                Some(error) => format!("rustup update failed: {error}"),
                None => "rustup update failed".to_string(),
            };
        }
        let _ = tx.send(pgrss);

        success
    }
}

// Export a function to create an instance of the plugin
renovatio::export_plugin!(Rustup);

/// Find the rustup binary. Apps started from the desktop often don't have
/// `~/.cargo/bin` on their `PATH`, so fall back to the default install location.
fn find_rustup() -> Option<PathBuf> {
    let on_path = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .map(|dir| dir.join("rustup"));

    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cargo")))
        .map(|cargo_home| cargo_home.join("bin").join("rustup"));

    on_path.chain(cargo_home).find(|path| path.is_file())
}

/// Turn rustup's per-toolchain summary into a one-line status, e.g.
/// `  stable-x86_64-unknown-linux-gnu updated - rustc 1.99.0 (...) (from rustc 1.98.0 (...))`
/// `  nightly-x86_64-unknown-linux-gnu unchanged - rustc 1.101.0-nightly (...)`
fn summarize(summary: &str) -> String {
    let mut updated = Vec::new();
    let mut failed = Vec::new();
    let mut unchanged = 0;

    for line in summary.lines() {
        let Some((toolchain_state, _version)) = line.trim().split_once(" - ") else {
            continue;
        };
        let Some((toolchain, state)) = toolchain_state.split_once(' ') else {
            continue;
        };
        let toolchain = short_name(toolchain);
        match state.trim() {
            "updated" | "installed" => updated.push(toolchain),
            "unchanged" => unchanged += 1,
            state if state.contains("fail") => failed.push(toolchain),
            _ => {}
        }
    }

    let mut parts = Vec::new();
    if !updated.is_empty() {
        parts.push(format!("Updated {}", updated.join(", ")));
    }
    if !failed.is_empty() {
        parts.push(format!("failed: {}", failed.join(", ")));
    }

    match (parts.is_empty(), unchanged) {
        (true, 0) => "Rust toolchains updated.".to_string(),
        (true, 1) => "Rust toolchain is up to date.".to_string(),
        (true, n) => format!("All {n} Rust toolchains are up to date."),
        (false, 0) => format!("{}.", parts.join("; ")),
        (false, n) => format!("{}; {n} unchanged.", parts.join("; ")),
    }
}

/// Drop the host triple from a toolchain name, e.g.
/// `stable-x86_64-unknown-linux-gnu` -> `stable`,
/// `nightly-2025-01-01-x86_64-unknown-linux-gnu` -> `nightly-2025-01-01`.
fn short_name(toolchain: &str) -> String {
    const ARCHES: &[&str] = &[
        "x86_64",
        "aarch64",
        "i686",
        "arm",
        "armv7",
        "riscv64gc",
        "powerpc64le",
        "s390x",
        "loongarch64",
    ];

    let segments: Vec<&str> = toolchain.split('-').collect();
    match segments.iter().position(|s| ARCHES.contains(s)) {
        Some(arch) if arch > 0 => segments[..arch].join("-"),
        _ => toolchain.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarize_reports_updated_and_unchanged() {
        let summary = "
  stable-x86_64-unknown-linux-gnu updated - rustc 1.99.0 (b940084d7 2026-09-28) (from rustc 1.98.0 (88d9e12ae 2026-08-18))
  nightly-x86_64-unknown-linux-gnu unchanged - rustc 1.101.0-nightly (6f5da406b 2026-10-10)
";
        assert_eq!(summarize(summary), "Updated stable; 1 unchanged.");
    }

    #[test]
    fn summarize_reports_all_up_to_date() {
        let summary = "
  stable-x86_64-unknown-linux-gnu unchanged - rustc 1.99.0 (b940084d7 2026-09-28)
  nightly-x86_64-unknown-linux-gnu unchanged - rustc 1.101.0-nightly (6f5da406b 2026-10-10)
";
        assert_eq!(summarize(summary), "All 2 Rust toolchains are up to date.");
    }

    #[test]
    fn summarize_reports_failures() {
        let summary = "
  stable-x86_64-unknown-linux-gnu updated - rustc 1.99.0 (b940084d7 2026-09-28) (from rustc 1.98.0 (88d9e12ae 2026-08-18))
  nightly-x86_64-unknown-linux-gnu update failed - rustc 1.100.0-nightly (e457a7b0d 2026-08-27)
";
        assert_eq!(summarize(summary), "Updated stable; failed: nightly.");
    }

    #[test]
    fn summarize_handles_unrecognized_output() {
        assert_eq!(summarize(""), "Rust toolchains updated.");
        assert_eq!(
            summarize("something else entirely\n"),
            "Rust toolchains updated."
        );
    }

    /// Write an executable fake `rustup` into `dir` that prints `stderr` and
    /// `stdout` and exits with `code`.
    fn fake_rustup(dir: &std::path::Path, stderr: &str, stdout: &str, code: i32) {
        use std::os::unix::fs::PermissionsExt;

        let path = dir.join("rustup");
        let script =
            format!("#!/bin/sh\nprintf '%s' '{stderr}' >&2\nprintf '%s' '{stdout}'\nexit {code}\n");
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn run_update() -> (bool, Vec<PluginProgress>) {
        let (tx, rx) = flume::unbounded();
        let success = Rustup.update(tx);
        (success, rx.iter().collect())
    }

    // A single test, because it changes PATH for the whole process.
    #[test]
    fn update_runs_rustup_and_reports_result() {
        let dir = std::env::temp_dir().join(format!("renovatio-rustup-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // SAFETY: no other test in this crate reads or writes the environment.
        unsafe { std::env::set_var("PATH", &dir) };

        fake_rustup(
            &dir,
            "info: syncing channel updates for stable-x86_64-unknown-linux-gnu\n",
            "  stable-x86_64-unknown-linux-gnu updated - rustc 1.99.0 (from rustc 1.98.0)\n",
            0,
        );
        let (success, messages) = run_update();
        assert!(success);
        assert!(
            messages.iter().any(|m| m.status
                == "rustup: syncing channel updates for stable-x86_64-unknown-linux-gnu")
        );
        let last = messages.last().unwrap();
        assert_eq!(last.progress, 100);
        assert_eq!(last.status, "Updated stable.");

        fake_rustup(&dir, "error: could not download file\n", "", 1);
        let (success, messages) = run_update();
        assert!(!success);
        let last = messages.last().unwrap();
        assert_ne!(last.progress, 100);
        assert_eq!(last.status, "rustup update failed: could not download file");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn short_name_drops_host_triple() {
        assert_eq!(short_name("stable-x86_64-unknown-linux-gnu"), "stable");
        assert_eq!(
            short_name("nightly-2025-01-01-x86_64-unknown-linux-gnu"),
            "nightly-2025-01-01"
        );
        assert_eq!(short_name("1.80.0-aarch64-apple-darwin"), "1.80.0");
        assert_eq!(short_name("my-custom-toolchain"), "my-custom-toolchain");
    }
}
