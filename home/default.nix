{
  lib,
  config,
  pkgs,
  ...
}: {
  imports = [
    ./yazi.nix
    ./cloud.nix
    ./ocr.nix
    ./vscode.nix
    ./jellyfin.nix
    ./shell
    ./personal
    
  ];

  # https://nixos.wiki/wiki/FAQ/When_do_I_update_stateVersion
  # home.stateVersion = "23.05";
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
    nil
    nixd
    neovim
    google-chrome
    whatsapp-for-linux
    discord
    vlc
    gimp-with-plugins
    kdePackages.kdenlive
    obs-studio
    qbittorrent
    anki-bin
    libreoffice
    pdf4qt
    kid3
    gui-for-clash
    zoom-us
    zed-editor
    # Useful shortcuts
    # `gra` - open code actions
    # `gd` - go to definition
    # `grr` - go to references (places where the current thing is being used)
    # `<ctrl+o> - go back
    # `<ctrl+i> - go forward
    wechat
    qq
    meld
    czkawka
    lollypop
    spotdl
    lrcget
    ytdownloader
  ];
  
  programs.home-manager.enable = true;
}
