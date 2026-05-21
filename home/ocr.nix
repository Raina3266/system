{ pkgs, ... }:
let
  ocrScreenshot = pkgs.writeShellScriptBin "ocr-screenshot.sh" ''
    IMG_PATH=/tmp/screenshot.png

    # Take screenshot
    gnome-screenshot --area --file=$IMG_PATH

    # Run OCR
    tesseract $IMG_PATH /tmp/ocr-output

    # Copy to Wayland clipboard and notify
    TEXT=$(cat /tmp/ocr-output.txt)
    echo "$TEXT" | wl-copy
    notify-send "Copied: $TEXT"
  '';
in
{
  home.packages = with pkgs; [
    gnome-screenshot
    tesseract
    wl-clipboard
    ocrScreenshot
  ];
  dconf.settings = {
    "org/gnome/desktop/interface" = {
      enable-hot-corners = false;
    };

    "org/gnome/desktop/interface" = {
      show-battery-percentage = true;
    };

    "org/gnome/mutter" = {
      center-new-windows = true;
    };

    # Custom keybindings
    "org/gnome/settings-daemon/plugins/media-keys" = {
      custom-keybindings = [
        "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/ocr-shortcut/"
      ];
    };
    "org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/ocr-shortcut" = {
      binding = "<Alt>s";
      command = "${ocrScreenshot}/bin/ocr-screenshot.sh";
      name = "OCR Screenshot";
    };
  };
}
