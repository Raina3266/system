# Theme files kept outside the Nix store.
#
# mkOutOfStoreSymlink points at the working copy in this repository, so an
# edit to a stylesheet is picked up the next time the program starts without a
# Home Manager rebuild. Every such link lives here so the repository path is
# written once rather than once per module.
{ config, ... }:
let
  repoRoot = "${config.home.homeDirectory}/System";

  link = path: { source = config.lib.file.mkOutOfStoreSymlink "${repoRoot}/${path}"; };

  # <path under $XDG_CONFIG_HOME> = <path in this repository>
  configLinks = {
    "gtk-3.0/gtk.css" = "themes/gtk3.css"; # GTK3 apps (pavucontrol, file dialogs)
    "gtk-4.0/gtk.css" = "themes/gtk4.css"; # GTK4 apps (portal file chooser, image viewer)
    "preview-panel/preview-panel.css" = "themes/preview-panel.css";
    "rofi/config.rasi" = "niri/rofi/config.rasi";
    "rofi/media-control.rasi" = "themes/media-control.rasi";
    "rofi/rofi-audio.rasi" = "themes/rofi-audio.rasi";
    "rofi/rofi-clipboard.rasi" = "themes/rofi-clipboard.rasi";
    "rofi/rofi-finder.rasi" = "themes/rofi-finder.rasi";
    "rofi/rofi-network.rasi" = "themes/rofi-network.rasi";
    "waybar/style.css" = "themes/waybar.css";
    "yazi/theme.toml" = "themes/yazi.toml";
  };

  # <path under $XDG_DATA_HOME> = <path in this repository>
  dataLinks = {
    "fcitx5/themes/cyberpunk/theme.conf" = "themes/fcitx5.conf";
  };
in
{
  xdg.configFile = builtins.mapAttrs (_name: link) configLinks;
  xdg.dataFile = builtins.mapAttrs (_name: link) dataLinks;
}
