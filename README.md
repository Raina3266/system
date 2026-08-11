# Scripts

This repository contains two Rust utilities used by the desktop configuration:

- [`rofi-clipboard`](#rofi-clipboard) — a Wayland clipboard history manager with a Rofi interface
- [`waybar-timer`](#waybar-timer) — an interactive countdown timer for Waybar

## rofi-clipboard

`scripts/rofi-clipboard` is a clipboard history manager for Wayland and Rofi. It watches the clipboard with `wl-paste`, stores text and images locally, and presents the history through custom Rofi modes.

### Features

- Separate views for memos, captured text, and images
- Editable memos with selection-change autosave
- Text and image clipboard history
- Image previews inside Rofi
- Pin and delete actions
- One Edit action for soft-wrapped text editing and full image preview in a companion panel
- The open panel follows Rofi selection changes and saves modified text before switching items
- Keeps the highlighted clipboard item selected when search text is shortened or cleared
- Restores the selected item with its original MIME type
- Detects local image files copied from a file manager
- Shows the saved file path for Niri screenshots
- Removes missing linked local images and their cached previews when Rofi next renders
- Deduplicates repeated clipboard entries
- Ignores empty and sensitive clipboard values
- Keeps up to 2,000 history entries
- Uses file locking and atomic writes to protect the history file

### Requirements

- Rust and Cargo
- [Rofi](https://github.com/davatorium/rofi)
- [wl-clipboard](https://github.com/bugaevc/wl-clipboard)
- A Wayland session

### Controls

| Action | Result |
| --- | --- |
| `Enter` | Copy the selected item |
| `Alt+P` | Pin or unpin the selected item |
| `Alt+D` | Delete the selected item |
| `Alt+E` | Open text for editing or show the full image; press again to save and close the panel |
| `Up` / `Down` while panel is open | Save modified text, then show the newly selected text or image |

The interface contains three modes:

- **Memo** — editable notes created from the Edit action; pinned memos stay at the top
- **Text** — captured text entries
- **Images** — captured images with previews

Rofi opens in Memo mode. Clicking **Edit** there creates a new memo and opens
the companion editor. Clicking **Edit** again saves and closes it. While the
editor is open, moving through the Memo list saves the previous memo and loads
the newly selected one, matching the Text mode preview behavior.

### Commands

```text
rofi-clipboard [run]
rofi-clipboard capture
rofi-clipboard store --mime MIME
rofi-clipboard script <memo|text|images>
```

`capture` is designed to receive clipboard data from `wl-paste --watch`. The `store` command reads an item from standard input and stores it with the supplied MIME type.

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
| `ROFI_CLIPBOARD_SCREENSHOT_DIR` | Directory used to identify and label saved screenshots (default: `~/Pictures/Screenshots`) |
| `ROFI_CLIPBOARD_PREVIEW_WIDTH` | One-launch preview width override (configured default: `400`) |
| `ROFI_CLIPBOARD_PREVIEW_HEIGHT` | Preview height in pixels (default: `615`) |
| `ROFI_CLIPBOARD_PREVIEW_SIDE` | Place the preview to the `left` or `right` of Rofi (default: `left`) |
| `ROFI_CLIPBOARD_PREVIEW_GAP` | Space between the preview and Rofi in pixels (default: `10`) |
| `ROFI_CLIPBOARD_ROFI_WIDTH` | Rofi window width used for companion placement (default: `400`) |

Panel placement, size, and GTK CSS normally come from
`scripts/preview-panel/config.toml`. Home Manager links that file into the
runtime config directory, so valid saves hot-reload without rebuilding.
`[position].x` and `[position].y` move the panel from its automatic position
beside Rofi: positive `x` moves right and positive `y` moves down. The environment
variables above remain available for one-launch overrides.

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
