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
      nodejs
      pnpm
      tailscale
      slack
      docker
      docker-compose
    ];
  };
}