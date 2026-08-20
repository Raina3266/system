{ inputs, pkgs, ... }:
{
  imports = [
    inputs.nixvim.homeModules.nixvim
    ./git.nix
    ./terminal.nix
  ];
  config = {
    # ── Neovim (nixvim) ───────────────────────────────────────────────────
    programs.nixvim = {
      enable = true;
      nixpkgs.source = pkgs.path;
    };

    # ── Shell (fish/bash, prompt, history, direnv) ────────────────────────
    programs.starship.enable = true;
    programs.bash.enable = true;

    programs.direnv = {
      enable = true;
      enableBashIntegration = true;
      nix-direnv.enable = true;
    };

    home.packages = with pkgs; [
      ffmpeg
      fzf
      bat
      tree
      yq
      bottom
      killall
      lsof
      coreutils
    ];

    programs.fish = {
      enable = true;
      interactiveShellInit = ''
        echo "hello from `programs.fish.interactiveShellInit`"
        source ~/.secrets.fish &>/dev/null || true
        fish_add_path ~/.cargo/bin
      '';
      shellAbbrs = {
        # Quick access to yazi bookmarks (press 'b' after launch)
        yb = "yazi";
      };
    };

    programs.atuin = {
      enable = true;
      enableFishIntegration = true;
      settings = {
        filter_mode_shell_up_key_binding = "session";
        enter_accept = true;
      };

      flags = [ "--disable-up-arrow" ];
    };
  };
}
