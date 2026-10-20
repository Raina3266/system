// The media card: artwork and metadata, seek bar, transport, stream volume,
// the per-stream output selector, and the source switcher.
//
// KDE-menu-style frame: translucent charcoal, a thin yellow outline, and a
// chamfer at the top-left and bottom-right. Only the background is translucent; text and
// controls keep their own opacity.
import QtQuick
import QtQuick.Layouts

import "shared" as Shared

Item {
    id: card

    required property var panel
    readonly property var colors: Shared.Colors {}
    readonly property var group: panel.activeGroup
    readonly property var player: group ? group.primary : null
    readonly property bool playing: player !== null && player.isPlaying
    MediaCredit {
        id: metadataCredit
        helper: card.panel.mediaControl
        active: card.panel.openScreen !== "" && card.player !== null
        url: card.player ? ((card.player.metadata || {})["xesam:url"] || "") : ""
        artist: card.player ? card.player.trackArtist : ""
        album: card.player ? card.player.trackAlbum : ""
    }

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
        if (!panel.streamNode)
            return "No separate audio stream"
        const sink = panel.streamSink
        if (!sink)
            return "Detecting output…"
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
        color: card.colors.primary
        font.pixelSize: 10
        font.letterSpacing: 2
        font.bold: true
    }

    // A section header row whose whole line toggles its dropdown, with a
    // hover wash and a plain chevron indicator.
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

    Shared.PopupFrame {
        anchors.fill: parent
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

        // ---- Header: artwork and metadata ------------------------------------
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
                    publishedArt: card.group ? card.group.artUrl : ""
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
                    wrapMode: Text.Wrap
                    maximumLineCount: 2
                    elide: Text.ElideRight
                }
                Text {
                    Layout.fillWidth: true
                    text: metadataCredit.text
                    color: card.colors.metadata
                    font.pixelSize: 13
                    elide: Text.ElideRight
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
                onSeekRequested: position => card.panel.mediaAction("seek", card.player, position)
            }
            RowLayout {
                Layout.fillWidth: true
                Text {
                    text: card.formatTime(card.player ? card.player.position : 0)
                    color: card.colors.accent
                    font.pixelSize: 12
                }
                Item { Layout.fillWidth: true }
                Text {
                    text: card.formatTime(card.player ? card.player.length : 0)
                    color: card.colors.muted
                    font.pixelSize: 12
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

                Shared.IconButton {
                    glyph: "󰒮"
                    enabled: card.player && card.player.canGoPrevious
                    onActivated: card.panel.mediaAction("previous", card.player)
                }
                Shared.IconButton {
                    glyph: card.player && card.player.isPlaying ? "󰏤" : "󰐊"
                    accent: true
                    size: 40
                    onActivated: card.panel.mediaAction("toggle", card.player)
                }
                Shared.IconButton {
                    glyph: "󰒭"
                    enabled: card.player && card.player.canGoNext
                    onActivated: card.panel.mediaAction("next", card.player)
                }
            }

            // Percentage, slider, and soft mute share the same volume backend.
            Shared.IconButton {
                id: volumeButton
                outlined: false
                enabled: volumeSlider.enabled
                implicitWidth: volumeContent.implicitWidth + 8
                property real preMuteVolume: 0.5
                glyph: {
                    const volume = volumeSlider.currentValue
                    return volume < 0.01 ? "󰝟" : volume < 0.5 ? "󰖀" : "󰕾"
                }
                contentItem: RowLayout {
                    id: volumeContent
                    spacing: 4
                    Text {
                        text: volumeButton.glyph
                        color: volumeButton.enabled ? card.colors.destructive : card.colors.muted
                        font.pixelSize: 16
                    }
                    Text {
                        text: volumeSlider.enabled ? Math.round(volumeSlider.currentValue * 100) + "%" : "—"
                        color: volumeButton.enabled ? card.colors.accent : card.colors.muted
                        font.pixelSize: 12
                        font.bold: true
                    }
                }
                onActivated: {
                    if (!volumeSlider.enabled)
                        return
                    const volume = volumeSlider.currentValue
                    if (volume > 0.01) {
                        preMuteVolume = volume
                        volumeSlider.setVolume(0)
                    } else {
                        volumeSlider.setVolume(preMuteVolume)
                    }
                }
            }
            VolumeSlider {
                id: volumeSlider
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
                    if (card.panel.canRoute)
                        outputList.open = !outputList.open
                }
                Text { text: "󰓃"; color: card.colors.primary; font.pixelSize: 13 }
                SectionLabel { text: "OUTPUT" }
                Text {
                    Layout.fillWidth: true
                    text: card.currentOutputLabel()
                    color: card.colors.accent
                    font.pixelSize: 12
                    font.bold: true
                    elide: Text.ElideRight
                }
                Text {
                    Layout.preferredWidth: 26
                    text: outputList.open ? "󰅀" : "󰅃"
                    color: card.panel.canRoute
                        ? card.colors.primary
                        : card.colors.muted
                    font.pixelSize: 14
                    horizontalAlignment: Text.AlignHCenter
                    verticalAlignment: Text.AlignVCenter
                }
            }
            Text {
                Layout.fillWidth: true
                visible: text !== ""
                text: card.panel.routingStatus
                textFormat: Text.PlainText
                color: card.colors.muted
                font.pixelSize: 11
                wrapMode: Text.Wrap
            }
            Text {
                Layout.fillWidth: true
                visible: text !== ""
                text: card.panel.routingError
                textFormat: Text.PlainText
                color: card.colors.destructive
                font.pixelSize: 11
                wrapMode: Text.Wrap
            }
            OutputList {
                id: outputList
                Layout.fillWidth: true
                enabled: card.panel.canRoute
                detailColor: card.colors.metadata
                routes: card.panel.routes
                currentName: card.panel.streamSink ? card.panel.streamSink.name : ""
                onSelected: key => {
                    card.panel.moveStream(key)
                    open = false
                }
            }
            Text {
                Layout.fillWidth: true
                visible: outputList.open && card.panel.routes.some(route => route.name === "")
                text: "Inactive outputs need a sound-card profile switch, which may affect other audio."
                color: card.colors.muted
                font.pixelSize: 11
                wrapMode: Text.Wrap
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
                    font.bold: true
                    elide: Text.ElideRight
                }
                Text {
                    Layout.preferredWidth: 26
                    text: sourcesList.open ? "󰅀" : "󰅃"
                    color: card.panel.groups.length > 1 ? card.colors.primary : card.colors.muted
                    font.pixelSize: 14
                    horizontalAlignment: Text.AlignHCenter
                    verticalAlignment: Text.AlignVCenter
                }
            }
            SourcesList {
                id: sourcesList
                Layout.fillWidth: true
                detailColor: card.colors.metadata
                panel: card.panel
                groups: card.panel.groups
                active: card.panel.activeGroup
                routes: card.panel.routes
                onSelected: group => {
                    card.panel.selectGroup(group)
                    open = false
                }
            }
        }
    }
}
