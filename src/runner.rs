//! Runs the enabled plugins sequentially and reports their lifecycle to the UI.
//!
//! Completion is signalled explicitly by `RunEvent::Finished` once a plugin's
//! `update()` returns, rather than being inferred from `PluginProgress::progress`.
use std::ffi::{CStr, c_char};
use std::path::Path;
use std::thread;

use libloading::{Library, Symbol};
use renovatio::{Plugin, PluginProgress};

type PluginType = unsafe fn() -> *mut dyn Plugin;
type AbiType = unsafe extern "C" fn() -> *const c_char;

/// Events sent from the update worker thread to the UI.
#[derive(Debug)]
pub enum RunEvent {
    /// A plugin is about to run. `index` is 1-based.
    Started {
        index: usize,
        total: usize,
        name: String,
    },
    /// Progress reported by the running plugin.
    Progress(PluginProgress),
    /// A plugin has finished, failed, or could not be loaded.
    Finished {
        index: usize,
        total: usize,
        name: String,
        success: bool,
        error: Option<String>,
    },
    /// Every plugin has been processed.
    RunFinished { succeeded: usize, total: usize },
}

/// A plugin together with the library that provides its code.
///
/// Field order matters: `plugin` is dropped before `_library`, because the
/// plugin's drop glue lives in the library.
pub struct LoadedPlugin {
    pub plugin: Box<dyn Plugin>,
    _library: Option<Library>,
}

impl LoadedPlugin {
    #[cfg(test)]
    pub fn from_plugin(plugin: Box<dyn Plugin>) -> Self {
        Self {
            plugin,
            _library: None,
        }
    }
}

/// Load a plugin from a shared library exporting `create_plugin`.
///
/// The plugin's `renovatio_abi` fingerprint is checked first, and nothing else
/// in the library is called unless it matches the host's.
pub fn load_plugin(path: &str) -> Result<LoadedPlugin, String> {
    unsafe {
        let library = Library::new(path).map_err(|e| format!("Failed to load {path}: {e}"))?;

        let plugin_abi = library
            .get::<AbiType>(b"renovatio_abi\0")
            .ok()
            .map(|renovatio_abi| CStr::from_ptr(renovatio_abi()));
        check_abi(path, plugin_abi, renovatio::ABI_FINGERPRINT)?;

        let plugin = {
            let create_plugin: Symbol<PluginType> = library
                .get(b"create_plugin\0")
                .map_err(|e| format!("{path} is not a renovatio plugin: {e}"))?;
            Box::from_raw(create_plugin())
        };
        Ok(LoadedPlugin {
            plugin,
            _library: Some(library),
        })
    }
}

/// Accept a plugin only if it was built with the same ABI fingerprint as the host.
fn check_abi(path: &str, plugin: Option<&CStr>, host: &CStr) -> Result<(), String> {
    let rebuild = "Rebuild and reinstall the plugins (cd plugins && cargo build --release && just install-release).";
    match plugin {
        None => Err(format!(
            "{path} was built for an older version of renovatio. {rebuild}"
        )),
        Some(plugin) if plugin != host => Err(format!(
            "{path} was built for a different renovatio or compiler ({}) than this app ({}). {rebuild}",
            plugin.to_string_lossy(),
            host.to_string_lossy()
        )),
        Some(_) => Ok(()),
    }
}

/// Run each plugin in `paths` in order, sending `RunEvent`s to `tx`.
///
/// Always sends exactly one `Started` and one `Finished` per path, followed by
/// a single `RunFinished`.
pub fn run<L>(paths: &[String], load: L, tx: &flume::Sender<RunEvent>)
where
    L: Fn(&str) -> Result<LoadedPlugin, String>,
{
    let total = paths.len();
    let mut succeeded = 0;

    for (i, path) in paths.iter().enumerate() {
        let index = i + 1;

        let (name, success, error) = match load(path) {
            Ok(loaded) => {
                let name = loaded.plugin.name().to_string();
                let _ = tx.send(RunEvent::Started {
                    index,
                    total,
                    name: name.clone(),
                });
                println!("Running update for Plugin: {name}");

                let success = run_plugin(loaded.plugin.as_ref(), tx);
                let error = (!success).then(|| "Update failed".to_string());
                (name, success, error)
            }
            Err(error) => {
                eprintln!("{error}");
                let name = display_name(path);
                let _ = tx.send(RunEvent::Started {
                    index,
                    total,
                    name: name.clone(),
                });
                (name, false, Some(error))
            }
        };

        if success {
            succeeded += 1;
        }
        let _ = tx.send(RunEvent::Finished {
            index,
            total,
            name,
            success,
            error,
        });
    }

    let _ = tx.send(RunEvent::RunFinished { succeeded, total });
}

/// Run a single plugin's blocking update, forwarding its progress to `tx`.
///
/// Returns once the plugin has returned and all of its progress messages have
/// been forwarded, so `Finished` is always sent after the last `Progress`.
fn run_plugin(plugin: &dyn Plugin, tx: &flume::Sender<RunEvent>) -> bool {
    let (progress_tx, progress_rx) = flume::unbounded::<PluginProgress>();

    thread::scope(|scope| {
        scope.spawn(|| {
            // Ends once the plugin drops every clone of its sender.
            for progress in progress_rx.iter() {
                if tx.send(RunEvent::Progress(progress)).is_err() {
                    break;
                }
            }
        });

        plugin.update(progress_tx)
    })
}

