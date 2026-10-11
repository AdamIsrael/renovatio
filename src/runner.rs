//! Runs the enabled plugins sequentially and reports their lifecycle to the UI.
//!
//! Completion is signalled explicitly by `RunEvent::Finished` once a plugin's
//! `update()` returns, rather than being inferred from `PluginProgress::progress`.
use std::ffi::{CStr, c_char};
use std::path::Path;
use std::thread;

use libloading::{Library, Symbol};
use renovatio::{Plugin, PluginMetadata, PluginProgress};

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
    /// Every plugin has been processed. `reboot_required` is true if any
    /// plugin reported that a reboot is needed to apply its updates.
    RunFinished {
        succeeded: usize,
        total: usize,
        reboot_required: bool,
    },
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

/// A discovered plugin, with the other discovered plugins it conflicts with.
#[derive(Clone, Debug)]
pub struct PluginEntry {
    pub metadata: PluginMetadata,
    /// `(path, name)` of plugins that shouldn't be enabled at the same time
    /// as this one, e.g. uupd and flatpak, since uupd also updates flatpaks.
    pub conflicts: Vec<(String, String)>,
}

/// Load each plugin in `paths` for its metadata and work out which of them
/// conflict. A pair conflicts if either plugin says so. Plugins that fail to
/// load are logged and left out.
pub fn discover<L>(paths: &[String], load: L) -> Vec<PluginEntry>
where
    L: Fn(&str) -> Result<LoadedPlugin, String>,
{
    let loaded: Vec<(String, LoadedPlugin)> = paths
        .iter()
        .filter_map(|path| match load(path) {
            Ok(loaded) => Some((path.clone(), loaded)),
            Err(error) => {
                eprintln!("{error}");
                None
            }
        })
        .collect();

    let conflicts: Vec<Vec<(String, String)>> = loaded
        .iter()
        .map(|(path, a)| {
            loaded
                .iter()
                .filter(|(other, b)| {
                    other != path
                        && (a.plugin.conflicts(b.plugin.name())
                            || b.plugin.conflicts(a.plugin.name()))
                })
                .map(|(other, b)| (other.clone(), b.plugin.name().to_string()))
                .collect()
        })
        .collect();

    loaded
        .into_iter()
        .zip(conflicts)
        .map(|((path, loaded), conflicts)| {
            let mut metadata = PluginMetadata::new(loaded.plugin);
            metadata.path = Some(path);
            PluginEntry {
                metadata,
                conflicts,
            }
        })
        .collect()
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
    let mut reboot_required = false;

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

                let (success, reboot) = run_plugin(loaded.plugin.as_ref(), tx);
                reboot_required |= reboot;
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

    let _ = tx.send(RunEvent::RunFinished {
        succeeded,
        total,
        reboot_required,
    });
}

/// Run a single plugin's blocking update, forwarding its progress to `tx`.
///
/// Returns once the plugin has returned and all of its progress messages have
/// been forwarded, so `Finished` is always sent after the last `Progress`.
/// Returns whether the update succeeded, and whether any progress message
/// reported `reboot_required`.
fn run_plugin(plugin: &dyn Plugin, tx: &flume::Sender<RunEvent>) -> (bool, bool) {
    let (progress_tx, progress_rx) = flume::unbounded::<PluginProgress>();

    thread::scope(|scope| {
        let forwarder = scope.spawn(|| {
            let mut reboot_required = false;
            // Ends once the plugin drops every clone of its sender.
            for progress in progress_rx.iter() {
                reboot_required |= progress.reboot_required;
                if tx.send(RunEvent::Progress(progress)).is_err() {
                    break;
                }
            }
            reboot_required
        });

        let success = plugin.update(progress_tx);
        (success, forwarder.join().unwrap_or(false))
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
        reboot: bool,
        conflicts_with: Vec<&'static str>,
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
        fn conflicts(&self, plugin_name: &str) -> bool {
            self.conflicts_with.contains(&plugin_name)
        }
        extern "Rust" fn update(&self, tx: flume::Sender<PluginProgress>) -> bool {
            for step in &self.steps {
                let mut progress = PluginProgress::new(self.name);
                progress.progress = *step;
                progress.reboot_required = self.reboot;
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
            reboot: false,
            conflicts_with: Vec::new(),
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
                total: 2,
                ..
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
                total: 2,
                ..
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
                total: 0,
                reboot_required: false
            }]
        ));
    }

    fn discover_fakes(plugins: Vec<Fake>) -> Vec<PluginEntry> {
        let paths: Vec<String> = plugins
            .iter()
            .map(|p| format!("/p/lib{}.so", p.name))
            .collect();
        let plugins = std::cell::RefCell::new(plugins.into_iter());
        discover(&paths, |_| {
            Ok(LoadedPlugin::from_plugin(Box::new(
                plugins.borrow_mut().next().unwrap(),
            )))
        })
    }

    fn conflict_names(entry: &PluginEntry) -> Vec<&str> {
        entry
            .conflicts
            .iter()
            .map(|(_, name)| name.as_str())
            .collect()
    }

    #[test]
    fn discover_finds_conflicts_in_both_directions() {
        // Only uupd declares the conflict; flatpak must still see it.
        let entries = discover_fakes(vec![
            Fake {
                conflicts_with: vec!["flatpak"],
                ..fake("uupd", &[], true)
            },
            fake("flatpak", &[], true),
            fake("rustup", &[], true),
        ]);

        assert_eq!(conflict_names(&entries[0]), vec!["flatpak"]);
        assert_eq!(conflict_names(&entries[1]), vec!["uupd"]);
        assert!(entries[2].conflicts.is_empty());
        assert_eq!(entries[1].conflicts[0].0, "/p/libuupd.so");
        assert_eq!(
            entries[1].metadata.path.as_deref(),
            Some("/p/libflatpak.so")
        );
    }

    #[test]
    fn discover_skips_plugins_that_fail_to_load() {
        let paths = vec!["/p/libgone.so".to_string(), "/p/libok.so".to_string()];
        let entries = discover(&paths, |path| {
            if path.contains("gone") {
                Err("Failed to load".to_string())
            } else {
                Ok(LoadedPlugin::from_plugin(Box::new(fake("ok", &[], true))))
            }
        });
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].metadata.name, "ok");
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
    fn reboot_required_is_reported_if_any_plugin_needs_it() {
        let events = run_fakes(&["os", "apps"], |path| match path {
            "os" => Some(Fake {
                reboot: true,
                ..fake("os", &[100], true)
            }),
            _ => Some(fake("apps", &[100], true)),
        });
        assert!(matches!(
            events.last(),
            Some(RunEvent::RunFinished {
                reboot_required: true,
                ..
            })
        ));
    }

    #[test]
    fn reboot_not_required_by_default() {
        let events = run_fakes(&["apps"], |_| Some(fake("apps", &[100], true)));
        assert!(matches!(
            events.last(),
            Some(RunEvent::RunFinished {
                reboot_required: false,
                ..
            })
        ));
    }

    #[test]
    fn display_name_strips_lib_prefix_and_extension() {
        assert_eq!(display_name("/x/libflatpak.so"), "flatpak");
        assert_eq!(display_name("rpm-ostree.so"), "rpm-ostree");
    }
}
