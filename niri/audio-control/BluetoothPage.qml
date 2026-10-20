import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Quickshell
import Quickshell.Io
import Quickshell.Bluetooth
import "shared" as Shared

ColumnLayout {
    id: page
    readonly property var colors: Shared.Colors {}
    property bool active: false
    readonly property var adapter: Bluetooth.defaultAdapter
    readonly property var devices: adapter ? adapter.devices.values.slice().sort((a, b) => {
        const rank = d => d.connected ? 0 : d.paired ? 1 : 2;
        return rank(a) - rank(b) || a.name.localeCompare(b.name);
    }) : []
    property var pairingDevice: null
    property string pairingLog: ""
    spacing: 10

    onActiveChanged: {
        if (!active) {
            stopDiscovery();
            cancelPairing();
        }
    }

    function stopDiscovery() {
        if (adapter)
            adapter.discovering = false;
    }

    function cancelPairing() {
        if (pairingDevice)
            pairingDevice.cancelPair();
        pairing.running = false;
        pairingDevice = null;
    }

    function pair(device) {
        pairingLog = "";
        pairingDevice = device;
        pairing.command = [Quickshell.env("AUDIO_CONTROL_HELPER") || "audio-control", "bluetooth-pair", device.address];
        pairing.running = true;
    }

    // Quickshell has native BlueZ devices, but no PIN/passkey agent. Reuse the
    // existing agent on this process's pairing connection, never a competing
    // session-wide default agent. Its prompts stream directly into the card.
    Process {
        id: pairing
        stdinEnabled: true
        stdout: SplitParser { onRead: data => page.pairingLog += data + "\n" }
        stderr: SplitParser { onRead: data => page.pairingLog += data + "\n" }
        onExited: page.pairingDevice = null
    }

    Shared.ControlButton {
        Layout.alignment: Qt.AlignRight
        text: page.adapter?.discovering ? "Stop scan" : "Scan"
        enabled: page.adapter?.enabled ?? false
        onActivated: page.adapter.discovering = !page.adapter.discovering
    }
    Label {
        Layout.fillWidth: true
        visible: !page.adapter || !page.adapter.enabled
        text: page.adapter ? "Bluetooth is off." : "No Bluetooth adapter."
        color: page.colors.muted
        wrapMode: Text.Wrap
    }
    Rectangle {
        Layout.fillWidth: true
        visible: pairing.running || page.pairingLog !== ""
        implicitHeight: pairingContent.implicitHeight + 20
        radius: 3
        color: Qt.alpha(page.colors.chrome, 0.5)
        ColumnLayout {
            id: pairingContent
            anchors.fill: parent
            anchors.margins: 10
            ScrollView {
                Layout.fillWidth: true
                Layout.preferredHeight: Math.min(160, messages.implicitHeight)
                contentWidth: availableWidth
                clip: true
                Label {
                    id: messages
                    width: parent.width
                    text: page.pairingLog
                    color: page.colors.accent
                    wrapMode: Text.Wrap
                    textFormat: Text.PlainText
                }
            }
            RowLayout {
                visible: pairing.running
                TextField {
                    id: answer
                    Layout.fillWidth: true
                    placeholderText: "PIN/passkey or yes/no"
                    color: page.colors.accent
                    placeholderTextColor: page.colors.muted
                    selectionColor: page.colors.primary
                    selectedTextColor: page.colors.background
                    font.pixelSize: 13
                    background: Rectangle {
                        radius: 3
                        color: Qt.alpha(page.colors.chrome, 0.8)
                        border.pixelAligned: false
                        border.width: answer.activeFocus ? 1.5 : 0.35
                        border.color: answer.activeFocus ? page.colors.warning : Qt.alpha(page.colors.text, 0.5)
                    }
                    onAccepted: submit.clicked()
                }
                Shared.ControlButton {
                    id: submit
                    text: "Send"
                    enabled: answer.text.trim() !== ""
                    onActivated: {
                        pairing.write(answer.text + "\n");
                        answer.clear();
                    }
                }
                Shared.ControlButton { text: "Cancel"; onActivated: page.cancelPairing() }
            }
        }
    }
    ScrollView {
        Layout.fillWidth: true
        Layout.fillHeight: true
        clip: true
        contentWidth: availableWidth
        ColumnLayout {
            width: parent.width
            spacing: 6
            Repeater {
                model: page.devices
                delegate: Rectangle {
                    id: row
                    required property var modelData
                    Layout.fillWidth: true
                    implicitHeight: deviceContent.implicitHeight + 12
                    radius: 3
                    color: row.modelData.connected ? Qt.alpha(page.colors.dimPrimary, 0.35) : "transparent"
                    RowLayout {
                        id: deviceContent
                        anchors.fill: parent
                        anchors.margins: 6
                        ColumnLayout {
                            Layout.fillWidth: true
                            Label {
                                Layout.fillWidth: true
                                text: row.modelData.name || row.modelData.address
                                textFormat: Text.PlainText
                                elide: Text.ElideRight
                                font.bold: true
                                color: page.colors.accent
                            }
                            Label {
                                Layout.fillWidth: true
                                text: (row.modelData.connected ? "Connected" : row.modelData.paired ? "Paired" : "Available")
                                    + (row.modelData.batteryAvailable ? " · " + Math.round(row.modelData.battery * 100) + "%" : "")
                                color: page.colors.metadata
                                font.pixelSize: 11
                            }
                        }
                        Shared.ControlButton {
                            text: row.modelData.connected ? "Disconnect" : row.modelData.paired ? "Connect" : "Pair"
                            enabled: page.adapter.enabled && !pairing.running
                            onActivated: {
                                if (row.modelData.connected)
                                    row.modelData.disconnect();
                                else if (row.modelData.paired)
                                    row.modelData.connect();
                                else
                                    page.pair(row.modelData);
                            }
                        }
                        Shared.IconButton {
                            glyph: "×"
                            size: 26
                            enabled: !pairing.running
                            onActivated: row.modelData.forget()
                            ToolTip.visible: hovered
                            ToolTip.text: "Forget " + row.modelData.name
                        }
                    }
                }
            }
        }
    }
}
