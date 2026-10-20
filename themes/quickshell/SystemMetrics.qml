import QtQuick
import Quickshell
import Quickshell.Io

// Read small kernel telemetry files directly in QML. Quickshell 0.3.1 has no
// general CPU/memory/disk monitor service; only filesystem capacity needs df.
Item {
    id: metrics

    required property bool active

    property int cpuPercent: -1
    property int memoryPercent: -1
    property real memoryUsedGiB: 0
    property real memoryTotalGiB: 0
    property int diskPercent: -1
    property real diskUsedGiB: 0
    property real diskTotalGiB: 0
    property real cpuTemperatureC: -1
    property real previousCpuTotal: -1
    property real previousCpuIdle: -1

    readonly property string dfCommand: Quickshell.env("COREUTILS_DF") || "df"

    function updateCpu(text) {
        const firstLine = text.split(/\r?\n/)[0] || ""
        const fields = firstLine.trim().split(/\s+/).slice(1).map(Number)
        if (fields.length < 4 || fields.some(value => !Number.isFinite(value)))
            return

        const idle = fields[3] + (fields[4] || 0)
        const total = fields.reduce((sum, value) => sum + value, 0)
        if (previousCpuTotal >= 0) {
            const totalDelta = total - previousCpuTotal
            const idleDelta = idle - previousCpuIdle
            if (totalDelta > 0)
                cpuPercent = Math.max(0, Math.min(100, Math.round((1 - idleDelta / totalDelta) * 100)))
        }
        previousCpuTotal = total
        previousCpuIdle = idle
    }

    function updateMemory(text) {
        const values = {}
        for (const line of text.split(/\r?\n/)) {
            const match = /^([A-Za-z_]+):\s+(\d+)/.exec(line)
            if (match)
                values[match[1]] = Number(match[2]) * 1024
        }

        const total = values.MemTotal || 0
        const available = values.MemAvailable
            || (values.MemFree || 0) + (values.Buffers || 0) + (values.Cached || 0)
        if (total <= 0)
            return

        const used = Math.max(0, total - available)
        memoryUsedGiB = used / (1024 * 1024 * 1024)
        memoryTotalGiB = total / (1024 * 1024 * 1024)
        memoryPercent = Math.round(used / total * 100)
    }

    function updateDisk(text) {
        const lines = text.trim().split(/\r?\n/)
        if (lines.length < 2) {
            diskPercent = -1
            return
        }

        const fields = lines[1].trim().split(/\s+/)
        const used = Number(fields[0])
        const total = Number(fields[1])
        const percent = Number.parseInt(fields[2], 10)
        if (!Number.isFinite(used) || !Number.isFinite(total) || total <= 0 || !Number.isFinite(percent)) {
            diskPercent = -1
            return
        }

        diskUsedGiB = used / (1024 * 1024 * 1024)
        diskTotalGiB = total / (1024 * 1024 * 1024)
        diskPercent = percent
    }

    function updateTemperature(text) {
        const value = Number(text.trim())
        if (!Number.isFinite(value) || value <= 0) {
            cpuTemperatureC = -1
            return
        }
        cpuTemperatureC = value > 1000 ? value / 1000 : value
    }

    function refresh() {
        cpuFile.reload()
        memoryFile.reload()
        temperatureFile.reload()
    }

    FileView {
        id: cpuFile
        path: "/proc/stat"
        printErrors: false
        onLoaded: metrics.updateCpu(text())
    }

    FileView {
        id: memoryFile
        path: "/proc/meminfo"
        printErrors: false
        onLoaded: metrics.updateMemory(text())
    }

    FileView {
        id: temperatureFile
        path: "/sys/class/thermal/thermal_zone0/temp"
        printErrors: false
        onLoaded: metrics.updateTemperature(text())
        onLoadFailed: metrics.cpuTemperatureC = -1
    }

    Process {
        id: diskProcess
        command: [metrics.dfCommand, "-B1", "--output=used,size,pcent", "/"]
        stdout: StdioCollector {
            onStreamFinished: metrics.updateDisk(this.text)
        }
        onExited: (exitCode, exitStatus) => {
            if (exitCode !== 0)
                metrics.diskPercent = -1
        }
    }

    Timer {
        interval: 2500
        repeat: true
        running: metrics.active
        triggeredOnStart: true
        onTriggered: metrics.refresh()
    }

    Timer {
        interval: 30000
        repeat: true
        running: metrics.active
        triggeredOnStart: true
        onTriggered: {
            if (!diskProcess.running)
                diskProcess.running = true
        }
    }

    onActiveChanged: {
        if (active)
            refresh()
    }
}
