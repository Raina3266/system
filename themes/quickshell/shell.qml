// Resident dashboard, network, and notifications. Media runs separately in AGS.
import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Bluetooth
import Quickshell.Networking

ShellRoot {
    id: root

    readonly property string mediaPanel: Quickshell.env("MEDIA_PANEL") || "media-panel"

    property string dashboardOpenScreen: ""
    readonly property var notifications: notificationCenter.history
    readonly property bool dndActive: notificationCenter.dnd
    property bool idleInhibitEnabled: false
    readonly property bool airplaneMode: {
        const adapters = Bluetooth.adapters.values
        return !Networking.wifiEnabled && adapters.every(adapter => !adapter.enabled)
    }

    function toggleDashboard(monitor) {
        networkCenter.close()
        let name = monitor
        if (!name && Quickshell.screens.length > 0)
            name = Quickshell.screens[0].name
        if (!name)
            return
        dashboardOpenScreen = dashboardOpenScreen === name ? "" : name
    }

    function closeDashboard() { dashboardOpenScreen = "" }

    function toggleAirplaneMode() {
        const enableRadios = airplaneMode
        Networking.wifiEnabled = enableRadios
        for (const adapter of Bluetooth.adapters.values)
            adapter.enabled = enableRadios
    }

    function toggleIdleInhibit() { idleInhibitEnabled = !idleInhibitEnabled }
    function toggleDnd() { notificationCenter.toggleDnd() }
    function clearNotifications() { notificationCenter.clearNotifications() }
    function dismissNotification(id) { notificationCenter.dismissNotification(id) }
    function invokeNotificationAction(id, identifier) {
        notificationCenter.invokeNotificationAction(id, identifier)
    }

    IpcHandler {
        target: "dashboard"
        function toggle(monitor: string): void { root.toggleDashboard(monitor) }
    }

    IpcHandler {
        target: "network"
        function toggle(monitor: string): void {
            root.closeDashboard()
            networkCenter.toggle(monitor)
            if (networkCenter.opened && !closeMediaProcess.running)
                closeMediaProcess.running = true
        }
        function close(): void { networkCenter.close() }
        function status(): string { return networkCenter.status() }
    }

    IpcHandler {
        target: "notifications"
        function toggleDnd(): void { notificationCenter.toggleDnd() }
        function status(): string {
            return JSON.stringify({
                historyCount: notificationCenter.history.length,
                popupCount: notificationCenter.popups.length,
                dnd: notificationCenter.dnd
            })
        }
    }

    // Cross-runtime dismissal is an action, not part of polled status.
    Process {
        id: closeMediaProcess
        command: [root.mediaPanel, "close"]
    }

    Notifications { id: notificationCenter }
    NotificationPopups { center: notificationCenter }
    NetworkController { id: networkCenter }
    NetworkPanel { controller: networkCenter }

    Variants {
        model: Quickshell.screens
        DashboardPanel { panel: root }
    }
    IdleInhibitorWindow { panel: root }

    // Each visual component creates its own Colors instance: ShellRoot creates
    // Variants before its own properties finish initializing.
}
