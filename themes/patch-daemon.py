#!/usr/bin/env python3
"""Build the local Daemon 2.0 desktop, GTK and VS Code themes.

``desktop`` changes Dolphin-facing icons plus normal and interactive backgrounds.
``gtk`` rebuilds upstream's complete Breeze-based GTK theme with the same
Daemon palette, including every GTK 2/3/4 image and symbolic asset.
``vscode`` applies the same UI accents to Daemon's workbench and imports
Dracula's syntax rules.
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
STATE_ELEMENT = re.compile(r"-(focused|pressed|toggled)(?:-|$)")
CHECKED_CONTROL = re.compile(
    r"^(?:checkbox-(?:checked|tristate)|radio-checked)-"
    r"(?:normal|focused|pressed|toggled)$"
)
CYAN_ICON_COLOURS = {"#5df4fe", "#5df2ff", "#5beedc", "#5aeedc"}
STATE_BASE_COLOURS = {"#272932", "#1e1e1e", "#14101f", "#331319"}
STATE_OUTLINE_COLOURS = CYAN_ICON_COLOURS | {"#ff5048", "#fb3048"}
UPSTREAM_BACKGROUND_COLOURS = {
    "#210e15": "main",
    "#14101f": "secondary",
    "#130f1e": "secondary",
    "#200d14": "chrome",
}

DAEMON_TEXT = "#5df4fe"
DAEMON_MUTED_TEXT = "#7a9b9f"
DAEMON_WARNING = "#fdf500"
DAEMON_ERROR = "#ff5048"
DAEMON_SUCCESS = "#28c775"

KDE_STRUCTURAL_ELEMENTS = {
    "common",
    "group",
    "header",
    "itemview",
    "lineedit",
    "menu",
    "menubaritem",
    "menuitem",
    "scrollbargroove",
    "scrollbarslider",
    "splitter",
    "ss",
    "tabframe",
    "tbutton",
    "toolbar",
    "tooltip",
}
KDE_STRUCTURAL_REDS = {"#710100", "#fb3048", "#ff5048"}
KDE_ALWAYS_YELLOW_ELEMENTS = {"scrollbargroove", "scrollbarslider", "tbutton"}

GTK_ICON_ASSET = re.compile(
    r"(?:arrow|bullet|check|close|dash|maximize|minimize|radio|slider|spinbutton|titlebutton)"
)
GTK_STOCK_SURFACES = {
    (49, 54, 59): "secondary",  # #31363b
    (42, 46, 50): "main",       # #2a2e32
    (48, 53, 58): "secondary",  # #30353a
    (44, 49, 54): "secondary",  # #2c3136
    (44, 49, 53): "secondary",  # #2c3135
    (27, 30, 32): "alternate",  # #1b1e20
}
GTK_STOCK_ACCENTS = {
    (61, 174, 233),  # Breeze blue
    (37, 164, 230),
}
GTK_STOCK_FOREGROUNDS = {
    (252, 252, 252),
    (253, 253, 253),
    (250, 250, 250),
    (255, 255, 255),
}
GTK_STOCK_MUTED_FOREGROUNDS = {
    (161, 169, 177),
    (147, 149, 151),
    (159, 170, 176),
    (159, 172, 179),
}
GTK_STOCK_BORDERS = {
    (78, 82, 86),
    (94, 97, 100),
    (68, 71, 76),
    (76, 79, 82),
}


def make_writable(path: pathlib.Path) -> None:
    path.chmod(path.stat().st_mode | stat.S_IWUSR)


def replace_colours(text: str, mapping: dict[str, str]) -> str:
    return HEX_COLOUR.sub(
        lambda match: mapping.get(match.group(0).lower(), match.group(0)), text
    )


def rewrite_text_colours(path: pathlib.Path, mapping: dict[str, str]) -> int:
    """Replace exact colour tokens in a text asset, ignoring letter case."""
    text = path.read_text()
    changed = 0
    for source, target in mapping.items():
        pattern = re.escape(source)
        if not source.startswith("#"):
            pattern = rf"(?<![0-9]){pattern}(?![0-9])"
        text, replacements = re.subn(
            pattern, target, text, flags=re.IGNORECASE
        )
        changed += replacements

    if changed:
        make_writable(path)
        path.write_text(text)
    return changed


def background_palette(args: argparse.Namespace) -> dict[str, str]:
    targets = {
        "main": args.main_background.lower(),
        "secondary": args.secondary_background.lower(),
        "chrome": args.chrome_background.lower(),
    }
    return {
        upstream: targets[layer]
        for upstream, layer in UPSTREAM_BACKGROUND_COLOURS.items()
    }


def hex_rgb(value: str) -> tuple[int, int, int]:
    value = value.removeprefix("#")
    if len(value) != 6:
        raise ValueError(f"expected #RRGGBB colour, got {value!r}")
    return tuple(int(value[index : index + 2], 16) for index in (0, 2, 4))


def css_rgba(value: str, alpha: float) -> str:
    red, green, blue = hex_rgb(value)
    return f"rgba({red}, {green}, {blue}, {alpha:g})"


def blend(foreground: str, background: str, ratio: float) -> tuple[int, int, int]:
    front = hex_rgb(foreground)
    back = hex_rgb(background)
    return tuple(
        round(front[index] * ratio + back[index] * (1 - ratio))
        for index in range(3)
    )


def gtk_named_palette(args: argparse.Namespace) -> dict[str, str]:
    """Return semantic GTK colours matching the patched Kvantum theme."""
    text = DAEMON_TEXT
    muted = DAEMON_MUTED_TEXT
    main = args.main_background.lower()
    alternate = args.alternate_background.lower()
    secondary = args.secondary_background.lower()
    chrome = args.chrome_background.lower()
    red = args.icon_colour.lower()
    pink = args.pink.lower()
    selected = args.dim_pink.lower()

    result: dict[str, str] = {}

    def assign(names: tuple[str, ...], value: str) -> None:
        result.update(dict.fromkeys(names, value))

    assign(
        (
            "theme_fg_color_breeze",
            "theme_text_color_breeze",
            "theme_selected_fg_color_breeze",
            "theme_unfocused_fg_color_breeze",
            "theme_unfocused_text_color_breeze",
            "theme_unfocused_selected_fg_color_breeze",
            "theme_button_foreground_normal_breeze",
            "theme_button_foreground_active_breeze",
            "theme_button_foreground_backdrop_breeze",
            "theme_button_foreground_active_backdrop_breeze",
            "theme_titlebar_foreground_breeze",
            "tooltip_text_breeze",
        ),
        text,
    )
    assign(
        (
            "theme_bg_color_breeze",
            "theme_unfocused_bg_color_breeze",
            "theme_titlebar_background_light_breeze",
            "theme_titlebar_background_backdrop_breeze",
        ),
        main,
    )
    assign(
        (
            "theme_base_color_breeze",
            "theme_unfocused_base_color_breeze",
            "content_view_bg_breeze",
        ),
        alternate,
    )
    assign(
        (
            "theme_button_background_normal_breeze",
            "theme_button_background_backdrop_breeze",
            "tooltip_background_breeze",
        ),
        secondary,
    )
    assign(("theme_titlebar_background_breeze",), chrome)
    assign(
        (
            "theme_hovering_selected_bg_color_breeze",
            "theme_selected_bg_color_breeze",
            "theme_unfocused_selected_bg_color_alt_breeze",
            "theme_unfocused_selected_bg_color_breeze",
        ),
        selected,
    )
    assign(
        (
            "theme_view_hover_decoration_color_breeze",
            "theme_view_active_decoration_color_breeze",
            "theme_button_decoration_hover_breeze",
            "theme_button_decoration_focus_breeze",
            "theme_button_decoration_hover_backdrop_breeze",
            "theme_button_decoration_focus_backdrop_breeze",
        ),
        pink,
    )
    assign(
        (
            "borders_breeze",
            "unfocused_borders_breeze",
            "tooltip_border_breeze",
        ),
        red,
    )
    assign(
        (
            "insensitive_selected_bg_color_breeze",
            "insensitive_unfocused_selected_bg_color_breeze",
        ),
        css_rgba(selected, 0.35),
    )
    assign(
        (
            "insensitive_bg_color_breeze",
            "insensitive_unfocused_bg_color_breeze",
        ),
        chrome,
    )
    assign(
        (
            "insensitive_fg_color_breeze",
            "insensitive_selected_fg_color_breeze",
            "insensitive_unfocused_fg_color_breeze",
            "insensitive_unfocused_selected_fg_color_breeze",
            "theme_unfocused_view_text_color_breeze",
            "theme_button_foreground_insensitive_breeze",
            "theme_button_foreground_active_insensitive_breeze",
            "theme_button_foreground_backdrop_insensitive_breeze",
            "theme_button_foreground_active_backdrop_insensitive_breeze",
            "theme_titlebar_foreground_insensitive_breeze",
        ),
        css_rgba(text, 0.35),
    )
    assign(
        (
            "insensitive_base_color_breeze",
            "insensitive_base_fg_color_breeze",
            "theme_unfocused_view_bg_color_breeze",
        ),
        secondary,
    )
    assign(
        (
            "insensitive_borders_breeze",
            "unfocused_insensitive_borders_breeze",
        ),
        css_rgba(red, 0.35),
    )
    assign(
        (
            "theme_button_background_insensitive_breeze",
            "theme_button_background_backdrop_insensitive_breeze",
        ),
        css_rgba(secondary, 0.6),
    )
    assign(
        (
            "theme_button_decoration_hover_insensitive_breeze",
            "theme_button_decoration_focus_insensitive_breeze",
            "theme_button_decoration_hover_backdrop_insensitive_breeze",
            "theme_button_decoration_focus_backdrop_insensitive_breeze",
        ),
        css_rgba(pink, 0.35),
    )
    assign(("theme_titlebar_foreground_backdrop_breeze",), muted)
    assign(
        ("theme_titlebar_foreground_insensitive_backdrop_breeze",),
        css_rgba(muted, 0.35),
    )
    assign(("warning_color_breeze", "warning_color_backdrop_breeze"), DAEMON_WARNING)
    assign(("error_color_breeze", "error_color_backdrop_breeze"), DAEMON_ERROR)
    assign(("success_color_breeze", "success_color_backdrop_breeze"), DAEMON_SUCCESS)
    assign(
        ("warning_color_insensitive_breeze", "warning_color_insensitive_backdrop_breeze"),
        css_rgba(DAEMON_WARNING, 0.35),
    )
    assign(
        ("error_color_insensitive_breeze", "error_color_insensitive_backdrop_breeze"),
        css_rgba(DAEMON_ERROR, 0.35),
    )
    assign(
        ("success_color_insensitive_breeze", "success_color_insensitive_backdrop_breeze"),
        css_rgba(DAEMON_SUCCESS, 0.35),
    )
    assign(("link_color_breeze",), text)
    assign(("link_visited_color_breeze",), pink)
    return result


def rewrite_gtk_named_colours(path: pathlib.Path, palette: dict[str, str]) -> int:
    pattern = re.compile(r"(@define-color\s+([^\s]+)\s+)([^;]+)(;)")
    text = path.read_text()
    changed = 0

    def replace(match: re.Match[str]) -> str:
        nonlocal changed
        replacement = palette.get(match.group(2))
        if replacement is None or replacement == match.group(3).strip():
            return match.group(0)
        changed += 1
        return f"{match.group(1)}{replacement}{match.group(4)}"

    rewritten = pattern.sub(replace, text)
    if changed:
        make_writable(path)
        path.write_text(rewritten)
    return changed


def gtk_direct_palette(args: argparse.Namespace) -> dict[str, str]:
    """Patch literal colours outside GTK's semantic named-colour header."""
    return {
        "#fcfcfc": DAEMON_TEXT,
        "#2a2e32": args.main_background.lower(),
        "#1b1e20": args.alternate_background.lower(),
        "#181b1d": args.secondary_background.lower(),
        "#191b1d": args.secondary_background.lower(),
        "#262a2d": args.chrome_background.lower(),
        "#31363b": args.secondary_background.lower(),
        "#3daee9": args.pink.lower(),
        "#54575a": args.icon_colour.lower(),
        "#5a5e62": args.icon_colour.lower(),
        "#5e6164": args.icon_colour.lower(),
        "#616364": DAEMON_MUTED_TEXT,
        "#6b6e70": DAEMON_MUTED_TEXT,
        "#707376": DAEMON_MUTED_TEXT,
        "#a1a9b1": DAEMON_MUTED_TEXT,
        "#da4453": DAEMON_ERROR,
        "#f67400": DAEMON_WARNING,
        "#27ae60": DAEMON_SUCCESS,
        "#1d99f3": DAEMON_TEXT,
        "#9b59b6": args.pink.lower(),
    }


