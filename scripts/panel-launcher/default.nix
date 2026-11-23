{ pkgs, mkWorkspacePackage, mkCargoArtifacts }:
let
  panelLauncher = mkWorkspacePackage "panel-launcher" {
    cargoArtifacts = mkCargoArtifacts "panel-launcher-deps" {
      cargoExtraArgs = "--locked --package panel-launcher";
    };
    passthru.withParentDeath = pkgs.writeShellScriptBin "with-parent-death" ''
      exec ${pkgs.lib.getExe panelLauncher} runtime with-parent-death "$@"
    '';
  };
in
panelLauncher
