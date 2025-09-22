{
  pkgs,
  lib,
  nixosConfig ? null,
  ...
}: let
  isNixOS = nixosConfig != null;
in {
  programs.zed-editor.enable = true;
  programs.zed-editor.package = lib.mkIf (!isNixOS) (pkgs.writeShellScriptBin "zeditor" ''
    ${pkgs.nixgl.nixVulkanIntel}/bin/nixVulkanIntel ${pkgs.zed-editor}/bin/zeditor "$@"
  '');
}
