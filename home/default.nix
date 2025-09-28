{
  pkgs,
  ...
}:
{
  imports = [
    ./yazi.nix
    ./ocr.nix
    ./vscode.nix
    ./shell
    ./personal
    ./toolchains.nix
    ./nixgl.nix
    ./zed.nix
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
    vlc
    obs-studio
    qbittorrent
    libreoffice
    pdf4qt
    zoom-us
    meld
    czkawka
    kodi
  ];

  programs.home-manager.enable = true;
}
