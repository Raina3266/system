import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Quickshell
import Quickshell.Io
import Quickshell.Wayland
import Quickshell.Bluetooth
import "shared" as Shared

ShellRoot {
    id: root

    property bool opened: false
    property string monitor: ""
    property int currentTab: 1
    readonly property string helper: Quickshell.env("AUDIO_CONTROL_HELPER") || "audio-control"

    function close() {
        opened = false;
    }

    IpcHandler {
        target: "panel"

        function toggle(monitor: string): void {
            if (root.opened && root.monitor === monitor) {
                root.close();
            } else {
                root.monitor = monitor;
                root.opened = true;
            }
        }

        function close(): void { root.close(); }

        function bluetoothPower(): void {
            if (bluetooth.adapter)
                bluetooth.adapter.enabled = !bluetooth.adapter.enabled;
        }
    }

    PanelWindow {
        id: window
        visible: root.opened
        screen: Quickshell.screens.find(s => s.name === root.monitor) || Quickshell.screens[0]
        anchors { top: true; bottom: true; left: true; right: true }
        color: "transparent"
        exclusionMode: ExclusionMode.Ignore
        WlrLayershell.layer: WlrLayer.Overlay
        WlrLayershell.keyboardFocus: WlrKeyboardFocus.Exclusive
        WlrLayershell.namespace: "audio-control"

        MouseArea {
            anchors.fill: parent
            onClicked: root.close()
        }

        Pane {
            id: panel
            readonly property var colors: Shared.Colors {}
            width: Math.min(400, parent.width - 16)
            height: Math.min(520, parent.height - 64)
            anchors {
                top: parent.top
                right: parent.right
                topMargin: 40
                rightMargin: Math.min(96, Math.max(8, (panel.parent.width - panel.width) / 2))
            }
            padding: 11
            focus: true
            Keys.onEscapePressed: root.close()
            font.family: "Noto Sans"
            font.pixelSize: 13
            palette {
                window: panel.colors.background
                windowText: panel.colors.accent
                base: panel.colors.surface
                alternateBase: panel.colors.elevated
                text: panel.colors.accent
                button: panel.colors.surface
                buttonText: panel.colors.accent
                highlight: panel.colors.primary
                highlightedText: panel.colors.background
                mid: panel.colors.muted
                dark: panel.colors.elevated
            }
            background: Shared.PopupFrame {
                // Consume inside clicks instead of dismissing the backdrop.
                MouseArea { anchors.fill: parent }
            }

            ColumnLayout {
                anchors.fill: parent
                spacing: 0

                RowLayout {
                    id: tabs
                    Layout.fillWidth: true
                    spacing: 0
                    readonly property real tabWidth: width / tabRepeater.count
                    Repeater {
                        id: tabRepeater
                        model: ["Bluetooth", "Audio"]
                        delegate: Shared.ControlButton {
                            required property int index
                            required property string modelData
                            Layout.fillWidth: true
                            Layout.minimumWidth: 0
                            Layout.preferredWidth: tabs.tabWidth
                            text: modelData.toUpperCase()
                            size: 40
                            tabStyle: true
                            checkable: true
                            autoExclusive: true
                            checked: root.currentTab === index
                            onActivated: root.currentTab = index
                        }
                    }
                }
                Item {
                    id: pageFrame
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    readonly property var selectedTab: tabRepeater.itemAt(root.currentTab)
                    readonly property real selectedTabLeft: selectedTab ? selectedTab.mapToItem(pageFrame, 0, 0).x : 0
                    readonly property real selectedTabRight: selectedTab ? selectedTab.mapToItem(pageFrame, selectedTab.width, 0).x : 0

                    Rectangle {
                        x: 0
                        y: 0
                        width: pageFrame.selectedTabLeft
                        height: 1
                        color: panel.colors.accent
                    }
                    Rectangle {
                        x: pageFrame.selectedTabRight
                        y: 0
                        width: pageFrame.width - x
                        height: 1
                        color: panel.colors.accent
                    }
                    Rectangle {
                        x: 0
                        y: 0
                        width: 1
                        height: pageFrame.height
                        color: panel.colors.accent
                    }
                    Rectangle {
                        x: pageFrame.width - width
                        y: 0
                        width: 1
                        height: pageFrame.height
                        color: panel.colors.accent
                    }
                    Rectangle {
                        x: 0
                        y: pageFrame.height - height
                        width: pageFrame.width
                        height: 1
                        color: panel.colors.accent
                    }
                    StackLayout {
                        anchors.fill: parent
                        anchors.margins: 12
                        currentIndex: root.currentTab
                        BluetoothPage {
                            id: bluetooth
                            active: root.opened && root.currentTab === 0
                        }
                        ColumnLayout {
                            spacing: 12
                            // Matching size hints keep output and input in equal halves.
                            AudioPage {
                                Layout.fillWidth: true
                                Layout.fillHeight: true
                                Layout.minimumHeight: 0
                                Layout.preferredHeight: 1
                                kind: "output"
                                helper: root.helper
                                active: root.opened && root.currentTab === 1
                            }
                            Rectangle {
                                Layout.fillWidth: true
                                implicitHeight: 1
                                color: panel.colors.muted
                            }
                            AudioPage {
                                Layout.fillWidth: true
                                Layout.fillHeight: true
                                Layout.minimumHeight: 0
                                Layout.preferredHeight: 1
                                kind: "input"
                                helper: root.helper
                                active: root.opened && root.currentTab === 1
                            }
                        }
                    }
                }
            }
        }
    }
}