def recolour_gtk_svg(path: pathlib.Path, args: argparse.Namespace) -> bool:
    mapping = gtk_direct_palette(args)
    mapping.update(
        {
            "#4e5256": args.icon_colour.lower(),
            "#939597": DAEMON_MUTED_TEXT,
            "#ff667c": DAEMON_ERROR,
        }
    )
    if GTK_ICON_ASSET.search(path.stem.lower()):
        mapping["#fcfcfc"] = args.icon_colour.lower()
        mapping["#a1a9b1"] = DAEMON_MUTED_TEXT
        mapping["#3daee9"] = (
            args.pink.lower()
            if any(state in path.stem.lower() for state in ("hover", "active", "focus"))
            else args.icon_colour.lower()
        )

    original = path.read_text()
    rewritten = replace_colours(original, mapping)
    if rewritten == original:
        return False
    make_writable(path)
    path.write_text(rewritten)
    return True


def recolour_gtk_png(path: pathlib.Path, args: argparse.Namespace) -> bool:
    # Pillow is deliberately imported only by this subcommand. The desktop and
    # VS Code builders do not need it.
    from PIL import Image

    image = Image.open(path).convert("RGBA")
    pixel_data = (
        image.get_flattened_data()
        if hasattr(image, "get_flattened_data")
        else image.getdata()
    )
    pixels = list(pixel_data)
    targets = {
        "main": hex_rgb(args.main_background),
        "alternate": hex_rgb(args.alternate_background),
        "secondary": hex_rgb(args.secondary_background),
    }
    icon_red = hex_rgb(args.icon_colour)
    pink = hex_rgb(args.pink)
    muted_red = blend(args.icon_colour, args.main_background, 0.5)
    accent = (
        pink
        if any(state in path.stem.lower() for state in ("hover", "active", "focus", "selected"))
        else icon_red
    )
    icon_asset = GTK_ICON_ASSET.search(path.stem.lower()) is not None
    changed = False
    output = []

    for red, green, blue, alpha in pixels:
        rgb = (red, green, blue)
        replacement: tuple[int, int, int] | None = None
        surface = GTK_STOCK_SURFACES.get(rgb)
        if surface is not None:
            replacement = targets[surface]
        elif rgb in GTK_STOCK_ACCENTS:
            replacement = accent
        elif rgb in GTK_STOCK_FOREGROUNDS:
            replacement = icon_red if icon_asset else hex_rgb(DAEMON_TEXT)
        elif rgb in GTK_STOCK_MUTED_FOREGROUNDS:
            replacement = muted_red if icon_asset else hex_rgb(DAEMON_MUTED_TEXT)
        elif rgb in GTK_STOCK_BORDERS:
            replacement = muted_red
        elif rgb in {(218, 68, 83), (255, 102, 124)}:
            replacement = hex_rgb(DAEMON_ERROR)
        elif alpha > 0 and max(rgb) - min(rgb) <= 18:
            # Breeze contains many anti-aliased one-off greys around the exact
            # colours above. Fold those residual pixels into Daemon's palette
            # too, while retaining near-black shadow pixels as neutral black.
            luminance = sum(rgb) / 3
            if luminance >= 145:
                replacement = icon_red if icon_asset else hex_rgb(DAEMON_TEXT)
            elif luminance >= 65:
                replacement = muted_red if icon_asset else hex_rgb(DAEMON_MUTED_TEXT)
            elif luminance >= 48:
                replacement = muted_red
            elif luminance >= 32:
                replacement = targets["secondary"]
            elif luminance >= 20:
                replacement = targets["main"]

        if replacement is not None and replacement != rgb:
            red, green, blue = replacement
            changed = True
        output.append((red, green, blue, alpha))

    if changed:
        make_writable(path)
        image.putdata(output)
        image.save(path)
    return changed


