# rofi-audio

A Rofi Bluetooth manager and PipeWire/PulseAudio mixer, implemented in Rust.
The existing `scripts/packages.nix` package and Waybar launcher run all five
tabs; there are no extra runtime commands or new dependencies to install.

## Tabs

| Tab | Rows | Enter / double-click |
| --- | --- | --- |
| Bluetooth | Discovered and paired Bluetooth devices | Pair/connect or disconnect, as before |
| Output | Audio output devices | Set the selected output as default |
| Input | Microphones and other non-monitor inputs | Set the selected input as default |
| Playback | Live playback streams, with app, volume and destination | Choose that stream's output |
| Recording | Live capture streams, with app, volume and source | Choose that stream's input |

Applications may expose several streams. They remain separate; no MPRIS support
is required. Start playback or recording in an application for its stream to
appear. Muted or paused streams are dimmed, and lists refresh every two seconds.
The popup is 640 px wide to accommodate all five tabs and routing information.

## Buttons and shortcuts

Buttons operate on the selected row, not necessarily the system default.

| Button | Shortcut | Action |
| --- | --- | --- |
| Scan | Alt+1 | Scan for Bluetooth devices |
| Forget | Alt+2 | Forget the selected Bluetooth pairing |
| Volume up | Alt+3 / Alt+Up | Increase selected device or stream volume by 5 percentage points |
| Volume down | Alt+4 / Alt+Down | Decrease selected device or stream volume by 5 percentage points |
| Mute | Alt+6 | Toggle mute on a selected device or stream, retaining its volume |
| Route | Alt+7 | Open the Playback/Recording stream's device picker |
| Port | Alt+8 | Choose a port exposed by an Output/Input device |
| Back | Alt+9 | Leave a picker without applying a change |

Alt+5 is reserved for the existing refresh action. Escape closes the entire
menu. In a picker, use the permanent **Back** row or Alt+9 to return instead.
Choose a destination/port with Enter; the current choice is checked and cyan.
The search filter resets on actions, as before, so searching for an application
does not hide all devices when its picker opens.

Outputs and playback streams can reach **150%**; inputs and recording streams
are capped at **100%**. Amplification above 100% can distort. Adjustments preserve
an existing channel balance, with the ceiling applied to the loudest channel.
The normal volume controls do not unmute a muted device or stream.

Routing changes only the selected stream, never the system default. The
Recording picker also offers monitor sources for recording desktop audio; these
remain hidden in the normal Input tab. Persistence across application restarts
is managed by PipeWire/WirePlumber, not this program.

Port selection does not change a card profile. Ports reported as unplugged are
labelled and disabled; ports with unknown availability remain selectable. Some
devices expose no selectable ports, in which case the picker only offers Back.
Bluetooth profiles, codecs, channel editing, latency offsets and digital
passthrough configuration are intentionally not included.

Normal audio tabs do not show a status panel. Picker instructions and errors
are shown when necessary. If a stream ends or a device is unplugged while its
picker is open, the program revalidates the target rather than acting on a
different row. Volume/mute/routing changes check the audio server's response.

## Development checks

From the repository root, with its Rust development environment:

```sh
nix develop .#rust
cargo fmt --manifest-path scripts/Cargo.toml --package rofi-audio -- --check
cargo test --manifest-path scripts/Cargo.toml --package rofi-audio --locked
cargo clippy --manifest-path scripts/Cargo.toml --package rofi-audio --locked -- -D warnings
```

Unit tests include volume conversion/limits, channel balance, stream identity,
picker cancellation and dispatch, disappearing targets, rejected choices,
state round trips, row rendering and refresh behavior without hardware.

For live checks, use a disposable PulseAudio/PipeWire session where possible:

1. Mute and unmute an output and microphone; verify each previous volume stays.
2. Start two playback applications and adjust/mute one; verify the other is unchanged.
3. Route one stream to a second output; verify the global default is unchanged.
4. Start recording, choose a microphone or output monitor, and verify the source.
5. Switch an available physical port, then check cancellation and unplugging.
6. Stop a stream with its picker open; verify that Back and refresh remain usable.
7. Check 100→105→150% output volume, the 150% ceiling, and the 100% input ceiling.
8. Check Bluetooth scan/connect/forget and pairing-code entry still work.
