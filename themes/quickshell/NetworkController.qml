import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Networking
import "NetworkFormat.js" as Format

Scope {
    id: center

    // Injectable for isolated tests; production always uses the native service.
    property var backend: Networking
    readonly property string helper: Quickshell.env("NETWORK_HELPER") || "network-manager"
    readonly property string editor: Quickshell.env("NETWORK_EDITOR") || "nm-connection-editor"
    property bool initialized: false
    property bool opened: false
    property string monitor: ""
    property string page: "list"
    property string error: ""
    property string selectedKey: ""
    property string credentialKey: ""
    property string credentialProfiles: ""
    property string forgetKey: ""
    property string pendingKey: ""
    property var details: ({})
    property string secret: ""
    property string qrImage: ""
    property bool detailsLoading: false
    property bool secretLoading: false
    property bool qrLoading: false
    property bool secretRequested: false
    property int requestSerial: 0
    property var wantedRequest: null
    property string statusKey: ""
    property var statusDetails: ({})
    property double statusTimestamp: 0
    property string wepPassword: ""

    readonly property bool wifiAvailable: wifiDevices.length > 0
    readonly property bool wifiEnabled: backend.wifiEnabled
    readonly property bool wifiHardwareEnabled: backend.wifiHardwareEnabled
    readonly property var devices: backend.devices.values
    readonly property var wifiDevices: devices.filter(device => device.type === DeviceType.Wifi)
    property alias listModel: networkModel
    readonly property var rows: {
        const result = []
        for (const device of devices) {
            if (device.type !== DeviceType.Wifi && device.type !== DeviceType.Wired)
                continue
            const wireless = device.type === DeviceType.Wifi
            for (const network of device.networks.values) {
                if (wireless && !network.name)
                    continue
                result.push({
                    key: JSON.stringify([device.name, network.name, wireless
                        ? network.security : network.nmSettings.map(profile => profile.uuid).join(",")]),
                    network: network,
                    name: network.name || (wireless ? "Hidden network" : device.name),
                    ssid: wireless ? network.name : "",
                    interface: device.name,
                    kind: wireless ? "WI-FI" : "WIRED",
                    wireless: wireless,
                    connected: network.connected,
                    changing: network.stateChanging,
                    known: network.known,
                    strength: wireless ? Math.round(network.signalStrength * 100) : 0,
                    security: wireless ? securityLabel(network.security) : "Wired",
                    profiles: network.nmSettings
                })
            }
        }
        return Format.sortRows(result)
    }
    readonly property var selectedRow: rowForKey(selectedKey)
    readonly property var credentialRow: rowForKey(credentialKey)
    readonly property var pendingRow: rowForKey(pendingKey)
    readonly property bool selectedConnected: selectedRow !== null && selectedRow.connected

    function rowForKey(key) {
        return rows.find(row => row.key === key) || null
    }

    function syncListModel() {
        for (let index = 0; index < rows.length; index++) {
            const key = rows[index].key
            if (index < networkModel.count && networkModel.get(index).rowKey === key)
                continue
            let found = -1
            for (let other = index; other < networkModel.count; other++) {
                if (networkModel.get(other).rowKey === key) {
                    found = other
                    break
                }
            }
            if (found === -1)
                networkModel.insert(index, { rowKey: key })
            else
                networkModel.move(found, index, 1)
        }
        if (networkModel.count > rows.length)
            networkModel.remove(rows.length, networkModel.count - rows.length)
    }

    function securityLabel(security) {
        switch (security) {
        case WifiSecurityType.Open: return "Open"
        case WifiSecurityType.StaticWep: return "WEP"
        case WifiSecurityType.DynamicWep: return "Dynamic WEP"
        case WifiSecurityType.WpaPsk: return "WPA"
        case WifiSecurityType.Wpa2Psk: return "WPA2"
        case WifiSecurityType.Sae: return "WPA3"
        case WifiSecurityType.WpaEap: return "WPA Enterprise"
        case WifiSecurityType.Wpa2Eap: return "WPA2 Enterprise"
        case WifiSecurityType.Wpa3SuiteB192: return "WPA3 Enterprise"
        case WifiSecurityType.Owe: return "Enhanced Open (OWE)"
        case WifiSecurityType.Leap: return "LEAP"
        default: return "Unknown"
        }
    }

    function acceptsPsk(row) {
        return row && [WifiSecurityType.WpaPsk, WifiSecurityType.Wpa2Psk, WifiSecurityType.Sae]
            .indexOf(row.network.security) !== -1
    }

    function acceptsPassword(row) {
        return acceptsPsk(row) || (row && row.network.security === WifiSecurityType.StaticWep)
    }

    function requestCredentials(row) {
        credentialProfiles = JSON.stringify(row.profiles.map(profile => profile.uuid))
        credentialKey = row.key
    }

    function toggle(name) {
        if (opened && monitor === name) {
            close()
            return
        }
        back()
        monitor = name
        opened = true
    }

    function close() {
        opened = false
        back()
    }

    function back() {
        requestSerial++
        wantedRequest = null
        if (detailProcess.running)
            detailProcess.running = false
        page = "list"
        selectedKey = ""
        credentialKey = ""
        credentialProfiles = ""
        forgetKey = ""
        pendingKey = ""
        wepPassword = ""
        details = ({})
        detailsLoading = false
        secretLoading = false
        qrLoading = false
        secretRequested = false
        secret = ""
        qrImage = ""
        error = ""
    }

    function toggleWifi() {
        if (!wifiHardwareEnabled) {
            error = "Wi-Fi is blocked by the hardware switch."
            return
        }
        backend.wifiEnabled = !wifiEnabled
        credentialKey = ""
        error = ""
    }

    function refresh() {
        error = ""
        for (const device of wifiDevices)
            device.scannerEnabled = false
        Qt.callLater(function() {
            if (center.opened && center.wifiEnabled && center.wifiHardwareEnabled) {
                for (const device of center.wifiDevices)
                    device.scannerEnabled = true
            }
        })
    }

    function connectNetwork(key, profileUuid) {
        const row = rowForKey(key)
        if (!row || row.changing || pendingKey
                || (row.wireless && (!wifiEnabled || !wifiHardwareEnabled)))
            return
        if (wepProcess.running) {
            error = "Wait for the current WEP connection attempt to finish."
            return
        }
        if (row.wireless && !row.known && !acceptsPassword(row)
                && row.network.security !== WifiSecurityType.Open && row.network.security !== WifiSecurityType.Owe) {
            error = "Create an authentication profile with Advanced before connecting to this network."
            return
        }
        error = ""
        pendingKey = key
        credentialKey = ""
        if (row.wireless && !row.known && acceptsPassword(row)) {
            requestCredentials(row)
            pendingKey = ""
            return
        }
        if (profileUuid) {
            const profile = row.profiles.find(profile => profile.uuid === profileUuid)
            if (!profile) {
                error = "The selected saved profile is no longer available."
                pendingKey = ""
                return
            }
            activateNative(row, function() { row.network.connectWithSettings(profile) })
        } else {
            // Try stored settings before requesting a password.
            activateNative(row, function() { row.network.connect() })
        }
    }

    function activateNative(row, action) {
        const serial = requestSerial
        nativeTimeout.restart()
        Qt.callLater(function() {
            if (center.pendingKey === row.key && center.requestSerial === serial
                    && center.rowForKey(row.key)
                    && (!row.wireless || (center.wifiEnabled && center.wifiHardwareEnabled)))
                action()
        })
    }

    function submitPassword(password) {
        const row = credentialRow
        if (!row || !acceptsPassword(row) || !password || wepProcess.running || pendingKey
                || !wifiEnabled || !wifiHardwareEnabled)
            return
        if (JSON.stringify(row.profiles.map(profile => profile.uuid)) !== credentialProfiles || row.profiles.length > 1) {
            cancelCredentials()
            error = "Saved profiles changed or need per-profile credentials. Use Advanced to update the selected profile."
            return
        }
        pendingKey = row.key
        error = ""
        if (acceptsPsk(row)) {
            activateNative(row, function() { row.network.connectWithPsk(password) })
        } else {
            // 0.3.1 has no native initial-WEP credential API.
            wepPassword = password
            wepProcess.requestKey = row.key
            wepProcess.serial = requestSerial
            wepProcess.command = [helper, "connect-wep", row.interface, row.ssid]
            wepProcess.stdinEnabled = true
            wepProcess.awaitingStart = true
            wepProcess.running = true
        }
        credentialKey = ""
    }

    function cancelCredentials() {
        credentialKey = ""
        credentialProfiles = ""
        pendingKey = ""
        error = ""
    }

    function disconnectNetwork(key) {
        const row = rowForKey(key)
        if (row && !row.changing)
            row.network.device.disconnect()
    }

    function requestForget(key) {
        const row = rowForKey(key)
        if (row && row.known) {
            back()
            forgetKey = key
        }
    }

    function confirmForget() {
        const row = rowForKey(forgetKey)
        forgetKey = ""
        if (row) {
            row.network.forget()
            if (row.connected)
                row.network.device.disconnect()
        }
    }

    function cancelForget() { forgetKey = "" }

    function openEditor() {
        close()
        Quickshell.execDetached([editor])
    }

    function connectionFailed(reason) {
        const row = pendingRow
        if (!row || wepProcess.running || wepProcess.awaitingStart)
            return
        const authenticationFailure = [
            ConnectionFailReason.NoSecrets, ConnectionFailReason.WifiAuthTimeout,
            ConnectionFailReason.WifiClientFailed, ConnectionFailReason.WifiClientDisconnected
        ].indexOf(reason) !== -1
        if (authenticationFailure && row.profiles.length > 1) {
            error = "This saved profile needs credentials. Use Advanced to update it without changing another profile."
        } else if (authenticationFailure && acceptsPassword(row) && opened) {
            requestCredentials(row)
            error = "A Wi-Fi password is required or was not accepted."
        } else {
            error = reason === ConnectionFailReason.NoSecrets
                ? "This network needs a configured authentication profile. Use Advanced to configure it."
                : "Connection failed: " + ConnectionFailReason.toString(reason)
        }
        pendingKey = ""
    }

    function info(key) {
        const row = rowForKey(key)
        if (!row || !row.connected)
            return
        back()
        selectedKey = key
        page = "info"
        queueRequest("info")
    }

    function qr(key) {
        const row = rowForKey(key)
        if (!row || !row.connected || !row.wireless)
            return
        back()
        selectedKey = key
        page = "qr"
        qrLoading = true
        queueRequest("qr-info")
    }

    function revealSecret() {
        if (selectedConnected && selectedRow.wireless && page === "info" && !detailsLoading) {
            secretRequested = true
            queueRequest("secret")
        }
    }

    function hideSecret() {
        secretRequested = false
        secret = ""
        secretLoading = false
        if (wantedRequest && wantedRequest.mode === "secret")
            wantedRequest = null
        if (detailProcess.running && detailProcess.request && detailProcess.request.mode === "secret")
            detailProcess.running = false
    }

    function queueRequest(mode) {
        if (!selectedConnected)
            return
        if (mode === "info") detailsLoading = true
        if (mode === "secret") secretLoading = true
        if (mode === "qr" || mode === "qr-info") qrLoading = true
        const sensitive = mode === "secret" || mode === "qr"
        if (sensitive && (!details.profileUuid || !details.bssid)) {
            secretLoading = false
            qrLoading = false
            error = "Read the current connection details before requesting credentials."
            return
        }
        wantedRequest = {
            mode: mode, page: page, key: selectedKey, serial: requestSerial,
            profileUuid: sensitive ? details.profileUuid : "",
            bssid: sensitive ? details.bssid : ""
        }
        Qt.callLater(center.runNextRequest)
    }

    function runNextRequest() {
        if (detailProcess.running || !wantedRequest)
            return
        const request = wantedRequest
        wantedRequest = null
        const row = rowForKey(request.key)
        if (!opened || !row || !row.connected || request.serial !== requestSerial)
            return
        detailProcess.request = request
        detailProcess.responseSeen = false
        detailProcess.responseError = ""
        detailProcess.awaitingStart = true
        detailProcess.command = [helper, request.mode === "qr-info" ? "info" : request.mode, row.interface, row.ssid]
            .concat(request.profileUuid ? [request.profileUuid, request.bssid] : [])
        detailProcess.running = true
    }

    function requestCurrent(request) {
        return request && opened && selectedConnected && request.key === selectedKey
            && request.serial === requestSerial && request.page === page
            && (request.mode !== "secret" || secretRequested)
    }

    function readResponse(line) {
        const request = detailProcess.request
        if (!requestCurrent(request))
            return
        detailProcess.responseSeen = true
        try {
            // SplitParser does not retain completed lines. Never store a raw
            // secret/QR response in a long-lived collector or request object.
            const result = JSON.parse(line)
            if (result.error) {
                detailProcess.responseError = String(result.error)
                return
            }
            if (result.interface !== selectedRow.interface || result.ssid !== selectedRow.ssid
                    || !result.profileUuid || (selectedRow.wireless && !result.bssid)
                    || (request.profileUuid && (result.profileUuid !== request.profileUuid || result.bssid !== request.bssid)))
                throw new Error("Connection identity changed")
            if (request.mode === "info" || request.mode === "qr-info") {
                details = result
                if (request.mode === "qr-info")
                    queueRequest("qr")
            } else if (request.mode === "secret" && typeof result.password === "string") {
                secret = result.password || "No password (open network)"
            } else if (request.mode === "qr" && typeof result.image === "string"
                       && result.image.startsWith("data:image/png;base64,")) {
                qrImage = result.image
            } else {
                throw new Error("Invalid payload")
            }
        } catch (error) {
            detailProcess.responseError = "The connection changed or its details could not be read."
        }
    }

    function finishRequest(failed) {
        const request = detailProcess.request
        if (requestCurrent(request)) {
            const errorMessage = detailProcess.responseError
                || (failed || !detailProcess.responseSeen ? "The network details request failed." : "")
            if (request.mode === "info") detailsLoading = false
            if (request.mode === "secret") secretLoading = false
            if (request.mode === "qr" || (request.mode === "qr-info" && errorMessage)) qrLoading = false
            if (errorMessage) {
                error = errorMessage
                secret = ""
                qrImage = ""
            }
        }
        detailProcess.request = null
        detailProcess.awaitingStart = false
        Qt.callLater(center.runNextRequest)
    }

    function status() {
        const row = Format.primary(rows)
        const key = row ? row.key : ""
        if (statusKey !== key) {
            statusKey = key
            statusDetails = ({})
            statusTimestamp = 0
        }
        if (row && Date.now() - statusTimestamp >= 5000 && !statusProcess.running) {
            statusProcess.requestKey = key
            statusProcess.command = [helper, "info", row.interface, row.ssid]
            statusTimestamp = Date.now()
            statusProcess.running = true
        }
        return JSON.stringify(Format.status(rows, statusDetails, devices.length > 0))
    }

    onSelectedConnectedChanged: {
        if (!selectedConnected && page !== "list") {
            back()
            error = "The connection changed. Reopen its details."
        }
    }
    onRowsChanged: {
        if (initialized)
            syncListModel()
        if (credentialKey && !credentialRow)
            cancelCredentials()
        if (pendingKey && !pendingRow) {
            if (opened)
                error = "The selected network is no longer available."
            pendingKey = ""
        }
        if (pendingRow && pendingRow.connected && !pendingRow.changing) {
            cancelCredentials()
            statusTimestamp = 0
        }
    }
    onMonitorChanged: if (initialized) back()
    onOpenedChanged: if (initialized && !opened) back()
    onPendingKeyChanged: if (!pendingKey) nativeTimeout.stop()
    onWifiEnabledChanged: if (initialized) back()
    onWifiHardwareEnabledChanged: if (initialized) back()
    Component.onCompleted: {
        initialized = true
        syncListModel()
    }

    ListModel {
        id: networkModel
    }

    Instantiator {
        model: center.wifiDevices
        delegate: Scope {
            required property var modelData
            Binding {
                target: modelData
                property: "scannerEnabled"
                when: center.initialized
                value: center.opened && center.wifiEnabled && center.wifiHardwareEnabled
            }
        }
    }

    Connections {
        target: center.pendingRow ? center.pendingRow.network : null
        function onConnectionFailed(reason) { center.connectionFailed(reason) }
        function onStateChangingChanged() {
            const key = center.pendingKey
            const serial = center.requestSerial
            if (center.pendingRow && !center.pendingRow.changing) {
                Qt.callLater(function() {
                    if (key && key === center.pendingKey && serial === center.requestSerial
                            && center.pendingRow && !center.pendingRow.changing
                            && !wepProcess.running) {
                        const connected = center.pendingRow.connected
                        center.pendingKey = ""
                        if (!connected && !center.error)
                            center.error = "The connection attempt ended."
                    }
                })
            }
        }
        function onConnectedChanged() {
            if (center.pendingRow && center.pendingRow.connected) {
                center.cancelCredentials()
                center.statusTimestamp = 0
            }
        }
    }

    Timer {
        id: nativeTimeout
        interval: 30000
        onTriggered: {
            if (center.pendingKey) {
                center.pendingKey = ""
                center.error = "The connection attempt timed out."
            }
        }
    }

    Process {
        id: wepProcess
        property string requestKey: ""
        property int serial: -1
        property bool awaitingStart: false
        function current() {
            return serial === center.requestSerial && center.opened && center.page === "list"
                && center.pendingKey === requestKey
        }
        function failed(message) {
            center.wepPassword = ""
            if (!current())
                return
            const row = center.rowForKey(requestKey)
            center.pendingKey = ""
            if (row && row.profiles.length <= 1)
                center.requestCredentials(row)
            center.error = message
        }
        stdout: StdioCollector {}
        stderr: StdioCollector { id: wepError }
        onStarted: {
            awaitingStart = false
            if (current())
                write(JSON.stringify({ password: center.wepPassword }) + "\n")
            center.wepPassword = ""
            stdinEnabled = false
        }
        onRunningChanged: {
            if (!running && awaitingStart) {
                awaitingStart = false
                failed("Could not start the legacy WEP helper.")
            }
        }
        onExited: (exitCode, exitStatus) => {
            center.wepPassword = ""
            if (exitCode !== 0 || exitStatus !== 0) {
                failed(wepError.text.trim() || "The WEP connection attempt failed.")
            } else if (current()) {
                center.pendingKey = ""
            }
        }
    }

    Timer {
        interval: 70000
        running: wepProcess.running
        onTriggered: {
            wepProcess.failed("The WEP connection attempt timed out.")
            wepProcess.running = false
            center.wepPassword = ""
        }
    }

    Process {
        id: detailProcess
        property var request: null
        property bool responseSeen: false
        property string responseError: ""
        property bool awaitingStart: false
        stdout: SplitParser {
            onRead: line => center.readResponse(line)
        }
        stderr: SplitParser {
            onRead: line => {
                if (!detailProcess.responseError)
                    detailProcess.responseError = line.trim()
            }
        }
        onStarted: awaitingStart = false
        onRunningChanged: {
            if (!running && awaitingStart) {
                responseError = "Could not start the network details helper."
                center.finishRequest(true)
            }
        }
        onExited: (exitCode, exitStatus) => center.finishRequest(exitCode !== 0 || exitStatus !== 0)
    }

    Timer {
        interval: 15000
        running: detailProcess.running
        onTriggered: {
            detailProcess.responseError = "The network details request timed out."
            if (center.requestCurrent(detailProcess.request))
                center.error = detailProcess.responseError
            detailProcess.running = false
        }
    }

    Process {
        id: statusProcess
        property string requestKey: ""
        stdout: StdioCollector { id: statusOutput }
        onExited: (exitCode, exitStatus) => {
            const current = Format.primary(center.rows)
            if (exitCode === 0 && exitStatus === 0 && current && current.key === requestKey) {
                try {
                    const result = JSON.parse(statusOutput.text)
                    if (result.interface === current.interface && result.ssid === current.ssid)
                        center.statusDetails = result
                } catch (error) { /* Keep native status even if extra details fail. */ }
            }
        }
    }
}
