// Resident Quickshell panels: Waybar toggles the media card and dashboard.
//
// Notifications, MPRIS, PipeWire, power, networking, Bluetooth, and idle
// inhibition use native Quickshell services. Processes are only needed for
// profile-aware media routing and filesystem capacity (`df`).
import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Bluetooth
import Quickshell.Networking
import Quickshell.Services.Mpris
import Quickshell.Services.Pipewire
import Quickshell.Wayland
import "Routing.js" as Routing
import "MediaStatus.js" as MediaStatus

ShellRoot {
    id: root

    // Injected by the systemd service; a bare name keeps terminal runs working.
    readonly property string audioControl: Quickshell.env("AUDIO_CONTROL") || "audio-control"

    // Name of the screen whose panel is open; "" when closed.
    property string openScreen: ""
    property string dashboardOpenScreen: ""

    property var groups: []
    property var activeGroup: null
    property var streamNode: null
    property var streamSink: null
    property var routes: []
    readonly property bool moving: moveProcess.running
    readonly property var notifications: notificationCenter.history
    readonly property bool dndActive: notificationCenter.dnd
    property bool idleInhibitEnabled: false
    readonly property bool airplaneMode: {
        const adapters = Bluetooth.adapters.values
        return !Networking.wifiEnabled && adapters.every(adapter => !adapter.enabled)
    }
    property string routeFailure: ""
    property var routeFailureSource: null
    readonly property string routingError: activeGroup && activeGroup.players.indexOf(routeFailureSource) >= 0 ? routeFailure : ""
    readonly property bool canRoute: activeGroup !== null && Routing.selectorFor(streamNode) !== "" && !moving
    readonly property string routingStatus: {
        if (!activeGroup)
            return ""
        if (moving)
            return "Moving this audio stream…"
        if (!streamNode)
            return activeGroup.routingReason || "No independently routable audio stream."
        if (Routing.selectorFor(streamNode) === "")
            return "Reading audio stream details. Try again in a moment."
        return ""
    }

    // Application/track metadata and native volume need bound nodes. Track only
    // playback streams, and release them when the panel closes.
    PwObjectTracker {
        objects: root.openScreen !== "" ? root.playbackStreams() : []
    }

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
        for (const group of merged) {
            group.primary = group.players.find(player => player.isPlaying) || group.players[0]
            group.displayName = displayNameFor(group.primary)
        }
        // Never assign streams by enumeration order. Refuse ambiguous/shared
        // matches instead of moving audio belonging to another media source.
        const matches = Routing.matchGroups(merged, playbackStreams())
        merged.forEach((group, index) => {
            group.streamNode = matches[index].node
            group.routingReason = matches[index].reason
            const sink = sinkForStream(group.streamNode)
            group.sinkName = sink ? sink.name : ""
        })

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
    // Playback streams are both a stream and a sink; output devices are not
    // streams. The tracker above exposes their native application metadata.
    function playbackStreams() {
        const streams = []
        for (const node of Pipewire.nodes.values) {
            if (node.isStream && node.isSink && node.audio)
                streams.push(node)
        }
        return streams
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
        function onValuesChanged() {
            if (root.openScreen !== "") root.refreshGroups()
            else root.refreshRouting()
        }
    }
    Connections {
        target: Pipewire.linkGroups
        function onValuesChanged() { root.refreshRouting() }
    }

    onActiveGroupChanged: refreshRouting()

    // ---- Route choices ---------------------------------------------------------
    // Canonical JSON device rows keep physical profile keys stable.
    function parseRoutes(text) {
        try {
            return JSON.parse(text).map(row => ({
                key: row.key,
                label: row.label,
                description: row.description,
                name: row.name,
                isDefault: row.default
            }))
        } catch (error) {
            console.warn("media panel: invalid device list:", error)
            return []
        }
    }

    Process {
        id: routesProcess
        command: [root.audioControl, "devices", "output"]
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
        property bool awaitingStart: false
        stderr: StdioCollector { id: moveErrors }
        onStarted: awaitingStart = false
        onRunningChanged: {
            if (!running && awaitingStart) {
                awaitingStart = false
                root.routeFailure = "Could not start audio-control. Rebuild the system or check the routing helper."
                console.warn("media panel:", root.routeFailure)
            }
        }
        onExited: (exitCode, exitStatus) => {
            root.routeFailure = exitCode === 0 && exitStatus === 0 ? ""
                : (moveErrors.text.trim() || "The audio routing helper failed or stopped unexpectedly.")
            if (root.routeFailure !== "")
                console.warn("media panel:", root.routeFailure)
            root.refreshRoutes()
            root.refreshGroups()
        }
    }

    function moveStream(key) {
        if (!activeGroup || moving)
            return
        const requestedPlayer = activeGroup.primary
        // Re-check the selected source immediately before taking its identity.
        refreshGroups()
        routeFailureSource = requestedPlayer
        const selector = Routing.selectorFor(streamNode)
        if (!activeGroup || activeGroup.players.indexOf(requestedPlayer) < 0 || selector === "") {
            routeFailure = "The selected media source has no independently routable audio stream."
            return
        }
        routeFailure = ""
        moveProcess.command = [audioControl, "stream-move", selector, key]
        moveProcess.awaitingStart = true
        moveProcess.running = true
    }

    // ---- Panel state -------------------------------------------------------------

    function togglePanel(monitor) {
        networkCenter.close()
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

    function toggleDashboard(monitor) {
        networkCenter.close()
        let name = monitor
        if (!name && Quickshell.screens.length > 0)
            name = Quickshell.screens[0].name
        if (!name)
            return
        dashboardOpenScreen = dashboardOpenScreen === name ? "" : name
    }

    function closeDashboard() {
        dashboardOpenScreen = ""
    }

    function toggleAirplaneMode() {
        const enableRadios = airplaneMode
        Networking.wifiEnabled = enableRadios
        for (const adapter of Bluetooth.adapters.values)
            adapter.enabled = enableRadios
    }

    function toggleIdleInhibit() {
        idleInhibitEnabled = !idleInhibitEnabled
    }

    function toggleDnd() {
        notificationCenter.toggleDnd()
    }

    function clearNotifications() {
        notificationCenter.clearNotifications()
    }

    function dismissNotification(id) {
        notificationCenter.dismissNotification(id)
    }

    function invokeNotificationAction(id, identifier) {
        notificationCenter.invokeNotificationAction(id, identifier)
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
        // Metadata changes do not change the players model. Refresh on demand
        // for Waybar even when the card (and its sweep timer) is closed.
        function status(): string {
            root.refreshGroups()
            return JSON.stringify(MediaStatus.badge(root.activeGroup))
        }
        function toggle(monitor: string): void { root.togglePanel(monitor) }
        function pauseAll(): void { root.pauseAll() }
        function togglePauseAll(): void { root.togglePauseAll() }
        function cycleMedia(): void { root.cycleMedia() }
    }

    IpcHandler {
        target: "dashboard"
        function toggle(monitor: string): void { root.toggleDashboard(monitor) }
    }

    IpcHandler {
        target: "network"
        function toggle(monitor: string): void {
            root.closePanel()
            root.closeDashboard()
            networkCenter.toggle(monitor)
        }
        function close(): void { networkCenter.close() }
        function status(): string { return networkCenter.status() }
    }

    IpcHandler {
        target: "notifications"
        function toggleDnd(): void { notificationCenter.toggleDnd() }
        function status(): string {
            return JSON.stringify({
                historyCount: notificationCenter.history.length,
                popupCount: notificationCenter.popups.length,
                dnd: notificationCenter.dnd
            })
        }
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

    Notifications {
        id: notificationCenter
    }

    NotificationPopups {
        center: notificationCenter
    }

    NetworkController {
        id: networkCenter
    }

    NetworkPanel {
        controller: networkCenter
    }

    Variants {
        model: Quickshell.screens
        // Variants inject each screen as the delegate's `modelData` property;
        // MediaPanel binds its own window to it.
        MediaPanel {
            panel: root
        }
    }

    Variants {
        model: Quickshell.screens
        DashboardPanel {
            panel: root
        }
    }

    IdleInhibitorWindow {
        panel: root
    }

    // Note: colours come from a Colors instance each visual component creates
    // for itself. Passing root's Colors down breaks: ShellRoot constructs
    // Variants children before its own properties finish initializing, so a
    // forwarded reference reads as undefined and never notifies.
}
