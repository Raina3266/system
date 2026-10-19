import QtQuick
import QtQuick.Controls

// A QQC2 Slider for stream volume: the PipeWire node when one is matched
// (live, works for players that publish no MPRIS volume), falling back to the
// player's own MPRIS volume. Clamped to 0-100% like media-panel.
Slider {
    id: slider

    readonly property var colors: Colors {}
    property var stream: null
    property var player: null

    // MPRIS volume first: it works for players that publish it (Elisa,
    // mprisence-bridged tabs). The PipeWire node's audio object is the
    // fallback — on this Quickshell build nodes stay unbound, where reads
    // return 0 and writes are silently dropped, so preferring it would show
    // nothing and swallow every drag.
    readonly property bool mprisVolume: player !== null && player.volumeSupported
    readonly property bool hasStream: !mprisVolume && stream !== null && stream.audio !== null
    readonly property real currentValue: {
        if (mprisVolume)
            return Math.min(1, Math.max(0, player.volume))
        if (hasStream)
            return Math.min(1, Math.max(0, stream.audio.volume))
        return 0
    }

    from: 0
    to: 1
    enabled: mprisVolume || hasStream
    implicitHeight: 16

    Binding on value {
        when: !slider.pressed
        value: slider.currentValue
    }

    onMoved: {
        if (mprisVolume)
            player.volume = value
        else if (hasStream)
            stream.audio.volume = value
    }

    background: Rectangle {
        x: slider.leftPadding
        y: slider.topPadding + slider.availableHeight / 2 - height / 2
        width: slider.availableWidth
        height: 4
        radius: 2
        color: slider.colors.secondaryBackground

        Rectangle {
            width: slider.visualPosition * parent.width
            height: parent.height
            radius: 2
            color: slider.colors.dimPrimary
            Behavior on width {
                enabled: !slider.pressed
                NumberAnimation { duration: 120; easing.type: Easing.OutCubic }
            }
        }
    }

    handle: Rectangle {
        x: slider.leftPadding + slider.visualPosition * (slider.availableWidth - width)
        y: slider.topPadding + slider.availableHeight / 2 - height / 2
        width: 10
        height: 10
        radius: 5
        color: slider.colors.primary
        visible: slider.enabled
        scale: slider.pressed ? 1.35 : (slider.hovered ? 1.15 : 1.0)
        Behavior on scale { NumberAnimation { duration: 120; easing.type: Easing.OutCubic } }
    }
}

