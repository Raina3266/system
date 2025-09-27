{
  pkgs,
  ...
}:
{
  home.packages = with pkgs; [
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
