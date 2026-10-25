# Custom Scripts

This repository contains seven Rust packages used by the desktop configuration:

- `audio-control` — headless audio compatibility policy, request-local PIN/passkey pairing, and HID++ battery publication
- `media-control` — headless media grouping, conservative stream matching, artwork, and video-author metadata
- `network-manager` — QR-only Wi-Fi share-code encoding; native network information, secrets, and WEP settings stay in GJS/libnm
- `ocr-screenshot` — screenshot OCR and clipboard integration
- [`clipboard-custom`](#clipboard-panel) — clipboard capture, history, and memo policy for the GTK4 panel
- [`waybar-timer`](#waybar-timer) — interactive Waybar countdown timer
- `webcam-crop` — on-demand virtual webcam cropper and supervisor

The Niri configuration uses **AGS 2.3/Astal GTK4 for media, audio, dashboard,
notifications, network, clipboard, and the app launcher**. The panel commands are
Nix-packaged launchers, not additional Rust GUIs. The dashboard is TypeScript-only, with its application,
metrics, notifications, styles, and tests directly in `scripts/dashboard/`.

## Live desktop editing

After a Home Manager/NixOS rebuild and switch, the configuration and style files
listed below are linked from or read directly from `/home/raina/System`. Panel
CSS is read directly from the checkout; application code is packaged in the Nix
store, not live-linked:

| What to edit | When it takes effect |
| --- | --- |
| `niri/config.kdl` | Niri reloads it on save |
| `scripts/{dashboard,app-launcher}/style.css` | Running panels hot-reload CSS; no restart or rebuild |
| `scripts/{audio-control,media-control,network-manager,clipboard}/astal/style.css` | Running panels hot-reload CSS; no restart or rebuild |
| `themes/waybar/top.jsonc`, `bottom.jsonc` | Waybar restarts automatically |
| `themes/waybar/waybar.css` | Waybar reloads CSS automatically |

Rust, JavaScript, TypeScript, packaged JSON, Nix module, package, service, and
kernel changes require a rebuild and switch. Restarting a panel or closing and
reopening it does not load edited checkout code. All six launchers enforce their
store-backed entry points; panel `*_CONFIG` variables are not live-source overrides.
Each Rust package now hashes only Cargo sources, so changing one UI stylesheet
or sibling utility no longer invalidates every desktop helper.

## audio-control

`scripts/audio-control/astal/` contains the combined AGS/Astal GTK4 Bluetooth/Audio
popup; its Rust helper lives in `scripts/audio-control/rust-custom/`. The Niri-only
`audio-control-ui` user service keeps its existing name, but now runs
`audio-panel run` as the separate AGS instance `audio-panel`.

The Waybar audio button uses `audio-panel status` for its JSON every five seconds.
Its glyph tracks the default output's volume/mute state. Bluetooth power and
connections are reflected in its CSS class and tooltip, alongside output/input
state. Middle-click toggles the default adapter through Astal Bluetooth.

Left-click opens the popup on the clicked monitor. Escape, another click on the
same Waybar button, or clicking outside dismisses it.

| Tab | Content |
| --- | --- |
| Bluetooth | Bluetooth power, automatic discovery, pair, connect, disconnect, forget, and battery levels |
| Audio | Output controls in the upper half; input controls in the lower half, each with volume/mute and device/port selection |

Bluetooth lists remembered/connected devices above ready-to-pair devices, with a
horizontal divider when both groups are present. Only paired devices show Forget.
A powered adapter scans automatically while the popup is open on Bluetooth; there
is no Scan/Stop scan button. Closing the popup, switching to Audio, powering off,
removing/replacing the adapter, or disposing the UI releases the panel's discovery
session through Astal. This never powers the adapter on automatically or stops
another client's discovery session. Pairing shows one current status—pairing,
paired, or failed—using the device's name and address, not an accumulated helper
log. The small × dismisses the notice without cancelling the request; a pending
PIN/passkey prompt must first be answered or cancelled. Authentication prompts
remain visible when needed, without keeping old prompts or trailing blank lines.
The text-entry field and Send button appear only when a PIN, passkey, or yes/no
response is requested. Automatic pairing shows no input controls; codes to type
on a keyboard/device are displayed as instructions, without a panel entry field.

Audio and media volume use compact rectangular bars with percentages inside the
moving thumb and hollow triangle buttons for 1% steps. Colors follow the existing
palette; mute controls remain separate. There is no Play tab in audio; playback
stays in the existing media panel. Output volume runs from
**0 to 150%**, retaining amplification above 100%. Input remains 0–100%.
Choosing an output changes the system default and makes all currently running
playback streams follow it through Astal's native stream API. This overrides old
per-application device pins, so Chrome, Elisa, and other active players use the
selected output and follow later default changes. Capture/recording targets are
not changed. Playback is rerouted only after the output selection succeeds, not
during metadata refresh, volume/mute changes, or failed profile-switch recovery.
Input and output use Devices dropdowns that follow the native default and are
hidden unless an available alternative exists. Unplugged ports are not
reintroduced as generic endpoints when their associated routes are
unavailable; successful helper omissions are scoped to the exact native snapshot
identities, while newly discovered endpoints retain native fallback until resolved.
Volume/mute remain independent of that list and always target the live native
default. A missing/not-yet-ready default displays `—`, not a misleading `0%`.
Audio uses unmodified `pkgs.astal`, without framework patches or monkey-patches.
Astal WirePlumber owns audio inventory and state, volume, mute, default-device
selection, physical routes, and profiles. Astal Bluetooth owns native device
state, adapter power, discovery, trust, connect/disconnect, forget, and battery
readings. UI and Waybar status use these native services.

Only functionality Astal genuinely lacks uses the Rust helper:

- `audio-control policy` consumes canonical Astal snapshots from AGS. Bounded,
  read-only `pw-dump -N` supplements only omitted `EnumProfile`/`EnumRoute`
  associations and per-endpoint `Route` state, not a second device inventory.
  `EnumProfile`/`classes` parsing is ALSA-policy-only: Bluetooth profiles use a
  different schema and must not block either audio selector. Supplemental route
  associations and native endpoint fallback remain available for non-ALSA cards.
  Rust owns compact **Speaker**, **Headphones**, and USB labels, inactive ALSA
  profile choices, microphone-preserving profile retention, and transaction/rollback
  decisions. It returns typed native action requests; JavaScript validates live
  IDs/serials and executes **all forward and rollback** profile, route, and default
  changes through Astal, including route writes on multi-endpoint cards.
- Astal exposes pairing but not an agent API or its connection. Rust registers a
  request-local PIN/passkey agent and issues `Device1.Pair` on that same connection:
  this is the inseparable exception, not a parallel Bluetooth backend. AGS passes
  the explicitly selected native adapter and device address. Rust does **not**
  power, discover, trust, or connect; after pairing, AGS trusts and connects through
  Astal. Prompts appear in the Bluetooth tab; for numeric comparison, compare the
  six-digit passkey and answer `yes` or `no`. Closing the panel or leaving Bluetooth
  cancels pending pairing and stops discovery through Astal. The helper never
  requests BlueZ's session-wide default-agent role.
- The unchanged system `bt-battery-provider` service publishes kernel HID++
  battery levels through BlueZ `Battery1`; Astal reads those native values.
  The popup does not introduce a second battery provider.

There is no PulseAudio backend or retained legacy native-control implementation.

The JavaScript follows the media panel's structure: `frontend.js` owns GTK
widgets, the window, and styling; `backend.js` bridges native Astal services and
the helper; `logic.js` contains GTK-independent formatting and request/state
logic. Custom compatibility policy stays in Rust, not a parallel JavaScript
port/profile implementation.

### Commands

The helper's only operational commands are:

```text
audio-control policy
audio-control bluetooth-pair <hciN> <address>
audio-control battery-provider
```

`policy` is the panel's JSON worker over stdin/stdout, not a stand-alone
status/control CLI. `bluetooth-pair` requires an explicit adapter such as `hci0`
and an `AA:BB:CC:DD:EE:FF` address, not a device name. It pairs only, with prompts
on stderr and answers on stdin. `battery-provider` serves the existing system
`bt-battery-provider.service`.

**Breaking CLI cleanup:** `audio-control status`, `bluetooth-power`,
`bluetooth-devices`, `bluetooth-scan`, `devices`, and `set-default` have been
removed, with no compatibility aliases. Old manual invocations now fail as
unknown commands. Use `audio-panel status` or `audio-panel bluetoothPower` for
those launcher operations, and the popup for device/port selection and Bluetooth
device management.

The popup can also be controlled directly:

```sh
audio-panel run                 # Foreground instance; stop the resident UI first
audio-panel status
audio-panel toggle eDP-1        # Monitor is optional
audio-panel close
audio-panel bluetoothPower
```

The launcher validates command arity and constructs JSON with `jq --arg`.
Requests use `ags request --instance audio-panel -- <JSON>`:

```json
{"command":"toggle","monitor":"eDP-1"}
```

`command` is `status`, `toggle`, `close`, or `bluetoothPower`; only `toggle`
accepts an optional monitor. Startup bundles the immutable store entry with
`ags bundle --gtk 4` and executes the panel-specific output through `panel-gjs`,
not `ags run` or a `run --instance` flag. The frontend's own `AudioApplication`
subclasses native `Astal.Application`, sets `instance_name: "audio-panel"`, and uses `acquire_socket()`
and the native application lifecycle, independently of `media-panel`. Its public
`vfunc_request(request, connection)` extension point replies with
`AstalIO.write_sock` and closes the GIO connection after the async write completes,
including on errors. Repeated Waybar polling does not rely on garbage collection
to release sockets, and no framework implementation is patched.

**Installation prerequisite:** apply the normal NixOS/Home Manager generation
after changing the runtime, helper, service, or configuration links. A restart
alone cannot install those changes; let the generation manage the links and
unit rather than repairing them manually.

After the switch, the service starts with the graphical session and Waybar wants
it. Rebuild and switch to install Rust or JavaScript changes; CSS reloads in the
running panel without restarting it. For service/startup diagnostics:

```sh
systemctl --user status audio-control-ui.service
journalctl --user -u audio-control-ui.service -b -n 100 --no-pager
```

`niri/panels.nix` packages the audio JavaScript/JSON and shared style
helper into an immutable store source. CSS is read directly from the checkout,
without installing links under legacy panel config directories. The generated palette remains at
`~/.config/ags/audio-palette.json`. Palette changes retain their service restart
trigger on the next generation switch.

`AUDIO_PANEL_CONFIG` is forced to the packaged store entry, even if supplied in
the environment. `PANEL_CSS_FILE` defaults to
`/home/raina/System/scripts/audio-control/astal/style.css` and is hot-reloaded.
`AUDIO_PANEL_PALETTE` selects the JSON palette; `AUDIO_CONTROL_HELPER` is an
executable path, not a shell command. The service uses the launcher's enforced
store entry, supplies the installed palette link and packaged helper, and sets
`GDK_BACKEND=wayland`.
Graphical-session Wayland and D-Bus variables are inherited.
`niri/panels.nix` supplies GTK4, Graphene, GTK4 layer-shell,
Astal WirePlumber, and Astal Bluetooth through AGS's generated runtime wrapper.
Unlike the media artwork panel, audio needs no custom GdkPixbuf loader cache.
`scripts/packages.nix` wraps the packaged helper with `AUDIO_CONTROL_PW_DUMP`
pointing to the store-backed PipeWire `pw-dump` executable. For an unwrapped local
build, set that variable to an executable path or ensure `pw-dump` is on `PATH`.

### Development checks

From the repository root:

```sh
node --test scripts/audio-control/astal/tests/audio-control.test.js
cargo fmt --manifest-path scripts/Cargo.toml --package audio-control -- --check
cargo test --manifest-path scripts/Cargo.toml --package audio-control --locked
cargo clippy --manifest-path scripts/Cargo.toml --package audio-control --locked -- -D warnings
```

The consolidated GTK-independent JavaScript tests need Node.js but no npm dependencies or
GTK/GI; `npm test` from `scripts/audio-control/astal/` runs the same suite. They
exercise formatting, request/state logic, and native-service integration through
test doubles. Rust tests cover supplemental metadata associations, stable
identities, compact labels, profile retention, transaction/rollback decisions,
the JSON worker and removed CLI surface, explicit pairing targets, and HID++
battery publication. Media grouping and conservative stream matching are tested
separately in `scripts/media-control/rust-custom/` and `scripts/media-control/astal/`.

`scripts/audio-control/astal/tests/native-smoke.js` requires the audio package's
AGS runtime wrapper, the new policy-capable helper selected by
`AUDIO_CONTROL_HELPER`, and Node.js (already included by that runtime wrapper);
generic AGS or an old installed helper is not sufficient. It checks real GI
integration and bounded subprocess pipes, split UTF-8, interactive input, errors,
and timeouts. It reads native state but never changes devices, starts discovery,
or initiates pairing. These Node/GJS checks have no Python dependency.

The optional `scripts/audio-control/astal/tests/dropdown-smoke.js` checks real
GTK dropdown models, keyboard/pointer selection, popup dismissal and bounded
layout with a fake backend. It requires authenticated private Xvfb and a
service-free private D-Bus (both bus addresses isolated), the audio AGS wrapper,
`AUDIO_PANEL_CONFIG`/`AUDIO_PANEL_PALETTE`, and
`AUDIO_DROPDOWN_ISOLATED=1`. Python 3 and libX11 provide its isolated XTEST input.
Do not run this UI entry point against the desktop display or buses.

The consolidated `tests/audio-control.test.js` includes launcher coverage. The
built-launcher test uses an isolated fake AGS executable, without desktop IPC,
and is skipped unless the built executable is supplied:

```sh
AUDIO_PANEL_LAUNCHER=/path/to/audio-panel \
  node --test scripts/audio-control/astal/tests/audio-control.test.js
```

---

## Clipboard panel

`clipboard-panel toggle [monitor]` opens a single AGS/Astal GTK4 clipboard
history and preview/editor window. `Mod+Alt+V` invokes `clipboard-panel toggle`;
without a monitor argument it uses the focused monitor. Waybar uses the existing
`custom/clipboard-panel` button on the clicked monitor; the legacy
`custom/clipboard` module is removed.

### Interface

The compact panel is 450 logical pixels wide, with history above the preview.
History has room for exactly five complete rows; the preview uses the remaining
height. Both sections scroll independently. It starts with search and tabs,
without a title/header row, item-count footer, or bottom toolbar.

- **Memo** — editable notes and an empty **New memo** creation row at the bottom.
  Saving the draft turns it into a regular memo and keeps a fresh draft available.
- **Text** — captured text, including remote URLs.
- **Files** — copied local file references and captured images with previews.

Each opening returns to Memo and reloads history with no selected item and an
empty preview. Click an entry or use Up/Down to select one; background refreshes
do not select an item. There is no Reload button; close and reopen to reload.
Tabs follow the audio panel: inactive tabs have pink outlines, the active tab has
a cyan outline open into the cyan-bordered content, and hover keeps text colors.
Pinned entries have a red `PINNED` badge and left accent; the pink selection
highlight is independent of pin status. Each row has compact pin/unpin and ×
(delete) buttons on the right; the pin button also uses red when pinned. These
buttons act on their own row without copying it.

Paths within the home directory display as `~/...`. Named screenshots show their
saved source paths. Copying restores the appropriate text or image MIME type;
linked local files publish standard URI lists that file managers can paste.

### Ownership and collection

- `scripts/clipboard/astal/` owns presentation, the controller, native GDK
  clipboard publication, GTK text editing/image previews, native Gio directory
  monitoring, and native Gio Trash operations for linked local files.
- `scripts/clipboard/rust-custom/` supplies `clipboard-custom`, the single helper
  for capture and persisted history: memo lifecycle, pin/delete/edit transactions,
  deduplication, pruning, retention, and MIME/file-reference payload policy. It
  never opens a display, launches a UI, publishes a clipboard selection, invokes
  a clipboard-copy command, or deletes linked source files.
- `clipboard-collector.service` is the **only** background Wayland collector,
  registered in `niri/panels.nix`. It runs:

  ```sh
  wl-paste --no-newline --watch clipboard-custom capture
  ```

GTK's focus-scoped clipboard API is not a global data-control watcher, so capture
uses `wl-paste --watch`. Capture batches and panel changes use the same helper,
store, and `history.lock`; there is no separate data writer or second database.
The resident `ags-clipboard.service` application retains clipboard ownership
when hidden; restarting it may lose the selection unless another clipboard
manager has retained it. No clipboard text or image payload is logged.

### Controls

| Action | Result |
| --- | --- |
| Single-click a history entry | Select its preview |
| Double-click a history entry | Copy that entry and close |
| Row pin icon / × | Pin/unpin or immediately delete that row; linked files go to Trash |
| `Escape` or outside click | Save pending edits and close |
| `Enter` outside the editor / `Ctrl+Enter` in the editor | Save, copy, and close |
| `Ctrl+S` | Save immediately, without waiting for autosave |
| `Ctrl+N` / `Ctrl+F` | New memo / focus search |
| `Alt+P` / `Alt+D` / `Alt+E` | Pin/unpin / delete / focus the editor |
| Up/Down outside the editor | Select history entries, saving pending edits before switching |

### Autosave and deletion

All Memo and Text preview edits autosave after 400 ms of idle typing. Each edit
restarts that delay; `Ctrl+S` saves immediately. Selection/tab changes, copying,
and closing also save pending edits. Searching alone does not switch the editor
or force a save, but does not cancel an already scheduled autosave. File/image
previews are read-only: preview editing changes clipboard history, never linked
source files.

Saves compare the original buffer with the stored item while holding the lock.
Typing can continue during autosave; newer text stays in the editor and is saved
next against the just-saved version. Conflicts, external deletions, and disk write
failures preserve unsaved text and show an inline error, rather than silently
discarding it or retrying indefinitely. Another edit schedules a new attempt;
`Ctrl+S` retries immediately. An unresolved save failure blocks
switching/closing/copying. Delete is an explicit discard action.

For a Files entry with an absolute local path in its `name`, Delete (`Alt+D` too)
immediately moves the source file to Trash using Gio, then removes the history
entry and its cached preview. There is no confirmation dialog. Named screenshot
sources are included; cached-only images and text/memo entries only lose their
history/cache data. A Trash failure keeps the entry and shows an inline error;
there is no permanent-delete fallback. Missing files only lose the stale
history entry. Folders are refused; symlinks are trashed without deleting their
targets. Paths are never inferred from arbitrary clipboard text.

Trash and history updates cannot be atomic: if the file is trashed but the history
write fails, the panel reports the partial success so it can be retried. Deletion
holds the controller's transaction lock and rechecks the history path before
operating on disk.

### Commands and helper contract

```text
clipboard-panel run
clipboard-panel status
clipboard-panel toggle [monitor]
clipboard-panel close
clipboard-custom capture
clipboard-custom store --mime MIME
```

With **no arguments**, `clipboard-custom` reads one JSON request on stdin and
writes one JSON reply on stdout. The `prepare`, `snapshot`, `memo`, `pin`,
`delete`, `save`, and `payload` requests retain their structured JSON errors.

`capture` reads the bytes supplied by `wl-paste --watch`, inspects the offered
MIME targets with `wl-paste`, and commits the capture. `store --mime MIME` reads a
payload directly from stdin without needing a display or `wl-paste`. Image MIME
types store cached images; URI lists/GNOME copied-files split into individual
file/image references and remote URLs; other UTF-8 input uses the normal
text/local-path policy. Empty captures are ignored. The standard
`CLIPBOARD_STATE` supplied by `wl-paste` is honored: `sensitive` and `nil` are
discarded, as are KDE password-manager MIME hints. Binary image reads use
`--no-newline`.

`capture` and `store` produce no stdout on success. Failures exit nonzero and
report an error on stderr without printing clipboard contents or subprocess
stderr. Stdin is limited to 64 MiB. Extra CLI arguments are rejected.

### Storage and migration

The default store is:

```text
$XDG_DATA_HOME/clipboard/
├── history.json
├── history.lock
└── images/
```

If `XDG_DATA_HOME` is unset/empty, the fallback is `$HOME/.local/share/clipboard`.
History keeps its version-1 JSON format, IDs, FNV-1a/length digests, pin order,
2,000-item limit, and independent 100-cached-image limit. Text, file-reference,
and image captures deduplicate within their own kind while preserving IDs and
pins; local file references also deduplicate by source path. An empty memo draft
is retained during trimming. Writes use `history.lock` flock and atomic history
replacement. Cache creation is rolled back if a transaction fails, and evicted
images are deleted only after history commits.

Default discovery automatically copies history and images from the former sibling
`rofi-clipboard` directory **only when the new default `clipboard` directory does
not exist**. Migration locks both migration and source history, validates the
history/cache paths, copies into a private staging directory, and atomically
publishes the copy without replacing an existing target. The source is retained
as a backup. Invalid history, unsafe cache entries, or copy errors leave it intact
and do not publish a partial store. Explicit `CLIPBOARD_DATA_DIR` stores are
isolated and never import default history. Former environment variables and
executables are not used.

Rebuild and switch to install the replacement collector and panel wiring; any
separately installed former collector must be stopped. Later writes to the
retained backup are intentionally not merged. The helper discovers/migrates
before returning any JSON snapshot. The GTK backend attaches its directory
monitor only after a successful helper reply, then schedules one reconciliation
to cover writes between snapshot and watch.

### Environment variables

| Variable | Meaning / default |
| --- | --- |
| `CLIPBOARD_DATA_DIR` | Explicit store root for capture and panel history; unset/empty uses the default above, with one-time migration. Explicit stores do not import default history. |
| `CLIPBOARD_SCREENSHOT_DIR` | Directory used to identify pathless PNG captures by exact bytes; defaults to `$HOME/Pictures/Screenshots`. |
| `CLIPBOARD_WL_PASTE` | Executable for MIME enumeration/reads; an unwrapped build defaults to `wl-paste`. Package wrappers set its absolute packaged path. |
| `CLIPBOARD_CUSTOM_HELPER` | Helper executable used by GTK for JSON requests; the launcher supplies the packaged `clipboard-custom`. |
| `CLIPBOARD_PANEL_CONFIG` | Enforced packaged Nix-store entry; cannot override the launcher with checkout application code. |
| `CLIPBOARD_PANEL_PALETTE` | Generated JSON palette, with a packaged default and installed palette link. |
| `PANEL_CSS_FILE` | Live stylesheet; defaults to `/home/raina/System/scripts/clipboard/astal/style.css`. |

### Performance and file monitoring

GTK's native `ListView` virtualizes history, so a large clipboard does not create
thousands of row widgets. Only changed list ranges are replaced, and typing or
busy-state updates do not rebuild the list. Rendering avoids redundant copies of
the native text buffer. Row previews are bounded to 160 Unicode characters;
search, editing, and copying still use the complete content.

Opening prepares history and its memo draft in one helper request. Clean tab and
selection changes use that snapshot without disk I/O or subprocesses. Mutations
return their committed snapshot, and Gio monitoring refreshes external changes.
While open, linked local files share native Gio monitors by parent directory.
Deletion or moving a linked file away triggers locked missing-file cleanup,
removing its history entry (even when pinned) and cached preview. This also works
for named screenshot sources; cached-only images are unaffected. Replacing a file
at the same path keeps the entry. Cleanup preserves the current tab, search, and
unsaved editor text. Monitors are released when the panel hides; opening it again
also cleans files deleted while hidden.

Rust serializes writes into one buffer and skips replacing unchanged history,
avoiding unnecessary fsyncs and self-generated file-watch refreshes without
changing conflict detection, locking, file format, or retention rules.

### Packaging and style reload

The clipboard package specification, shared runtime/launcher builders, Home
Manager registration, `ags-clipboard.service`, and `clipboard-collector.service`
all live in `niri/panels.nix`. The panel service uses the launcher's enforced
store entry and packaged helper default, with the installed palette link.
Generated palettes remain separate from live CSS.

Rust, JavaScript, and packaged JSON changes require a NixOS/Home Manager rebuild
and switch. The launcher bundles an immutable Nix-store entry; restarting
`ags-clipboard.service` or closing and reopening the panel does not load checkout
code. CSS is read directly from the checkout, without Home Manager links under
`~/.config/ags/clipboard/`. The shared native style helper hot-reloads
`PANEL_CSS_FILE` without a rebuild or service restart.

### Validation

From the repository root:

```sh
cargo test --manifest-path scripts/Cargo.toml --locked --offline -p clipboard-custom
cargo clippy --manifest-path scripts/Cargo.toml --locked --offline -p clipboard-custom --all-targets -- -D warnings
npm --prefix scripts/clipboard/astal test
```

All Rust policy, capture, migration, and real-binary CLI tests live in
`scripts/clipboard/rust-custom/src/tests.rs`, registered as the Cargo test target
`tests` (select it with `--test tests`). They use temporary stores and a fake
`wl-paste`, not the live clipboard. The JavaScript tests also run with
`node --test scripts/clipboard/astal/test.js`. Unit tests inject a deterministic
autosave scheduler, without real timer waits. They cover debouncing and exact
Memo/Text payloads, typing and undoing to the original base during slow saves,
refresh deferral, conflict/write failures without retry loops, stale-timer
cancellation on save/selection/deletion, undo, read-only previews, and cleanup.

Real UI and launcher checks:

```sh
node scripts/clipboard/astal/test.js --ui \
  --runtime /nix/store/...-ags-2.3.0 \
  --helper /nix/store/...-clipboard-custom-0.1.0/bin/clipboard-custom \
  --palette /nix/store/...-clipboard-panel-palette.json \
  --panel /nix/store/...-clipboard-panel
```

Use the actual absolute store paths from the package's `ags`, `clipboardCustom`,
`palette`, and panel outputs; `--panel` takes the package directory, not its
executable. The runner needs `Xvfb`, `dbus-daemon`, `niri`, and `wl-paste` on PATH.
It creates nested Niri on private Xvfb, a service-free D-Bus session, and temporary
history, then tears them down. It never uses the live desktop's clipboard/history.
It checks real imports and CSS, compact per-row actions, toolbar-free layout,
400 ms GLib autosave with exact persisted preview text, Ctrl+S saves, exact-row
copy activation, file URI policy, image previews, cross-client PNG clipboard
bytes after hiding, and the packaged launcher/IPC.

File deletion tests use only temporary fixtures and an isolated Trash, covering
immediate Delete/Alt+D without dialogs, named screenshots, cached-only images,
missing files, folder refusal, and symlink target preservation. Native monitoring
checks cover external deletion/rename, pinned named-image cleanup, atomic
replacement, draft preservation, and deletion while the panel is hidden.
`--screenshot /absolute/path.png` optionally captures the fixture panel.
Add `--benchmark` to the same `--ui` command to measure tab switching, filtering,
and editing with 1,800 synthetic text entries on the private display. Timings
include GTK frame processing; the check also verifies virtualization and that
clean switches and typing do not launch helper processes.

---

## waybar-timer

`scripts/waybar-timer` is a small countdown timer that outputs Waybar-compatible JSON. The main process owns the timer state, while command invocations communicate with it over a Unix datagram socket.

### Features

- Adds time in five-minute steps. Supports countdowns up to two hours
- Start, pause, add time, and clear actions. Plays a three-beep alarm when the countdown finishes.
- Uses a per-user Unix socket for commands. Cleans up stale socket files when it starts

### Requirements

- Rust and Cargo
- Waybar
- `ffplay` from FFmpeg for the alarm sound

### Commands

| Command | Result |
| --- | --- |
| `add` | Add five minutes |
| `toggle` | Start, pause, or resume the countdown |
| `clear` / `stop` | Stop and reset the countdown |

### Waybar configuration

A minimal custom module configuration looks like this:

```jsonc
{
  "custom/timer": {
    "exec": "/path/to/waybar-timer",
    "return-type": "json",
    "escape": false,
    "restart-interval": 1,
    "exec-on-event": false,
    "on-click": "/path/to/waybar-timer add",
    "on-click-middle": "/path/to/waybar-timer toggle",
    "on-click-right": "/path/to/waybar-timer clear"
  }
}
```

---

## Desktop panels

### Dashboard (AGS/Astal GTK4)

`Mod+N` and the right-most Waybar battery button invoke `dashboard-panel toggle`.
The 440px dashboard retains its top-right placement, yellow cut-corner frame
and glow, cyan/pink palette, three sections, scrolling, and Escape/outside-click
dismissal. The Waybar badge retains battery percentage and charging/low states,
now read from Astal rather than a parallel shell status reader.

- **Power:** native Astal battery and power profiles, plus airplane mode through
  native NetworkManager and Bluetooth adapter power. Idle inhibition is removed.
- **System parameters:** asynchronous Gio reads of `/proc/stat`, `/proc/meminfo`,
  and `/sys/class/thermal/thermal_zone0/temp` every 2.5 seconds while open;
  root filesystem usage from the existing coreutils `df` every 30 seconds.
  Closing cancels reads, timers, and pending `df`. No monitoring daemon is added.
- **Notifications:** native AstalNotifd owns `org.freedesktop.Notifications`.
  TypeScript supplies archive/popup policy, not another notification backend.

Sources, tests, and runtime metadata live in `scripts/dashboard/`; the launcher
is defined in `niri/panels.nix`. The `ags-dashboard` user service owns the
resident `dashboard-panel` application, starting `power.ts`. `parameters.ts`
contains metrics and `notification.ts` contains notification state and widgets.
Run `npm test` in `scripts/dashboard/` for the consolidated `tests.ts` suite.

### Notification popups

The AGS dashboard displays up to five newest-first popup cards near the
top-right of the first available monitor. Popups and history share GTK
notification rows with app images, plain-text bodies, close controls, and native
action buttons. Popups do not take keyboard focus.

The default timeout is eight seconds; explicit application timeouts are
respected. Hover pauses expiry, zero-timeout and critical notifications remain
until dismissed, and DND hides popups while retaining non-transient history.
Expired entries stay in the dashboard without stale actions. Transient
notifications are never archived, and client-requested closes remove entries.

The latest 200 entries are saved atomically through Gio to
`$XDG_STATE_HOME/dashboard/notifications.json`, with private directory/file
permissions. When the new archive is absent, the previous
`$XDG_STATE_HOME/quickshell/notifications.json` is imported once as archived text,
without modifying the old file or retaining stale actions. Native Astal
DND state is mirrored in the archive. Restarts do not replay popup windows;
Astal-restored live actions are reattached only to matching archived rows.

`F8` invokes `dashboard-panel toggleDnd`. Apply a normal NixOS/Home Manager
switch before restarting the new service: the updated runtime, packaged sources, and
notification-bus ownership cannot be installed by a service restart alone.

### Network panel (AGS/Astal GTK4)

`network-panel` opens the network card on the monitor whose Waybar button was
clicked. It retains the bounded 420×520 layout, chamfered translucent frame,
colors, profile chooser, and outside-click/Escape dismissal, without a header
or manual scan button. Middle-click the Waybar network button to toggle Wi-Fi
without opening or closing the panel. Scanning starts when the panel opens and
repeats every 15 seconds while it stays open and Wi-Fi is enabled, including
on the Info and QR pages; it stops when the panel closes or the radio is off.

AstalNetwork supplies the shared native NetworkManager client. Its exposed
libnm objects in GJS own **all** Wi-Fi/Ethernet information and operations:
multiple adapters, signal/security, software/hardware radio state, saved
profiles, scanning, activation, disconnection, forgetting, IP metadata and
permission-controlled secrets. No Astal patches, second client, Rust network
inventory, or `nmcli` backend is used.

Astal's `wifi`/`wired` convenience views select a single adapter, and its AP
activation helper does not cover every required security/profile flow. The GJS
adapter therefore uses the **same Astal-owned client** and its exposed libnm
objects for multi-adapter lifecycle, exact-UUID activation, open/WEP/SAE/OWE
settings, profile deletion, metadata and secrets. These remain native
operations, not custom-backend exceptions. Activation completes only when the
requested connection and device both report `ACTIVATED`; closing a pending UI
request does not issue an implicit network disconnect.

Connected and saved networks are grouped above nearby unsaved networks, with
one divider between the groups, like the Bluetooth device list. Connect or
Disconnect sits beside each network name; unsaved networks have no Forget
button or empty actions row. Within each group, native connection ordering and
signal-icon buckets avoid reshuffling for one-percent changes. Ethernet takes
priority in Waybar. Profiles are selected by UUID, not their display names.
Open, OWE, WEP and personal WPA/WPA2/WPA3 connections use native libnm settings.
Ambiguous per-profile credential updates and enterprise/hidden configuration
require a separately configured saved profile; the panel does not launch an
external connection editor. Forget confirms deletion of all matching saved
profiles and disconnects the network.

**Info** shows SSID, signal/security, active profile ID/UUID, interface/MAC,
IPv4/IPv6, gateways, DNS, BSSID, frequency/band/channel/mode, and link rate.
Passwords are queried only after **Reveal password**, not on opening Info.
**QR** explicitly requests a share code for supported open/WEP/personal WPA
profiles, including the hidden flag. Enterprise and Enhanced Open sharing
remain unsupported, with an explanation.

Rust owns only the QR encoder, a capability Astal/libnm does not provide:
`network-manager qr` reads bounded JSON on stdin and returns a PNG data URL.
All old `info`, `secret`, and `connect-wep` commands have been removed. Neither
credentials nor QR payloads appear in command arguments or temporary files.
GJS pins the interface, SSID, profile UUID, active connection path and BSSID;
late secret/QR replies are discarded after roaming, connection changes,
selection changes or dismissal, and displayed sensitive state is cleared.

The helper protocol accepts only `network-manager qr`, with no extra
arguments. One JSON object is read from stdin, limited to 16 KiB including
syntax and whitespace:

```json
{"ssid":"Guest","authentication":"WPA","password":"example-only","hidden":false}
```

`ssid` is a nonempty UTF-8 string of at most 32 bytes without control
characters; `authentication` is exactly `nopass`, `WPA`, or `WEP`; `password`
contains no control characters and is nonempty for WPA/WEP (optional for
`nopass`, whose payload never includes a password); `hidden` is a required
boolean. Unknown fields, wrong types, malformed JSON/UTF-8, and oversized
input fail. Backslash, semicolon, comma, colon, and double quote are escaped
in QR fields; Unicode, whitespace, and the hidden flag are preserved. The
payload goes only to `qrencode` stdin — the packaged wrapper sets only
`NETWORK_MANAGER_QRENCODE` — with encoder diagnostics suppressed and PNG reads
capped at 1 MiB. Success returns `{"image":"data:image/png;base64,..."}`;
failures exit with status 2 and generic JSON `error`/stderr diagnostics, never
credential-bearing parser or subprocess output. The helper does not query
NetworkManager.

UI, native integration and tests live in `scripts/network-manager/astal/`,
with the same file layout as the media and audio panels: `frontend.js` (GTK4
widgets, rendering, application IPC), `backend.js` (Astal service integration,
native actions, transport to the Rust helper), `logic.js` (GTK-independent
presentation/controller utilities), `style.css`, and the consolidated
cross-runtime `test.js`. The QR-only Rust helper and its protocol tests live in
`scripts/network-manager/rust-custom/`. The launcher is
`niri/panels.nix`, alongside the runtime typelibs, JSON palette generated
from `themes/palette.nix`, and resident `ags-network.service`, with palette restart
triggers. CSS is read directly from the checkout without Home Manager links. `scripts/Cargo.toml` and `scripts/packages.nix` integrate the
workspace and packages; both the Cargo package and installed helper executable
are named `network-manager`.

```sh
network-panel run
network-panel status
network-panel toggle eDP-1
network-panel close
network-panel toggleWifi  # Toggle the native Wi-Fi radio without changing panel visibility
systemctl --user restart ags-network.service  # Restart the installed generation, not checkout JS
```

The native application owns the `network-panel` AGS instance. Requests are
JSON; monitor names are transported without shell interpolation. Status
polling is read-only and never dismisses panels or mutates network state.

All six panel launchers bundle their immutable Nix-store entries with
`ags bundle --gtk 4` into distinct files
under `$XDG_RUNTIME_DIR/ags-panels/`, then execute their packaged `panel-gjs`
wrapper. AGS 2.3's `run` command shares one `ags.js` output across instances,
which can execute another panel's code during concurrent startup; the wrapper
preserves each panel's native GI environment and GTK4 layer-shell preload,
while `XDG_RUNTIME_DIR` itself stays unchanged so Astal IPC sockets remain
accessible. The network application checks native registration before holding
its event loop, so registration failure exits instead of serving an
uninitialized badge forever.

Environment overrides:

- `NETWORK_PANEL_CONFIG`: enforced packaged Nix-store entry; environment values
  cannot select checkout sources.
- `PANEL_CSS_FILE`: defaults to
  `/home/raina/System/scripts/network-manager/astal/style.css`; CSS hot-reloads.
- `NETWORK_PANEL_PALETTE`: generated JSON palette, with a store-backed launcher
  default.
- `NETWORK_CUSTOM_HELPER`: QR-only `network-manager` executable.

- `DASHBOARD_PANEL`/`MEDIA_PANEL`: peer launchers invoked by
  `dismissOtherPanels()`; other panels use `NETWORK_PANEL` (default
  `network-panel`) to dismiss network.

Apply the normal NixOS/Home Manager switch once to install the new launcher,
service and links. A service restart alone cannot install migration wiring.


### Media panel (AGS/Astal GTK4)

The centre Waybar button toggles the resident AGS media panel in
`scripts/media-control/astal/`. Its JavaScript is organized into three files:

- `frontend.js`: GTK4 widgets, popup layout, rendering, input, theme loading, and application IPC.
- `backend.js`: Astal service integration, native actions, lifecycle management, and transport to the Rust helper.
- `logic.js`: small GTK-independent presentation/controller utilities, tested with Node.js.

Custom media policy and metadata discovery remain in Rust; `backend.js` is the
integration layer, not a second implementation of that policy. Use native Astal
APIs for capabilities the framework already provides, without dependency patches
or replacement framework implementations.

The 460px card retains artwork, title, artist/author, album,
seekable progress, transport controls, volume, and the playing/paused source
selector. **There is no Output section or per-stream routing.** Physical output
selection remains in the separate AGS/Astal audio panel.

Astal MPRIS owns player discovery, metadata, transport, and seeking. Volume is
**0–100%, MPRIS-first**: use the selected player's writable volume when available,
otherwise use Astal WirePlumber only for a conservatively identified stream.
Ambiguous/shared streams are not guessed, and the system default output is never
used as a media-volume fallback. Mute uses only the matched stream's native
WirePlumber mute flag, preserving its volume. Without a safely matched stream,
the mute button is disabled; an available MPRIS volume slider still works.
Global audio output volume still allows 150%.

Position, position support, and seek capability come from Astal MPRIS. The UI
does not extrapolate time, fabricate seek acknowledgements, or overwrite native
state with optimistic seeks. Popup fades use `Gtk.Revealer`, respecting GTK's
animation setting rather than running a custom frame-clock animator.

The separate Rust `media-control` helper owns duplicate/source policy,
conservative stream matching, native action plans, artwork, and author metadata;
JavaScript bridges native snapshots and executes the returned plans. Absolute
play/pause targets are grouped; Next and Previous address one publisher.
Middle-clicking Waybar toggles all playback, and right-click cycles the selected
source. Waybar requests status once per second, sharing selection with the card
even while closed. Playing/paused/inactive classes, full tooltips, and the
50-character/35-CJK title limits are preserved by the AGS formatter.

`media-control model` is a resident JSONL snapshot/action planner over stdin/stdout.
`media-control artwork <xesam-url|path>` reuses local cover discovery and embedded
art extraction. `media-control credit` accepts JSON track metadata on stdin and
owns video-ID parsing, YouTube/Bilibili author lookups, fallback credits, and a
bounded private cache under `$XDG_CACHE_HOME/media-control/`. Lookups use curl
with a ten-second timeout; opening a supported video sends its video ID to that
site's public metadata endpoint. No credentials or browser cookies are sent.

`niri/panels.nix` packages `scripts/media-control/astal/` JavaScript/JSON
and the shared style helper into an immutable store source, preserving relative
imports. CSS is read directly from the checkout through `PANEL_CSS_FILE`.
The palette is serialized from `themes/palette.nix` to the separate
`~/.config/ags/media-palette.json`; the frontend generates GTK `@define-color`
rules programmatically from this JSON without writing generated CSS into the
repository.

The Niri-only `ags-media.service` runs `media-panel run`, restarts on failure,
and is wanted by Waybar and the graphical session. The launcher uses the pinned
AGS 2.3 bundler: `ags bundle --gtk 4 -- <store-entry> <output>`. All six panel
launchers use distinct `$XDG_RUNTIME_DIR/ags-panels/<name>-panel.js` outputs;
`ags run` would otherwise share a single `ags.js` file and race at login.
`mkAgsRuntime` in `niri/panels.nix` adds a companion `panel-gjs` wrapper using each
AGS package's captured runtime environment and GTK4 layer-shell preload.
Media retains GTK4, Graphene, Astal MPRIS and Astal WirePlumber; no manual
`GI_TYPELIB_PATH` supplement is needed.
`niri/panels.nix` also builds a combined native GdkPixbuf loader cache for
standard formats, librsvg, and `webp-pixbuf-loader`, preserving PNG/JPEG/SVG while
adding WebP artwork support. AGS's wrapper sets `GDK_PIXBUF_MODULE_FILE` to this
cache rather than the default librsvg cache that omits WebP; no JavaScript
decoder or user environment override is needed. The frontend subclasses native
`Astal.Application`, constructs it with `instance_name: "media-panel"`, and uses
its `acquire_socket()` and application lifecycle APIs. Requests use Astal's
native request socket through
`ags request --instance media-panel -- <JSON>` with this payload:

```json
{"command":"toggle","monitor":"eDP-1"}
```

`command` is one of `status`, `toggle`, `togglePauseAll`, `cycleMedia`, `pauseAll`,
or `close`; `monitor` is optional. `status` returns Waybar JSON on stdout. The
launcher constructs JSON with `jq --arg`, never shell-evaluated JSON. The
frontend implements the native `vfunc_request(request, connection)` extension
point, writes replies with `AstalIO.write_sock`, and closes the connection through
GIO after the async write completes, including on errors. This avoids the GJS
convenience responder's delayed cleanup without patching dependencies or adding
another IPC interface or client. Cross-panel IPC also stays in `frontend.js`;
Rust-helper command construction and reply validation stay in `backend.js`.

```sh
media-panel status
media-panel toggle eDP-1
media-panel togglePauseAll
media-panel cycleMedia
media-panel pauseAll
media-panel close
systemctl --user restart ags-media.service  # Restart the installed generation, not checkout JS
```

`MEDIA_PANEL_CONFIG` is enforced to the packaged Nix-store entry, not a
configurable checkout path. `PANEL_CSS_FILE` defaults to
`/home/raina/System/scripts/media-control/astal/style.css` and is hot-reloaded.
`MEDIA_PANEL_PALETTE` is a JSON file path; `MEDIA_CONTROL` and `NETWORK_PANEL`
are executable paths, not shell commands. The service uses the launcher's
store entry, supplies the installed palette link and packaged helper/peer launcher,
and sets `GDK_BACKEND=wayland`. Graphical-session Wayland and D-Bus variables
are inherited.

Rebuild and switch after changing JavaScript, packaged JSON, Nix/service wiring,
or the Rust helper. CSS changes need neither a rebuild nor a service restart.
Network requests use the separate `network-panel` launcher.

### Shared popup style

The AGS/Astal GTK4 cards use per-panel GTK CSS for controls and Cairo-drawn
chamfered frames for translucent backgrounds and yellow outlines.
`themes/palette.nix` supplies the shared colors as generated JSON palettes,
kept separate from the CSS files read directly from the checkout.

| Panel | Presentation sources |
| --- | --- |
| Audio | `scripts/audio-control/astal/`: `style.css`, frame in `frontend.js` |
| Media | `scripts/media-control/astal/`: `style.css`, frame in `frontend.js` |
| Network | `scripts/network-manager/astal/`: `style.css`, frame in `frontend.js` |
| Dashboard/notifications | `scripts/dashboard/`: `style.css`, frames and shared widgets in `notification.ts` |
| App launcher | `scripts/app-launcher/`: `style.css`, frame in `frontend.js` |
| Clipboard | `scripts/clipboard/astal/`: `style.css`, frame in `frontend.js` |

Audio uses Bluetooth/Audio tabs and native services. Output volume
still spans 0–150%; the separate media panel is limited to 100%.
Popup sizes and placement remain specific to their content, not separate themes.

After installing the generation through Home Manager, edit CSS in place; the
shared native `scripts/panels/style.js` helper monitors the stylesheet and reloads
it without a restart. `PANEL_CSS_FILE` defaults to the checkout's per-panel
`style.css`. JavaScript/TypeScript, including that shared helper, and packaged
JSON are filtered into per-panel Nix-store sources with the relative `scripts/`
layout intact. Editing them requires a rebuild and switch. Runtime bundling
still occurs at service startup, but reads only the immutable store entry; there
is no opt-in environment override for live application sources.

`niri/panels.nix` owns the `mkAgsRuntime`, `mkPanel`, and `packages`
exports, per-panel package specifications, and Home Manager services. The
`packages` function takes `pkgs`, `audioControl`, `mediaControl`,
`networkManager`, `clipboardCustom`, and `repoRoot`.

### Audio panel

The audio button opens the AGS/Astal GTK4 Bluetooth/Audio popup described
under [`audio-control`](#audio-control). Its service remains `audio-control-ui`,
but its AGS instance is `audio-panel`. Playback remains in the media panel.

### External panel behavior

Dashboard, network, media, and audio use transparent monitor-sized layer
surfaces with a visible card. The backdrop consumes outside clicks;
Escape and a second button press dismiss the card. Opening network invokes
`dashboard-panel close` and `media-panel close` directly through asynchronous
Gio subprocesses; network status polling never spawns another panel command.
Opening media or dashboard closes network through `network-panel close`.
The audio popup stays
independent. Native notifications use a small non-focus-taking layer surface
instead, so toasts do not grab keyboard input.

Network scanning, dashboard metrics, and audio compatibility refreshes stop while
the corresponding panel is hidden. Astal audio/Bluetooth state and notifications
remain event-driven, and the media badge requests metadata on demand even while
its card is closed.

### Verifying changes

From the repository root, run the frontend and Rust policy/protocol checks:

```sh
npm --prefix scripts/dashboard test
npm --prefix scripts/network-manager/astal test
node --test scripts/panels/test.js
cargo test --manifest-path scripts/Cargo.toml --locked \
  -p audio-control -p media-control -p network-manager --all-targets
cargo build --manifest-path scripts/Cargo.toml --locked -p network-manager -p media-control
```

The Rust tests for `audio-control` and `media-control` each live in that
crate's `src/tests.rs`, registered as the Cargo test target `tests`. This keeps
unit and real-binary CLI/protocol coverage together. The commands above run
both suites; use `--test tests` to select only the consolidated target.

The consolidated `scripts/media-control/astal/test.js` covers guarded
action queues, native position and seek capabilities, native mute, helper protocol
validation, and badge formatting. Its private harness supplies in-memory native-service
doubles; `logic.js` is tested directly. The tests use Node.js, need no npm
dependencies, and do not load GTK/GI:

```sh
node --test scripts/media-control/astal/test.js
node --test scripts/audio-control/astal/tests/audio-control.test.js
```

The live IPC lifecycle regression is an ignored test in the existing Rust
`media-control` crate, not another runtime script or dependency. It requires a
running `ags-media.service` and native `media-panel` launcher. It sends only
`status` requests: no playback/UI mutation and no service restart.

```sh
cargo test --manifest-path scripts/Cargo.toml --package media-control --locked --test tests live_panel_ipc_lifecycle -- --ignored --nocapture
# Shorter check; MEDIA_PANEL_LAUNCHER can select an explicit launcher executable:
MEDIA_PANEL_IPC_REQUESTS=100 cargo test --manifest-path scripts/Cargo.toml --package media-control --locked --test tests live_panel_ipc_lifecycle -- --ignored --nocapture
```

The default is 1,200 requests with five-second command timeouts, validating each
Waybar badge response and checking that the same GJS process remains alive.
It compares accepted connections at `/run/user/<uid>/astal/media-panel.sock` and
total descriptors before/after a one-second settling interval. Growth above two
request sockets or eight total descriptors fails the check. Progress and counts
are printed; normal Cargo tests skip this live-desktop test.

Rust `media-control` tests cover duplicate/source policy, conservative stream
matching, native action plans, metadata caching, and the JSON bridges. Media
frontend tests live with the JavaScript modules in `scripts/media-control/astal/`.

Dashboard tests are consolidated in `scripts/dashboard/tests.ts` and use Node's
built-in TypeScript transformation. `npm --prefix scripts/dashboard test` runs
the parsing, metrics, battery/status, notification, DND, archive, and mocked
native-service lifecycle checks without a display or desktop-service mutations.
No separate Python helpers or test-output directories are retained.

The consolidated `scripts/network-manager/astal/test.js` is one cross-runtime
entry point; the former `controller.js` and `runtime.js` were folded into
`backend.js`/`frontend.js`/`logic.js`, and the former `tests/` directory into
`test.js`'s Node and GJS modes. `node --test test.js` exercises controller
policy and formatting with simulated native dependencies — identity-pinned
requests, cancellation races, launcher and startup checks — without GTK/GI or
live NetworkManager state. Its Node mode also runs the shared
`scripts/panels/test.js` suite, which requires Nix and uses the root
`nixpkgs` input pinned in `flake.lock` (fetching that input if it is not cached).
This single shared test file covers CSS reloading, panel packaging, and launcher
behavior. It evaluates the consolidated package specifications and executes
launchers against fake AGS/GJS programs, checking immutable entries, source
filtering, CSS-only overrides, native dependencies, per-panel bundles, compiler
failures, commands, help/arity policy, JSON requests/replies, runtime isolation,
exit statuses, and `exec` behavior. Its Node mode does not build Rust/AGS packages
or contact desktop services. Its native GJS mode checks real GTK CSS reloading
on a private X11 display, including atomic saves and invalid-CSS recovery.
Rust tests cover QR escaping, validation,
malformed/oversized input, output bounds, and rejected legacy arguments:

```sh
cargo test --locked --manifest-path scripts/Cargo.toml -p network-manager
cargo fmt --manifest-path scripts/Cargo.toml -p network-manager -- --check

nix-instantiate --parse niri/panels.nix >/dev/null
nix-instantiate --parse scripts/packages.nix >/dev/null
```

The same `test.js` carries GJS modes for native and QR-pipe coverage. Use the
network launcher's overridden AGS runtime (it includes all required GTK4,
AstalNetwork, and NM typelibs), not an unrelated global AGS installation:

```sh
panel-gjs -m scripts/network-manager/astal/test.js native
panel-gjs -m scripts/network-manager/astal/test.js runtime
```

`native` uses real libnm settings and Gio cancellation with simulated
device/reply objects, without creating a live NM client or mutating network
state. `runtime` exercises the real asynchronous QR pipes with fake encoders,
malformed/oversized replies, cancellation, reaping, and fake panel launchers;
`NETWORK_TEST_QR_HELPER` optionally exercises the packaged helper.

`node test.js --ui --ags PATH --palette FILE` runs the real GTK UI widgets and
the closed application bootstrap — native offline initialization and socket
requests — through its own authenticated Xvfb and a service-free private D-Bus
for both bus addresses. Never run the `ui`/`application` checks directly
against the desktop session. Xvfb checks bounded GTK allocation and
interaction, not Wayland layer-shell placement or exclusive keyboard grabs;
verify those in the Niri session after installing the configuration.

Do not switch generations or send real playback/network commands just to run
isolated checks. Evaluate/build `networkManager` and `networkPanel` from
`scripts/packages.nix` for package/runtime validation; no test requires
rebuilding/switching the system or changing live network state.
