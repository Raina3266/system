# Custom Scripts

This repository contains six Rust utilities used by the desktop configuration:

- [`audio-control`](#audio-control) — a Bluetooth manager and audio mixer for devices and playback streams
- `preview-panel` — a reusable GTK4 text and image preview window
- [`rofi-clipboard`](#rofi-clipboard) — a clipboard + Memo manager with a Rofi interface
- `rofi-network` — Wi-Fi and Ethernet controls with a Rofi interface
- [`waybar-timer`](#waybar-timer) — an interactive countdown timer for Waybar
- `webcam-crop` — an on-demand virtual webcam cropper and supervisor

It also documents the [Wayle dashboard](#wayle-dashboard) opened from the
far-left Waybar button and the [Bluetooth and audio panel](#bluetooth-and-audio-panel)
opened from the audio button.

## audio-control

`scripts/audio-control` replaces the former `custom/audio` shell script and
`custom/bt` Waybar module with one program and one Waybar entry. Bluetooth is
driven by [`bluer`](https://crates.io/crates/bluer), the official BlueZ crate,
and the audio tabs by [`pulsectl-rs`](https://crates.io/crates/pulsectl-rs) over
PulseAudio, which `pipewire-pulse` serves. Neither backend is reimplemented by
hand.

The package in `scripts/packages.nix` and the existing Waybar launcher provide
all four tabs.

The Waybar button now opens Wayle's
[Bluetooth and audio panel](#bluetooth-and-audio-panel), which has the same four
tabs. `audio-control` keeps the Rofi menu described below — run it from a terminal
or a binding of your own — and stays the Waybar entry's status backend and its
right-click Bluetooth switch.

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
The popup width is configured in `themes/audio-control.rasi`.
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
audio-control [launch]
audio-control status
audio-control bluetooth-power [on|off|toggle]
audio-control script <bluetooth|output|input|playback>
audio-control connect-bg <row-key>
audio-control scan-bg
```

`script`, `connect-bg`, and `scan-bg` are internal: Rofi invokes the first,
and the Pair tab spawns the other two.

### Waybar module

```jsonc
{
  "custom/audio": {
    "exec": "/path/to/audio-control status",
    "interval": 5,
    "return-type": "json",
    "escape": false,
    "on-click": "busctl --user call com.wayle.Shell1 /com/wayle/Shell com.wayle.Shell1 DropdownToggle ss audio eDP-1",
    "on-click-right": "/path/to/audio-control bluetooth-power toggle"
  }
}
```

The configured `on-click` opens Wayle's
[Bluetooth and audio panel](#bluetooth-and-audio-panel) on the monitor whose
button was clicked; each Waybar instance passes its own output name. Use
`/path/to/audio-control` instead to keep the Rofi menu on the button.

The text is a single glyph:

| Bluetooth | Glyph |
| --- | --- |
| Off | 󰕿 󰖀 󰕾 by the default output's level, 󰝟 when it is muted |
| On, nothing connected | 󰂯 |
| On, device connected | 󰂱 |

Giving the slot to Bluetooth while the adapter is on costs nothing: the level is
back as soon as the adapter is off, and the panel the button opens carries a
volume slider besides. The module also sets a `class` — `bluetooth-connected`,
`bluetooth-on`, `muted`, `active`, or `unavailable` — so the glyph can be
recoloured per state from `waybar.css`.

The tooltip carries the rest: the default output, the default input, and any
connected Bluetooth devices. Right-click is the Bluetooth adapter's on/off
switch; `bluetooth-power on` and `off` are available for bindings of your own.

### Environment variables

| Variable | Purpose |
| --- | --- |
| `AUDIO_CONTROL_ROFI` | Override the `rofi` executable |
| `AUDIO_CONTROL_THEME` | Override the Rofi theme path (default: `$XDG_CONFIG_HOME/rofi/audio-control.rasi`) |
| `AUDIO_CONTROL_SCAN_SECONDS` | Length of the Bluetooth discovery window (default: `10`) |

Styling lives in `themes/audio-control.rasi`, symlinked to
`~/.config/rofi/audio-control.rasi` so edits apply without a rebuild.

### Development checks

From the repository root, with its Rust development environment:

```sh
nix develop .#rust
cargo fmt --manifest-path scripts/Cargo.toml --package audio-control -- --check
cargo test --manifest-path scripts/Cargo.toml --package audio-control --locked
cargo clippy --manifest-path scripts/Cargo.toml --package audio-control --locked -- -D warnings
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

## Wayle dashboard

`Mod+N` and the far-left Waybar button open Wayle's dashboard in a monitor-local
layer-shell window. The button shows the current battery percentage and uses the
same charging, low, warning and blinking-critical states as the old battery
module.

The dashboard is the agenda and the notification list. It uses Wayle's own
notification service and native notification groups, icons, actions, dismiss
controls, Do Not Disturb switch and Clear All button. Each group initially shows
at most three messages and the bounded notification area scrolls when its
contents are taller. Chrome and Chromium transient notifications are retained in
Wayle's history.

A notification shows two lines of its title and two of its body; the chevron
expands both and collapses them again. The chevron appears whenever collapsing
hides something — a body, or a summary long enough to need a third line.

Everything the other Waybar buttons already own has been taken out of it: the
Bluetooth and Do Not Disturb tiles, the volume card, and the battery and network
row. Quick actions are one row — airplane mode, idle inhibit and the power
profile. Airplane mode still turns Bluetooth off and back on. Do Not Disturb is
the switch beside the notification card's title. The header's settings button is
gone too: this Wayle configuration is declared in Home Manager, so anything
Wayle's settings app writes is replaced on the next rebuild. The space that
frees goes to the two lists: the agenda grows from 190 to 320 pixels,
notifications from 270 to 460, and the panel itself from 760 to 900 before it
scrolls. Each is a ceiling, so a quiet day still gets a short panel.

Every card title — `CALENDAR`, `NOTIFICATIONS`, `SYSTEM` — is drawn in the
palette's red, the colour the calendar's date range already used.

The calendar card is the one small adapter Wayle does not provide natively. It
reads the next seven days from `~/.cache/waybar-ycal/events.json`, the same cache
written by `waybar-ycal`, and renders the result with Wayle widgets and styling.
A timed event's `HH:MM-HH:MM` is split off the label that cache stores and shown
on its own line above the event. The old standalone GTK notification/calendar
panel and its Waybar notification button have been removed.

The separate Wi-Fi button opens Wayle's native network manager, where networks
can be scanned, selected and connected. Each Waybar instance passes its output
name, so the panel appears on the display that was clicked. Wi-Fi is no longer
duplicated in the dashboard. The existing right-side network button remains the
Rofi Wi-Fi/Ethernet manager.

The centre media button opens a separate native Wayle media panel. It lists
every MPRIS source that is playing or paused, with artwork, source, track,
artist, album, an adjustable progress bar, and independent transport,
shuffle, and repeat controls. Each card's first line carries the source and the
artist together, an icon in front of each saying which is which, so neither
needs a line of its own; the cover, the title and the album sit below it, and
the elapsed and total times sit at the ends of the control row rather than on a
row of their own. The cover is a 3.5rem square that crops rather than stretches
whatever the player publishes, and a player that publishes nothing gets the same
square with a disc in it, so cards stay the same size either way. Like every
other card in Wayle, they are painted on the palette's elevated layer.

The panel is only as tall as the cards it holds, up to four of them; a fifth
source is reached by scrolling. Its height comes from measuring a card once the
panel is on screen rather than from a fixed number, so it follows the
configured scale and stays right if the card's contents change. Two players
therefore get a short panel instead of a tall one with space under it.

The play/pause button sends `Play` or `Pause`, not `PlayPause`. A toggle leaves
the decision to the player, and `mprisence`, which republishes a browser tab as
its own MPRIS player, decides from its own copy of the tab's state; when the two
disagree the toggle resolves to the state the tab is already in and the press
does nothing. Naming the command also makes it idempotent, so a card that stands
for both a bridge and the player it mirrors sends to both and lets whichever one
is listening act. Right-clicking the Waybar media button pauses everything at
once, through `playerctl --all-players pause`.

Three things decide the direction of that command, because a card that gets any
of them wrong is a card whose play/pause button appears to do nothing:

- The direction is read from the player at the moment of the press, not from
  the copy the card holds. The card is only told about changes while the panel
  is open, so a press moments after it opens would otherwise send whichever
  command the player is already obeying.
- A card is rebuilt when the player behind it is replaced, not only when the
  list of bus names changes. `mprisence` claims the same name each time it
  recreates a player for a tab, and a card left holding the replaced one is
  frozen: its state never changes again, so every press sends the command that
  player is already in.
- The players a card stands for are recorded as they are folded into it, rather
  than worked out afterwards from a resemblance test. The bridge rule pairs a
  bridge with the browser it mirrors even though the two disagree about their
  identity, and those are exactly the pairs that most need both ends to hear the
  command.

If the player is still where it started after all that, the card falls back to
`PlayPause`. MPRIS asks a player that cannot honour a command to ignore it
rather than report an error, and some publishers answer `Play` and `Pause` that
way while still acting on the toggle. The fallback waits long enough that a
player which was merely slow is not sent back where it came from, and it goes
only to the card's own player, never to the duplicates that already took the
idempotent command.

Hover the active Wi-Fi connection in Wayle and press `Info` for the SSID,
signal, saved profile and UUID, security, interface, password, IP addresses,
DNS, BSSID, frequency and band, channel, mode, and link rate. Press `QR` for a
larger share code; open, WEP, WPA/WPA2 and WPA3 Personal profiles are supported,
while Enterprise and Enhanced Open profiles show an explanation instead.

`Info` resolves the access point from the list Wayle keeps live rather than from
the device's cached `ActiveAccessPoint` path. `wayle-network` reads that path
once, when it builds the Wi-Fi model, and never refreshes it; NetworkManager
gives an access point a new object path every time it recreates one, which it
does after scans, roams and reconnects, so the cached path goes stale and asking
for it fails with `object not found at path`. The cached path is used while it
still names an access point that exists, and otherwise the strongest access
point advertising the connected SSID stands in. If neither is available, the
signal and frequency fall back to the values the Wi-Fi model itself publishes
and the remaining radio rows read **Unknown**, instead of the whole panel being
replaced by an error.

### Bluetooth and audio panel

The right-side audio button opens Wayle's Bluetooth and audio panel in its own
monitor-local layer-shell window. It has the same four tabs as
[`audio-control`](#audio-control), built from Wayle's own components wherever Wayle
provides one:

| Tab | Contents | Native |
| --- | --- | --- |
| Pair | Wayle's Bluetooth dropdown: paired and discovered devices, connect, disconnect, forget, the adapter switch, the scan button, and the pairing card for PINs and passkeys | yes |
| Output | The default output's volume and mute, then every output device and its available ports | ports added |
| Input | The default input's volume and mute, then every microphone and its available ports | ports added |
| Play | Wayle's per-application volume list, with a route button at the end of each stream's row | route added |

The audio-specific additions reuse `audio-control`'s profile-aware selection
backend, so **Speaker** and **Headphones** remain separate destinations even
when the laptop exposes them through mutually exclusive ALSA profiles. Selecting
one revalidates the card and port, chooses a compatible profile that preserves
the current microphone inputs, waits for the new sink, and then makes it the
default. USB, Bluetooth and virtual devices use the same list. Rows show only
the distinguishing port or model name; the full PulseAudio description remains
in the tooltip. Ports reported as unplugged are hidden and unknown availability
stays selectable. The panel is two thirds of its former width. The route button
opens the native device list for one playback stream and calls Wayle's
`move_to_device` instead of changing the system default, so the rest of the
system keeps playing where it was; the picker stays open with the stream's
current destination checked, and `Back` returns to Play. A stream that ends while
its picker is open is not routed.

The panel is opened over D-Bus, like the dashboard and the media and Wi-Fi
panels:

```sh
busctl --user call com.wayle.Shell1 /com/wayle/Shell com.wayle.Shell1 \
  DropdownToggle ss audio eDP-1
```

Switching to Pair, and opening the panel while Pair is the current tab, starts a
timed Bluetooth discovery. As in `audio-control`, that leaves a powered-off adapter
off: use the tab's switch, or right-click the Waybar button.

`wayle-audio` itself has no card/profile API, so a small machine interface calls
the already-tested `audio-control` logic rather than duplicating that hardware
policy inside the Wayle patch. It also reconciles Wayle's reactive default after
a profile change: PulseAudio can announce the new default before Wayle has added
the replacement sink, which otherwise leaves the removed Speaker object shown
in the header after Headphones became live.

### What is still patched

The local patches are:

| Patch | Purpose |
| --- | --- |
| `notification-history.patch` | Keeps Chrome and Chromium transient popups in Wayle's native history. |
| `dashboard-waybar-host.patch` | Adds the D-Bus dropdown request used by the external Waybar and removes the dashboard's session power actions. |
| `dashboard-layer-window.patch` | Hosts the native dashboard in a real monitor-local layer-shell window. A Waybar click belongs to a different Wayland client, so Niri cannot reliably grant Wayle's old GTK popover the required popup grab. |
| `wayle-wifi.patch` | Gives Wayle's network manager its own monitor-local Waybar window, adds complete active-connection information from the live access-point list rather than the device's stale cached path, and generates a large inline QR code from the active NetworkManager profile without putting its password in argv or a temporary file. |
| `dashboard-power-profile.patch` | Makes the dashboard power-profile action cycle through every profile supported by the machine. |
| `wayle-media-panel.patch` | Removes the duplicate dashboard Wi-Fi tile and turns Wayle's native single-player media dropdown into a centred, monitor-local list of every playing or paused source, with equally sized cards that pair the source with the artist on one line, a panel that is as tall as the cards it holds up to four of them, and a play/pause button that names the command, aims it at the live player, and falls back to a toggle only if nothing moved. |
| `dashboard-notifications.patch` | Replaces the dashboard's Now Playing card with Wayle's native notification groups plus a seven-day calendar adapter, and adds expandable notification bodies. |
| `wayle-audio-panel.patch` | Turns Wayle's audio dropdown into the tabbed [Bluetooth and audio panel](#bluetooth-and-audio-panel) and gives it a monitor-local Waybar window. |
| `wayle-audio-profile-bridge.patch` | Uses `audio-control`'s stable card/port choices in Wayle, repairs profile switching and stale defaults, shortens device labels, and narrows the panel. |
| `dashboard-slim.patch` | Removes the dashboard cards the audio, Wi-Fi and dashboard buttons already cover, and gives the space to the agenda and notification lists. |
| `dashboard-polish.patch` | Drops the header's settings button, moves Do Not Disturb beside the notification card's title, expands a notification's title along with its body, puts an event's time above the event, and colours every card title red. |
| `mprisence-position.patch` | Unrelated to Wayle: it stops mprisence clamping a browser's position backwards after a replay or a backward seek. That is a fix to what it publishes, so no reader can correct it. |

The Wayle patches apply to v0.7.0.

### Verifying a change

`cargo test -p control-centre` covers the remaining Waybar media-title streamer.
The seven-day agenda has a unit test in `dashboard-notifications.patch`; the
device/port rules and profile bridge have focused parsing, naming and row-shape
tests. The Wayle patch stack is checked against v0.7.0 before updates are
committed.

```sh
cargo test -p control-centre
```

Wayle's own tests need a checkout of v0.7.0 with the patch stack applied:

```sh
cargo test -p wayle-shell --lib
```
