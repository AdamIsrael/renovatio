// A preferences modal that allows the user to configure the application's settings.
use crate::{PluginMetadata, config};

use adw::prelude::*;
use adw::{PreferencesDialog, PreferencesGroup, PreferencesPage, SwitchRow};

pub fn show(parent: &impl IsA<gtk::Widget>, plugins: Vec<PluginMetadata>) {
    let dialog = PreferencesDialog::builder()
        // .transient_for(&parent) // Make it a transient dialog for the main window
        // .modal(true) // Make it a modal dialog
        .build();

    let page1 = PreferencesPage::builder()
        .title("General")
        .icon_name("preferences-system-symbolic")
        .build();

    let group1 = PreferencesGroup::builder().title("Plugins").build();

    for metadata in plugins {
        let switch_row = SwitchRow::builder()
            .title(&metadata.name)
            .subtitle(&metadata.description)
            .build();

        let settings = gio::Settings::new(config::APP_ID);

        // get the current plugins key
        let plugins = settings.get::<Vec<String>>("plugins");
        if let Some(path) = metadata.path.clone() {
            let plugin = plugins.contains(&path);
            if plugin {
                // Set the row to active if the plugin is enabled
                switch_row.set_active(true);
            }
        }

        // Connect a signal to the switch
        switch_row.connect_active_notify(move |state| {
            let mut plugins = settings.get::<Vec<String>>("plugins");

            // get the path for the current plugin
            let plugin_path = metadata.path.clone().unwrap();

            // append or remove the current plugin
            if state.is_active() {
                // append the plugin to the list
                plugins.push(plugin_path);
            } else {
                // remove the plugin from the list
                plugins.retain(|path| !path.contains(&plugin_path));
            }
            println!("Saving plugins: {:?}", plugins);
            // save the changes to gsettings
            let _ = settings.set("plugins", &plugins);

            // Handle the state change here
            // println!("{} is active: {}", metadata.name, state.is_active());
        });

        group1.add(&switch_row);
    }

    // group1.add(&row1);
    page1.add(&group1);
    dialog.add(&page1);

    dialog.present(Some(parent));
}
