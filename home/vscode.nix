# The VS Code settings this configuration owns.
#
# Home Manager can generate settings.json from Nix, but the result lands in the
# store read-only, so VS Code's own settings UI can no longer write to it.
# Linking a copy out of the store does not help either: VS Code writes user
# data atomically and replaces a symlinked settings.json with a regular file
# the first time it saves, which would leave a stale link behind for the next
# activation to trip over.
#
# So merge the keys below into whatever is already in the file, the way
# ../themes/default.nix drives kwriteconfig for KDE. settings.json stays an
# ordinary writable file that the application is free to edit, everything not
# listed here is left alone, and these keys are put back on every rebuild.
{
  config,
  pkgs,
  ...
}:
let
  settingsPath = "${config.xdg.configHome}/Code/User/settings.json";

  # The official Dracula extension, and a variant of its standard theme with
  # the workbench colours stripped out so only code is recoloured. Installing
  # the extension itself as well costs nothing — it is where the variant is
  # built from — and leaves "Dracula Theme" and "Dracula Theme Soft" available
  # to pick from in the editor.
  dracula = pkgs.vscode-extensions.dracula-theme.theme-dracula;

  draculaSyntax = import ../themes/lib/vscode-syntax-theme.nix {
    inherit pkgs;
    source = "${dracula}/share/vscode/extensions/${dracula.vscodeExtUniqueId}";
    sourceLabel = "Dracula Theme";
    label = "Dracula Syntax";
    name = "dracula-syntax";
  };

  settings = {
    # Dracula's syntax highlighting over VS Code's own chrome. The full
    # "Dracula Theme" and "Dracula Theme Soft" are installed alongside it, and
    # ../themes/default.nix adds "Daemon-2.0" and "Daemon-2.0 Syntax"; any of
    # those names works here.
    "workbench.colorTheme" = "Dracula Syntax";

    # VS Code ships Copilot chat and inline suggestions as built-in features.
    # chat.disableAIFeatures is the switch for all of it — it hides chat and
    # inline suggestions and disables the Copilot extensions. The other two
    # cover what it does not: the chat entry in the title bar's command centre,
    # and ghost text from any other inline completion provider.
    "chat.disableAIFeatures" = true;
    "chat.commandCenter.enabled" = false;
    "editor.inlineSuggest.enabled" = false;
  };

  wantedSettings = (pkgs.formats.json { }).generate "vscode-settings.json" settings;

  # jq only reads strict JSON. VS Code accepts comments in settings.json, so a
  # hand-annotated file is left untouched rather than rewritten or destroyed.
  mergeSettings = pkgs.writeShellScript "merge-vscode-settings" ''
    set -eu

    target="$1"
    wanted="$2"

    mkdir -p "$(dirname "$target")"

    # An earlier generation may have left settings.json pointing into the
    # store, which is read-only. Drop that link so there is a real file to
    # merge into; links anywhere else are the user's own and are left be.
    if [ -L "$target" ]; then
      case "$(readlink -f "$target" || true)" in
        /nix/store/*) rm "$target" ;;
      esac
    fi

    [ -e "$target" ] || printf '{}\n' > "$target"

    if ! ${pkgs.jq}/bin/jq -e . "$target" > /dev/null 2>&1; then
      echo "VS Code: $target is not strict JSON, leaving it alone" >&2
      exit 0
    fi

    merged="$(mktemp "$target.XXXXXX")"
    ${pkgs.jq}/bin/jq --slurpfile wanted "$wanted" '. + $wanted[0]' "$target" > "$merged"

    # Write through rather than renaming over the target, so a settings.json
    # the user symlinked somewhere themselves keeps its link and its mode.
    cat "$merged" > "$target"
    rm -f "$merged"
  '';
in
{
  # ../themes/default.nix contributes the Daemon themes to the same list.
  programs.vscode.profiles.default.extensions = [
    dracula
    draculaSyntax
  ];

  home.activation.applyVscodeSettings = config.lib.dag.entryAfter [ "linkGeneration" ] ''
    $DRY_RUN_CMD ${mergeSettings} "${settingsPath}" "${wantedSettings}"
  '';
}
