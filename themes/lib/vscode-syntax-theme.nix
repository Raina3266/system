# Build a VS Code extension contributing the syntax highlighting of an existing
# colour theme, without its workbench colours.
#
# A colour theme carries two independent layers: `colors`, which paints the
# editor's chrome, and `tokenColors`, which colours code. Dropping the former
# leaves VS Code to fall back on its own defaults for every workbench colour,
# so the editor keeps its usual appearance and only the code is recoloured.
{
  pkgs,
  # Extension directory to read the theme from, i.e. the directory holding its
  # package.json.
  source,
  # Label of the theme to take, as its own package.json spells it.
  sourceLabel,
  # Label of the theme this produces, and the identity to install it under.
  label,
  name,
  publisher ? "local",
  version ? "1.0.0",
}:
let
  uniqueId = "${publisher}.${name}";
in
pkgs.runCommandLocal "vscode-extension-${name}"
  {
    nativeBuildInputs = [
      pkgs.jq
      pkgs.python3
    ];
    # Read by Home Manager to install the extension and to describe it in
    # extensions.json.
    passthru = {
      inherit version;
      vscodeExtPublisher = publisher;
      vscodeExtName = name;
      vscodeExtUniqueId = uniqueId;
    };
  }
  ''
    dir="$out/share/vscode/extensions/${uniqueId}"
    mkdir -p "$dir/themes"

    themePath="$(jq -r --arg label "${sourceLabel}" \
      '.contributes.themes[] | select(.label == $label) | .path' \
      "${source}/package.json")"

    if [ -z "$themePath" ]; then
      echo "no theme labelled '${sourceLabel}' in ${source}" >&2
      exit 1
    fi

    python3 ${./vscode-syntax-only.py} \
      "${source}/$themePath" "$dir/themes/theme.json" "${label}"

    jq -n --arg name "${name}" \
          --arg publisher "${publisher}" \
          --arg version "${version}" \
          --arg label "${label}" \
      '{
        name: $name,
        publisher: $publisher,
        version: $version,
        displayName: $label,
        engines: { vscode: "^1.70.0" },
        categories: [ "Themes" ],
        contributes: {
          themes: [ { label: $label, uiTheme: "vs-dark", path: "./themes/theme.json" } ]
        }
      }' > "$dir/package.json"
  ''
