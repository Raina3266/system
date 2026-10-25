# Clipboard panel

`clipboard-panel toggle [monitor]` opens a single AGS/Astal GTK4 history and
preview/editor window. The additional Waybar button is independent of the
original Rofi clipboard button and `Mod+Alt+V`; neither old tool is replaced.

## Ownership

- `astal/`: presentation, controller, native GDK clipboard publication, GTK text
  editing/image previews, and native Gio directory monitoring.
- `rust-custom/`: persisted history, memo lifecycle, pin/delete/edit transactions,
  deduplication, pruning, retention, and MIME/file-reference payload policy.
  It does not own a display, publish a clipboard selection, or invoke Rofi.
- The existing `rofi-clipboard-collector.service` remains the **only** background
  Wayland collector. GTK's focus-scoped clipboard API is not a global data-control
  watcher, so the existing `wl-paste --watch` integration is deliberately retained.

The adapted Rust policy uses the original version-1 history format, data directory,
`history.lock` flock, image cache, digest and limits (2,000 entries / 100 images).
`ROFI_CLIPBOARD_DATA_DIR` overrides the shared location for both tools; otherwise
it is `$XDG_DATA_HOME/rofi-clipboard`. There is no migration or second database.
Any future format change must account for both writers. The original source files
remain untouched, as requested.

Saves compare the original buffer with the stored item while holding the lock.
An external edit or deletion blocks switching/closing/copying and keeps the local
draft visible. Reload explicitly discards that draft after confirmation. Delete
is also an explicit discard action. Changes save on selection/tab changes or close;
searching alone does not switch or save the editor.

The resident application retains clipboard ownership when hidden; a service
restart may lose its selection unless another clipboard manager has retained it.
No clipboard text or image payload is logged.

## Controls

- `Escape`: save and close; outside click and the header close button do the same.
- `Enter`: copy and close outside the text editor; `Ctrl+Enter` works in the editor.
- `Ctrl+S`: save; `Ctrl+N`: new memo; `Ctrl+F`: focus search.
- `Alt+P`: pin; `Alt+D`: delete; `Alt+E`: focus the editor.
- Up/Down outside the editor selects history entries.

## Validation

```sh
cargo test --manifest-path scripts/Cargo.toml --locked -p clipboard-custom
node --test scripts/clipboard/astal/test.js
```

Real UI and launcher checks:

```sh
node scripts/clipboard/astal/test.js --ui \
  --runtime /nix/store/...-ags-2.3.0 \
  --helper /nix/store/...-clipboard-custom-0.1.0/bin/clipboard-custom \
  --palette /nix/store/...-clipboard-panel-palette.json \
  --panel /nix/store/...-clipboard-panel
```

These paths are the package's `ags`, `clipboardCustom`, `palette`, and panel
outputs. The runner needs `Xvfb`, `dbus-daemon`, `niri`, and `wl-paste` on PATH.
It creates nested Niri on private Xvfb, a service-free D-Bus session, and temporary
history, then tears them down. It never uses the live desktop's clipboard/history.
It checks real imports and CSS, editor saves, file URI policy, image previews,
cross-client PNG clipboard bytes after hiding, and the packaged launcher/IPC.
`--screenshot /absolute/path.png` optionally captures the fixture panel.

Runtime launchers live in `niri/panels/clipboard.nix`; Home Manager registration
and the `ags-clipboard` service live in `niri/panels/default.nix`.
