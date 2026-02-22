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
        slack
        terraform
        terragrunt
        azure-cli
        nodejs
        pnpm
        docker-compose
        wasm-pack
        yq
        # v4l2-ctl -d /dev/video33 --set-fmt-video=width=1280,height=720,pixelformat=YUYV
        v4l-utils
        webcamoid
        ipu6epmtl-camera-hal
        gst_all_1.icamerasrc-ipu6epmtl
      ];
  };
}
