.pragma library

function newestFirst(rows, row, limit) {
    return [row].concat(rows.filter(existing => existing.id !== row.id)).slice(0, limit)
}

function archived(row) {
    // Native action objects and notification-provided image buffers stop
    // existing when the notification closes or the shell process restarts.
    return Object.assign({}, row, {
        actions: [],
        icon: row.icon && !row.icon.startsWith("image://") ? row.icon : "",
        closed: true
    })
}

function restore(text, limit) {
    if (!text.trim())
        return { history: [], dnd: false }
    const state = JSON.parse(text)
    if (state.version !== 1 || !Array.isArray(state.history))
        throw new Error("Unsupported notification history format")
    const history = state.history.filter(row =>
        row && typeof row.id === "string"
        && typeof row.app === "string"
        && typeof row.summary === "string"
        && typeof row.body === "string"
    ).slice(0, limit).map(archived)
    return { history: history, dnd: state.dnd === true }
}
