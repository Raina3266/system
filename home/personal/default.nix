{
  lib,
  pkgs,
  config,
  ...
}:
{
  imports = [
    ./cloud.nix
    ./jellyfin.nix
  ];
  options = {
    personal.enable = lib.mkEnableOption "Personal stuff";
  };
  config = lib.mkIf config.personal.enable {
    home.packages = with pkgs; [
      obsidian
      discord
      kid3
      gui-for-clash
      wechat
      qq
      ytdownloader
      whatsapp-for-linux
      kdePackages.kdenlive
      gimp-with-plugins
    ];
  };
}
