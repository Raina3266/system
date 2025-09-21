{
  lib,
  config,
  pkgs,
  ...
}: {
  home.packages = with pkgs; [
    jellyfin
    jellyfin-web
    jellyfin-ffmpeg
    jellyfin-media-player
  ];
}
