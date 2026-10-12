// A preferences modal that allows the user to configure the application's settings.
use crate::config;
use crate::runner::PluginEntry;

use adw::prelude::*;
use adw::{PreferencesDialog, PreferencesGroup, PreferencesPage, SwitchRow, Toast};

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// The plugin rows, and their order on screen, which is the run order.
struct PluginList {
    list_box: gtk::ListBox,
    rows: HashMap<String, SwitchRow>,
    /// Each row's "Move Up" and "Move Down" actions.
    moves: HashMap<String, [gio::SimpleAction; 2]>,
    order: RefCell<Vec<String>>,
    settings: gio::Settings,
}

impl PluginList {
    /// Save the enabled plugins, in the order shown.
    fn save(&self) {
        let previous = self.settings.get::<Vec<String>>("plugins");
        let plugins = enabled_in_order(
            &self.order.borrow(),
            |path| self.rows.get(path).is_some_and(|row| row.is_active()),
            &previous,
        );
        println!("Saving plugins: {:?}", plugins);
        let _ = self.settings.set("plugins", &plugins);
    }

    /// Move the row for `path` to position `to`, then save.
    fn move_row(&self, path: &str, to: usize) {
        let Some(from) = self.order.borrow().iter().position(|p| p == path) else {
            return;
        };
        let Some(row) = self.rows.get(path) else {
            return;
        };
        if !move_item(&mut self.order.borrow_mut(), from, to) {
            return;
        }
        self.list_box.remove(row);
        self.list_box.insert(row, to as i32);
        row.grab_focus();
        self.update_move_actions();
        self.save();
    }

    /// Move `path` one place up (`-1`) or down (`1`).
    fn step(&self, path: &str, delta: isize) {
        let Some(from) = self.order.borrow().iter().position(|p| p == path) else {
            return;
        };
        if let Some(to) = from.checked_add_signed(delta) {
            self.move_row(path, to);
        }
    }

    /// Disable "Move Up" on the first row and "Move Down" on the last.
    fn update_move_actions(&self) {
        let order = self.order.borrow();
        for (index, path) in order.iter().enumerate() {
            if let Some([up, down]) = self.moves.get(path) {
                up.set_enabled(index > 0);
                down.set_enabled(index + 1 < order.len());
            }
        }
    }
}

