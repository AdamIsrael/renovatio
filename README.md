# Renovatio

Prototype gtk4 application to update the OS, flatpak, brew, etc, show a progress bar based on the parsed terminal output (from `uupd --json`), and possibly (stretch goal) display the changelog(s).

- re changelogs: @AdamIsrael it'd be just snagging/caching the release notes from github releases.

## Screenshots

<p align="center">
  <img src="docs/screenshots/main-window.png" width="392" alt="The main window during an update: the brew plugin's progress bar shows &quot;Getting outdated packages...&quot; and the overall bar shows &quot;Updating brew (1/4)...&quot;">
</p>

Each plugin's progress is shown in the upper bar, and the overall run in the lower one.

<p align="center">
  <img src="docs/screenshots/preferences.png" width="690" alt="The Preferences dialog listing the plugins brew, flatpak, distrobox, bootc, rpm-ostree, rustup and uupd, each with a drag handle, a switch, and a menu; conflicting plugins are noted under each one">
</p>

In Preferences, switch plugins on or off and drag them into the order they should run in. Plugins that update the same things (e.g. uupd and flatpak) can't be enabled together.

## Installing

Download the latest `renovatio-vX.Y.Z-x86_64-linux.tar.gz` and `SHA256SUMS` from [Releases](https://github.com/AdamIsrael/renovatio/releases), then:

```bash
sha256sum -c SHA256SUMS
tar -xzf renovatio-vX.Y.Z-x86_64-linux.tar.gz -C ~/.local
~/.local/bin/renovatio
```

Then enable plugins in Preferences. Requires x86_64 Linux with glibc 2.39+, GTK 4.18+ and libadwaita 1.7+ (e.g. Fedora 42 or later). The plugins only work with the app from the same release, so replace them all when upgrading.

Renovatio adds an icon to the notification area; closing the window then keeps it running there (reopen it from the icon or by launching it again). This needs a desktop that shows StatusNotifierItem icons: KDE and most others do, GNOME needs the AppIndicator extension. Without one, closing the window quits.

## Building

To build from source instead:

### Prerequisites

Dependencies are managed via devcontainer.

- Install DevPod: https://devpod.io/docs/installation

### Setup

```bash
git clone https://github.com/adamisrael/renovatio.git
devpod up .
ssh renovatio.devpod

# Build the application and the plugins
cargo build --workspace --release

# Install the plugins
cd plugins
just install-release

# Run the application
cd ..
target/release/renovatio
```


## TODO

- Investigate feasibility of adding a changelog display feature.
- Add support for `gettext`
- Re-add terminal output for stdout/stderr.
- Build flatpak(s)
