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

    # Dedicated file finder with the preview pane. Kept separate from the combi
    # launcher because drun entries feed their own app icon to the preview
    # widget, which looks bad blown up to pane size.
    # ROFI_FILESEARCH_PREVIEW makes filesearch.sh tag each row with
    # `icon\x1fthumbnail://<path>`. Script modes get no thumbnails otherwise:
    # rofi only feeds icon-current-entry from a row's explicit `icon` option.
    (writeShellScriptBin "rofi-files" ''
      export ROFI_FILESEARCH_PREVIEW=1
      exec rofi -show filesearch \
        -modes "filesearch:rofi-filesearch" \
        -theme "$HOME/.config/rofi/filepreview.rasi" \
        -display-filesearch "󰈞 Files "
    '')
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
