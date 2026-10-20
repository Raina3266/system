import QtQuick
import "shared" as Shared

// MPRIS volume when published, otherwise the selected PipeWire stream.
// Clamped to 0-100% like media-panel.
Shared.StyledSlider {
    id: slider

    property var stream: null
    property var player: null

    // MPRIS volume first: it works for players that publish it (Elisa,
    // mprisence-bridged tabs). The shell tracks the selected PipeWire node
    // while open; wait for it to be ready before exposing fallback volume.
    readonly property bool mprisVolume: player !== null && player.volumeSupported
    readonly property bool hasStream: !mprisVolume && stream !== null && stream.ready && stream.audio !== null
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

    Binding on value {
        when: !slider.pressed
        value: slider.currentValue
    }

    function setVolume(level) {
        const bounded = Math.min(to, Math.max(from, level))
        if (mprisVolume)
            player.volume = bounded
        else if (hasStream)
            stream.audio.volume = bounded
    }

    onMoved: setVolume(value)
}
