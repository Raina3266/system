{pkgs, lib, nixosConfig ? null, ...}: 
let
    isNixOS = nixosConfig != null;
in
{
    programs.kitty.enable = true;
    programs.kitty.themeFile = "gruvbox-dark-hard";
    programs.kitty.package = lib.mkIf (!isNixOS) (pkgs.writeShellScriptBin "kitty" ''
        ${pkgs.nixgl.nixGLIntel}/bin/nixGLIntel ${pkgs.kitty}/bin/kitty "$@"
    '');
}
