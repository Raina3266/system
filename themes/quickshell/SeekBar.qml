import QtQuick
import "shared" as Shared

// A QQC2 Slider restyled with the shell palette: the control supplies drag,
// keyboard and focus semantics; seeking fans out to every merged publisher,
// skipping players that cannot seek.
Shared.StyledSlider {
    id: bar

    property var player: null
    property var players: []

    from: 0
    to: player && player.length > 0 ? player.length : 1
    enabled: player !== null && player.canSeek
    trackColor: colors.primary
    animateChanges: false

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
}
