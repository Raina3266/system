{
  lib,
  pkgs,
  nixosConfig,
  ...
}:
{
  imports = [
    ./cloud.nix
    ./jellyfin.nix
  ];
  config = lib.mkIf nixosConfig.services'.personal.enable {
    home.packages = with pkgs; [
      gui-for-clash
      wechat
      qq
      ytdownloader
      waylyrics
      whatsapp-for-linux
      inkscape
      shotcut
      qbittorrent
    ];
  };
}
