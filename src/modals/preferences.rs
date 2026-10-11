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
                                disable(&mut plugins, other_path);
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
                    enable(&mut plugins, &plugin_path);
                } else {
                    disable(&mut plugins, &plugin_path);
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

/// Add `path` to the enabled plugins, unless it's already there.
/// Plugins run in the order they were enabled.
fn enable(plugins: &mut Vec<String>, path: &str) {
    if !plugins.iter().any(|p| p == path) {
        plugins.push(path.to_string());
    }
}

/// Remove every entry for exactly `path` from the enabled plugins.
fn disable(plugins: &mut Vec<String>, path: &str) {
    plugins.retain(|p| p != path);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(paths: &[&str]) -> Vec<String> {
        paths.iter().map(|p| p.to_string()).collect()
    }

    #[test]
    fn enable_appends_once() {
        let mut plugins = list(&["/p/libbrew.so"]);
        enable(&mut plugins, "/p/libflatpak.so");
        enable(&mut plugins, "/p/libflatpak.so");
        assert_eq!(plugins, list(&["/p/libbrew.so", "/p/libflatpak.so"]));
    }

    #[test]
    fn disable_removes_only_the_exact_path() {
        // The old substring match also removed the second entry.
        let mut plugins = list(&["/p/libuupd.so", "/home/u/p/libuupd.so"]);
        disable(&mut plugins, "/p/libuupd.so");
        assert_eq!(plugins, list(&["/home/u/p/libuupd.so"]));
    }

    #[test]
    fn disable_removes_duplicates_left_by_older_versions() {
        let mut plugins = list(&["/p/libbrew.so", "/p/libflatpak.so", "/p/libbrew.so"]);
        disable(&mut plugins, "/p/libbrew.so");
        assert_eq!(plugins, list(&["/p/libflatpak.so"]));
    }
}
