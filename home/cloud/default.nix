{ pkgs, ... }:
{
  # The rclone systemd units are centralised in ../../nixos/systemd.nix.
  home.packages = with pkgs; [
    fuse3
    rclone
  ];
}
