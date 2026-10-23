{ pkgs, ags }:
let
  inherit (pkgs) lib;
in
ags.overrideAttrs (old: {
  postFixup = (old.postFixup or "") + ''
    makeWrapper ${lib.getExe pkgs.gjs} "$out/bin/panel-gjs" \
      "''${gappsWrapperArgs[@]}" \
      --set LD_PRELOAD "${pkgs.gtk4-layer-shell}/lib/libgtk4-layer-shell.so"
  '';
})
