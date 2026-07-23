{ pkgs, lib ? pkgs.lib }:
let
  # Pinned upstream release.
  src = pkgs.fetchzip {
    url = "https://github.com/yagybaba/waybar-ycal/archive/refs/tags/v1.1.0.tar.gz";
    sha256 = "0483nv1dspa7a90s8hxkb3kmva9r6c8qb61hilaks483n92lwf7a";
  };

  python = pkgs.python3;

  pythonWithDeps = python.withPackages (ps: [
    ps.google-api-python-client
    ps.google-auth
    ps.google-auth-oauthlib
    ps.google-auth-httplib2
    # GTK bindings (popup.py uses gi + Gtk4LayerShell)
    ps.pygobject3
  ]);

  # Build-time-resolved typelib directories so the wrapper doesn't scan
  # /nix/store at runtime (which is slow and can cause systemd timeouts).
  typelibPath = lib.makeSearchPath "lib/girepository-1.0" [
    pkgs.gtk4
    pkgs.gtk4-layer-shell
    (lib.getLib pkgs.pango)
    pkgs.graphene
    pkgs.gobject-introspection
    pkgs.harfbuzz
    pkgs.gdk-pixbuf
  ];

  libraryPath = lib.makeLibraryPath [
    pkgs.gtk4
    pkgs.gtk4-layer-shell
    pkgs.pango
    pkgs.harfbuzz
  ];

  popupWrapper = pkgs.writeShellScriptBin "waybar-ycal-popup" ''
    #!/usr/bin/env sh
    set -euo pipefail

    # Wrapper is shipped inside the derivation output; derive popup.py location
    # relative to the wrapper path.
    OUT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
    POPUP_PY="$OUT_DIR/share/waybar-ycal/popup.py"

    export GI_TYPELIB_PATH="${typelibPath}"
    export LD_LIBRARY_PATH="${libraryPath}"

    exec "${pythonWithDeps}/bin/python" "$POPUP_PY" "$@"
  '';

  waybarYcal = pkgs.stdenvNoCC.mkDerivation {
    pname = "waybar-ycal";
    version = "1.1.0";
    dontUnpack = true;

    nativeBuildInputs = [ python ];

    installPhase = ''
      mkdir -p $out/share/waybar-ycal
      mkdir -p $out/bin

      cp ${src}/bar.py ${src}/popup.py ${src}/toggle.sh $out/share/waybar-ycal/
      chmod +x $out/share/waybar-ycal/bar.py
      chmod +x $out/share/waybar-ycal/popup.py
      chmod u+w $out/share/waybar-ycal/popup.py
      chmod +x $out/share/waybar-ycal/toggle.sh

      # Patch: force your cyberpunk palette + place popup at top-left.
      python - <<PY
      import re
      from pathlib import Path

      p = Path('$out/share/waybar-ycal/popup.py')
      s = p.read_text()

      # Replace load_theme() entirely with cyberpunk colors.
      s = re.sub(
        r"def load_theme\(\):\n\s*defaults = \{[\s\S]*?\n\s*except Exception:\n\s*return defaults\n",
        "def load_theme():\n    return {\n        'foreground': '#cbe3e7',\n        'background': '#0E0616',\n        'accent': '#ff7edb',\n    }\n",
        s,
      )

      # Anchor popup to the top-left corner.
      s = s.replace(
        "Gtk4LayerShell.set_anchor(self, Gtk4LayerShell.Edge.LEFT, False)",
        "Gtk4LayerShell.set_anchor(self, Gtk4LayerShell.Edge.LEFT, True)",
      )

      # Add a small left margin to avoid touching the edge.
      s = s.replace(
        "Gtk4LayerShell.set_margin(self, Gtk4LayerShell.Edge.TOP, 4)\n",
        "Gtk4LayerShell.set_margin(self, Gtk4LayerShell.Edge.TOP, 4)\n        Gtk4LayerShell.set_margin(self, Gtk4LayerShell.Edge.LEFT, 4)\n",
      )

      p.write_text(s)
      PY

      # Install wrapper that sets GI_TYPELIB_PATH/LD_LIBRARY_PATH for GTK4.
      cp ${popupWrapper}/bin/waybar-ycal-popup $out/bin/waybar-ycal-popup
      chmod +x $out/bin/waybar-ycal-popup
    '';
  };

  # Exec command for the bar module.
  ycalBarExec = "${pythonWithDeps}/bin/python ${waybarYcal}/share/waybar-ycal/bar.py";

  ycalToggle = pkgs.writeShellScript "waybar-ycal-toggle" ''
    set -euo pipefail

    PID_FILE="$HOME/.cache/waybar-ycal/popup.pid"

    if [ -f "$PID_FILE" ]; then
      PID="$(cat "$PID_FILE" 2>/dev/null || true)"
      if [ -n "$PID" ] && kill -0 "$PID" 2>/dev/null; then
        kill -SIGUSR1 "$PID"
        exit 0
      fi
    fi

    # Prefer systemd-managed daemon.
    systemctl --user start waybar-ycal.service >/dev/null 2>&1 || true

    # Wait briefly for the daemon to write the pid file.
    for _ in 1 2 3 4 5; do
      if [ -f "$PID_FILE" ]; then
        PID="$(cat "$PID_FILE" 2>/dev/null || true)"
        if [ -n "$PID" ] && kill -0 "$PID" 2>/dev/null; then
          kill -SIGUSR1 "$PID"
          exit 0
        fi
      fi
      sleep 0.2
    done

    # Fallback: launch directly via the wrapper.
    ${waybarYcal}/bin/waybar-ycal-popup &
  '';
in
{
  inherit waybarYcal pythonWithDeps ycalBarExec ycalToggle;
}
