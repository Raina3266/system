# Native Quickshell gap helper

These read-only commands are supported:

```
network-manager info <interface> <expected-ssid>
network-manager secret <interface> <expected-ssid> <expected-profile-uuid> <expected-bssid>
network-manager qr <interface> <expected-ssid> <expected-profile-uuid> <expected-bssid>
```

Pass an empty SSID for Ethernet; it is never a wireless wildcard. Invoke with
an argument array, not a shell-interpolated command. The UI owns all network
inventory, scanning, status, native connection operations and rendering.

The sole activation exception is `network-manager connect-wep <interface>
<expected-ssid>`, for initial/rejected unsaved static-WEP credentials unsupported
by Quickshell. Saved WEP activation remains native. Supply exactly one JSON object
on stdin, `{"password":"..."}`, then close stdin. Maximum input is 4096 bytes;
password must be a nonempty string without control characters, with no extra keys.
Never include the password in arguments. Success exits 0 and returns
`{"success":true,"interface":"...","ssid":"...","bssid":"...","profileUuid":"..."}`.
Failure uses the same safe error contract below; it does not imply rollback.

This command resolves an exact SSID and `WEP` security on the requested interface
using cached APs (`--rescan no`), chooses the first matching WEP BSSID, and passes
both interface and BSSID explicitly to `nmcli --ask --wait 60 device wifi connect`.
It never requests hidden-network scans. nmcli reuses a compatible available saved
profile, otherwise creates one; NetworkManager may persist its credentials as part
of normal profile management. The helper writes no credential files. Both child
output streams are discarded (including readline echo), and its sole password
line goes through a pipe. Activation is followed by active UUID/path, SSID, WEP
key-management, profile SSID and BSSID validation. No WPA connection command exists.

Upstream source verification: NetworkManager `src/nmcli/devices.c`,
`do_device_wifi_connect`, searches available connections with
`nm_access_point_connection_valid` before creating one. `src/nmcli/common.c`,
`nmc_readline_helper`, watches `STDIN_FILENO` with a GLib source and readline's
callback API, without a TTY requirement; `nmc_readline_echo` disables secret
history/redisplay. This supports newline-delimited pipe input for `--ask`.
Only one credential response is provided: repeated prompts, authorization failure,
or unsupported credentials fail safely instead of receiving additional input.
Sources: https://github.com/NetworkManager/NetworkManager/blob/main/src/nmcli/devices.c
and https://github.com/NetworkManager/NetworkManager/blob/main/src/nmcli/common.c .

## JSON contract

Each invocation prints exactly one JSON object and a newline. Success exits 0.
Failure exits 2, returns `{"error":"safe diagnostic"}`, and prints a diagnostic
to stderr. Subprocess output/errors are never included in diagnostics.

`info` returns all of the following keys, each a **string**:

| Key | Meaning |
| --- | --- |
| `interface`, `ssid` | Selected interface and active SSID; SSID is empty for wired |
| `profileId`, `profileUuid` | Active NetworkManager profile name and UUID |
| `security` | NM key management: empty for open, `none` for WEP, `wpa-psk`, `sae`, `owe`, enterprise key management values, or `wired` |
| `ipv4`, `ipv6` | Newline-separated CIDR addresses; empty when unassigned |
| `dns` | Newline-separated IPv4 then IPv6 DNS servers; empty when unassigned |
| `gateway4`, `gateway6` | Gateway addresses; empty when unassigned |
| `mac`, `bssid` | Device MAC and active access point BSSID |
| `frequency` | nmcli frequency string, e.g. `6115 MHz` |
| `band` | `2.4 GHz`, `5 GHz`, `6 GHz`, `60 GHz`, or empty if unknown |
| `channel`, `mode`, `rate` | nmcli radio strings, e.g. `33`, `Infra`, `100 Mbit/s` |

Radio fields are empty for wired. `info` never requests secrets.

`secret` returns `{"password":"...","interface":"...","ssid":"...","profileUuid":"...","bssid":"..."}`.
Open networks return an empty password.
WPA-PSK/SAE and the profile's selected WEP transmit key are supported.
Enterprise/managed, OWE, and wired credentials return explicit errors. Missing
or agent-owned secrets return an error, not a guessed password.

`qr` returns `{"image":"data:image/png;base64,...","interface":"...","ssid":"...","profileUuid":"...","bssid":"..."}`.
Its Wi-Fi payload includes
hidden status and escaped delimiters. It supports open/WEP/WPA and rejects the
same unsupported types as `secret`. Passwords and QR payloads go only through
qrencode stdin, never arguments or temporary files.

For `secret` and `qr`, both expected UUID and BSSID must be nonempty and match
the freshly resolved active identity exactly before any secret read or QR encoding.
First obtain this identity using nonsecret `info`; supply it on explicit Reveal
or the subsequent QR request. A same-SSID profile switch or AP roam is rejected.
Every successful mode includes `interface`, `ssid`, `profileUuid`, and `bssid`;
`info` retains its 17 string fields.

Read-only commands resolve the device's active UUID, never a saved profile by SSID or
name. Activated state, interface, SSID, UUID, active-connection object path and
AP BSSID are checked again after reads and before returning. QR rechecks after
encoding too. A detected switch/roam discards the result. As with any separate
process read, this cannot guarantee the connection stays unchanged after return;
the UI must discard pending responses when its selected device/SSID changes.

## Dependencies and validation

Runtime binaries: `nmcli`, `qrencode`. Override paths with
`NETWORK_MANAGER_NMCLI` and `NETWORK_MANAGER_QRENCODE` (also used by fake tests).
AP reads explicitly use `--rescan no`.

Cargo package ID remains `rofi-network`. Removed direct dependencies: `nmrs`
and `tokio`. Added: `serde_json = "1"` and `base64 = "0.22"`. Root workspace
and lock updates are intentionally left to the workspace owner.

After workspace lock reconciliation:

```
cargo test --manifest-path scripts/Cargo.toml -p rofi-network --locked
cargo build --manifest-path scripts/Cargo.toml -p rofi-network --locked
python3 scripts/network-manager/test_protocol.py scripts/target/debug/network-manager
```

Protocol tests use fake commands only and never read real saved credentials.
