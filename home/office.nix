{
  config,
  lib,
  pkgs,
  ...
}:

let
  onlyofficeFonts = "${config.xdg.dataHome}/fonts/onlyoffice";

  onlyofficeScaled = pkgs.symlinkJoin {
    name = "onlyoffice-desktopeditors-scaled";

    paths = [
      pkgs.onlyoffice-desktopeditors
    ];

    nativeBuildInputs = [
      pkgs.makeWrapper
    ];

    postBuild = ''
      rm "$out/bin/onlyoffice-desktopeditors"

      makeWrapper \
        "${pkgs.onlyoffice-desktopeditors}/bin/onlyoffice-desktopeditors" \
        "$out/bin/onlyoffice-desktopeditors" \
        --unset QT_SCALE_FACTOR \
        --unset QT_SCREEN_SCALE_FACTORS \
        --unset QT_AUTO_SCREEN_SCALE_FACTOR \
        --add-flags "--force-scale=1"
    '';
  };
in
{
  programs.onlyoffice = {
    enable = true;
    package = onlyofficeScaled;

    settings = {
      UITheme = "theme-contrast-dark";
      maximized = false;
    };
  };

  home.activation.onlyofficeFonts = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
    set -eu

    sourceDir="/run/current-system/sw/share/X11/fonts"
    destination="${onlyofficeFonts}"
    fontCache="${config.xdg.dataHome}/onlyoffice/desktopeditors/data/fonts"

    if [ ! -d "$sourceDir" ]; then
      echo "OnlyOffice: $sourceDir is missing; enable fonts.fontDir.enable"
    else
      mkdir -p "$destination"

      # -L dereferences NixOS font symlinks into real files.
      ${pkgs.rsync}/bin/rsync \
        -aL \
        --delete \
        "$sourceDir/" \
        "$destination/"

      ${pkgs.findutils}/bin/find "$destination" \
        -type d -exec chmod 0755 {} +

      ${pkgs.findutils}/bin/find "$destination" \
        -type f -exec chmod 0644 {} +

      # Force OnlyOffice to rebuild its internal font cache.
      if [ -d "$fontCache" ]; then
        ${pkgs.findutils}/bin/find "$fontCache" \
          -mindepth 1 -delete
      fi

      ${pkgs.fontconfig}/bin/fc-cache \
        -f "$destination" >/dev/null 2>&1 || true
    fi
  '';
}
