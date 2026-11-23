// Pure display formatting; custom policy stays in Rust.
export function emptyMetrics() {
    return { cpuPercent: -1, memoryPercent: -1, memoryUsedGiB: -1, memoryTotalGiB: -1,
        diskPercent: -1, diskUsedGiB: -1, diskTotalGiB: -1, cpuTemperatureC: -1 };
}

const BATTERY_ICONS = ['󰁺', '󰁻', '󰁼', '󰁽', '󰁾', '󰁿', '󰂀', '󰂁', '󰂂', '󰁹'];

export function formatBatteryStatus(battery, activeProfile) {
    const available = battery.ready && battery.present && Number.isFinite(battery.percentage) && battery.percentage >= 0;
    const percent = Math.round(battery.percentage * 100);
    const charging = battery.status === 'CHARGING' || battery.status === 'CONNECTED · CHARGING SOON';
    const full = battery.status === 'FULLY CHARGED';
    const icon = charging || full ? '󰂄' : BATTERY_ICONS[Math.min(9, Math.floor(percent / 10))];
    return {
        text: available ? `${icon} ${percent}%` : '󰕮',
        tooltip: available ? `${battery.status} | Profile: ${activeProfile} | Click: Dashboard` : 'Dashboard',
        class: !available ? 'clear' : charging ? 'charging' : full ? 'full'
            : percent <= 15 ? 'critical' : percent <= 25 ? 'warning' : percent < 50 ? 'low' : 'discharging',
    };
}

export const SCREEN_TIMEOUT_CHOICES = [
    { id: '0', label: 'Sleep after: None' },
    { id: '10', label: 'Sleep after: 10 min' },
    { id: '30', label: 'Sleep after: 30 min' },
    { id: '60', label: 'Sleep after: 1h' },
    { id: '120', label: 'Sleep after: 2h' },
    { id: '180', label: 'Sleep after: 3h' },
    { id: '360', label: 'Sleep after: 6h' },
];

export function parseRequest(text) {
    const request = JSON.parse(text);
    if (
        !request || typeof request !== 'object' || Array.isArray(request) ||
        !['status', 'toggle', 'close', 'toggleDnd', 'clearNotifications'].includes(request.command)
    ) throw new Error('Unknown dashboard command.');
    if (Object.keys(request).some((key) => !['command', 'monitor'].includes(key))) throw new Error('Unexpected dashboard request field.');
    if (
        Object.hasOwn(request, 'monitor') &&
        (request.command !== 'toggle' || typeof request.monitor !== 'string' || request.monitor.length > 256 ||
            /[\x00-\x1f\x7f]/.test(request.monitor))
    ) throw new Error('Monitor must be a connector name, only for toggle.');
    return request;
}
