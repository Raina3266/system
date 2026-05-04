{ inputs, pkgs, ... }:
{
  home.packages = with pkgs; [
    bottom
    openssl
    gcc
    nil
    nixd
    neovim
    rustup
    cargo-machete
    diesel-cli
    cargo-audit
    cargo-autoinherit
  ];

  targets.genericLinux.nixGL.packages = inputs.nixGL.packages;
  targets.genericLinux.nixGL.defaultWrapper = "mesa";
  targets.genericLinux.nixGL.installScripts = [ "mesa" ];
}
