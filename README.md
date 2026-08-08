# Raina's NixOS system

A personal, reproducible NixOS configuration for an `x86_64-linux` workstation. The flake combines the system configuration and Home Manager setup for the `raina` host, with both GNOME and the [niri](https://github.com/YaLTeR/niri) scrollable-tiling Wayland compositor.

> [!WARNING]
> This repository is tailored to one user and one machine. It contains hardware UUIDs, a fixed username and home path, hardware-specific services, and personal defaults. Do not deploy it unchanged on another computer.

## Highlights

- Nix flakes with `nixos-unstable`, Home Manager, nixvim, nixGL, and Zed nightly
- GNOME and niri sessions with GDM
- A custom niri desktop built around Waybar, Walker, Rofi, and themed GTK applications
- Fish, Starship, Atuin, direnv, Neovim, Ghostty, Yazi, Git, and development toolchains
- PipeWire audio, NetworkManager with iwd, printing, fingerprint support, SSH, and GSConnect
- On-demand Immich and Jellyfin services grouped under `media-stack.target`
- Automated Google Drive synchronization using rclone bisync, systemd timers, and file watchers
- Region-select screenshot OCR for GNOME and niri
- A cropped virtual webcam backed by v4l2loopback and FFmpeg
- Small Rust utilities for Waybar timers and Rofi clipboard history

## Repository layout

| Path | Purpose |
| --- | --- |
| `flake.nix` | Flake inputs and the `raina` NixOS configuration |
| `nixos/configuration.nix` | Core host, user, boot, networking, locale, and Home Manager settings |
| `nixos/hardware.nix` | Machine-specific disks, filesystems, LUKS, and CPU configuration |
| `nixos/services.nix` | Desktop, audio, network, media, database, and system services |
| `nixos/webcam-crop.nix` | Cropped virtual webcam NixOS module |
| `home/default.nix` | Home Manager entry point and desktop applications |
| `home/niri/` | niri, Waybar, Walker, Rofi, themes, and key bindings |
| `home/shell/` | Shell, terminal, Git, and nixvim configuration |
| `home/yazi/` | Yazi configuration, plugins, theme, and keymap |
| `home/cloud/` | rclone bisync services, watchers, timers, and video sync |
| `scripts/` | Rust utilities used by the desktop configuration |

## Prerequisites

- A NixOS installation on `x86_64-linux`
- Git
- Internet access for the flake inputs and packages
- A UEFI system if you keep the included systemd-boot configuration
- An rclone remote if you keep the cloud synchronization modules

Flakes are enabled by this configuration. If the current system does not already support them, enable `nix-command` and `flakes` temporarily before the first rebuild.

## Adapting the configuration

Clone the repository to the path expected by its out-of-store theme links:

```bash
git clone https://github.com/Raina3266/system.git ~/System
cd ~/System
```

Before building, review and replace the machine-specific settings:

1. Generate hardware configuration for the target machine:

   ```bash
   nixos-generate-config --show-hardware-config > nixos/hardware.nix
   ```

2. Search for values tied to the original installation:

   ```bash
   rg -n 'raina|/home/raina|System|Europe/London|keyboard-gb'
   ```

   Update the flake output name, username, home directory, host name, repository path, timezone, locale, and keyboard layout as needed.

3. Review hardware-dependent configuration:

   - Intel graphics and microcode
   - the latest kernel selection
   - systemd-boot and EFI settings
   - the swap file and Btrfs layout
   - fingerprint and thermal services
   - `nixos/webcam-crop.nix`, including its camera udev rules

4. Review network-facing services and firewall settings. SSH, Jellyfin, Immich, Sunshine, and GSConnect/KDE Connect are configured in this repository.

5. Review `home/cloud/bisync.nix` and the rest of `home/cloud/`. Configure the referenced rclone remote with:

   ```bash
   rclone config
   ```

6. Remove any modules or packages that are not needed on the target system.

If the flake output remains named `raina`, evaluate and test the system with:

```bash
nix flake check
sudo nixos-rebuild test --flake .#raina
```

Once the test configuration works:

```bash
sudo nixos-rebuild switch --flake .#raina
```

If you rename `nixosConfigurations.raina` in `flake.nix`, use the new name after `#`.

## Common commands

Rebuild the current configuration:

```bash
sudo nixos-rebuild switch --flake ~/System#raina
```

Update all flake inputs, inspect the changes, and rebuild:

```bash
cd ~/System
nix flake update
git diff -- flake.lock
sudo nixos-rebuild switch --flake .#raina
```

Start or stop the on-demand media stack:

```bash
sudo systemctl start media-stack.target
sudo systemctl stop media-stack.target
```

Inspect the services grouped into it:

```bash
systemctl list-dependencies media-stack.target
```

Check the cloud synchronization timers:

```bash
systemctl --user list-timers 'rclone-bisync-*'
```

## Secrets and local state

Secrets are intentionally kept outside the repository. Fish optionally loads `~/.secrets.fish`, and rclone stores its credentials in its normal user configuration. Keep generated credentials, tokens, and machine-local secrets untracked.

The cloud synchronization setup also writes operational state and retained local backups beneath the user's XDG state/data directories.

## License

No license has been added. Unless a license is provided, the repository's contents remain under the copyright holder's default rights.
