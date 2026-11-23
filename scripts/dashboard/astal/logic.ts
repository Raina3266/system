// Shared wire types and pure display formatting; custom policy stays in Rust.
export type Metrics = {
    cpuPercent: number;
    memoryPercent: number;
    memoryUsedGiB: number;
    memoryTotalGiB: number;
    diskPercent: number;
    diskUsedGiB: number;
    diskTotalGiB: number;
    cpuTemperatureC: number;
};

export function emptyMetrics(): Metrics {
    return {
        cpuPercent: -1, memoryPercent: -1, memoryUsedGiB: -1, memoryTotalGiB: -1,
        diskPercent: -1, diskUsedGiB: -1, diskTotalGiB: -1, cpuTemperatureC: -1,
    };
}

export type NotificationRow = {
    token?: string;
    id: string;
    app: string;
    summary: string;
    body: string;
    icon: string;
    urgency: number;
    transient: boolean;
    timeout: number;
    actions: { identifier: string; text: string }[];
    closed: boolean;
};

export type Notice = { kind: 'init'; dnd: boolean; user: boolean }
    | { kind: 'received'; native: number; token: string; time: number; restore: boolean; row: NotificationRow }
    | { kind: 'resolved'; token: string; reason: number } | { kind: 'dnd' | 'seedAck'; value: boolean }
    | { kind: 'hover'; id: string; token: string; value: boolean } | { kind: 'action'; id: string; token: string; identifier: string }
    | { kind: 'dismiss'; id: string; token: string } | { kind: 'toggle' }
    | { kind: 'clear'; instances: { native: number; token: string }[] };
export type RequestFrame = { op: 'active'; active: boolean; generation: number } | { op: 'timeout'; minutes: number }
    | { op: 'notify'; command: Notice };
export type EventFrame = { event: 'offline' } | { event: 'toggle' } | { event: 'seed'; value: boolean }
    | { event: 'error'; code: string } | { event: 'timeout'; minutes: number }
    | { event: 'metrics'; generation: number; data: Metrics }
    | { event: 'notifications'; history: NotificationRow[]; popups: NotificationRow[] }
    | { event: 'native'; operation: 'expire' | 'invoke' | 'dismiss'; native: number; token: string; argument: string };
export interface Transport {
    send(frame: RequestFrame): void;
    whenDrained(callback: () => void): void;
    dispose(): void;
}

export type Palette = Record<'background' | 'secondaryBackground' | 'elevated' | 'foreground'
    | 'separator' | 'cyan' | 'pink' | 'dimPink' | 'yellow' | 'red', string>;

export interface NotificationController {
    setHovered(id: string, hovered: boolean, token?: string): void;
    dismissNotification(id: string, token?: string): void;
    invokeNotificationAction(id: string, identifier: string, token?: string): void;
}

export type BatterySnapshot = { ready: boolean; present: boolean; percentage: number; status: string };
const BATTERY_ICONS = ['󰁺', '󰁻', '󰁼', '󰁽', '󰁾', '󰁿', '󰂀', '󰂁', '󰂂', '󰁹'];

export function formatBatteryStatus(battery: BatterySnapshot, activeProfile: string) {
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
] as const;

export type DashboardRequest = {
    command: 'status' | 'toggle' | 'close' | 'toggleDnd' | 'clearNotifications';
    monitor?: string;
};

export function parseRequest(text: string): DashboardRequest {
    const request = JSON.parse(text);
    if (!request || typeof request !== 'object' || Array.isArray(request)
        || !['status', 'toggle', 'close', 'toggleDnd', 'clearNotifications'].includes(request.command))
        throw new Error('Unknown dashboard command.');
    if (Object.keys(request).some(key => !['command', 'monitor'].includes(key)))
        throw new Error('Unexpected dashboard request field.');
    if (Object.hasOwn(request, 'monitor') && (request.command !== 'toggle'
        || typeof request.monitor !== 'string' || request.monitor.length > 256
        || /[\x00-\x1f\x7f]/.test(request.monitor)))
        throw new Error('Monitor must be a connector name, only for toggle.');
    return request;
}
