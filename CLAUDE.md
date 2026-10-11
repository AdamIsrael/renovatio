# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Overview

Renovatio is a prototype GTK4/libadwaita (Rust, edition 2024) desktop app that runs system updates (OS via bootc/rpm-ostree/uupd, flatpak, brew, distrobox, Rust toolchains via rustup) through dynamically loaded plugins and shows per-plugin and overall progress bars.

## Commands

The repo is **one Cargo workspace**: the root package is the app (and the `renovatio` lib), and every `plugins/<name>/` crate is a member, sharing one `Cargo.lock` and `target/` so dependency versions can't drift across the plugin boundary. Plain `cargo build`/`cargo test` at the root covers only the app; add `--workspace` (or `-p <plugin>`) for plugins. Plugin `.so` files land in `target/<profile>/`; the install recipes live in `plugins/justfile`.

Building requires GTK4 + libadwaita dev libraries (`libgtk-4-dev libadwaita-1-dev`), which are **not** available on the host. The host is read-only outside select mounts (e.g. `/tmp`, `/var/home`), so don't try to install them there. All cargo commands run inside the devpod workspace (devcontainer spec, `.devcontainer/`), where the repo is mounted at `/workspaces/renovatio`:

```bash
devpod status renovatio                 # check whether the workspace is running
devpod up .                             # create/start it (from the repo root)
devpod ssh renovatio --workdir /workspaces/renovatio --command "cargo clippy"
```

```bash
cargo build --workspace [--release]   # app + all plugins
cargo run                             # the app
cargo test --workspace                # single test: cargo test -p <crate> <name>
cargo clippy --workspace --all-targets -- -D warnings   # CI enforces this
cargo fmt --all -- --check            # CI enforces this
cargo build -p flatpak                # a single plugin

# Installing plugins (from plugins/)
just install-release            # copies ../target/release/*.so (except the test plugin) to ~/.local/lib/renovatio/plugins/
just install-test-plugin        # dev-only plugin; RENOVATIO_TEST_PLUGIN=fail|panic|slow|succeed (see plugins/README.md)
```

CI (`.github/workflows/ci.yaml`) builds the root `Dockerfile` (GTK/libadwaita dev libs) and runs `cargo check --workspace --all-targets`, `cargo test --workspace`, and `cargo clippy --workspace --all-targets -- -D warnings` inside it, plus `cargo fmt --all -- --check`. It covers the app and every plugin, and any clippy warning fails the build.

A Meson build (`meson.build`, `src/meson.build`, root `justfile`: `just setup-meson`, `just build`) wraps `cargo build` for eventual packaging; plain cargo is the normal dev loop.

## Architecture

**Plugin ABI.** `src/lib.rs` is the `renovatio` library crate shared by the app and all plugins. It defines the `Plugin` trait, `PluginProgress` (status messages sent from plugin to UI), `PluginMetadata`, and the `execute()` shell helper returning `(stdout, stderr, exit_code)`. Each plugin in `plugins/<name>/` is a `cdylib` depending on `renovatio = { path = "../.." }` and must export:

```rust
renovatio::export_plugin!(MyPlugin);   // generates create_plugin() and renovatio_abi()
```

The host loads this via `libloading` (`runner::load_plugin`) and reclaims it with `Box::from_raw`. This is a Rust-ABI (not C-ABI) boundary, so the app and plugins must be built with the same compiler and the same `renovatio` lib version — changing `Plugin`/`PluginProgress` requires rebuilding and reinstalling every plugin. Each `.so` links its own std, so **the host cannot catch a plugin panic** (the process aborts with "Rust cannot catch foreign exceptions"). `export_plugin!` wraps the plugin in `PanicGuard`, whose `catch_unwind` is compiled into the plugin and turns a panic into a failed update — always export via the macro. The macro also exports `renovatio_abi()` (C ABI), returning `ABI_FINGERPRINT`: renovatio version + `rustc -V` + a hash of `src/lib.rs`, computed in `build.rs`. `runner::load_plugin` rejects plugins whose fingerprint is missing or differs from the host's, before calling anything else in them, so any edit to `src/lib.rs` (even a comment) requires rebuilding and reinstalling the plugins. The Rust ABI is kept deliberately: plugins are built in this workspace alongside the app, and the fingerprint turns a mismatch into a clear error rather than a crash. Supporting plugins built separately (third-party) would need a C-ABI shim or `abi_stable` instead. `Plugin` requires `Send + Sync`.

**Plugin discovery and enablement.** `utils::find_plugins()` scans `/usr/lib/renovatio/plugins`, `/usr/local/lib/renovatio/plugins`, and `~/.local/lib/renovatio/plugins` for `.so` files at startup. `runner::discover` loads each one once and keeps it (and its `Library`) loaded for the life of the app, in `PluginEntry::plugin`; Preferences shows their metadata and runs reuse them via `runner::Plugins`, so after reinstalling plugins, restart the app. Enabled paths that weren't found at startup are loaded on demand for each run. Which plugins actually run is stored as a list of `.so` paths in the GSettings key `plugins` (schema `com.github.AdamIsrael.Renovatio`), toggled in `src/modals/preferences.rs`. The schema lives in `data/com.github.AdamIsrael.Renovatio.gschema.xml`; Meson installs it system-wide (and `gnome.post_install` compiles it). It is also embedded in the binary: at startup `utils::install_gsettings_schema()` writes it to `~/.local/share/glib-2.0/schemas` and compiles it if no copy is installed anywhere, or if the user copy differs from this build's, so new keys reach existing installs.