pub fn show(parent: &impl IsA<gtk::Widget>, plugins: Vec<PluginEntry>) {
    let dialog = PreferencesDialog::builder()
        // .transient_for(&parent) // Make it a transient dialog for the main window
        // .modal(true) // Make it a modal dialog
        .build();

    let page1 = PreferencesPage::builder()
        .title("General")
        .icon_name("preferences-system-symbolic")
        .build();

    let group1 = PreferencesGroup::builder()
        .title("Plugins")
        .description("Plugins run from top to bottom. Drag them to change the order.")
        .build();

    let list_box = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    group1.add(&list_box);

    // Create every row first, so enabling one plugin can switch off the rows
    // of plugins it conflicts with.
    let settings = gio::Settings::new(config::APP_ID);
    let enabled = settings.get::<Vec<String>>("plugins");
    let mut rows = HashMap::new();
    let mut moves = HashMap::new();
    let mut discovered = Vec::new();
    for entry in &plugins {
        let metadata = &entry.metadata;
        let Some(path) = &metadata.path else {
            continue;
        };
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
            .active(enabled.contains(path))
            .build();
        switch_row.add_prefix(&gtk::Image::from_icon_name("list-drag-handle-symbolic"));

        // Keyboard-accessible alternative to dragging
        let menu = gio::Menu::new();
        menu.append(Some("Move Up"), Some("row.move-up"));
        menu.append(Some("Move Down"), Some("row.move-down"));
        let menu_button = gtk::MenuButton::builder()
            .icon_name("view-more-symbolic")
            .menu_model(&menu)
            .valign(gtk::Align::Center)
            .tooltip_text("Move")
            .css_classes(["flat"])
            .build();
        switch_row.add_suffix(&menu_button);

        let up = gio::SimpleAction::new("move-up", None);
        let down = gio::SimpleAction::new("move-down", None);
        let actions = gio::SimpleActionGroup::new();
        actions.add_action(&up);
        actions.add_action(&down);
        switch_row.insert_action_group("row", Some(&actions));
        moves.insert(path.clone(), [up, down]);

        rows.insert(path.clone(), switch_row);
        discovered.push(path.clone());
    }

    let order = display_order(&discovered, &enabled);
    for path in &order {
        list_box.append(&rows[path]);
    }

    let list = Rc::new(PluginList {
        list_box,
        rows,
        moves,
        order: RefCell::new(order),
        settings,
    });

    for entry in plugins {
        let Some(path) = entry.metadata.path.clone() else {
            continue;
        };
        let switch_row = &list.rows[&path];

        // Move Up / Move Down
        for (action, delta) in list.moves[&path].iter().zip([-1, 1]) {
            let list = Rc::downgrade(&list);
            let path = path.clone();
            action.connect_activate(move |_, _| {
                if let Some(list) = list.upgrade() {
                    list.step(&path, delta);
                }
            });
        }

        // Drag a row by its path, and drop it onto another row to take its place.
        let drag_source = gtk::DragSource::builder()
            .actions(gtk::gdk::DragAction::MOVE)
            .build();
        drag_source.connect_prepare(glib::clone!(
            #[strong]
            path,
            move |_, _, _| Some(gtk::gdk::ContentProvider::for_value(&path.to_value()))
        ));
        drag_source.connect_drag_begin(glib::clone!(
            #[weak]
            switch_row,
            move |source, _| {
                let icon = gtk::WidgetPaintable::new(Some(&switch_row));
                source.set_icon(Some(&icon), 0, 0);
            }
        ));
        switch_row.add_controller(drag_source);

        let drop_target = gtk::DropTarget::new(String::static_type(), gtk::gdk::DragAction::MOVE);
        drop_target.connect_drop(glib::clone!(
            #[weak]
            list,
            #[strong]
            path,
            #[upgrade_or]
            false,
            move |_, value, _, _| {
                let Ok(dragged) = value.get::<String>() else {
                    return false;
                };
                let Some(to) = list.order.borrow().iter().position(|p| *p == path) else {
                    return false;
                };
                list.move_row(&dragged, to);
                true
            }
        ));
        switch_row.add_controller(drop_target);

        // Connect a signal to the switch
        let weak_list = Rc::downgrade(&list);
        switch_row.connect_active_notify(glib::clone!(
            #[weak]
            dialog,
            move |state| {
                let Some(list) = weak_list.upgrade() else {
                    return;
                };

                // Switch off enabled plugins that conflict with this one.
                // Their own handlers save the change.
                if state.is_active() {
                    for (other_path, other_name) in &entry.conflicts {
                        let Some(other_row) = list.rows.get(other_path) else {
                            continue;
                        };
                        if !other_row.is_active() {
                            continue;
                        }
                        other_row.set_active(false);
                        dialog.add_toast(Toast::new(&format!(
                            "Disabled {other_name}: it conflicts with {}",
                            entry.metadata.name
                        )));
                    }
                }

                list.save();
            }
        ));
    }
    list.update_move_actions();

    // The row handlers only hold weak references, so the dialog keeps the list.
    dialog.connect_closed(move |_| {
        let _ = &list;
    });

    page1.add(&group1);
    dialog.add(&page1);

    dialog.present(Some(parent));
}

/// The order to show plugins in, which is the order they run in: enabled
/// plugins first, in the order they're saved, then the rest as discovered.
fn display_order(discovered: &[String], enabled: &[String]) -> Vec<String> {
    let mut order: Vec<String> = Vec::new();
    for path in enabled {
        if discovered.contains(path) && !order.contains(path) {
            order.push(path.clone());
        }
    }
    for path in discovered {
        if !order.contains(path) {
            order.push(path.clone());
        }
    }
    order
}

