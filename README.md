# Scripts

This repository contains two Rust utilities used by the desktop configuration:

- [`rofi-clipboard`](#rofi-clipboard) — a Wayland clipboard history manager with a Rofi interface
- [`waybar-timer`](#waybar-timer) — an interactive countdown timer for Waybar

Both projects use Rust 2024 edition and can be built independently with Cargo.

## rofi-clipboard

`scripts/rofi-clipboard` is a clipboard history manager for Wayland. It watches the clipboard with `wl-paste`, stores text and images locally, and presents the history through custom Rofi modes.

### Features

- Separate views for pinned items, text, and images
- Text and image clipboard history
- Image previews inside Rofi
- Pin, delete, and edit actions
- Full-text preview toggle
- Restores the selected item with its original MIME type
- Detects local image files copied from a file manager
- Deduplicates repeated clipboard entries
- Ignores empty and sensitive clipboard values
- Keeps up to 2,000 history entries
- Uses file locking and atomic writes to protect the history file

### Requirements

- Rust and Cargo
- [Rofi](https://github.com/davatorium/rofi)
- [wl-clipboard](https://github.com/bugaevc/wl-clipboard)
- A Wayland session

### Build

```bash
cd scripts/rofi-clipboard
cargo build --release
```

The executable is created at:

```text
target/release/rofi-clipboard
```

### Run

Start the clipboard watcher:

```bash
wl-paste --watch ./target/release/rofi-clipboard capture
```

Open the clipboard interface:

```bash
./target/release/rofi-clipboard
```

The repository's Home Manager configuration builds the program, wraps its Rofi and wl-clipboard paths, and runs the watcher as a user service.

### Controls

| Action | Result |
| --- | --- |
| `Enter` | Copy the selected item |
| `Alt+P` | Pin or unpin the selected item |
| `Alt+D` | Delete the selected item |
| `Alt+E` | Edit a text item |
| `Alt+V` | Toggle the preview layout |
| `Ctrl+Enter` | Save edited text |

The interface contains three modes:

- **Pinned** — items marked for quick access
- **Text** — captured text entries
- **Images** — captured images with previews

### Commands

```text
rofi-clipboard [run]
rofi-clipboard capture
rofi-clipboard store --mime MIME
rofi-clipboard script <pinned|text|images>
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
| `ROFI_CLIPBOARD_PREVIEW` | Start Rofi with the preview layout enabled |

## waybar-timer

`scripts/waybar-timer` is a small countdown timer that outputs Waybar-compatible JSON. The main process owns the timer state, while command invocations communicate with it over a Unix datagram socket.

### Features

- Adds time in five-minute steps
- Supports countdowns up to two hours
- Start, pause, resume, and clear actions
- Emits JSON only when the displayed state changes
- Uses a per-user Unix socket for commands
- Plays a three-beep alarm when the countdown finishes
- Cleans up stale socket files when it starts

### Requirements

- Rust and Cargo
- Waybar
- `ffplay` from FFmpeg for the alarm sound

### Build

```bash
cd scripts/waybar-timer
cargo build --release
```

The executable is created at:

```text
target/release/waybar-timer
```

### Commands

Run the continuous Waybar module:

```bash
./target/release/waybar-timer
```

Control the running timer from another process:

```bash
./target/release/waybar-timer add
./target/release/waybar-timer toggle
./target/release/waybar-timer clear
```

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

With this configuration:

- left click starts or pauses the timer
- middle click adds five minutes
- right click clears the timer

### Socket and alarm configuration

The command socket is normally created at:

```text
$XDG_RUNTIME_DIR/waybar-countdown.sock
```

If `XDG_RUNTIME_DIR` is unavailable, the program creates a user-specific socket in the system temporary directory.

Set `WAYBAR_TIMER_FFPLAY` to use a specific `ffplay` executable:

```bash
WAYBAR_TIMER_FFPLAY=/path/to/ffplay ./target/release/waybar-timer
```

The repository's Nix configuration sets this automatically when packaging the utility.
