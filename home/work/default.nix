{
  lib,
  pkgs,
  nixosConfig,
  ...
}:
{
  config = lib.mkIf nixosConfig.services'.work.enable {
    programs.docker-cli.enable = true;
    programs.lazydocker.enable = true;
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