def append_gtk_overrides(path: pathlib.Path, args: argparse.Namespace) -> None:
    """Make symbolic icons and keyboard focus follow the Daemon state roles."""
    with path.open("a") as stylesheet:
        stylesheet.write(
            "\n/* Local Daemon 2.0 state and symbolic-icon accents. */\n"
            f"@define-color daemon_icon_red {args.icon_colour.lower()};\n"
            f"@define-color daemon_pink {args.pink.lower()};\n"
            f"@define-color daemon_dim_pink {args.dim_pink.lower()};\n"
            "image:not(:disabled), button image:not(:disabled), "
            "headerbar image:not(:disabled), entry image:not(:disabled) {\n"
            "  color: @daemon_icon_red;\n"
            "}\n"
            "button:focus, entry:focus, row:focus, check:focus, radio:focus {\n"
            "  outline-color: @daemon_pink;\n"
            "}\n"
            "headerbar button.titlebutton, .titlebar button.titlebutton,\n"
            "headerbar windowcontrols button, .titlebar windowcontrols button {\n"
            "  min-width: 18px;\n"
            "  min-height: 18px;\n"
            "  margin: 0 2px;\n"
            "  padding: 6px;\n"
            "  border-color: transparent;\n"
            "  background-color: transparent;\n"
            "  box-shadow: none;\n"
            "  outline: none;\n"
            "}\n"
            "headerbar button.titlebutton:hover, .titlebar button.titlebutton:hover,\n"
            "headerbar windowcontrols button:hover, .titlebar windowcontrols button:hover {\n"
            "  border-color: transparent;\n"
            "  background-color: @daemon_dim_pink;\n"
            "}\n"
        )


