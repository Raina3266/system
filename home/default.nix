{
  pkgs,
  ...
}:
{
  imports = [
    ./niri
    ./shell
    ./yazi
    ./thunderbird.nix
    ./cloud.nix
    ./ocr.nix
    ./toolchains.nix
    ./desktop.nix
  ];

  home = {
    username = "raina";
    homeDirectory = "/home/raina";
    stateVersion = "26.05";
  };

  programs.home-manager.enable = true;
  programs.zed-editor.enable = true;
  programs.vscode.enable = true;

  home.packages = with pkgs; [
    # browsers
    google-chrome
    firefox

    # handy
    handy
    wtype
    wl-clipboard

    # communication
    discord
    zoom-us
    wechat
    qq
    wemeet
    whatsie

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
    gimp
    shotcut
    kid3
    spotdl
    yt-dlp

    # downloads / torrent
    qbittorrent
    clash-verge-rev
  ];
}
