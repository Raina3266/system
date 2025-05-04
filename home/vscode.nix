{ pkgs, ...}: 
{
  programs.vscode = {
    enable = true;
    profiles.default.extensions = with pkgs.vscode-extensions; [
      dracula-theme.theme-dracula
      dart-code.flutter
      rust-lang.rust-analyzer
      tamasfe.even-better-toml
      mechatroner.rainbow-csv

      bbenoist.nix # nix language support
      kamadorueda.alejandra # better nix formatter
    ];
    mutableExtensionsDir = false;
    profiles.default.userSettings = {
      workbench.colorTheme = "Dracula";
      files.autoSave = "afterDelay";
    };
  };
}