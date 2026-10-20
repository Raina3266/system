import QtQuick
import QtQuick.Layouts

DashboardSection {
    id: section

    required property var metrics
    glyph: "󰍛"
    title: "SYSTEM PARAMETERS"

    MetricBar {
        label: "CPU"
        value: section.metrics.cpuPercent < 0 ? "—" : section.metrics.cpuPercent + "%"
        progress: section.metrics.cpuPercent
    }
    MetricBar {
        label: "MEMORY"
        value: section.metrics.memoryPercent < 0
            ? "—"
            : section.metrics.memoryUsedGiB.toFixed(1) + " / " + section.metrics.memoryTotalGiB.toFixed(1) + " GiB"
        progress: section.metrics.memoryPercent
    }
    MetricBar {
        label: "ROOT DISK"
        value: section.metrics.diskPercent < 0
            ? "—"
            : section.metrics.diskUsedGiB.toFixed(1) + " / " + section.metrics.diskTotalGiB.toFixed(1) + " GiB"
        progress: section.metrics.diskPercent
    }
    MetricBar {
        label: "CPU TEMP"
        value: section.metrics.cpuTemperatureC < 0
            ? "—"
            : Math.round(section.metrics.cpuTemperatureC) + " °C"
        progress: section.metrics.cpuTemperatureC
    }
}
