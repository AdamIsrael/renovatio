mod actions;
mod config;
mod modals;
mod ui;
mod utils;

use flume::{Receiver, unbounded};
use renovatio::{Plugin, PluginMetadata, PluginProgress};

use gtk::prelude::*;
use libloading::{Library, Symbol};

// use std::cell::RefCell;
// use std::sync::{Arc, Mutex, mpsc};
use std::thread;

type PluginType = unsafe fn() -> *mut dyn Plugin;

fn main() -> glib::ExitCode {
    // Initialize our GSettings schema, if it doesn't exist
    utils::install_gsettings_schema();

    // Create a new application
    let application = adw::Application::builder()
        .application_id(config::APP_ID)
        .build();

    // Load plugins at startup, in a Vec<PluginMetadata>
    let mut plugins: Vec<PluginMetadata> = Vec::new();
    unsafe {
        let plugin_libraries = utils::find_plugins();
        for plugin_path in &plugin_libraries {
            if let Ok(lib) = Library::new(plugin_path) {
                let result: Result<Symbol<PluginType>, _> = lib.get(b"create_plugin\0");
                if let Ok(create_plugin) = result {
                    let plugin_ptr = create_plugin();
                    let plugin: Box<dyn Plugin> = Box::from_raw(plugin_ptr); // Reclaim ownership
                    let mut metadata = PluginMetadata::new(plugin);
                    metadata.path = Some(plugin_path.clone());
                    plugins.push(metadata);
                }
            }
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
    let (tx, rx): (flume::Sender<PluginProgress>, Receiver<PluginProgress>) = unbounded();

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

    // Clone handles for the closure that will be run in a new thread
    let tx_clone = tx.clone();

    // Connect update button to run a system update command
    update_button.connect_clicked(move |_| {
        // Disable the update button and checkbox while running updates
        apply.set_sensitive(false);
        update.set_sensitive(false);
        ppbar.set_visible(true);
        tpbar.set_visible(true);

        let tx_worker = tx_clone.clone();

        thread::spawn(move || {
            let settings = gio::Settings::new(config::APP_ID);

            // Load the enabled plugin(s)
            let plugins = settings.get::<Vec<String>>("plugins");

            for plugin in plugins {
                let tx_plugin = tx_worker.clone();

                unsafe {
                    // Load the shared library
                    if let Ok(lib) = Library::new(plugin) {
                        // Instantiate the plugin
                        let result: Result<Symbol<PluginType>, _> = lib.get(b"create_plugin\0");
                        if let Ok(create_plugin) = result {
                            // Get the plugin object
                            let plugin_ptr = create_plugin();
                            let plugin: Box<dyn Plugin> = Box::from_raw(plugin_ptr); // Reclaim ownership

                            println!("Running update for Plugin: {}", plugin.name());

                            // Run the blocking update
                            if plugin.update(tx_plugin) {
                                println!("Update successful");
                            } else {
                                println!("Update failed");
                            }
                        }
                    }
                };
            }
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
    let settings = gio::Settings::new(config::APP_ID);
    let mut plugin_index = 1;

    glib::idle_add_local(move || {
        // Load the enabled plugin(s)
        let plugins = settings.get::<Vec<String>>("plugins");

        let plugin_count = plugins.len();

        // Even if we don't have a progress update from a plugin,
        // pulse the progress bar if we have a pulse_step defined.
        if ppbar_clone.pulse_step() > 0.0 {
            ppbar_clone.pulse();
        }

        // Try to receive a message. `try_recv` is non‑blocking.
        match rx.try_recv() {
            Ok(progress) => {
                // handle stdout/stderr
                if let Some(stdout) = progress.stdout {
                    println!("[{}]: {}", progress.name, stdout);
                }
                if let Some(stderr) = progress.stderr {
                    println!("[{}]: {}", progress.name, stderr);
                }

                let total_status = format!(
                    "Updating {} ({}/{})...",
                    progress.name, plugin_index, plugin_count
                );

                // Update the UI
                tpbar_clone.set_text(Some(&total_status));
                ppbar_clone.set_text(Some(&progress.status));

                if progress.pulse {
                    println!("Pulsing plugin progress bar");
                    ppbar_clone.set_pulse_step(0.25);
                    ppbar_clone.pulse();
                } else {
                    ppbar_clone.set_pulse_step(0.0);
                    ppbar_clone.set_fraction(progress.progress as f64 / 100.0);
                }

                // TODO: Append stdout and stderr to a `TextView` in the UI

                if progress.progress == 100 {
                    apply_clone.set_sensitive(true);
                    update_clone.set_sensitive(true);

                    // If we're done updating the last plugin, update the UI
                    if plugin_index == plugin_count {
                        // Disable pulsing and set the bar to 100%
                        ppbar_clone.set_pulse_step(0.0);
                        ppbar_clone.set_fraction(1.0);

                        tpbar_clone.set_text(Some("Updates complete!"));

                        tpbar_clone.set_fraction(1.0);

                        // Check to see if we need to reboot
                        let reboot = apply_clone.is_active();

                        let msg = format!(
                            "Updates complete! {}",
                            if reboot { "Rebooting..." } else { "" }
                        );

                        tpbar_clone.set_text(Some(&msg));

                        if reboot && utils::check_reboot_needed() {
                            std::thread::sleep(std::time::Duration::from_secs(3));
                            utils::reboot_system();
                        }
                    } else {
                        plugin_index += 1;
                    }
                }

                // Return Continue if you want to keep listening,
                // or Stop if this idle callback should run only once.
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
