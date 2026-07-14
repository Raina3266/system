{
  pkgs,
  ...
}:
{
  imports = [
    ./waybar.nix
  ];

  xdg.configFile."niri/config.kdl".source = ./config.kdl;
  programs.rofi = {
    enable = true;
    theme = ./themes/rofi-cyberpunk.rasi;
    # Use a wrapper that ensures the Wayland backend is used under niri.
    # Without this, rofi may fall back to X11/Xwayland when WAYLAND_DISPLAY
    # isn't in the inherited systemd environment, breaking outside-click
    # dismissal. Under GNOME (no layer-shell), the wrapper leaves rofi alone.
    package = pkgs.writeShellScriptBin "rofi" ''
      if [ -z "''${WAYLAND_DISPLAY:-}" ] && pgrep -x niri >/dev/null 2>&1; then
        niri_pid=$(pgrep -x niri | head -1)
        wd=$(tr '\0' '\n' < /proc/$niri_pid/environ | grep '^WAYLAND_DISPLAY=' | cut -d= -f2-)
        [ -n "$wd" ] && export WAYLAND_DISPLAY="$wd"
      fi
      # -normal-window makes rofi a regular Wayland surface instead of a
      # layer-shell overlay, so the compositor can dismiss it when focus
      # moves away (i.e. clicking outside rofi closes it).
      exec ${pkgs.rofi}/bin/rofi -normal-window "$@"
    '';
  };

  # Dropdown theme for bottom bar modules — rofi appears above the bottom bar.
  xdg.dataFile."rofi/themes/rofi-cyberpunk-bottom.rasi".source = ./themes/rofi-cyberpunk-bottom.rasi;

  # Tools invoked by niri binds in config.kdl.
  home.packages = with pkgs; [
    brightnessctl  # F5/F6 brightness keys
  ];

  programs'.waybar = {
    enable = true;
    enableNiriIntegration = true;
  };

  # Configure fcitx5 input methods: English (keyboard-gb) + Chinese (pinyin).
  xdg.configFile."fcitx5/profile".text = ''
    [Groups/0]
    Name="Default"
    Default Layout=gb
    DefaultIM=keyboard-gb

    [Groups/0/Items/0]
    Name=keyboard-gb
    Layout=

    [Groups/0/Items/1]
    Name=pinyin
    Layout=

    [GroupOrder]
    0="Default"
  '';

  # Cyberpunk fcitx5 theme (matches waybar/rofi palette).
  xdg.dataFile."fcitx5/themes/cyberpunk/theme.conf".source = ./themes/fcitx5-cyberpunk.conf;

  # Use the cyberpunk theme for the classic UI.
  xdg.configFile."fcitx5/conf/classicui.conf".text = ''
    Vertical Center=False
    PerScreenDPI=True
    UseDarkTheme=False
    Theme=cyberpunk
  '';
  
  home.pointerCursor = {
    enable = true;
    package = pkgs.bibata-cursors;
    name = "Bibata-Modern-Classic";
    size = 24;
    gtk.enable = true;
    x11.enable = true;
  };
}
