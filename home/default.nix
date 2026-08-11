{
  pkgs,
  ...
}:
{
  imports = [
    ./cloud
    ./niri
    ./shell
    ./yazi
    ./desktop.nix
    ./thunderbird.nix
    ./ocr.nix
    ./office.nix
    ./toolchains.nix
    
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
    obsidian
    meld
    czkawka
    exercism
    stirling-pdf-desktop

    # media playback
    vlc
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

    # can be removed when it update later
    (tauon.overrideAttrs (old: {
      makeWrapperArgs = (old.makeWrapperArgs or [ ]) ++ [
        "--prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath [ libappindicator ]}"
      ];
    }))
  ];
}
