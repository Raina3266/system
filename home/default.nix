{
  pkgs,
  ...
}:
let
  mprisenceNativeHost = pkgs.writeTextDir
    "etc/chromium/native-messaging-hosts/mprisence.web.bridge.json"
    (builtins.toJSON {
      name = "mprisence.web.bridge";
      description = "Publish each browser media tab as an MPRIS player";
      path = "${pkgs.mprisence}/bin/mprisence";
      type = "stdio";
      allowed_origins = [
        "chrome-extension://pnkkjbdopihogobhhjbgapbpfccinjjo/"
        "chrome-extension://pphdmbejbipjlocngoefnmjoijcbdejf/"
      ];
    });
in
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

  programs = {
    home-manager.enable = true;
    zed-editor.enable = true;
    vscode.enable = true;

    google-chrome = {
      enable = true;
      commandLineArgs = [
        "--disable-features=HardwareMediaKeyHandling,MediaSessionService"
      ];
      nativeMessagingHosts = [ mprisenceNativeHost ];
    };

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

    # productivity / office
    obsidian
    meld
    czkawka
    exercism
    stirling-pdf-desktop

    # media playback
    mprisence
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
