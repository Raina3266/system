{pkgs, ...}: {
  imports = [./nvim ./git.nix ./terminal.nix ./tmux.nix];
  config = {
    programs.starship.enable = true;

    home.packages = with pkgs; [
      bat
      tree
      flutter
      rustup
      gcc
    ];
  };

  # programs.fish = {
  #   enable = true;
  #   shellAbbrs = {
  #     "gs" = "git switch";
  #   };
  # };
}
