{
  pkgs,
  config,
  lib,
  ...
}:
let
  # Smart playerctl wrapper: targets the currently playing MPRIS player,
  # or falls back to the most recently active. Used by media key binds.
  mediactl = pkgs.writeShellScriptBin "mediactl" ''
    cmd="$1"
    players=$(${pkgs.playerctl}/bin/playerctl -l 2>/dev/null)
    [ -z "$players" ] && exit 0
    target=""
    for p in $players; do
      st=$(${pkgs.playerctl}/bin/playerctl -p "$p" status 2>/dev/null)
      if [ "$st" = "Playing" ]; then
        target="$p"
        break
      fi
    done
    # Fall back to first listed player (most recently active)
    [ -z "$target" ] && target=$(echo "$players" | head -n1)
    exec ${pkgs.playerctl}/bin/playerctl -p "$target" "$cmd"
  '';

  # Shared by niri's environment block and environment.d below, so Qt apps
  # are themed however they get started.
  qtEnvironment =
    let
      inherit (config.home) profileDirectory;
      qtPluginPath = qt: "${profileDirectory}/${qt.qtbase.qtPluginPrefix}";
      qtQmlPath = qt: "${profileDirectory}/${qt.qtbase.qtQmlPrefix}";
    in
    {
      QT_QPA_PLATFORMTHEME = "kde";
      QT_STYLE_OVERRIDE = "kvantum";
      QT_PLUGIN_PATH = "${qtPluginPath pkgs.qt5}:${qtPluginPath pkgs.qt6}";
      QML2_IMPORT_PATH = "${qtQmlPath pkgs.qt5}:${qtQmlPath pkgs.qt6}";
    };
in
{
  imports = [
    ./waybar
    ./rofi
  ];

  xdg.configFile."niri/config.kdl".text = ''
    environment {
    ${lib.concatStringsSep "\n" (
      lib.mapAttrsToList (name: value: "  ${name} \"${value}\"") qtEnvironment
    )}
    }

    // KDE authentication dialogs in Niri. GNOME starts its own agent when
    // that session is selected, so this remains scoped to Niri.
    spawn-at-startup "${pkgs.kdePackages.polkit-kde-agent-1}/libexec/polkit-kde-authentication-agent-1"

    ${builtins.readFile ./config.kdl}
  '';

  programs'.waybar.enable = true;

  # Tools for niri binds and X11 app support
  home.packages = with pkgs; [
    swaybg
    nwg-displays
    bluez-tools
    wireplumber # PipeWire/WirePlumber control (wpctl for niri audio binds)
    brightnessctl # Screen brightness control
    xwayland-satellite # Rootless XWayland for X11 apps
    xrandr # Display layout info for X11 apps
    snixembed # System tray bridge for Qt5-xcb apps
    mediactl
  ];

  # fcitx5 theme: cyberpunk color palette matching waybar/rofi.
  # The theme file itself is linked from ../themes/default.nix.
  xdg.configFile."fcitx5/conf/classicui.conf".text = ''
    Vertical Center=False
    PerScreenDPI=True
    UseDarkTheme=False
    Theme=cyberpunk
    Font="Sans 14"
    MenuFont="Sans 14"
  '';

  # GTK and its icon fallback settings live here; ../themes/default.nix builds
  # and selects the complete Daemon GTK theme from the pinned upstream assets.
  gtk = {
    enable = true;

    iconTheme.name = "Daemon-Icons";

    gtk3.extraConfig = {
      gtk-application-prefer-dark-theme = 1;
    };
    gtk4.extraConfig = {
      gtk-application-prefer-dark-theme = 1;
    };
  };

  # GTK and Qt/KDE appearance are both managed from ../themes/default.nix.

  home.pointerCursor = {
    enable = true;
    package = pkgs.bibata-cursors;
    name = "Bibata-Original-Classic";
    size = 18;
    gtk.enable = true;
    x11.enable = true;
  };

  systemd.user.sessionVariables = qtEnvironment;
}
