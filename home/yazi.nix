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
    shellWrapperName = "y";
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
