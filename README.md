# Custom Scripts

This repository contains nine Rust packages used by the desktop configuration:

- `audio-control` — headless audio compatibility policy, request-local PIN/passkey pairing, and HID++ battery publication
- `media-control` — headless media grouping, conservative stream matching, artwork, and video-author metadata
- `network-manager` — QR-only Wi-Fi share-code encoding; native network information, secrets, and WEP settings stay in GJS/libnm
- `ocr-screenshot` — screenshot OCR and clipboard integration
- `preview-panel` — the shared GTK4 preview surface
- [`rofi-clipboard`](#rofi-clipboard) — clipboard history and memos
- `rofi-filesearch` — searchable file launcher with previews
- [`waybar-timer`](#waybar-timer) — interactive Waybar countdown timer
- `webcam-crop` — on-demand virtual webcam cropper and supervisor

The Niri configuration uses **AGS 2.3/Astal GTK4 for media, audio, dashboard,
notifications, and network**. The panel commands are Nix-packaged launchers,
not additional Rust GUIs. The dashboard is TypeScript-only, with its application,
metrics, notifications, styles, and tests directly in `scripts/dashboard/`.

## Live desktop editing

After one Home Manager/NixOS switch, presentation files are linked directly
from `/home/raina/System`. They no longer need another system rebuild:

| What to edit | When it takes effect |
| --- | --- |
| `niri/config.kdl` | Niri reloads it on save |
| `niri/rofi/config.rasi`, `themes/rofi/*.rasi` | Next Rofi launch |
| `scripts/dashboard/*.ts`, `style.css` | Live-linked AGS dashboard/notification sources; restart `ags-dashboard.service` to apply edits |
| `scripts/network-manager/astal/*.js`, `style.css` | Live-linked AGS network sources; restart `ags-network.service` to apply edits |
| `scripts/media-control/astal/*.js`, `style.css` | Live-linked AGS media sources; restart `ags-media.service` to apply edits |
| `themes/waybar/top.jsonc`, `bottom.jsonc` | Waybar restarts automatically |
| `themes/waybar/waybar.css` | Waybar reloads CSS automatically |
| `themes/rofi/preview-panel.css` | Preview panel hot-reloads |
| `scripts/audio-control/astal/*.js`, `style.css` | Live-linked AGS audio sources; restart `audio-control-ui.service` to apply edits |

Rust, Nix module, package, service, and kernel changes still require a rebuild.
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
accepts an optional monitor. Startup uses `ags run --gtk4 -- <entry>`, not a
`run --instance` flag. The frontend's own `AudioApplication` subclasses native
`Astal.Application`, sets `instance_name: "audio-panel"`, and uses `acquire_socket()`
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
it. For development, stop the resident UI first and run the installed
configuration with a locally built helper:

```sh
cargo build --manifest-path scripts/Cargo.toml --package audio-control --locked
systemctl --user stop audio-control-ui.service
GDK_BACKEND=wayland AUDIO_CONTROL_HELPER="$PWD/scripts/target/debug/audio-control" \
  audio-panel run
# In another terminal:
audio-panel toggle eDP-1
```

After stopping the manual instance, restart `audio-control-ui.service`. For
service/startup diagnostics:

```sh
systemctl --user status audio-control-ui.service
journalctl --user -u audio-control-ui.service -b -n 100 --no-pager
```

`niri/panels/default.nix` live-links the audio directory to `~/.config/ags/audio`.
The generated palette stays outside that link at `~/.config/ags/audio-palette.json`,
so Home Manager never writes generated child links into the repository. Palette
changes trigger a service restart on the next generation switch. JavaScript/CSS
edits need only `systemctl --user restart audio-control-ui.service`; changes to
Nix wiring or the Rust helper require a rebuild.

`AUDIO_PANEL_CONFIG` defaults to `$XDG_CONFIG_HOME/ags/audio/frontend.js` (or
`~/.config/ags/audio/frontend.js`). `AUDIO_PANEL_PALETTE` selects the JSON palette;
`AUDIO_CONTROL_HELPER` is an executable path, not a shell command. The launcher
supplies store-backed palette/helper defaults, and the service explicitly sets
the installed config/palette paths, helper, and `GDK_BACKEND=wayland`.
Graphical-session Wayland and D-Bus variables are inherited.
`niri/panels/audio.nix` supplies GTK4, Graphene, GTK4 layer-shell,
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
is `niri/panels/dashboard.nix`. The `ags-dashboard` user service owns the
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
switch before restarting the new service: the updated runtime, live links, and
notification-bus ownership cannot be installed by a service restart alone.

### Network panel (AGS/Astal GTK4)

`network-panel` opens the network card on the monitor whose Waybar button was
clicked. It retains the bounded 420×520 layout, chamfered translucent frame,
colors, controls, profile chooser, and outside-click/Escape dismissal. Network
scanning runs only while the panel is open and Wi-Fi is enabled.

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

Rows put active connections first, then saved profiles, and sort strength by
icon bucket to avoid reshuffling for one-percent changes. Ethernet takes
priority in Waybar. Profiles are selected by UUID, not their display names.
Open, OWE, WEP and personal WPA/WPA2/WPA3 connections use native libnm settings.
Ambiguous per-profile credential updates and enterprise/hidden configuration
remain in **Advanced** (`nm-connection-editor`). Forget confirms deletion of
all matching saved profiles and disconnects the network.

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
`niri/panels/network.nix`, with the runtime typelibs and JSON palette generated
from `themes/palette.nix`; `niri/panels/default.nix` provides the resident
`ags-network.service` and live Home Manager links, with palette restart
triggers. `scripts/Cargo.toml` and `scripts/packages.nix` integrate the
workspace and packages; Cargo's package remains `rofi-network` to preserve
`Cargo.lock`, while the installed helper executable is `network-manager`.

```sh
network-panel run
network-panel status
network-panel toggle eDP-1
network-panel close
systemctl --user restart ags-network.service  # Apply live JS/CSS edits after installation
```

The native application owns the `network-panel` AGS instance. Requests are
JSON; monitor names are transported without shell interpolation. Status
polling is read-only and never dismisses panels or mutates network state.

All four panel launchers compile with `ags bundle --gtk 4` into distinct files
under `$XDG_RUNTIME_DIR/ags-panels/`, then execute their packaged `panel-gjs`
wrapper. AGS 2.3's `run` command shares one `ags.js` output across instances,
which can execute another panel's code during concurrent startup; the wrapper
preserves each panel's native GI environment and GTK4 layer-shell preload,
while `XDG_RUNTIME_DIR` itself stays unchanged so Astal IPC sockets remain
accessible. The network application checks native registration before holding
its event loop, so registration failure exits instead of serving an
uninitialized badge forever.

Environment overrides:

- `NETWORK_PANEL_CONFIG`: defaults to
  `${XDG_CONFIG_HOME:-$HOME/.config}/ags/network/frontend.js`.
- `NETWORK_PANEL_PALETTE`: generated JSON palette; the service uses its Home
  Manager configuration link.
- `NETWORK_CUSTOM_HELPER`: QR-only `network-manager` executable.
- `NETWORK_EDITOR`: `nm-connection-editor` for the Advanced flow.
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

`niri/panels/default.nix` links `scripts/media-control/astal/` to
`~/.config/ags/media`, including `frontend.js`, `backend.js`, `logic.js`, and `style.css`.
It does not install generated children into that live link. The palette is
serialized from `themes/palette.nix` to the separate
`~/.config/ags/media-palette.json`; the frontend generates GTK `@define-color`
rules programmatically from this JSON without writing generated CSS under the
live link.

The Niri-only `ags-media.service` runs `media-panel run`, restarts on failure,
and is wanted by Waybar and the graphical session. The launcher uses the pinned
AGS 2.3 compiler: `ags bundle --gtk 4 -- <entry> <output>`. All four panel
launchers use distinct `$XDG_RUNTIME_DIR/ags-panels/<name>-panel.js` outputs;
`ags run` would otherwise share a single `ags.js` file and race at login.
`niri/panels/ags-runtime.nix` adds a companion `panel-gjs` wrapper using each
AGS package's captured runtime environment and GTK4 layer-shell preload.
Media retains GTK4, Graphene, Astal MPRIS and Astal WirePlumber; no manual
`GI_TYPELIB_PATH` supplement is needed.
`niri/panels/media.nix` also builds a combined native GdkPixbuf loader cache for
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
systemctl --user restart ags-media.service  # Apply live JS/CSS edits
```

`MEDIA_PANEL_CONFIG` selects the entry file, defaulting to
`$XDG_CONFIG_HOME/ags/media/frontend.js` (or `~/.config/ags/media/frontend.js`).
`MEDIA_PANEL_PALETTE` is a JSON file path; `MEDIA_CONTROL` and `NETWORK_PANEL`
are executable paths, not shell commands. The launcher supplies store-backed
defaults, and the service explicitly supplies these values plus
`GDK_BACKEND=wayland`. Graphical-session Wayland and D-Bus variables are inherited.

Rebuild after changing Nix/service wiring or the Rust helper; JS/CSS changes
only require restarting the media service. Network requests use the separate
`network-panel` launcher.

### Shared popup style

The AGS/Astal GTK4 cards use per-panel GTK CSS for controls and Cairo-drawn
chamfered frames for translucent backgrounds and yellow outlines.
`themes/palette.nix` supplies the shared colors as generated JSON palettes,
kept outside the live-linked source directories.

| Panel | Presentation sources |
| --- | --- |
| Audio | `scripts/audio-control/astal/`: `style.css`, frame in `frontend.js` |
| Media | `scripts/media-control/astal/`: `style.css`, frame in `frontend.js` |
| Network | `scripts/network-manager/astal/`: `style.css`, frame in `frontend.js` |
| Dashboard/notifications | `scripts/dashboard/`: `style.css`, frames and shared widgets in `notification.ts` |

Audio uses Bluetooth/Audio tabs and native services. Output volume
still spans 0–150%; the separate media panel is limited to 100%.
Popup sizes and placement remain specific to their content, not separate themes.

After installing service/configuration links through Home Manager, restart
the corresponding panel service to apply live JS/CSS edits. For network:

```sh
systemctl --user restart ags-network.service
```

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
cargo test --manifest-path scripts/Cargo.toml --locked \
  -p audio-control -p media-control -p rofi-network --all-targets
cargo build --manifest-path scripts/Cargo.toml --locked -p rofi-network -p media-control
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
live NetworkManager state. Rust tests cover QR escaping, validation,
malformed/oversized input, output bounds, and rejected legacy arguments:

```sh
cargo test --locked --manifest-path scripts/Cargo.toml -p rofi-network
cargo fmt --manifest-path scripts/Cargo.toml -p rofi-network -- --check
nix-instantiate --parse niri/panels/network.nix >/dev/null
nix-instantiate --parse niri/panels/default.nix >/dev/null
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
