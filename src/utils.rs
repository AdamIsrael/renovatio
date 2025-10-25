use inline_xml::xml;
use std::process::{Command, Stdio};

static PLUGIN_DIRS: &[&str] = &[
    "/usr/lib/renovatio/plugins",
    "/usr/local/lib/renovatio/plugins",
    "~/.local/lib/renovatio/plugins",
];

pub fn find_plugins() -> Vec<String> {
    let mut plugins = Vec::<String>::new();

    for dir in PLUGIN_DIRS {
        let mut path = dir.to_string();

        if path.starts_with("~") {
            path = shellexpand::tilde(&path).to_string();
        }

        // Scan the files in each directory for .so files
        if std::fs::exists(&path).unwrap_or(false) {
            for entry in std::fs::read_dir(&path).unwrap() {
                let entry = entry.unwrap();
                if entry.file_type().unwrap().is_file()
                    && entry.file_name().to_str().unwrap().ends_with(".so")
                {
                    let plugin = format!("{}/{}", path, entry.file_name().to_str().unwrap());

                    plugins.push(plugin);
                }
            }
        }
    }
    plugins
}

/// Installs our GSettings schema, if they're not already installed.
pub fn install_gsettings_schema() {
    let xml = xml! {
        <schemalist>
            <schema id="com.github.AdamIsrael.Renovatio" path="/com/github/AdamIsrael/Renovatio/">
            <key name="auto-reboot" type="b">
                <default>false</default>
                <summary>A flag to enable automatic reboot after update</summary>
            </key>

            <key name="plugins" type="as">
              <default>{"[]"}</default>
              <summary></summary>
              <description></description>
            </key>

            </schema>
        </schemalist>
    };

    // write the xml to ~/.local/share/glib-2.0/schemas
    let glib_schemas_dir = format!(
        "{}/.local/share/glib-2.0/schemas",
        std::env::var("HOME").unwrap()
    );
    std::fs::create_dir_all(&glib_schemas_dir).unwrap();

    let xml_path = format!(
        "{}/{}",
        glib_schemas_dir, "com.github.AdamIsrael.Renovatio.gschema.xml"
    );

    if !std::fs::exists(&xml_path).unwrap_or(false) {
        println!("Installing schema...");
        std::fs::write(&xml_path, xml.to_string()).unwrap();

        // glib-compile-schemas
        println!("Compiling schema...");
        let _ = std::process::Command::new("glib-compile-schemas")
            .arg(&glib_schemas_dir)
            .output()
            .unwrap();
    }
}

/// Checks rpm-ostree status for pending updates.
pub fn check_reboot_needed() -> bool {
    let cmd = "rpm-ostree status --pending-exit-77";
    let rc = Command::new("sh")
        .args(["-c", cmd])
        // pipe stdout to /dev/null for now. Otherwise it goes to this app's stdout
        .stdout(Stdio::null())
        .status()
        .expect("Failed to execute command");
    rc.code() == Some(77)
}

/// Reboots the system.
pub fn reboot_system() -> bool {
    let cmd = "systemctl reboot";
    let rc = Command::new("sh")
        .args(["-c", cmd])
        .status()
        .expect("Failed to reboot");
    rc.code() == Some(0)
}
