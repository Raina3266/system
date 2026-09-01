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

    programs.zoxide.enable = true;
    programs.zoxide.enableFishIntegration = true;

    programs.direnv = {
      enable = true;
      enableFishIntegration = true;
      nix-direnv.enable = true;
    };

    home.packages = with pkgs; [
      # CLI utilities. coreutils is deliberately absent: NixOS always provides
      # it via environment.defaultPackages, and a second copy in the user
      # profile only shadows the system one.
      fzf
      bat
      tree
      yq
      bottom
      killall
      lsof
      ffmpeg-full
      ripgrep

      # JavaScript/TypeScript
      deno

      # Rust
      cargo
      cargo-fuzz
      rustc
      rustfmt
      clippy
      rust-analyzer
      diesel-cli
      cargo-machete
      cargo-audit
      cargo-autoinherit

      # Lean
      elan

      # C toolchain / libraries
      openssl
      gcc

      # Nix
      nil
      nixd

      # Python
      uv

      # Typst
      typst
      tinymist
    ];

    programs.fish = {
      enable = true;
      interactiveShellInit = ''
        echo "hello from `programs.fish.interactiveShellInit`"
        source ~/.secrets.fish &>/dev/null || true
      '';
      shellInit = ''
        set -gx RUST_BACKTRACE 1
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

    targets.genericLinux.nixGL.packages = inputs.nixGL.packages;
    targets.genericLinux.nixGL.defaultWrapper = "mesa";
    targets.genericLinux.nixGL.installScripts = [ "mesa" ];
  };
}