**Update flow (`src/main.rs`, `src/runner.rs`).** Clicking Update snapshots the GSettings plugin list and spawns a worker thread running `runner::run`, which takes each plugin (already loaded, see above) and calls its blocking `update(tx)` sequentially. Plugins send `PluginProgress` on a per-plugin `flume` channel; the runner forwards these as `RunEvent::Progress` and emits exactly one `Started` and one `Finished { success, error }` per configured path (including paths that fail to load), then a final `RunFinished`. `Finished` is sent only after `update()` returns and its progress is drained, so plugin completion never depends on `progress` values. The GTK main thread handles `RunEvent`s in a `glib::spawn_future_local` task awaiting `rx.recv_async()` (don't reintroduce `idle_add_local` + `try_recv`, which busy-loops); `pulse: true` starts a 100 ms `ui::Pulser` timer that runs only while the plugin is pulsing. `RunFinished { reboot_required }` is true if any plugin's progress set `PluginProgress::reboot_required`. If the "apply" (auto-reboot) check is set and either that flag or `rpm-ostree status --pending-exit-77` (the fallback for plugins that don't set it, e.g. rpm-ostree, uupd) says a reboot is needed, the app counts down and runs `systemctl reboot`, keeping the controls disabled; otherwise controls are re-enabled and, if the flag is set, the summary says "Reboot to finish." New plugins that stage OS changes should set `reboot_required`. `runner` tests use in-process fake plugins via `LoadedPlugin::from_plugin`.

**UI.** Widgets are built imperatively in `src/ui.rs` (no Blueprint/`.ui` files, no gresources despite the `glib-build-tools` build-dependency); app menu actions (about/preferences/quit) are wired in `src/actions.rs`; dialogs live in `src/modals/`.

**Conflicts.** `Plugin::conflicts(name)` says which plugins update the same things (e.g. uupd also updates the OS, flatpak, brew and distrobox). At startup `runner::discover` loads the installed plugins and pairs up conflicts in both directions (either side declaring it is enough) into `PluginEntry::conflicts`. Enabling a plugin in Preferences switches off any enabled plugin it conflicts with and shows a toast. The runner doesn't check conflicts, so settings saved before this existed may still list both.

**Notes.** stdout/stderr from plugins is currently only printed to the console (displaying it in a `TextView` is a TODO).


<!-- BEGIN BEADS INTEGRATION v:1 profile:minimal hash:1105d646 -->
## Beads Issue Tracker

This project uses **bd (beads)** for issue tracking. Run `bd prime` to see full workflow context and commands.

### Quick Reference

```bash
bd ready              # Find available work
bd show <id>          # View issue details
bd update <id> --claim  # Claim work
bd close <id>         # Complete work
```

### Rules

- Use `bd` for ALL task tracking — do NOT use TodoWrite, TaskCreate, or markdown TODO lists
- Run `bd prime` for detailed command reference and session close protocol
- Use `bd remember` for persistent knowledge — do NOT use MEMORY.md files

**Architecture in one line:** issues live in a local Dolt DB; sync uses `refs/dolt/data` on your git remote; `.beads/issues.jsonl` is a passive export. See https://github.com/gastownhall/beads/blob/main/docs/core-concepts/sync-concepts.md for details and anti-patterns.

## Agent Context Profiles

The managed Beads block is task-tracking guidance, not permission to override repository, user, or orchestrator instructions.

- **Conservative (default)**: Use `bd` for task tracking. Do not run git commits, git pushes, or Dolt remote sync unless explicitly asked. At handoff, report changed files, validation, and suggested next commands.
- **Minimal**: Keep tool instruction files as pointers to `bd prime`; use the same conservative git policy unless active instructions say otherwise.
- **Team-maintainer**: Only when the repository explicitly opts in, agents may close beads, run quality gates, commit, and push as part of session close. A current "do not commit" or "do not push" instruction still wins.

## Session Completion

This protocol applies when ending a Beads implementation workflow. It is subordinate to explicit user, repository, and orchestrator instructions.

1. **File issues for remaining work** - Create beads for anything that needs follow-up
2. **Run quality gates** (if code changed) - Tests, linters, builds
3. **Update issue status** - Close finished work, update in-progress items
4. **Handle git/sync by active profile**:
   ```bash
   # Conservative/minimal/default: report status and proposed commands; wait for approval.
   git status

   # Team-maintainer opt-in only, unless current instructions forbid it:
   git pull --rebase
   git push
   git status
   ```
5. **Hand off** - Summarize changes, validation, issue status, and any blocked sync/commit/push step

**Critical rules:**
- Explicit user or orchestrator instructions override this Beads block.
- Do not commit or push without clear authority from the active profile or the current user request.
- If a required sync or push is blocked, stop and report the exact command and error.
<!-- END BEADS INTEGRATION -->
