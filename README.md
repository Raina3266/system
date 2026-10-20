# Custom Scripts

This repository contains ten Rust packages used by the desktop configuration:

- `audio-control` — headless Waybar status, Bluetooth pairing agent, and physical audio port/profile bridge
- `control-centre` — the long-running Waybar media-title stream
- `media-panel` — the former multi-player MPRIS panel, kept in the tree until its Quickshell replacement proves parity
- `network-manager` — the NetworkManager info and Wi-Fi QR bridge used by Wayle
- `ocr-screenshot` — screenshot OCR and clipboard integration
- `preview-panel` — the shared GTK4 preview surface
- [`rofi-clipboard`](#rofi-clipboard) — clipboard history and memos
- `rofi-filesearch` — searchable file launcher with previews
- [`waybar-timer`](#waybar-timer) — interactive Waybar countdown timer
- `webcam-crop` — on-demand virtual webcam cropper and supervisor

The Niri configuration applies one structure-and-behavior Wayle v0.7.0 delta
for the [dashboard and network panels](#wayle-dashboard). Audio and media use
unmodified Quickshell.

## Live desktop editing

After one Home Manager/NixOS switch, presentation files are linked directly
from `/home/raina/System`. They no longer need another system rebuild:

| What to edit | When it takes effect |
| --- | --- |
| `niri/config.kdl` | Niri reloads it on save |
| `niri/rofi/config.rasi`, `themes/rofi/*.rasi` | Next Rofi launch |
| `themes/quickshell/*.qml` | Quickshell reloads live UI components on save; shared controls affect both popups |
| `themes/waybar/top.jsonc`, `bottom.jsonc` | Waybar restarts automatically |
| `themes/waybar/waybar.css` | Waybar reloads CSS automatically |
| `themes/rofi/preview-panel.css` | Preview panel hot-reloads |
| `themes/media-panel.css` | Open media panel hot-reloads |
| `themes/wayle/*.scss` | Wayle recompiles the override on save |
| `niri/audio-control/*.qml` | Quickshell reloads the audio panel on save |

Rust, Nix module, package, service, and kernel changes still require a rebuild.
Each Rust package now hashes only Cargo sources, so changing one UI stylesheet
or sibling utility no longer invalidates every desktop helper.

## audio-control

`niri/audio-control` is a small Quickshell configuration using the stock Nixpkgs
package, with no Quickshell patches. `scripts/audio-control` remains headless.
The `audio-control-ui` user service runs only in Niri.

The Waybar audio button uses `audio-control status` for its JSON. Its glyph
tracks the default output while Bluetooth is off, changes when the adapter is
on or connected, and leaves the full output/input/device state in the tooltip.
Right-click toggles the default adapter through Quickshell's native Bluetooth API.

Left-click opens the popup on the clicked monitor. Escape, the close button,
another click on the same Waybar button, or clicking outside dismisses it.

| Tab | Content |
| --- | --- |
| Pair | Bluetooth power, scan, pair, connect, disconnect, forget, and battery levels |
| Output | Default volume/mute, output devices, and physical ports |
| Input | Default volume/mute, microphones, and their ports |

There is no Play tab; playback stays in the existing media panel. Output volume
runs from **0 to 150%**, retaining amplification above 100%. Input remains
0–100%. Volume and mute use Quickshell's native PipeWire API, and live device-only
choices use its preferred-default API.

Quickshell 0.3.1 provides Bluetooth device discovery and connections, but no
PIN/passkey pairing agent or physical audio port/profile API. Only those gaps
use the Rust helper:

- Physical choices preserve compact **Speaker**, **Headphones**, and USB labels
  with full descriptions in tooltips. The helper retains mutually exclusive
  ALSA profiles, revalidates stale choices, prefers profiles preserving
  microphones, and attempts rollback after a failed switch.
- Pairing starts `Device1.Pair` on the same connection as the existing agent,
  shows its prompts/passkeys in the Pair tab, accepts PIN/passkey input, then
  trusts and connects the device. For numeric comparison, compare the displayed
  six-digit passkey and send `yes` to confirm or `no` to reject. Closing the
  panel or leaving Pair cancels pending pairing and stops discovery. The helper
  does not compete for BlueZ's session-wide default-agent role.

### Commands

```text
audio-control status
audio-control bluetooth-power [on|off|toggle]
audio-control devices <output|input>
audio-control set-default <output|input> <key>
audio-control wayle-list <output|input>
audio-control wayle-set-default <output|input> <key>
audio-control stream-move <key|serial:N|node:N|name> <destination-key>
audio-control artwork <xesam-url|path>
audio-control bluetooth-devices
audio-control bluetooth-scan
audio-control bluetooth-pair <address|name>
```

`devices` prints JSON rows for the popup; `set-default` applies a stable device
or physical-port key. `bluetooth-pair` is
interactive: it prints the passkey to type on the device, or asks on stdin for
one the device displays, and accepts either an address or a distinctive part of
a device's name.

The popup can also be controlled directly:

```sh
quickshell ipc --config audio-control call panel toggle eDP-1
quickshell ipc --config audio-control call panel close
quickshell ipc --config audio-control call panel bluetoothPower
```

After switching the NixOS/Home Manager generation, the service starts with the
graphical session and Waybar wants it. For development without switching:

```sh
cargo build --manifest-path scripts/Cargo.toml --package audio-control --locked
AUDIO_CONTROL_HELPER="$PWD/scripts/target/debug/audio-control" \
  QT_QUICK_CONTROLS_STYLE=Basic quickshell --path niri/audio-control
# In another terminal:
quickshell ipc --path niri/audio-control call panel toggle eDP-1
```

`wayle-list` and `wayle-set-default` retain their existing NUL-delimited
protocol for the separate Quickshell media panel. For `stream-move`, prefer
`serial:N` (a PipeWire object serial) or `node:N` (a PipeWire node ID). These
namespaces cannot collide, but node IDs can be reused after a stream ends.
The panel therefore uses stable `serial:N` selectors only. Legacy bare
identifiers and names still work when unambiguous across all matching fields.
The `artwork` command is unchanged.

### Development checks

From the repository root:

```sh
cargo fmt --manifest-path scripts/Cargo.toml --package audio-control -- --check
cargo test --manifest-path scripts/Cargo.toml --package audio-control --locked
cargo clippy --manifest-path scripts/Cargo.toml --package audio-control --locked -- -D warnings
```

Tests cover device/port identity, compact labels, volume bounds, stream routing,
inactive-profile Speaker/Headphones selection, delayed device creation, stale
targets, server rejection, rollback, and typed stream-identifier collisions.
The pure QML/JavaScript matching tests in `themes/quickshell/tests/` run with
Qt Quick Test and cover shared streams, browser aliases, paused sources, and ordering.

---

## rofi-clipboard

`scripts/rofi-clipboard` is a clipboard history manager for Wayland and Rofi. It watches the clipboard with `wl-paste`, stores text, file references, and images locally, and presents the history through custom Rofi modes.

### Features

- Separate modes for memos, captured text, and files
- Editable memos with selection-change autosave
- Text, file-reference, and image previews inside Rofi
- Pin and delete actions
- One Edit action for soft-wrapped text editing and full image preview in a companion panel
- The open panel follows Rofi selection changes and saves modified text before switching items
- Keeps the highlighted clipboard item selected when search text is shortened or cleared
- Restores text, URL, and image MIME types; local files copy back as standard URI lists that Dolphin can paste
- Detects local files copied from a file manager and keeps them in File mode
- Detects standalone web URLs and keeps them in File mode
- Shortens paths inside the home directory from `/home/raina/...` to `~/...`
- Shows the saved file path for Niri screenshots
- Removes missing linked local files and any cached image previews when Rofi next renders
- Deduplicates repeated clipboard entries
- Ignores empty and sensitive clipboard values
- Keeps up to 2,000 history entries
- Uses file locking and atomic writes to protect the history file

### Controls

| Action | Result |
| --- | --- |
| `Enter` | Copy the selected item |
| `Alt+P` | Pin or unpin the selected item |
| `Alt+D` | Delete the selected item |
| `Alt+E` | Open text for editing or show the full image; press again to save and close the panel |
| `Up` / `Down` while panel is open | Save modified text, then show the newly selected text or image |

The interface contains three modes:

- **Memo** — editable notes plus a permanent empty creation row at the bottom; pinned memos stay at the top
- **Text** — captured text entries
- **Files** — copied local files, web URLs, and captured images with previews

Rofi opens in Memo mode with an empty **New memo** row at the bottom. Clicking
**Edit** opens the currently selected memo in the companion editor. Saving text
in the empty row turns it into a regular memo and immediately creates a new
empty row at the bottom. While the editor is open, moving through the Memo list
saves the previous memo and loads the newly selected one, matching the Text
mode preview behavior.

### Commands

```text
rofi-clipboard [run]
rofi-clipboard status
rofi-clipboard clear
rofi-clipboard capture
rofi-clipboard store --mime MIME
rofi-clipboard script <memo|text|files>
```

`status` is the Waybar `custom/clipboard` backend: a long-running process that keeps a `wl-paste --watch rofi-clipboard capture` child for event-driven capture and emits a JSON status line whenever the history changes. `clear` clears the current Wayland selection (stored history is untouched). `capture` receives clipboard data from `wl-paste --watch` and is what `status` drives internally. The `store` command reads an item from standard input and stores it with the supplied MIME type.

### Data storage

By default, history is stored in:

```text
$XDG_DATA_HOME/rofi-clipboard/
├── history.json
├── history.lock
└── images/
```

If `XDG_DATA_HOME` is not set, the fallback is `~/.local/share/rofi-clipboard`.

### Environment variables

| Variable | Purpose |
| --- | --- |
| `ROFI_CLIPBOARD_DATA_DIR` | Override the history and image data directory |
| `ROFI_CLIPBOARD_THEME` | Override the Rofi theme path |
| `ROFI_CLIPBOARD_ROFI` | Override the `rofi` executable |
| `ROFI_CLIPBOARD_WL_COPY` | Override the `wl-copy` executable |
| `ROFI_CLIPBOARD_WL_PASTE` | Override the `wl-paste` executable |
| `ROFI_CLIPBOARD_PREVIEW_PANEL` | Override the `preview-panel` executable |
| `PREVIEW_PANEL_CSS` | Override the preview panel CSS/configuration path |
| `ROFI_CLIPBOARD_SCREENSHOT_DIR` | Directory used to identify and label saved screenshots (default: `~/Pictures/Screenshots`) |
| `ROFI_CLIPBOARD_PREVIEW_WIDTH` | One-launch preview width override (configured default: `400`) |
| `ROFI_CLIPBOARD_PREVIEW_HEIGHT` | Preview height in pixels (default: `615`) |
| `ROFI_CLIPBOARD_PREVIEW_SIDE` | Place the preview to the `left` or `right` of Rofi (default: `left`) |
| `ROFI_CLIPBOARD_PREVIEW_GAP` | Space between the preview and Rofi in pixels (default: `10`) |
| `ROFI_CLIPBOARD_ROFI_WIDTH` | Rofi window width used for companion placement (default: `400`) |

Default panel placement, size, and GTK styling come from
`themes/rofi/preview-panel.css`. Home Manager links that file to
`~/.config/preview-panel/preview-panel.css`, so valid saves hot-reload without
rebuilding. The `preview-panel-settings` comment at the top controls `width`,
`height`, `companion_width`, `side`, `gap`, `x`, and `y`; the rest is normal
GTK4 CSS. Positive `x` moves right and positive `y` moves down.

Each launcher's Rasi file can override any subset of those geometry settings
with a `preview-panel-layout` comment. For example:

```css
/* preview-panel-layout
width: 300px;
height: 400px;
companion-width: 375px;
*/
```

Omitted fields inherit from `preview-panel.css`. The effective priority is
environment/command-line override, then launcher Rasi, then the global CSS.

For a session-wide environment override, set values such as:

```nix
home.sessionVariables = {
  ROFI_CLIPBOARD_PREVIEW_WIDTH = "560";
  ROFI_CLIPBOARD_PREVIEW_SIDE = "right";
  ROFI_CLIPBOARD_PREVIEW_GAP = "10";
};
```

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

## Wayle dashboard

`Mod+N` and the right-most Waybar battery button open Wayle's dashboard in a
monitor-local layer-shell window directly below Waybar and flush with the
screen's right edge. The button shows capacity and charging state,
with low, warning, and critical classes for Waybar styling.

The dashboard intentionally contains only:

- airplane mode, idle inhibit, and Power Profile quick actions;
- Wayle's native system stats: CPU, RAM, disk, and CPU temperature; and
- Wayle's native notification history, including Do Not Disturb and Clear All.

Wi-Fi, media, audio, battery details, settings, and session power actions are
omitted because they are either separate Waybar panels or managed
declaratively. The agenda is not here either: the `custom/ycal` Waybar module
owns it and opens its own popup. The notification viewport fits roughly three
rows before it scrolls. Titles always show in full; long bodies can expand and
collapse instead of being permanently ellipsized.

### Network panel

The separate right-side network button opens Wayle's native network manager on
the monitor whose button was clicked. It scans, selects, and connects networks
without duplicating Wi-Fi in the dashboard.

An active connection offers **Info** and **QR**. Info includes the SSID, signal,
security, interface, profile, IP addresses, DNS, BSSID, band/frequency, channel,
mode, and link rate. The bridge resolves the access point against Wayle's live
list so a NetworkManager scan or roam cannot leave it using a stale object path.
QR produces a large inline share code from the saved profile and explains
unsupported Enterprise or Enhanced Open profiles.

### Media panel

The centre Waybar button toggles the Quickshell media panel (`themes/quickshell/`), a
resident QML shell that replaced the Rust GTK `media-panel`. One card shows
the active source: artwork, title, artist, album, a seekable progress bar,
transport controls, and the stream's volume. A sources dropdown lists every
currently playing or paused MPRIS source and retargets the card, and an output
dropdown routes the active stream through `audio-control stream-move`.

All live state comes from Quickshell's native Mpris and Pipewire services:
players, metadata, artwork, stream volumes, and the link graph that shows
which device a stream feeds. Duplicate publishers of one playback (an
mprisence browser bridge and the real player) merge by metadata and position;
absolute, idempotent commands fan out to every publisher in the merge, while
Next and Previous go to one. The only spawned processes are one-shot user
actions: `wayle-list output` for the route choices — the one source of
inactive-profile Speaker/Headphones rows — and `stream-move` for the
profile-switching, rollback-guarded move itself. Right-clicking the Waybar
button pauses every player over the same IPC.

Routing first scopes streams to the application's native identity, then uses
track titles or URLs where they uniquely distinguish streams. There is no
fallback that pairs browser entries and unrelated streams by list order.
Ambiguous/shared matches are disabled with an explanation in the Output section.
A paused browser source can be routed only when its own stream is identifiable;
starting playback may make that stream available. Tabs sharing one browser audio
stream cannot be routed independently through this panel.

The panel binds playback metadata only while open and moves an identified stream
by stable typed serial. It rechecks the selected source before
moving, reports helper failures in the panel, and does not silently substitute
another source or the system default. Inactive physical outputs may need an ALSA
profile switch that affects other playback; the expanded list warns about this.

After changing the routing helper or adding `Routing.js`, run `./switch.sh` so
the rebuilt helper and Home Manager's live module link are installed together.

The panel occupies a transparent monitor-sized layer surface, so Escape, a
second button press, or a click outside the card closes it. Position and
grouping refresh only while it is open.

The short title displayed directly in Waybar still comes from
`control-centre media-waybar`, including the configured 50-character limit
for ordinary text and 35-character limit for CJK-heavy text.

### Shared popup style

Media and audio use the same components from `themes/quickshell/`.
`niri/quickshell/style.nix` installs a local `shared/` directory of aliases
inside each popup's configuration root. All aliases point to the same
source files and palette; styles are not copied. This keeps imports
inside Quickshell's configuration sandbox without patching Quickshell:

| Source | Shared behavior |
| --- | --- |
| `themes/quickshell/PopupFrame.qml` | Charcoal background, 10% transparency, thin yellow outline, matching corner cuts |
| `themes/quickshell/ControlButton.qml` | Cyan text/red glyphs, 50%-transparent gray outlines, yellow/thicker outlines only on hover |
| `themes/quickshell/IconButton.qml` | Circular variant of the shared button; volume buttons can suppress their outlines |
| `themes/quickshell/StyledSlider.qml` | Shared track and handle presentation; each caller owns its range and backend |
| `themes/palette.nix` | One palette, including purple metadata text, exported to both shells |

Audio keeps its Pair/Output/Input layout and native services. Output volume
still spans 0–150%; sharing presentation does not inherit media's 100% range.
Popup sizes and placement remain specific to their content, not separate themes.

Run `./switch.sh` once to install the shared directory and palette links.
If an edit is not picked up by Quickshell's live reload, refresh both UIs:

```sh
systemctl --user restart quickshell-media.service audio-control-ui.service
```

### Audio panel

The audio button opens the unpatched Quickshell Pair/Output/Input popup described
under [`audio-control`](#audio-control). Playback remains in the media panel.

### External panel behavior

Wayle dropdowns requested through the external bar use a fullscreen transparent
host with the visible child positioned under its actual button. Dashboard and
network follow their slots at the right edge. The separate media panel is
centered, and Quickshell's audio popup uses its own transparent layer surface.
This makes outside-click dismissal work without the cross-client popup grab
that a GTK popover would require. Audio device refreshes stop while hidden.

### Local source deltas

Wayle presentation is kept out of source patches. Colours, spacing, card
shadows, scroll-area sizing, and the dashboard/network appearance live
in `themes/wayle/`, one partial per panel behind the fixed `index.scss` entry
file, and Wayle recompiles them on save.

One rule is deliberately not left to that file. Wayle's own `base/_index.scss`
gives every `window` the palette background, and the external dropdown host is
a layer surface covering the whole monitor, so without an override that host is
an opaque sheet over the output. Wayle compiles `themes/wayle/index.scss` from
outside the store and drops all of it when the file is missing or fails to
compile, which would black out the desktop on every panel press. The transparent
host therefore lives in `wayle-features.patch`; `index.scss` is still appended
after it and can restyle the panel.

| Source delta | Purpose |
| --- | --- |
| `wayle-features.patch` | Behavior that CSS cannot provide, plus the transparent click-away host: D-Bus panel requests, monitor-local hosting and placement, notification history/expansion, a dashboard trimmed to quick actions, system stats and notification history, and network Info/QR actions in a panel narrowed to a 420 px base. Generated directly against pristine Wayle v0.7.0, with no dependent patch order. Audio and Bluetooth customizations are no longer patched into Wayle. |

### Verifying changes

Workspace checks do not require live Bluetooth, audio, or MPRIS hardware:

```sh
cargo test --manifest-path scripts/Cargo.toml --locked \
  -p audio-control -p media-panel -p control-centre -p rofi-network --all-targets
```

The media tests cover deduplication, source naming, progress, volume conversion,
merged volume state, controls, artwork discovery, and cropping. Audio tests
cover the physical-port/profile bridge. The dashboard's seven-day parser has
focused tests in the source delta.

Wayle itself is verified from a clean v0.7.0 checkout after applying
`niri/wayle/wayle-features.patch`:

```sh
cargo check --locked -p wayle-shell --tests
```
