{ pkgs, ... }:
{
  programs.vscode = {
    enable = true;
    package = pkgs.vscode.override { commandLineArgs = "--no-sandbox"; };
    profiles.default.extensions = with pkgs.vscode-extensions; [
      jdinhlife.gruvbox
      dart-code.flutter
      rust-lang.rust-analyzer
      tamasfe.even-better-toml
      mechatroner.rainbow-csv
      redhat.vscode-yaml
      bbenoist.nix
      vue.volar
      bbenoist.nix
      kamadorueda.alejandra
      eamodio.gitlens
    ];
    mutableExtensionsDir = false;
    profiles.default.userSettings = {
      workbench.colorTheme = "Gruvbox Dark Hard";
      files.autoSave = "afterDelay";
      editor.inlayHints.enabled = "offUnlessPressed";
      rust-analyzer.check.command = "clippy";
    };
  };
}
