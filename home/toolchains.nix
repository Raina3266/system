{ pkgs, ... }:
{
  home.packages = with pkgs; [
    nil
    nixd
    neovim
    flutter
    rustup
  ];
}
