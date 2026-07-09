{
  pkgs,
  inputs,
  ...
}:
{
  imports = [
    ./niri
    ./cloud.nix
    ./shell
    ./ocr.nix
    ./toolchains.nix
  ];

  home = {
    username = "raina";
    homeDirectory = "/home/raina";
  };

  programs.home-manager.enable = true;

  programs.zed-editor = {
    enable = true;
    # Nightly from the upstream flake (matches zed.cachix.org; see flake.nix).
    package = inputs.zed.packages.${pkgs.stdenv.hostPlatform.system}.default;
  };

  programs.thunderbird = {
    enable = true;
    profiles.default.isDefault = true;
  };

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

    # productivity / office
    libreoffice
    pdf4qt
    obsidian
    anki
    meld
    czkawka
    exercism

    # media playback
    vlc
    tauon
    fooyin
    strawberry
    nightingale
    waylyrics

    # media creation / editing
    obs-studio
    inkscape
    shotcut
    sunshine
    kid3
    spotdl
    yt-dlp
    openai-whisper
    piper-tts

    # media servers / sync
    jellyfin
    jellyfin-web
    immich
    onedrivegui

    # downloads / torrent
    qbittorrent

    # audio / system
    pavucontrol
    clash-verge-rev
  ];
}
