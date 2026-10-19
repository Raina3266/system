import QtQuick
import QtQuick.Controls

// A QQC2 Slider restyled with the shell palette: the control supplies drag,
// keyboard and focus semantics; seeking fans out to every merged publisher,
// skipping players that cannot seek.
Slider {
    id: bar

    readonly property var colors: Colors {}
    property var player: null
    property var players: []

    from: 0
    to: player && player.length > 0 ? player.length : 1
    enabled: player !== null && player.canSeek
    implicitHeight: 16

    // Follow playback position except while the user is dragging.
    Binding on value {
        when: !bar.pressed
        value: bar.player ? bar.player.position : 0
    }

    onMoved: {
        for (const player of bar.players) {
            if (player.canSeek)
                player.position = bar.value
        }
    }

    background: Rectangle {
        x: bar.leftPadding
        y: bar.topPadding + bar.availableHeight / 2 - height / 2
        width: bar.availableWidth
        height: 4
        radius: 2
        color: bar.colors.secondaryBackground

        Rectangle {
            width: bar.visualPosition * parent.width
            height: parent.height
            radius: 2
            // The played part is the bright pink; the groove stays dark.
            color: bar.colors.primary
        }
    }

    handle: Rectangle {
        x: bar.leftPadding + bar.visualPosition * (bar.availableWidth - width)
        y: bar.topPadding + bar.availableHeight / 2 - height / 2
        width: 10
        height: 10
        radius: 5
        color: bar.colors.primary
        visible: bar.enabled
        scale: bar.pressed ? 1.35 : (bar.hovered ? 1.15 : 1.0)
        Behavior on scale { NumberAnimation { duration: 120; easing.type: Easing.OutCubic } }
    }
}
