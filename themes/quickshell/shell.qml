// Quickshell media panel: the popup Waybar's centre button toggles.
//
// Everything live is a native Quickshell service binding (Mpris, Pipewire);
// the only spawned processes are one-shot user actions through audio-control:
// `wayle-list output` for the route choices (it alone knows inactive-profile
// Speaker/Headphones rows) and `stream-move` for the profile-aware move.
import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Services.Mpris
import Quickshell.Services.Pipewire

ShellRoot {
    id: root

    // Injected by the systemd service; a bare name keeps terminal runs working.
    readonly property string audioControl: Quickshell.env("AUDIO_CONTROL") || "audio-control"

    // Name of the screen whose panel is open; "" when closed.
    property string openScreen: ""

    property var groups: []
    property var activeGroup: null
    property var streamNode: null
    property var streamSink: null
    property var routes: []
    readonly property bool moving: moveProcess.running

    // ---- Player grouping -----------------------------------------------------
    // Stopped players, playerctld and kdeconnect proxies are dropped; duplicate
    // publishers of one playback (mprisence bridge + real player) merge by
    // metadata and position, like media-panel did.

    // Browser tabs are only sources on real media pages: mprisence publishes
    // every tab that merely contains a video element (docs pages, GitHub, and
    // site homepages, which keep reporting their last track), so browser
    // players are filtered by page URL — host, then media-path prefix. "*"
    // means any non-root path. Everything else passes.
    readonly property var allowedMediaPaths: ({
        "youtu.be": ["*"],
        "youtube.com": ["watch", "shorts", "live", "embed", "playlist", "v", "e"],
        "music.youtube.com": ["watch", "playlist"],
        "bilibili.com": ["video", "bangumi", "cheese", "medialist", "list", "festival", "audio"],
        "b23.tv": ["*"],
        "live.bilibili.com": ["*"],
        "spotify.com": ["track", "album", "playlist", "artist", "show", "episode", "collection"]
    })

    // What the user recognises: the site for browser tabs (mprisence names
    // them all "Chromium"), the player's own identity for everything else.
    function displayNameFor(player) {
        if (!isBrowserPlayer(player))
            return player.identity
        const host = (((player.metadata || {})["xesam:url"] || "").toLowerCase().split("/"))[2] || ""
        if (host.endsWith("music.youtube.com"))
            return "YouTube Music"
        if (host.endsWith("youtube.com") || host.endsWith("youtu.be"))
            return "YouTube"
        if (host.endsWith("live.bilibili.com"))
            return "Bilibili Live"
        if (host.endsWith("bilibili.com") || host.endsWith("b23.tv"))
            return "Bilibili"
        if (host.endsWith("spotify.com"))
            return "Spotify"
        return player.identity
    }

    function isBrowserPlayer(player) {
        const meta = player.metadata || {}
        if (meta["mprisence:browser"] !== undefined)
            return true
        const identity = player.identity.toLowerCase()
        return ["chrom", "firefox", "edge", "brave", "vivaldi", "opera"].some(name => identity.indexOf(name) !== -1)
    }

    // YouTube appends image-selection parameters to MPRIS artwork URLs. Those
    // parameters can make i.ytimg.com return AVIF data at a .jpg URL, which
    // this Qt image plugin set cannot decode; the path alone serves JPEG.
    function artworkUrlFor(player) {
        let url = player && player.trackArtUrl ? String(player.trackArtUrl) : ""
        if (url.startsWith("https://i.ytimg.com/") || url.startsWith("http://i.ytimg.com/"))
            url = url.split("?")[0]
        return url
    }

    function browserMediaAllowed(player) {
        const url = ((player.metadata || {})["xesam:url"] || "").toLowerCase()
        const host = url.split("/")[2] || ""
        if (host === "")
            return false
        // Path segments below the host, query stripped; Spotify's optional
        // /intl-xx/ language segment is skipped before the type check.
        let segments = url.split("/").slice(3).join("/").split("?")[0].split("/").filter(part => part !== "")
        if (segments.length > 0 && segments[0].indexOf("intl-") === 0)
            segments = segments.slice(1)
        // Most specific host wins: music.youtube.com must not fall through to
        // youtube.com's rules.
        const domains = Object.keys(allowedMediaPaths).sort((a, b) => b.length - a.length)
        for (const domain of domains) {
            if (host === domain || host.endsWith("." + domain)) {
                const patterns = allowedMediaPaths[domain]
                if (patterns[0] === "*")
                    return segments.length > 0
                return segments.length > 0 && patterns.indexOf(segments[0]) !== -1
            }
        }
        return false
    }

    function refreshGroups() {
        const merged = []
        for (const player of Mpris.players.values) {
            if (player.playbackState === MprisPlaybackState.Stopped)
                continue
            const bus = player.dbusName.toLowerCase()
            if (bus.indexOf("playerctld") !== -1 || bus.indexOf("kdeconnect") !== -1)
                continue
            if (isBrowserPlayer(player) && !browserMediaAllowed(player))
                continue
            let group = null
            for (const candidate of merged) {
                if (candidate.title === player.trackTitle && candidate.artist === player.trackArtist) {
                    for (const member of candidate.players) {
                        if (Math.abs(member.position - player.position) < 3) {
                            group = candidate
                            break
                        }
                    }
                }
                if (group)
                    break
            }
            if (group)
                group.players.push(player)
            else
                merged.push({ title: player.trackTitle, artist: player.trackArtist, players: [player] })
        }
        // Stream matching, two passes: players whose PipeWire node carries
        // their name match directly; then, since a browser's stream never
        // carries the tab's identity, a lone browser group gets the lone
        // unclaimed stream. Two browsers playing at once stay unmatched
        // rather than risking a wrong route.
        const streams = playbackStreams()
        const claimed = {}
        for (const group of merged) {
            group.primary = group.players.find(player => player.isPlaying) || group.players[0]
            group.displayName = displayNameFor(group.primary)
            group.streamNode = namedStreamFor(group.primary, streams)
            if (group.streamNode)
                claimed[group.streamNode.id] = true
        }
        const leftover = streams.filter(node => !claimed[node.id])
        const browsers = merged.filter(group => !group.streamNode && isBrowserPlayer(group.primary))
        // Pair leftovers with browser groups, playing first: a playing tab has
        // a live stream, a paused one usually has none, so leftovers most
        // likely belong to the playing sources. More browsers than streams
        // leaves the rest unmatched rather than risking a wrong route.
        const playing = browsers.filter(group => group.primary.isPlaying)
        const candidates = playing.concat(browsers.filter(group => !group.primary.isPlaying))
        for (let index = 0; index < Math.min(candidates.length, leftover.length); index++)
            candidates[index].streamNode = leftover[index]
        for (const group of merged) {
            const sink = sinkForStream(group.streamNode || null)
            group.sinkName = sink ? sink.name : ""
        }

        const previous = activeGroup
        groups = merged
        if (previous) {
            for (const group of merged) {
                if (group.players.indexOf(previous.primary) !== -1) {
                    activeGroup = group
                    return
                }
            }
        }
        activeGroup = merged.find(group => group.primary.isPlaying) || merged[0] || null
    }

    Connections {
        target: Mpris.players
        function onValuesChanged() { root.refreshGroups() }
    }

    // ---- Stream / sink matching ----------------------------------------------
    // Quickshell binds nodes lazily: `ready` stays false and `properties`
    // empty until something subscribes, so matching uses node.name/nickname,
    // which are always populated. Playback streams are the nodes that are
    // both a stream and a sink; devices are never streams.
    function playbackStreams() {
        const streams = []
        for (const node of Pipewire.nodes.values) {
            if (node.isStream && node.isSink && node.audio)
                streams.push(node)
        }
        return streams
    }

    function namedStreamFor(player, streams) {
        if (!player)
            return null
        const identity = player.identity.toLowerCase()
        const entry = (player.desktopEntry || "").toLowerCase()
        for (const node of streams) {
            const name = (node.name || "").toLowerCase()
            const nickname = (node.nickname || "").toLowerCase()
            if (name !== "" && (name === identity || (entry !== "" && name.indexOf(entry) !== -1)))
                return node
            if (nickname !== "" && nickname === identity)
                return node
        }
        return null
    }

    function sinkForStream(node) {
        if (!node)
            return null
        // Compare by object id, not instance: the nodes model and the link
        // group model may wrap one PipeWire node in different QObjects.
        for (const group of Pipewire.linkGroups.values) {
            if (group.source && group.source.id === node.id)
                return group.target
        }
        return null
    }

    function refreshRouting() {
        streamNode = activeGroup ? (activeGroup.streamNode || null) : null
        streamSink = sinkForStream(streamNode)
    }

    Connections {
        target: Pipewire.nodes
        function onValuesChanged() { root.refreshRouting() }
    }
    Connections {
        target: Pipewire.linkGroups
        function onValuesChanged() { root.refreshRouting() }
    }

    // Waybar's media title follows Wayle's active player, so a selection here
    // is mirrored there and the bar shows the same source as the card.
    Process {
        id: activePlayerProcess
    }

    onActiveGroupChanged: {
        refreshRouting()
        if (activeGroup) {
            activePlayerProcess.command = ["busctl", "--user", "call", "com.wayle.Media1", "/com/wayle/Media", "com.wayle.Media1", "SetActivePlayer", "s", activeGroup.primary.dbusName]
            activePlayerProcess.running = true
        }
    }

    // ---- Route choices ---------------------------------------------------------
    // `wayle-list output` rows: key, label, description, live node name, default.
    function parseRoutes(text) {
        const fields = text.split("\0")
        const rows = []
        for (let index = 0; index + 4 < fields.length && fields[index] !== ""; index += 5) {
            rows.push({
                key: fields[index],
                label: fields[index + 1],
                description: fields[index + 2],
                name: fields[index + 3],
                isDefault: fields[index + 4] === "1"
            })
        }
        return rows
    }

    Process {
        id: routesProcess
        command: [root.audioControl, "wayle-list", "output"]
        stdout: StdioCollector {
            onStreamFinished: root.routes = root.parseRoutes(this.text)
        }
    }

    function refreshRoutes() {
        if (!routesProcess.running)
            routesProcess.running = true
    }

    Process {
        id: moveProcess
        onExited: (exitCode, exitStatus) => {
            if (exitCode !== 0)
                console.warn("media panel: stream-move exited with", exitCode)
            root.refreshRoutes()
        }
    }

    function moveStream(key) {
        if (!activeGroup)
            return
        // The matched stream's node name ("Google Chrome") resolves where the
        // MPRIS identity ("Chromium") cannot; the id form is exact even for
        // two tabs of one browser once the helper learns object ids.
        const selector = streamNode && streamNode.name !== "" ? streamNode.name : activeGroup.primary.identity
        moveProcess.command = [audioControl, "stream-move", selector, key]
        moveProcess.running = true
    }

    // ---- Panel state -------------------------------------------------------------

    function togglePanel(monitor) {
        let name = monitor
        if (!name && Quickshell.screens.length > 0)
            name = Quickshell.screens[0].name
        if (!name)
            return
        openScreen = openScreen === name ? "" : name
    }

    function closePanel() {
        openScreen = ""
    }

    onOpenScreenChanged: {
        if (openScreen !== "") {
            refreshGroups()
            refreshRoutes()
        }
    }

    // Deliberately not playerctl --all-players: media-panel skipped CanPause
    // for a reason, but browser bridges that cannot reach their tab are a rare
    // edge; every ordinary player answers pause(). Proxies stay excluded so a
    // bridge is not asked twice through its own player.
    function pauseAll() {
        for (const player of Mpris.players.values) {
            const bus = player.dbusName.toLowerCase()
            if (bus.indexOf("playerctld") !== -1 || bus.indexOf("kdeconnect") !== -1)
                continue
            if (player.isPlaying)
                player.pause()
        }
    }

    // Middle-click pauses everything that is playing. When nothing is
    // playing, it resumes the currently selected source instead.
    function togglePauseAll() {
        const anyPlaying = Mpris.players.values.some(player => {
            const bus = player.dbusName.toLowerCase()
            return bus.indexOf("playerctld") === -1
                && bus.indexOf("kdeconnect") === -1
                && player.isPlaying
        })
        if (anyPlaying) {
            pauseAll()
            return
        }

        refreshGroups()
        if (activeGroup) {
            for (const player of activeGroup.players)
                player.isPlaying = true
        }
    }

    // Cycle through the same ordered, merged source groups shown in the panel.
    function cycleMedia() {
        refreshGroups()
        if (groups.length === 0)
            return
        const current = groups.indexOf(activeGroup)
        activeGroup = groups[(current + 1) % groups.length]
    }

    IpcHandler {
        target: "media"
        function toggle(monitor: string): void { root.togglePanel(monitor) }
        function pauseAll(): void { root.pauseAll() }
        function togglePauseAll(): void { root.togglePauseAll() }
        function cycleMedia(): void { root.cycleMedia() }
    }

    // Track changes are not model changes, so a slow sweep keeps grouping right
    // while open. MPRIS pushes no position updates; nudge at 2 Hz while playing.
    Timer {
        interval: 2000
        repeat: true
        running: root.openScreen !== ""
        onTriggered: { root.refreshGroups(); root.refreshRouting() }
    }
    // Frame-driven nudge while playing: the seek bar glides instead of
    // stepping twice a second.
    FrameAnimation {
        running: root.openScreen !== "" && root.activeGroup && root.activeGroup.primary.isPlaying
        onTriggered: {
            for (const player of root.activeGroup.players)
                player.positionChanged()
        }
    }

    Variants {
        model: Quickshell.screens
        // Variants injects each screen as the delegate's `modelData` property;
        // MediaPanel binds its own window to it.
        MediaPanel {
            panel: root
        }
    }

    // Note: colours come from a Colors instance each visual component creates
    // for itself. Passing root's Colors down breaks: ShellRoot constructs
    // Variants children before its own properties finish initializing, so a
    // forwarded reference reads as undefined and never notifies.
}
