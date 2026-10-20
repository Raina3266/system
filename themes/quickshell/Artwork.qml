import QtQuick
import Quickshell.Io
import "shared" as Shared

// Shared cover resolver for the card and source rows. Use published MPRIS
// artwork when available; local players fall back to media-control, which
// checks beside the track and extracts embedded covers asynchronously.
Item {
    id: artwork

    required property var panel
    required property var player
    readonly property var colors: Shared.Colors {}
    property int glyphSize: 20

    property string sourceUrl: ""
    property string pendingTrackUrl: ""
    property string activeTrackUrl: ""
    property bool lookupPending: false
    property bool usingPublishedArt: false

    property string publishedArt: ""
    readonly property string trackUrl: {
        const meta = player ? (player.metadata || {}) : {}
        return meta["xesam:url"] || ""
    }

    onPublishedArtChanged: refresh()
    onTrackUrlChanged: refresh()
    Component.onCompleted: refresh()

    function refresh() {
        if (publishedArt) {
            usingPublishedArt = true
            sourceUrl = publishedArt
            pendingTrackUrl = ""
            lookupPending = false
            return
        }

        usingPublishedArt = false
        sourceUrl = ""
        queueLocalLookup()
    }

    function queueLocalLookup() {
        const track = trackUrl
        if (track === "") {
            pendingTrackUrl = ""
            lookupPending = false
            return
        }
        if (track === activeTrackUrl && artProcess.running) {
            pendingTrackUrl = ""
            lookupPending = false
            return
        }
        if (track === activeTrackUrl && sourceUrl !== "")
            return
        pendingTrackUrl = track
        if (artProcess.running) {
            lookupPending = true
            return
        }
        startLocalLookup()
    }

    function startLocalLookup() {
        lookupPending = false
        activeTrackUrl = pendingTrackUrl
        if (activeTrackUrl === "")
            return
        artProcess.command = [panel.mediaControl, "artwork", activeTrackUrl]
        artProcess.running = true
    }

    function fallbackToLocal() {
        if (!usingPublishedArt)
            return
        usingPublishedArt = false
        sourceUrl = ""
        queueLocalLookup()
    }

    Process {
        id: artProcess

        stdout: StdioCollector {
            onStreamFinished: {
                const path = this.text.trim().split("\n").pop() || ""
                if (path !== "" && !artwork.usingPublishedArt && artwork.activeTrackUrl === artwork.trackUrl)
                    artwork.sourceUrl = "file://" + path
            }
        }
        onExited: (exitCode, exitStatus) => {
            if (artwork.lookupPending)
                artwork.startLocalLookup()
        }
    }

    Image {
        id: image
        anchors.fill: parent
        source: artwork.sourceUrl
        asynchronous: true
        fillMode: Image.PreserveAspectCrop
        visible: status === Image.Ready
        onStatusChanged: {
            if (status === Image.Error)
                artwork.fallbackToLocal()
        }
    }

    Text {
        anchors.centerIn: parent
        visible: image.status !== Image.Ready
        text: "󰝚"
        color: artwork.colors.muted
        font.pixelSize: artwork.glyphSize
    }
}
