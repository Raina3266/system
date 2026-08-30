# Custom Scripts

This repository contains seven Rust utilities used by the desktop configuration:

- `media-control` — a dynamic MPRIS controller for Rofi and Waybar
- `preview-panel` — a reusable GTK4 text and image preview window
- [`rofi-audio`](#rofi-audio) — a merged Bluetooth, output, and input controller with a Rofi interface
- [`rofi-clipboard`](#rofi-clipboard) — a clipboard + Memo manager with a Rofi interface
- `rofi-network-manager` — Wi-Fi and Ethernet controls with a Rofi interface
- [`waybar-timer`](#waybar-timer) — an interactive countdown timer for Waybar
- `webcam-crop` — an on-demand virtual webcam cropper and supervisor

## rofi-audio

`scripts/rofi-audio` replaces the former `custom/audio` shell script and
`custom/bt` Waybar module with one program and one Waybar entry. Bluetooth is
driven by [`bluer`](https://crates.io/crates/bluer), the official BlueZ crate,
and the audio tabs by [`pulsectl-rs`](https://crates.io/crates/pulsectl-rs) over
PulseAudio, which `pipewire-pulse` serves. Neither backend is reimplemented by
hand.

### Modes

Rofi opens on **Bluetooth** and starts scanning; `Shift+Left`/`Shift+Right`
move between tabs.

Discovery runs in a detached `scan-bg` process, so the menu appears at once
with the devices BlueZ already knows about rather than waiting out the scan
window. Rofi cannot redraw a script mode on its own, so devices found while the
window is open show up the next time the list is drawn — pressing **Scan**
again is the cheap way to do that, and it no longer blocks. An adapter you
turned off stays off: only the Scan button powers it on, never the automatic
scan at launch.

- **Bluetooth** — connected devices are cyan and sort to the top, paired
  devices are white, everything discovery turned up is dimmed. Battery level is
  appended when the device reports one.
- **Output** and **Input** — every sink or source, each row showing its volume
  before the name. The current default is cyan and stays in place. Volume is
  per device, so an idle output can be adjusted without touching the one
  currently playing. Monitor sources are hidden from **Input**.

Rows show a shortened device name: PulseAudio descriptions carry boilerplate
that identifies nothing ("GA104 High Definition Audio Controller Digital Stereo
(HDMI 2)" becomes "GA104 (HDMI 2)"), and when a name is boilerplate all the way
through, the active port's name — "Speakers", "Headphones" — is used instead.
Filtering still matches the full description, so typing any part of the long
name finds the row.

The input bar shows a filter glyph rather than a prompt. Rofi drives the prompt
widget from the mode's display name, which is the same string the mode-switcher
tab shows, so a prompt here could only repeat the tab beside it; the theme
leaves the widget out of the input bar's children and puts a static glyph in
its place.

The message panel below the list is one line and is only used by **Bluetooth**,
where it carries the highlighted device's address and pairing state. In
**Output** and **Input** the row already shows the volume and name, so the
panel stays empty until an action has something to report.

### Controls

| Action | Bluetooth | Output / Input |
| --- | --- | --- |
| `Enter`, double-click | Connect, or disconnect if connected | Make the row the default device |
| `Alt+1` | Refresh the list, and scan if the window has closed | — |
| `Alt+2` | Forget the paired device | — |
| `Alt+3`, `Alt+Up` | — | Volume +5% |
| `Alt+4`, `Alt+Down` | — | Volume −5% |

Connecting, disconnecting and confirming a device have no button of their own:
double-clicking a row does all three, so the action bar is left to the things a
row cannot say for itself. Single-click still just highlights a row.

The bar carries all four buttons at once and every one of them is live. Rofi
builds its widget tree once and does not re-run a script mode when you return
to a tab it has already drawn, so the bar can neither swap its buttons per tab
nor keep per-tab colouring in sync with the tab you are on — a dimmed button
would go stale the moment you switched back. The selected tab in the mode
switcher is what says which two buttons are meaningful; the other two fall back
to the nearest sensible action rather than doing nothing.

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
rofi-audio script <bluetooth|output|input>
rofi-audio connect-bg <row-key>
rofi-audio scan-bg
```

`script`, `connect-bg`, and `scan-bg` are internal: Rofi invokes the first,
and the Bluetooth tab spawns the other two.

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

Giving the slot to Bluetooth while the adapter is on costs nothing, because the
separate `pulseaudio` module in the top-left group already shows the volume and
its own mute glyph. The module also sets a `class` — `bluetooth-connected`,
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
