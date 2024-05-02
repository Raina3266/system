{
  lib,
  config,
  pkgs,
  ...
}: {
  nixpkgs = {
    config = {
      allowUnfree = true;
    };
  };

  # https://nixos.wiki/wiki/FAQ/When_do_I_update_stateVersion
  home.stateVersion = "23.05";
  home = {
    username = "raina";
    homeDirectory = "/home/raina";
  };

  # Add stuff for your user as you see fit:
  # programs.neovim.enable = true;
  # home.packages = with pkgs; [ steam ];

  # Enable home-manager and git
  programs.git.enable = true;
  programs.git.userName = "Raina";
  programs.git.userEmail = "chenganlin990326@gmail.com";
  programs.gh.enable = true;


  home.packages = with pkgs; [
    vscode
    google-chrome
    vlc
    libreoffice
    obsidian
  ];

}