def patch_gtk(args: argparse.Namespace) -> None:
    source = pathlib.Path(args.source)
    output = pathlib.Path(args.out)
    # Do not copy the upstream settings file in the first place. copytree
    # preserves the Nix store's read-only directory mode, so copying and then
    # unlinking it would fail even if the file itself were made writable.
    # Cursor selection is already managed by Home Manager, and the file also
    # requests an otherwise unused colour-reload module.
    shutil.copytree(
        source,
        output,
        symlinks=True,
        ignore=shutil.ignore_patterns("settings.ini"),
    )

    named_palette = gtk_named_palette(args)
    named_changes = 0
    literal_changes = 0
    for relative in ("gtk-3.0/gtk.css", "gtk-4.0/gtk.css"):
        css = output / relative
        named_changes += rewrite_gtk_named_colours(css, named_palette)
        literal_changes += rewrite_text_colours(css, gtk_direct_palette(args))
        append_gtk_overrides(css, args)

    gtk2_changes = 0
    for path in sorted((output / "gtk-2.0").rglob("*")):
        if path.is_file():
            gtk2_changes += rewrite_text_colours(path, gtk_direct_palette(args))

    svg_changes = sum(
        recolour_gtk_svg(path, args)
        for path in sorted((output / "assets").glob("*.svg"))
    )
    png_changes = sum(
        recolour_gtk_png(path, args)
        for path in sorted((output / "assets").glob("*.png"))
    )

    if named_changes < 60:
        raise RuntimeError(
            f"only {named_changes} GTK semantic colours changed; upstream layout may have changed"
        )
    if png_changes == 0 or svg_changes == 0:
        raise RuntimeError("GTK image assets did not contain the expected Breeze colours")

    print(
        f"built Daemon GTK theme: {named_changes} semantic and "
        f"{literal_changes + gtk2_changes} literal colour changes; "
        f"recoloured {png_changes} PNG and {svg_changes} SVG assets"
    )


