// An icon in the notification area, with a menu to open the window, open
// Preferences, or quit.
//
// tray-icon is built with only its ksni backend (StatusNotifierItem over
// D-Bus): the default libappindicator backend pulls in GTK3, which can't share
// a process with GTK4. ksni runs on its own thread, so its events are sent to
// the GTK main thread over a channel.
use adw::prelude::*;
use std::cell::Cell;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, TrayIcon, TrayIconBuilder, TrayIconEvent};

const ICON: &[u8] = include_bytes!("../data/icons/renovatio-tray.png");

/// The bus name of the service that shows StatusNotifierItem icons. Without
/// it (e.g. GNOME without the AppIndicator extension), there's no tray icon.
const WATCHER: &str = "org.kde.StatusNotifierWatcher";

const OPEN: &str = "open";
const PREFERENCES: &str = "preferences";
const QUIT: &str = "quit";

/// Add the tray icon. Keep the returned `TrayIcon` alive: dropping it removes
/// the icon.
///
/// While a tray is shown, closing the window hides it instead of quitting, so
/// it can be reopened from the tray. Without one, closing quits as before.
pub fn install(app: &adw::Application, window: &adw::ApplicationWindow) -> Option<TrayIcon> {
    let tray = match build() {
        Ok(tray) => tray,
        Err(err) => {
            eprintln!("Couldn't add the tray icon: {err}");
            return None;
        }
    };

    let (tx, rx) = flume::unbounded::<&'static str>();

    let menu_tx = tx.clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        for command in [OPEN, PREFERENCES, QUIT] {
            if event.id == command {
                let _ = menu_tx.send(command);
            }
        }
    }));
    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
        if let TrayIconEvent::Click {
            button: MouseButton::Left,
            ..
        } = event
        {
            let _ = tx.send(OPEN);
        }
    }));

    // Handle the tray's commands on the main thread.
    glib::spawn_future_local(glib::clone!(
        #[weak]
        app,
        #[weak]
        window,
        async move {
            while let Ok(command) = rx.recv_async().await {
                match command {
                    OPEN => window.present(),
                    PREFERENCES => {
                        // The dialog is shown over the window, so show that first.
                        window.present();
                        app.activate_action("preferences", None);
                    }
                    QUIT => app.activate_action("quit", None),
                    _ => {}
                }
            }
        }
    ));

    // Hide on close only while something can show the tray icon.
    let watch = Cell::new(Some(gio::bus_watch_name(
        gio::BusType::Session,
        WATCHER,
        gio::BusNameWatcherFlags::NONE,
        glib::clone!(
            #[weak]
            window,
            move |_, _, _| window.set_hide_on_close(true)
        ),
        glib::clone!(
            #[weak]
            window,
            move |_, _| window.set_hide_on_close(false)
        ),
    )));
    window.connect_destroy(move |_| {
        if let Some(watch) = watch.take() {
            gio::bus_unwatch_name(watch);
        }
    });

    Some(tray)
}

fn build() -> Result<TrayIcon, Box<dyn std::error::Error>> {
    let menu = Menu::with_items(&[
        &MenuItem::with_id(OPEN, "Open Renovatio", true, None),
        &MenuItem::with_id(PREFERENCES, "Preferences…", true, None),
        &PredefinedMenuItem::separator(),
        &MenuItem::with_id(QUIT, "Exit", true, None),
    ])?;

    Ok(TrayIconBuilder::new()
        .with_id("renovatio")
        .with_title("Renovatio")
        .with_tooltip("Renovatio")
        .with_icon(load_icon()?)
        .with_menu(Box::new(menu))
        .build()?)
}

/// Decode the embedded PNG into the RGBA pixels tray-icon expects.
fn load_icon() -> Result<Icon, Box<dyn std::error::Error>> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(ICON));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info()?;
    let mut buf = vec![0; reader.output_buffer_size().ok_or("icon too large")?];
    let info = reader.next_frame(&mut buf)?;
    buf.truncate(info.buffer_size());

    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|&[r, g, b]| [r, g, b, 0xff])
            .collect(),
        other => return Err(format!("unsupported icon color type {other:?}").into()),
    };

    Ok(Icon::from_rgba(rgba, info.width, info.height)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_icon_decodes() {
        assert!(load_icon().is_ok());
    }
}
