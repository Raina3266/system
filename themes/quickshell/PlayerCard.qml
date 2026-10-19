// The media card: artwork and metadata, seek bar, transport, stream volume,
// the per-stream output selector, and the source switcher.
//
// Styled after the Daemon KDE theme (Kvantum): burgundy window, darker
// controls, cyan text, pink icons, dim-pink highlight, yellow structural
// frames — flat, small radius, restrained yellow glow.
import QtQuick
import QtQuick.Effects
import QtQuick.Layouts

Item {
    id: card

    required property var panel
    readonly property var colors: Colors {}
    readonly property var group: panel.activeGroup
    readonly property var player: group ? group.primary : null
    readonly property bool playing: player !== null && player.isPlaying

    implicitHeight: contentColumn.implicitHeight + 32

    // The card grows and shrinks with its dropdowns, not in jumps.
    Behavior on implicitHeight { NumberAnimation { duration: 180; easing.type: Easing.OutCubic } }

    function formatTime(seconds) {
        const total = Math.floor(seconds)
        const hours = Math.floor(total / 3600)
        const minutes = Math.floor((total % 3600) / 60)
        const rest = total % 60
        const padded = rest < 10 ? "0" + rest : "" + rest
        return hours > 0 ? hours + ":" + (minutes < 10 ? "0" : "") + minutes + ":" + padded : minutes + ":" + padded
    }

    function currentOutputLabel() {
        const sink = panel.streamSink
        if (!sink)
            return "System default"
        for (const route of panel.routes) {
            if (route.name === sink.name)
                return route.label
        }
        return sink.description || sink.name
    }

    component Divider: Rectangle {
        Layout.fillWidth: true
        height: 1
        color: Qt.alpha(card.colors.warning, 0.15)
    }

    component SectionLabel: Text {
        color: card.colors.muted
        font.pixelSize: 10
        font.letterSpacing: 2
        font.bold: true
    }

    // A section header row whose whole line toggles its dropdown, with a
    // hover wash for affordance. The chevron button still works on its own.
    component SectionRow: Rectangle {
        default property alias content: rowLayout.data
        signal toggled()

        Layout.fillWidth: true
        implicitHeight: rowLayout.implicitHeight + 8
        radius: 3
        color: hoverArea.containsMouse ? Qt.alpha(card.colors.dimPrimary, 0.35) : "transparent"
        Behavior on color { ColorAnimation { duration: 120 } }

        MouseArea {
            id: hoverArea
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: toggled()
        }
        RowLayout {
            id: rowLayout
            anchors { left: parent.left; right: parent.right; verticalCenter: parent.verticalCenter; leftMargin: 8; rightMargin: 4 }
            spacing: 10
        }
    }

    // Eats clicks on empty card areas so the backdrop does not close the panel.
    MouseArea {
        anchors.fill: parent
        onClicked: { }
    }

    Rectangle {
        id: cardFrame
        anchors.fill: parent
        radius: 3
        color: card.colors.background
        border.width: 2
        border.color: card.colors.warning
    }

    MultiEffect {
        anchors.fill: cardFrame
        source: cardFrame
        shadowEnabled: true
        shadowColor: Qt.alpha(card.colors.warning, 0.7)
        shadowBlur: 0.65
        shadowOpacity: 0.65
        shadowVerticalOffset: 3
    }

    ColumnLayout {
        id: contentColumn
        anchors { left: parent.left; right: parent.right; top: parent.top; margins: 16 }
        spacing: 12

        Text {
            Layout.fillWidth: true
            Layout.topMargin: 12
            Layout.bottomMargin: 12
            visible: card.player === null
            text: "󰝚  NOTHING PLAYING"
            color: card.colors.muted
            font.pixelSize: 12
            font.letterSpacing: 3
            horizontalAlignment: Text.AlignHCenter
        }

        // ---- Header: artwork, metadata, source identity -----------------------
        RowLayout {
            Layout.fillWidth: true
            spacing: 12
            visible: card.player !== null

            Rectangle {
                Layout.preferredWidth: 68
                Layout.preferredHeight: 68
                radius: 3
                color: card.colors.secondaryBackground
                border.width: 1
                border.color: Qt.alpha(card.colors.muted, 0.4)
                clip: true

                Artwork {
                    anchors.fill: parent
                    panel: card.panel
                    player: card.player
                    glyphSize: 26
                }
            }

            ColumnLayout {
                Layout.fillWidth: true
                spacing: 4

                Text {
                    Layout.fillWidth: true
                    text: card.player && card.player.trackTitle !== "" ? card.player.trackTitle : "Unknown title"
                    color: card.colors.accent
                    font.pixelSize: 15
                    font.bold: true
                    elide: Text.ElideRight
                }
                Text {
                    Layout.fillWidth: true
                    text: {
                        if (!card.player)
                            return ""
                        const parts = []
                        if (card.player.trackArtist !== "")
                            parts.push(card.player.trackArtist)
                        if (card.player.trackAlbum !== "")
                            parts.push(card.player.trackAlbum)
                        return parts.join(" · ")
                    }
                    color: card.colors.muted
                    font.pixelSize: 11
                    elide: Text.ElideRight
                }
                RowLayout {
                    spacing: 6
                    Rectangle {
                        width: 6
                        height: 6
                        radius: 3
                        color: card.playing ? card.colors.primary : card.colors.muted
                        SequentialAnimation on opacity {
                            loops: Animation.Infinite
                            running: card.playing && card.visible
                            NumberAnimation { to: 0.25; duration: 700; easing.type: Easing.InOutSine }
                            NumberAnimation { to: 1.0; duration: 700; easing.type: Easing.InOutSine }
                        }
                    }
                    Text {
                        Layout.fillWidth: true
                        text: card.group ? card.group.displayName : ""
                        color: card.colors.accent
                        font.pixelSize: 11
                        font.letterSpacing: 1
                        elide: Text.ElideRight
                    }
                }
            }
        }

        Divider { visible: card.player !== null }

        // ---- Seek bar + times --------------------------------------------------
        ColumnLayout {
            Layout.fillWidth: true
            spacing: 4
            visible: card.player !== null

            SeekBar {
                Layout.fillWidth: true
                player: card.player
                players: card.group ? card.group.players : []
            }
            RowLayout {
                Layout.fillWidth: true
                Text {
                    text: card.formatTime(card.player ? card.player.position : 0)
                    color: card.colors.accent
                    font.pixelSize: 10
                }
                Item { Layout.fillWidth: true }
                Text {
                    text: card.formatTime(card.player ? card.player.length : 0)
                    color: card.colors.muted
                    font.pixelSize: 10
                }
            }
        }

        // ---- Transport + volume on one line ---------------------------------------
        RowLayout {
            Layout.fillWidth: true
            Layout.topMargin: -6
            Layout.bottomMargin: -6
            spacing: 14
            visible: card.player !== null

            RowLayout {
                spacing: 14

                IconButton {
                    glyph: "󰒮"
                    enabled: card.player && card.player.canGoPrevious
                    // Next/previous go to one publisher or merged players double-skip.
                    onActivated: {
                        if (card.player.canGoPrevious)
                            card.player.previous()
                    }
                }
                IconButton {
                    glyph: card.player && card.player.isPlaying ? "󰏤" : "󰐊"
                    accent: true
                    size: 40
                    onActivated: {
                        // Idempotent, and fanned out to every publisher of the track.
                        const playing = card.group.players.some(player => player.isPlaying)
                        for (const player of card.group.players)
                            player.isPlaying = !playing
                    }
                }
                IconButton {
                    glyph: "󰒭"
                    enabled: card.player && card.player.canGoNext
                    onActivated: {
                        if (card.player.canGoNext)
                            card.player.next()
                    }
                }
            }

            // Volume glyph doubles as a soft mute: 0 and back through the
            // same MPRIS volume path, since PipeWire node writes are no-ops
            // on this Quickshell build.
            IconButton {
                property real preMuteVolume: 0.5
                glyph: {
                    const player = card.player
                    if (player && player.volumeSupported) {
                        const volume = player.volume
                        return volume < 0.01 ? "󰝟" : volume < 0.5 ? "󰖀" : "󰕾"
                    }
                    return "󰕾"
                }
                onActivated: {
                    const player = card.player
                    if (!player || !player.volumeSupported)
                        return
                    if (player.volume > 0.01) {
                        preMuteVolume = player.volume
                        player.volume = 0
                    } else {
                        player.volume = preMuteVolume
                    }
                }
            }
            VolumeSlider {
                Layout.fillWidth: true
                stream: card.panel.streamNode
                player: card.player
            }
        }

        Divider { visible: card.player !== null }

        // ---- Output device selector ------------------------------------------------
        ColumnLayout {
            Layout.fillWidth: true
            spacing: 4
            visible: card.player !== null

            SectionRow {
                onToggled: {
                    if (card.panel.streamNode !== null && !card.panel.moving)
                        outputList.open = !outputList.open
                }
                Text { text: "󰓃"; color: card.colors.primary; font.pixelSize: 13 }
                SectionLabel { text: "OUTPUT" }
                Text {
                    Layout.fillWidth: true
                    text: card.currentOutputLabel()
                    color: card.colors.accent
                    font.pixelSize: 12
                    elide: Text.ElideRight
                }
                IconButton {
                    glyph: outputList.open ? "󰅀" : "󰅃"
                    size: 26
                    enabled: card.panel.streamNode !== null && !card.panel.moving
                    onActivated: outputList.open = !outputList.open
                }
            }
            OutputList {
                id: outputList
                Layout.fillWidth: true
                routes: card.panel.routes
                currentName: card.panel.streamSink ? card.panel.streamSink.name : ""
                onSelected: key => {
                    card.panel.moveStream(key)
                    open = false
                }
            }
        }

        // ---- Source switcher ---------------------------------------------------------
        ColumnLayout {
            Layout.fillWidth: true
            spacing: 4
            visible: card.player !== null

            SectionRow {
                onToggled: {
                    if (card.panel.groups.length > 1)
                        sourcesList.open = !sourcesList.open
                }
                Text { text: "󰐊"; color: card.colors.primary; font.pixelSize: 13 }
                SectionLabel { text: "MEDIA" }
                Text {
                    Layout.fillWidth: true
                    text: card.group ? card.group.displayName : ""
                    color: card.colors.accent
                    font.pixelSize: 12
                    elide: Text.ElideRight
                }
                IconButton {
                    glyph: sourcesList.open ? "󰅀" : "󰅃"
                    size: 26
                    enabled: card.panel.groups.length > 1
                    onActivated: sourcesList.open = !sourcesList.open
                }
            }
            SourcesList {
                id: sourcesList
                Layout.fillWidth: true
                panel: card.panel
                groups: card.panel.groups
                active: card.panel.activeGroup
                routes: card.panel.routes
                onSelected: group => {
                    card.panel.activeGroup = group
                    open = false
                }
            }
        }
    }
}
