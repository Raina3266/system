{
  pkgs,
  ...
}:
{
  imports = [
    ./niri
    ./shell
    ./thunderbird.nix
    ./cloud.nix
    ./ocr.nix
    ./toolchains.nix
  ];

  home = {
    username = "raina";
    homeDirectory = "/home/raina";
    stateVersion = "26.05";
  };

  programs.home-manager.enable = true;
  programs.zed-editor.enable = true;

  home.packages = with pkgs; [
    # browsers
    google-chrome
    firefox

    # handy
    handy
    wtype
    wl-clipboard

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
    gimp
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
  xdg.mimeApps = {
    enable = true;
    defaultApplications = {
      # PDF
      "application/pdf" = [ "google-chrome.desktop" ];

      # Video
      "video/mp4" = [ "vlc.desktop" ];
      "video/x-matroska" = [ "vlc.desktop" ];
      "video/webm" = [ "vlc.desktop" ];
      "video/quicktime" = [ "vlc.desktop" ];
      "video/x-msvideo" = [ "vlc.desktop" ];
      "video/mpeg" = [ "vlc.desktop" ];
      "video/ogg" = [ "vlc.desktop" ];
      "video/3gpp" = [ "vlc.desktop" ];
      "video/3gpp2" = [ "vlc.desktop" ];
      "video/x-flv" = [ "vlc.desktop" ];
      "video/x-ms-wmv" = [ "vlc.desktop" ];
      "video/x-ms-asf" = [ "vlc.desktop" ];
      "video/divx" = [ "vlc.desktop" ];
      "video/mp2t" = [ "vlc.desktop" ];

      # Audio
      "audio/mpeg" = [ "vlc.desktop" ];
      "audio/mp4" = [ "vlc.desktop" ];
      "audio/x-m4a" = [ "vlc.desktop" ];
      "audio/ogg" = [ "vlc.desktop" ];
      "audio/flac" = [ "vlc.desktop" ];
      "audio/x-flac" = [ "vlc.desktop" ];
      "audio/wav" = [ "vlc.desktop" ];
      "audio/x-wav" = [ "vlc.desktop" ];
      "audio/webm" = [ "vlc.desktop" ];
      "audio/aac" = [ "vlc.desktop" ];
      "audio/x-aac" = [ "vlc.desktop" ];
      "audio/opus" = [ "vlc.desktop" ];
      "audio/x-matroska" = [ "vlc.desktop" ];
      "audio/x-ms-wma" = [ "vlc.desktop" ];
      "audio/vorbis" = [ "vlc.desktop" ];
      "audio/x-vorbis+ogg" = [ "vlc.desktop" ];
      "audio/ac3" = [ "vlc.desktop" ];
      "audio/eac3" = [ "vlc.desktop" ];
      "audio/x-ape" = [ "vlc.desktop" ];
      "audio/x-musepack" = [ "vlc.desktop" ];
      "audio/x-wavpack" = [ "vlc.desktop" ];
    };
  };
}
