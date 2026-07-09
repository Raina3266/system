{
  pkgs,
  ...
}:
let
  # Region-select OCR that works under both GNOME and niri.
  # The script detects the running session and uses the appropriate
  # screenshot tool: gnome-screenshot under GNOME, grim+slurp under niri.
  ocrScreenshot = pkgs.writeShellScriptBin "ocr-screenshot.sh" ''
    set -euo pipefail

    IMG_PATH=/tmp/ocr-screenshot.png

    # Detect the session and use the appropriate screenshot tool.
    case "''${XDG_CURRENT_DESKTOP:-}" in
      *GNOME*)
        # GNOME Shell provides its own screenshot interface.
        ${pkgs.gnome-screenshot}/bin/gnome-screenshot --area --file="$IMG_PATH"
        ;;
      *)
        # niri / wlroots: slurp picks a region, grim captures it.
        ${pkgs.grim}/bin/grim -g "$(${pkgs.slurp}/bin/slurp)" "$IMG_PATH"
        ;;
    esac

    # Abort if the user cancelled the region selection (no file produced).
    [ -f "$IMG_PATH" ] || exit 0

    # Run OCR on the captured region.
    ${pkgs.tesseract}/bin/tesseract "$IMG_PATH" /tmp/ocr-output

    # Copy to Wayland clipboard and notify.
    TEXT=$(cat /tmp/ocr-output.txt)
    echo "$TEXT" | ${pkgs.wl-clipboard}/bin/wl-copy
    ${pkgs.libnotify}/bin/notify-send "OCR" "Copied: $TEXT"
  '';
in
{
  home.packages = with pkgs; [
    # Screenshot tools — both sets are installed so the script can pick
    # the right one at runtime depending on the active session.
    gnome-screenshot # GNOME
    grim # niri / wlroots
    slurp # niri / wlroots (region selector)
    tesseract
    wl-clipboard
    libnotify
    ocrScreenshot
  ];

  # GNOME keybind: <Super><Shift>o (matches niri's Mod+Shift+O, since
  # Mod = Super on a TTY). Registered via dconf so GNOME's settings-daemon
  # picks it up when running a GNOME session.
  dconf.settings = {
    "org/gnome/settings-daemon/plugins/media-keys" = {
      custom-keybindings = [
        "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/ocr-shortcut/"
      ];
    };
    "org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/ocr-shortcut" = {
      binding = "<Shift>Print";
      command = "${ocrScreenshot}/bin/ocr-screenshot.sh";
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
