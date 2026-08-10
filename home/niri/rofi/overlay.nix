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
    # Keep the visible script row short while allowing the selection-change
    # callback to receive that row's original, stable value.
    patches = (old.patches or [ ]) ++ [ ./on-selection-changed-completion.patch ];
  });
}
