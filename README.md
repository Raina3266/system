# Custom Scripts

This repository contains eight Rust utilities used by the desktop configuration:

- `media-control` — a dynamic MPRIS controller for Rofi and Waybar
- `preview-panel` — a reusable GTK4 text and image preview window
- [`rofi-audio`](#rofi-audio) — a Bluetooth manager and audio mixer for devices and playback streams
- [`rofi-clipboard`](#rofi-clipboard) — a clipboard + Memo manager with a Rofi interface
- `rofi-network-manager` — Wi-Fi and Ethernet controls with a Rofi interface
- [`swaync-sysmon`](#swaync-sysmon) — the live system readings inside the notification center
- [`waybar-timer`](#waybar-timer) — an interactive countdown timer for Waybar
- `webcam-crop` — an on-demand virtual webcam cropper and supervisor

It also documents [the notification center](#notification-center) those last
two sit beside: SwayNC, the single popup that now owns brightness, volume, the
hardware readings and the session's power actions.

## rofi-audio

`scripts/rofi-audio` replaces the former `custom/audio` shell script and
`custom/bt` Waybar module with one program and one Waybar entry. Bluetooth is
driven by [`bluer`](https://crates.io/crates/bluer), the official BlueZ crate,
and the audio tabs by [`pulsectl-rs`](https://crates.io/crates/pulsectl-rs) over
PulseAudio, which `pipewire-pulse` serves. Neither backend is reimplemented by
hand.

The package in `scripts/packages.nix` and the existing Waybar launcher provide
all four tabs.

### Modes

Rofi opens on **Pair** (Bluetooth); `Shift+Left`/`Shift+Right` move between tabs.
The internal mode name remains `bluetooth`.

| Tab | Rows | Enter / double-click |
| --- | --- | --- |
| Pair | Discovered and paired Bluetooth devices | Pair/connect or disconnect, as before |
| Output | Output devices and their available ports | Activate the row's port, then set its device as default |
| Input | Microphones and other non-monitor inputs, including their available ports | Activate the row's port, then set its device as default |
| Play | Live playback streams, with app, volume and meaningful stream title | Choose that stream's output |

Applications may expose several streams. They remain separate; no MPRIS support
is required. Start playback in an application for its stream to
appear. Muted or paused streams are dimmed, and lists refresh every two seconds.
The popup width is configured in `themes/rofi-audio.rasi`.
Tabs size themselves to their labels instead of splitting the width equally,
so a longer label such as Output gets more space than Pair.
There is no Recording tab or per-application input routing. The Input tab still
controls microphone devices: default selection, volume, mute and physical ports.

For example, Output can show **Speakers**, **Headphones**, and **HDMI / DisplayPort 1**
as separate rows, without repeating the chipset name. Hardware names are added
only when port labels collide; any remaining identical or identically clipped
labels get a small number at the front. Full device descriptions remain searchable.
Input can similarly show an internal microphone
and a microphone jack. Enter/double-click switches to that port and makes its
device the default. Only the active port of the default device is highlighted
in cyan. Devices without named ports still appear once, as before.

For ALSA cards, Output also includes available ports from compatible inactive
profiles. This lets **Speaker** and **Headphones** both appear on laptops whose
HiFi profile exposes only one of them at a time. A row without a live device
shows **—** instead of a volume: select it before using volume or mute.
Enter/double-click switches profiles when necessary, waits for the new device,
activates the port and sets the default output. Card/port identities remain
stable even when the audio server replaces the underlying devices.

Ports reported as unplugged are hidden; unknown availability remains selectable.
Ports and profiles are rechecked before activation. Automatic profile selection
preserves the current microphone ports and prefers profiles retaining the most
other ports; it does not switch Bluetooth codecs or profiles. Profile changes
can briefly interrupt all audio on the card. If activation fails after a switch,
the program attempts to restore the previous profile and default output, without
overwriting a newer profile choice made elsewhere. Input still lists ports from
its current profile; no Profile button is added.

Playback omits the generic **Playback** title and the **→ destination** suffix:
for example, **Google Chrome: Playback → Alder Lake…** becomes **Google Chrome**.
Real stream titles are retained. The destination remains searchable and can be
viewed or changed in the route picker; this only changes the displayed text.

Discovery runs in a detached `scan-bg` process, so the menu appears immediately
with devices BlueZ already knows about. Lists refresh automatically while the
menu is idle; **Scan** starts another discovery window without blocking the
menu. Automatic discovery at launch leaves a powered-off adapter off; **Scan**
turns it on.

In **Pair**, connected devices are cyan and sort to the top, paired devices
are white, and discovered devices are dimmed. Battery level is appended when
the device reports one. The message panel shows the highlighted device's
address and pairing state.

The input bar shows a filter glyph rather than a prompt. Rofi drives the prompt
widget from the mode's display name, which is the same string the mode-switcher
tab shows, so a prompt here could only repeat the tab beside it; the theme
leaves the widget out of the input bar's children and puts a static glyph in
its place.

### Controls

Buttons appear in this order, left to right. They operate on the selected row,
not necessarily the system default. All buttons stay visible when switching
tabs; use them only in the tabs or pickers listed below.

| Button | Shortcut | Where | Action |
| --- | --- | --- | --- |
| 󰑐 Scan | Alt+1 | Pair | Look for nearby Bluetooth devices |
| 󰆴 Forget | Alt+2 | Pair | Remove the selected device's saved pairing; reconnecting may require pairing again |
| 󰝝 Volume up | Alt+3 / Alt+Up | Output, Input, Play | Increase selected device or stream volume by 5 percentage points, up to its ceiling |
| 󰝞 Volume down | Alt+4 / Alt+Down | Output, Input, Play | Decrease selected device or stream volume by 5 percentage points, down to 0% |
| 󰝟 Mute | Alt+6 | Output, Input, Play | Toggle mute on the selected device or stream, retaining its volume |
| 󰁔 Route | Alt+7 | Play | Choose an output for the selected app's stream without changing the system default |

Alt+5 is reserved for the existing refresh action. Escape closes the entire
menu. In the routing picker, use the permanent **Back** row or Alt+9 to return
without changing anything. There are no separate Port or Back toolbar buttons,
and no port picker. Enter or double-click selects a routing destination and
keeps the picker open, with that output checked and highlighted. Selecting the
current output does nothing to the audio. Use **Back** or Alt+9 to return to the
stream list; selecting an output does not mute or pause playback.
The search filter resets on actions, as before, so searching for an application
does not hide all devices when its picker opens.

Outputs and playback streams can reach **150%**; inputs are capped at **100%**.
Amplification above 100% can distort. Adjustments preserve
an existing channel balance, with the ceiling applied to the loudest channel.
The normal volume controls do not unmute a muted device or stream.

The routing picker offers the same physical outputs as the Output tab,
including **Speaker** and **Headphones** when one requires an inactive ALSA
profile. Selecting a row activates its port/profile when needed, then moves
the selected playback stream. It does not explicitly change the system default.
The checked row is the stream's current device and active port, which may
differ from the system default. Compact port names, hardware context for
ambiguous labels, and full searchable descriptions match the Output tab.
Persistence across application restarts is managed by PipeWire/WirePlumber.

Port rows of the same device are not independent outputs: switching ports
affects every stream using that device. A profile switch can briefly interrupt
audio on the card; mutually exclusive Speaker/Headphones profiles cannot play
both at once. If the old default output disappears, PipeWire may choose a
replacement. A failed route after a profile switch attempts to restore the
previous profile and default. Volume/mute controls target a live device,
without activating a different port or profile; they are not per-port controls.
Manual profile selection, Bluetooth codecs, channel editing, latency offsets
and digital passthrough configuration are intentionally not included.

Normal audio tabs do not show a status panel. Picker instructions and errors
are shown when necessary, on one visual line. Messages flatten embedded line
breaks, truncate long text, and disable Pango line wrapping; overflow at narrow
window widths is clipped rather than increasing the panel height.
If a stream ends or a device is unplugged while its
picker is open, the program revalidates the target rather than acting on a
different row. Volume/mute/routing changes check the audio server's response.

Single-click highlights a row. Enter or double-click connects/disconnects a
Bluetooth device, activates an audio port, or opens a playback routing picker.

### Pairing

Connecting to an unpaired device starts a detached `connect-bg` process that
registers its own BlueZ authorization agent. BlueZ routes that pairing's
prompts to the caller's agent, so when a device asks for a PIN or a passkey the
filter box turns into a code entry — the same flow the Wi-Fi menu uses for
passwords. Type the code and press `Enter`. When the device is the one that has
to be typed on, the code is shown in the message line instead. The two halves
talk through single-use files in `$XDG_RUNTIME_DIR`, so an abandoned pairing
never leaves a stale prompt behind.

Pairings started from the other side still go to the session-wide `bt-agent`
service, which auto-confirms them.

### Commands

```text
rofi-audio [launch]
rofi-audio status
rofi-audio bluetooth-power [on|off|toggle]
rofi-audio script <bluetooth|output|input|playback>
rofi-audio connect-bg <row-key>
rofi-audio scan-bg
```

`script`, `connect-bg`, and `scan-bg` are internal: Rofi invokes the first,
and the Pair tab spawns the other two.

### Waybar module

```jsonc
{
  "custom/audio": {
    "exec": "/path/to/rofi-audio status",
    "interval": 5,
    "return-type": "json",
    "escape": false,
    "on-click": "/path/to/rofi-audio",
    "on-click-right": "/path/to/rofi-audio bluetooth-power toggle"
  }
}
```

The text is a single glyph:

| Bluetooth | Glyph |
| --- | --- |
| Off | 󰕿 󰖀 󰕾 by the default output's level, 󰝟 when it is muted |
| On, nothing connected | 󰂯 |
| On, device connected | 󰂱 |

Giving the slot to Bluetooth while the adapter is on costs nothing: the level is
back as soon as the adapter is off, and the [notification center](#notification-center)
carries a volume slider besides. The module also sets a `class` — `bluetooth-connected`,
`bluetooth-on`, `muted`, `active`, or `unavailable` — so the glyph can be
recoloured per state from `waybar.css`.

The tooltip carries the rest: the default output, the default input, and any
connected Bluetooth devices. Right-click is the Bluetooth adapter's on/off
switch; `bluetooth-power on` and `off` are available for bindings of your own.

### Environment variables

| Variable | Purpose |
| --- | --- |
| `ROFI_AUDIO_ROFI` | Override the `rofi` executable |
| `ROFI_AUDIO_THEME` | Override the Rofi theme path (default: `$XDG_CONFIG_HOME/rofi/rofi-audio.rasi`) |
| `ROFI_AUDIO_SCAN_SECONDS` | Length of the Bluetooth discovery window (default: `10`) |

Styling lives in `themes/rofi-audio.rasi`, symlinked to
`~/.config/rofi/rofi-audio.rasi` so edits apply without a rebuild.

### Development checks

From the repository root, with its Rust development environment:

```sh
nix develop .#rust
cargo fmt --manifest-path scripts/Cargo.toml --package rofi-audio -- --check
cargo test --manifest-path scripts/Cargo.toml --package rofi-audio --locked
cargo clippy --manifest-path scripts/Cargo.toml --package rofi-audio --locked -- -D warnings
```

Unit tests include port-row identities, availability, active/default marking,
device-only fallback, volume conversion/limits, channel balance, stream identity,
picker cancellation and dispatch, disappearing targets, rejected choices,
state round trips, row rendering and refresh behavior without hardware. Profile
switching tests cover the split Speaker/Headphones HiFi layout, delayed device
creation, stale/unplugged targets, server rejection and rollback.

For live checks, use a disposable PulseAudio/PipeWire session where possible:

1. Mute and unmute an output and microphone; verify each previous volume stays.
2. Start two playback applications and adjust/mute one; verify the other is unchanged.
3. Route one playing stream to a second live output; verify playback continues and
   the global default is unchanged. Check that the picker stays open and the
   selected output is checked. Select it again; playback must remain unchanged.
   Use Back to return to the stream list. Repeat with an already-paused stream
   and check that routing does not resume it.
4. Double-click the speakers/headphone port rows, then the microphone port rows;
   verify the selected port and default device, and that only one row is cyan.
   Unplug a jack; check that its row disappears and an old selection is rejected.
5. Stop a stream with its picker open; verify that Back and refresh remain usable.
6. Check 100→105→150% output volume, the 150% ceiling, and the 100% input ceiling.
7. Check Bluetooth scan/connect/forget and pairing-code entry still work.
8. Verify that only Pair, Output, Input and Play tabs appear, the toolbar
   has six buttons, and the routing picker's Back row/Alt+9 still work.
9. On a laptop with separate Speaker/Headphones profiles, plug in headphones and
   verify both rows appear in Output. Switch in both directions; check the selected
   row becomes default, its volume appears, and microphone/HDMI choices survive.
   An inactive row's volume/mute buttons must not switch profiles.
10. With the Headphones profile active, open a stream's playback picker and
    choose Speaker. Check that Speaker appears, becomes checked after routing,
    and the picker stays open. Repeat in reverse. When the system default is
    a separate live output, check that it is unchanged.

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

## swaync-sysmon

`scripts/swaync-sysmon` prints the CPU, memory, temperature, disk and network
block shown inside the SwayNC control center — the readings that used to be
Waybar's `group/hardware` drawer. It is a plain command, not a daemon: SwayNC's
patched `label` widget runs it every few seconds and again whenever the panel
opens, and takes its standard output as the widget's text.

### Output

One line per available reading, as Pango markup in the same cyberpunk palette as
the rest of the desktop. `--plain` prints the same rows without the markup:

```text
󰻠  12%               2.41 GHz
󰍛  7.5G/32.0G        24%
󰄏  47°C
󰋊  412G free         58% used
󰖩  ↓1.2K/s ↑512B/s   wlan0
```

There is no name column. The glyphs are distinct enough to carry the meaning
on their own, and dropping six words of dim text is most of what makes the
block read like a control-centre panel rather than a table. The value column
is padded so the dimmed detail after it lines up; a row with nothing after
its value is not padded at all.

A reading the machine cannot supply is left out rather than shown as a
placeholder: a desktop with no battery-class thermal sensor gets no `Temp` row,
and a machine with no routing table gets no `Network` row. Swap appears only once
something is actually swapped out. If nothing at all can be read, the block says
so in one line.

Values turn amber and then red past the thresholds the replaced Waybar modules
used: 55/80 °C for temperature, 80/95% for CPU, 80/90% for memory and disk.

### Readings

| Row | Source | Notes |
| --- | --- | --- |
| CPU | `/proc/stat`, `/proc/cpuinfo` | Busy share of the interval since the last run; `idle` and `iowait` are not busy. The detail column is the mean core frequency |
| Memory | `/proc/meminfo` | `MemTotal - MemAvailable`, so reclaimable page cache is not counted as used |
| Swap | `/proc/meminfo` | Hidden unless something is in it |
| Temp | `/sys/class/thermal`, `/sys/class/hwmon` | Prefers the package sensor (`x86_pkg_temp`, `coretemp`, `k10temp`, …) over the chassis one |
| Disk | `df -P -B1` | The root filesystem by default |
| Network | `/proc/net/route`, `/proc/net/dev` | The interface carrying the lowest-metric default route, so a VPN outranks the Wi-Fi link it runs over |

### Counters between runs

CPU load and throughput are differences between two readings of a counter that
only ever increases, but each run is a separate process. The previous sample —
and the rates derived from it — therefore live in a small file under
`$XDG_RUNTIME_DIR/swaync-sysmon/`, written through a temporary file so a
concurrent read never sees half a sample.

Two consequences are deliberate. The very first run after login takes one
reading, waits 250 ms and reports the difference, so the first panel opened is
not blank. And a run that lands within a quarter second of the previous one —
opening the control center just after a timer tick — repeats the figures already
derived instead of dividing a counter delta by a near-zero interval.

### Commands

```text
swaync-sysmon [--plain]
swaync-sysmon --help
```

### Environment variables

| Variable | Purpose |
| --- | --- |
| `SWAYNC_SYSMON_DF` | Override the `df` executable |
| `SWAYNC_SYSMON_DISK` | Filesystem to report (default: `/`) |
| `SWAYNC_SYSMON_THERMAL_ZONE` | A `/sys/class/thermal` zone index, or a full path to a sensor file (default: auto-detect) |
| `SWAYNC_SYSMON_INTERFACE` | Network interface (default: the default route's) |
| `SWAYNC_SYSMON_STATE` | Counter state file |
| `SWAYNC_SYSMON_PROC` | `/proc` replacement (default: `/proc`) |
| `SWAYNC_SYSMON_SYS` | `/sys` replacement (default: `/sys`) |

The last two exist so the tests can describe fixed readings instead of asserting
against whatever the machine running them happens to be doing.

### Development checks

```sh
nix develop .#rust
cargo fmt --manifest-path scripts/Cargo.toml --package swaync-sysmon -- --check
cargo test --manifest-path scripts/Cargo.toml --package swaync-sysmon --locked
cargo clippy --manifest-path scripts/Cargo.toml --package swaync-sysmon --locked -- -D warnings
```

Tests cover every parser against real `/proc` and `df` layouts (including a
device name containing spaces and a routing table with two default routes), the
state file's round trip and truncation behaviour, the rate arithmetic including
reset counters and zero intervals, the formatting and thresholds, and a fixture
`/proc` + `/sys` tree that exercises the whole block: sensor preference, the
repeated-rates path, an absent sensor, a disconnected machine, and a machine
where nothing can be read at all.

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
    "on-click": "/path/to/waybar-timer toggle",
    "on-click-middle": "/path/to/waybar-timer add",
    "on-click-right": "/path/to/waybar-timer clear"
  }
}
```

---

## Notification center

SwayNC (`SwayNotificationCenter`) is this desktop's notification daemon and the
one popup behind the bell at the right end of the top bar. Configuration lives in
`niri/swaync/default.nix`; the stylesheet is `themes/swaync.css`.

### What moved

The top bar used to carry two hover drawers, `group/system` and
`group/hardware`, holding nine modules between them. Both are gone; their
contents are widgets in the control center instead, and the bar keeps the button
that opens it.

| Was | Is now |
| --- | --- |
| `backlight` in `group/system` | The control center's `backlight` slider |
| `pulseaudio` in `group/system` | The control center's `volume` slider, with per-application volumes |
| `temperature`, `memory`, `cpu`, `disk`, `network` in `group/hardware` | One live [`swaync-sysmon`](#swaync-sysmon) block |
| — | A power dropdown: Suspend, Log out, Restart, Shut down |
| `custom/battery` in `group/system` | Unchanged, but now the first module on the bar |
| `tray` at the right | Moved to the left, after the battery |

The battery deliberately stayed in the bar: it is the one reading worth seeing
without opening anything. `custom/audio` (Bluetooth and the default devices) and
`custom/network` (Wi-Fi) are Rofi menus of their own and were not touched.

The top bar is therefore `custom/battery`, `tray`, `custom/ycal` on the left,
media and lyrics in the centre, and `custom/timer`, `custom/clipboard`,
`custom/audio`, `custom/network`, `custom/swaync` on the right — the bell last,
in the corner.

### The panel

The panel is meant to read like GNOME's quick settings or the macOS Control
Centre: a short stack of small cards you take in at a glance, not a settings
page. It is five rows in a 380px-wide popup — a header of pill buttons, the two
sliders, the system readings, and the notification list.

There is no separate title row and no separate Do Not Disturb row. Both collapse
into the header, which is most of what makes the panel short: Do Not Disturb is a
toggle pill on the left, Clear is a button beside it, and the session menu is a
single 󰐥 button on the right.

Brightness drives `/sys/class/backlight/intel_backlight` and is floored at 5 so
the slider cannot black the screen out. A different GPU reports a different
device name there (`amdgpu_bl0`, `acpi_video0`); it is the `backlight.device`
key in `niri/swaync/default.nix`.

Volume shows each playing application as well as the default sink, so a single
loud tab can be turned down without touching everything else.

### The power dropdown

The 󰐥 button on the right of the header is a revealer: clicking it slides the
four session actions open underneath the row as a plain list, and clicking it
again folds them away. They cost no height until asked for, which is the whole
reason they are a dropdown rather than the grid of four large buttons this
started as.

Suspend acts immediately — it costs a keypress to undo. Log out, Restart and
Shut down first close the panel and then ask for confirmation through Rofi,
using `themes/rofi-power.rasi`, because the panel opens under the pointer and a
mis-click there is not recoverable. Cancel is highlighted first, so a stray
Enter is harmless, and Escape cancels.

The `swaync-power` wrapper takes `suspend`, `logout`, `reboot` or `poweroff`, and
is available for bindings of your own. `SWAYNC_POWER_THEME` overrides the Rofi
theme it uses.

### The Waybar bell

`custom/swaync` subscribes to SwayNC over `swaync-client -swb`, which streams the
notification count and the daemon's state — but no glyph. Rather than map that
state onto `format-icons` and depend on Waybar resolving `{icon}` through the
`alt` field, `jq` builds the finished label in the pipeline: the bell alone when
nothing is waiting, the bell and the count when something is, and a struck-out
bell under Do Not Disturb.

| Action | Result |
| --- | --- |
| Click | Open or close the control center |
| Right-click | Toggle Do Not Disturb |
| Middle-click | Close every notification |

The module sets a class per state — `none`, `notification`, the `dnd-` and
`inhibited-` variants, plus `cc-open` while the panel is up — so `themes/waybar.css`
recolours it: pink with something waiting, dimmed under Do Not Disturb, and
washed pink while the panel is open.

`Mod+N` toggles the panel and `F8` toggles Do Not Disturb, both through
`swaync-client`.

### The upstream patch

SwayNC's `label` widget shows a fixed string, so there is no built-in way to put
live readings in the panel. `niri/swaync/label-exec.patch` adds three optional
keys to it:

| Key | Meaning |
| --- | --- |
| `exec` | A command whose standard output becomes the label's text |
| `interval` | How often, in seconds, to re-run it. `0` re-runs it only when the control center opens |
| `pango-markup` | Whether that output is parsed as Pango markup |

Without `exec` the widget behaves exactly as it does upstream, and the patch
carries the matching `configSchema.json` and man-page entries. It applies cleanly
to v0.12.5, v0.12.6 and upstream `main`; the overlay that applies it is at the
top of `niri/swaync/default.nix`, next to the same pattern used for Rofi in
`niri/rofi/default.nix`.

### Theming

`themes/swaync.css` is linked to `~/.config/swaync/style.css` by
`themes/default.nix`, so edits apply without a rebuild — SwayNC re-reads it on
`swaync-client --reload-css`. SwayNC loads its packaged stylesheet first and this
one second at the same priority, so the file overrides rather than replaces
upstream: most of it is the palette shared with `themes/waybar.css` and the Rofi
`.rasi` themes, expressed through SwayNC's own CSS variables.

The system monitor supplies its own colours as Pango markup, so the widget's CSS
only sets the monospace grid its column alignment assumes.

The same file sets the panel's density: 5-7px of card padding, 9px radii, a 6px
slider track with a 10px handle, and 11-13px text. If the panel ever wants to
breathe, those are the numbers to raise, along with `control-center-width` and
`control-center-height` in `niri/swaync/default.nix`.

### GNOME

The systemd unit is conditioned on `XDG_CURRENT_DESKTOP=niri`, like Waybar's.
GNOME runs its own notification daemon and two owners of
`org.freedesktop.Notifications` cannot coexist, so SwayNC stays out of that
session.
