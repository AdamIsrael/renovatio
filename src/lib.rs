use std::ffi::CStr;
use std::os::unix::process::ExitStatusExt;
use std::panic::{self, AssertUnwindSafe};
use std::process::Command;

/// Fingerprint of the plugin ABI this crate was built with (renovatio version,
/// rustc version, and a hash of this file). See `build.rs`.
///
/// Plugins export it via [`export_plugin!`] as the C-ABI symbol `renovatio_abi`,
/// which the host checks before calling `create_plugin`: calling into a plugin
/// built by a different compiler or against a different interface is undefined
/// behavior (in practice, a segfault).
pub const ABI_FINGERPRINT: &CStr =
    match CStr::from_bytes_with_nul(concat!(env!("RENOVATIO_ABI"), "\0").as_bytes()) {
        Ok(fingerprint) => fingerprint,
        Err(_) => panic!("RENOVATIO_ABI must not contain NUL bytes"),
    };

#[derive(Clone, Debug)]
/// PluginProgress is used to communicate the progress of a plugin update
pub struct PluginProgress {
    /// The name of the plugin this status belongs to
    pub name: String,

    /// The progress of the update, from 0 to 100
    pub progress: u32,

    /// In lieu of incremental progress, this will pulse the ProgressBar
    /// to indicate that progress is being made.
    pub pulse: bool,

    // Indicate if the update requires a reboot
    pub reboot_required: bool,

    /// The status of the update
    pub status: String,

    /// The standard output from the update, if available
    pub stdout: Option<String>,

    /// The standard error from the update, if available
    pub stderr: Option<String>,
}

impl PluginProgress {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            progress: 0,
            pulse: false,
            reboot_required: false,
            status: String::new(),
            stdout: None,
            stderr: None,
        }
    }
}

/// PluginMetadata is used to describe a plugin
#[derive(Clone, Debug)]
pub struct PluginMetadata {
    pub name: String,
    pub version: String,
    pub description: String,
    pub path: Option<String>,
}

impl PluginMetadata {
    /// Create a new plugin metadata
    pub fn new(plugin: &dyn Plugin) -> Self {
        Self {
            name: plugin.name().to_string(),
            version: plugin.version().to_string(),
            description: plugin.description().to_string(),
            path: None,
        }
    }
}

/// Plugin is a trait that defines the interface for a plugin.
///
/// The host loads each plugin once, at startup, and runs `update` on a worker
/// thread, so plugins must be `Send + Sync`.
pub trait Plugin: Send + Sync {
    /// The name of the plugin
    fn name(&self) -> &str;

    /// The description of the plugin
    fn description(&self) -> &str;

    /// The version of the plugin
    fn version(&self) -> &str;

    /// Determine if this plugin conflicts with another plugin.
    fn conflicts(&self, plugin_name: &str) -> bool;

    /// Run a blocking update
    ///
    /// # Arguments
    ///
    /// * `tx` - The sender channel to send progress updates to
    ///
    /// # Returns
    ///
    /// * `bool` - Whether the update was successful
    extern "Rust" fn update(&self, tx: flume::Sender<PluginProgress>) -> bool;
}

/// Wraps a plugin so that a panic in `update` is reported as a failed update.
///
/// Each plugin library links its own copy of std, so the host cannot catch a
/// plugin's panic (the process aborts with "Rust cannot catch foreign
/// exceptions"). This wrapper is monomorphized inside the plugin crate, so
/// its `catch_unwind` runs on the plugin's side of the boundary. Use it via
/// [`export_plugin!`].
pub struct PanicGuard<P: Plugin>(pub P);

impl<P: Plugin> Plugin for PanicGuard<P> {
    fn name(&self) -> &str {
        self.0.name()
    }

    fn description(&self) -> &str {
        self.0.description()
    }

    fn version(&self) -> &str {
        self.0.version()
    }

    fn conflicts(&self, plugin_name: &str) -> bool {
        self.0.conflicts(plugin_name)
    }

    extern "Rust" fn update(&self, tx: flume::Sender<PluginProgress>) -> bool {
        let guard_tx = tx.clone();
        match panic::catch_unwind(AssertUnwindSafe(|| self.0.update(tx))) {
            Ok(success) => success,
            Err(payload) => {
                let message = payload
                    .downcast_ref::<&str>()
                    .map(|s| s.to_string())
                    .or_else(|| payload.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "unknown panic".to_string());

                let mut progress = PluginProgress::new(self.name());
                progress.status = format!("{} crashed: {}", self.name(), message);
                progress.stderr = Some(message);
                let _ = guard_tx.send(progress);
                false
            }
        }
    }
}