def patch_dolphin_icons(directory: pathlib.Path, icon_colour: str) -> int:
    """Recolour Dolphin action, place/sidebar and MIME/file icons."""
    changed = 0
    mapping = {colour: icon_colour for colour in CYAN_ICON_COLOURS}
    for relative in (
        "scalable/actions",
        "symbolic/actions",
        "scalable/places",
        "symbolic/places",
        "scalable/mimes",
        "symbolic/mimes",
    ):
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
        raise RuntimeError(
            "no Daemon action/place/MIME icons contained the expected cyan colours"
        )
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


def set_stroke(node: ET.Element, colour: str) -> bool:
    style = parse_style(node.get("style", ""))
    if "stroke" in style:
        if style["stroke"].lower() in ("none", colour.lower()):
            return False
        style["stroke"] = colour
        node.set("style", format_style(style))
        return True

    stroke = node.get("stroke")
    if stroke and stroke.lower() not in ("none", colour.lower()):
        node.set("stroke", colour)
        return True
    return False


def stroke_colour(node: ET.Element) -> str | None:
    style = parse_style(node.get("style", ""))
    return style.get("stroke", node.get("stroke"))


def patch_state_backgrounds(
    path: pathlib.Path,
    state_fill_colour: str,
    outline_colour: str,
    dim_pink: str,
    normal_input_background: str,
    indicator_colour: str,
) -> tuple[int, int, int, int]:
    """Patch every background slice of interactive Kvantum states.

    Kvantum draws a single row/button from a centre plus edge and corner SVG
    elements, such as ``itemview-focused`` and ``itemview-focused-left``. All
    of their translucent cyan background layers need to change together or a
    selected row remains cyan around a pink centre. Translucent interaction
    layers retain the pink state fill (rendering as dim pink), while only
    opaque frame geometry and strokes use the configured outline colour.
    Focused line edits keep their normal input background so selected text is
    still distinguishable; only their outline changes colour.
    """
    for prefix, uri in SVG_NAMESPACES.items():
        ET.register_namespace(prefix, uri)

    tree = ET.parse(path)
    root = tree.getroot()
    parents = {child: parent for parent in root.iter() for child in parent}

    def inside_state(node: ET.Element) -> bool:
        parent = parents.get(node)
        while parent is not None:
            if STATE_ELEMENT.search(parent.get("id", "")):
                return True
            parent = parents.get(parent)
        return False

    states = [
        node
        for node in root.iter()
        if STATE_ELEMENT.search(node.get("id", "")) and not inside_state(node)
    ]
    changed = 0
    outline_changes = 0

    for state in states:
        state_id = state.get("id", "")
        state_background = (
            normal_input_background
            if state_id == "lineedit-focused"
            or state_id.startswith("lineedit-focused-")
            else dim_pink
        )
        nodes = list(nodes_with_opacity(state))
        for node, opacity in nodes:
            fill = fill_colour(node)
            if fill:
                lowered = fill.lower()
                if lowered in STATE_BASE_COLOURS:
                    changed += set_fill(node, state_background)
                elif lowered in STATE_OUTLINE_COLOURS:
                    if opacity >= 1 and len(node) == 0:
                        outline_changes += set_fill(node, outline_colour)
                    else:
                        changed += set_fill(node, state_fill_colour)

            stroke = stroke_colour(node)
            if stroke and stroke.lower() in STATE_OUTLINE_COLOURS:
                outline_changes += set_stroke(node, outline_colour)

    # Checked/radio glyphs are button indicators, not backgrounds or frames.
    # Keep them on Daemon red as well, independently of the outline colour.
    indicator_changes = 0
    indicator_colours = CYAN_ICON_COLOURS | {
        state_fill_colour.lower(),
        outline_colour.lower(),
    }
    for control in root.iter():
        if not CHECKED_CONTROL.match(control.get("id", "")):
            continue
        for node, opacity in nodes_with_opacity(control):
            fill = fill_colour(node)
            if fill and fill.lower() in indicator_colours and opacity >= 1:
                changed_here = set_fill(node, indicator_colour)
                if changed_here or fill.lower() == indicator_colour.lower():
                    indicator_changes += 1

    if not states or changed == 0:
        raise RuntimeError("no Kvantum interactive-state backgrounds were changed")
    if outline_changes == 0:
        raise RuntimeError("no Kvantum interactive-state outlines were changed")
    if indicator_changes == 0:
        raise RuntimeError("no Kvantum checked-control indicators were changed")

    make_writable(path)
    tree.write(path, encoding="UTF-8", xml_declaration=True)
    return len(states), changed, outline_changes, indicator_changes


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


