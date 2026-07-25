# Yazi configuration.
# This is the main entry point that imports modular configuration files.
{
  pkgs,
  ...
}:
let
  # Import the separate configuration modules
  plugins = import ./plugins.nix { inherit pkgs; };
  settings = import ./settings.nix;
  keymap = import ./keymap.nix;
in
{
  programs.yazi = {
    enable = true;
    enableFishIntegration = true;
    enableBashIntegration = true;

    # Import plugin definitions and runtime dependencies
    inherit (plugins) plugins extraPackages;

    initLua = ./main.lua;
    settings = settings;
    theme = import ./theme.nix;
    keymap = keymap;
  };
}