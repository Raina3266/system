# Rofi's system package and Rust-backed application, file and folder finder.
# Imported by ../../nixos/default.nix because Home Manager uses global pkgs.
{ ... }:
{
  nixpkgs.overlays = [
    (final: prev: {
      rofi-unwrapped = prev.rofi-unwrapped.overrideAttrs (oldAttrs: {
        version = "2.0.0-dev";

        # Upstream revision with Wayland click-to-exit support from PR #2272.
        src = final.fetchFromGitHub {
          owner = "davatorium";
          repo = "rofi";
          rev = "6d2a5281e45dee92dfbdaf6f9ba6081c4c608682";
          fetchSubmodules = true;
          hash = "sha256-4F76JPNaM43DgnM+F0WoYvL5aBbyPSZt3q0YWKAQ9Zs=";
        };

        patches = (oldAttrs.patches or [ ]) ++ [ ./rofi.patch ];
      });
    })
  ];

  home-manager.sharedModules = [
    (
      { pkgs, repoPackages, ... }:
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

        xdg.configFile."rofi/config.rasi".text = ''
          /* ROFI: base configuration */

          configuration {
              /* ── Top-right corner ── */
              location: 3;
              x-offset: 5px;
              y-offset: 44px;

              scroll-method: 0;
              cycle: false;
              click-to-exit: true;

              sidebar-mode: true;

              show-icons: true;
              icon-theme: "WhiteSur-dark";
          }

          @theme "~/.config/rofi/rofi-finder.rasi"
        '';

        # Reuse the Rust package's PDF renderer for small list-row thumbnails.
        xdg.dataFile."thumbnailers/pdftoppm.thumbnailer".text = ''
          [Thumbnailer Entry]
          TryExec=${repoPackages.rofiFilesearch}/bin/rofi-filesearch
          Exec=${repoPackages.rofiFilesearch}/bin/rofi-filesearch thumbnail %i %o %s
          MimeType=application/pdf;
        '';

        # The separate .rasi themes remain live-linked by ../../themes/default.nix.
      }
    )
  ];
}
