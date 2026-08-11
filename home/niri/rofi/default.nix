# Rofi: application launcher / dmenu (replacing walker incrementally)
#
{ pkgs, config, ... }:
let
  # Wrapped so the .thumbnailer's Exec line is a single absolute path, with
  # pdftoppm resolved from the closure rather than looked up on the consumer's
  # PATH (tumblerd and rofi's fetcher do not inherit the user's environment).
  thumbnailer = pkgs.writeShellApplication {
    name = "thumbnailer";
    # writeShellApplication pins PATH to exactly these, so coreutils has to be
    # explicit for mktemp/mv -- the script must not depend on an inherited PATH.
    runtimeInputs = [
      pkgs.poppler-utils # pdftoppm
      pkgs.coreutils # mktemp, mv
    ];
    text = builtins.readFile ./thumbnailer.sh;
  };
in
{
  imports = [
    ./clipboard.nix
  ];

  home.packages = with pkgs; [
    rofi
    rofi-rbw
    rofi-file-browser
    rofi-network-manager
    rofi-bluetooth
    whitesur-icon-theme

    (writeShellScriptBin "rofi-filesearch" (builtins.readFile ./filesearch.sh))
    fd

    iwmenu
    # Thumbnailers rofi's icon fetcher shells out to. ffmpegthumbnailer and
    # gdk-pixbuf ship their own .thumbnailer files; PDFs get one below.
    thumbnailer
    ffmpegthumbnailer
    gdk-pixbuf
  ];

  # Nothing in nixpkgs provides an application/pdf thumbnailer (only evince,
  # which is excluded), so rofi and nemo both fall back to a generic mimetype
  # icon for PDFs. This fills that gap for every XDG consumer at once.
  xdg.dataFile."thumbnailers/pdftoppm.thumbnailer".text = ''
    [Thumbnailer Entry]
    TryExec=${thumbnailer}/bin/thumbnailer
    Exec=${thumbnailer}/bin/thumbnailer %i %o %s
    MimeType=application/pdf;
  '';

  xdg.configFile."rofi/config.rasi".source =
    config.lib.file.mkOutOfStoreSymlink "/home/raina/System/home/niri/rofi/config.rasi";
  xdg.configFile."rofi/rofi-finder.rasi".source =
    config.lib.file.mkOutOfStoreSymlink "/home/raina/System/home/niri/rofi/theme/rofi-finder.rasi";
  xdg.configFile."rofi/rofi-clipboard.rasi".source =
    config.lib.file.mkOutOfStoreSymlink "/home/raina/System/home/niri/rofi/theme/rofi-clipboard.rasi";

  # preview-panel reads this file at runtime and reloads it while open, so
  # placement and CSS changes need neither a rebuild nor a panel restart.
  xdg.configFile."preview-panel/config.toml".source =
    config.lib.file.mkOutOfStoreSymlink "/home/raina/System/scripts/preview-panel/config.toml";
}
