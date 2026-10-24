# Rofi's shared system package and configuration for clipboard/password menus.
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
      { config, pkgs, repoRoot, ... }:
      {
        home.packages = with pkgs; [
          rofi
          rofi-rbw
          whitesur-icon-theme
        ];

        xdg.configFile."rofi/config.rasi".source =
          config.lib.file.mkOutOfStoreSymlink "${repoRoot}/niri/rofi/config.rasi";

        # The clipboard theme remains live-linked by ../../themes/default.nix.
      }
    )
  ];
}
