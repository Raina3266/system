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
    theme = ./rofi.rasi;
  };

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
  
  home.pointerCursor = {
    enable = true;
    package = pkgs.bibata-cursors;
    name = "Bibata-Modern-Classic";
    size = 24;
    gtk.enable = true;
    x11.enable = true;
  };
}
