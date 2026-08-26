# Apply this configuration's accent overrides to the upstream Daemon KDE MK2
# theme, writing patched copies of the files that carry colour.
#
# Hovered, focused, selected and pressed widgets get an accent outline and a
# dimmed background, the latter derived from the theme's own red so every red
# on screen agrees. The one text colour that changes is the header's, in both
# places KDE defines one: Kvantum's header sections, which are the column
# headers inside an application, and the colour scheme's Header set. All other
# text stays the cyan upstream chose.
#
# Kvantum paints widget frames from an SVG rather than from its config, so the
# states have to be recoloured there as well: every element whose id carries a
# Kvantum status (-focused, which is Kvantum's name for hover and focus,
# -pressed and -toggled) is recoloured along with everything inside it, while
# -normal elements keep the theme's own red frames.
#
# The icon theme is a third place colour lives. Upstream builds it by recolouring
# Breeze's grey to the theme's cyan, so the icons on buttons and toolbars take
# the accent by the same one-colour substitution.
import argparse
import pathlib
import re
import shutil
import xml.etree.ElementTree as ET

# Kvantum statuses that make up "hover, select and focus".
STATES = re.compile(r"-(focused|pressed|toggled)")

NAMESPACES = {
    "": "http://www.w3.org/2000/svg",
    "svg": "http://www.w3.org/2000/svg",
    "inkscape": "http://www.inkscape.org/namespaces/inkscape",
    "sodipodi": "http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd",
    "xlink": "http://www.w3.org/1999/xlink",
}

COLOUR = re.compile(r"(?i)#[0-9a-f]{6}")


def hex_to_rgb(value):
    value = value.lstrip("#")
    return tuple(int(value[i : i + 2], 16) for i in (0, 2, 4))


def recolour_svg(path, mapping):
    """Recolour every element whose id carries a Kvantum status, and its
    children, leaving the -normal elements alone."""
    for prefix, uri in NAMESPACES.items():
        ET.register_namespace(prefix, uri)

    tree = ET.parse(path)
    root = tree.getroot()
    parents = {child: parent for parent in root.iter() for child in parent}

    def within_state(element):
        parent = parents.get(element)
        while parent is not None:
            if STATES.search(parent.get("id", "")):
                return True
            parent = parents.get(parent)
        return False

    def substitute(match):
        return mapping.get(match.group(0).lower(), match.group(0))

    changed = 0
    subtrees = 0
    for element in root.iter():
        if not STATES.search(element.get("id", "")) or within_state(element):
            continue
        subtrees += 1
        for node in element.iter():
            for attribute in ("style", "fill", "stroke"):
                value = node.get(attribute)
                if value is None:
                    continue
                replaced = COLOUR.sub(substitute, value)
                if replaced != value:
                    node.set(attribute, replaced)
                    changed += 1

    tree.write(path, encoding="UTF-8", xml_declaration=True)
    return subtrees, changed


def recolour_icons(directory, mapping):
    """Recolour the icon theme in place. The theme is mostly symbolic links
    pointing at a few thousand real files; only the real ones are rewritten,
    which is what upstream's own recolouring script does too."""
    files = 0
    for path in sorted(directory.rglob("*.svg")):
        if path.is_symlink():
            continue
        text = path.read_text()
        replaced = COLOUR.sub(
            lambda match: mapping.get(match.group(0).lower(), match.group(0)), text
        )
        if replaced != text:
            path.write_text(replaced)
            files += 1
    return files


# Keys naming the background a selection is painted with.
STATE_BACKGROUND = re.compile(r"^(inactive\.)?highlight\.color$")
# The header's text at rest, inside Kvantum's header section.
HEADER_SECTION = "HeaderSection"
HEADER_TEXT = "text.normal.color"


def rewrite_kvconfig(path, accent_dim, header_text):
    """Rewrite the background Kvantum paints behind a selection, and the
    header's own text colour. Every other text colour is left alone."""
    lines = path.read_text().splitlines()
    out = []
    changed = 0
    section = ""
    for line in lines:
        heading = re.match(r"^\[(.+)\]$", line)
        if heading:
            section = heading.group(1)
        match = re.match(r"^([A-Za-z.]+)=(.+)$", line)
        if match:
            key, value = match.group(1), match.group(2).strip()
            new = None
            if STATE_BACKGROUND.match(key):
                new = accent_dim
            elif section == HEADER_SECTION and key == HEADER_TEXT:
                new = header_text
            if new is not None and new != value:
                line = f"{key}={new}"
                changed += 1
        out.append(line)
    path.write_text("\n".join(out) + "\n")
    return changed


