# Custom Scripts

This repository contains ten Rust packages used by the desktop configuration:

- `audio-control` — the headless Waybar status, Bluetooth-power, and profile-aware Wayle audio bridge
- `control-centre` — the long-running Waybar media-title stream
- `media-panel` — the multi-player MPRIS panel opened from Waybar's centre
- `network-manager` — the NetworkManager info and Wi-Fi QR bridge used by Wayle
- `ocr-screenshot` — screenshot OCR and clipboard integration
- `preview-panel` — the shared GTK4 preview surface
- [`rofi-clipboard`](#rofi-clipboard) — clipboard history and memos
- `rofi-filesearch` — searchable file launcher with previews
- [`waybar-timer`](#waybar-timer) — interactive Waybar countdown timer
- `webcam-crop` — on-demand virtual webcam cropper and supervisor

The Niri configuration applies one structure-and-behavior Wayle v0.7.0 delta
to supply the native [dashboard, network, and audio panels](#wayle-dashboard).

## Live desktop editing

After one Home Manager/NixOS switch, presentation files are linked directly
from `/home/raina/System`. They no longer need another system rebuild:

| What to edit | When it takes effect |
| --- | --- |
| `niri/config.kdl` | Niri reloads it on save |
| `niri/rofi/config.rasi`, `themes/rofi-*.rasi` | Next Rofi launch |
| `themes/waybar/top.jsonc`, `bottom.jsonc` | Waybar restarts automatically |
| `themes/waybar/waybar.css` | Waybar reloads CSS automatically |
| `themes/preview-panel.css` | Preview panel hot-reloads |
| `scripts/media-panel/src/style.css` | Open media panel hot-reloads |
| `themes/wayle/*.scss` | Wayle recompiles the override on save |

Rust, Nix module, package, service, and kernel changes still require a rebuild.
Each Rust package now hashes only Cargo sources, so changing one UI stylesheet
or sibling utility no longer invalidates every desktop helper.

## audio-control

`scripts/audio-control` has no visible UI. Wayle owns the Bluetooth and audio
presentation; this helper supplies only behavior that Wayle's services do not
natively expose.

The Waybar audio button uses `audio-control status` for its JSON. Its glyph
tracks the default output while Bluetooth is off, changes when the adapter is
on or connected, and leaves the full output/input/device state in the tooltip.
Right-click runs `bluetooth-power toggle`.

Left-click asks Wayle to open its native four-tab panel:

| Tab | Native Wayle content |
| --- | --- |
| Pair | Bluetooth power, scan, pair, connect, disconnect, and forget |
| Output | Default volume/mute plus distinct output devices and physical ports |
| Input | Default volume/mute plus microphones and their ports |
| Play | Per-application volume and output routing |

The panel is two-thirds of Wayle's original audio width. Device rows favor the
distinguishing label—such as **Speaker**, **Headphones**, or a USB model—while
keeping the full description in a tooltip. Plugged headphones remain a
clickable output instead of being collapsed into Speaker. Application streams
can be adjusted independently and routed without changing the system default.

Pairing takes more than the row. Wayle's device rows call `Device1.Connect`,
which opens no bonding request: BlueZ brings the link up and reports the device
as connected, leaving whatever bond the device then asks for to whichever agent
holds BlueZ's default-agent role. That is enough for a speaker, but a keyboard —
which has to be shown a six-digit passkey to type back — ends up listed as
connected while nothing it types arrives. `wayle-features.patch` makes an
unpaired row pair first, so the bonding request carries Wayle's own agent and
the passkey reaches its pairing card whoever else has registered one, and
trusts the device before connecting so its later reconnections raise no
authorization prompt nothing is showing. `audio-control bluetooth-pair` does
the same from a terminal for pairing without the panel.

Wayle already handles live devices, sliders, Bluetooth, and stream routing. The
`WAYLE_AUDIO_HELPER` bridge is used only for mutually exclusive ALSA profiles:
it keeps Speaker and Headphones available as stable choices even when selecting
one causes PipeWire to replace the other sink. It revalidates a choice before
switching, prefers a compatible profile that retains microphone ports, and
attempts rollback after a failed switch.

### Commands

```text
audio-control status
audio-control bluetooth-power [on|off|toggle]
audio-control wayle-list <output|input>
audio-control wayle-set-default <output|input> <key>
audio-control bluetooth-devices
audio-control bluetooth-scan
audio-control bluetooth-pair <address|name>
```

`wayle-list` and `wayle-set-default` form a NUL-delimited machine interface for
Wayle and are not intended as an interactive picker. `bluetooth-pair` is
interactive: it prints the passkey to type on the device, or asks on stdin for
one the device displays, and accepts either an address or a distinctive part of
a device's name.

### Development checks

From the repository root:

```sh
cargo fmt --manifest-path scripts/Cargo.toml --package audio-control -- --check
cargo test --manifest-path scripts/Cargo.toml --package audio-control --locked
cargo clippy --manifest-path scripts/Cargo.toml --package audio-control --locked -- -D warnings
```

Tests cover device/port identity, compact labels, volume bounds, stream routing,
inactive-profile Speaker/Headphones selection, delayed device creation, stale
targets, server rejection, and rollback.

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
`themes/preview-panel.css`. Home Manager links that file to
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
- the next seven days from `~/.cache/waybar-ycal/events.json`; and
- Wayle's native notification history, including Do Not Disturb and Clear All.

Wi-Fi, media, audio, battery details, settings, session power actions, and system
telemetry are omitted because they are either separate Waybar panels or managed
declaratively. Timed calendar events put their `HH:MM-HH:MM` range above the
title. The notification viewport fits roughly three rows before it scrolls.
Titles always show in full; long bodies can expand and collapse instead of
being permanently ellipsized.

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

The centre Waybar button opens `media-panel`. A custom panel is retained here
because Wayle's native MPRIS dropdown selects one source, while this desktop
needs every currently playing or paused source at once.

Each compact card shows source, state, artwork, title, artist, album, progress,
elapsed/total time, transport controls, and the player's own volume when MPRIS
publishes it. The panel shows up to four cards before scrolling. It occupies a
transparent monitor-sized layer surface, so Escape, a second button press, or a
click outside the visible panel closes it.

The reader filters stopped players, `playerctld`, and `kdeconnect`. Duplicate
publishers of the same playback are merged by metadata and position; absolute,
idempotent commands go to every publisher represented by the card. Next and
Previous go to one publisher to avoid double-skipping. Seeking uses the typed
`mpris:trackid` object path and falls back to a relative seek when needed.
Volume changes are clamped to 0–100% and sent to every merged publisher.

Remote artwork is fetched once and cached. For local files, `media-panel`
checks nearby cover filenames and embedded pictures through `lofty`.
`media-panel players` prints the panel's current bus view, and
`MEDIA_PANEL_TRACE=1` reports command and refresh timing. Right-clicking the
Waybar button runs `media-panel pause-all`.

The short title displayed directly in Waybar still comes from
`control-centre media-waybar`, including the configured 50-character limit
for ordinary text and 35-character limit for CJK-heavy text.

### Audio panel

The audio button opens Wayle's native Pair/Output/Input/Play panel described
under [`audio-control`](#audio-control). The Rust helper remains headless and
is invoked only for status, Bluetooth power, pairing, and profile-aware device
choices.

### External panel behavior

Wayle dropdowns requested through the external bar use a fullscreen transparent
host with the visible child positioned under its actual button. Media is
centered; dashboard, network, and audio follow their slots at the right edge.
This makes outside-click dismissal work without the cross-client popup grab
that a GTK popover would require. Closing audio or media also propagates the
visibility change so background refresh work stops.

### Local source deltas

Wayle presentation is kept out of source patches. Colours, spacing, card
shadows, scroll-area sizing, and the dashboard/network/audio appearance live
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
| `wayle-features.patch` | Behavior that CSS cannot provide, plus the one structural style that must not depend on the live stylesheet (the transparent click-away host): D-Bus panel requests, monitor-local click-away hosting and placement, notification history/expansion, the seven-day agenda, network Info/QR actions in a panel narrowed to a 420 px base (Wayle's own is 382; the QR view needed more, 520 was excessive), audio tabs/routing, compact device labels, inactive-profile switching, and
pairing an unpaired device with `Device1.Pair` before connecting it. This is generated directly against pristine Wayle v0.7.0, with no dependent patch order. |
| `mprisence-position.patch` | Prevent browser positions from being clamped backward after replay or a backward seek. |

### Verifying changes

Workspace checks do not require live Bluetooth, audio, or MPRIS hardware:

```sh
cargo test --manifest-path scripts/Cargo.toml --locked \
  -p audio-control -p media-panel -p control-centre -p rofi-network --all-targets
```

The media tests cover deduplication, source naming, progress, volume conversion,
merged volume state, controls, artwork discovery, and cropping. Audio tests
cover the native picker's profile bridge. The dashboard's seven-day parser and
the Wayle audio helper functions have focused tests in the source delta.

Wayle itself is verified from a clean v0.7.0 checkout after applying
`niri/wayle/wayle-features.patch`:

```sh
cargo check --locked -p wayle-shell --tests
```