def rewrite_ini_keys_in_sections(
    path: pathlib.Path, section_prefix: str, keys: set[str], value: str
) -> int:
    section = ""
    changed = 0
    output = []
    for line in path.read_text().splitlines():
        heading = re.match(r"^\[(.+)]$", line)
        if heading:
            section = heading.group(1)
        elif section.startswith(section_prefix):
            setting = re.match(r"^([^=]+)=(.*)$", line)
            if setting and setting.group(1) in keys and setting.group(2).strip() != value:
                line = f"{setting.group(1)}={value}"
                changed += 1
        output.append(line)

    if changed == 0:
        raise RuntimeError(f"no {sorted(keys)} keys changed in {path}")
    make_writable(path)
    path.write_text("\n".join(output) + "\n")
    return changed


def set_ini_key(path: pathlib.Path, section_name: str, key: str, value: str) -> int:
    """Set an INI key, inserting it at the end of an existing section if absent."""
    lines = path.read_text().splitlines()
    section = ""
    section_found = False
    insert_at = None

    for index, line in enumerate(lines):
        heading = re.match(r"^\[(.+)]$", line)
        if heading:
            if section == section_name and insert_at is None:
                insert_at = index
            section = heading.group(1)
            section_found = section_found or section == section_name
            continue

        if section == section_name:
            setting = re.match(r"^([^=]+)=(.*)$", line)
            if setting and setting.group(1) == key:
                replacement = f"{key}={value}"
                if line == replacement:
                    return 0
                lines[index] = replacement
                make_writable(path)
                path.write_text("\n".join(lines) + "\n")
                return 1

    if not section_found:
        raise RuntimeError(f"section [{section_name}] was not found in {path}")

    if insert_at is None:
        insert_at = len(lines)
    lines.insert(insert_at, f"{key}={value}")
    make_writable(path)
    path.write_text("\n".join(lines) + "\n")
    return 1


def patch_kde_structural_accents(
    path: pathlib.Path, colour: str, interactive_colour: str
) -> int:
    """Patch structural reds plus the explicitly requested popup outline."""
    tree = ET.parse(path)
    root = tree.getroot()
    changed = 0

    for element in root.iter():
        element_id = element.get("id", "")
        prefix = element_id.split("-", 1)[0]
        if prefix not in KDE_STRUCTURAL_ELEMENTS:
            continue
        if (
            STATE_ELEMENT.search(element_id)
            and prefix not in KDE_ALWAYS_YELLOW_ELEMENTS
        ):
            # Hovered, focused and selected widget frames stay red. Scrollbars
            # and tool buttons are exceptions because their entire visuals are
            # explicitly assigned to the yellow structure role.
            continue
        source_colours = KDE_STRUCTURAL_REDS
        if prefix in KDE_ALWAYS_YELLOW_ELEMENTS:
            source_colours = source_colours | {interactive_colour.lower()}
        if element_id == "menu-normal" or element_id.startswith("menu-normal-"):
            # The menu frame is cyan upstream rather than red, but it is the
            # outer popup outline and belongs to the same yellow structure role.
            source_colours = source_colours | CYAN_ICON_COLOURS

        for node, opacity in nodes_with_opacity(element):
            if prefix == "tbutton" and (len(node) > 0 or opacity < 1):
                # A tool-button state also contains inherited, transparent and
                # translucent interaction fills. Only its opaque leaf geometry
                # draws the requested yellow box outline.
                continue
            style = parse_style(node.get("style", ""))
            style_changed = False
            for key in ("fill", "stroke"):
                if style.get(key, "").lower() in source_colours:
                    style[key] = colour
                    changed += 1
                    style_changed = True
            if style_changed:
                node.set("style", format_style(style))

            for key in ("fill", "stroke"):
                if node.get(key, "").lower() in source_colours:
                    node.set(key, colour)
                    changed += 1

    if changed == 0:
        raise RuntimeError("no red KDE structural accents were changed")

    make_writable(path)
    tree.write(path, encoding="UTF-8", xml_declaration=True)
    return changed


