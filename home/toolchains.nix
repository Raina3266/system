{ pkgs, ... }:
{
  home.packages = with pkgs; [
    nodejs
    pnpm
    flutter
    rustup
  ];
}
