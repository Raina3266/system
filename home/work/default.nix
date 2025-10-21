{
  lib,
  pkgs,
  nixosConfig,
  ...
}:
{
  config = lib.mkIf nixosConfig.services'.work.enable {
    home.packages = with pkgs; [
      slack
    ] ++ [
      nodejs
      pnpm
      docker-compose
      wasm-pack
    ];
  };
}