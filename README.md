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

## Building

Until the packaging is complete, you'll have to build the application from source.

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
