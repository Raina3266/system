# Rofi: Rust-backed application, file and folder finder.
{
  pkgs,
  repoPackages,
  ...
}:
{
  home.packages = with pkgs; [
    rofi
    rofi-rbw
    whitesur-icon-theme
    repoPackages.rofiFilesearch
    fd

    # Thumbnailers rofi's icon fetcher shells out to. ffmpegthumbnailer and
    # gdk-pixbuf ship their own .thumbnailer files; the Rust finder handles PDF.
    ffmpegthumbnailer
    gdk-pixbuf
  ];

  # Reuse the Rust package's PDF renderer for Rofi's small list-row thumbnails.
  xdg.dataFile."thumbnailers/pdftoppm.thumbnailer".text = ''
    [Thumbnailer Entry]
    TryExec=${repoPackages.rofiFilesearch}/bin/rofi-filesearch
    Exec=${repoPackages.rofiFilesearch}/bin/rofi-filesearch thumbnail %i %o %s
    MimeType=application/pdf;
  '';

  # The rofi config and every .rasi theme are linked from ../../themes/default.nix.
}
