mod actions;
mod config;
mod modals;
mod runner;
mod ui;
mod utils;

use flume::{Receiver, unbounded};
use renovatio::PluginMetadata;
use runner::RunEvent;

use gtk::prelude::*;

use std::thread;

fn main() -> glib::ExitCode {
    // Initialize our GSettings schema, if it doesn't exist
    utils::install_gsettings_schema();

    // Create a new application
    let application = adw::Application::builder()
        .application_id(config::APP_ID)
        .build();

    // Load plugins at startup, in a Vec<PluginMetadata>
    let mut plugins: Vec<PluginMetadata> = Vec::new();
    for plugin_path in utils::find_plugins() {
        match runner::load_plugin(&plugin_path) {
            Ok(loaded) => {
                let mut metadata = PluginMetadata::new(loaded.plugin);
                metadata.path = Some(plugin_path);
                plugins.push(metadata);
            }
            Err(error) => eprintln!("{error}"),
        }
    }

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

fn build_ui(app: &adw::Application, plugins: Vec<PluginMetadata>) -> adw::ApplicationWindow {
    // Create a channel that will be used to send messages from worker threads
    let (tx, rx): (flume::Sender<RunEvent>, Receiver<RunEvent>) = unbounded();

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
        thread::spawn(move || {
            runner::run(&paths, runner::load_plugin, &tx_worker);
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

    // This is called each time GTK is idle (i.e., not processing events).
    // It will run as often as possible but never blocks the main loop.
    glib::idle_add_local(move || {
        // Even if we don't have a progress update from a plugin,
        // pulse the progress bar if we have a pulse_step defined.
        if ppbar_clone.pulse_step() > 0.0 {
            ppbar_clone.pulse();
        }

        // Try to receive a message. `try_recv` is non‑blocking.
        match rx.try_recv() {
            Ok(RunEvent::Started { index, total, name }) => {
                tpbar_clone.set_text(Some(&format!("Updating {name} ({index}/{total})...")));
                tpbar_clone.set_fraction((index - 1) as f64 / total as f64);

                ppbar_clone.set_pulse_step(0.0);
                ppbar_clone.set_fraction(0.0);
                ppbar_clone.set_text(Some(""));
                glib::ControlFlow::Continue
            }
            Ok(RunEvent::Progress(progress)) => {
                // handle stdout/stderr
                if let Some(stdout) = progress.stdout {
                    println!("[{}]: {}", progress.name, stdout);
                }
                if let Some(stderr) = progress.stderr {
                    println!("[{}]: {}", progress.name, stderr);
                }

                ppbar_clone.set_text(Some(&progress.status));

                if progress.pulse {
                    ppbar_clone.set_pulse_step(0.25);
                    ppbar_clone.pulse();
                } else {
                    ppbar_clone.set_pulse_step(0.0);
                    ppbar_clone.set_fraction(progress.progress.min(100) as f64 / 100.0);
                }

                // TODO: Append stdout and stderr to a `TextView` in the UI
                glib::ControlFlow::Continue
            }
            Ok(RunEvent::Finished {
                index,
                total,
                name,
                success,
                error,
            }) => {
                ppbar_clone.set_pulse_step(0.0);
                if success {
                    println!("Update successful: {name}");
                    ppbar_clone.set_fraction(1.0);
                } else {
                    let error = error.unwrap_or_else(|| "Update failed".to_string());
                    println!("Update failed: {name}: {error}");
                    ppbar_clone.set_text(Some(&format!("{name}: {error}")));
                }
                tpbar_clone.set_fraction(index as f64 / total as f64);
                glib::ControlFlow::Continue
            }
            Ok(RunEvent::RunFinished { succeeded, total }) => {
                apply_clone.set_sensitive(true);
                update_clone.set_sensitive(true);
                tpbar_clone.set_fraction(1.0);

                if total == 0 {
                    tpbar_clone.set_text(Some("No plugins enabled. See Preferences."));
                    return glib::ControlFlow::Continue;
                }

                // Check to see if we need to reboot
                let reboot = apply_clone.is_active();
                let failed = total - succeeded;

                let msg = format!(
                    "Updates complete{}! {}",
                    if failed > 0 {
                        format!(" ({failed} of {total} failed)")
                    } else {
                        String::new()
                    },
                    if reboot { "Rebooting..." } else { "" }
                );

                tpbar_clone.set_text(Some(&msg));

                if reboot && utils::check_reboot_needed() {
                    std::thread::sleep(std::time::Duration::from_secs(3));
                    utils::reboot_system();
                }

                glib::ControlFlow::Continue
            }
            Err(flume::TryRecvError::Empty) => {
                // No messages, but there are sender(s) alive – keep the idle handler alive
                glib::ControlFlow::Continue
            }
            Err(flume::TryRecvError::Disconnected) => {
                // All senders dropped – no more work will come.
                println!("All senders disconnected. Stopping idle callback.");
                glib::ControlFlow::Break
            }
        }
    });

    window
}
