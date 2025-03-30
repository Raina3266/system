{
  lib,
  config,
  pkgs,
  ...
}: {
  nixpkgs = {
    config = {
      allowUnfree = true;
    };
  };

  # https://nixos.wiki/wiki/FAQ/When_do_I_update_stateVersion
  home.stateVersion = "23.05";
  home = {
    username = "raina";
    homeDirectory = "/home/raina";
  };

  xdg.enable = true;

  # trick to force gnome to use proper icons for apps installed through nix
  #  xdg.systemDirs.data = ["~/.nix-profile/share"];

  # Enable home-manager and git
  programs.git.enable = true;
  programs.git.userName = "Raina";
  programs.git.userEmail = "chenganlin990326@gmail.com";
  programs.git.extraConfig = {
    push.autoSetupRemote = true;
  };

  programs.gh.enable = true;

  programs.direnv.enable = true;
  programs.direnv.enableBashIntegration = true;
  programs.direnv.nix-direnv.enable = true;
  programs.bash.enable = true;

  home.packages = with pkgs; [
    google-chrome
    whatsapp-for-linux
    obsidian
    kitty
    bat
    tree
    flutter
    vlc
    gimp-with-plugins
    kdePackages.kdenlive
    masterpdfeditor
    obs-studio
    libreoffice
    qbittorrent
    anki-bin
    rustup

    gcc

    onedrivegui
    onedrive
  ];

  programs.fish = {
    enable = true;
    shellAbbrs = {
      "gs" = "git switch";
    };
  };

  programs.starship = {
    enable = true;
  };

  programs.vscode = {
    enable = true;
    extensions = with pkgs.vscode-extensions; [
      dracula-theme.theme-dracula
      dart-code.flutter
      rust-lang.rust-analyzer
      tamasfe.even-better-toml

      bbenoist.nix # nix language support
      kamadorueda.alejandra # better nix formatter
    ];
    mutableExtensionsDir = false;
    userSettings = {
      workbench.colorTheme = "Dracula";
      files.autoSave = "afterDelay";
    };
  };

  programs.kitty.enable = true;

  # gtk = {
  #   enable = true;
  #   font.name = "FiraSans";
  #   font.package = pkgs.fira;
  # };
}
