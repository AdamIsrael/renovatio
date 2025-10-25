use super::config;

use adw::prelude::*;
use gtk::{Box, Button, CheckButton, ProgressBar};

pub fn get_apply_check_button() -> CheckButton {
    let settings = gio::Settings::new(config::APP_ID);
    let reboot = settings.boolean("auto-reboot");

    let cb = CheckButton::builder()
        .label("Auto-reboot if the OS image is updated?")
        .margin_start(12)
        .margin_end(12)
        .active(reboot)
        .build();

    cb.connect_toggled(|button| {
        // Update the model when the button is toggled
        let settings = gio::Settings::new(config::APP_ID);
        if let Err(err) = settings.set_boolean("auto-reboot", button.is_active()) {
            eprintln!("Failed to set auto-reboot setting: {}", err);
        }
    });

    cb
}

pub fn get_plugin_progress_bar() -> ProgressBar {
    ProgressBar::builder()
        .margin_top(12)
        .margin_bottom(3)
        .margin_start(12)
        .margin_end(12)
        .pulse_step(0.0)
        // .pulse_step(25.0)
        .show_text(true)
        .text("")
        .visible(false)
        .build()
}

pub fn get_total_progress_bar() -> ProgressBar {
    ProgressBar::builder()
        .margin_top(12)
        .margin_bottom(6)
        .margin_start(12)
        .margin_end(12)
        .show_text(true)
        .text("")
        .visible(false)
        .build()
}

pub fn get_update_button() -> Button {
    Button::builder()
        .label("Update")
        .margin_top(12)
        .margin_bottom(6)
        .margin_start(12)
        .margin_end(12)
        .build()
}

pub fn get_header_bar() -> adw::HeaderBar {
    let header_bar = adw::HeaderBar::new();
    let window_title = adw::WindowTitle::builder().title("Renovatio").build();

    let main_menu = gio::Menu::new();
    main_menu.append(Some("About"), Some("app.about"));
    main_menu.append(Some("Preferences"), Some("app.preferences"));
    main_menu.append(Some("Quit"), Some("app.quit"));

    let mb = gtk::MenuButton::new();
    mb.set_icon_name("open-menu-symbolic");
    mb.set_menu_model(Some(&main_menu));
    header_bar.pack_end(&mb);

    header_bar.set_title_widget(Some(&window_title));
    header_bar
}

pub fn get_main_container(
    header_bar: &adw::HeaderBar,
    update_button: &Button,
    apply_check_button: &CheckButton,
    plugin_progress_bar: &ProgressBar,
    total_progress_bar: &ProgressBar,
) -> Box {
    // Create main container
    let parent = Box::new(gtk::Orientation::Vertical, 6);

    parent.append(header_bar);

    let main_box = Box::new(gtk::Orientation::Vertical, 6);

    main_box.append(update_button);
    main_box.append(apply_check_button);
    main_box.append(plugin_progress_bar);
    main_box.append(total_progress_bar);

    let clamp = adw::Clamp::builder()
        .child(&main_box)
        .maximum_size(500)
        .tightening_threshold(10)
        // .unit(adw::ClampUnit::Pixels::new(10))
        .build();
    parent.append(&clamp);
    parent
}
pub fn get_window(app: &adw::Application, title: &str, main_box: Box) -> adw::ApplicationWindow {
    // Create window
    adw::ApplicationWindow::builder()
        .application(app)
        .title(title)
        .resizable(false)
        .width_request(300)
        .content(&main_box)
        .build()
}
