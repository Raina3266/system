{
  lib,
  config,
  pkgs,
  ...
}:
{
  config = lib.mkIf config.personal.enable {
    home.packages = with pkgs; [
      jellyfin
      jellyfin-web
      jellyfin-ffmpeg
    ];
  };
}
