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
      QT_STYLE_OVERRIDE = "breeze";
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

    ${builtins.readFile ./config.kdl}
  '';

  # The block above only reaches niri's own children: niri imports just
  # WAYLAND_DISPLAY, DISPLAY, XDG_CURRENT_DESKTOP, XDG_SESSION_TYPE and
  # NIRI_SOCKET into systemd and D-Bus. "Open folder" in Chrome or Zed D-Bus
  # activates Dolphin, which would otherwise start without the KDE theme.
  systemd.user.sessionVariables = qtEnvironment;

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

  # Default Bluetooth pairing agent, used for pairings started from the *other*
  # side (a phone or headset initiating the connection) so they never hang.
  # Pairings started from rofi-audio use the agent rofi-audio registers on its
  # own D-Bus connection instead: BlueZ prefers the caller's agent, which is
  # what lets rofi-audio prompt for a PIN or passkey inside the menu.
  systemd.user.services.bt-agent = {
    Unit = {
      Description = "Persistent Bluetooth pairing agent";
      PartOf = [ "graphical-session.target" ];
      After = [ "graphical-session.target" ];
    };
    Service = {
      # DisplayYesNo capability required for SSP passkey devices (keyboards,
      # earbuds, phones). Auto-answers "yes" to pairing requests.
      ExecStart = "${pkgs.bluez-tools}/bin/bt-agent --capability=DisplayYesNo";
      Restart = "on-failure";
    };
    Install = {
      WantedBy = [ "graphical-session.target" ];
    };
  };

  # fcitx5 input methods: English (GB keyboard) and Chinese (Pinyin)
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

  # fcitx5 theme: cyberpunk color palette matching waybar/rofi
  xdg.dataFile."fcitx5/themes/cyberpunk/theme.conf".source =
    config.lib.file.mkOutOfStoreSymlink "/home/raina/System/niri/themes/fcitx5.conf";

  xdg.configFile."fcitx5/conf/classicui.conf".text = ''
    Vertical Center=False
    PerScreenDPI=True
    UseDarkTheme=False
    Theme=cyberpunk
    Font="Sans 14"
    MenuFont="Sans 14"
  '';

  # GTK theme: cyberpunk palette for all GTK apps (file dialogs, etc.)
  # Matches waybar/rofi/fcitx5 themes in ./themes/
  gtk = {
    enable = true;

    gtk3.extraConfig = {
      gtk-application-prefer-dark-theme = 1;
    };
    gtk4.extraConfig = {
      gtk-application-prefer-dark-theme = 1;
    };
  };

  # GTK stylesheets:
  #   gtk3.css - GTK3 apps (file managers, pavucontrol)
  #   gtk4.css - GTK4 apps (portal file chooser, image viewer)
  xdg.configFile."gtk-3.0/gtk.css".source =
    config.lib.file.mkOutOfStoreSymlink "/home/raina/System/niri/themes/gtk3.css";
  xdg.configFile."gtk-4.0/gtk.css".source =
    config.lib.file.mkOutOfStoreSymlink "/home/raina/System/niri/themes/gtk4.css";
  # Qt/KDE colours are not themed from this repo; they live in kdeglobals and
  # are managed with System Settings -> Colours.

  home.pointerCursor = {
    enable = true;
    package = pkgs.bibata-cursors;
    name = "Bibata-Modern-Classic";
    size = 18;
    gtk.enable = true;
    x11.enable = true;
  };
}
