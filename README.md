# Custom Scripts

This repository contains nine Rust packages used by the desktop configuration:

- `audio-control` — headless audio compatibility policy, request-local PIN/passkey pairing, and HID++ battery publication
- `media-control` — headless media grouping, conservative stream matching, artwork, and video-author metadata
- `network-manager` — only the detail/secret/QR and legacy-WEP gaps in Quickshell’s native networking API
- `ocr-screenshot` — screenshot OCR and clipboard integration
- `preview-panel` — the shared GTK4 preview surface
- [`rofi-clipboard`](#rofi-clipboard) — clipboard history and memos
- `rofi-filesearch` — searchable file launcher with previews
- [`waybar-timer`](#waybar-timer) — interactive Waybar countdown timer
- `webcam-crop` — on-demand virtual webcam cropper and supervisor

The Niri configuration uses **AGS 2.3/Astal GTK4 for media and audio**. The
dashboard, network, and notification popups remain in Quickshell. `media-panel`
and `audio-panel` are Nix-packaged launchers, not additional Rust GUIs.

## Live desktop editing

After one Home Manager/NixOS switch, presentation files are linked directly
from `/home/raina/System`. They no longer need another system rebuild:

| What to edit | When it takes effect |
| --- | --- |
| `niri/config.kdl` | Niri reloads it on save |
| `niri/rofi/config.rasi`, `themes/rofi/*.rasi` | Next Rofi launch |
| `themes/quickshell/*.qml` | Quickshell reloads dashboard, network, and notifications on save |
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
| Bluetooth | Bluetooth power, scan, pair, connect, disconnect, forget, and battery levels |
| Audio | Output controls in the upper half; input controls in the lower half, each with volume/mute and device/port selection |

Volume percentages sit beside their sliders on the same row. There is no Play
tab; playback stays in the existing media panel. Output volume runs from
**0 to 150%**, retaining amplification above 100%. Input remains 0–100%.
Audio uses unmodified `pkgs.astal`, without framework patches or monkey-patches.
Astal WirePlumber owns audio inventory and state, volume, mute, default-device
selection, physical routes, and profiles. Astal Bluetooth owns native device
state, adapter power, discovery, trust, connect/disconnect, forget, and battery
readings. UI and Waybar status use these native services.

Only functionality Astal genuinely lacks uses the Rust helper:

- `audio-control policy` consumes canonical Astal snapshots from AGS. Bounded,
  read-only `pw-dump -N` supplements only omitted `EnumProfile`/`EnumRoute`
  associations and per-endpoint `Route` state, not a second device inventory.
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

**Migration prerequisite:** apply the normal NixOS/Home Manager generation before
restarting the UI. The migration audit found the installed generation still using
the old Quickshell audio unit and dangling live QML links; the new
`~/.config/ags/audio` link was not yet installed. A restart alone cannot install
the AGS configuration or updated helper. Let the generation manage the links and
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
or initiates pairing. The audio-panel tests have no Python dependency.

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
live link. Quickshell still gets its generated `Colors.qml` from the same palette.

The Niri-only `ags-media.service` runs `media-panel run`, restarts on failure,
and is wanted by Waybar and the graphical session. The launcher uses the pinned
AGS 2.3 CLI: `ags run --gtk4 -- <entry>`. Its overridden AGS package includes GTK4,
Graphene, GTK4 layer-shell, Astal MPRIS, and Astal WirePlumber in the generated
runtime wrapper; no manual `GI_TYPELIB_PATH` supplement is needed.
`niri/panels/media.nix` also builds a combined native GdkPixbuf loader cache for
standard formats, librsvg, and `webp-pixbuf-loader`, preserving PNG/JPEG/SVG while
adding WebP artwork support. AGS's wrapper sets `GDK_PIXBUF_MODULE_FILE` to this
cache rather than the default librsvg cache that omits WebP; no JavaScript
decoder or user environment override is needed. The frontend subclasses native
`Astal.Application`, constructs it with `instance_name: "media-panel"`, and uses
its `acquire_socket()` and application lifecycle APIs. AGS 2.3 has no
`run --instance` flag. Requests use Astal's native request socket through
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
`MEDIA_PANEL_PALETTE` is a JSON file path; `MEDIA_CONTROL` and `QUICKSHELL`
are executable paths, not shell commands. The launcher supplies store-backed
defaults, and the service explicitly supplies these values plus
`GDK_BACKEND=wayland`. Graphical-session Wayland and D-Bus variables are inherited.

Rebuild after changing Nix/service wiring or the Rust helper; JS/CSS changes
only require restarting the media service. The legacy `quickshell-media.service`
and `quickshell --config media` names are intentionally retained for the
**non-media** desktop panels. Quickshell no longer exports a `media` IPC target.

### Shared popup style

The dashboard, network, and notification popups share QML components from
`themes/quickshell/`. The AGS media and audio cards recreate the same presentation
in GTK4 rather than loading QML; only palette values are shared across toolkits.
`niri/quickshell/style.nix` installs a local `shared/` directory of aliases
inside each popup's configuration root. All aliases point to the same
source files and palette; styles are not copied. This keeps imports
inside Quickshell's configuration sandbox without patching Quickshell:

| Source | Shared behavior |
| --- | --- |
| `themes/quickshell/PopupFrame.qml` | Charcoal background, 10% transparency, thin yellow outline, matching corner cuts |
| `themes/quickshell/ControlButton.qml` | Cyan text/red glyphs, 50%-transparent gray outlines, yellow/thicker outlines only on hover |
| `themes/quickshell/IconButton.qml` | Circular variant of the shared button; volume buttons can suppress their outlines |
| `themes/palette.nix` | One palette exported as Quickshell colors and AGS media/audio JSON |

Audio uses Bluetooth/Audio tabs and native services. Output volume
still spans 0–150%; the separate media panel is limited to 100%.
Popup sizes and placement remain specific to their content, not separate themes.

After installing the shared directory and palette links through Home Manager,
if an edit is not picked up by Quickshell's live reload, refresh its UIs:

```sh
systemctl --user restart quickshell-media.service
```

### Audio panel

The audio button opens the AGS/Astal GTK4 Bluetooth/Audio popup described
under [`audio-control`](#audio-control). Its service remains `audio-control-ui`,
but its AGS instance is `audio-panel`. Playback remains in the media panel.

### External panel behavior

Dashboard, network, media, and audio use transparent monitor-sized layer
surfaces with a visible card. The backdrop consumes outside clicks;
Escape and a second button press dismiss the card. Opening network closes the
dashboard and invokes `media-panel close` once through a Quickshell `Process`;
network status polling never spawns a media command. Opening media closes network
through `QUICKSHELL ipc --config media call network close`. The audio popup stays
independent. Native notifications use a small non-focus-taking layer surface
instead, so toasts do not grab keyboard input.

Network scanning, dashboard metrics, and audio compatibility refreshes stop while
the corresponding panel is hidden. Astal audio/Bluetooth state and notifications
remain event-driven, and the media badge requests metadata on demand even while
its card is closed.

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
frontend tests live with the JavaScript modules in `scripts/media-control/astal/`, not
under Quickshell. `themes/quickshell/tests/panel_handoff.py` loads the real shell with fake panel
components on a private D-Bus, proving network-open cross-close without media
subprocesses during status polling.

The Qt Quick tests in `themes/quickshell/tests/` cover network ordering/status,
notification row sizing, and native action controls. They use installed shared
component aliases and palette (or a staged equivalent). Do not activate a new
generation or send real playback/network commands just to run isolated checks.
