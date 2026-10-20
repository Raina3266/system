import QtQuick
import "shared" as Shared

// Native playback position with UI drag/keyboard handling. Rust chooses
// the publisher targets; Quickshell performs the resulting seek operations.
Shared.StyledSlider {
    id: bar

    property var player: null
    signal seekRequested(real position)

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

    onMoved: seekRequested(value)
}
