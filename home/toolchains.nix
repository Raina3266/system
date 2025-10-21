{ pkgs, ... }:
{
  home.packages = with pkgs; [
    gcc
    
    nil
    nixd
    neovim
    flutter
    rustup
  ];
}
