# When you are in a home-manager module:
#  - use `config` to access the home-manager config
#  - use `nixosConfig` to access the nixos config
#
# When you are in a nixos module:
#  - you cannot access the home-manager config (because there are lots potentially)
#  - use `config` to access the nixos config

# The nixos config CONTAINS the home-manager config
{
  pkgs,
  ...
}:
{
  imports = [
    ./work
    ./personal
    # ./cloud.nix
    ./yazi.nix
    ./ocr.nix
    ./shell
    ./toolchains.nix
    ./nixgl.nix
    ./zed.nix
  ];

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
    google-chrome
    vlc
    obs-studio
    libreoffice
    pdf4qt
    zoom-us
    meld
    czkawka
    spotdl
    lrcget
    obsidian
    discord
    kid3
    yt-dlp
    ffmpeg
  ];

  programs.home-manager.enable = true;
}
