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
  inputs,
  ...
}:
{
  imports = [
    ./work
    ./personal
    ./shell
    ./ocr.nix
    ./toolchains.nix
  ];

  home = {
    username = "raina";
    homeDirectory = "/home/raina";
  };
  
  programs.home-manager.enable = true;
  programs.zed-editor.enable = true;
  # Zed nightly from the upstream flake (matches zed.cachix.org; see flake.nix).
  programs.zed-editor.package = inputs.zed.packages.${pkgs.stdenv.hostPlatform.system}.default;
  programs.thunderbird = {
    enable = true;
    profiles.default = {
      isDefault = true;
    };
  };

  home.packages = with pkgs; [
    google-chrome
    firefox
    vlc
    obs-studio
    slack
    libreoffice
    pdf4qt
    zoom-us
    meld
    czkawka
    obsidian
    discord
    kid3
    spotdl
    sunshine
    exercism
    waylyrics
    tauon
    fooyin
    clash-verge-rev
    yt-dlp
    openai-whisper
    anki
    jupyter
  ];

  home.shellAliases = {
    obcli = "~/.local/bin/obsidian";
  };
}
