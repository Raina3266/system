import GLib from 'gi://GLib';
import Gio from 'gi://Gio';
import GObject from 'gi://GObject';
import Gtk from 'gi://Gtk?version=4.0';
import Gdk from 'gi://Gdk?version=4.0';
import Astal from 'gi://Astal?version=4.0';
import AstalIO from 'gi://AstalIO';
import System from 'system';
import Battery from 'gi://AstalBattery';
import PowerProfiles from 'gi://AstalPowerProfiles';
import Network from 'gi://AstalNetwork';
import Bluetooth from 'gi://AstalBluetooth';
import { MetricBar, MetricsSampler, type Metrics } from './parameters.ts';
import {
    NotificationCenter, NotificationPopups, NotificationStack, CutCornerFrame, Section,
    connectedMonitors, box, ellipsis, iconButton, invoke, label, loadTheme, scroll, style,
    type Palette,
} from './notification.ts';

const BATTERY_ICONS = ['󰁺', '󰁻', '󰁼', '󰁽', '󰁾', '󰁿', '󰂀', '󰂁', '󰂂', '󰁹'];

export class DashboardBackend {
    readonly notifications: NotificationCenter;
    battery = { ready: false, present: false, percentage: -1, status: 'READING BATTERY STATUS' };
    airplaneMode = false;
    profiles: string[] = [];
    activeProfile = '';
    private batteryDevice = Battery.get_default();
    private upower = new Battery.UPower();
    private power = PowerProfiles.get_default();
    private network = Network.get_default();
    private bluetooth = Bluetooth.get_default();
    private sampler: MetricsSampler;
    private listeners = new Set<() => void>();
    private signals: [any, number][] = [];
    private radioSignals: [any, number][] = [];
    private unsubscribeNotifications: () => void;
    private disposed = false;

    constructor() {
        this.sampler = new MetricsSampler(() => this.emit());
        this.notifications = new NotificationCenter();
        this.unsubscribeNotifications = this.notifications.subscribe(() => this.emit());
        const refresh = () => this.refresh();
        this.watch(this.batteryDevice, 'notify', refresh);
        this.watch(this.upower, 'notify', refresh);
        this.watch(this.power, 'notify::profiles', refresh);
        this.watch(this.power, 'notify::active-profile', refresh);
        this.watch(this.network, 'notify::client', () => this.watchRadios());
        this.watch(this.bluetooth, 'notify::adapters', () => this.watchRadios());
        this.watchRadios();
    }

    get metrics(): Metrics { return this.sampler.data; }

    subscribe(callback: () => void): () => void {
        if (this.disposed) return () => {};
        this.listeners.add(callback);
        return () => this.listeners.delete(callback);
    }

    private watch(object: any, signal: string, callback: () => void, radios = false) {
        if (object) (radios ? this.radioSignals : this.signals).push([object, object.connect(signal, callback)]);
    }

    private watchRadios() {
        for (const [object, signal] of this.radioSignals) object.disconnect(signal);
        this.radioSignals = [];
        this.watch(this.network.client, 'notify::wireless-enabled', () => this.refresh(), true);
        for (const adapter of this.bluetooth.adapters || [])
            this.watch(adapter, 'notify::powered', () => this.refresh(), true);
        this.refresh();
    }

    private refresh() {
        if (this.disposed) return;
        const device = this.batteryDevice;
        const ready = Boolean(device && this.upower.daemon_version);
        const present = ready && device.is_present;
        let status = 'READING BATTERY STATUS';
        if (ready) {
            if (!present) status = 'NO BATTERY DETECTED';
            else if (device.state === Battery.State.CHARGING) status = 'CHARGING';
            else if (device.state === Battery.State.PENDING_CHARGE) status = 'CONNECTED · CHARGING SOON';
            else if (device.state === Battery.State.FULLY_CHARGED) status = 'FULLY CHARGED';
            else if (this.upower.on_battery) status = 'ON BATTERY';
            else status = 'AC POWER';
        }
        this.battery = { ready, present, status,
            percentage: present && Number.isFinite(device.percentage) ? Math.max(0, Math.min(1, device.percentage)) : -1 };
        this.airplaneMode = !this.network.client?.wireless_enabled
            && (this.bluetooth.adapters || []).every((adapter: Bluetooth.Adapter) => !adapter.powered);
        this.profiles = (this.power.profiles || []).map((profile: PowerProfiles.Profile) => profile.profile)
                    .filter((profile: string) => typeof profile === 'string' && profile.length > 0);
        this.activeProfile = this.power.active_profile || '';
        this.emit();
    }

