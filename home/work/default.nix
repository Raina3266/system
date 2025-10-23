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
      terraform
      terragrunt
      azure-cli
      nodejs
      pnpm
      docker-compose
      wasm-pack
      yq
    ];
  };
}