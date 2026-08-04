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
    
    postPatch = ''
      # Make textbox-current-entry wrap instead of truncating/ellipsizing.
      sed -i 's/TB_MARKUP | TB_AUTOHEIGHT, NORMAL, "", 0, 0)/TB_MARKUP | TB_AUTOHEIGHT | TB_WRAP, NORMAL, "", 0, 0)/' source/view.c
    '';
  });
}
