{
  lib,
  nixosConfig,
  pkgs,
  ...
}:
{
  config = lib.mkIf nixosConfig.services'.personal.enable {
    home.packages = with pkgs; [
      virt-manager
      virt-viewer
      spice
      spice-gtk
    ];
  };
}