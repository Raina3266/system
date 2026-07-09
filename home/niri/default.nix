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

  gtk = {
    enable = true;
    cursorTheme.name = "Everforest cursors";
    cursorTheme.package = pkgs.everforest-cursors;
  };
}
