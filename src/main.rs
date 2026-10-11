mod actions;
mod config;
mod modals;
mod runner;
mod ui;
mod utils;

use flume::{Receiver, unbounded};
use runner::{PluginEntry, RunEvent};

use gtk::prelude::*;

use std::thread;

fn main() -> glib::ExitCode {
    // Initialize our GSettings schema, if it doesn't exist
    utils::install_gsettings_schema();

    // Create a new application
    let application = adw::Application::builder()
        .application_id(config::APP_ID)
        .build();

    // Load plugins once, at startup: their metadata and conflicts are shown in
    // Preferences, and every run reuses them.
    let plugins = runner::discover(&utils::find_plugins(), runner::load_plugin);

    application.connect_activate(move |app| {
        let window = build_ui(app, plugins.clone());

        // Connect to the "close-request" signal
        window.connect_close_request(move |window| {
            // If the window closes, quit the application even if there are active sender channels
            window.application().unwrap().quit();

            // Return `glib::Propagation::Stop` to prevent the default handler
            // from closing the window, or `glib::Propagation::Proceed` to allow it.
            // For example, you might show a confirmation dialog here.
            glib::Propagation::Proceed
        });
    });

    // Run the application
    application.run()
}

fn build_ui(app: &adw::Application, plugins: Vec<PluginEntry>) -> adw::ApplicationWindow {
    // Create a channel that will be used to send messages from worker threads
    let (tx, rx): (flume::Sender<RunEvent>, Receiver<RunEvent>) = unbounded();
    let loaded = runner::Plugins::new(&plugins);

    let header_bar = ui::get_header_bar();
    let update_button = ui::get_update_button();
    let plugin_progress_bar = ui::get_plugin_progress_bar();
    let total_progress_bar = ui::get_total_progress_bar();
    let apply_check_button = ui::get_apply_check_button();

    // Create cloned references because the closure will capture them
    let tpbar = total_progress_bar.clone();
    let ppbar = plugin_progress_bar.clone();
    let apply = apply_check_button.clone();
    let update = update_button.clone();

    // Connect update button to run a system update command
    update_button.connect_clicked(move |_| {
        // Disable the update button and checkbox until the whole run finishes
        apply.set_sensitive(false);
        update.set_sensitive(false);
        ppbar.set_visible(true);
        tpbar.set_visible(true);
        tpbar.set_fraction(0.0);

        // Snapshot the enabled plugin(s) so the run isn't affected by later changes
        let settings = gio::Settings::new(config::APP_ID);
        let paths = settings.get::<Vec<String>>("plugins");

        let tx_worker = tx.clone();
        let loaded = loaded.clone();
        thread::spawn(move || {
            runner::run(
                &paths,
                |path| loaded.get(path, runner::load_plugin),
                &tx_worker,
            );
        });
    });

    let main_box = ui::get_main_container(
        &header_bar,
        &update_button,
        &apply_check_button,
        &plugin_progress_bar,
        &total_progress_bar,
    );
    let window = ui::get_window(app, "Renovatio", main_box);

    // Now that we have the window, connect the menu actions
    actions::set_about(app, &window);
    actions::set_preferences(app, &window, plugins.clone());
    actions::set_quit(app);

    // Present window
    window.present();

    let ppbar_clone = plugin_progress_bar.clone();
    let tpbar_clone = total_progress_bar.clone();
    let apply_clone = apply_check_button.clone();
    let update_clone = update_button.clone();

    // Handle events from the worker on the main thread. `recv_async` suspends
    // this task until an event arrives, so nothing runs while the app is idle.
    glib::spawn_future_local(async move {
        let pulser = ui::Pulser::new(&ppbar_clone);

        while let Ok(event) = rx.recv_async().await {
            match event {
                RunEvent::Started { index, total, name } => {
                    tpbar_clone.set_text(Some(&format!("Updating {name} ({index}/{total})...")));
                    tpbar_clone.set_fraction((index - 1) as f64 / total as f64);

                    pulser.stop();
                    ppbar_clone.set_fraction(0.0);
                    ppbar_clone.set_text(Some(""));
                }
                RunEvent::Progress(progress) => {
                    // handle stdout/stderr
                    if let Some(stdout) = progress.stdout {
                        println!("[{}]: {}", progress.name, stdout);
                    }
                    if let Some(stderr) = progress.stderr {
                        println!("[{}]: {}", progress.name, stderr);
                    }

                    ppbar_clone.set_text(Some(&progress.status));

                    if progress.pulse {
                        pulser.start();
                    } else {
                        pulser.stop();
                        ppbar_clone.set_fraction(progress.progress.min(100) as f64 / 100.0);
                    }

                    // TODO: Append stdout and stderr to a `TextView` in the UI
                }
                RunEvent::Finished {
                    index,
                    total,
                    name,
                    success,
                    error,
                } => {
                    pulser.stop();
                    if success {
                        println!("Update successful: {name}");
                        ppbar_clone.set_fraction(1.0);
                    } else {
                        let error = error.unwrap_or_else(|| "Update failed".to_string());
                        println!("Update failed: {name}: {error}");
                        ppbar_clone.set_text(Some(&format!("{name}: {error}")));
                    }
                    tpbar_clone.set_fraction(index as f64 / total as f64);
                }
                RunEvent::RunFinished {
                    succeeded,
                    total,
                    reboot_required,
                } => {
                    tpbar_clone.set_fraction(1.0);

                    if total == 0 {
                        tpbar_clone.set_text(Some("No plugins enabled. See Preferences."));
                        apply_clone.set_sensitive(true);
                        update_clone.set_sensitive(true);
                        continue;
                    }

                    let failed = total - succeeded;
                    let summary = if failed > 0 {
                        format!("Updates complete ({failed} of {total} failed)!")
                    } else {
                        "Updates complete!".to_string()
                    };

                    // A plugin may have reported that a reboot is needed. Not all
                    // do (rpm-ostree, uupd), so with auto-reboot on, fall back to
                    // checking for a staged deployment. rpm-ostree can be slow,
                    // so run it off the main thread.
                    let auto_reboot = apply_clone.is_active();
                    let reboot = auto_reboot
                        && (reboot_required
                            || gio::spawn_blocking(utils::check_reboot_needed)
                                .await
                                .unwrap_or(false));

                    if reboot {
                        // Leave the controls disabled: an update started now
                        // would be cut short by the reboot.
                        tpbar_clone.set_text(Some(&format!("{summary} Rebooting...")));
                        glib::timeout_future_seconds(3).await;
                        if utils::reboot_system() {
                            continue;
                        }
                        tpbar_clone.set_text(Some(&format!("{summary} Reboot failed.")));
                    } else if reboot_required && !auto_reboot {
                        tpbar_clone.set_text(Some(&format!("{summary} Reboot to finish.")));
                    } else {
                        tpbar_clone.set_text(Some(&summary));
                    }

                    apply_clone.set_sensitive(true);
                    update_clone.set_sensitive(true);
                }
            }
        }
    });

    window
}
