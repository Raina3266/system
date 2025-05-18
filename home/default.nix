{
  lib,
  config,
  pkgs,
  ...
}: {
  imports = [
    ./cloud.nix
    ./ocr.nix
    ./vscode.nix
    ./shell/default.nix
  ];

  # https://nixos.wiki/wiki/FAQ/When_do_I_update_stateVersion
  home.stateVersion = "23.05";
  home = {
    username = "raina";
    homeDirectory = "/home/raina";
  };

  programs.thunderbird = {
    enable = true;
    profiles.default = {
      isDefault = true;
    };
  };

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
    libreoffice
    pdfstudioviewer
    kid3
    gui-for-clash

    lollypop
    ffmpeg
    spotdl
    lrcget
    ytdownloader
  ];
}
