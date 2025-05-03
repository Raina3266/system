{pkgs, lib, ...}: 
let
    isNixOS = true;
in
{
    programs.kitty.enable = true;
    programs.kitty.themeFile = if isNixOS then "Catppuccin-Mocha" else "gruvbox-dark-hard";
    programs.kitty.package = lib.mkIf (!isNixOS) (pkgs.writeShellScriptBin "kitty" ''
        ${pkgs.nixgl.nixGLIntel}/bin/nixGLIntel ${pkgs.kitty}/bin/kitty "$@"
    '');
}
