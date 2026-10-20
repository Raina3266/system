import QtQuick
import QtTest
import ".." as Components

TestCase {
    id: tests
    name: "NotificationHistory"
    width: 440
    height: 600
    visible: true
    when: windowShown

    QtObject {
        id: panel
        property var notifications: []
        property bool dndActive: false
        property int dismissedId: 0
        property string invokedAction: ""
        function toggleDnd() {}
        function clearNotifications() {}
        function dismissNotification(id) { dismissedId = id }
        function invokeNotificationAction(id, action) { invokedAction = id + ":" + action }
    }

    Component {
        id: sectionComponent
        Components.NotificationSection {
            width: 408
            height: implicitHeight
        }
    }

    function init() {
        panel.dismissedId = 0
        panel.invokedAction = ""
        panel.notifications = [
            { id: 1, app: "Test", summary: "First notification", body: "Visible body text" },
            { id: 2, app: "Test", summary: "Second notification", body: "" }
        ]
    }

    function historyIn(item) {
        if (item instanceof ListView)
            return item
        for (const child of item.children) {
            const history = historyIn(child)
            if (history)
                return history
        }
        return null
    }

    function createSection() {
        const section = createTemporaryObject(sectionComponent, tests, { panel: panel })
        verify(section !== null)
        return section
    }

    function test_rowsFillTheViewport() {
        const section = createSection()
        const history = historyIn(section)
        verify(history !== null)
        tryCompare(history, "count", 2)
        tryVerify(() => history.width > 0 && history.itemAtIndex(0) !== null)
        const row = history.itemAtIndex(0)
        compare(row.width, history.width)
        verify(row.height >= 64)
        compare(row.notification.summary, "First notification")
    }

    function test_rowsFollowResizing() {
        const section = createSection()
        const history = historyIn(section)
        tryVerify(() => history.itemAtIndex(0) !== null)
        section.width = 300
        tryCompare(history, "width", section.width - 24)
        tryCompare(history.itemAtIndex(0), "width", history.width)
    }

    function test_liveArrivalAfterEmptyHistory() {
        panel.notifications = []
        const section = createSection()
        const history = historyIn(section)
        tryCompare(history, "count", 0)
        panel.notifications = [
            { id: 3, app: "Test", summary: "Just arrived", body: "Live update" }
        ]
        tryCompare(history, "count", 1)
        tryVerify(() => history.visible && history.width > 0 && history.itemAtIndex(0) !== null)
        compare(history.itemAtIndex(0).width, history.width)
        verify(history.height > 0)
    }

    function test_dismissButtonUsesTheRowId() {
        const section = createSection()
        const history = historyIn(section)
        tryVerify(() => history.itemAtIndex(0) !== null)
        const row = history.itemAtIndex(0)
        const button = row.children.find(child => child.glyph === "󰅖")
        verify(button !== undefined)
        mouseClick(button, button.width / 2, button.height / 2)
        compare(panel.dismissedId, 1)
    }

    function actionIn(item, text) {
        if (item.text === text && typeof item.activated === "function")
            return item
        for (const child of item.children) {
            const action = actionIn(child, text)
            if (action)
                return action
        }
        return null
    }

    function test_nativeActionsAreVisibleAndClickable() {
        panel.notifications = [{
            id: 3, app: "Test", summary: "Action required", body: "Test body",
            actions: [{ identifier: "open", text: "Open details" }]
        }]
        const section = createSection()
        const history = historyIn(section)
        tryVerify(() => history.itemAtIndex(0) !== null)
        const row = history.itemAtIndex(0)
        tryVerify(() => actionIn(row, "Open details") !== null)
        const button = actionIn(row, "Open details")
        verify(button.visible && button.width > 0 && button.height > 0)
        mouseClick(button, button.width / 2, button.height / 2)
        compare(panel.invokedAction, "3:open")
    }
}
