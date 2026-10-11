// A preferences modal that allows the user to configure the application's settings.
use crate::config;
use crate::runner::PluginEntry;

use adw::prelude::*;
use adw::{PreferencesDialog, PreferencesGroup, PreferencesPage, SwitchRow, Toast};

use std::collections::HashMap;
use std::rc::Rc;

pub fn show(parent: &impl IsA<gtk::Widget>, plugins: Vec<PluginEntry>) {
    let dialog = PreferencesDialog::builder()
        // .transient_for(&parent) // Make it a transient dialog for the main window
        // .modal(true) // Make it a modal dialog
        .build();

    let page1 = PreferencesPage::builder()
        .title("General")
        .icon_name("preferences-system-symbolic")
        .build();

    let group1 = PreferencesGroup::builder().title("Plugins").build();

    // Create every row first, so enabling one plugin can switch off the rows
    // of plugins it conflicts with.
    let settings = gio::Settings::new(config::APP_ID);
    let enabled = settings.get::<Vec<String>>("plugins");
    let mut rows = HashMap::new();
    for entry in &plugins {
        let metadata = &entry.metadata;
        let subtitle = if entry.conflicts.is_empty() {
            metadata.description.clone()
        } else {
            let names: Vec<&str> = entry
                .conflicts
                .iter()
                .map(|(_, name)| name.as_str())
                .collect();
            format!(
                "{} Conflicts with {}.",
                metadata.description,
                names.join(", ")
            )
        };

        let switch_row = SwitchRow::builder()
            .title(&metadata.name)
            .subtitle(&subtitle)
            .build();

        if let Some(path) = &metadata.path {
            // Set the row to active if the plugin is enabled
            switch_row.set_active(enabled.contains(path));
            rows.insert(path.clone(), switch_row.clone());
        }

        group1.add(&switch_row);
    }
    let rows = Rc::new(rows);

    for entry in plugins {
        let Some(switch_row) = entry.metadata.path.as_ref().and_then(|path| rows.get(path)) else {
            continue;
        };
        let settings = gio::Settings::new(config::APP_ID);
        let rows = Rc::clone(&rows);

        // Connect a signal to the switch
        switch_row.connect_active_notify(glib::clone!(
            #[weak]
            dialog,
            move |state| {
                // get the path for the current plugin
                let plugin_path = entry.metadata.path.clone().unwrap();

                // Switch off enabled plugins that conflict with this one.
                // Their own handlers remove them from the settings.
                if state.is_active() {
                    let enabled = settings.get::<Vec<String>>("plugins");
                    for (other_path, other_name) in &entry.conflicts {
                        if !enabled.contains(other_path) {
                            continue;
                        }
                        match rows.get(other_path) {
                            Some(other_row) => other_row.set_active(false),
                            None => {
                                let mut plugins = settings.get::<Vec<String>>("plugins");
                                plugins.retain(|path| path != other_path);
                                let _ = settings.set("plugins", &plugins);
                            }
                        }
                        dialog.add_toast(Toast::new(&format!(
                            "Disabled {other_name}: it conflicts with {}",
                            entry.metadata.name
                        )));
                    }
                }

                let mut plugins = settings.get::<Vec<String>>("plugins");

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
            }
        ));
    }

    // group1.add(&row1);
    page1.add(&group1);
    dialog.add(&page1);

    dialog.present(Some(parent));
}
