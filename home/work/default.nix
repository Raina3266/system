{
  lib,
  pkgs,
  nixosConfig,
  ...
}:
{
  config = lib.mkIf nixosConfig.services'.work.enable {
    home.packages =
      with pkgs;
      [
        pnpm
        docker-compose
        wasm-pack
        yq
        v4l-utils
        xh
        protobuf
        net-tools
      ];
  };
}
