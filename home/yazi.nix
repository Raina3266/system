{
  lib,
  config,
  pkgs,
  ...
}: {
  home.packages = with pkgs; [
    ffmpeg
    mediainfo
    exiftool
  ];

  programs.yazi = {
    enable = true;
    enableFishIntegration = true;
    settings = {
      opener = {
        text = [
          {
            run = ''nvim "$@"'';
            block = true;
          }
        ];
      };
    };
  };
}