    private emit() {
        if (!this.disposed) for (const callback of this.listeners) callback();
    }

    setActive(active: boolean) {
        if (!this.disposed) this.sampler.setActive(Boolean(active));
    }

    toggleAirplaneMode() {
        if (this.disposed) return;
        const enabled = this.airplaneMode;
        // NM's global client property works even without a selected wifi device.
        if (this.network.client) this.network.client.wireless_enabled = enabled;
        // AstalBluetooth.toggle() only changes the default adapter; use all of them.
        for (const adapter of this.bluetooth.adapters || []) adapter.powered = enabled;
    }

    setProfile(profile: string) {
        if (this.disposed) return;
        if (!this.profiles.includes(profile)) throw new Error(`Unavailable power profile: ${profile}`);
        this.power.active_profile = profile;
    }

    status() {
        const historyCount = this.notifications.history.length;
        const popupCount = this.notifications.popups.length;
        const dnd = this.notifications.dnd;
        const battery = this.battery;
        const available = battery.ready && battery.present && Number.isFinite(battery.percentage) && battery.percentage >= 0;
        const percent = Math.round(battery.percentage * 100);
        const charging = battery.status === 'CHARGING' || battery.status === 'CONNECTED · CHARGING SOON';
        const full = battery.status === 'FULLY CHARGED';
        const icon = charging || full ? '󰂄' : BATTERY_ICONS[Math.min(9, Math.floor(percent / 10))];
        return {
            text: available ? `${icon} ${percent}%` : '󰕮',
            tooltip: available ? `${battery.status} | Profile: ${this.activeProfile} | Click: Dashboard` : 'Dashboard',
            class: !available ? 'clear' : charging ? 'charging' : full ? 'full'
                : percent <= 15 ? 'critical' : percent <= 25 ? 'warning' : percent < 50 ? 'low' : 'discharging',
            historyCount, popupCount, dnd, airplaneMode: this.airplaneMode,
            activeProfile: this.activeProfile,
        };
    }

    dispose() {
        if (this.disposed) return;
        this.disposed = true;
        this.sampler.dispose();
        this.unsubscribeNotifications();
        this.notifications.dispose();
        for (const [object, signal] of [...this.signals, ...this.radioSignals]) object.disconnect(signal);
        this.signals = [];
        this.radioSignals = [];
        this.listeners.clear();
    }
}

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

export class ActionTile {
    readonly widget: Gtk.Button;

    constructor(glyph: string, text: string, clicked: () => unknown) {
        const icon = label(glyph, ['tile-glyph']);
        const caption = label(text, ['tile-caption']);
        icon.xalign = caption.xalign = 0.5;
        const content = box(true, 3, [icon, caption]);
        content.halign = content.valign = Gtk.Align.CENTER;
        this.widget = new Gtk.Button({ child: content, css_classes: ['action-tile'],
            height_request: 62, hexpand: true, tooltip_text: text });
        this.widget.connect('clicked', () => invoke(clicked));
    }

    update(active: boolean, available = true): void {
        style(this.widget, 'active', active);
        this.widget.sensitive = available;
    }
}

const CARD_WIDTH = 440;
const CARD_TOP = 48;
const CARD_RIGHT = 24;
const CARD_PADDING = 16;
const CARD_GLOW = 12;


export function monitorFor(name?: string): Gdk.Monitor | null {
    const monitors = connectedMonitors();
    return monitors.find(monitor => monitor.get_connector() === name) || monitors[0] || null;
}

function sameMonitor(left: Gdk.Monitor | null, right: Gdk.Monitor | null): boolean {
    return left === right || Boolean(left && right && left.get_connector()
        && left.get_connector() === right.get_connector());
}

export function closeNetwork(): void {
    try {
        AstalIO.Process.exec_asyncv([GLib.getenv('NETWORK_PANEL') || 'network-panel', 'close'], (_process, result) => {
            try { AstalIO.Process.exec_asyncv_finish(result); }
            catch (error) { console.warn(`Could not dismiss the network panel: ${error.message || error}`); }
        });
    } catch (error) { console.warn(`Could not dismiss the network panel: ${error.message || error}`); }
}

