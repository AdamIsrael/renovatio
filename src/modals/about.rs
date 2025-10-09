use adw::prelude::AdwDialogExt;
use gtk::prelude::GtkApplicationExt;
use relm4::{ComponentParts, ComponentSender, SimpleComponent, adw, gtk};

use crate::config::VERSION;

pub struct AboutDialog {}

impl SimpleComponent for AboutDialog {
    type Init = ();
    type Widgets = adw::AboutDialog;
    type Input = ();
    type Output = ();
    type Root = adw::AboutDialog;

    fn init_root() -> Self::Root {
        adw::AboutDialog::builder()
            // .application_icon(APP_ID)
            // Insert your license of choice here
            .license_type(gtk::License::MitX11)
            // Insert your website here
            .website("https://github.com/AdamIsrael/ublue-updater/")
            // Insert your Issues page
            .issue_url("https://github.com/AdamIsrael/ublue-updater/issues")
            // Insert your application name here
            .application_name("Ublue Updater")
            .version(VERSION)
            // .translator_credits("translator-credits")
            .copyright("© 2025 Adam Israel")
            .developers(vec!["Adam Israel"])
            // .designers(vec!["Bilal Elmoussaoui"])
            .build()
    }

    fn init(
        _: Self::Init,
        root: Self::Root,
        _sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let model = Self {};

        let widgets = root.clone();
        widgets.present(Some(&relm4::main_application().windows()[0]));

        ComponentParts { model, widgets }
    }

    fn update_view(&self, _dialog: &mut Self::Widgets, _sender: ComponentSender<Self>) {}
}