/// The plugins list to save: the active plugins, in the order shown, then any
/// previously enabled paths that aren't shown (e.g. a plugin that is no longer
/// installed), so they aren't lost. Paths are matched exactly and never
/// repeated.
fn enabled_in_order(
    order: &[String],
    is_active: impl Fn(&str) -> bool,
    previous: &[String],
) -> Vec<String> {
    let mut plugins: Vec<String> = order
        .iter()
        .filter(|path| is_active(path))
        .cloned()
        .collect();
    for path in previous {
        if !order.contains(path) && !plugins.contains(path) {
            plugins.push(path.clone());
        }
    }
    plugins
}

/// Move the item at `from` to `to`, shifting the items in between. Returns
/// false, leaving `order` unchanged, if either index is out of range or they're
/// the same.
fn move_item(order: &mut Vec<String>, from: usize, to: usize) -> bool {
    if from == to || from >= order.len() || to >= order.len() {
        return false;
    }
    let item = order.remove(from);
    order.insert(to, item);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(paths: &[&str]) -> Vec<String> {
        paths.iter().map(|p| p.to_string()).collect()
    }

    #[test]
    fn display_order_puts_enabled_first_in_saved_order() {
        let discovered = list(&["/p/a.so", "/p/b.so", "/p/c.so", "/p/d.so"]);
        let enabled = list(&["/p/c.so", "/gone/x.so", "/p/a.so", "/p/c.so"]);
        assert_eq!(
            display_order(&discovered, &enabled),
            list(&["/p/c.so", "/p/a.so", "/p/b.so", "/p/d.so"])
        );
    }

    #[test]
    fn enabled_in_order_follows_the_order_shown() {
        let order = list(&["/p/c.so", "/p/a.so", "/p/b.so"]);
        let active = |path: &str| path != "/p/b.so";
        assert_eq!(
            enabled_in_order(&order, active, &list(&["/p/a.so", "/p/c.so"])),
            list(&["/p/c.so", "/p/a.so"])
        );
    }

    #[test]
    fn enabled_in_order_keeps_paths_not_shown_once() {
        // The old substring match also removed the second uupd entry.
        let order = list(&["/p/libuupd.so"]);
        let previous = list(&[
            "/home/u/p/libuupd.so",
            "/p/libuupd.so",
            "/home/u/p/libuupd.so",
        ]);
        assert_eq!(
            enabled_in_order(&order, |_| false, &previous),
            list(&["/home/u/p/libuupd.so"])
        );
    }

    #[test]
    fn enabled_in_order_drops_duplicates_left_by_older_versions() {
        let order = list(&["/p/libbrew.so", "/p/libflatpak.so"]);
        let previous = list(&["/p/libbrew.so", "/p/libflatpak.so", "/p/libbrew.so"]);
        assert_eq!(
            enabled_in_order(&order, |_| true, &previous),
            list(&["/p/libbrew.so", "/p/libflatpak.so"])
        );
    }

    #[test]
    fn move_item_down_lands_after_the_target() {
        let mut order = list(&["a", "b", "c", "d"]);
        assert!(move_item(&mut order, 0, 2));
        assert_eq!(order, list(&["b", "c", "a", "d"]));
    }

    #[test]
    fn move_item_up_lands_before_the_target() {
        let mut order = list(&["a", "b", "c", "d"]);
        assert!(move_item(&mut order, 3, 1));
        assert_eq!(order, list(&["a", "d", "b", "c"]));
    }

    #[test]
    fn move_item_rejects_out_of_range_and_no_op_moves() {
        let mut order = list(&["a", "b"]);
        assert!(!move_item(&mut order, 1, 1));
        assert!(!move_item(&mut order, 0, 2));
        assert!(!move_item(&mut order, 2, 0));
        assert_eq!(order, list(&["a", "b"]));
    }
}
