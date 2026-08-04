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

    (writeShellScriptBin "rofi-filesearch" (builtins.readFile ./filesearch.sh))
    fd
  ];

  xdg.configFile."rofi/config.rasi".source =
    config.lib.file.mkOutOfStoreSymlink
      "/home/raina/System/home/niri/rofi/config.rasi";
  xdg.configFile."rofi/rofi-single.rasi".source =
    config.lib.file.mkOutOfStoreSymlink
      "/home/raina/System/home/niri/themes/rofi-single.rasi";
  xdg.configFile."rofi/rofi-double.rasi".source =
    config.lib.file.mkOutOfStoreSymlink
      "/home/raina/System/home/niri/themes/rofi-double.rasi";
}
