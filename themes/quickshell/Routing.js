.pragma library

// Matching is deliberately independent of enumeration order. Evidence narrows
// application-scoped candidates; a node is usable only if no other source
// claims it. Do not resolve collisions by consuming nodes in a loop.
function text(value) {
    return value === undefined || value === null ? "" : String(value).trim()
}

function canonical(value) {
    var name = text(value).toLowerCase().replace(/\.desktop$/, "")
    var aliases = {
        "google chrome": "chromium", "google-chrome": "chromium",
        "google-chrome-stable": "chromium", "chrome": "chromium",
        "org.chromium.chromium": "chromium", "org.google.chrome": "chromium",
        "mozilla firefox": "firefox", "org.mozilla.firefox": "firefox"
    }
    return aliases[name] || name
}

function browser(name) {
    return ["chromium", "firefox", "brave", "brave-browser", "vivaldi",
            "opera", "microsoft-edge", "microsoft edge"].indexOf(name) >= 0
}

function add(values, value) {
    var key = canonical(value)
    if (key && values.indexOf(key) < 0)
        values.push(key)
}

function source(group) {
    var players = (group.players || []).slice()
    if (group.primary)
        players.push(group.primary)
    var keys = [], titles = [], urls = []
    players.forEach(function(player) {
        var metadata = player.metadata || {}
        add(keys, player.identity)
        add(keys, player.desktopEntry)
        // Boolean/presence-only bridge metadata is not browser identity.
        var hint = canonical(metadata["mprisence:browser"])
        if (browser(hint)) {
            keys = keys.filter(function(key) { return !browser(key) })
            add(keys, hint)
        }
        var title = text(player.trackTitle || metadata["xesam:title"])
        if (title)
            titles.push(title)
        var url = text(metadata["xesam:url"])
        if (url)
            urls.push(url)
    })
    if (text(group.title))
        titles.push(text(group.title))
    return { keys: keys, titles: titles, urls: urls, browser: keys.some(browser) }
}

function nodeKeys(node) {
    var properties = node.properties || {}
    var keys = []
    ;[node.name, node.nickname, properties["application.name"],
      properties["application.id"], properties["application.process.binary"]]
        .forEach(function(value) { add(keys, value) })
    return keys
}

function selectorFor(node) {
    if (!node || node.ready !== true)
        return ""
    var serial = text((node.properties || {})["object.serial"])
    if (/^[0-9]+$/.test(serial) && /[1-9]/.test(serial))
        return "serial:" + serial
    // Node IDs are reusable. Never send a panel command that could resolve to
    // a replacement stream after the selected one disappears.
    return ""
}

function matchGroups(groups, streams) {
    var sources = groups.map(source)
    // Deduplicate service references, including wrappers with the same node ID.
    var nodes = []
    streams.forEach(function(node) {
        if (node && !nodes.some(function(other) {
            return other === node || (Number(node.id) > 0 && other.id === node.id)
        }))
            nodes.push(node)
    })
    var candidates = [], reasons = [], detailedMatches = [], claims = []
    sources.forEach(function(src, index) {
        var scoped = nodes.filter(function(node) {
            return nodeKeys(node).some(function(key) { return src.keys.indexOf(key) >= 0 })
        })
        var ready = scoped.filter(function(node) { return selectorFor(node) !== "" })
        var detailed = ready.filter(function(node) {
            var props = node.properties || {}
            return (src.titles.indexOf(text(props["media.name"])) >= 0)
                || (src.titles.indexOf(text(props["media.title"])) >= 0)
                || (src.urls.indexOf(text(props["media.filename"])) >= 0)
                || (src.urls.indexOf(text(props["media.url"])) >= 0)
        })
        detailedMatches[index] = detailed.length
        candidates[index] = detailed.length ? detailed : ready
        claims[index] = candidates[index].slice()
        reasons[index] = !scoped.length ? "No matching audio stream. Start playback in this source."
            : !ready.length ? (scoped.some(function(node) { return !node.ready })
                ? "Reading audio stream details. Try again in a moment."
                : "No stable audio stream identity is available for independent routing.")
            : "Application playback stream"
    })
    // Playback state does not prove ownership: paused streams can linger.
    // Multiple browser sources need track evidence, and their original claims
    // still participate in conflict detection even when routing is refused.
    sources.forEach(function(src, index) {
        if (src.browser && detailedMatches[index] !== 1 && sources.some(function(other, otherIndex) {
            return otherIndex !== index && other.browser && other.keys.some(function(key) {
                return browser(key) && src.keys.indexOf(key) >= 0
            })
        })) {
            candidates[index] = []
            reasons[index] = "Several sources share this browser; a distinct track-specific audio stream is required."
        }
    })
    return candidates.map(function(options, index) {
        if (!options.length)
            return { node: null, reason: reasons[index] }
        if (options.length !== 1)
            return { node: null, reason: "Multiple audio streams match this source; independent routing is unavailable." }
        var node = options[0]
        if (claims.some(function(other, otherIndex) {
            return otherIndex !== index && other.indexOf(node) >= 0
        }))
            return { node: null, reason: "Shared or ambiguous audio stream; independent routing is unavailable." }
        return { node: node, reason: "Unique application playback stream" }
    })
}
