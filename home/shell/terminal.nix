{
  pkgs,
  lib,
  nixosConfig ? null,
  ...
}:
let
  isNixOS = nixosConfig != null;
in
{
  programs.yazi = {
    enable = true;
  };

  programs.kitty = {
    enable = true;
  };
  programs.kitty.themeFile = "Dracula";
  programs.kitty.package = lib.mkIf (!isNixOS) (
    pkgs.writeShellScriptBin "kitty" ''
      ${pkgs.nixgl.nixGLIntel}/bin/nixGLIntel ${pkgs.kitty}/bin/kitty "$@"
    ''
  );
  programs.kitty.settings = {
    shell = "fish";
  };

  programs.ghostty = {
    enable = true;
    enableFishIntegration = true;
    settings = {
      command = "fish";
      theme = "Bright Lights";
    };
  };
}
