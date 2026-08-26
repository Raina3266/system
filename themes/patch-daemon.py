#!/usr/bin/env python3
"""Build the two small local variants of the Daemon 2.0 theme.

``desktop`` changes only action icons and interactive-state backgrounds.
``vscode`` keeps Daemon's workbench colours and imports Dracula's syntax rules.
"""

import argparse
import json
import pathlib
import re
import shutil
import stat
import xml.etree.ElementTree as ET


SVG_NAMESPACES = {
    "": "http://www.w3.org/2000/svg",
    "svg": "http://www.w3.org/2000/svg",
    "inkscape": "http://www.inkscape.org/namespaces/inkscape",
    "sodipodi": "http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd",
    "xlink": "http://www.w3.org/1999/xlink",
}

HEX_COLOUR = re.compile(r"#[0-9a-f]{6}", re.IGNORECASE)
STATE_CENTRE = re.compile(r"-(focused|pressed|toggled)$")
CYAN_ICON_COLOURS = {"#5df4fe", "#5df2ff", "#5beedc", "#5aeedc"}
STATE_BASE_COLOURS = {"#272932", "#1e1e1e", "#14101f", "#331319"}


def make_writable(path: pathlib.Path) -> None:
    path.chmod(path.stat().st_mode | stat.S_IWUSR)


def replace_colours(text: str, mapping: dict[str, str]) -> str:
    return HEX_COLOUR.sub(
        lambda match: mapping.get(match.group(0).lower(), match.group(0)), text
    )


def patch_action_icons(directory: pathlib.Path, icon_colour: str) -> int:
    """Recolour only icons used for actions/buttons; leave every other icon alone."""
    changed = 0
    mapping = {colour: icon_colour for colour in CYAN_ICON_COLOURS}
    for relative in ("scalable/actions", "symbolic/actions"):
        for path in sorted((directory / relative).rglob("*.svg")):
            if path.is_symlink():
                continue
            text = path.read_text()
            replaced = replace_colours(text, mapping)
            if replaced != text:
                make_writable(path)
                path.write_text(replaced)
                changed += 1
    if changed == 0:
        raise RuntimeError("no Daemon action icons contained the expected cyan colours")
    return changed


def parse_style(value: str) -> dict[str, str]:
    result = {}
    for item in value.split(";"):
        if ":" in item:
            key, setting = item.split(":", 1)
            result[key.strip()] = setting.strip()
    return result


def format_style(style: dict[str, str]) -> str:
    return ";".join(f"{key}:{value}" for key, value in style.items())


def numeric_opacity(node: ET.Element) -> float:
    style = parse_style(node.get("style", ""))
    opacity = 1.0
    for key in ("opacity", "fill-opacity"):
        raw = style.get(key, node.get(key))
        if raw is not None:
            try:
                opacity *= float(raw)
            except ValueError:
                pass
    return opacity


def nodes_with_opacity(root: ET.Element):
    def walk(node: ET.Element, inherited: float):
        effective = inherited * numeric_opacity(node)
        yield node, effective
        for child in node:
            yield from walk(child, effective)

    yield from walk(root, 1.0)


def set_fill(node: ET.Element, colour: str) -> bool:
    """Set fill without changing strokes or any unrelated style properties."""
    style = parse_style(node.get("style", ""))
    if "fill" in style:
        if style["fill"].lower() == colour.lower():
            return False
        style["fill"] = colour
        node.set("style", format_style(style))
        return True

    fill = node.get("fill")
    if fill and fill.lower() != "none" and fill.lower() != colour.lower():
        node.set("fill", colour)
        return True
    return False


def fill_colour(node: ET.Element) -> str | None:
    style = parse_style(node.get("style", ""))
    return style.get("fill", node.get("fill"))


def patch_state_backgrounds(
    path: pathlib.Path, pink: str, dim_pink: str
) -> tuple[int, int]:
    """Patch only the central/interior layer of interactive Kvantum states.

    Kvantum stores frame edges as sibling elements with suffixes such as
    ``-focused-left``. Restricting the match to IDs that *end* in the state
    name changes the background/interior while preserving those frame edges.
    """
    for prefix, uri in SVG_NAMESPACES.items():
        ET.register_namespace(prefix, uri)

    tree = ET.parse(path)
    root = tree.getroot()
    states = [node for node in root.iter() if STATE_CENTRE.search(node.get("id", ""))]
    changed = 0

    for state in states:
        nodes = list(nodes_with_opacity(state))
        translucent_cyan = []
        for node, opacity in nodes:
            fill = fill_colour(node)
            if fill and fill.lower() in CYAN_ICON_COLOURS and 0 < opacity < 1:
                translucent_cyan.append(node)

        if translucent_cyan:
            for node in translucent_cyan:
                changed += set_fill(node, pink)
            continue

        # Focused interiors without a translucent overlay use Daemon's dark
        # surface directly. Replace that surface with an already-dimmed pink.
        for node, _opacity in nodes:
            fill = fill_colour(node)
            if fill and fill.lower() in STATE_BASE_COLOURS:
                changed += set_fill(node, dim_pink)

    if not states or changed == 0:
        raise RuntimeError("no Kvantum interactive-state backgrounds were changed")

    make_writable(path)
    tree.write(path, encoding="UTF-8", xml_declaration=True)
    return len(states), changed


def rewrite_ini_key(
    path: pathlib.Path, section_name: str, keys: set[str], value: str
) -> int:
    section = ""
    changed = 0
    output = []
    for line in path.read_text().splitlines():
        heading = re.match(r"^\[(.+)]$", line)
        if heading:
            section = heading.group(1)
        elif section == section_name:
            setting = re.match(r"^([^=]+)=(.*)$", line)
            if setting and setting.group(1) in keys and setting.group(2).strip() != value:
                line = f"{setting.group(1)}={value}"
                changed += 1
        output.append(line)

    if changed == 0:
        raise RuntimeError(f"no keys changed in [{section_name}] of {path}")
    make_writable(path)
    path.write_text("\n".join(output) + "\n")
    return changed


