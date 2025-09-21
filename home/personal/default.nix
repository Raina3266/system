{
  lib,
  pkgs,
  config,
  ...
}: {
  imports = [ ];
  options = { 
    personal.enable = lib.mkEnableOption "Personal stuff";
  };
  config = lib.mkIf config.personal.enable {
    home.packages = with pkgs;[
      obsidian
    ];
  };
}
