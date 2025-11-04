{
  lib,
  pkgs,
  nixosConfig,
  ...
}:
{
  imports = [
    ./jellyfin.nix
    ./cloud.nix
  ];
  config = lib.mkIf nixosConfig.services'.personal.enable {
    home.packages = with pkgs; [
      onedrivegui
      gui-for-clash
      wechat
      qq
      wemeet
      ytdownloader
      waylyrics
      whatsapp-for-linux
      inkscape
      shotcut
      qbittorrent
    ];
  };
}
