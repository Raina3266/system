# Rofi: application launcher / dmenu (replacing walker incrementally)
{ pkgs, ... }:
{
  home.packages = with pkgs; [
    rofi-vpn
    rofi-network-manager
    rofi-bluetooth
  ];

  programs.rofi = {
    enable = true;
    package = pkgs.rofi;
    theme = ./cyberpunk.rasi;

    location = "top-right";
    xoffset = 5;
    yoffset = 44;
    cycle = false;

    modes = [ "combi" "filebrowser" "recursivebrowser" ];
  };
}
