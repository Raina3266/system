{
  pkgs,
  inputs,
  ...
}:
{
  imports = [
    ../niri
    ../themes
    ./shell
    ./cloud.nix
    ./desktop.nix
    ./custom.nix
  ];

  home = {
    username = "raina";
    homeDirectory = "/home/raina";
    stateVersion = "26.05";
  };

  programs = {
    home-manager.enable = true;
    zed-editor.enable = true;
    vscode.enable = true;
    google-chrome.enable = true;
    firefox.enable = true;
  };

  home.packages = with pkgs; [
    # communication
    discord
    wechat
    qq
    whatsie
    telegram-desktop
    zoom-us
    wemeet
    handy
    wtype
    wl-clipboard

    # productivity
    digikam
    obsidian
    exercism
    onlyoffice-desktopeditors
    clash-verge-rev

    # Qt/Kde based.
    qdirstat
    qbittorrent
    kdePackages.kdenlive
    kdePackages.elisa
    kdePackages.dolphin
    kdePackages.ark
    kdePackages.baloo
    kdePackages.baloo-widgets
    kdePackages.kfilemetadata
    kdePackages.kio-fuse
    kdePackages.kompare
    kdePackages.dolphin-plugins
    kdePackages.plasma-integration
    kdePackages.print-manager
    kdePackages.skanpage

    # media
    vlc
    spotube
    puddletag
    obs-studio
    shotcut
    kid3
    gimp
    yt-dlp
    waylyrics
    inputs.sonora.packages.${pkgs.stdenv.hostPlatform.system}.default
  ];
}
