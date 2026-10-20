import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Services.Mpris
import Quickshell.Services.Pipewire

// Native service/JSON bridge only. Rust owns source policy, grouping, stream
// ownership and action plans; this component never guesses which stream to move.
Scope {
    id: controller

    required property string helper
    property string routingHelper: "audio-control"
    property bool trackNativeObjects: true
    property bool opened: false
    // Injectable native models let isolated QML tests avoid desktop mutations.
    property var mpris: Mpris
    property var pipewire: Pipewire
    property var groups: []
    property var activeGroup: null
    property var routes: []
    property string failure: ""
    property string routeFailure: ""
    property string routeFailurePlayer: ""
    property int selectionSerial: 0
    property int requestSerial: 0
    property var inFlight: null
    property var queuedActions: []
    property bool refreshPending: true
    property bool workerReady: false
    property bool movePending: false

    readonly property var streamNode: activeGroup ? activeGroup.streamNode : null
    readonly property var streamSink: sinkForStream(streamNode)
    readonly property bool moving: movePending || moveProcess.running
    readonly property bool canRoute: workerReady && activeGroup !== null
        && activeGroup.selector !== "" && !moving
    readonly property string routingError: failure || (activeGroup
        && activeGroup.players.some(player => player.dbusName === routeFailurePlayer) ? routeFailure : "")
    readonly property string routingStatus: moving ? "Moving this audio stream…"
        : activeGroup && !streamNode ? activeGroup.routingReason : ""

    PwObjectTracker {
        objects: controller.opened && controller.trackNativeObjects ? controller.playbackStreams() : []
    }

    function playerFor(bus) {
        return mpris.players.values.find(player => player.dbusName === bus) || null
    }

    function playbackStreams() {
        return pipewire.nodes.values.filter(node => node.isStream && node.isSink && node.audio)
    }

    function sinkForStream(node) {
        if (!node)
            return null
        const link = pipewire.linkGroups.values.find(group => group.source && group.source.id === node.id)
        return link ? link.target : null
    }

    // Serialize native values, never QObjects. Only Rust interprets these fields.
    function snapshot() {
        return {
            players: mpris.players.values.map(player => ({
                bus: player.dbusName,
                identity: player.identity,
                desktopEntry: player.desktopEntry || "",
                title: player.trackTitle,
                artist: player.trackArtist,
                album: player.trackAlbum,
                url: (player.metadata || {})["xesam:url"] || "",
                artUrl: player.trackArtUrl || "",
                browserHint: (player.metadata || {})["mprisence:browser"] ?? null,
                playing: player.isPlaying,
                stopped: player.playbackState === MprisPlaybackState.Stopped,
                position: player.position,
                positionSupported: player.positionSupported,
                rate: player.rate,
                length: player.lengthSupported && player.length > 0 ? player.length : null,
                canSeek: player.canSeek,
                canNext: player.canGoNext,
                canPrevious: player.canGoPrevious
            })),
            streams: playbackStreams().map(node => ({
                id: node.id, name: node.name || "", nickname: node.nickname || "",
                ready: node.ready, properties: node.properties || {}
            }))
        }
    }

    // Position advances without metadata changes. All identity, ownership and
    // capability fields must still match before accepting an asynchronous plan.
    function stamp(data) {
        return JSON.stringify({
            players: data.players.map(player => [player.bus, player.identity, player.desktopEntry,
                player.title, player.artist, player.album, player.url, player.artUrl,
                player.browserHint, player.playing, player.stopped, player.length,
                player.positionSupported, player.rate,
                player.canSeek, player.canNext, player.canPrevious]),
            streams: data.streams
        })
    }

    function refresh() {
        refreshPending = true
        Qt.callLater(pump)
    }

    function current(request) {
        const data = snapshot()
        if (request.selection !== selectionSerial || request.stamp !== stamp(data))
            return false
        // Native position normally advances at playback rate. A seek/discontinuity
        // invalidates an old plan even when metadata and the bus are unchanged.
        const elapsed = Math.max(0, (Date.now() - request.time) / 1000)
        return data.players.every((player, index) => {
            const before = request.data.players[index]
            return !before.positionSupported
                || Math.abs(player.position - before.position - (before.playing ? elapsed * before.rate : 0)) < 0.15
        })
    }

    function action(kind, player, position, destination) {
        const data = snapshot()
        const operation = { kind: kind, player: player ? player.dbusName : "",
            position: position === undefined ? 0 : position, destination: destination || "" }
        // Slider drags only need the latest unsubmitted position, not a backlog.
        if (kind === "seek")
            queuedActions = queuedActions.filter(queued => queued.action.kind !== "seek" || queued.action.player !== operation.player)
        queuedActions.push({ action: operation, data: data, stamp: stamp(data),
            time: Date.now(), selection: selectionSerial })
        if (kind === "move")
            movePending = true
        Qt.callLater(pump)
    }

    function pump() {
        if (!workerReady || inFlight || (!refreshPending && queuedActions.length === 0))
            return
        let queued = null
        while (queuedActions.length > 0) {
            const candidate = queuedActions.shift()
            if (current(candidate)) {
                queued = candidate
                break
            }
            failure = "The media source changed. Try the control again."
            if (candidate.action.kind === "move")
                movePending = false
            refreshPending = true
        }
        const data = snapshot()
        const request = {
            requestId: ++requestSerial,
            selectedPlayer: activeGroup ? activeGroup.primary.dbusName : "",
            players: data.players,
            streams: data.streams
        }
        if (queued)
            request.action = queued.action
        inFlight = { request: request, data: data, stamp: stamp(data),
            time: Date.now(), selection: selectionSerial }
        refreshPending = false
        worker.write(JSON.stringify(request) + "\n")
    }

    function readResponse(line) {
        const pending = inFlight
        if (!pending)
            return
        let result
        try {
            result = JSON.parse(line)
        } catch (error) {
            failure = "Invalid response from the media helper."
            worker.running = false
            return
        }
        if (result.requestId !== pending.request.requestId) {
            failure = result.error || "Unexpected response from the media helper."
            worker.running = false
            return
        }
        inFlight = null
        const stale = !current(pending)
        if (stale) {
            if (pending.request.action) {
                failure = "The media source changed. Try the control again."
                if (pending.request.action.kind === "move")
                    movePending = false
            }
            refresh()
        } else {
            failure = result.error || ""
            adoptGroups(result)
            if (!result.error) {
                // Preserve the latest drag target across our own native seek,
                // without rebasing requests already invalidated by another source.
                const rebase = queuedActions.filter(queued => queued.action.kind === "seek"
                    && current(queued) && result.actions.some(operation => operation.operation === "seek"
                        && operation.player === queued.action.player))
                for (const operation of result.actions)
                    applyNative(operation)
                for (const queued of rebase) {
                    queued.data = snapshot()
                    queued.stamp = stamp(queued.data)
                    queued.time = Date.now()
                }
                if (result.route)
                    runMove(result.route)
            }
            if (pending.request.action && pending.request.action.kind === "move")
                movePending = false
        }
        Qt.callLater(pump)
    }

    function adoptGroups(result) {
        const nodes = pipewire.nodes.values
        groups = result.groups.map(group => {
            const node = group.streamId === null ? null
                : (nodes.find(candidate => candidate.id === group.streamId) || null)
            const sink = sinkForStream(node)
            return {
                title: group.title, artist: group.artist, displayName: group.displayName,
                artUrl: group.artUrl, selector: group.selector, routingReason: group.routingReason,
                primary: playerFor(group.primary), players: group.players.map(bus => playerFor(bus)),
                streamNode: node, sinkName: sink ? sink.name : ""
            }
        })
        activeGroup = groups.find(group => group.primary.dbusName === result.activePlayer) || null
    }

    // These operations are implemented by Quickshell's native MPRIS service.
    function applyNative(operation) {
        const player = playerFor(operation.player)
        if (!player)
            return
        switch (operation.operation) {
        case "play": player.play(); break
        case "pause": player.pause(); break
        case "next": player.next(); break
        case "previous": player.previous(); break
        case "seek": player.position = operation.value; break
        }
    }

    function selectGroup(group) {
        activeGroup = group
        selectionSerial++
        refresh()
    }

    function cycle() {
        if (groups.length > 0)
            selectGroup(groups[(groups.indexOf(activeGroup) + 1) % groups.length])
    }

    function moveStream(key) {
        if (canRoute)
            action("move", activeGroup.primary, 0, key)
    }

    function runMove(route) {
        routeFailure = ""
        routeFailurePlayer = route.player
        moveProcess.command = [routingHelper, "stream-move", route.selector, route.destination]
        moveProcess.awaitingStart = true
        moveProcess.running = true
    }

    function refreshRoutes() {
        if (!routesProcess.running)
            routesProcess.running = true
    }

    Connections {
        target: controller.mpris.players
        function onValuesChanged() { controller.refresh() }
    }
    Connections {
        target: controller.pipewire.nodes
        function onValuesChanged() { controller.refresh() }
    }
    Connections {
        target: controller.pipewire.linkGroups
        function onValuesChanged() { controller.refresh() }
    }
    onOpenedChanged: {
        refresh()
        if (opened)
            refreshRoutes()
    }
    Component.onCompleted: worker.running = true

    Timer {
        interval: 2000
        repeat: true
        running: controller.opened
        onTriggered: controller.refresh()
    }
    Timer {
        interval: 1000
        repeat: true
        running: !worker.running
        onTriggered: worker.running = true
    }

    Process {
        id: worker
        command: [controller.helper, "model"]
        stdinEnabled: true
        stdout: SplitParser { onRead: line => controller.readResponse(line) }
        stderr: SplitParser { onRead: line => console.warn("media helper:", line) }
        onStarted: {
            controller.workerReady = true
            controller.refresh()
        }
        onRunningChanged: {
            if (!running) {
                controller.workerReady = false
                controller.inFlight = null
                controller.queuedActions = []
                controller.movePending = false
                controller.groups = []
                controller.activeGroup = null
                controller.failure = "Media helper unavailable. Rebuild or check media-control."
            }
        }
    }
    Process {
        id: routesProcess
        command: [controller.routingHelper, "devices", "output"]
        stdout: StdioCollector {
            onStreamFinished: {
                try {
                    controller.routes = JSON.parse(this.text).map(row => ({
                        key: row.key, label: row.label, description: row.description,
                        name: row.name, isDefault: row.default
                    }))
                } catch (error) {
                    console.warn("media panel: invalid device list:", error)
                    controller.routes = []
                }
            }
        }
    }
    Process {
        id: moveProcess
        property bool awaitingStart: false
        stderr: StdioCollector { id: moveErrors }
        onStarted: awaitingStart = false
        onRunningChanged: {
            if (!running && awaitingStart) {
                awaitingStart = false
                controller.routeFailure = "Could not start the audio routing helper."
            }
        }
        onExited: (exitCode, exitStatus) => {
            controller.routeFailure = exitCode === 0 && exitStatus === 0 ? ""
                : (moveErrors.text.trim() || "The audio routing helper failed.")
            controller.refreshRoutes()
            controller.refresh()
        }
    }
}
