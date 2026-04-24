{ pkgs, ... }:
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
}
