# Custom Scripts

This repository contains nine Rust packages used by the desktop configuration:

- `audio-control` — headless Waybar status, Bluetooth pairing agent, and physical audio port/profile bridge
- `media-control` — headless media grouping, conservative stream matching, artwork, and video-author metadata
- `network-manager` — only the detail/secret/QR and legacy-WEP gaps in Quickshell’s native networking API
- `ocr-screenshot` — screenshot OCR and clipboard integration
- `preview-panel` — the shared GTK4 preview surface
- [`rofi-clipboard`](#rofi-clipboard) — clipboard history and memos
- `rofi-filesearch` — searchable file launcher with previews
- [`waybar-timer`](#waybar-timer) — interactive Waybar countdown timer
- `webcam-crop` — on-demand virtual webcam cropper and supervisor

The Niri configuration uses **AGS 2.3/Astal GTK4 only for media**. The dashboard,
network, audio, and notification popups remain in Quickshell. `media-panel` is a
Nix-packaged launcher, not another Rust GUI.

## Live desktop editing

After one Home Manager/NixOS switch, presentation files are linked directly
from `/home/raina/System`. They no longer need another system rebuild:

| What to edit | When it takes effect |
| --- | --- |
| `niri/config.kdl` | Niri reloads it on save |
| `niri/rofi/config.rasi`, `themes/rofi/*.rasi` | Next Rofi launch |
| `themes/quickshell/*.qml` | Quickshell reloads dashboard, network, and notifications on save; shared controls also affect audio |
| `niri/panels/media-control/*.js`, `style.css` | Live-linked AGS media sources; restart `ags-media.service` to apply edits |
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
graphical session and Waybar wants it. For development, stop the resident UI
first and run the installed configuration with a locally built helper:

```sh
cargo build --manifest-path scripts/Cargo.toml --package audio-control --locked
systemctl --user stop audio-control-ui.service
AUDIO_CONTROL_HELPER="$PWD/scripts/target/debug/audio-control" \
  QT_QUICK_CONTROLS_STYLE=Basic quickshell --config audio-control
# In another terminal:
quickshell ipc --config audio-control call panel toggle eDP-1
```

After stopping the manual instance, restart `audio-control-ui.service`. Shared
QML aliases and generated colors belong to the installed configuration, not
inside `niri/audio-control/` in the repository.

The audio panel uses the canonical `devices` JSON protocol; the old
NUL-delimited adapter and its CLI aliases are gone. Physical port/profile
switching through `set-default` remains in the helper because Quickshell 0.3.1
does not expose it. The separate media panel has no output selector or
per-stream routing command.

### Development checks

From the repository root:

```sh
cargo fmt --manifest-path scripts/Cargo.toml --package audio-control -- --check
cargo test --manifest-path scripts/Cargo.toml --package audio-control --locked
cargo clippy --manifest-path scripts/Cargo.toml --package audio-control --locked -- -D warnings
```

Tests cover device/port identity, compact labels, status percentages, pairing
validation, inactive-profile Speaker/Headphones selection, delayed device
creation, stale targets, server rejection, and rollback. Media grouping and conservative
stream matching are tested separately in `scripts/media-control/` and
`niri/panels/media-control/`.

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

## Desktop panels

### Quickshell dashboard

`Mod+N` and the right-most Waybar battery button open the Quickshell dashboard
in a monitor-local layer-shell window near the top-right of the screen. The
button continues to show battery level and charging state, with low, warning,
and critical classes for Waybar styling.

The dashboard uses Quickshell's shared frame, palette, and buttons. Its
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


### Media panel (AGS/Astal GTK4)

The centre Waybar button toggles the resident AGS media panel in
`niri/panels/media-control/`. Its JavaScript is organized into three files:

- `frontend.js`: application entry, GTK window/widgets, theme, artwork, and credits.
- `backend.js`: native Astal services, Rust helper transport, and model coordination.
- `logic.js`: GTK-independent timelines, request validation/queues, and formatting.

Custom media policy and metadata discovery remain in Rust; `backend.js` is the
integration layer, not a second implementation of that policy.

The 460px card retains artwork, title, artist/author, album,
seekable progress, transport controls, volume, and the playing/paused source
selector. **There is no Output section or per-stream routing.** Physical output
selection remains in the separate Quickshell audio panel and
`audio-control set-default`.

Astal MPRIS owns player discovery, metadata, transport, and seeking. Volume is
**0–100%, MPRIS-first**: use the selected player's writable volume when available,
otherwise use Astal WirePlumber only for a conservatively identified stream.
Ambiguous/shared streams are not guessed, and the system default output is never
used as a media-volume fallback. Global audio output volume still allows 150%.

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

`niri/panels/default.nix` links `niri/panels/media-control/` to
`~/.config/ags/media`, including `frontend.js`, `backend.js`, `logic.js`, and `style.css`.
It does not install generated children into that live link. The palette is
serialized from `themes/palette.nix` to the separate
`~/.config/ags/media-palette.json`; the frontend generates GTK `@define-color`
rules programmatically from this JSON without writing generated CSS under the
live link. Quickshell still gets its generated `Colors.qml` from the same palette.

The Niri-only `ags-media.service` runs `media-panel run`, restarts on failure,
and is wanted by Waybar and the graphical session. The launcher uses the pinned
AGS 2.3 CLI: `ags run --gtk4 -- <entry>`. Its overridden AGS package includes GTK4,
Graphene, GTK4 layer-shell, Astal MPRIS, and Astal WirePlumber in the generated
runtime wrapper; no manual `GI_TYPELIB_PATH` supplement is needed.
`niri/panels/media-control/media.nix` also builds a combined native GdkPixbuf loader cache for
standard formats, librsvg, and `webp-pixbuf-loader`, preserving PNG/JPEG/SVG while
adding WebP artwork support. AGS's wrapper sets `GDK_PIXBUF_MODULE_FILE` to this
cache rather than the default librsvg cache that omits WebP; no JavaScript
decoder or user environment override is needed. The frontend must call `App.start` with
`instanceName: "media-panel"`; this CLI has no `run --instance` flag. Requests use
`ags request --instance media-panel -- <JSON>` with this wire format:

```json
{"command":"toggle","monitor":"eDP-1"}
```

`command` is one of `status`, `toggle`, `togglePauseAll`, `cycleMedia`, `pauseAll`,
or `close`; `monitor` is optional. `status` returns Waybar JSON on stdout. The
launcher builds requests with `jq --arg`, never shell-evaluated JSON.

```sh
media-panel status
media-panel toggle eDP-1
media-panel togglePauseAll
media-panel cycleMedia
media-panel pauseAll
media-panel close
systemctl --user restart ags-media.service  # Apply live JS/CSS edits
```

`MEDIA_PANEL_CONFIG` selects the entry file, defaulting to
`$XDG_CONFIG_HOME/ags/media/frontend.js` (or `~/.config/ags/media/frontend.js`).
`MEDIA_PANEL_PALETTE` is a JSON file path; `MEDIA_CONTROL` and `QUICKSHELL`
are executable paths, not shell commands. The launcher supplies store-backed
defaults, and the service explicitly supplies these values plus
`GDK_BACKEND=wayland`. Graphical-session Wayland and D-Bus variables are inherited.

Rebuild after changing Nix/service wiring or the Rust helper; JS/CSS changes
only require restarting the media service. The legacy `quickshell-media.service`
and `quickshell --config media` names are intentionally retained for the
**non-media** desktop panels. Quickshell no longer exports a `media` IPC target.

### Shared popup style

The dashboard, network, audio, and notification popups share QML components from
`themes/quickshell/`. The AGS media card recreates the same presentation in GTK4
rather than loading QML; only palette values are shared across toolkits.
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
| `themes/palette.nix` | One palette exported as Quickshell colors and AGS media JSON |

Audio uses Bluetooth/Audio tabs and native services. Output volume
still spans 0–150%; the separate media panel is limited to 100%.
Popup sizes and placement remain specific to their content, not separate themes.

After installing the shared directory and palette links through Home Manager,
if an edit is not picked up by Quickshell's live reload, refresh its UIs:

```sh
systemctl --user restart quickshell-media.service audio-control-ui.service
```

### Audio panel

The audio button opens the Quickshell Bluetooth/Audio popup described
under [`audio-control`](#audio-control). Playback remains in the media panel.

### External panel behavior

Dashboard, network, media, and audio use transparent monitor-sized layer
surfaces with a visible card. The backdrop consumes outside clicks;
Escape and a second button press dismiss the card. Opening network closes the
dashboard and invokes `media-panel close` once through a Quickshell `Process`;
network status polling never spawns a media command. Opening media closes network
through `QUICKSHELL ipc --config media call network close`. The audio popup stays
independent. Native notifications use a small non-focus-taking layer surface
instead, so toasts do not grab keyboard input.

Network scanning, dashboard metrics, and audio device refreshes stop while
the corresponding panel is hidden. Notifications remain event-driven, and the
media badge requests metadata on demand even while its card is closed.

### Verifying changes

These protocol/controller checks use private D-Bus sessions and temporary/fake
data, not the desktop notification owner or real network mutations:

```sh
python3 themes/quickshell/tests/native_notifications.py
python3 themes/quickshell/tests/network_controller.py
python3 themes/quickshell/tests/panel_handoff.py
cargo test --manifest-path scripts/Cargo.toml --locked \
  -p audio-control -p media-control -p rofi-network --all-targets
cargo build --manifest-path scripts/Cargo.toml --locked -p rofi-network -p media-control


```

The Rust tests for `audio-control` and `media-control` each live in that
crate's `src/tests.rs`, registered as the Cargo test target `tests`. This keeps
unit and real-binary CLI/protocol coverage together. The commands above run
both suites; use `--test tests` to select only the consolidated target.

The focused media frontend tests cover action queues and stale requests,
sampled playback timelines, seeking, and badge formatting in `logic.js`. They use Node.js,
need no npm dependencies, and do not load GTK/GI:

```sh
node --test niri/panels/media-control/tests/*.test.js
```

Rust `media-control` tests cover duplicate/source policy, conservative stream
matching, native action plans, metadata caching, and the JSON bridges. Media
frontend tests live with the JavaScript modules in `niri/panels/media-control/`, not
under Quickshell. `themes/quickshell/tests/panel_handoff.py` loads the real shell with fake panel
components on a private D-Bus, proving network-open cross-close without media
subprocesses during status polling.

The Qt Quick tests in `themes/quickshell/tests/` cover network ordering/status,
notification row sizing, and native action controls. They use installed shared
component aliases and palette (or a staged equivalent). Do not activate a new
generation or send real playback/network commands just to run isolated checks.
