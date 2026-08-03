# Rofi: application launcher / dmenu (replacing walker incrementally)
#
{ pkgs, config, ... }:
{
  home.packages = with pkgs; [
    rofi
    rofi-rbw
    rofi-vpn
    rofi-network-manager
    rofi-bluetooth

    (writeShellScriptBin "rofi-preview" (builtins.readFile ./preview.sh))
    bat
    ansifilter
    imagemagick
    poppler-utils
    ffmpegthumbnailer
    file
    
    (writeShellScriptBin "rofi-filesearch" (builtins.readFile ./filesearch.sh))
    fd
  ];

  xdg.configFile."rofi/config.rasi".source =
    config.lib.file.mkOutOfStoreSymlink
      "/home/raina/System/home/niri/rofi/config.rasi";
  xdg.configFile."rofi/cyberpunk.rasi".source =
    config.lib.file.mkOutOfStoreSymlink
      "/home/raina/System/home/niri/rofi/cyberpunk.rasi";
  xdg.configFile."rofi/filepreview.rasi".source =
    config.lib.file.mkOutOfStoreSymlink
      "/home/raina/System/home/niri/rofi/filepreview.rasi";
}
