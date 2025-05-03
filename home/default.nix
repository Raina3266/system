{
  lib,
  config,
  pkgs,
  ...
}: {

  imports = [
    ./cloud.nix
    ./ocr.nix
    ./shell/default.nix
  ];

  # https://nixos.wiki/wiki/FAQ/When_do_I_update_stateVersion
  home.stateVersion = "23.05";
  home = {
    username = "raina";
    homeDirectory = "/home/raina";
  };

  xdg.enable = true;

  # trick to force gnome to use proper icons for apps installed through nix
  #  xdg.systemDirs.data = ["~/.nix-profile/share"];


  programs.gh.enable = true;

  programs.direnv.enable = true;
  programs.direnv.enableBashIntegration = true;
  programs.direnv.nix-direnv.enable = true;
  programs.bash.enable = true;

  home.packages = with pkgs; [
    google-chrome
    whatsapp-for-linux
    discord
    obsidian
    vlc
    gimp-with-plugins
    kdePackages.kdenlive
    obs-studio
    qbittorrent
    anki-bin
    libreoffice-qt
    strawberry
    yt-dlp
    spotdl
    ytdownloader
  ];

  # home.file.".local/share/applications/libreoffice-writer.desktop".text = ''
  #   NoDisplay = true
  #   Name = LibreOffice Writer
  #   Exec = libreoffice --writer
  #   Hidden = true
  #   Type = Application
  # '';

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
