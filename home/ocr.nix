{ pkgs, ... }:
let
  ocrScreenshot = pkgs.writeShellScriptBin "ocr-screenshot.sh" ''
    IMG_PATH=/tmp/screenshot.png

    gnome-screenshot --area --file=$IMG_PATH

    tesseract $IMG_PATH /tmp/ocr-output

    TEXT=$(cat /tmp/ocr-output.txt)
    echo $TEXT | wl-copy
    notify-send "Copied: $TEXT"

    # pkgs.translate-shell
    # trans -t zh-CN hello
  '';
in
{
  home.packages = [
    pkgs.gnome-screenshot
    pkgs.tesseract
    pkgs.wl-clipboard
    ocrScreenshot
  ];
}
