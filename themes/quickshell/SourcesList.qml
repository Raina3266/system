import QtQuick
import QtQuick.Layouts

// The source switcher: one row per merged player group, playing or paused.
// Selecting a row only retargets the card; nothing is raised or focused.
ColumnLayout {
    id: list

    readonly property var colors: Colors {}
    required property var panel
    property var groups: []
    property var active: null
    property var routes: []
    property bool open: false

    signal selected(var group)

    // The compact route label for a sink node name (Speaker, Headphones...).
    function outputLabel(sinkName) {
        if (!sinkName)
            return ""
        for (const route of routes) {
            if (route.name === sinkName)
                return route.label
        }
        return sinkName
    }

    visible: open
    opacity: open ? 1 : 0
    Behavior on opacity { NumberAnimation { duration: 140 } }
    spacing: 2

    Repeater {
        model: list.groups

        delegate: Rectangle {
            required property var modelData

            Layout.fillWidth: true
            implicitHeight: 42
            radius: 4
            readonly property bool current: modelData === list.active
            color: current ? list.colors.dimPrimary : (area.containsMouse ? Qt.alpha(list.colors.dimPrimary, 0.5) : "transparent")
            Behavior on color { ColorAnimation { duration: 120 } }

            // Below the content: texts let clicks through to select the row,
            // while the play/pause button consumes its own click instead.
            MouseArea {
                id: area
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onClicked: list.selected(modelData)
            }

            RowLayout {
                anchors { fill: parent; leftMargin: 6; rightMargin: 8 }
                spacing: 8

                // Per-source play/pause without retargeting the card, so
                // several players can be paused or resumed one after another.
                IconButton {
                    glyph: modelData.primary.isPlaying ? "󰏤" : "󰐊"
                    size: 26
                    onActivated: {
                        const playing = modelData.players.some(player => player.isPlaying)
                        for (const player of modelData.players)
                            player.isPlaying = !playing
                    }
                }

                // The source's published art (bilibili/YouTube covers); local
                // players without art fall back to the glyph — embedded-tag
                // extraction stays a card-only concern.
                Rectangle {
                    Layout.preferredWidth: 34
                    Layout.preferredHeight: 34
                    radius: 3
                    color: list.colors.secondaryBackground
                    border.width: 1
                    border.color: Qt.alpha(list.colors.muted, 0.4)
                    clip: true

                    Artwork {
                        anchors.fill: parent
                        panel: list.panel
                        player: modelData.primary
                        glyphSize: 14
                    }
                }
                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 1

                    RowLayout {
                        Layout.fillWidth: true
                        spacing: 5

                        Text {
                            Layout.fillWidth: true
                            text: modelData.displayName
                            color: list.colors.accent
                            font.pixelSize: 13
                            font.bold: parent.parent.parent.current
                            elide: Text.ElideRight
                            Behavior on color { ColorAnimation { duration: 120 } }
                        }
                        // Each source may route to a different output; shown
                        // inline so long names are not clipped off the edge.
                        Text {
                            visible: modelData.sinkName !== ""
                            text: "(" + list.outputLabel(modelData.sinkName) + ")"
                            color: list.colors.muted
                            font.pixelSize: 11
                            elide: Text.ElideRight
                            Layout.maximumWidth: 160
                        }
                    }
                    Text {
                        Layout.fillWidth: true
                        text: modelData.title
                        color: list.colors.muted
                        font.pixelSize: 11
                        elide: Text.ElideRight
                    }
                }
            }
        }
    }
}
