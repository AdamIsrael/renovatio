# Plugins

- [x] bootc
  - [x] Fix the Progress Bar when complete
- [x] brew
- [x] distrobox
  - [x] parse distrobox names
  - [x] Check/fix ProgressBar progress
- [x] flatpak
  - [x] implement everything
- [x] rpm-tree
- [x] rustup (disabled by default; runs `rustup update` as the user)
- [x] uupd

## Test plugin (development only)

`test-plugin` misbehaves on demand so you can exercise the UI's failure paths. It is not installed by `just install-release`.

```bash
cargo build --workspace --release   # inside the devcontainer
just install-test-plugin         # then enable "test-plugin" in Preferences
RENOVATIO_TEST_PLUGIN=panic ../target/release/renovatio
just uninstall-test-plugin       # disable it in Preferences first
```

Modes (`RENOVATIO_TEST_PLUGIN`, read on every run):

- `fail` (default): reports some progress, then fails without reaching 100%
- `panic`: panics mid-update; `PanicGuard` should report "test-plugin crashed: ..."
- `slow`: pulses for about 5 seconds, then succeeds
- `succeed`: reports 100% twice, then succeeds

