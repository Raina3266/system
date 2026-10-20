// Pure Waybar formatting; player selection belongs to shell.qml's activeGroup.
function isCjkIdeograph(code) {
    return (code >= 0x3400 && code <= 0x4dbf)
        || (code >= 0x4e00 && code <= 0x9fff)
        || (code >= 0xf900 && code <= 0xfaff)
        || (code >= 0x20000 && code <= 0x2ebef)
        || (code >= 0x30000 && code <= 0x3134f)
}

function truncateTitle(title) {
    let output = ""
    let characters = 0
    let ideographs = 0
    for (const character of title) {
        const cjk = isCjkIdeograph(character.codePointAt(0))
        if (characters === 50 || (cjk && ideographs === 35))
            return output + "…"
        output += character
        characters++
        if (cjk)
            ideographs++
    }
    return output
}

function badge(group) {
    const icon = "\u{f040a}"
    if (!group)
        return { text: icon, class: "inactive", alt: "inactive", tooltip: "No media playing" }
    const player = group.primary
    const title = (player.trackTitle || "").trim()
        || (player.identity || "").trim() || "Unknown title"
    const artist = (player.trackArtist || "").trim()
    const state = player.isPlaying ? "playing" : "paused"
    return {
        text: icon + "  " + truncateTitle(title),
        class: state,
        alt: state,
        tooltip: title + (artist ? " — " + artist : "")
    }
}
