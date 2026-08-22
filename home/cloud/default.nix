{ pkgs, ... }:
{
  imports = [ ./sync.nix ];

  home.packages = with pkgs; [
    fuse3
    rclone
  ];
}
