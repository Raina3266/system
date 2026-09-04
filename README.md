# Custom Scripts

This repository contains eight Rust utilities used by the desktop configuration:

- [`media-control`](#media-control) — the bar's centre button: MPRIS control for Rofi, Waybar and the notification centre
- `preview-panel` — a reusable GTK4 text and image preview window
- [`rofi-audio`](#rofi-audio) — a Bluetooth manager and audio mixer for devices and playback streams
- [`rofi-clipboard`](#rofi-clipboard) — a clipboard + Memo manager with a Rofi interface
- `rofi-network` — Wi-Fi and Ethernet controls with a Rofi interface
- [`swaync-panel`](#swaync-panel) — the readings, calendar and Do Not Disturb rows of the notification centre
- [`waybar-timer`](#waybar-timer) — an interactive countdown timer for Waybar
- `webcam-crop` — an on-demand virtual webcam cropper and supervisor

It also documents [the notification center](#notification-center) two of them
feed: SwayNC, the single popup that owns the volume slider, the media overview,
today's calendar and the hardware readings.

## media-control

`scripts/media-control` is the MPRIS controller behind the bar's centre button,
its Rofi menu, and the media row inside the notification centre. One program
owns all three, so "the current player" means the same thing wherever it is
shown.

The players it considers, how they are ranked, and how browser tabs are filtered
are unchanged; what follows is only the three faces it renders.

### The bar button

`media-control waybar --watch` streams Waybar JSON for `custom/media`, the one
module in the centre of the top bar. Its label is a notification badge followed
by the current track:

```text
󰂜                          nothing waiting, nothing playing
󰂚 3                        three notifications, nothing playing
󰂜  ·  󰐊  Delulu — SZA      playing, nothing waiting
󰂚 3  ·  󰏤  Delulu — SZA    three notifications, paused
󰂛                          Do Not Disturb; the count is not worth showing
```

The count comes from `swaync-client --subscribe`, which prints one line per
change and flushes it, so following it costs one long-lived child rather than a
D-Bus round trip on every tick. The subscriber runs on its own thread and
restarts itself if the daemon does; until its first line arrives the button
shows a quiet bell, which is also what it shows when there is genuinely nothing
waiting. A missing `swaync-client` stops the thread rather than retrying
forever — waiting will not make the binary appear.

Waybar escapes the label before setting it as markup, so the text stays plain
and the colours come from classes. The module emits two, sometimes three: the
notification state (`quiet`, `notification`, `dnd`), the playback state
(`playing`, `paused`, `empty`), and `cc-open` while the panel is up.
`themes/waybar.css` recolours it from those.

| Action | Result |
| --- | --- |
| Click | Open or close the notification centre |
| Middle-click | Play/pause the current player |
| Right-click | Open the Rofi media menu |

Left-click is the panel rather than playback because the button is the only way
into the notification centre, and because the panel is where the rest of the
media detail lives.

### The panel list

`media-control players` prints one tab-separated line per player for SwayNC's
`media` widget, which turns each into a row with a play/pause button, a progress
bar and a volume slider:

```text
<id>	<status>	<volume>	<position>	<length>	<title>	<subtitle>
spotify	playing	45	83	296	Delulu — SZA	Spotify  ·  1:23 / 4:56
```

Volume, position and length are all optional in MPRIS — browser-tab bridges
routinely omit them — and a field the player does not report is a single `-`.
The widget hides the slider or the bar for a row rather than drawing an empty
one. `clean_text` collapses every run of whitespace, tabs included, so no field
can contain the separator.

The widget calls back with `media-control play-pause <player>` when a row's
button is pressed, `media-control volume <player> <percent>` when its volume
slider moves, and `media-control seek <player> <seconds>` when its progress bar
does, so the list stays a view of this program rather than a second
implementation of it. Volume and position are both optional in MPRIS, and a
player that does not report one is left alone rather than being told to change
something it does not have.

### The Rofi menu

Unchanged: `media-control menu` opens the full list, with per-player volume,
pinning and transport controls. `media-control list`, `toggle` and `pause-all`
are also unchanged.

### Commands

```text
media-control menu
media-control waybar --watch [--interval-ms 750]
media-control players
media-control play-pause <player>
media-control volume <player> <percent>
media-control seek <player> <seconds>
media-control toggle
media-control pause-all
media-control list
```

`toggle` acts on the most relevant player, which is what the bar button's middle
click wants; `play-pause` names one, which is what a row in the list wants.

### Environment variables

| Variable | Purpose |
| --- | --- |
| `MEDIA_CONTROL_PLAYERCTL` | Override the `playerctl` executable |
| `MEDIA_CONTROL_ROFI` | Override the `rofi` executable |
| `MEDIA_CONTROL_SWAYNC_CLIENT` | Override the `swaync-client` executable |
| `MEDIA_CONTROL_WITH_PARENT_DEATH` | Guard the subscriber child so it exits with this process |
| `MEDIA_CONTROL_THEME` | Override the Rofi theme path |
| `MEDIA_CONTROL_FALLBACK_THEME` | Theme used when no configured one exists |

### Development checks

```sh
nix develop .#rust
cargo fmt --manifest-path scripts/Cargo.toml --package media-control -- --check
cargo test --manifest-path scripts/Cargo.toml --package media-control --locked
cargo clippy --manifest-path scripts/Cargo.toml --package media-control --locked -- -D warnings
```

Tests cover the badge for each daemon state, the badge and track sharing one
label, truncation of a long title, the class array including `cc-open`, the
seven fields of a widget row including the ones MPRIS leaves out, the clock
formatting either side of an hour, and the `--subscribe` line scanner.

---

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

## swaync-panel

`scripts/swaync-panel` renders every row of the notification centre that SwayNC
itself cannot: the system readings, today's calendar, and the Do Not Disturb
pill's command. They used to be a Rust binary, a `jq` program, a shell script
and an empty JSON file between them; they are three subcommands here, so the
panel's configuration in `niri/swaync/default.nix` is now Nix and nothing else.

| Subcommand | Row |
| --- | --- |
| `sysmon` | CPU, memory, temperature, disk and network |
| `calendar` | Today's Google Calendar events and Tasks |
| `dnd` | What the Do Not Disturb toggle runs |

`sysmon` and `calendar` print Pango markup for SwayNC's patched `label` widget;
`--plain` prints the same rows without it, which is what the tests read.

### The readings row

#### Output

Every reading the machine can supply, as Pango markup in the same cyberpunk
palette as the rest of the desktop. `--plain` prints the same cells without the
markup:

```text
CPU  12% 2.41GHz          Temperature  47°C
Memory  7.5G/32.0G 24%   Disk  412G free 58%
Network  ↓1.2K/s ↑512B/s
```

Two labelled readings share a line. Each cell pads its text to a shared width
so the second column starts in the same place on every line; a cell wider than
that column is not truncated, it just pushes its neighbour along.

A reading the machine cannot supply is left out rather than shown as a
placeholder: a desktop with no battery-class thermal sensor gets no `Temp` row,
and a machine with no routing table gets no `Network` row. Swap appears only once
something is actually swapped out. If nothing at all can be read, the block says
so in one line.

Values turn amber and then red past the thresholds the replaced Waybar modules
used: 55/80 °C for temperature, 80/95% for CPU, 80/90% for memory and disk.

#### Readings

| Row | Source | Notes |
| --- | --- | --- |
| CPU | `/proc/stat`, `/proc/cpuinfo` | Busy share of the interval since the last run; `idle` and `iowait` are not busy. The detail column is the mean core frequency |
| Memory | `/proc/meminfo` | `MemTotal - MemAvailable`, so reclaimable page cache is not counted as used |
| Swap | `/proc/meminfo` | Hidden unless something is in it |
| Temp | `/sys/class/thermal`, `/sys/class/hwmon` | Prefers the package sensor (`x86_pkg_temp`, `coretemp`, `k10temp`, …) over the chassis one |
| Disk | `df -P -B1` | The root filesystem by default |
| Network | `/proc/net/route`, `/proc/net/dev` | The interface carrying the lowest-metric default route, so a VPN outranks the Wi-Fi link it runs over. The wired/wireless/tunnel glyph says which kind of link it is |

#### Counters between runs

CPU load and throughput are differences between two readings of a counter that
only ever increases, but each run is a separate process. The previous sample —
and the rates derived from it — therefore live in a small file under
`$XDG_RUNTIME_DIR/swaync-panel/`, written through a temporary file so a
concurrent read never sees half a sample.

Two consequences are deliberate. The very first run after login takes one
reading, waits 250 ms and reports the difference, so the first panel opened is
not blank. And a run that lands within a quarter second of the previous one —
opening the control center just after a timer tick — repeats the figures already
derived instead of dividing a counter delta by a near-zero interval.

### The calendar row
waybar-ycal's popup already fetches Google Calendar and Google Tasks and caches
them in `~/.cache/waybar-ycal/events.json`, so this row reads that file rather
than starting Python and a set of API calls of its own. The cache is keyed by
ISO date; a day's entries are a mix of plain strings (events, already carrying
their time range) and objects (tasks, carrying a done flag).

```text
󰄱  Pay rent & council tax
󰄱  Submit form
󰃭  Standup 09:30-10:00
+ 2 more
```

Open tasks come first because they are the part that still needs doing, then
events, then anything already ticked off. Three entries show at most; the rest
are a count. A cache that is missing, truncated mid-write, or simply has nothing
for today all render the same way — there is no useful difference between "no
events" and "no events yet known", and neither is worth an error in a status
panel.

Clicking `custom/ycal` in the bar still opens waybar-ycal's own popup, which is
where the full month and the task checkboxes live.
The bar label itself includes the weekday, date, month and time (for example,
`Friday 4 Sep 09:30`).


### The Do Not Disturb command
SwayNC hands a toggle button's new state to its command in `SWAYNC_TOGGLE_STATE`,
so `dnd` sets Do Not Disturb to exactly that rather than toggling blind and
hoping the pill and the daemon still agree.


### Commands

```text
swaync-panel sysmon [--plain]
swaync-panel calendar [--plain]
swaync-panel dnd
swaync-panel --help
```

### Environment variables

| Variable | Purpose |
| --- | --- |
| `SWAYNC_PANEL_DF` | Override the `df` executable |
| `SWAYNC_PANEL_THERMAL_ZONE` | A `/sys/class/thermal` zone index, or a full path to a sensor file (default: auto-detect) |
| `SWAYNC_PANEL_DISK` | Filesystem to report (default: `/`) |
| `SWAYNC_PANEL_INTERFACE` | Network interface (default: the default route's) |
| `SWAYNC_PANEL_STATE` | Counter state file |
| `SWAYNC_PANEL_CALENDAR` | waybar-ycal's event cache |
| `SWAYNC_PANEL_SWAYNC_CLIENT` | Override the `swaync-client` executable |
| `SWAYNC_PANEL_PROC` | `/proc` replacement (default: `/proc`) |
| `SWAYNC_PANEL_SYS` | `/sys` replacement (default: `/sys`) |

The last two exist so the tests can describe fixed readings instead of asserting
against whatever the machine running them happens to be doing.

### Dependencies

`serde_json` for the calendar cache and `chrono` for today's local date. The
readings need neither and use nothing outside `std`.

### Development checks

```sh
nix develop .#rust
cargo fmt --manifest-path scripts/Cargo.toml --package swaync-panel -- --check
cargo test --manifest-path scripts/Cargo.toml --package swaync-panel --locked
cargo clippy --manifest-path scripts/Cargo.toml --package swaync-panel --locked -- -D warnings
```

Tests cover every reading parser against real `/proc` and `df` layouts
(including a device name containing spaces and a routing table with two default
routes), the state file's round trip and truncation behaviour, the rate
arithmetic including reset counters and zero intervals, the formatting and
thresholds, and a fixture `/proc` + `/sys` tree exercising the whole block. The
calendar has its own: entry ordering, the three-then-count limit, a quiet day,
four shapes of unusable cache, entries that are neither event nor task, markup
escaping, and the dimming of a finished task.

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
one popup behind the bar's centre button. Configuration lives in
`niri/swaync/default.nix`; the stylesheet is `themes/swaync.css`.

Chrome is pinned to Linux system notifications, and its transient web
notifications are explicitly retained in the control center instead of
disappearing with their popup. After rebuilding, fully quit and reopen Chrome
so existing browser processes pick up the system policy.

### What moved

The top bar used to carry two hover drawers, `group/system` and
`group/hardware`, holding nine modules between them. Both are gone; their
contents are widgets in the control center instead, and the bar keeps one button
that opens it.

| Was | Is now |
| --- | --- |
| `pulseaudio` in `group/system` | A slider on each row of the control center's [media list](#the-media-list) |
| `temperature`, `memory`, `cpu`, `disk`, `network` in `group/hardware` | One live [`swaync-panel sysmon`](#the-readings-row) block, two readings to a line |
| `custom/media` in the centre | Still there, but now also the notification badge and the panel's opener; the same snapshot is the panel's [media list](#the-media-list) as well |
| `custom/ycal` on the left | Still there; today's events and tasks also appear as a panel row |
| `backlight` in `group/system` | Gone. Brightness stays on `XF86MonBrightness*` and `F5`/`F6` |
| `custom/battery` in `group/system` | Unchanged, but now the first module on the bar |
| `tray` at the right | Moved to the left, after the battery |

The battery deliberately stayed in the bar: it is the one reading worth seeing
without opening anything. `custom/audio` (Bluetooth and the default devices) and
`custom/network` (Wi-Fi) are Rofi menus of their own and were not touched.

The top bar is therefore `custom/battery`, `tray`, `custom/ycal` on the left,
`custom/media` and `custom/lyrics` in the centre, and `custom/timer`,
`custom/clipboard`, `custom/audio`, `custom/network` on the right. There is no
separate notification module: the centre button is it.

### The panel

The panel reads like GNOME's quick settings or the macOS Control Centre: a short
stack of small cards you take in at a glance, not a settings page. It is five
rows in a 380px-wide popup.

| Row | Widget | Source |
| --- | --- | --- |
| Header | `menubar#header` | A Do Not Disturb toggle pill and a Clear button |
| Calendar | `label#calendar` | [`swaync-panel calendar`](#the-calendar-row) |
| Notifications | `notifications` | The daemon itself |
| Media | `media` | [`media-control players`](#the-panel-list) |
| Readings | `label#sysmon` | [`swaync-panel sysmon`](#the-readings-row) |

There is no title row and no separate Do Not Disturb row: both collapse into the
header, which is most of what keeps the panel short.

Four of the five rows are driven by a command rather than by SwayNC itself,
which is what [the patches below](#the-upstream-patches) are for, and every one
of those commands is one of this repository's own Rust programs — there is no
shell or `jq` left in the panel's configuration. Two are `label` rows rendering
Pango markup; SwayNC turns the `#suffix` in a widget's name into a CSS class,
which is how `themes/swaync.css` tells `calendar` from `sysmon`.

### The media list

Every player that is playing or paused gets a row: a play/pause button beside
its title, the track's progress, and that player's own volume.

Browser progress comes through mprisence. Its upstream stale-timing guard used
to reject every large backward jump, including replaying a finished video or
dragging the seek bar backwards. `mprisence-position.patch` keeps that guard for
updates with a broken duration, but accepts a real restart or backward seek when
the duration remains stable.

```text
󰏤  Delulu — SZA
    Spotify  ·  1:23 / 4:56
    ▓▓▓▓▓░░░░░░░░░░░░░░░
    󰕾 ────────●───────────
```

The progress bar can be dragged to seek, and the volume slider is the only
volume control in the panel: there is no system slider above it any more. It is
the same control either way, and a slider attached to the track you can hear is
easier to aim at than one attached to a process name.

The widget itself knows nothing about MPRIS: it renders whatever
`media-control players` reports and calls that program back when a button or a
slider is used. A row whose player does not report a volume or a length — which
is common for browser tabs — simply gets no slider or no bar, rather than an
empty one.

Two things keep the sliders usable against a row that refreshes every second. A
refresh landing mid-drag would yank the handle back to the value already in
flight, so a slider ignores incoming values for two seconds after you move it,
and rows are updated in place rather than rebuilt. And dragging emits a value
per motion event, each of which would otherwise be a process, so the latest
value is sent once the drag has been still for 150ms — a drag across the row
costs a handful of commands rather than a hundred. The one-second refresh only
runs while the panel is actually open.

### What is deliberately not in the panel

**A system volume slider.** Every media row carries the slider that matters, and
one more above them would only ever be the thing you did not mean to move.
`custom/audio` in the bar is still the way to the default sink and its ports.

**Brightness.** SwayNC's `backlight` widget drives one named device under
`/sys/class/backlight`, which is a guess that goes wrong on any machine with a
different GPU. The function keys and `XF86MonBrightness*` already drive
`brightnessctl`, which finds the device itself.

**Power actions.** A panel that opens under the pointer is the wrong place for
Shut down. `Mod+Shift+E` and `Ctrl+Alt+Delete` quit the session, and
`Mod+Alt+L` locks it.

### The calendar and readings rows

Both are SwayNC `label` widgets running [`swaync-panel`](#swaync-panel), which is
where they are documented: today's events and tasks come from
[`calendar`](#the-calendar-row), the hardware figures from
[`sysmon`](#the-readings-row). Clicking `custom/ycal` in the bar still opens
waybar-ycal's own popup, which is where the full month and the task checkboxes
live.

### The bar button

The centre button is `custom/media`, and its label, classes and click actions
are all described under [media-control](#the-bar-button). `Mod+N` toggles the
panel and `F8` toggles Do Not Disturb, both through `swaync-client`.

### The upstream patches

Three patches are applied by the overlay at the top of
`niri/swaync/default.nix`, the same pattern used for Rofi in
`niri/rofi/default.nix`. The two widget patches keep everything which knows
about players, calendars or sensors in this repository's own programs rather
than moving into Vala, and they carry matching `configSchema.json` and man-page
entries. All three apply cleanly to v0.12.5, v0.12.6 and upstream `main`.

**`label-exec.patch`.** SwayNC's `label` widget shows a fixed string, so there
is no built-in way to put a program's output in the panel. The patch adds three
optional keys:

| Key | Meaning |
| --- | --- |
| `exec` | A command whose standard output becomes the label's text |
| `interval` | How often, in seconds, to re-run it. `0` re-runs it only when the control center opens |
| `pango-markup` | Whether that output is parsed as Pango markup |

Without `exec` the widget behaves exactly as it does upstream.

**`media-widget.patch`.** A label cannot hold a slider, and SwayNC's own `mpris`
widget is a carousel with neither volume nor progress, so the media list needed
a widget of its own. The patch adds `media`: a list of rows built from a
command's output, each with a play/pause button, a draggable progress bar and a
volume slider.

| Key | Meaning |
| --- | --- |
| `exec` | A command printing one tab-separated line per player: id, status, volume, position, length, title, subtitle |
| `interval` | How often, in seconds, to re-run it while the panel is open |
| `toggle-command` | Run when a row's button is pressed, with `$id` substituted |
| `volume-command` | Run when a row's volume slider moves, with `$id` and `$value` substituted |
| `seek-command` | Run when a row's progress bar moves, with `$value` a position in seconds |
| `empty-text` | Shown when no player is reported |

The widget holds no MPRIS code at all; it is a renderer with a callback.

**`retain-transient.patch`.** Chrome marks web notifications transient, which
normally makes SwayNC show only the floating popup and omit the notification
from control-center history. This patch lets an explicit matching
`notification-visibility` rule with state `enabled` retain it without muting
the popup; the configuration applies that rule only to Chrome/Chromium desktop
IDs.

### Theming

`themes/swaync.css` is linked to `~/.config/swaync/style.css` by
`themes/default.nix`, so edits apply without a rebuild — SwayNC re-reads it on
`swaync-client --reload-css`. SwayNC loads its packaged stylesheet first and this
one second at the same priority, so the file overrides rather than replaces
upstream: most of it is the palette shared with `themes/waybar.css` and the Rofi
`.rasi` themes, expressed through SwayNC's own CSS variables.

Both `label` rows supply their own colours as Pango markup, so their CSS is only
spacing, and both stay monospace: the readings need it for their two columns.
The media rows are real widgets rather than text, so they are styled properly —
`widget-media-row`, `-title`, `-subtitle`, `-progress`, `-volume` and `-toggle`,
all listed in the man-page entry the patch adds. The progress bar is a scale so
its whole track can be clicked or dragged, and is styled back down into looking
like a bar: a thinner track and smaller handle than the volume slider.

The same file sets the panel's density: 5-7px of card padding, 9px radii, a 6px
slider track with a 10px handle, 32px notification icons, and 11-13px text. If
the panel ever wants to breathe, those are the numbers to raise, along with
`control-center-width` and `control-center-height` in `niri/swaync/default.nix`.

### GNOME

The systemd unit is conditioned on `XDG_CURRENT_DESKTOP=niri`, like Waybar's.
GNOME runs its own notification daemon and two owners of
`org.freedesktop.Notifications` cannot coexist, so SwayNC stays out of that
session.
