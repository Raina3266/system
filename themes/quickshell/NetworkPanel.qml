import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Quickshell
import Quickshell.Wayland
import "shared" as Shared

// Presentation only: the controller owns all native operations and secret retrieval.
PanelWindow {
    id: window
    required property var controller
    readonly property var targetScreen: Quickshell.screens.find(s => s.name === controller.monitor)
        || Quickshell.screens[0] || null
    readonly property var colors: Shared.Colors {}
    property bool secretRequested: false
    property var profileChoices: ({})
    readonly property var detailFields: [
        { key: "profileId", label: "Profile" }, { key: "profileUuid", label: "Profile UUID" },
        { key: "ipv4", label: "IPv4" }, { key: "gateway4", label: "IPv4 gateway" },
        { key: "ipv6", label: "IPv6" }, { key: "gateway6", label: "IPv6 gateway" },
        { key: "dns", label: "DNS" }, { key: "mac", label: "MAC" },
        { key: "bssid", label: "BSSID", wireless: true },
        { key: "frequency", label: "Frequency", wireless: true },
        { key: "band", label: "Band", wireless: true },
        { key: "channel", label: "Channel", wireless: true },
        { key: "mode", label: "Mode", wireless: true },
        { key: "rate", label: "Link rate", wireless: true }
    ]

    screen: targetScreen
    visible: controller.opened && targetScreen !== null
    anchors { top: true; bottom: true; left: true; right: true }
    color: "transparent"
    exclusionMode: ExclusionMode.Ignore
    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.namespace: "quickshell:network"
    WlrLayershell.keyboardFocus: visible ? WlrKeyboardFocus.Exclusive : WlrKeyboardFocus.None

    function dismiss() {
        password.clear()
        secretRequested = false
        if (controller.credentialKey)
            controller.cancelCredentials()
        controller.hideSecret()
        controller.close()
    }

    function formatted(value) {
        if (value === null || value === undefined || value === "")
            return "—"
        if (Array.isArray(value))
            return value.map(formatted).join("\n")
        if (typeof value === "object")
            return Object.keys(value).map(key => key + ": " + formatted(value[key])).join("\n")
        return String(value)
    }

    onVisibleChanged: {
        password.clear()
        secretRequested = false
        if (visible)
            card.forceActiveFocus()
    }
    Connections {
        target: window.controller
        function onCredentialKeyChanged() { password.clear() }
        function onSelectedKeyChanged() { window.secretRequested = false }
        function onPageChanged() {
            if (window.controller.page !== "info")
                window.secretRequested = false
        }
    }

    component Label: Text {
        color: window.colors.accent
        font.family: "Noto Sans"
        font.pixelSize: 12
        textFormat: Text.PlainText
        wrapMode: Text.Wrap
    }

    component Action: Shared.ControlButton {
        size: 28
    }

    MouseArea {
        anchors.fill: parent
        onClicked: window.dismiss()
    }

    Pane {
        id: card
        width: Math.max(0, Math.min(420, parent.width - 16))
        height: Math.max(0, Math.min(520, parent.height - 54))
        anchors {
            top: parent.top
            right: parent.right
            topMargin: Math.min(46, Math.max(0, parent.height - card.height - 8))
            rightMargin: Math.min(72, Math.max(8, parent.width - card.width - 8))
        }
        padding: 12
        focus: true
        Keys.onEscapePressed: window.dismiss()
        font.family: "Noto Sans"
        font.pixelSize: 12
        palette {
            window: window.colors.background
            windowText: window.colors.accent
            base: window.colors.surface
            text: window.colors.accent
            button: window.colors.surface
            buttonText: window.colors.accent
            highlight: window.colors.primary
            highlightedText: window.colors.background
        }
        background: Shared.PopupFrame {
            MouseArea { anchors.fill: parent }
        }

        ColumnLayout {
            anchors.fill: parent
            spacing: 8
            RowLayout {
                Layout.fillWidth: true
                Label { text: "NETWORK"; font.bold: true; font.letterSpacing: 1; Layout.fillWidth: true }
                Action {
                    text: window.controller.wifiEnabled ? "Wi-Fi on" : "Wi-Fi off"
                    enabled: window.controller.wifiAvailable && window.controller.wifiHardwareEnabled
                    accent: window.controller.wifiEnabled
                    onActivated: window.controller.toggleWifi()
                }
                Shared.IconButton {
                    glyph: "↻"
                    size: 28
                    enabled: window.controller.wifiAvailable && window.controller.wifiEnabled
                    Accessible.name: "Refresh networks"
                    onActivated: window.controller.refresh()
                }
                Shared.IconButton {
                    glyph: "×"
                    size: 28
                    Accessible.name: "Close network panel"
                    onActivated: window.dismiss()
                }
            }
            Label {
                Layout.fillWidth: true
                visible: !window.controller.wifiAvailable || !window.controller.wifiHardwareEnabled || !window.controller.wifiEnabled
                text: !window.controller.wifiAvailable ? "No Wi-Fi adapter"
                    : !window.controller.wifiHardwareEnabled ? "Wi-Fi is hardware blocked" : "Wi-Fi is off"
                color: window.colors.warning
            }
            Label {
                Layout.fillWidth: true
                visible: text !== ""
                text: window.controller.error || ""
                color: window.colors.warning
                maximumLineCount: 3
                elide: Text.ElideRight
            }
            RowLayout {
                visible: window.controller.page !== "list"
                Action {
                    text: "Back"
                    onActivated: {
                        window.secretRequested = false
                        window.controller.hideSecret()
                        window.controller.back()
                    }
                }
                Label {
                    Layout.fillWidth: true
                    text: window.controller.selectedRow ? window.controller.selectedRow.name : ""
                    elide: Text.ElideRight
                    maximumLineCount: 1
                }
            }

            ListView {
                id: networks
                Layout.fillWidth: true
                Layout.fillHeight: true
                visible: window.controller.page === "list"
                clip: true
                spacing: 8
                model: window.controller.listModel
                ScrollBar.vertical: ScrollBar {}
                delegate: ColumnLayout {
                    id: entry
                    required property string rowKey
                    readonly property var modelData: window.controller.rowForKey(rowKey)
                        || { key: rowKey, name: "", interface: "", kind: "WI-FI", wireless: true,
                            connected: false, known: false, changing: true, strength: 0, security: "", profiles: [] }
                    readonly property var profiles: modelData.profiles || []
                    readonly property string profileUuid: profiles.some(profile => profile.uuid === window.profileChoices[modelData.key])
                        ? window.profileChoices[modelData.key] : ""
                    width: networks.width
                    spacing: 4
                    Label {
                        Layout.fillWidth: true
                        text: (entry.modelData.kind === "WIRED" ? "Wired · " : "Wi-Fi · ") + entry.modelData.name
                        font.bold: true
                    }
                    Label {
                        Layout.fillWidth: true
                        color: window.colors.metadata
                        text: [entry.modelData.connected ? "Connected" : entry.modelData.known ? "Saved" : "Not saved",
                            entry.modelData.interface,
                            entry.modelData.wireless ? entry.modelData.strength + "%" : "",
                            entry.modelData.security || (entry.modelData.wireless ? "Open" : ""),
                            entry.modelData.changing ? "Working…" : ""].filter(v => v).join(" · ")
                    }
                    // UUID selection is kept separate from translated/user-controlled IDs.
                    ComboBox {
                        Layout.fillWidth: true
                        visible: entry.profiles.length > 1
                        model: [{ id: "Automatic (default)", uuid: "" }].concat(entry.profiles)
                        textRole: "id"
                        currentIndex: entry.profiles.findIndex(profile => profile.uuid === entry.profileUuid) + 1
                        onActivated: {
                            const choices = Object.assign({}, window.profileChoices)
                            choices[entry.modelData.key] = currentIndex ? entry.profiles[currentIndex - 1].uuid : ""
                            window.profileChoices = choices
                        }
                        delegate: ItemDelegate {
                            required property var modelData
                            width: parent ? parent.width : 0
                            contentItem: Label { text: window.formatted(modelData.id) + " · " + modelData.uuid }
                        }
                        contentItem: Label {
                            text: parent.displayText
                            verticalAlignment: Text.AlignVCenter
                            elide: Text.ElideRight
                            maximumLineCount: 1
                        }
                    }
                    RowLayout {
                        Action {
                            text: entry.modelData.connected ? "Disconnect" : "Connect"
                            enabled: !entry.modelData.changing
                                && (!entry.modelData.wireless || (window.controller.wifiEnabled && window.controller.wifiHardwareEnabled))
                            onActivated: {
                                if (entry.modelData.connected)
                                    window.controller.disconnectNetwork(entry.modelData.key)
                                else if (entry.profiles.length)
                                    window.controller.connectNetwork(entry.modelData.key,
                                        entry.profileUuid)
                                else
                                    window.controller.connectNetwork(entry.modelData.key)
                            }
                        }
                        Action {
                            text: "Forget"
                            visible: entry.modelData.known
                            enabled: !entry.modelData.changing
                            onActivated: window.controller.requestForget(entry.modelData.key)
                        }
                        Action {
                            text: "Info"
                            visible: entry.modelData.connected
                            onActivated: window.controller.info(entry.modelData.key)
                        }
                        Action {
                            text: "QR"
                            visible: entry.modelData.connected && entry.modelData.wireless
                            onActivated: window.controller.qr(entry.modelData.key)
                        }
                    }
                    Rectangle { Layout.fillWidth: true; height: 1; color: window.colors.muted; opacity: 0.4 }
                }
                Label {
                    anchors.centerIn: parent
                    visible: networks.count === 0
                    text: "No networks available"
                }
            }

            // Never place credential state in a delegate: native row updates may recreate it.
            ColumnLayout {
                Layout.fillWidth: true
                visible: !!window.controller.credentialRow
                Label {
                    Layout.fillWidth: true
                    text: "Password · " + (window.controller.credentialRow ? window.controller.credentialRow.name : "")
                }
                TextField {
                    id: password
                    Layout.fillWidth: true
                    echoMode: TextInput.Password
                    placeholderText: "Network password"
                    selectByMouse: true
                    color: window.colors.accent
                    placeholderTextColor: window.colors.muted
                    selectionColor: window.colors.primary
                    selectedTextColor: window.colors.background
                    background: Rectangle {
                        radius: 3
                        color: Qt.alpha(window.colors.chrome, 0.8)
                        border.pixelAligned: false
                        border.width: password.activeFocus ? 1.5 : 0.35
                        border.color: password.activeFocus ? window.colors.warning : Qt.alpha(window.colors.text, 0.5)
                    }
                    onAccepted: submit.activated()
                }
                RowLayout {
                    Action {
                        id: submit
                        text: "Connect"
                        enabled: password.text.length > 0
                        onActivated: {
                            if (!window.controller.credentialRow)
                                return
                            window.controller.submitPassword(password.text)
                            password.clear()
                        }
                    }
                    Action {
                        text: "Cancel"
                        onActivated: {
                            password.clear()
                            window.controller.cancelCredentials()
                        }
                    }
                }
            }

            ColumnLayout {
                Layout.fillWidth: true
                visible: !!window.controller.forgetKey
                Label { Layout.fillWidth: true; text: "Forget ALL saved profiles for this network and disconnect it?" }
                RowLayout {
                    Action { text: "Forget"; onActivated: window.controller.confirmForget() }
                    Action { text: "Cancel"; onActivated: window.controller.cancelForget() }
                }
            }

            ScrollView {
                Layout.fillWidth: true
                Layout.fillHeight: true
                visible: window.controller.page === "info"
                clip: true
                contentWidth: availableWidth
                ColumnLayout {
                    width: parent.width
                    Label {
                        Layout.fillWidth: true
                        text: window.controller.detailsLoading ? "Loading details…" : ""
                        visible: text !== ""
                    }
                    Label {
                        Layout.fillWidth: true
                        text: window.controller.selectedRow
                            ? "SSID / name: " + window.formatted(window.controller.selectedRow.name)
                                + "\nInterface: " + window.formatted(window.controller.selectedRow.interface)
                                + "\nSecurity: " + window.formatted(window.controller.selectedRow.security)
                                + "\nSignal: " + window.formatted(window.controller.selectedRow.strength) + "%"
                            : ""
                    }
                    Label {
                        Layout.fillWidth: true
                        visible: window.controller.selectedRow && window.controller.selectedRow.profiles.length > 1
                        text: "Saved profiles (ID / UUID):\n" + window.formatted(
                            window.controller.selectedRow
                                ? window.controller.selectedRow.profiles.map(profile => profile.id + " (" + profile.uuid + ")") : [])
                    }
                    Repeater {
                        model: window.detailFields.filter(field => !field.wireless
                            || (window.controller.selectedRow && window.controller.selectedRow.wireless))
                        delegate: Label {
                            required property var modelData
                            Layout.fillWidth: true
                            text: modelData.label + ": " + window.formatted(window.controller.details[modelData.key])
                        }
                    }
                    RowLayout {
                        Action {
                            text: window.secretRequested ? "Hide password" : "Reveal password"
                            enabled: !window.controller.secretLoading
                                && !window.controller.detailsLoading
                                && !!window.controller.selectedRow && window.controller.selectedRow.wireless
                            onActivated: {
                                if (window.secretRequested) {
                                    window.secretRequested = false
                                    window.controller.hideSecret()
                                } else {
                                    window.secretRequested = true
                                    window.controller.revealSecret()
                                }
                            }
                        }
                        Label { visible: window.controller.secretLoading; text: "Loading…" }
                    }
                    Label {
                        Layout.fillWidth: true
                        visible: window.secretRequested && !!window.controller.secret
                        text: window.secretRequested ? window.controller.secret || "" : ""
                    }
                }
            }
            ColumnLayout {
                Layout.fillWidth: true
                Layout.fillHeight: true
                visible: window.controller.page === "qr"
                Label {
                    Layout.fillWidth: true
                    text: window.controller.qrLoading ? "Generating QR code…"
                        : qrImage.status === Image.Error ? "Could not display QR code"
                        : !window.controller.qrImage ? "QR code unavailable" : "Scan to join this network"
                }
                Item {
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    Image {
                        id: qrImage
                        anchors.centerIn: parent
                        width: Math.min(parent.width, parent.height)
                        height: width
                        source: window.controller.page === "qr" ? window.controller.qrImage : ""
                        cache: false
                        fillMode: Image.PreserveAspectFit
                        smooth: false
                    }
                }
            }
            Action {
                text: "Advanced…"
                onActivated: window.controller.openEditor()
            }
        }
    }
}