/// Export `create_plugin` for a plugin, wrapped in a [`PanicGuard`], and the
/// `renovatio_abi` fingerprint the host checks before loading it.
///
/// ```ignore
/// renovatio::export_plugin!(Flatpak);
/// ```
#[macro_export]
macro_rules! export_plugin {
    ($plugin:expr) => {
        /// The ABI fingerprint this plugin was built with. Uses the C ABI so
        /// the host can call it safely even when the Rust ABI doesn't match.
        #[unsafe(no_mangle)]
        pub extern "C" fn renovatio_abi() -> *const ::std::ffi::c_char {
            $crate::ABI_FINGERPRINT.as_ptr()
        }

        #[unsafe(no_mangle)]
        pub fn create_plugin() -> *mut dyn $crate::Plugin {
            ::std::boxed::Box::into_raw(::std::boxed::Box::new($crate::PanicGuard($plugin)))
        }
    };
}

/// Run `command` with `sh -c` and return its stdout, stderr, and exit code.
///
/// Both streams are always returned, whatever the outcome. The exit code is 0
/// only on success: a process killed by a signal reports `128 + signal` (the
/// shell convention), and a command that can't be started reports 1 with the
/// reason in stderr.
///
/// Prefer [`execute_args`] whenever the command includes a value such as a
/// package or container name: the shell would interpret quotes, spaces and
/// `;` in it.
pub fn execute(command: &str) -> (String, String, i32) {
    run(Command::new("sh").args(["-c", command]), command)
}

/// Run `program` with `args` directly, without a shell, and return its
/// stdout, stderr, and exit code, with the same conventions as [`execute`].
/// Arguments are passed through as-is, so no quoting is needed.
pub fn execute_args(program: &str, args: &[&str]) -> (String, String, i32) {
    let display = std::iter::once(program)
        .chain(args.iter().copied())
        .collect::<Vec<_>>()
        .join(" ");
    run(Command::new(program).args(args), &display)
}

fn run(command: &mut Command, display: &str) -> (String, String, i32) {
    match command.output() {
        Ok(output) => {
            let rc = output
                .status
                .code()
                .or_else(|| output.status.signal().map(|signal| 128 + signal))
                .unwrap_or(1);
            (
                String::from_utf8_lossy(&output.stdout).into_owned(),
                String::from_utf8_lossy(&output.stderr).into_owned(),
                rc,
            )
        }
        Err(error) => {
            eprintln!("Error executing command: {}", error);
            (
                String::new(),
                format!("Failed to run {display}: {error}"),
                1,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Panics;

    impl Plugin for Panics {
        fn name(&self) -> &str {
            "panics"
        }
        fn description(&self) -> &str {
            ""
        }
        fn version(&self) -> &str {
            "0.0.0"
        }
        fn conflicts(&self, _plugin_name: &str) -> bool {
            false
        }
        extern "Rust" fn update(&self, _tx: flume::Sender<PluginProgress>) -> bool {
            panic!("boom");
        }
    }

    #[test]
    fn execute_returns_both_streams_on_success() {
        let (stdout, stderr, rc) = execute("echo out; echo err >&2");
        assert_eq!(
            (stdout.as_str(), stderr.as_str(), rc),
            ("out\n", "err\n", 0)
        );
    }

    #[test]
    fn execute_returns_both_streams_and_exit_code_on_failure() {
        let (stdout, stderr, rc) = execute("echo out; echo err >&2; exit 3");
        assert_eq!(
            (stdout.as_str(), stderr.as_str(), rc),
            ("out\n", "err\n", 3)
        );
    }

    #[test]
    fn execute_args_passes_arguments_without_a_shell() {
        // A shell would expand $HOME, split on the space and run `echo pwned`.
        let (stdout, _, rc) = execute_args("printf", &["%s|", "a b", "$HOME; echo pwned"]);
        assert_eq!((stdout.as_str(), rc), ("a b|$HOME; echo pwned|", 0));
    }

    #[test]
    fn execute_args_returns_both_streams_and_exit_code() {
        let (stdout, stderr, rc) = execute_args("sh", &["-c", "echo out; echo err >&2; exit 4"]);
        assert_eq!(
            (stdout.as_str(), stderr.as_str(), rc),
            ("out\n", "err\n", 4)
        );
    }

    #[test]
    fn execute_args_reports_missing_program() {
        let (_, stderr, rc) = execute_args("renovatio-no-such-program", &["--flag"]);
        assert_eq!(rc, 1);
        assert!(stderr.starts_with("Failed to run renovatio-no-such-program --flag: "));
    }

    #[test]
    fn execute_reports_signal_kill_as_failure() {
        let (_, _, rc) = execute("kill -KILL $$");
        assert_eq!(rc, 128 + 9);
    }

    #[test]
    fn panic_guard_reports_panic_as_failure() {
        let (tx, rx) = flume::unbounded();
        assert!(!PanicGuard(Panics).update(tx));

        let progress = rx.recv().unwrap();
        assert_eq!(progress.status, "panics crashed: boom");
        assert_eq!(progress.stderr.as_deref(), Some("boom"));
    }
}
