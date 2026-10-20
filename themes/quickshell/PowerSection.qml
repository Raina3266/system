import QtQuick
import QtQuick.Layouts
import Quickshell.Services.UPower

DashboardSection {
    id: section

    required property var panel
    glyph: "󰚥"
    title: "POWER"

    function batteryStatus() {
        const battery = UPower.displayDevice
        if (!battery.ready)
            return "READING BATTERY STATUS"
        if (!battery.isPresent)
            return "NO BATTERY DETECTED"
        if (battery.state === UPowerDeviceState.Charging)
            return "CHARGING"
        if (battery.state === UPowerDeviceState.PendingCharge)
            return "CONNECTED · CHARGING SOON"
        if (battery.state === UPowerDeviceState.FullyCharged)
            return "FULLY CHARGED"
        if (UPower.onBattery)
            return "ON BATTERY"
        return "AC POWER"
    }

    RowLayout {
        Layout.fillWidth: true
        spacing: 10

        Text {
            text: section.batteryStatus() === "CHARGING" ? "󰂄" : "󰁹"
            color: section.colors.primary
            font.pixelSize: 23
        }
        ColumnLayout {
            Layout.fillWidth: true
            spacing: 2
            Text {
                text: "BATTERY"
                color: section.colors.muted
                font.pixelSize: 9
                font.bold: true
                font.letterSpacing: 1.2
            }
            Text {
                Layout.fillWidth: true
                text: section.batteryStatus()
                color: section.colors.accent
                font.pixelSize: 11
                elide: Text.ElideRight
            }
        }
        Text {
            // Quickshell exposes charge as a fraction, not UPower's 0–100 value.
            text: UPower.displayDevice.ready && UPower.displayDevice.isPresent
                ? Math.round(UPower.displayDevice.percentage * 100) + "%"
                : "—"
            color: section.colors.text
            font.pixelSize: 17
            font.family: "JetBrains Mono"
        }
    }

    RowLayout {
        Layout.fillWidth: true
        spacing: 8

        ActionTile {
            Layout.fillWidth: true
            glyph: "󰀝"
            label: "AIRPLANE"
            active: section.panel.airplaneMode
            onActivated: section.panel.toggleAirplaneMode()
        }
        ActionTile {
            Layout.fillWidth: true
            glyph: "󰒓"
            label: "IDLE INHIBIT"
            active: section.panel.idleInhibitEnabled
            onActivated: section.panel.toggleIdleInhibit()
        }
    }

    RowLayout {
        Layout.fillWidth: true
        spacing: 6

        ActionTile {
            Layout.fillWidth: true
            implicitWidth: 0
            glyph: "󰌪"
            label: "SAVER"
            active: PowerProfiles.profile === PowerProfile.PowerSaver
            onActivated: PowerProfiles.profile = PowerProfile.PowerSaver
        }
        ActionTile {
            Layout.fillWidth: true
            implicitWidth: 0
            glyph: "󰁹"
            label: "BALANCED"
            active: PowerProfiles.profile === PowerProfile.Balanced
            onActivated: PowerProfiles.profile = PowerProfile.Balanced
        }
        ActionTile {
            Layout.fillWidth: true
            implicitWidth: 0
            glyph: "󰓅"
            label: "PERFORMANCE"
            active: PowerProfiles.profile === PowerProfile.Performance
            enabled: PowerProfiles.hasPerformanceProfile
            onActivated: PowerProfiles.profile = PowerProfile.Performance
        }
    }
}