export class DashboardPanel {
    readonly window: Astal.Window;
    readonly root: Gtk.Overlay;
    readonly card: Gtk.Overlay;
    readonly viewport: Gtk.ScrolledWindow;
    readonly content: Gtk.Box;
    readonly airplane: ActionTile;
    readonly profiles = new Map<string, ActionTile>();
    readonly metrics: Record<'cpu' | 'memory' | 'disk' | 'temperature', MetricBar>;
    readonly history: NotificationStack;
    readonly archive: Gtk.ScrolledWindow;
    readonly historyCount: Gtk.Label;
    readonly empty: Gtk.Label;
    readonly dnd: Gtk.Button;
    readonly clear: Gtk.Button;
    readonly batteryGlyph: Gtk.Label;
    readonly batteryStatus: Gtk.Label;
    readonly batteryPercentage: Gtk.Label;
    opened = false;
    monitor: Gdk.Monitor | null = null;
    private animation = 0;
    private offset = -12;

    constructor({ app, backend, palette }: { app: Astal.Application; backend: DashboardBackend; palette: Palette }) {
        this.backend = backend;
        this.window = new Astal.Window({ application: app, name: 'dashboard-panel', namespace: 'astal:dashboard',
            anchor: Astal.WindowAnchor.TOP | Astal.WindowAnchor.BOTTOM | Astal.WindowAnchor.LEFT | Astal.WindowAnchor.RIGHT,
            exclusivity: Astal.Exclusivity.IGNORE, layer: Astal.Layer.OVERLAY,
            keymode: Astal.Keymode.NONE, visible: false, css_classes: ['dashboard-ui', 'dashboard-panel'] });
        this.window.connect('close-request', () => { this.close(); return true; });
        const keys = new Gtk.EventControllerKey({ propagation_phase: Gtk.PropagationPhase.CAPTURE });
        keys.connect('key-pressed', (_controller, key) => {
            if (!this.opened || key !== Gdk.KEY_Escape) return false;
            this.close();
            return true;
        });
        this.window.add_controller(keys);
        this.root = new Gtk.Overlay({ hexpand: true, vexpand: true, focusable: true });
        this.root.set_child(new Gtk.Box({ hexpand: true, vexpand: true }));
        this.window.set_child(this.root);
        this.window.connect('map', () => { if (this.opened) this.root.grab_focus(); });

        const power = new Section('󰚥', 'POWER');
        this.batteryGlyph = label('󰁹', ['battery-glyph']);
        this.batteryStatus = ellipsis('', ['battery-status']);
        this.batteryPercentage = label('—', ['battery-percentage']);
        power.append(box(false, 10, [this.batteryGlyph,
            box(true, 2, [label('BATTERY', ['battery-label']), this.batteryStatus]), this.batteryPercentage]));
        this.airplane = new ActionTile('󰀝', 'AIRPLANE', () => backend.toggleAirplaneMode());
        power.append(this.airplane.widget);
        const profileRow = box(false, 6);
        profileRow.homogeneous = true;
        for (const [profile, glyph, title] of [
            ['power-saver', '󰌪', 'SAVER'], ['balanced', '󰁹', 'BALANCED'], ['performance', '󰓅', 'PERFORMANCE'],
        ]) {
            const tile = new ActionTile(glyph, title, () => backend.setProfile(profile));
            this.profiles.set(profile, tile);
            profileRow.append(tile.widget);
        }
        power.append(profileRow);

        const system = new Section('󰍛', 'SYSTEM PARAMETERS');
        this.metrics = { cpu: new MetricBar('CPU'), memory: new MetricBar('MEMORY'),
            disk: new MetricBar('ROOT DISK'), temperature: new MetricBar('CPU TEMP') };
        Object.values(this.metrics).forEach(metric => system.append(metric.widget));

        this.historyCount = label('', ['history-count']);
        this.dnd = iconButton('󰂚', 'Enable do not disturb', () => backend.notifications.toggleDnd());
        this.clear = iconButton('󰆴', 'Clear all', () => backend.notifications.clearNotifications());
        const notifications = new Section('󰂚', 'NOTIFICATIONS', [this.historyCount, this.dnd, this.clear]);
        this.empty = label('NO RECENT ALERTS', ['notifications-empty'], true);
        this.empty.xalign = 0.5;
        notifications.append(this.empty);
        this.history = new NotificationStack(backend.notifications, palette);
        this.archive = scroll(this.history.widget, 234);
        this.archive.add_css_class('notification-archive');
        notifications.append(this.archive);

        this.content = box(true, 10, [power.widget, system.widget, notifications.widget]);
        this.viewport = scroll(this.content, 800);
        this.viewport.add_css_class('dashboard-viewport');
        this.card = new CutCornerFrame(palette, this.viewport,
            { padding: CARD_PADDING, glow: CARD_GLOW, width: CARD_WIDTH }).widget;
        this.card.add_css_class('dashboard-card');
        this.card.halign = Gtk.Align.END;
        this.card.valign = Gtk.Align.START;
        this.card.margin_end = CARD_RIGHT - CARD_GLOW;
        this.card.margin_top = CARD_TOP - CARD_GLOW + this.offset;
        this.card.opacity = 0;
        this.root.add_overlay(this.card);
        const outside = new Gtk.GestureClick({ propagation_phase: Gtk.PropagationPhase.CAPTURE, button: 0 });
        outside.connect('released', (_gesture, _count, x, y) => {
            if (!this.opened) return;
            const [ok, bounds] = this.card.compute_bounds(this.root);
            if (ok && (x < bounds.get_x() + CARD_GLOW || x > bounds.get_x() + bounds.get_width() - CARD_GLOW
                || y < bounds.get_y() + CARD_GLOW || y > bounds.get_y() + bounds.get_height() - CARD_GLOW)) this.close();
        });
        this.root.add_controller(outside);
    }

