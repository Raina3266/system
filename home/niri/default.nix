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
  home.packages = with pkgs; [
    brightnessctl # F5/F6 brightness keys
    wob # Wayland overlay progress bar — used by the timer
  ];

  # wob daemon — reads integer percentages from $XDG_RUNTIME_DIR/wob.sock
  # and renders an overlay progress bar. The timer scripts write to this
  # FIFO to visualize the countdown. See waybar/scripts.nix.
  systemd.user.services.wob = {
    Unit = {
      Description = "wob — Wayland overlay bar";
      ConditionEnvironment = [ "WAYLAND_DISPLAY" ];
      PartOf = [ "graphical-session.target" ];
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
    Font="Sans 16"
    MenuFont="Sans 16"
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