def hex_to_kde_rgb(value: str) -> str:
    value = value.removeprefix("#")
    if len(value) != 6:
        raise ValueError(f"expected #RRGGBB colour, got {value!r}")
    return ",".join(str(int(value[index : index + 2], 16)) for index in (0, 2, 4))


def patch_desktop(args: argparse.Namespace) -> None:
    source = pathlib.Path(args.source)
    output = pathlib.Path(args.out)

    kvantum = output / "Kvantum" / "daemon-2.0"
    shutil.copytree(source / "Kvantum" / "daemon-2.0", kvantum, symlinks=True)

    icons = output / "icons" / "Daemon-Icons"
    shutil.copytree(source / "Icon Theme" / "Daemon-Icons", icons, symlinks=True)

    schemes = output / "color-schemes"
    schemes.mkdir(parents=True)
    colours = schemes / "Daemon2.colors"
    shutil.copy2(source / "Color Scheme" / "Daemon2.colors", colours)

    icon_files = patch_action_icons(icons, args.icon_colour.lower())
    states, svg_changes = patch_state_backgrounds(
        kvantum / "daemon-2.0.svg", args.pink.lower(), args.dim_pink.lower()
    )
    kvconfig_changes = rewrite_ini_key(
        kvantum / "daemon-2.0.kvconfig",
        "GeneralColors",
        {"highlight.color", "inactive.highlight.color"},
        args.dim_pink,
    )
    colour_changes = rewrite_ini_key(
        colours,
        "Colors:Selection",
        {"BackgroundNormal"},
        hex_to_kde_rgb(args.dim_pink),
    )

    print(
        f"patched {icon_files} action icons; {svg_changes} backgrounds across "
        f"{states} interactive states; {kvconfig_changes + colour_changes} selection keys"
    )


def strip_jsonc(text: str) -> str:
    """Remove JSONC comments and trailing commas without touching strings."""
    output = []
    index = 0
    in_string = False
    escaped = False
    while index < len(text):
        char = text[index]
        if in_string:
            output.append(char)
            if escaped:
                escaped = False
            elif char == "\\":
                escaped = True
            elif char == '"':
                in_string = False
            index += 1
        elif char == '"':
            in_string = True
            output.append(char)
            index += 1
        elif text.startswith("//", index):
            index = text.find("\n", index)
            if index < 0:
                break
        elif text.startswith("/*", index):
            end = text.find("*/", index + 2)
            index = len(text) if end < 0 else end + 2
        else:
            output.append(char)
            index += 1

    without_comments = "".join(output)
    output = []
    index = 0
    in_string = False
    escaped = False
    while index < len(without_comments):
        char = without_comments[index]
        if in_string:
            output.append(char)
            if escaped:
                escaped = False
            elif char == "\\":
                escaped = True
            elif char == '"':
                in_string = False
            index += 1
            continue
        if char == '"':
            in_string = True
            output.append(char)
            index += 1
            continue
        if char == ",":
            lookahead = index + 1
            while lookahead < len(without_comments) and without_comments[lookahead].isspace():
                lookahead += 1
            if lookahead < len(without_comments) and without_comments[lookahead] in "}]":
                index += 1
                continue
        output.append(char)
        index += 1
    return "".join(output)


def load_jsonc(path: pathlib.Path) -> dict:
    return json.loads(strip_jsonc(path.read_text()))


SYNTAX_KEYS = (
    "tokenColors",
    "semanticTokenColors",
    "semanticHighlighting",
    "encodedTokensColors",
)


def patch_vscode(args: argparse.Namespace) -> None:
    daemon_path = pathlib.Path(args.daemon_theme)
    dracula_path = pathlib.Path(args.dracula_theme)
    output = pathlib.Path(args.out)

    daemon = load_jsonc(daemon_path)
    dracula = load_jsonc(dracula_path)
    if "colors" not in daemon:
        raise RuntimeError("Daemon theme has no workbench colors")
    if "tokenColors" not in dracula:
        raise RuntimeError("Dracula theme has no tokenColors")

    for key in SYNTAX_KEYS:
        if key in dracula:
            daemon[key] = dracula[key]
        else:
            daemon.pop(key, None)

    daemon["name"] = args.name
    output.parent.mkdir(parents=True, exist_ok=True)
    if output.exists():
        make_writable(output)
    output.write_text(json.dumps(daemon, indent=2) + "\n")
    print(
        f"kept {len(daemon['colors'])} Daemon workbench colours and imported "
        f"{len(dracula['tokenColors'])} Dracula syntax rules"
    )


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(description=__doc__)
    commands = result.add_subparsers(dest="command", required=True)

    desktop = commands.add_parser("desktop", help="patch KDE/Qt theme assets")
    desktop.add_argument("--source", required=True)
    desktop.add_argument("--out", required=True)
    desktop.add_argument("--icon-colour", required=True)
    desktop.add_argument("--pink", required=True)
    desktop.add_argument("--dim-pink", required=True)
    desktop.set_defaults(run=patch_desktop)

    vscode = commands.add_parser("vscode", help="combine Daemon UI with Dracula syntax")
    vscode.add_argument("--daemon-theme", required=True)
    vscode.add_argument("--dracula-theme", required=True)
    vscode.add_argument("--out", required=True)
    vscode.add_argument("--name", default="Daemon-2.0")
    vscode.set_defaults(run=patch_vscode)
    return result


def main() -> None:
    args = parser().parse_args()
    args.run(args)


if __name__ == "__main__":
    main()
