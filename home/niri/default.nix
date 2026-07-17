{
  pkgs,
  ...
}:
{
  imports = [
    ./waybar
    ./walker
  ];

  xdg.configFile."niri/config.kdl".source = ./config.kdl;

  programs'.waybar.enable = true;

  # Tools invoked by niri binds in config.kdl.
  # Includes mediactl — a wrapper around playerctl that targets the
  # right player when multiple MPRIS players are running (e.g. VLC +
  # Chromium). Prefers a player that is currently Playing; falls back
  # to the most recently active player. Usage: mediactl <play-pause|next|previous|stop>
  # Also includes xwayland-satellite (spawned at startup by config.kdl)
  # for rootless XWayland so legacy X11 apps work under niri.
  home.packages = with pkgs; [
    swaybg
    brightnessctl # F5/F6 brightness keys
    wob # Wayland overlay progress bar — used by the timer
    xwayland-satellite # rootless XWayland for X11 apps
    xrandr # for X11 apps that query display layout
    (writeShellScriptBin "mediactl" ''
      cmd="$1"
      players=$(${playerctl}/bin/playerctl -l 2>/dev/null)
      [ -z "$players" ] && exit 0
      target=""
      for p in $players; do
        st=$(${playerctl}/bin/playerctl -p "$p" status 2>/dev/null)
        if [ "$st" = "Playing" ]; then
          target="$p"
          break
        fi
      done
      # Fall back to the first listed player (playerctl -l orders by
      # most recent activity).
      [ -z "$target" ] && target=$(echo "$players" | head -n1)
      exec ${playerctl}/bin/playerctl -p "$target" "$cmd"
    '')
  ];

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
    Install = { WantedBy = [ "graphical-session.target" ]; };
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

    [Groups/0/Items/2]
    Name=mozc
    Layout=

    [Groups/0/Items/3]
    Name=hangul
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

  home.pointerCursor = {
    enable = true;
    package = pkgs.bibata-cursors;
    name = "Bibata-Modern-Classic";
    size = 20;
    gtk.enable = true;
    x11.enable = true;
  };
}
