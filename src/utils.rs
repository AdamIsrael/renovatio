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

const SCHEMA_FILE: &str = "com.github.AdamIsrael.Renovatio.gschema.xml";

/// The schema, as installed by Meson (`data/`). Embedded so a plain
/// `cargo run` works too.
const SCHEMA: &str = include_str!("../data/com.github.AdamIsrael.Renovatio.gschema.xml");

/// Make sure GSettings can find our schema, and that a copy in
/// `~/.local/share/glib-2.0/schemas` matches this build.
///
/// Packaged installs get the schema from Meson. Otherwise (e.g. `cargo run`)
/// it's written to the user's data dir and compiled. A user copy left by an
/// older build is replaced, so new keys reach existing installs: it takes
/// precedence over a system-wide copy, so it must not go stale.
///
/// Must run before anything uses GSettings, which caches the schemas it finds.
pub fn install_gsettings_schema() {
    let dir = glib::user_data_dir().join("glib-2.0").join("schemas");
    let path = dir.join(SCHEMA_FILE);
    let user_copy = std::fs::read_to_string(&path).ok();
    let installed_elsewhere = glib::system_data_dirs().iter().any(|data_dir| {
        data_dir
            .join("glib-2.0/schemas")
            .join(SCHEMA_FILE)
            .is_file()
    });

    if !needs_user_schema(user_copy.as_deref(), installed_elsewhere) {
        return;
    }

    println!("Installing schema to {}...", dir.display());
    if let Err(error) = std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&path, SCHEMA)) {
        eprintln!("Failed to install schema {}: {error}", path.display());
        return;
    }
    match Command::new("glib-compile-schemas").arg(&dir).output() {
        Ok(output) if output.status.success() => {}
        Ok(output) => eprintln!(
            "glib-compile-schemas failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ),
        Err(error) => eprintln!("Failed to run glib-compile-schemas: {error}"),
    }
}

/// Whether the user copy of the schema must be (re)written: it's missing and
/// no system-wide copy exists, or it differs from this build's schema.
fn needs_user_schema(user_copy: Option<&str>, installed_elsewhere: bool) -> bool {
    match user_copy {
        Some(content) => content != SCHEMA,
        None => !installed_elsewhere,
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
    fn user_schema_is_written_only_when_missing_everywhere_or_stale() {
        assert!(needs_user_schema(None, false));
        assert!(!needs_user_schema(None, true));
        assert!(!needs_user_schema(Some(SCHEMA), false));
        assert!(!needs_user_schema(Some(SCHEMA), true));
        // An older build's copy shadows a system-wide one, so replace it either way.
        assert!(needs_user_schema(Some("<schemalist/>"), false));
        assert!(needs_user_schema(Some("<schemalist/>"), true));
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
