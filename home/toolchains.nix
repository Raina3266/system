{ pkgs, ... }:
{
  home.packages = with pkgs; [
    nil
    nixd
    neovim
    rustup
  ];
}
