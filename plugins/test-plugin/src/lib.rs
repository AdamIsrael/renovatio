//! Development-only plugin that misbehaves on demand, to exercise the UI's
//! failure paths without breaking a real update.
//!
//! The mode is read from `RENOVATIO_TEST_PLUGIN` at the start of every run:
//!
//! - `fail` (default): send some progress, then return false without reaching 100
//! - `panic`: panic mid-update (`PanicGuard` should turn it into a failure)
//! - `slow`: pulse for a few seconds, then succeed
//! - `succeed`: send 100 twice, then succeed (the old double-count case)
//!
//! Install with `just install-test-plugin`; `just install-release` skips it.
use renovatio::{Plugin, PluginProgress};

use std::thread;
use std::time::Duration;

const MODE_VAR: &str = "RENOVATIO_TEST_PLUGIN";

#[derive(Debug, PartialEq)]
enum Mode {
    Fail,
    Panic,
    Slow,
    Succeed,
}

impl Mode {
    fn parse(value: Option<&str>) -> Result<Mode, String> {
        match value.map(str::trim) {
            None | Some("") | Some("fail") => Ok(Mode::Fail),
            Some("panic") => Ok(Mode::Panic),
            Some("slow") => Ok(Mode::Slow),
            Some("succeed") => Ok(Mode::Succeed),
            Some(other) => Err(format!(
                "Unknown {MODE_VAR} mode '{other}' (expected fail, panic, slow or succeed)"
            )),
        }
    }
}

// Implementation of the test plugin
pub struct TestPlugin {
    /// How long each simulated step takes. `slow` pulses for five steps.
    step: Duration,
}

impl Plugin for TestPlugin {
    fn name(&self) -> &str {
        "test-plugin"
    }

    fn description(&self) -> &str {
        "Development only: fails, panics or stalls on demand (RENOVATIO_TEST_PLUGIN=fail|panic|slow|succeed)."
    }

    fn version(&self) -> &str {
        env!("CARGO_PKG_VERSION")
    }

    fn conflicts(&self, _plugin_name: &str) -> bool {
        false
    }

    extern "Rust" fn update(&self, tx: flume::Sender<PluginProgress>) -> bool {
        let mode = std::env::var(MODE_VAR).ok();
        self.run(Mode::parse(mode.as_deref()), &tx)
    }
}

impl TestPlugin {
    fn run(&self, mode: Result<Mode, String>, tx: &flume::Sender<PluginProgress>) -> bool {
        let send = |progress: u32, pulse: bool, status: &str| {
            let mut pgrss = PluginProgress::new(self.name());
            pgrss.progress = progress;
            pgrss.pulse = pulse;
            pgrss.status = status.to_string();
            let _ = tx.send(pgrss);
        };

        let mode = match mode {
            Ok(mode) => mode,
            Err(error) => {
                send(0, false, &error);
                return false;
            }
        };

        match mode {
            Mode::Fail => {
                send(30, false, "Pretending to update...");
                thread::sleep(self.step);

                let mut pgrss = PluginProgress::new(self.name());
                pgrss.progress = 30;
                pgrss.status = "Simulated failure (RENOVATIO_TEST_PLUGIN=fail)".to_string();
                pgrss.stderr = Some("test-plugin: simulated failure".to_string());
                let _ = tx.send(pgrss);
                false
            }
            Mode::Panic => {
                send(50, false, "About to panic...");
                thread::sleep(self.step);
                panic!("simulated panic (RENOVATIO_TEST_PLUGIN=panic)");
            }
            Mode::Slow => {
                send(0, true, "Pulsing for a while...");
                thread::sleep(self.step * 5);
                send(100, false, "Slow update complete.");
                true
            }
            Mode::Succeed => {
                send(100, false, "Already at 100%...");
                thread::sleep(self.step);
                send(100, false, "Done (sent 100% twice).");
                true
            }
        }
    }
}

// Export a function to create an instance of the plugin
renovatio::export_plugin!(TestPlugin {
    step: Duration::from_secs(1)
});

#[cfg(test)]
mod tests {
    use super::*;

    fn run(mode: Result<Mode, String>) -> (bool, Vec<PluginProgress>) {
        let plugin = TestPlugin {
            step: Duration::ZERO,
        };
        let (tx, rx) = flume::unbounded();
        let success = plugin.run(mode, &tx);
        drop(tx);
        (success, rx.iter().collect())
    }

    #[test]
    fn parse_defaults_to_fail() {
        assert_eq!(Mode::parse(None), Ok(Mode::Fail));
        assert_eq!(Mode::parse(Some("")), Ok(Mode::Fail));
        assert_eq!(Mode::parse(Some(" slow ")), Ok(Mode::Slow));
        assert!(
            Mode::parse(Some("explode"))
                .unwrap_err()
                .contains("'explode'")
        );
    }

    #[test]
    fn fail_never_reaches_100() {
        let (success, messages) = run(Ok(Mode::Fail));
        assert!(!success);
        assert!(messages.iter().all(|m| m.progress < 100));
        assert!(messages.last().unwrap().stderr.is_some());
    }

    #[test]
    fn panic_mode_is_turned_into_failure_by_panic_guard() {
        // SAFETY: this is the only test that touches the environment.
        unsafe { std::env::set_var(MODE_VAR, "panic") };
        let plugin = renovatio::PanicGuard(TestPlugin {
            step: Duration::ZERO,
        });
        let (tx, rx) = flume::unbounded();
        assert!(!plugin.update(tx));

        let last = rx.iter().last().unwrap();
        assert_eq!(
            last.status,
            "test-plugin crashed: simulated panic (RENOVATIO_TEST_PLUGIN=panic)"
        );
    }

    #[test]
    fn slow_pulses_then_succeeds() {
        let (success, messages) = run(Ok(Mode::Slow));
        assert!(success);
        assert!(messages.first().unwrap().pulse);
        assert_eq!(messages.last().unwrap().progress, 100);
    }

    #[test]
    fn succeed_sends_100_twice() {
        let (success, messages) = run(Ok(Mode::Succeed));
        assert!(success);
        assert_eq!(messages.iter().filter(|m| m.progress == 100).count(), 2);
    }

    #[test]
    fn unknown_mode_fails_with_message() {
        let (success, messages) = run(Mode::parse(Some("explode")));
        assert!(!success);
        assert!(
            messages[0]
                .status
                .contains("Unknown RENOVATIO_TEST_PLUGIN mode")
        );
    }
}
