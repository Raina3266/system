import QtQuick
import QtQuick.Controls

// A QQC2 ToolButton as a round glyph button: hover glow, springy press.
ToolButton {
    id: button

    readonly property var colors: Colors {}
    property string glyph: ""
    property int size: 32
    // Accent buttons (the play/pause) take the primary neon colour.
    property bool accent: false

    signal activated()
    onClicked: activated()

    implicitWidth: size
    implicitHeight: size
    padding: 0
    scale: pressed ? 0.86 : 1.0
    Behavior on scale { NumberAnimation { duration: 140; easing.type: Easing.OutBack; easing.overshoot: 3 } }

    contentItem: Text {
        text: button.glyph
        // Daemon icons are pink; text elsewhere is cyan.
        color: button.enabled ? button.colors.primary : button.colors.muted
        font.pixelSize: button.size * 0.5
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
        Behavior on color { ColorAnimation { duration: 120 } }
    }

    background: Rectangle {
        radius: width / 2
        color: button.hovered && button.enabled
            ? Qt.alpha(button.colors.dimPrimary, 0.6)
            : (button.accent ? Qt.alpha(button.colors.dimPrimary, 0.35) : "transparent")
        Behavior on color { ColorAnimation { duration: 120 } }
    }
}