/// Derive a readable name from a plugin path, e.g. `/x/libflatpak.so` -> `flatpak`.
fn display_name(path: &str) -> String {
    let stem = Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(path);
    stem.strip_prefix("lib").unwrap_or(stem).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fake plugin that sends the given progress values, then returns `result`.
    struct Fake {
        name: &'static str,
        steps: Vec<u32>,
        result: bool,
    }

    impl Plugin for Fake {
        fn name(&self) -> &str {
            self.name
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
        extern "Rust" fn update(&self, tx: flume::Sender<PluginProgress>) -> bool {
            for step in &self.steps {
                let mut progress = PluginProgress::new(self.name);
                progress.progress = *step;
                let _ = tx.send(progress);
            }
            self.result
        }
    }

    fn fake(name: &'static str, steps: &[u32], result: bool) -> Fake {
        Fake {
            name,
            steps: steps.to_vec(),
            result,
        }
    }

    /// Run `plugins` (keyed by path) and collect every event.
    fn run_fakes(paths: &[&str], make: impl Fn(&str) -> Option<Fake>) -> Vec<RunEvent> {
        let paths: Vec<String> = paths.iter().map(|p| p.to_string()).collect();
        let (tx, rx) = flume::unbounded();
        run(
            &paths,
            |path| {
                make(path)
                    .map(|p| LoadedPlugin::from_plugin(Box::new(p)))
                    .ok_or_else(|| format!("Failed to load {path}"))
            },
            &tx,
        );
        drop(tx);
        rx.iter().collect()
    }

    fn finished(events: &[RunEvent]) -> Vec<(usize, String, bool)> {
        events
            .iter()
            .filter_map(|e| match e {
                RunEvent::Finished {
                    index,
                    name,
                    success,
                    ..
                } => Some((*index, name.clone(), *success)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn repeated_100_progress_finishes_once() {
        // Like flatpak with one update: 100 after the item, then 100 again when done.
        let events = run_fakes(&["a", "b"], |path| match path {
            "a" => Some(fake("a", &[0, 100, 100], true)),
            _ => Some(fake("b", &[0, 50, 100], true)),
        });

        assert_eq!(
            finished(&events),
            vec![(1, "a".to_string(), true), (2, "b".to_string(), true)]
        );
    }

    #[test]
    fn failing_plugin_without_100_still_finishes() {
        let events = run_fakes(&["brew", "flatpak"], |path| match path {
            "brew" => Some(fake("brew", &[5], false)),
            _ => Some(fake("flatpak", &[100], true)),
        });

        assert_eq!(
            finished(&events),
            vec![
                (1, "brew".to_string(), false),
                (2, "flatpak".to_string(), true)
            ]
        );
        assert!(matches!(
            events.last(),
            Some(RunEvent::RunFinished {
                succeeded: 1,
                total: 2
            })
        ));
    }

    #[test]
    fn unloadable_plugin_is_reported_and_run_continues() {
        let events = run_fakes(&["/gone/libmissing.so", "ok"], |path| {
            (path == "ok").then(|| fake("ok", &[100], true))
        });

        assert_eq!(
            finished(&events),
            vec![
                (1, "missing".to_string(), false),
                (2, "ok".to_string(), true)
            ]
        );
        assert!(matches!(
            events.last(),
            Some(RunEvent::RunFinished {
                succeeded: 1,
                total: 2
            })
        ));
    }

    #[test]
    fn progress_is_forwarded_between_started_and_finished() {
        let events = run_fakes(&["a"], |_| Some(fake("a", &[10, 20], true)));

        assert!(matches!(events[0], RunEvent::Started { index: 1, .. }));
        assert!(matches!(&events[1], RunEvent::Progress(p) if p.progress == 10));
        assert!(matches!(&events[2], RunEvent::Progress(p) if p.progress == 20));
        assert!(matches!(
            events[3],
            RunEvent::Finished { success: true, .. }
        ));
        assert!(matches!(events[4], RunEvent::RunFinished { .. }));
        assert_eq!(events.len(), 5);
    }

    #[test]
    fn empty_run_finishes_immediately() {
        let events = run_fakes(&[], |_| None);
        assert!(matches!(
            events.as_slice(),
            [RunEvent::RunFinished {
                succeeded: 0,
                total: 0
            }]
        ));
    }

    #[test]
    fn check_abi_accepts_matching_fingerprint() {
        assert!(check_abi("p.so", Some(c"abi 1"), c"abi 1").is_ok());
    }

    #[test]
    fn check_abi_rejects_mismatched_fingerprint() {
        let error = check_abi("p.so", Some(c"abi 1"), c"abi 2").unwrap_err();
        assert!(error.contains("different renovatio or compiler (abi 1)"));
        assert!(error.contains("(abi 2)"));
    }

    #[test]
    fn check_abi_rejects_plugin_without_fingerprint() {
        let error = check_abi("p.so", None, c"abi 1").unwrap_err();
        assert!(error.contains("older version of renovatio"));
    }

    #[test]
    fn display_name_strips_lib_prefix_and_extension() {
        assert_eq!(display_name("/x/libflatpak.so"), "flatpak");
        assert_eq!(display_name("rpm-ostree.so"), "rpm-ostree");
    }
}
