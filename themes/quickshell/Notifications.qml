import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Services.Notifications
import "NotificationStore.js" as Store

// One owner for the native notification protocol, popup state, and history.
Scope {
    id: center

    readonly property int historyLimit: 200
    readonly property int popupLimit: 5
    readonly property int defaultTimeout: 8000
    readonly property string historyPath: Quickshell.env("NOTIFICATION_HISTORY")
        || (Quickshell.env("XDG_STATE_HOME") || Quickshell.env("HOME") + "/.local/state")
        + "/quickshell/notifications.json"
    property var history: []
    property var popups: []
    property bool dnd: false
    property var hoveredIds: []
    property bool initialized: false

    PersistentProperties {
        id: session
        reloadableId: "notification-session"
        // Keep native IDs stable on QML reload, without reusing archive keys
        // when the OS eventually recycles a PID after a process restart.
        property string nonce: Date.now().toString(36) + "-" + Math.random().toString(36).slice(2)
    }

    function keyFor(id) {
        return session.nonce + ":" + id
    }

    function received(row, replayed) {
        if (row.transient)
            history = history.filter(existing => existing.id !== row.id)
        else if (replayed && history.some(existing => existing.id === row.id))
            history = history.map(existing => existing.id === row.id ? row : existing)
        else
            history = Store.newestFirst(history, row, historyLimit)
        if (!dnd && !replayed)
            popups = Store.newestFirst(popups, row, popupLimit)
        Qt.callLater(center.prune)
    }

    function prune() {
        // A native object must remain reachable through history or a popup.
        // Defer until model insertion/replacement has finished before closing
        // objects evicted by the limits, including hidden transient alerts.
        const retained = new Set(history.concat(popups).map(row => row.id))
        for (const notification of server.trackedNotifications.values.slice()) {
            if (!retained.has(keyFor(notification.id)))
                notification.expire()
        }
    }

    function closed(id, reason) {
        popups = popups.filter(row => row.id !== id)
        setHovered(id, false)
        if (reason === NotificationCloseReason.Expired)
            history = history.map(row => row.id === id ? Store.archived(row) : row)
        else
            history = history.filter(row => row.id !== id)
    }

    function nativeNotification(id) {
        return server.trackedNotifications.values.find(notification => keyFor(notification.id) === id)
    }

    function dismissNotification(id) {
        const notification = nativeNotification(id)
        if (notification)
            notification.dismiss()
        history = history.filter(row => row.id !== id)
        popups = popups.filter(row => row.id !== id)
    }

    function clearNotifications() {
        // Copy before dismissing: each dismissal updates the native model.
        for (const notification of server.trackedNotifications.values.slice())
            notification.dismiss()
        history = []
        popups = []
    }

    function invokeNotificationAction(id, identifier) {
        const notification = nativeNotification(id)
        if (!notification)
            return
        const action = notification.actions.find(action => action.identifier === identifier)
        if (action)
            action.invoke()
    }

    function toggleDnd() {
        dnd = !dnd
    }

    function setHovered(id, hovered) {
        if (hovered && hoveredIds.indexOf(id) === -1)
            hoveredIds = hoveredIds.concat([id])
        else if (!hovered)
            hoveredIds = hoveredIds.filter(existing => existing !== id)
    }

    function persist() {
        if (initialized)
            stateFile.setText(JSON.stringify({
                version: 1,
                dnd: dnd,
                history: history.map(Store.archived)
            }))
    }

    // The archive is bounded and small. Complete atomic writes before a new
    // QML generation can read it; delayed writes can restore stale DND/rows.
    onHistoryChanged: if (initialized) persist()
    onDndChanged: {
        if (dnd) {
            popups = []
            Qt.callLater(center.prune)
        }
        if (initialized)
            persist()
    }

    FileView {
        id: stateFile
        path: center.historyPath
        blockLoading: true
        blockWrites: true
        printErrors: false
        onSaveFailed: error => console.warn("notifications: could not save history:", error)
    }

    Component.onCompleted: {
        try {
            const state = Store.restore(stateFile.text(), historyLimit)
            // Read the latest completed write before the native server
            // replays live notifications, updating those rows in place.
            history = state.history
            dnd = state.dnd
        } catch (error) {
            console.warn("notifications: could not restore history:", error)
        }
        initialized = true
    }

    NotificationServer {
        id: server
        keepOnReload: true
        actionsSupported: true
        bodySupported: true
        bodyMarkupSupported: false
        imageSupported: true
        persistenceSupported: true
        onNotification: notification => notification.tracked = true
    }

    Instantiator {
        model: server.trackedNotifications

        delegate: QtObject {
            id: tracker
            required property var modelData
            property bool initialized: false
            readonly property string key: center.keyFor(modelData.id)
            readonly property var snapshot: ({
                id: key,
                app: modelData.appName,
                summary: modelData.summary,
                body: modelData.body,
                icon: modelData.image || (modelData.appIcon ? Quickshell.iconPath(modelData.appIcon) : ""),
                urgency: modelData.urgency,
                transient: modelData.transient,
                timeout: modelData.expireTimeout,
                actions: modelData.actions.map(action => ({ identifier: action.identifier, text: action.text })),
                closed: false
            })

            onSnapshotChanged: if (initialized) updateTimer.restart()
            Component.onCompleted: {
                center.received(snapshot, modelData.lastGeneration)
                initialized = true
            }

            property Connections lifecycle: Connections {
                target: tracker.modelData
                function onClosed(reason) { center.closed(tracker.key, reason) }
            }

            property Timer updateTimer: Timer {
                interval: 0
                onTriggered: {
                    center.received(tracker.snapshot, false)
                    if (lifetime.running)
                        lifetime.restart()
                }
            }

            property Timer lifetime: Timer {
                // Quickshell 0.3.1 exposes the protocol's milliseconds here
                // (despite its reference describing this property as seconds).
                interval: tracker.modelData.expireTimeout > 0
                    ? tracker.modelData.expireTimeout : center.defaultTimeout
                running: tracker.initialized
                    && tracker.modelData.expireTimeout !== 0
                    && tracker.modelData.urgency !== NotificationUrgency.Critical
                    && center.hoveredIds.indexOf(tracker.key) === -1
                onTriggered: tracker.modelData.expire()
            }
        }
    }
}