def add_kvantum_menu_separator(path: pathlib.Path, colour: str) -> int:
    """Add the SVG element Kvantum expects for QMenu separators."""
    tree = ET.parse(path)
    root = tree.getroot()
    if any(node.get("id") == "menuitem-separator" for node in root.iter()):
        raise RuntimeError("Kvantum SVG already contains menuitem-separator")

    namespace = f"{{{SVG_NAMESPACES['']}}}"
    group = ET.SubElement(root, f"{namespace}g", {"id": "menuitem-separator"})
    ET.SubElement(
        group,
        f"{namespace}rect",
        {
            "x": "0",
            "y": "0",
            "width": "40",
            "height": "10",
            "style": f"opacity:0;fill:{colour}",
        },
    )
    ET.SubElement(
        group,
        f"{namespace}rect",
        {
            "x": "0",
            "y": "4.5",
            "width": "40",
            "height": "1",
            "style": f"fill:{colour}",
        },
    )

    make_writable(path)
    tree.write(path, encoding="UTF-8", xml_declaration=True)
    return 1


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

    icon_files = patch_dolphin_icons(icons, args.icon_colour.lower())
    states, svg_changes, outline_changes, indicator_changes = patch_state_backgrounds(
        kvantum / "daemon-2.0.svg",
        args.pink.lower(),
        args.icon_colour.lower(),
        args.dim_pink.lower(),
        args.secondary_background.lower(),
        args.icon_colour.lower(),
    )
    structural_changes = patch_kde_structural_accents(
        kvantum / "daemon-2.0.svg",
        args.structure_colour.lower(),
        args.icon_colour.lower(),
    )
    separator_changes = add_kvantum_menu_separator(
        kvantum / "daemon-2.0.svg", args.structure_colour.lower()
    )
    kvconfig_changes = rewrite_ini_key(
        kvantum / "daemon-2.0.kvconfig",
        "GeneralColors",
        {"highlight.color", "inactive.highlight.color"},
        args.dim_pink,
    )
    structural_config_changes = rewrite_ini_key(
        kvantum / "daemon-2.0.kvconfig",
        "GeneralColors",
        {"light.color", "mid.light.color", "mid.color"},
        args.structure_colour,
    )
    separator_config_changes = set_ini_key(
        kvantum / "daemon-2.0.kvconfig",
        "%General",
        "menu_separator_height",
        "7",
    )
    colour_changes = rewrite_ini_key(
        colours,
        "Colors:Selection",
        {"BackgroundNormal"},
        hex_to_kde_rgb(args.dim_pink),
    )
    decoration_changes = rewrite_ini_keys_in_sections(
        colours,
        "Colors:",
        {"DecorationFocus", "DecorationHover"},
        hex_to_kde_rgb(args.icon_colour),
    )
    palette = background_palette(args)
    svg_background_changes = rewrite_text_colours(
        kvantum / "daemon-2.0.svg", palette
    )
    kvconfig_background_changes = rewrite_text_colours(
        kvantum / "daemon-2.0.kvconfig", palette
    )
    kde_palette = {
        hex_to_kde_rgb(source): hex_to_kde_rgb(target)
        for source, target in palette.items()
    }
    colour_background_changes = rewrite_text_colours(colours, kde_palette)
    palette_changes = (
        svg_background_changes
        + kvconfig_background_changes
        + colour_background_changes
    )
    if palette_changes == 0:
        raise RuntimeError("no desktop background colours were changed")
    # Apply this after the broad palette conversion: the requested alternate
    # colour is also Daemon's original main background, which the conversion
    # above intentionally maps to the darker normal background everywhere else.
    alternate_background_changes = rewrite_ini_key(
        colours,
        "Colors:View",
        {"BackgroundAlternate"},
        hex_to_kde_rgb(args.alternate_background),
    )

    print(
        f"patched {icon_files} Dolphin action/place/MIME icons and "
        f"{indicator_changes} control indicators; "
        f"{svg_changes} backgrounds and {outline_changes + decoration_changes} outlines "
        f"across {states} interactive state elements; "
        f"{structural_changes + structural_config_changes} yellow structural accents; "
        f"{separator_changes + separator_config_changes} menu separator settings; "
        f"{kvconfig_changes + colour_changes} selection keys; "
        f"{alternate_background_changes} alternate-view background; "
        f"{palette_changes} normal backgrounds darkened"
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

VSCODE_STATE_BACKGROUND_KEYS = {
    "activityBar.activeBackground",
    "activityBarTop.activeBackground",
    "button.hoverBackground",
    "button.secondaryHoverBackground",
    "checkbox.selectBackground",
    "commandCenter.activeBackground",
    "editor.inactiveSelectionBackground",
    "editor.selectionBackground",
    "editor.selectionHighlightBackground",
    "editor.wordHighlightBackground",
    "editor.wordHighlightStrongBackground",
    "editorActionList.focusBackground",
    "editorStickyScrollHover.background",
    "editorSuggestWidget.selectedBackground",
    "extensionButton.hoverBackground",
    "extensionButton.prominentHoverBackground",
    "inputOption.activeBackground",
    "inputOption.hoverBackground",
    "list.activeSelectionBackground",
    "list.focusBackground",
    "list.hoverBackground",
    "list.inactiveFocusBackground",
    "list.inactiveSelectionBackground",
    "menu.selectionBackground",
    "menubar.selectionBackground",
    "minimapSlider.activeBackground",
    "minimapSlider.hoverBackground",
    "notebook.focusedCellBackground",
    "notebook.selectedCellBackground",
    "peekViewResult.selectionBackground",
    "quickInputList.focusBackground",
    "radio.activeBackground",
    "scrollbarSlider.activeBackground",
    "scrollbarSlider.hoverBackground",
    "settings.focusedRowBackground",
    "settings.rowHoverBackground",
    "statusBarItem.activeBackground",
    "statusBarItem.focusHoverBackground",
    "statusBarItem.hoverBackground",
    "statusBarItem.remoteHoverBackground",
    "tab.activeBackground",
    "tab.hoverBackground",
    "terminal.hoverHighlightBackground",
    "terminal.inactiveSelectionBackground",
    "terminal.selectionBackground",
    "toolbar.activeBackground",
    "toolbar.hoverBackground",
    "welcomePage.tileHoverBackground",
}

VSCODE_ICON_FOREGROUND_KEYS = {
    "activityBar.foreground",
    "activityBar.inactiveForeground",
    "activityBarTop.foreground",
    "activityBarTop.inactiveForeground",
    "checkbox.foreground",
    "icon.foreground",
    "list.activeSelectionIconForeground",
    "list.inactiveSelectionIconForeground",
    "quickInputList.focusIconForeground",
    "radio.activeForeground",
}

VSCODE_STATE_OUTLINE_KEYS = {
    "activityBar.activeBorder",
    "activityBarTop.activeBorder",
    "button.border",
    "checkbox.border",
    "commandCenter.activeBorder",
    "contrastActiveBorder",
    "focusBorder",
    "inputOption.activeBorder",
    "list.focusAndSelectionOutline",
    "list.focusOutline",
    "list.inactiveFocusOutline",
    "menu.selectionBorder",
    "menubar.selectionBorder",
    "panelTitle.activeBorder",
    "radio.activeBorder",
    "sash.hoverBorder",
    "settings.focusedRowBorder",
    "statusBarItem.focusBorder",
    "tab.activeBorder",
    "tab.activeBorderTop",
}


def patch_vscode_workbench(
    colours: dict[str, str], icon_colour: str, pink: str, dim_pink: str
) -> tuple[int, int, int]:
    """Apply Daemon's local interaction and icon accents to VS Code."""
    background_changes = 0
    for key in VSCODE_STATE_BACKGROUND_KEYS:
        if colours.get(key, "").lower() != dim_pink.lower():
            colours[key] = dim_pink
            background_changes += 1

    icon_changes = 0
    for key in VSCODE_ICON_FOREGROUND_KEYS:
        if colours.get(key, "").lower() != icon_colour.lower():
            colours[key] = icon_colour
            icon_changes += 1

    outline_changes = 0
    for key in VSCODE_STATE_OUTLINE_KEYS:
        if colours.get(key, "").lower() != pink.lower():
            colours[key] = pink
            outline_changes += 1

    return background_changes, icon_changes, outline_changes


def patch_vscode_backgrounds(
    colours: dict[str, str], palette: dict[str, str]
) -> int:
    """Darken normal workbench backgrounds without touching foregrounds."""
    changed = 0
    for key, value in colours.items():
        if "background" not in key.lower() or not isinstance(value, str):
            continue
        replacement = palette.get(value.lower())
        if replacement and replacement.lower() != value.lower():
            colours[key] = replacement
            changed += 1

    if changed == 0:
        raise RuntimeError("no VS Code normal backgrounds were changed")
    return changed


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

    normal_background_changes = patch_vscode_backgrounds(
        daemon["colors"], background_palette(args)
    )
    (
        state_background_changes,
        icon_changes,
        outline_changes,
    ) = patch_vscode_workbench(
        daemon["colors"], args.icon_colour, args.pink, args.dim_pink
    )

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
        f"darkened {normal_background_changes} normal VS Code backgrounds and "
        f"patched {state_background_changes} state backgrounds plus "
        f"{outline_changes} outlines and {icon_changes} icon colours; "
        f"kept Daemon's remaining workbench colours "
        f"and imported "
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
    desktop.add_argument("--structure-colour", required=True)
    desktop.add_argument("--alternate-background", required=True)
    desktop.add_argument("--main-background", required=True)
    desktop.add_argument("--secondary-background", required=True)
    desktop.add_argument("--chrome-background", required=True)
    desktop.set_defaults(run=patch_desktop)

    gtk = commands.add_parser("gtk", help="build the complete Daemon GTK theme")
    gtk.add_argument("--source", required=True)
    gtk.add_argument("--out", required=True)
    gtk.add_argument("--icon-colour", required=True)
    gtk.add_argument("--pink", required=True)
    gtk.add_argument("--dim-pink", required=True)
    gtk.add_argument("--alternate-background", required=True)
    gtk.add_argument("--main-background", required=True)
    gtk.add_argument("--secondary-background", required=True)
    gtk.add_argument("--chrome-background", required=True)
    gtk.set_defaults(run=patch_gtk)

    vscode = commands.add_parser("vscode", help="combine Daemon UI with Dracula syntax")
    vscode.add_argument("--daemon-theme", required=True)
    vscode.add_argument("--dracula-theme", required=True)
    vscode.add_argument("--out", required=True)
    vscode.add_argument("--name", default="Daemon-2.0")
    vscode.add_argument("--icon-colour", required=True)
    vscode.add_argument("--pink", required=True)
    vscode.add_argument("--dim-pink", required=True)
    vscode.add_argument("--main-background", required=True)
    vscode.add_argument("--secondary-background", required=True)
    vscode.add_argument("--chrome-background", required=True)
    vscode.set_defaults(run=patch_vscode)
    return result


def main() -> None:
    args = parser().parse_args()
    args.run(args)


if __name__ == "__main__":
    main()
