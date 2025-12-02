{
  lib,
  pkgs,
  nixosConfig,
  ...
}:
{
  imports = [
    ./jellyfin.nix
  ];
  config = lib.mkIf nixosConfig.services'.personal.enable {
    home.packages = with pkgs; [
      onedrivegui
      wechat
      qq
      wemeet
      ytdownloader
      waylyrics
      wasistlos
      inkscape
      shotcut
      qbittorrent
    ];
  };
}
