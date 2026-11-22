# Custom Scripts

Desktop panels and utilities for this NixOS/Niri configuration. Panels use
AGS/Astal GTK4; Rust helpers handle custom policy and background tasks.

## Architecture rule for all custom scripts

**AGS/Astal owns the frontend and Astal-native integrations; Rust owns custom
backend functionality.** This rule applies to every repository-maintained panel
and utility, not just the media panel.

- Use JavaScript/TypeScript with AGS/Astal for UI rendering, widget bindings,
  interaction handling, and configuring or controlling features already exposed
  by Astal's native libraries.
- Implement backend features that Astal does **not** natively provide in Rust.
  This includes custom discovery or protocol adapters, system-data collection,
  business/policy decisions, processing, persistence, and external integrations.
  Using Gio/GLib, raw D-Bus, or subprocesses from JavaScript does not make such
  custom backend logic an Astal-native feature.
- Keep the frontend-to-Rust boundary thin: send requests, receive results, manage
  cancellation, and present state or errors. Do not duplicate backend decisions
  or maintain a second backend implementation in the frontend.
- Display formatting, widget/layout calculations, UI lifecycle, CSS reloading,
  and temporary interaction state belong in the frontend. Nix and launch wrappers
  may package, configure, and start components; custom backend work still belongs
  in Rust. Packaged upstream tools and build-time asset generators are not locally
  maintained runtime backends. Stateful runtime helpers are not exempt merely
  because their source is embedded in Nix.

