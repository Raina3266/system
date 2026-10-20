// Resident panels. Quickshell owns native services and windows; custom media
// policy and metadata live in the Rust media-control helper.
import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Bluetooth
import Quickshell.Networking
import Quickshell.Wayland
import "MediaStatus.js" as MediaStatus

ShellRoot {
    id: root

    readonly property string audioControl: Quickshell.env("AUDIO_CONTROL") || "audio-control"
    readonly property string mediaControl: Quickshell.env("MEDIA_CONTROL") || "media-control"

    property string openScreen: ""
    property string dashboardOpenScreen: ""
    property alias groups: mediaController.groups
    property alias activeGroup: mediaController.activeGroup
    readonly property var streamNode: mediaController.streamNode
    readonly property var streamSink: mediaController.streamSink
    readonly property var routes: mediaController.routes
    readonly property bool moving: mediaController.moving
    readonly property bool canRoute: mediaController.canRoute
    readonly property string routingError: mediaController.routingError
    readonly property string routingStatus: mediaController.routingStatus
    readonly property var notifications: notificationCenter.history
    readonly property bool dndActive: notificationCenter.dnd
    property bool idleInhibitEnabled: false
    readonly property bool airplaneMode: {
        const adapters = Bluetooth.adapters.values
        return !Networking.wifiEnabled && adapters.every(adapter => !adapter.enabled)
    }

    MediaController {
        id: mediaController
        helper: root.mediaControl
        routingHelper: root.audioControl
        opened: root.openScreen !== ""
    }

    function mediaAction(kind, player, position) {
        mediaController.action(kind, player, position)
    }
    function selectGroup(group) { mediaController.selectGroup(group) }
    function moveStream(key) { mediaController.moveStream(key) }

    function togglePanel(monitor) {
        networkCenter.close()
        let name = monitor
        if (!name && Quickshell.screens.length > 0)
            name = Quickshell.screens[0].name
        if (!name)
            return
        openScreen = openScreen === name ? "" : name
    }

    function closePanel() { openScreen = "" }

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
        target: "media"
        // Native snapshots are refreshed asynchronously by the Rust bridge;
        // Waybar receives the latest completed model, including while closed.
        function status(): string {
            mediaController.refresh()
            return JSON.stringify(MediaStatus.badge(root.activeGroup))
        }
        function toggle(monitor: string): void { root.togglePanel(monitor) }
        function pauseAll(): void { mediaController.action("pauseAll") }
        function togglePauseAll(): void { mediaController.action("togglePauseAll") }
        function cycleMedia(): void { mediaController.cycle() }
    }

    IpcHandler {
        target: "dashboard"
        function toggle(monitor: string): void { root.toggleDashboard(monitor) }
    }

    IpcHandler {
        target: "network"
        function toggle(monitor: string): void {
            root.closePanel()
            root.closeDashboard()
            networkCenter.toggle(monitor)
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

    // Nudge native MPRIS position bindings each frame for a smooth seek bar.
    FrameAnimation {
        running: root.openScreen !== "" && root.activeGroup && root.activeGroup.primary.isPlaying
        onTriggered: {
            for (const player of root.activeGroup.players)
                player.positionChanged()
        }
    }

    Notifications { id: notificationCenter }
    NotificationPopups { center: notificationCenter }
    NetworkController { id: networkCenter }
    NetworkPanel { controller: networkCenter }

    Variants {
        model: Quickshell.screens
        // Variants inject each screen as the delegate's modelData property.
        MediaPanel { panel: root }
    }
    Variants {
        model: Quickshell.screens
        DashboardPanel { panel: root }
    }
    IdleInhibitorWindow { panel: root }

    // Each visual component creates its own Colors instance: ShellRoot creates
    // Variants before its own properties finish initializing.
}