    private readonly backend: DashboardBackend;

    get connector(): string { return this.monitor?.get_connector() || ''; }

    render(): void {
        const { battery, metrics, notifications } = this.backend;
        const status = !battery.ready ? 'READING BATTERY STATUS'
            : !battery.present ? 'NO BATTERY DETECTED' : (battery.status || '—').toUpperCase();
        this.batteryStatus.label = status;
        this.batteryGlyph.label = status === 'CHARGING' ? '󰂄' : '󰁹';
        this.batteryPercentage.label = battery.ready && battery.present && Number.isFinite(battery.percentage)
            ? `${Math.round(Math.max(0, Math.min(1, battery.percentage)) * 100)}%` : '—';
        this.airplane.update(this.backend.airplaneMode);
        this.profiles.forEach((tile, profile) => tile.update(this.backend.activeProfile === profile,
            this.backend.profiles.includes(profile)));
        this.metrics.cpu.update(metrics.cpuPercent, metrics.cpuPercent < 0 ? '—' : `${metrics.cpuPercent}%`);
        this.metrics.memory.update(metrics.memoryPercent, metrics.memoryPercent < 0 ? '—'
            : `${metrics.memoryUsedGiB.toFixed(1)} / ${metrics.memoryTotalGiB.toFixed(1)} GiB`);
        this.metrics.disk.update(metrics.diskPercent, metrics.diskPercent < 0 ? '—'
            : `${metrics.diskUsedGiB.toFixed(1)} / ${metrics.diskTotalGiB.toFixed(1)} GiB`);
        this.metrics.temperature.update(metrics.cpuTemperatureC, metrics.cpuTemperatureC < 0 ? '—'
            : `${Math.round(metrics.cpuTemperatureC)} °C`);
        this.historyCount.label = `HISTORY · ${notifications.history.length}`;
        (this.dnd.get_child() as Gtk.Label).label = notifications.dnd ? '󰂛' : '󰂚';
        this.dnd.tooltip_text = notifications.dnd ? 'Disable do not disturb' : 'Enable do not disturb';
        this.clear.sensitive = notifications.history.length > 0;
        this.empty.visible = notifications.history.length === 0;
        this.archive.visible = notifications.history.length > 0;
        this.history.sync(notifications.history);
    }

    private resize(): void {
        if (!this.monitor) return;
        const { width, height } = this.monitor.get_geometry();
        this.card.width_request = Math.min(CARD_WIDTH, Math.max(1, width - CARD_RIGHT * 2)) + CARD_GLOW * 2;
        this.viewport.max_content_height = Math.max(1, height - 64 - CARD_PADDING * 2);
    }

