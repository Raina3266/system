import QtQuick
import Quickshell
import Quickshell.Wayland

PanelWindow {
    id: window

    // Variants inject each screen as modelData.
    property ShellScreen modelData
    required property var panel

    readonly property bool open: panel.dashboardOpenScreen === modelData.name

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
    WlrLayershell.namespace: "quickshell:dashboard"
    WlrLayershell.keyboardFocus: open ? WlrKeyboardFocus.Exclusive : WlrKeyboardFocus.None

    MouseArea {
        anchors.fill: parent
        visible: window.open
        focus: true
        onClicked: window.panel.closeDashboard()
        Keys.onEscapePressed: window.panel.closeDashboard()
    }

    Dashboard {
        id: card
        panel: window.panel
        active: window.open
        width: 440
        height: Math.min(implicitHeight, Math.max(0, parent.height - 64))
        anchors {
            top: parent.top
            right: parent.right
            topMargin: 48
            rightMargin: 24
        }

        y: window.open ? 0 : -12
        opacity: window.open ? 1 : 0
        Behavior on y { NumberAnimation { duration: 220; easing.type: Easing.OutCubic } }
        Behavior on opacity { NumberAnimation { duration: 180; easing.type: Easing.OutCubic } }
    }
}
