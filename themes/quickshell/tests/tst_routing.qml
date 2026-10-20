import QtQuick
import QtTest
import "../Routing.js" as Routing

TestCase {
    name: "ConservativeRouting"

    function group(app, title, playing) {
        return { primary: { identity: app, isPlaying: playing !== false,
                            trackTitle: title || "", metadata: {} },
                 players: [], title: title || "" }
    }
    function node(id, app, title) {
        return { id: id, name: app, nickname: "", ready: true,
                 properties: { "application.name": app, "media.name": title || "",
                               "object.serial": String(1000 + id) } }
    }
    function test_distinctAppsAndOrder() {
        var groups = [group("Spotify", "Same"), group("Firefox", "Same")]
        var nodes = [node(1, "Spotify", "Same"), node(2, "Firefox", "Same")]
        for (var g = 0; g < 2; ++g) {
            for (var n = 0; n < 2; ++n) {
                var result = Routing.matchGroups(groups, nodes)
                compare(result[0].node.name, groups[0].primary.identity)
                compare(result[1].node.name, groups[1].primary.identity)
                nodes.reverse()
            }
            groups.reverse()
        }
        compare(Routing.matchGroups([group("Other", "Same")], nodes)[0].node, null)
    }
    function test_titlesAndUrls() {
        var groups = [group("Chromium", "A"), group("Chromium", "B")]
        var nodes = [node(1, "Chromium", "A"), node(2, "Chromium", "B")]
        compare(Routing.matchGroups(groups, nodes)[0].node.id, 1)
        nodes.reverse()
        compare(Routing.matchGroups(groups, nodes)[1].node.id, 2)
        groups[0].primary.metadata["xesam:url"] = "https://example/media"
        nodes[1].properties["media.name"] = "Unknown"
        nodes[1].properties["media.url"] = "https://example/media"
        compare(Routing.matchGroups(groups, nodes)[0].node.id, 1)
        delete nodes[1].properties["media.url"]
        nodes[1].properties["media.title"] = "A"
        compare(Routing.matchGroups(groups, nodes)[0].node.id, 1)
    }
    function test_sharedAndDuplicate() {
        var groups = [group("Chromium"), group("Chromium")]
        var nodes = [node(1, "Chromium")]
        var result = Routing.matchGroups(groups, nodes)
        compare(result[0].node, null)
        compare(result[1].node, null)
        verify(result[0].reason.toLowerCase().indexOf("share") >= 0)
        nodes.push(node(2, "Chromium"))
        compare(Routing.matchGroups([groups[0]], nodes)[0].node, null)
        compare(Routing.matchGroups([group("Spotify"), group("Spotify")],
                                    [node(3, "Spotify")])[0].node, null)
    }
    function test_activePausedAndAliases() {
        var groups = [group("Google Chrome", "", false), group("Chromium")]
        var nodes = [node(1, "chrome")]
        var result = Routing.matchGroups(groups, nodes)
        compare(result[0].node, null)
        compare(result[1].node, null)
        groups.reverse()
        compare(Routing.matchGroups(groups, nodes)[0].node, null)
        compare(Routing.matchGroups(groups, [node(2, "Spotify")])[0].node, null)
        compare(Routing.matchGroups([group("Chrome")],
                                    [node(1, "Chrome"), node(2, "Chromium")])[0].node, null)
    }
    function test_missingAndUnready() {
        verify(Routing.matchGroups([group("Spotify")], [])[0].reason.length > 0)
        var stream = node(1, "Spotify")
        stream.ready = false
        var result = Routing.matchGroups([group("Spotify")], [stream])[0]
        compare(result.node, null)
        verify(result.reason.indexOf("Reading") >= 0)
        compare(Routing.matchGroups([group("Spot")], [node(1, "Spotify")])[0].node, null)
    }
    function test_nativeKeysAndBrowserHint() {
        var source = group("Chromium")
        source.primary.metadata["mprisence:browser"] = "Firefox"
        var firefox = node(1, "unrelated-node")
        firefox.properties["application.process.binary"] = "firefox"
        var chrome = node(2, "Chromium")
        var before = JSON.stringify([source, firefox, chrome])
        compare(Routing.matchGroups([source], [chrome, firefox])[0].node.id, 1)
        compare(JSON.stringify([source, firefox, chrome]), before)
        source.primary.metadata["mprisence:browser"] = true
        compare(Routing.matchGroups([source], [firefox])[0].node, null)
        source = group("Unknown")
        source.primary.desktopEntry = "org.example.Player.desktop"
        firefox.properties["application.id"] = "org.example.Player"
        compare(Routing.matchGroups([source], [firefox])[0].node.id, 1)
    }
    function test_selectors() {
        var stream = node(42, "Spotify")
        compare(Routing.selectorFor(stream), "serial:1042")
        stream.properties["object.serial"] = "42"
        compare(Routing.selectorFor(stream), "serial:42")
        stream.properties["object.serial"] = "0"
        compare(Routing.selectorFor(stream), "")
        stream.properties["object.serial"] = "000"
        compare(Routing.selectorFor(stream), "")
        stream.ready = false
        compare(Routing.selectorFor(stream), "")
        compare(Routing.selectorFor(null), "")
        stream.ready = true
        stream.properties = {}
        var invalid = [0, -1, 1.2, "bad", null]
        for (var i = 0; i < invalid.length; ++i) {
            stream.id = invalid[i]
            compare(Routing.selectorFor(stream), "")
        }
        stream.id = 42
        compare(Routing.selectorFor(stream), "")
    }

    function test_pausedWithExplicitTrackEvidence() {
        var groups = [group("Chromium", "Paused track", false), group("Chromium", "Playing track")]
        var nodes = [node(1, "chrome", "Paused track"), node(2, "chrome", "Playing track")]
        var result = Routing.matchGroups(groups, nodes)
        compare(result[0].node.id, 1)
        compare(result[1].node.id, 2)
        nodes.reverse()
        groups.reverse()
        result = Routing.matchGroups(groups, nodes)
        compare(result[0].node.id, 2)
        compare(result[1].node.id, 1)
    }

    function test_sharedStreamEvenWithOneMatchingTitle() {
        var groups = [group("Chromium", "A"), group("Chromium", "B")]
        var nodes = [node(1, "chrome", "A")]
        for (var i = 0; i < 2; ++i) {
            var result = Routing.matchGroups(groups, nodes)
            compare(result[0].node, null)
            compare(result[1].node, null)
            groups.reverse()
        }
    }

    function test_pausedStreamCannotBeClaimedByNewPlayingSource() {
        var groups = [group("Chromium", "Old track", false), group("Chromium", "New track")]
        var nodes = [node(1, "chrome", "Old track")]
        compare(Routing.matchGroups(groups, nodes)[1].node, null)
        var fresh = node(2, "chrome", "New track")
        fresh.ready = false
        nodes.push(fresh)
        compare(Routing.matchGroups(groups, nodes)[1].node, null)
        fresh.ready = true
        var result = Routing.matchGroups(groups, nodes)
        compare(result[0].node.id, 1)
        compare(result[1].node.id, 2)
    }
}
