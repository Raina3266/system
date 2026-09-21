# Stock Rofi launchers plus the Rust-backed shared clipboard and file finder.
# Imported by ../../nixos/default.nix because Home Manager uses global pkgs.
{ ... }:
{
  home-manager.sharedModules = [
    (
      { config, pkgs, repoPackages, repoRoot, ... }:
      {
        home.packages = with pkgs; [
          rofi
          rofi-rbw
          whitesur-icon-theme
          repoPackages.rofiFilesearch
          fd

          # Thumbnailers rofi's icon fetcher shells out to. ffmpegthumbnailer
          # and gdk-pixbuf ship .thumbnailer files; the Rust finder handles PDF.
          ffmpegthumbnailer
          gdk-pixbuf
        ];

        xdg.configFile."rofi/config.rasi".source =
          config.lib.file.mkOutOfStoreSymlink "${repoRoot}/niri/rofi/config.rasi";

        # Reuse the Rust package's PDF renderer for small list-row thumbnails.
        xdg.dataFile."thumbnailers/pdftoppm.thumbnailer".text = ''
          [Thumbnailer Entry]
          TryExec=${repoPackages.rofiFilesearch}/bin/rofi-filesearch
          Exec=${repoPackages.rofiFilesearch}/bin/rofi-filesearch thumbnail %i %o %s
          MimeType=application/pdf;
        '';

        # The stock Rofi theme remains live-linked by ../../themes/default.nix.
      }
    )
  ];
}