The [architecture audit](#architecture-audit) records how the current source meets
this boundary. Keep new features on the same side of it; frontend bindings and
Rust helpers must be rebuilt and deployed together when their protocol changes.

## Quick access

| Feature | Open / use | Extra Waybar controls |
| --- | --- | --- |
| Audio & Bluetooth | Audio button | Middle-click: toggle Bluetooth power |
| Network | Network button | Middle-click: toggle Wi-Fi |
| Media | Centre media button | Middle-click: toggle all playback; right-click: next source |
| Dashboard | Battery button or `Mod+N` | `F8`: toggle Do Not Disturb |
| Clipboard | Clipboard button or `Mod+Alt+V` | — |
| App launcher | `Mod+F` | — |
| Countdown timer | Timer button | Left-click: +5 min; middle-click: start/pause; right-click: reset |
| Screenshot OCR | `Shift+Print` | Select an area to copy its text |
| Cropped webcam | Select **Cropped Webcam** in an app | Starts the physical camera on demand |

## Shared panel behavior

- Waybar opens panels on the clicked monitor. `Escape` or clicking outside closes
  them; toggling again on the same monitor closes them too.
- Opening Network closes Dashboard and Media; opening either of those closes
  Network. Audio stays independent.
- Panels use translucent cut-corner frames, inline feedback, and no hover
  tooltips. Waybar still has tooltips. Lists use thin, dim outlines: yellow on hover,
  red when selected, with a subtle 10%-opacity red tint. Selection stays red while
  hovered. Text-selection highlights in editors are unchanged.
- Audio, Clipboard, and Network have compact underlined tabs: cyan when inactive,
  red when selected, with no background tint on click.
- Network scans, dashboard metrics, and audio compatibility refreshes stop while
  hidden. Notifications and native audio/Bluetooth state continue updating.
- Hidden Network and Media panels defer GTK updates until opening, while native
  state and Waybar badges stay live; closing still clears secrets and cancels lookups.

## Audio & Bluetooth

Sources: [`scripts/audio-control/`](scripts/audio-control/).

- **Audio:** separate output and microphone controls, mute buttons, and device/port
  selectors. Selectors appear only when an alternative is available.
- Output volume is **0–150%**; microphone volume is **0–100%**. Volume bars show
  percentages and have 1% step buttons. Unavailable devices show `—`, not `0%`.
- Selecting an output changes the system default and moves active playback streams
  to it, overriding old per-app device pins. Recording targets are unchanged.
- **Bluetooth:** power, automatic discovery, pair/connect/disconnect, forget, and
  battery levels. Named devices come before address-only entries, with named
  unpaired devices first. Friendly aliases count as names. The list fits five
  unpaired devices plus any remembered rows above them; more devices scroll.
- Discovery runs only while the Bluetooth tab is open and the adapter is powered.
  Leaving the tab or closing the panel stops its scan and cancels pending pairing.
- Pairing shows status and any PIN/passkey/confirmation prompt inline. Input
  controls appear only when an answer is needed; **×** dismisses a finished notice.

## Network

Sources: [`scripts/network-manager/`](scripts/network-manager/).

- Manage Wi-Fi and Ethernet: connect, disconnect, choose saved profiles, and forget
  networks. Forget asks for confirmation, removes matching profiles, and disconnects.
- **Nearby:** connected, connecting, and detected networks; connected/saved entries
  come first. Wi-Fi titles show security and known signal strength. There are no
  redundant “Saved”/“Not saved” labels; empty status lines disappear.
- **Saved:** disconnected saved Wi-Fi networks that are not currently detected,
  with a live count and remove buttons. Rows move between tabs as detection changes.
- Scans run on opening and every **15 seconds** while open with Wi-Fi enabled.
- Nearby measures its rows to fit six unsaved entries below remembered/connected
  networks, including padding and wrapped names. Extra entries and small screens
  scroll. Connected/default devices in
  Network and Audio are not automatically selected or highlighted.
- A second-line chevron expands one network’s connection/profile details at a time. Passwords
  are fetched only when the eye button is pressed and cleared when hidden, when
  selection changes, or when the panel closes.
- Open, Enhanced Open, WEP, and personal WPA/WPA2/WPA3 are supported. Enterprise
  and hidden networks need a separately configured saved profile.
- Waybar prioritizes Ethernet. The card matches Audio’s **90%-opaque `#25252f`
  background and 1px yellow outline**.

## Media

Sources: [`scripts/media-control/`](scripts/media-control/).

- Artwork, title, artist/author, album, playback controls, seeking, and a source
  selector. Source selection is shared with Waybar even while the panel is closed.
- Play/pause actions group duplicate sources; Next/Previous target one publisher.
- The backend volume API is **0–100%**, using the player’s volume when writable,
  otherwise a safely matched audio stream. Ambiguous streams are not guessed;
  system output volume is never a fallback. Mute requires a matched stream and
  preserves volume. The Media UI has no volume or mute controls; use Audio.
- Physical output selection belongs in Audio, not Media.
- Badge requests reuse the current grouping model. Native changes still update it;
  silent audio-property changes are checked at most every five seconds during badge polling.
- Finds local/embedded artwork and YouTube/Bilibili author credits. Online credit
  lookups send the video ID to the site’s public endpoint, without cookies or credentials.

## Dashboard & notifications

Sources: [`scripts/dashboard/`](scripts/dashboard/).

- Battery/charging status, power profiles, CPU usage, memory, temperature, and disk
  usage. Metrics refresh while the dashboard is open.
- **Idle sleep:** None, 10 min, 30 min, 1h, 2h, 3h, or 6h. None disables automatic
  sleep. The saved choice stays active while closed, respects idle/suspend
  inhibitors, and suspends the computer rather than just turning off the screen.
  It does not automatically lock the session.
- Notification history keeps the latest **200 entries** across restarts.
- Up to **five** newest-first popups appear without taking keyboard focus. Default
  expiry is **8 seconds**, hovering pauses it, and critical/zero-timeout notices
  remain until dismissed. Application timeouts are respected.
- **Do Not Disturb** hides popups but retains non-transient history. Transient
  notices are not archived; old history entries do not retain expired actions.
  Restarting does not replay popups.

## Clipboard panel

Sources: [`scripts/clipboard/`](scripts/clipboard/).

- **Memo:** editable notes with a fresh **New memo** draft. **Text:** captured text
  and URLs. **Files:** local file references and images with previews.
- Searchable, pinnable history above an editor/preview, with five visible history
  rows. Each opening returns to Memo with nothing selected.
- Single-click previews; double-click copies and closes. Copying preserves text,
  image, or file-list formats as appropriate.
- Memo/Text edits autosave after **400 ms** of idle typing, and before switching,
  copying, or closing. Save failures preserve edits and block those actions until
  resolved. File/image previews are read-only; editing never modifies source files.
- **Deleting a linked file or named screenshot moves its source to Trash immediately,
  without confirmation.** Other deletions remove only history/cache data. Trash
  failures keep the entry; there is no permanent-delete fallback.
- Captures deduplicate; history is limited to **2,000 items** and **100 cached images**.
  Missing linked files are pruned on opening and while watched. Sensitive clipboard
  offers are ignored; clipboard contents are not logged.
- History lives in `$XDG_DATA_HOME/clipboard` (default `~/.local/share/clipboard`).
  The resident panel retains clipboard ownership while hidden; restarting it may
  lose the current selection.

| Shortcut | Action |
| --- | --- |
| Up/Down outside the editor | Select an entry |
| `Enter` outside the editor / `Ctrl+Enter` inside | Save, copy, close |
| `Ctrl+S` | Save now |
| `Ctrl+N` / `Ctrl+F` | New memo / search |
| `Alt+P` / `Alt+D` / `Alt+E` | Pin/unpin / delete / focus editor |
| `Escape` or outside click | Save and close |

Row pin and **×** buttons act on their own entry without copying it.

## App launcher

Sources: [`scripts/app-launcher/`](scripts/app-launcher/).

- Alphabetical app grid with icons and fuzzy search; installed-app changes refresh
  automatically. Opens at the bottom-left of the focused monitor.
- Click or press `Enter` to launch. Arrow keys navigate; `Home`/`End` jump to the
  ends; `Ctrl+F` focuses search. An empty search does not preselect an app.
- Successful launches close it; failures leave it open with error feedback.

## Countdown timer

Sources: [`scripts/waybar-timer/`](scripts/waybar-timer/).

Adds time in **5-minute steps**, up to **2 hours**, with start/pause/resume and
reset. Plays a three-beep alarm when finished. `waybar-timer` supplies Waybar JSON;
`waybar-timer add`, `toggle`, and `clear` control the running timer.

## Screenshot OCR

Sources: [`scripts/ocr-screenshot/`](scripts/ocr-screenshot/).

`ocr-screenshot` selects a screen region, recognizes **English, Korean, and
Japanese locally**, copies the text, and shows it in a notification. Cancelling
leaves the clipboard unchanged; an empty result still replaces it. Temporary
captures are removed, but recognized text remains in the clipboard/notification,
so avoid capturing secrets. Works in Niri and GNOME.

## Cropped webcam

Sources: [`scripts/webcam-crop/`](scripts/webcam-crop/).

- **Cropped Webcam** (`/dev/video10`, also `/dev/cam-cropped`) supplies a centred
  square crop: by default, 1280×720 input becomes 720×720 at 30 fps.
- `cropped-webcam.service` keeps a dark placeholder while idle, activates the real
  camera after an app opens the virtual device, and releases it when unused.
  Activation waits about 2 seconds; release takes about 3 seconds.
- Previews or apps retaining a camera handle can keep it active. This is not a
  hardware privacy switch; use the configured service rather than a second instance.

## Dolphin: Google Drive links

Uses upstream [dolphin-gdrive-actions](https://github.com/sktk4/dolphin-gdrive-actions),
packaged in [`scripts/packages.nix`](scripts/packages.nix) and configured in
[`home/cloud.nix`](home/cloud.nix). There is no locally maintained link helper.

- Right-click **one** local file or folder and choose **Copy Google Drive Link**.
  Supports items inside `~/GoogleDrive`, `~/Music`, and the `~/Videos` Drive mount.
  The configured root directories themselves are not supported by upstream.
- Looks up existing remote IDs only when clicked; no background service, uploads,
  sync runs, or sharing-permission changes. Recipients still need existing access.
- Requires an internet connection and the existing rclone login. New or renamed
  items must finish syncing first. Upstream does not apply the bisync exclusion
  list to link lookups; the bisync exclusions themselves remain unchanged.
- Private rclone aliases map the configured folders to the existing remote without
  changing its credentials. Nix manages the TOML configuration and Dolphin menu;
  do not run upstream's imperative `dga setup` installer.
- `dga copy-link /path/to/item` prints a URL; add `--clipboard` to copy it.
- Tracks upstream `main`, not a release tag. `switch.sh` refreshes this input before
  every switch; `update.sh` refreshes all inputs. To refresh only this project:
  `nix flake update dolphin-gdrive-actions`, then rebuild/switch. Nix still records
  the resolved commit in `flake.lock` so each build remains reproducible.
- The package runs upstream's pytest suite and an import check during its build.

## Helpers & commands

The resident Rust backends are `audio-control panel`, `media-control service`,
`panel-launcher backend network`, and `panel-launcher backend dashboard`.
They own custom policy, state reconciliation, timing, persistence, and guarded
action orchestration. Frontends render their results, marshal native snapshots,
and execute only Astal-native operations selected by Rust. NetworkManager’s
custom D-Bus integration lives entirely in Rust.

`clipboard-custom` owns capture, history, linked-source observation, deletion
validation, and Trash; its `observe` command streams invalidations while the panel
is open. GDK clipboard ownership stays in the resident GUI. Media artwork/credits
and the stateless `media-control model` command also remain available.

[`panel-launcher`](scripts/panel-launcher/) still starts packaged panels and
forwards their commands. Its `runtime` subcommands provide the focused-monitor
query, input-method toggle, calendar toggle, parent-death supervisor, and cloud
sync/watch orchestration. `media-control playerctl <command>` owns the media-key
playing-first selection policy; `mediactl` is only an exec wrapper.

The six panel commands are `audio-panel`, `network-panel`, `media-panel`,
`dashboard-panel`, `clipboard-panel`, and `app-launcher`. Each supports
`toggle [monitor]`, `close`, and `status` (Waybar JSON); `run` starts the resident
app and is normally managed by its service. Audio also has `bluetoothPower`,
Network has `toggleWifi`, Dashboard has `toggleDnd`, and Media has
`togglePauseAll`, `pauseAll`, and `cycleMedia`.

## Editing & checks

| Change | Apply it |
| --- | --- |
| Panel `style.css` under `scripts/` | Hot-reloads; no restart or rebuild |
| `niri/waybar/waybar.css` | Reloads automatically |
| `niri/config.kdl` | Reloads on save |
| Rust, JS/TS, Nix, packaged JSON, palettes, or service configuration | Rebuild and switch |

Panel code runs from immutable Nix-store bundles. Restarting a service or reopening
its panel does **not** load edited checkout code. Packaging/services live in
`niri/panels.nix` and `scripts/packages.nix`; shared colors are in `themes/default.nix`.

For checks, run `npm test` in a panel’s source directory or
`cargo test --manifest-path scripts/Cargo.toml --locked` from the repository root.
Normal frontend tests use simulated services; native/UI test modes have additional
runtime and isolation requirements documented in their test files. To check all
Rust components, use `cargo test --manifest-path scripts/Cargo.toml --workspace --locked`
and `cargo clippy --manifest-path scripts/Cargo.toml --workspace --all-targets --locked -- -D warnings`.

Deploy the migrated frontend bundles and Rust helpers together. In particular,
Niri’s input-toggle binding hot-reloads but needs the updated installed
`panel-launcher`; restarting an old panel does not install the new subcommands.

## Architecture audit

All audited repository-maintained runtime components now follow the rule in
source. **Compliant** describes the implementation boundary, not activation of
the edited checkout or a guarantee that every live desktop scenario has been
tested. UI state, display formatting, layout, CSS loading, native Astal calls,
and bounded/cancellable Rust IPC remain frontend responsibilities.

| Script / component | Status | Backend boundary |
| --- | --- | --- |
| Audio & Bluetooth | Compliant | Rust [`policy.rs`](scripts/audio-control/rust-custom/src/policy.rs) and [`audio.rs`](scripts/audio-control/rust-custom/src/audio.rs) own route reliability, selection transactions/rollback, stream-pin resets, discovery ownership, and post-pair trust/connect orchestration. JS captures raw Astal state and executes guarded native calls. |
| Network | Compliant | Rust’s `network_backend` in [`main.rs`](scripts/panel-launcher/src/main.rs) owns NetworkManager discovery/reconciliation, connection construction/activation, profile deletion, credential authorization, and scans. JS consumes opaque context stamps and action descriptors, renders state, and manages UI cancellation. |
| Media | Compliant | Rust [`model.rs`](scripts/media-control/rust-custom/src/model.rs) owns custom policy, selection, timing, and action transactions; [`playback.rs`](scripts/media-control/rust-custom/src/playback.rs) owns private-player recovery. Artwork, credits, and media-key selection also live in Rust. |
| Dashboard & notifications | Compliant | Rust’s `dashboard_backend` in [`main.rs`](scripts/panel-launcher/src/main.rs) owns telemetry, idle-sleep persistence/supervision, notification archive migration/retention, popup admission, and lifetime/hover policy. TS retains GTK rendering and guarded Astal notification operations/settings. |
| Clipboard | Compliant | Rust [`main.rs`](scripts/clipboard/rust-custom/src/main.rs) and [`store.rs`](scripts/clipboard/rust-custom/src/store.rs) own source classification/observation, deletion guards, Trash, history, and payload selection. Native GDK clipboard publication and UI editing remain in JS. |
| App launcher | Compliant | Rust’s `runtime focused-output` handles the external Niri query. AstalApps lookup/launch and GTK monitor/layout handling stay in [`backend.js`](scripts/app-launcher/backend.js). |
| Screenshot OCR | Compliant | Backend is in [`scripts/ocr-screenshot/`](scripts/ocr-screenshot/). |
| Countdown timer | Compliant | Backend is in [`scripts/waybar-timer/`](scripts/waybar-timer/). |
| Cropped webcam | Compliant | Backend is in [`scripts/webcam-crop/`](scripts/webcam-crop/). |
| Panel launcher | Compliant | Launch/request handling and shared backend/runtime entrypoints are in [`scripts/panel-launcher/`](scripts/panel-launcher/). |
| Shared stylesheet loader | Frontend-only | [`scripts/live-style.js`](scripts/live-style.js) loads and reloads UI styles. |
| Build/update entrypoints | Configuration/build glue | `switch.sh`, `update.sh`, ordinary Nix build/launch wiring, and [`themes/patch-daemon.py`](themes/patch-daemon.py) are build-time/configuration tools, not custom runtime backends. |

Locally maintained runtime helpers outside `scripts/` have been migrated too:

- [`niri/default.nix`](niri/default.nix): `mediactl` only execs the Rust media-key
  selector. Native player listing order and playing-first fallback are preserved;
  bounded queries never retry a playback command.
- [`home/cloud.nix`](home/cloud.nix): static rclone settings/excludes stay in Nix;
  Rust chooses resync from state and debounces filesystem events before scheduling
  sync. The packaged upstream inotify/rclone tools are adapters, not shell policy.
- [`scripts/packages.nix`](scripts/packages.nix): calendar PID validation,
  signalling, retries/fallback, and parent-death supervision are Rust runtime
  subcommands. The wrappers only pass configuration and exec them; the local C
  supervisor has been removed.
- [`niri/config.kdl`](niri/config.kdl): the English/Chinese input binding invokes
  Rust’s `runtime input-toggle`, rather than inspecting state in inline shell.

These source migrations do not activate themselves. Use the rebuild/switch step
above to install the matching helpers, bundles, and configuration. Packaged
upstream applications and build-time UI asset/type generators remain outside the
custom runtime-backend rule.
