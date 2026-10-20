import QtQuick
import QtQuick.Layouts
import "shared" as Shared

// A compact toggle/action tile using the media panel's palette and motion.
Rectangle {
    id: tile

    readonly property var colors: Shared.Colors {}
    property string glyph: ""
    property string label: ""
    property bool active: false
    signal activated()

    implicitWidth: 112
    implicitHeight: 62
    radius: 3
    color: active
        ? Qt.alpha(colors.dimPrimary, 0.5)
        : (pointer.containsMouse ? Qt.alpha(colors.dimPrimary, 0.25) : colors.secondaryBackground)
    border.width: 1
    border.color: active ? Qt.alpha(colors.accent, 0.8) : Qt.alpha(colors.muted, 0.35)
    scale: pointer.pressed ? 0.97 : 1

    Behavior on color { ColorAnimation { duration: 120 } }
    Behavior on border.color { ColorAnimation { duration: 120 } }
    Behavior on scale { NumberAnimation { duration: 120; easing.type: Easing.OutCubic } }

    ColumnLayout {
        anchors.centerIn: parent
        spacing: 3

        Text {
            Layout.alignment: Qt.AlignHCenter
            text: tile.glyph
            color: tile.active ? tile.colors.accent : tile.colors.primary
            font.pixelSize: 17
        }
        Text {
            Layout.alignment: Qt.AlignHCenter
            text: tile.label
            color: tile.active ? tile.colors.text : tile.colors.muted
            font.pixelSize: 9
            font.bold: true
            font.letterSpacing: 1
        }
    }

    MouseArea {
        id: pointer
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: tile.activated()
    }
}