    updateMonitors(monitors: Gdk.Monitor[]): void {
        if (!this.monitor) return;
        const current = monitors.find(monitor => sameMonitor(monitor, this.monitor));
        if (!current) {
            this.monitor = null;
            this.close(false);
            return;
        }
        if (current !== this.monitor) {
            this.monitor = current;
            this.window.gdkmonitor = current;
        }
        this.resize();
    }

    toggle(monitor: Gdk.Monitor): void {
        if (this.opened && sameMonitor(monitor, this.monitor)) { this.close(); return; }
        this.monitor = monitor;
        this.window.gdkmonitor = monitor;
        this.resize();
        this.opened = true;
        this.window.keymode = Astal.Keymode.EXCLUSIVE;
        this.window.visible = true;
        this.root.grab_focus();
        this.backend.setActive(true);
        closeNetwork();
        this.animate(true);
    }

    private stopAnimation(): void {
        if (this.animation) this.root.remove_tick_callback(this.animation);
        this.animation = 0;
    }

    private animate(open: boolean): void {
        this.stopAnimation();
        const started = GLib.get_monotonic_time();
        const opacity = this.card.opacity, offset = this.offset;
        const targetOpacity = open ? 1 : 0, targetOffset = open ? 0 : -12;
        this.animation = this.root.add_tick_callback(() => {
            const elapsed = (GLib.get_monotonic_time() - started) / 1000;
            const fade = 1 - (1 - Math.min(1, elapsed / 180)) ** 3;
            const slide = 1 - (1 - Math.min(1, elapsed / 220)) ** 3;
            this.card.opacity = opacity + (targetOpacity - opacity) * fade;
            this.offset = offset + (targetOffset - offset) * slide;
            this.card.margin_top = Math.round(CARD_TOP - CARD_GLOW + this.offset);
            if (elapsed < 220) return GLib.SOURCE_CONTINUE;
            this.animation = 0;
            if (!this.opened) this.window.visible = false;
            return GLib.SOURCE_REMOVE;
        });
    }

    close(animate = true): void {
        if (!this.opened && animate) return;
        const wasOpen = this.opened;
        this.opened = false;
        this.window.keymode = Astal.Keymode.NONE;
        if (wasOpen) invoke(() => this.backend.setActive(false));
        if (animate && this.window.visible) this.animate(false);
        else {
            this.stopAnimation();
            this.card.opacity = 0;
            this.offset = -12;
            this.card.margin_top = CARD_TOP - CARD_GLOW + this.offset;
            this.window.visible = false;
        }
    }

    dispose(): void {
        this.close(false);
        Object.values(this.metrics).forEach(metric => metric.dispose());
        this.history.dispose();
        this.window.destroy();
    }
}


