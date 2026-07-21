{
  pkgs,
  ...
}:
let
  # Wrapper around playerctl that targets the right player when multiple
  # MPRIS players are running (e.g. VLC + Chromium). Prefers a player
  # that is currently Playing; falls back to the most recently active
  # player. Usage: mediactl <play-pause|next|previous|stop>
  # Invoked by niri media-key binds in config.kdl.
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
    # Fall back to the first listed player (playerctl -l orders by
    # most recent activity).
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

  # Tools invoked by niri binds in config.kdl.
  # Includes mediactl (defined above) and xwayland-satellite (spawned at
  # startup by config.kdl) for rootless XWayland so legacy X11 apps work
  # under niri.
  home.packages = with pkgs; [
    swaybg
    bluez-tools
    brightnessctl # F5/F6 brightness keys
    wob # Wayland overlay progress bar — used by the timer
    xwayland-satellite # rootless XWayland for X11 apps
    xrandr # for X11 apps that query display layout
    networkmanagerapplet # graphical NetworkManager secret agent / Wi-Fi password dialogs
    mediactl
  ];

  # Bluetooth auto-confirm agent — registers a NoInputNoOutput BlueZ
  # agent that auto-accepts pairing requests. Without this, Walker's
  # bluetooth provider hangs at "Pairing..." because bluetoothctl pair
  # (one-shot) has no agent to confirm the request.
  systemd.user.services.bt-agent = {
    Unit = {
      Description = "Persistent Bluetooth pairing agent";
      PartOf = [ "graphical-session.target" ];
      After = [ "graphical-session.target" ];
    };
    Service = {
      ExecStart = "${pkgs.bluez-tools}/bin/bt-agent --capability=NoInputNoOutput";
      Restart = "on-failure";
    };
    Install = {
      WantedBy = [ "graphical-session.target" ];
    };
  };

  # NetworkManager applet — runs as a secret agent so graphical password
  # prompts appear when connecting to new Wi-Fi networks. The --indicator
  # flag keeps it out of the system tray while still handling requests.
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

  # wob daemon — reads integer percentages from $XDG_RUNTIME_DIR/wob.sock
  # and renders an overlay progress bar. The timer scripts write to this
  # FIFO to visualize the countdown. See waybar/scripts.nix.
  systemd.user.services.wob = {
    Unit = {
      Description = "wob — Wayland overlay bar";
      ConditionEnvironment = [ "WAYLAND_DISPLAY" ];
      PartOf = [ "graphical-session.target" ];
      # PartOf alone doesn't order startup after the target — it only
      # propagates stop/restart. Without this, wob can start before niri
      # (which sets WAYLAND_DISPLAY) finishes and reaches the target,
      # so ConditionEnvironment intermittently fails right after boot.
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

  # Cyberpunk fcitx5 theme (matches waybar/walker palette).
  xdg.dataFile."fcitx5/themes/cyberpunk/theme.conf".source = ./themes/fcitx5-cyberpunk.conf;

  xdg.configFile."fcitx5/conf/classicui.conf".text = ''
    Vertical Center=False
    PerScreenDPI=True
    UseDarkTheme=False
    Theme=cyberpunk
    Font="Sans 14"
    MenuFont="Sans 14"
  '';

  # Global GTK theme — applies the cyberpunk palette to all GTK apps,
  # including nm-applet's Wi-Fi password dialogs and connection editor.
  # Palette matches waybar/walker/fcitx5 themes (see ./themes/).
  gtk = {
    enable = true;

    gtk3.extraConfig = {
      gtk-application-prefer-dark-theme = 1;
    };
    gtk4.extraConfig = {
      gtk-application-prefer-dark-theme = 1;
    };
    gtk3.extraCss = builtins.readFile ./themes/gtk-cyberpunk.css;
    gtk4.extraCss = builtins.readFile ./themes/gtk-cyberpunk.css;
  };

  home.pointerCursor = {
    enable = true;
    package = pkgs.bibata-cursors;
    name = "Bibata-Modern-Classic";
    size = 18;
    gtk.enable = true;
    x11.enable = true;
  };
}
