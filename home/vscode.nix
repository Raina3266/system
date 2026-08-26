# Keep a small managed subset of VS Code settings while leaving settings.json
# writable. Home Manager's normal userSettings option would link it read-only
# from the Nix store.
{
  config,
  pkgs,
  ...
}:
let
  settingsPath = "${config.xdg.configHome}/Code/User/settings.json";

  managedSettings = {
    # ../themes/default.nix builds this as Daemon's application/workbench
    # colours combined with Dracula's code syntax highlighting.
    "workbench.colorTheme" = "Daemon-2.0";

    # VS Code ships Copilot chat and inline suggestions as built-in features.
    # chat.disableAIFeatures is the switch for all of it — it hides chat and
    # inline suggestions and disables the Copilot extensions. The other two
    # cover what it does not: the chat entry in the title bar's command centre,
    # and ghost text from any other inline completion provider.
    "chat.disableAIFeatures" = true;
    "chat.commandCenter.enabled" = false;
    "editor.inlineSuggest.enabled" = false;
  };

  managedSettingsFile =
    (pkgs.formats.json { }).generate "vscode-settings.json" managedSettings;

  # jq only reads strict JSON. VS Code accepts comments in settings.json, so a
  # hand-annotated file is left untouched rather than rewritten or destroyed.
  mergeSettings = pkgs.writeShellScript "merge-vscode-settings" ''
    set -eu

    target="$1"
    managed="$2"

    mkdir -p "$(dirname "$target")"

    # An earlier generation may have left settings.json pointing into the
    # store, which is read-only. Drop that link so there is a real file to
    # merge into; links anywhere else are the user's own and are left be.
    if [ -L "$target" ]; then
      case "$(readlink -f "$target" || true)" in
        /nix/store/*) rm "$target" ;;
      esac
    fi

    if [ ! -e "$target" ]; then
      printf '{}\n' > "$target"
    elif ! ${pkgs.jq}/bin/jq -e . "$target" > /dev/null 2>&1; then
      echo "VS Code: $target is not strict JSON, leaving it alone" >&2
      exit 0
    fi

    temporary="$(mktemp "$target.XXXXXX")"
    trap 'rm -f "$temporary"' EXIT
    ${pkgs.jq}/bin/jq -s '.[0] + .[1]' "$target" "$managed" > "$temporary"

    # Write through rather than renaming over the target, so a settings.json
    # the user symlinked somewhere themselves keeps its link and its mode.
    cat "$temporary" > "$target"
  '';
in
{
  home.activation.applyVscodeSettings = config.lib.dag.entryAfter [ "linkGeneration" ] ''
    $DRY_RUN_CMD ${mergeSettings} "${settingsPath}" "${managedSettingsFile}"
  '';
}
