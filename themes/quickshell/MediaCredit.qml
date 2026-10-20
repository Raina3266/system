import QtQuick
import Quickshell
import Quickshell.Io

// Thin JSON bridge. Rust owns video recognition, fetching, caching and fallback
// credits; UI identity checks prevent old replies from replacing a new track.
Scope {
    id: credit

    required property string helper
    property bool active: false
    property string url: ""
    property string artist: ""
    property string album: ""
    property string text: ""
    property string requestedIdentity: ""
    property string completedIdentity: ""
    readonly property string identity: JSON.stringify({ url: url, artist: artist, album: album })

    onIdentityChanged: {
        text = ""
        completedIdentity = ""
        if (!lookup.running)
            requestedIdentity = ""
        Qt.callLater(request)
    }
    onActiveChanged: {
        if (active) {
            if (!lookup.running && completedIdentity !== identity)
                requestedIdentity = ""
            Qt.callLater(request)
        }
    }
    Component.onCompleted: Qt.callLater(request)

    function request() {
        if (!active || lookup.running || requestedIdentity === identity || completedIdentity === identity)
            return
        requestedIdentity = identity
        lookup.stdinEnabled = true
        lookup.running = true
    }

    Process {
        id: lookup
        command: [credit.helper, "credit"]
        property bool replySeen: false
        onStarted: {
            replySeen = false
            write(credit.requestedIdentity + "\n")
            stdinEnabled = false
        }
        stdout: SplitParser {
            onRead: line => {
                try {
                    const reply = JSON.parse(line)
                    lookup.replySeen = true
                    if (reply.url === credit.url
                            && reply.artist === credit.artist && reply.album === credit.album) {
                        credit.text = reply.credit
                        credit.completedIdentity = credit.identity
                    }
                } catch (error) {
                    console.warn("media panel: invalid credit reply:", error)
                }
            }
        }
        stderr: SplitParser { onRead: line => console.warn("media credit:", line) }
        onExited: {
            if (replySeen || credit.requestedIdentity !== credit.identity) {
                credit.requestedIdentity = ""
                Qt.callLater(credit.request)
            }
        }
    }
}
