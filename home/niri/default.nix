{
  pkgs,
  ...
}:
{
  imports = [
    ./waybar.nix
  ];

  xdg.configFile."niri/config.kdl".source = ./config.kdl;
  programs.rofi.enable = true;

  # Tools invoked by niri binds in config.kdl.
  home.packages = with pkgs; [
    brightnessctl  # F5/F6 brightness keys
  ];
  
  home.pointerCursor = {
    enable = true;
    package = pkgs.bibata-cursors;
    name = "Bibata-Modern-Classic";
    size = 24;
    gtk.enable = true;
    x11.enable = true;
  };
}
