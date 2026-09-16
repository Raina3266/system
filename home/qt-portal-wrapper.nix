{ pkgs }:
package:
pkgs.symlinkJoin {
  name = "${package.name}-portal";
  paths = [ package ];
  nativeBuildInputs = [ pkgs.makeWrapper ];
  postBuild = ''
    for program in "$out/bin/"*; do
      # Directories can be executable too; only wrap regular files.
      if [ -f "$program" ] && [ -x "$program" ]; then
        wrapProgram "$program" --set QT_QPA_PLATFORMTHEME xdgdesktopportal
      fi
    done
  '';
  inherit (package) meta;
}
