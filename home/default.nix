{
  pkgs,
  ...
}:
{
  imports = [
    ./cloud
    ../niri
    ./shell
    ./yazi
    ./desktop.nix
    ./thunderbird.nix
    ./ocr.nix
    ./office.nix
    ./spotdl.nix
    ./toolchains.nix

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
    telegram-desktop

    # productivity / office
    obsidian
    meld
    czkawka
    exercism
    stirling-pdf-desktop

    # media playback
    lrcget
    vlc
    waylyrics
    kdePackages.elisa

    # media creation / editing
    pavucontrol
    obs-studio
    inkscape
    gimp
    shotcut
    kid3
    yt-dlp # spotdl's own copy is patched+wrapped in ./spotdl.nix

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
