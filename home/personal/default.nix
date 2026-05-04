{
  lib,
  pkgs,
  nixosConfig,
  ...
}:
{
  imports = [
  #  ./cloud.nix
  ];
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
      jellyfin
      jellyfin-web
    ];
  };
}
