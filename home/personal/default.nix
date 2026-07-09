{
  lib,
  pkgs,
  nixosConfig,
  ...
}:
{
  config = lib.mkIf nixosConfig.services'.personal.enable {
    home.packages = with pkgs; [
      onedrivegui
      wechat
      qq
      wemeet
      whatsie
      inkscape
      shotcut
      qbittorrent
      android-tools
      immich
      jellyfin
      jellyfin-web
      nightingale
    ];
  };
}
