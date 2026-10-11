use inline_xml::xml;
use std::process::{Command, Stdio};

static PLUGIN_DIRS: &[&str] = &[
    "/usr/lib/renovatio/plugins",
    "/usr/local/lib/renovatio/plugins",
    "~/.local/lib/renovatio/plugins",
];

/// Find plugin libraries (`*.so`) in the standard plugin directories.
pub fn find_plugins() -> Vec<String> {
    let dirs: Vec<String> = PLUGIN_DIRS
        .iter()
        .map(|dir| shellexpand::tilde(dir).to_string())
        .collect();
    find_plugins_in(&dirs)
}

/// Find `*.so` files in `dirs`, sorted within each directory. Missing
/// directories are skipped; unreadable directories and entries, and names
/// that aren't valid UTF-8 (plugin paths are stored as strings), are logged
/// and skipped rather than stopping the app from starting.
fn find_plugins_in(dirs: &[String]) -> Vec<String> {
    let mut plugins = Vec::new();

    for dir in dirs {
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                eprintln!("Skipping plugin directory {dir}: {error}");
                continue;
            }
        };

        let mut found = Vec::new();
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    eprintln!("Skipping an entry in {dir}: {error}");
                    continue;
                }
            };
            let path = entry.path();
            if path.extension().is_none_or(|ext| ext != "so") || !path.is_file() {
                continue;
            }
            match path.to_str() {
                Some(path) => found.push(path.to_string()),
                None => eprintln!("Skipping plugin with a non-UTF-8 path: {}", path.display()),
            }
        }

        found.sort();
        plugins.extend(found);
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

#[cfg(test)]
mod tests {
    use super::*;

    use std::fs;
    use std::path::PathBuf;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("renovatio-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn find_plugins_in_returns_sorted_so_files_only() {
        let dir = temp_dir("find");
        for name in ["libzeta.so", "libalpha.so", "README.md", "libbeta.so.bak"] {
            fs::write(dir.join(name), "").unwrap();
        }
        fs::create_dir(dir.join("subdir.so")).unwrap();

        let dirs = vec![dir.to_str().unwrap().to_string()];
        let found = find_plugins_in(&dirs);
        let names: Vec<&str> = found
            .iter()
            .map(|p| p.rsplit('/').next().unwrap())
            .collect();
        assert_eq!(names, vec!["libalpha.so", "libzeta.so"]);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn find_plugins_in_skips_missing_and_unreadable_dirs() {
        let dir = temp_dir("skip");
        let file = dir.join("not-a-dir");
        fs::write(&file, "").unwrap();
        fs::write(dir.join("libok.so"), "").unwrap();

        let dirs = vec![
            "/nonexistent/renovatio/plugins".to_string(),
            file.to_str().unwrap().to_string(),
            dir.to_str().unwrap().to_string(),
        ];
        assert_eq!(find_plugins_in(&dirs).len(), 1);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn find_plugins_in_skips_non_utf8_names() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let dir = temp_dir("utf8");
        fs::write(dir.join(OsStr::from_bytes(b"lib\xff.so")), "").unwrap();
        fs::write(dir.join("libok.so"), "").unwrap();

        let dirs = vec![dir.to_str().unwrap().to_string()];
        let found = find_plugins_in(&dirs);
        assert_eq!(found.len(), 1);
        assert!(found[0].ends_with("libok.so"));

        fs::remove_dir_all(&dir).unwrap();
    }
}
