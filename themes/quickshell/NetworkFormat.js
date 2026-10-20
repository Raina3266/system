.pragma library

function signalBars(strength) {
    return Math.ceil(Math.max(0, Math.min(100, strength)) / 25)
}

function signalIcon(strength) {
    return ["󰤯", "󰤟", "󰤢", "󰤥", "󰤨"][signalBars(strength)]
}

function rank(row) {
    return row.connected || row.changing ? 0 : row.known ? 1 : 2
}

function sortRows(rows) {
    return rows.slice().sort((left, right) => {
        if (left.wireless !== right.wireless)
            return left.wireless ? 1 : -1
        return rank(left) - rank(right)
            || (left.wireless ? signalBars(right.strength) - signalBars(left.strength) : 0)
            || left.name.toLowerCase().localeCompare(right.name.toLowerCase())
            || left.interface.localeCompare(right.interface)
    })
}

function primary(rows) {
    return rows.find(row => row.connected && !row.wireless)
        || rows.find(row => row.connected) || null
}

function escapeMarkup(value) {
    return String(value).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;")
        .replace(/"/g, "&quot;").replace(/'/g, "&apos;")
}

function status(rows, details, available) {
    const row = primary(rows)
    const ipv4 = details && details.ipv4
    const loaded = details && Object.prototype.hasOwnProperty.call(details, "ipv4")
    const address = (Array.isArray(ipv4) ? ipv4.join(", ") : ipv4) || (loaded ? "No IPv4" : "Reading IPv4…")
    if (row) {
        return {
            text: "<span size='large'>" + (row.wireless ? signalIcon(row.strength) : "󰈀") + "</span>",
            tooltip: escapeMarkup(row.wireless
                ? "Wi-Fi: " + row.name + "\nSignal: " + row.strength + "%\nSecurity: " + row.security + "\nIPv4: " + address
                : "Ethernet: " + row.interface + "\nIPv4: " + address),
            class: row.wireless ? "wifi" : "ethernet"
        }
    }
    if (rows.some(row => row.changing))
        return { text: "<span size='large'>󰔟</span>", tooltip: "Connecting…", class: "connecting" }
    return {
        text: "<span size='large'>󰤭</span>",
        tooltip: available ? "Network disconnected" : "NetworkManager unavailable or no network devices",
        class: "offline"
    }
}
