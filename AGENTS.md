# Panel architecture

- Use AGS/Astal's existing native APIs for every capability they expose. Do not patch Astal, monkey-patch its implementation, or add parallel audio/Bluetooth backends.
- Keep native service state and native mutations in Astal. Missing metadata or custom transaction policy does not justify executing an exposed native operation in Rust.
- Rust helpers own only functionality Astal genuinely lacks, such as omitted route/profile associations, custom policy, request-local PIN/passkey agents, and HID++ battery publication. Document inseparable exceptions precisely.
- Remove superseded code and unused compatibility commands after migration; do not retain a second implementation just for hypothetical compatibility.
- Audio/media Astal UI, runtime manifests, and tests belong under `scripts/{audio-control,media-control}/astal/`; custom Rust functionality belongs under each `rust-custom/` directory. Panel launcher Nix files belong directly under `niri/panels/`.
