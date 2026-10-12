## Install

Download `renovatio-TAG-x86_64-linux.tar.gz`, check it against `SHA256SUMS`, and extract it into `~/.local` (or `/usr/local` with `sudo`):

```bash
sha256sum -c SHA256SUMS
tar -xzf renovatio-TAG-x86_64-linux.tar.gz -C ~/.local
~/.local/bin/renovatio
```

This installs `bin/renovatio` and the plugins in `lib/renovatio/plugins/`, where the app finds them. Then enable plugins in Preferences.

**Requirements:** x86_64 Linux with glibc 2.39+, GTK 4.18+ and libadwaita 1.7+ (e.g. Fedora 42 or later, Debian 13). The binaries link against the system GTK/libadwaita.

**Use these plugins only with this app.** The app and plugins are built together and must match exactly (same version and compiler); the app rejects plugins from another release or a local build. Replace all of them when upgrading, and remove any older copies from the other plugin directories (`~/.local/lib/renovatio/plugins`, `/usr/local/lib/renovatio/plugins`, `/usr/lib/renovatio/plugins`).
