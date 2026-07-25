{
  pkgs,
  config,
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
in
{
  imports = [
    ./waybar
    ./walker
  ];

  xdg.configFile."niri/config.kdl".source = ./config.kdl;

  programs'.waybar.enable = true;

  # Tools for niri binds and X11 app support
  home.packages = with pkgs; [
    swaybg
    bluez-tools
    brightnessctl # Screen brightness control
    wob # Wayland overlay progress bar for timer
    xwayland-satellite # Rootless XWayland for X11 apps
    xrandr # Display layout info for X11 apps
    snixembed # System tray bridge for Qt5-xcb apps
    networkmanagerapplet # NetworkManager GUI and Wi-Fi password dialogs
    mediactl
  ];

  # Bluetooth pairing agent: auto-confirms pairing requests so Walker's
  # bluetooth provider doesn't hang.
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

  # NetworkManager applet: provides graphical Wi-Fi password prompts.
  # Uses --indicator flag to avoid system tray icon.
  systemd.user.services.nm-applet = {
    Unit = {
      Description = "NetworkManager applet / secret agent";
      ConditionEnvironment = [ "WAYLAND_DISPLAY" ];
      PartOf = [ "graphical-session.target" ];
      After = [ "graphical-session.target" ];
    };
    Service = {
      ExecStart = "${pkgs.networkmanagerapplet}/bin/nm-applet --indicator";
      Restart = "on-failure";
      RestartSec = 3;
    };
    Install = {
      WantedBy = [ "graphical-session.target" ];
    };
  };

  # wob: overlay progress bar daemon for timer countdown visualization.
  # Reads percentages from $XDG_RUNTIME_DIR/wob.sock.
  systemd.user.services.wob = {
    Unit = {
      Description = "wob — Wayland overlay bar";
      ConditionEnvironment = [ "WAYLAND_DISPLAY" ];
      PartOf = [ "graphical-session.target" ];
      # After ensures wob starts after niri sets WAYLAND_DISPLAY.
      # Without this, ConditionEnvironment can fail at boot.
      After = [ "graphical-session.target" ];
    };
    Service = {
      ExecStart = "${pkgs.wob}/bin/wob";
      Restart = "on-failure";
      RestartSec = 3;
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

  # fcitx5 theme: cyberpunk color palette matching waybar/walker
  xdg.dataFile."fcitx5/themes/cyberpunk/theme.conf".source = ./themes/fcitx5.conf;

  xdg.configFile."fcitx5/conf/classicui.conf".text = ''
    Vertical Center=False
    PerScreenDPI=True
    UseDarkTheme=False
    Theme=cyberpunk
    Font="Sans 14"
    MenuFont="Sans 14"
  '';

  # GTK theme: cyberpunk palette for all GTK apps (nm-applet, file dialogs, etc.)
  # Matches waybar/walker/fcitx5 themes in ./themes/
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
  #   gtk-base.css - shared colors/widgets (imported by both versions)
  #   gtk3.css     - GTK3 apps (file managers, pavucontrol, nm-applet)
  #   gtk4.css     - GTK4 apps (portal file chooser, image viewer)
  # gtk-base.css symlinked to both dirs for @import resolution.
  xdg.configFile."gtk-3.0/gtk.css".source =
    config.lib.file.mkOutOfStoreSymlink
      "/home/raina/System/home/niri/themes/gtk3.css";
  xdg.configFile."gtk-3.0/gtk-base.css".source =
    config.lib.file.mkOutOfStoreSymlink
      "/home/raina/System/home/niri/themes/gtk-base.css";
  xdg.configFile."gtk-4.0/gtk.css".source =
    config.lib.file.mkOutOfStoreSymlink
      "/home/raina/System/home/niri/themes/gtk4.css";
  xdg.configFile."gtk-4.0/gtk-base.css".source =
    config.lib.file.mkOutOfStoreSymlink
      "/home/raina/System/home/niri/themes/gtk-base.css";

  home.pointerCursor = {
    enable = true;
    package = pkgs.bibata-cursors;
    name = "Bibata-Modern-Classic";
    size = 18;
    gtk.enable = true;
    x11.enable = true;
  };
}
