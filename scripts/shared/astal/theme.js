import GLib from 'gi://GLib';
import { loadPanelStyle } from './live-style.js';

const colour = /^#[a-f\d]{6}$/i;

export function paletteDefinitions(palette, selector, prefix) {
    return `${selector} {\n` + Object.entries(palette)
        .filter(([key, value]) => /^[a-zA-Z]+$/.test(key) && colour.test(value))
        .map(([key, value]) => `    --${prefix}_${key}: ${value};`).join('\n') + '\n}';
}

// Palette values stay fixed until restart; loadPanelStyle owns live CSS reloads.
export function loadPanelTheme({
    entry, palettePath, required, selector, prefix, label, errors = {},
}) {
    const name = label.toLowerCase();
    if (!palettePath) throw new Error(errors.missing
        || `Run through ${name}-panel, or set ${label.toUpperCase()}_PANEL_CONFIG and ${label.toUpperCase()}_PANEL_PALETTE.`);
    const [ok, contents] = GLib.file_get_contents(palettePath);
    if (!ok) throw new Error(errors.read || `Could not read ${name} theme: ${palettePath}`);
    const palette = JSON.parse(new TextDecoder().decode(contents));
    for (const key of required) {
        if (!colour.test(palette?.[key]))
            throw new Error(`${errors.invalid || `Invalid ${name} palette colour`}: ${key}`);
    }
    const definitions = paletteDefinitions(palette, selector, prefix);
    const provider = loadPanelStyle({ entry, definitions, label });
    return { palette, provider };
}
