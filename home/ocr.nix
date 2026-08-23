{
  pkgs,
  ...
}:
let
  packages = import ../packages.nix { inherit pkgs; };
  inherit (packages) ocrScreenshot;
in
{
  home.packages = [ ocrScreenshot ];

  # GNOME keybind: Shift+Print, matching the niri binding. Registered via
  # dconf so GNOME's settings-daemon picks it up in a GNOME session.
  dconf.settings = {
    "org/gnome/settings-daemon/plugins/media-keys" = {
      custom-keybindings = [
        "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/ocr-shortcut/"
      ];
    };
    "org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/ocr-shortcut" = {
      binding = "<Shift>Print";
      command = "${ocrScreenshot}/bin/ocr-screenshot";
      name = "OCR Screenshot";
    };

    # GNOME desktop preferences (only active under a GNOME session).
    "org/gnome/desktop/interface" = {
      enable-hot-corners = false;
      show-battery-percentage = true;
    };
    "org/gnome/mutter" = {
      center-new-windows = true;
    };
  };
}
