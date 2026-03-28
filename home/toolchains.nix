{ pkgs, ... }:
{
  home.packages = with pkgs; [
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
