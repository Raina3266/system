// One transparent fullscreen layer surface per screen. The visible card sits
// centred directly below the top bar; clicking anywhere else (or Escape, or a
// second press of the Waybar button) closes the panel.
import QtQuick
import Quickshell
import Quickshell.Wayland

PanelWindow {
    id: window

    // Injected by Variants after creation; it cannot be `required`.
    property ShellScreen modelData
    required property var panel

    readonly property bool open: panel.openScreen === modelData.name

    // The card is clipped to the layer surface, so the surface must cover
    // the whole screen; without anchors it is a 100x100 compositor-centred
    // window and only a fragment of the card shows.
    anchors {
        top: true
        left: true
        bottom: true
        right: true
    }

    screen: modelData
    visible: open || card.opacity > 0
    color: "transparent"
    exclusionMode: ExclusionMode.Ignore
    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.namespace: "quickshell:media"
    WlrLayershell.keyboardFocus: open ? WlrKeyboardFocus.Exclusive : WlrKeyboardFocus.None

    MouseArea {
        id: backdrop
        anchors.fill: parent
        focus: true
        onClicked: panel.closePanel()
        Keys.onEscapePressed: panel.closePanel()
    }

    PlayerCard {
        id: card
        anchors.horizontalCenter: parent.horizontalCenter
        width: 460
        panel: window.panel

        // Fade + rise on open, reverse on close; the window stays mapped
        // until the exit animation finishes (visible binding above).
        y: window.open ? 40 : 24
        opacity: window.open ? 1 : 0
        Behavior on y { NumberAnimation { duration: 220; easing.type: Easing.OutCubic } }
        Behavior on opacity { NumberAnimation { duration: 180; easing.type: Easing.OutCubic } }
    }
}
