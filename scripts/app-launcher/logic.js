export const ANIMATION_MS = 260;
export const BAR_GAP = 48;
export const PANEL_LEFT = 12;
export const TILE_WIDTH = 66;
export const TILE_HEIGHT = 90;
export const TILE_GAP = 10;
export const GRID_COLUMNS = 4;
export const GRID_ROWS = 2;
export const PANEL_PADDING = 12;
export const SEARCH_HEIGHT = 34;
export const SEARCH_GAP = 10;

export function parseRequest(text) {
    const request = JSON.parse(text);
    if (!request || typeof request !== 'object' || Array.isArray(request)
        || !['toggle', 'close', 'status'].includes(request.command)
        || (request.monitor !== undefined && (typeof request.monitor !== 'string' || !request.monitor.trim()))) {
        throw new Error('Expected toggle [monitor], close, or status.');
    }
    return request;
}

export function panelLayout(width, height) {
    const available = Math.max(1, width - PANEL_LEFT * 2);
    const columns = Math.max(1, Math.min(GRID_COLUMNS,
        Math.floor((available - PANEL_PADDING * 2 + TILE_GAP) / (TILE_WIDTH + TILE_GAP))));
    return {
        columns,
        width: Math.min(available, columns * TILE_WIDTH + (columns - 1) * TILE_GAP + PANEL_PADDING * 2),
        gridHeight: Math.max(1, Math.min(GRID_ROWS * TILE_HEIGHT + (GRID_ROWS - 1) * TILE_GAP,
            height - BAR_GAP - PANEL_PADDING * 2 - SEARCH_HEIGHT - SEARCH_GAP)),
    };
}

export function applicationsFor(apps, query = '') {
    const search = query.trim();
    if (search) return apps.fuzzy_query(search);
    return [...apps.list].sort((a, b) =>
        a.name.localeCompare(b.name) || a.entry.localeCompare(b.entry));
}

export function nextIndex(index, key, columns, count) {
    if (!count) return -1;
    const moves = { Left: -1, Right: 1, Up: -columns, Down: columns, Home: -count, End: count };
    return Math.max(0, Math.min(count - 1, index + (moves[key] || 0)));
}