export const DashboardApplication = GObject.registerClass({ GTypeName: 'DashboardPanelApplication' },
    class DashboardApplication extends Astal.Application {
        backend?: DashboardBackend;
        panel?: DashboardPanel;
        popups?: NotificationPopups;
        private unsubscribe?: () => void;
        private monitorList?: Gio.ListModel;
        private monitorSignal = 0;
        private geometrySignals: [Gdk.Monitor, number][] = [];
        private theme?: ReturnType<typeof loadTheme>;

        handleRequest(request: string): string {
            const command = parseRequest(request);
            if (!this.backend || !this.panel) throw new Error('Dashboard panel is starting.');
            switch (command.command) {
            case 'status': {
                return JSON.stringify({ ...this.backend.status(), opened: this.panel.opened, monitor: this.panel.connector || null });
            }
            case 'toggle': {
                const monitor = monitorFor(command.monitor);
                if (!monitor) throw new Error('No connected monitor is available.');
                this.panel.toggle(monitor);
                break;
            }
            case 'close': this.panel.close(); break;
            case 'toggleDnd': this.backend.notifications.toggleDnd(); break;
            case 'clearNotifications': this.backend.notifications.clearNotifications(); break;
            }
            return 'ok';
        }

        vfunc_request(request: string, connection: Gio.SocketConnection): void {
            let response: string;
            try { response = this.handleRequest(request); }
            catch (error) { response = JSON.stringify({ error: error.message || String(error) }); }
            const close = () => {
                try {
                    connection.close_async(GLib.PRIORITY_DEFAULT, null, (source, result) => {
                        try { source.close_finish(result); }
                        catch (error) { console.warn(`Dashboard request close: ${error.message || error}`); }
                    });
                } catch (error) { console.warn(`Dashboard request close: ${error.message || error}`); }
            };
            try {
                AstalIO.write_sock(connection, response, (_source, result) => {
                    try { AstalIO.write_sock_finish(result); }
                    catch (error) { console.warn(`Dashboard request reply: ${error.message || error}`); }
                    finally { close(); }
                });
            } catch (error) {
                close();
                console.warn(`Dashboard request reply: ${error.message || error}`);
            }
        }

        private render(): void {
            this.panel?.render();
            this.popups?.render();
        }

        private watchGeometry(): void {
            this.geometrySignals.forEach(([monitor, signal]) => monitor.disconnect(signal));
            this.geometrySignals = connectedMonitors().map((monitor): [Gdk.Monitor, number] => [monitor, monitor.connect('notify::geometry', () => {
                this.panel?.updateMonitors(connectedMonitors());
                this.popups?.render();
            })]);
        }

        startPanel(): void {
            if (this.backend) return;
            this.theme = loadTheme(GLib.getenv('DASHBOARD_PANEL_CONFIG'), GLib.getenv('DASHBOARD_PANEL_PALETTE'));
            this.backend = new DashboardBackend();
            this.panel = new DashboardPanel({ app: this, backend: this.backend, palette: this.theme.palette });
            this.popups = new NotificationPopups({ app: this, backend: this.backend, palette: this.theme.palette });
            this.unsubscribe = this.backend.subscribe(() => this.render());
            this.monitorList = Gdk.Display.get_default()!.get_monitors();
            this.monitorSignal = this.monitorList.connect('items-changed', () => {
                this.watchGeometry();
                this.panel?.updateMonitors(connectedMonitors());
                this.popups?.render();
            });
            this.watchGeometry();
            this.backend.setActive(false);
            this.render();
        }

        disposePanel(): void {
            this.unsubscribe?.();
            this.unsubscribe = undefined;
            if (this.monitorSignal) this.monitorList?.disconnect(this.monitorSignal);
            this.monitorSignal = 0;
            this.monitorList = undefined;
            this.geometrySignals.forEach(([monitor, signal]) => monitor.disconnect(signal));
            this.geometrySignals = [];
            try {
                this.popups?.dispose();
                this.panel?.dispose();
            } finally {
                this.popups = undefined;
                this.panel = undefined;
                this.backend?.dispose();
                this.backend = undefined;
                if (this.theme) {
                    const display = Gdk.Display.get_default();
                    if (display) Gtk.StyleContext.remove_provider_for_display(display, this.theme.provider);
                    this.theme = undefined;
                }
            }
        }
    });

export async function main(): Promise<number> {
    Gtk.init();
    // AGS preloads GTK4 layer-shell. Child processes must not inherit it.
    GLib.unsetenv('LD_PRELOAD');
    const app = new DashboardApplication({ instance_name: 'dashboard-panel' });
    let exitCode = 0;
    app.connect('activate', () => {
        try { app.startPanel(); }
        catch (error) {
            console.error(`Dashboard initialization: ${error.message || error}`);
            exitCode = 1;
            app.quit();
        }
    });
    app.connect('shutdown', () => app.disposePanel());
    try { app.acquire_socket(); }
    catch (error) {
        console.error(`Dashboard startup: ${error.message || error}`);
        return 1;
    }
    app.hold();
    const status = await app.runAsync([]);
    return exitCode || status;
}

export function isMainModule(url: string): boolean {
    // GJS exposes import.meta.url and System.programPath, not import.meta.main.
    return Boolean(System.programPath && Gio.File.new_for_uri(url).equal(Gio.File.new_for_path(System.programPath)));
}

// AGS/esbuild merges imported modules into one main file. Bundled importers
// explicitly opt out before evaluation; ordinary GJS imports need no flag.
if (isMainModule(import.meta.url) && GLib.getenv('DASHBOARD_PANEL_IMPORT_ONLY') !== '1') System.exit(await main());
