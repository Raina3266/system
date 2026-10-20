# Custom Scripts

This repository contains nine Rust packages used by the desktop configuration:

- `audio-control` — headless Waybar status, Bluetooth pairing agent, and physical audio port/profile bridge
- `media-control` — headless media grouping, stream matching, artwork, and video-author metadata
- `network-manager` — only the detail/secret/QR and legacy-WEP gaps in Quickshell’s native networking API
- `ocr-screenshot` — screenshot OCR and clipboard integration
- `preview-panel` — the shared GTK4 preview surface
- [`rofi-clipboard`](#rofi-clipboard) — clipboard history and memos
- `rofi-filesearch` — searchable file launcher with previews
- [`waybar-timer`](#waybar-timer) — interactive Waybar countdown timer
- `webcam-crop` — on-demand virtual webcam cropper and supervisor

The Niri configuration uses Quickshell for the dashboard, network,
media, audio, and notification popups.

## Live desktop editing

After one Home Manager/NixOS switch, presentation files are linked directly
from `/home/raina/System`. They no longer need another system rebuild:

| What to edit | When it takes effect |
| --- | --- |
| `niri/config.kdl` | Niri reloads it on save |
| `niri/rofi/config.rasi`, `themes/rofi/*.rasi` | Next Rofi launch |
| `themes/quickshell/*.qml` | Quickshell reloads the resident panels on save; shared controls affect the popups |
| `themes/waybar/top.jsonc`, `bottom.jsonc` | Waybar restarts automatically |
| `themes/waybar/waybar.css` | Waybar reloads CSS automatically |
| `themes/rofi/preview-panel.css` | Preview panel hot-reloads |

| `niri/audio-control/*.qml` | Quickshell reloads the audio panel on save |

Rust, Nix module, package, service, and kernel changes still require a rebuild.
Each Rust package now hashes only Cargo sources, so changing one UI stylesheet
or sibling utility no longer invalidates every desktop helper.

## audio-control

`niri/audio-control` uses the same pinned Quickshell package as the resident
panels. `scripts/audio-control` remains headless.
The `audio-control-ui` user service runs only in Niri.

The Waybar audio button uses `audio-control status` for its JSON. Its glyph
tracks the default output while Bluetooth is off, changes when the adapter is
on or connected, and leaves the full output/input/device state in the tooltip.
Middle-click toggles the default adapter through Quickshell's native Bluetooth API.

Left-click opens the popup on the clicked monitor. Escape, the close button,
another click on the same Waybar button, or clicking outside dismisses it.

| Tab | Content |
| --- | --- |
| Bluetooth | Bluetooth power, scan, pair, connect, disconnect, forget, and battery levels |
| Audio | Output controls in the upper half; input controls in the lower half, each with volume/mute and device/port selection |

Volume percentages sit beside their sliders on the same row. There is no Play
tab; playback stays in the existing media panel. Output volume runs from
**0 to 150%**, retaining amplification above 100%. Input remains
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
  shows its prompts/passkeys in the Bluetooth tab, accepts PIN/passkey input, then
  trusts and connects the device. For numeric comparison, compare the displayed
  six-digit passkey and send `yes` to confirm or `no` to reject. Closing the
  panel or leaving Bluetooth cancels pending pairing and stops discovery. The helper
  does not compete for BlueZ's session-wide default-agent role.

### Commands

```text
audio-control status
audio-control bluetooth-power [on|off|toggle]
audio-control devices <output|input>
audio-control set-default <output|input> <key>
audio-control stream-move <key|serial:N|node:N|name> <destination-key>

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

The audio and media panels share the canonical `devices` JSON protocol;
the old NUL-delimited adapter and its CLI aliases are gone. For `stream-move`,
prefer `serial:N` (a PipeWire object serial) or `node:N` (a node ID). The media
panel uses stable `serial:N` selectors only; legacy names are accepted only
when unambiguous. Physical port/profile switching remains in the helper because
Quickshell 0.3.1 does not expose it.

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

## Quickshell dashboard

`Mod+N` and the right-most Waybar battery button open the Quickshell dashboard
in a monitor-local layer-shell window near the top-right of the screen. The
button continues to show battery level and charging state, with low, warning,
and critical classes for Waybar styling.

The dashboard uses the media panel's shared frame, palette, and buttons. Its
three sections are:

- **Power:** battery state and power profiles through Quickshell's native
  UPower/PowerProfiles services, airplane mode through NetworkManager and
  Bluetooth bindings, and idle inhibition through Quickshell's Wayland
  `IdleInhibitor`;
- **System parameters:** CPU and RAM from `/proc`, temperature from the kernel
  thermal interface, and root filesystem usage from `df`. Quickshell 0.3.1 has
  no general system-monitor service, so procfs/sysfs use `FileView` and `df`
  supplies filesystem capacity; and
- **Notifications:** Quickshell's native `NotificationServer` owns
  `org.freedesktop.Notifications`. The dashboard and popup stack share the same
  event-driven history, DND, dismiss, clear-all, and native action handlers.

Only system metrics are polled while the dashboard is open. Notifications
update directly from native service signals. The history viewport shows
roughly three rows and scrolls for more. Wi-Fi details,
media, audio, the agenda, settings, and session power actions remain in their
existing Waybar panels or declarative system controls.

### Notification popups

Quickshell displays up to five newest-first popup cards near the top-right of
the first available monitor. They reuse `PopupFrame`, `ControlButton`,
`IconButton`, and the dashboard's `NotificationEntry`, including app images,
plain-text bodies, close controls, and native action buttons. Popups do not
take keyboard focus.

The default timeout is eight seconds; explicit application timeouts are
respected. Hover pauses expiry, zero-timeout and critical notifications remain
until dismissed, and DND hides popups while retaining non-transient history.
Expired entries stay in the dashboard without stale actions. Transient
notifications are never archived, and client-requested closes remove entries.

The latest 200 entries and DND state are saved through native `FileView` to
`$XDG_STATE_HOME/quickshell/notifications.json` (normally
`~/.local/state/quickshell/notifications.json`). The user service creates its
private state directory. History survives QML reloads and process restarts;
application actions cannot survive a full process restart.


Earlier daemon history can be imported once as archived text before the
handoff; subsequent notifications go directly to Quickshell.

### Network panel

The network button opens the Quickshell panel on the monitor whose button was
clicked. It shares the same frame, controls, colors, and outside-click/Escape
dismissal as the other popups. Scanning is enabled only while this panel is open.

Native `Quickshell.Networking` supplies device/network models, Wi-Fi software
and hardware state, signal/security, saved profiles, open/OWE/personal activation,
PSK prompts/retries, disconnect, and Forget with confirmation. Rows put active
connections first, then saved profiles, and sort strength by icon bucket to
avoid reshuffling for one-percent changes. Ethernet takes priority in Waybar.
The badge is native IPC JSON rather than a Rust inventory/status process.

Saved profiles can be selected by UUID. Quickshell 0.3.1 cannot target a chosen
profile when supplying a replacement PSK, so ambiguous per-profile credential
edits go to **Advanced** (`nm-connection-editor`) rather than modifying another
profile. Preconfigured enterprise/hidden profiles remain usable; creating those
profiles also belongs in Advanced, not a misleading generic password prompt.

**Info** shows SSID, signal/security, active profile ID/UUID, interface/MAC,
IPv4/IPv6, gateways, DNS, BSSID, frequency/band/channel/mode, and link rate.
Passwords are queried only after **Reveal password**, not on opening Info.
**QR** explicitly requests a large share code for supported saved open/WEP/WPA
profiles. Hidden flags and QR delimiters are handled; enterprise and Enhanced
Open credentials are rejected with an explanation.

Quickshell 0.3.1 does not expose IP/radio details, saved-secret retrieval, a QR
encoder, or initial WEP credentials. Only these gaps remain in Rust:

```text
network-manager info <interface> <expected-ssid>
network-manager secret <interface> <expected-ssid> <profile-uuid> <bssid>
network-manager qr <interface> <expected-ssid> <profile-uuid> <bssid>
network-manager connect-wep <interface> <expected-ssid>
```

The helper no longer enumerates networks or implements status, scanning,
ordinary connection, disconnect, or forget. `info` never requests secrets.
WEP input is a JSON password record over stdin; QR payloads also go through
stdin, never process arguments or temporary files. Interface, active profile
UUID, SSID, connection path, and BSSID are validated before/after reads; stale
UI responses are discarded after connection/selection/visibility changes.
Secret/QR requests pin the identity obtained by a preceding nonsecret Info
read and reject a changed profile/AP before querying any secret. Completed
secret-bearing stdout lines are not retained in a collector after dispatch.


### Media panel

The centre Waybar button toggles the Quickshell media panel (`themes/quickshell/`), a
resident QML shell. One card shows
the active source: artwork, title, artist, album, a seekable progress bar,
transport controls, and the stream's volume. A sources dropdown lists every
currently playing or paused MPRIS source and retargets the card, and an output
dropdown routes the active stream through `audio-control stream-move`.

Quickshell's native MPRIS and PipeWire services own player discovery, metadata,
transport, seeking, volume, and the native audio graph. QML defines the UI and
bridges native snapshots to the separate Rust `media-control` helper; it does
not implement browser filtering, duplicate detection, or stream-ownership rules.
The headless helper reuses the former Rust panel's conservative duplicate/source
policy without restoring its GTK UI or duplicating Quickshell's D-Bus backend.
It returns native action plans: absolute play/pause targets are grouped, while
Next and Previous address one publisher. Middle-clicking the Waybar button
toggles all playback; right-click cycles the displayed source selection.

`media-control model` is a resident JSONL snapshot/action planner over stdin/stdout.
`media-control artwork <xesam-url|path>` reuses local cover discovery and embedded
art extraction. `media-control credit` accepts JSON track metadata on stdin and
owns video-ID parsing, YouTube/Bilibili author lookups, fallback credits, and a
bounded private cache under `$XDG_CACHE_HOME/media-control/`. Lookups use curl
with a ten-second timeout; opening a supported video sends its video ID to that
site's public metadata endpoint. No credentials or browser cookies are sent.

`audio-control` stays separate: `devices output` supplies inactive-profile
Speaker/Headphones choices, and `stream-move` handles profile switching and
rollback. QML binds stream volume and current output through native PipeWire.

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

After changing the Rust media helper or its QML bridge, run `./switch.sh` so
the rebuilt helper and Home Manager's live module links are installed together.

The panel occupies a transparent monitor-sized layer surface, so Escape, a
second button press, or a click outside the card closes it. Position animation
runs only while it is open; native snapshot analysis also runs on Waybar status
requests while closed.

Waybar requests `media status` over Quickshell IPC once per second. The native
MPRIS selection is shared with the card, including while it is closed. Full
tooltips, playing/paused/inactive classes, and the 50-character/35-CJK title
limits are preserved in `MediaStatus.js`. The former `control-centre` crate
and active-player D-Bus bridge have been removed.

### Shared popup style

The dashboard, network, media, audio, and notification popups share components from `themes/quickshell/`.
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

Audio uses Bluetooth/Audio tabs and native services. Output volume
still spans 0–150%; sharing presentation does not inherit media's 100% range.
Popup sizes and placement remain specific to their content, not separate themes.

Run `./switch.sh` once to install the shared directory and palette links.
If an edit is not picked up by Quickshell's live reload, refresh both UIs:

```sh
systemctl --user restart quickshell-media.service audio-control-ui.service
```

### Audio panel

The audio button opens the Quickshell Bluetooth/Audio popup described
under [`audio-control`](#audio-control). Playback remains in the media panel.

### External panel behavior

Dashboard, network, media, and audio use transparent monitor-sized layer
surfaces with a visible shared card. The backdrop consumes outside clicks;
Escape and a second button press dismiss the card. Opening network closes the
other resident cards. Native notifications use a small non-focus-taking layer
surface instead, so toasts do not grab keyboard input.

Network scanning, dashboard metrics, and audio device refreshes stop while
the corresponding panel is hidden. Notifications remain event-driven, and the
media badge requests metadata on demand even while its card is closed.

### Verifying changes

These protocol/controller checks use private D-Bus sessions and temporary/fake
data, not the desktop notification owner or real network mutations:

```sh
python3 themes/quickshell/tests/native_notifications.py
python3 themes/quickshell/tests/network_controller.py
cargo test --manifest-path scripts/Cargo.toml --locked \
  -p audio-control -p media-control -p rofi-network --all-targets
cargo build --manifest-path scripts/Cargo.toml --locked -p rofi-network

```

Rust `media-control` tests cover the reused duplicate policy, conservative
routing, native action plans, metadata caching, and the JSON bridges. Bridge
tests run Quickshell against fake native models on a private D-Bus session when
Quickshell and the repository QML are available; no real media is controlled.
The QML tests in `themes/quickshell/tests/` cover media-title/CJK formatting,
network ordering/status, notification row sizing, and native action controls.
UI tests use the installed shared component aliases and palette (or a staged equivalent).
