{
  pkgs,
  ...
}:
{
  imports = [
    ./yazi.nix
    ./ocr.nix
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
    (pkgs.writeShellScriptBin "obs" ''
      ${pkgs.nixgl.nixGLIntel}/bin/nixGLIntel ${pkgs.obs-studio}/bin/obs "$@"
    '')
    qbittorrent
    libreoffice
    pdf4qt
    zoom-us
    meld
    czkawka
    spotdl
    lrcget    
    fooyin
  ];

  programs.home-manager.enable = true;
}
