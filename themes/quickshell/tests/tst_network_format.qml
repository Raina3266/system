import QtQuick
import QtTest
import "../NetworkFormat.js" as Format

TestCase {
    name: "NetworkFormat"

    function row(name, properties) {
        return Object.assign({
            name: name, interface: "wlan0", wireless: true,
            connected: false, changing: false, known: false,
            strength: 80, security: "WPA2"
        }, properties || {})
    }

    function test_signalBuckets() {
        compare(Format.signalBars(0), 0)
        compare(Format.signalBars(25), 1)
        compare(Format.signalBars(26), 2)
        compare(Format.signalBars(50), 2)
        compare(Format.signalBars(51), 3)
        compare(Format.signalBars(75), 3)
        compare(Format.signalBars(76), 4)
        compare(Format.signalIcon(77), Format.signalIcon(94))
    }

    function test_stablePriorityAndStrength() {
        const rows = [
            row("Z strong", { strength: 100 }),
            row("Saved", { known: true, strength: 10 }),
            row("Active", { connected: true }),
            row("B", { strength: 77 }),
            row("a", { strength: 94 })
        ]
        compare(Format.sortRows(rows).map(row => row.name), ["Active", "Saved", "a", "B", "Z strong"])
    }

    function test_statusPriorityAndEscaping() {
        const wifi = row("<Guest & friends>", { connected: true })
        const wired = row("Wired", { interface: "eth0", wireless: false, connected: true })
        compare(Format.status([wifi, wired], { ipv4: "192.0.2.2/24" }, true).class, "ethernet")
        const status = Format.status([wifi], { ipv4: "" }, true)
        compare(status.class, "wifi")
        verify(status.tooltip.indexOf("&lt;Guest &amp; friends&gt;") !== -1)
        verify(status.tooltip.indexOf("No IPv4") !== -1)
        compare(Format.status([row("Working", { changing: true })], {}, true).class, "connecting")
        compare(Format.status([], {}, false).class, "offline")
    }
}
