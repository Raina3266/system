{
  pkgs,
  lib,
  ...
}:
{
  imports = [
    ./niri
    ./shell
    ./thunderbird.nix
    ./handy.nix
    ./cloud.nix
    ./ocr.nix
    ./toolchains.nix
    ./zed.nix
  ];

  home = {
    username = "raina";
    homeDirectory = "/home/raina";
    stateVersion = "26.05";
  };

  programs.home-manager.enable = true;

  home.packages = with pkgs; [
    # browsers
    google-chrome
    firefox

    # communication
    slack
    discord
    zoom-us
    wechat
    qq
    wemeet
    whatsie
    thunderbird

    # productivity / office
    onlyoffice-desktopeditors
    obsidian
    anki
    meld
    czkawka
    exercism
    stirling-pdf-desktop

    # media playback
    vlc
    tauon
    waylyrics

    # media creation / editing
    pavucontrol
    obs-studio
    inkscape
    shotcut
    sunshine
    kid3
    spotdl
    yt-dlp

    # media servers / sync
    jellyfin
    jellyfin-web
    immich

    # downloads / torrent
    qbittorrent
    clash-verge-rev

    # gnome extensions
    gnomeExtensions.simple-timer
    gnomeExtensions.clipboard-history
    gnomeExtensions.astra-monitor
  ];

}
