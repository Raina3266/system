# nixpkgs overlay for rofi, imported by nixos/configuration.nix.
#
# This lives next to the home-manager rofi module (./default.nix) because
# the module depends on it: programs.rofi uses `pkgs.rofi`, which only has
# the filebrowser/recursivebrowser modes available thanks to the plugin
# injected below.
#
# rofi = the bash wrapper, rofi-unwrapped = the compiled C program.
final: prev: {
  # Pinned to the merge commit of PR #2272, which implements
  # click-to-exit on Wayland (fullscreen capture surface).
  rofi-unwrapped = prev.rofi-unwrapped.overrideAttrs (old: {
    version = "2.0.0-dev";
    src = final.fetchFromGitHub {
      owner = "davatorium";
      repo = "rofi";
      rev = "6d2a5281e45dee92dfbdaf6f9ba6081c4c608682";
      fetchSubmodules = true;
      hash = "sha256-4F76JPNaM43DgnM+F0WoYvL5aBbyPSZt3q0YWKAQ9Zs=";
    };
  });
  # rofi-file-browser builds against `rofi` (see its buildInputs),
  # so overriding `rofi` to include it would create a cycle.
  # Build the plugin against the unwrapped rofi instead.
  rofi-file-browser = prev.rofi-file-browser.override {
    rofi = final.rofi-unwrapped;
  };
  rofi = prev.rofi.override {
    plugins = [ final.rofi-file-browser ];
  };
}
