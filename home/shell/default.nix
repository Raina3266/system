{ pkgs, ... }:
{
  imports = [
    ./nvim
    ./atuin.nix
    ./git.nix
    ./terminal.nix
    ./tmux.nix
  ];
  config = {
    programs.starship.enable = true;
    programs.bash.enable = true;
    programs.direnv.enable = true;
    programs.direnv.enableBashIntegration = true;
    programs.direnv.nix-direnv.enable = true;

    home.packages = with pkgs; [
      bat
      tree
      yq 
      bottom
      killall
      
      coreutils
    ];
    
    programs.fish.enable = true;
    programs.fish.interactiveShellInit = ''
      source ~/.secrets.fish &>/dev/null || true
    '';
  };
}