def rewrite_colours(path, accent, accent_dim, header_text):
    """Rewrite the KDE colour scheme: the accent for the focus and hover
    decorations, the dimmed red behind a selection, and the header set's text.
    Every other foreground is left as it is."""
    highlight = ",".join(str(part) for part in hex_to_rgb(accent))
    dim = ",".join(str(part) for part in hex_to_rgb(accent_dim))
    header = ",".join(str(part) for part in hex_to_rgb(header_text))

    section = ""
    out = []
    changed = 0
    for line in path.read_text().splitlines():
        heading = re.match(r"^\[(.+)\]$", line)
        if heading:
            section = heading.group(1)
        else:
            match = re.match(r"^([A-Za-z]+)=(.+)$", line)
            if match:
                key, value = match.group(1), match.group(2).strip()
                new = None
                if key in ("DecorationFocus", "DecorationHover"):
                    new = highlight
                elif section == "Colors:Selection" and key == "BackgroundNormal":
                    new = dim
                elif section == "Colors:Header" and key == "ForegroundNormal":
                    new = header
                if new is not None and new != value:
                    line = f"{key}={new}"
                    changed += 1
        out.append(line)
    path.write_text("\n".join(out) + "\n")
    return changed


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", required=True, help="upstream theme checkout")
    parser.add_argument("--out", required=True, help="directory to write into")
    parser.add_argument("--accent", required=True, help="hover, focus and selection colour")
    parser.add_argument("--accent-dim", required=True, help="background behind those states")
    parser.add_argument("--header-text", required=True, help="text in an application's headers")
    parser.add_argument("--accent-dim-strong", required=True, help="background for pressed states")
    parser.add_argument("--replaces", required=True, help="the theme colour being replaced")
    parser.add_argument(
        "--icon-accent-secondary",
        required=True,
        help="second tone, for the icons that use one",
    )
    args = parser.parse_args()

    source = pathlib.Path(args.source)
    out = pathlib.Path(args.out)

    kvantum = out / "Kvantum" / "daemon-2.0"
    kvantum.mkdir(parents=True)
    for name in ("daemon-2.0.svg", "daemon-2.0.kvconfig"):
        (kvantum / name).write_bytes((source / "Kvantum" / "daemon-2.0" / name).read_bytes())

    icons = out / "icons" / "Daemon-Icons"
    shutil.copytree(source / "Icon Theme" / "Daemon-Icons", icons, symlinks=True)
    # Everything arrives read-only from the store; make what we rewrite writable.
    for path in icons.rglob("*"):
        if not path.is_symlink():
            path.chmod(0o755 if path.is_dir() else 0o644)

    schemes = out / "color-schemes"
    schemes.mkdir(parents=True)
    colours = schemes / "Daemon2.colors"
    colours.write_bytes((source / "Color Scheme" / "Daemon2.colors").read_bytes())

    # The dim interiors the theme uses behind its state frames, mapped onto
    # dimmed accent so a hovered or selected widget reads as one colour.
    mapping = {
        args.replaces.lower(): args.accent.lower(),
        "#ff5048": args.accent.lower(),  # red frames, in state elements only
        "#fb3048": args.accent.lower(),
        "#331319": args.accent_dim.lower(),
        "#272932": args.accent_dim.lower(),
        "#710100": args.accent_dim_strong.lower(),
    }

    subtrees, colours_changed = recolour_svg(kvantum / "daemon-2.0.svg", mapping)
    icon_files = recolour_icons(
        icons,
        {
            args.replaces.lower(): args.accent.lower(),
            "#5df2ff": args.accent.lower(),
            "#5beedc": args.icon_accent_secondary.lower(),
        },
    )
    keys = rewrite_kvconfig(
        kvantum / "daemon-2.0.kvconfig", args.accent_dim, args.header_text
    )
    scheme_keys = rewrite_colours(
        colours, args.accent, args.accent_dim, args.header_text
    )

    print(
        f"recoloured {colours_changed} colours across {subtrees} Kvantum state elements, "
        f"{keys} Kvantum config keys, {scheme_keys} colour scheme keys, {icon_files} icons"
    )


if __name__ == "__main__":
    main()
