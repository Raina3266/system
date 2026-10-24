// Deterministic pure/lifecycle coverage without GI, a display, D-Bus, or host telemetry.
import assert from 'node:assert/strict';
import { beforeEach, test } from 'node:test';
import { registerHooks } from 'node:module';
import path from 'node:path';
import type { NotificationRow, Palette } from './notification.ts';

class Signals {
    private handlers = new Map<number, { signal: string; callback: (...args: any[]) => void }>();
    private serial = 0;
    connect(signal: string, callback: (...args: any[]) => void) {
        const id = ++this.serial;
        this.handlers.set(id, { signal, callback });
        return id;
    }
    disconnect(id: number) { this.handlers.delete(id); }
    emit(signal: string, ...args: any[]) {
        for (const handler of [...this.handlers.values()])
            if (handler.signal === signal) handler.callback(this, ...args);
    }
}

class Widget extends Signals {
    [key: string]: any;
    sensitive = true;
    visible = true;
    parent: Box | null = null;
    get_next_sibling(): Widget | null {
        const siblings = this.parent?.children || [];
        return siblings[siblings.indexOf(this) + 1] || null;
    }
    css_classes: string[] = [];
    accessible = new Map<number, unknown>();
    constructor(properties: Record<string, any> = {}) { super(); Object.assign(this, properties); }
    add_css_class(name: string) { if (!this.css_classes.includes(name)) this.css_classes.push(name); }
    remove_css_class(name: string) { this.css_classes = this.css_classes.filter(value => value !== name); }
    has_css_class(name: string) { return this.css_classes.includes(name); }
    update_property(properties: number[], values: unknown[]) {
        properties.forEach((property, index) => this.accessible.set(property, values[index]));
    }
    queue_draw() {}
}

class Box extends Widget {
    children: Widget[] = [];
    append(child: Widget) { child.parent = this; this.children.push(child); }
    get_first_child(): Widget | null { return this.children[0] || null; }
}
class Label extends Widget { label = ''; constructor(properties: Record<string, any> = {}) { super(properties); Object.assign(this, properties); } }
class Button extends Widget {
    get_child(): Widget | null { return this.child || null; }
}
class Overlay extends Widget {
    overlays: Widget[] = [];
    measured = new Map<Widget, boolean>();
    set_child(child: Widget) { this.child = child; }
    add_overlay(child: Widget) { this.overlays.push(child); }
    set_measure_overlay(child: Widget, measure: boolean) { this.measured.set(child, measure); }
}
class DrawingArea extends Widget {
    draw: ((...args: any[]) => void) | null = null;
    set_draw_func(callback: (...args: any[]) => void) { this.draw = callback; }
}
const Gtk = {
    Box, Label, Button, Overlay, DrawingArea,
    Orientation: { HORIZONTAL: 0, VERTICAL: 1 },
    Align: { FILL: 0, START: 1, END: 2, CENTER: 3 },
    Overflow: { VISIBLE: 0 },
    AccessibleProperty: { LABEL: 0, DESCRIPTION: 1 },
};

let clock = 0, serial = 0;
const timers = new Map<number, { due: number; interval: number; callback: () => boolean }>();
const files = new Map<string, string>();
const writes: { path: string; flags: number }[] = [];
const settings = new Map<string, any>();
const environment = new Map<string, string>();
const reads: { path: string; io: any; callback: (file: any, result: any) => void }[] = [];
const children: Child[] = [];
const missing = () => Object.assign(new Error('Not found'), { matches: () => true });
const delay = () => new Promise<void>(resolve => setImmediate(resolve));

function advance(ms: number) {
    const end = clock + ms;
    while (true) {
        const next = [...timers].filter(([, timer]) => timer.due <= end).sort((a, b) => a[1].due - b[1].due)[0];
        if (!next) break;
        const [id, timer] = next;
        clock = timer.due;
        timers.delete(id);
        if (timer.callback()) timers.set(id, { ...timer, due: clock + timer.interval });
    }
    clock = end;
}

const GLib = {
    PRIORITY_DEFAULT: 0, SOURCE_REMOVE: false, SOURCE_CONTINUE: true,
    timeout_add(_priority: number, interval: number, callback: () => boolean) {
        const id = ++serial;
        timers.set(id, { due: clock + interval, interval, callback });
        return id;
    },
    source_remove(id: number) { return timers.delete(id); },
    getenv(name: string) { return environment.get(name) ?? null; },
    get_home_dir: () => '/test/home', build_filenamev: (parts: string[]) => path.join(...parts),
    path_get_dirname: path.dirname, mkdir_with_parents: () => 0,
    uuid_string_random: () => `uuid-${++serial}`,
};

class Child {
    argv: string[];
    killed = false;
    done = false;
    wait: ((child: Child, result: any) => void) | null = null;
    communicate: ((child: Child, result: any) => void) | null = null;
    constructor(argv: string[]) { this.argv = argv; children.push(this); }
    wait_async(_io: any, callback: any) { this.wait = callback; }
    wait_finish() { return true; }
    communicate_utf8_async(_input: any, _io: any, callback: any) { this.communicate = callback; }
    communicate_utf8_finish(result: any) { return [true, result, '']; }
    get_successful() { return !this.killed; }
    force_exit() { this.killed = true; this.finish(''); }
    finish(output: string) {
        if (this.done) return;
        this.done = true;
        this.wait?.(this, null);
        this.communicate?.(this, output);
    }
}

class Settings {
    static sync() {}
    constructor(_options: any) {}
    get_user_value(key: string) { return settings.get(key) ?? null; }
    get_boolean(key: string) { return settings.get(key) ?? false; }
    set_strv(key: string, value: string[]) { settings.set(key, [...value]); return true; }
}

const Gio = {
    Settings,
    Cancellable: class { cancelled = false; cancel() { this.cancelled = true; } },
    FileQueryInfoFlags: { NONE: 0 }, IOErrorEnum: { NOT_FOUND: 1 }, io_error_quark: () => 0,
    FileCreateFlags: { PRIVATE: 1, REPLACE_DESTINATION: 2 },
    SubprocessFlags: { STDOUT_PIPE: 1, STDERR_SILENCE: 2 },
    SubprocessLauncher: class {
        constructor(_options: any) {}
        setenv() {}
        spawnv(argv: string[]) { return new Child(argv); }
    },
    File: { new_for_path(filename: string) {
        return {
            query_info() { if (!files.has(filename)) throw missing(); return { get_size: () => files.get(filename)!.length }; },
            load_contents() { if (!files.has(filename)) throw missing(); return [true, new TextEncoder().encode(files.get(filename)!)]; },
            replace_contents(bytes: Uint8Array, _etag: any, _backup: boolean, flags: number) {
                files.set(filename, new TextDecoder().decode(bytes)); writes.push({ path: filename, flags }); return [true, 'etag'];
            },
            load_contents_async(io: any, callback: any) { reads.push({ path: filename, io, callback }); },
            load_contents_finish(result: any) {
                if (result.error) throw result.error;
                return [true, new TextEncoder().encode(result.text)];
            },
        };
    } },
};

class NativeNotification {
    id: number;
    time = 100;
    app_name = 'Test';
    app_icon = 'test-symbolic';
    image = '';
    summary: string;
    body = 'Body';
    urgency = 1;
    transient = false;
    resident = false;
    expire_timeout = 0;
    actions = [{ id: 'open', label: 'Open' }];
    constructor(id: number, summary: string, overrides = {}) { this.id = id; this.summary = summary; Object.assign(this, overrides); }
    expire() { native.resolve(this.id, 1); }
    dismiss() { native.resolve(this.id, 2); }
    invoke(action: string) { native.invoked.push([this.id, action]); if (!this.resident) native.resolve(this.id, 3); }
}

class Native extends Signals {
    notifications: NativeNotification[] = [];
    invoked: [number, string][] = [];
    resolutions: [number, number][] = [];
    ignore_timeout = false;
    default_timeout = 42;
    get dont_disturb() { return settings.get('dont-disturb') ?? false; }
    set dont_disturb(value: boolean) { settings.set('dont-disturb', value); this.emit('notify::dont-disturb'); }
    get_notification(id: number) { return this.notifications.find(row => row.id === id); }
    send(row: NativeNotification) {
        const replaced = !!this.get_notification(row.id);
        this.notifications = [row, ...this.notifications.filter(existing => existing.id !== row.id)];
        this.emit('notified', row.id, replaced);
    }
    resolve(id: number, reason: number) {
        if (!this.get_notification(id)) return;
        this.notifications = this.notifications.filter(row => row.id !== id);
        this.resolutions.push([id, reason]);
        this.emit('resolved', id, reason);
    }
}
let native = new Native();
const device = Object.assign(new Signals(), { is_present: true, percentage: 0.42, state: 1 });
const upower = Object.assign(new Signals(), { daemon_version: '1', on_battery: false });
const power = Object.assign(new Signals(), { profiles: [{ profile: 'balanced' }, { profile: 'power-saver' }], active_profile: 'balanced' });
const network = Object.assign(new Signals(), { client: Object.assign(new Signals(), { wireless_enabled: true }) as any });
const bluetooth = Object.assign(new Signals(), { adapters: [] as any[] });
const mockGI: Record<string, any> = {
    GLib, Gio, AstalNotifd: { get_default: () => native, ClosedReason: { EXPIRED: 1 } },
    AstalBattery: { get_default: () => device, UPower: class { constructor() { return upower; } },
        State: { CHARGING: 1, PENDING_CHARGE: 5, FULLY_CHARGED: 4 } },
    AstalPowerProfiles: { get_default: () => power }, AstalNetwork: { get_default: () => network },
    AstalBluetooth: { get_default: () => bluetooth },
    // Native widget doubles exercise controls without starting the application.
    GObject: { registerClass: (_metadata: any, implementation: any) => implementation },
    Astal: { Application: class {} }, AstalIO: {}, Gtk, Gdk: {}, Pango: {}, Gtk4LayerShell: {},
    system: { programPath: '' },
};
(globalThis as any)[Symbol.for('dashboard-test-gi')] = mockGI;
const hooks = registerHooks({
    resolve(specifier, context, next) {
        if (specifier.startsWith('gi://') || specifier === 'system') {
            const name = specifier === 'system' ? 'system' : specifier.slice(5).split('?')[0];
            return { url: `dashboard-test:${name}`, shortCircuit: true };
        }
        return next(specifier, context);
    },
    load(url, context, next) {
        if (url.startsWith('dashboard-test:')) return { format: 'module', shortCircuit: true,
            source: `export default globalThis[Symbol.for('dashboard-test-gi')][${JSON.stringify(url.slice(15))}];` };
        return next(url, context);
    },
});
const {
    NotificationCenter, archived, DEFAULT_TIMEOUT, HISTORY_LIMIT, newestFirst,
    notificationTimeout, POPUP_LIMIT, restoreArchive, serializeArchive,
} = await import('./notification.ts');
const {
    MetricsSampler, cpuPercent, emptyMetrics, parseCpu, parseDisk, parseMemory, parseTemperature,
} = await import('./parameters.ts');
const { ButtonSwitcher, DashboardApplication, DashboardBackend, parseRequest } = await import('./power.ts');
hooks.deregister();

beforeEach(() => {
    clock = 0; serial = 0; timers.clear(); files.clear(); writes.length = 0; settings.clear(); environment.clear();
    reads.length = 0; children.length = 0; native = new Native();
    environment.set('XDG_STATE_HOME', '/test/state');
    device.is_present = true; device.percentage = 0.42; device.state = 1;
    upower.daemon_version = '1'; upower.on_battery = false;
    power.profiles = [{ profile: 'balanced' }, { profile: 'power-saver' }]; power.active_profile = 'balanced';
    network.client = Object.assign(new Signals(), { wireless_enabled: true });
    bluetooth.adapters = [Object.assign(new Signals(), { powered: true }), Object.assign(new Signals(), { powered: false })];
});

const archivePath = '/test/state/dashboard/notifications.json';
function completeReads(cpu = 'cpu 10 0 0 90', memory = 'MemTotal: 2097152 kB\nMemAvailable: 1048576 kB', temperature = '45000') {
    for (const request of reads.splice(0)) request.callback(Gio.File.new_for_path(request.path), request.io.cancelled
        ? { error: new Error('Cancelled') } : { text: request.path === '/proc/stat' ? cpu : request.path === '/proc/meminfo' ? memory : temperature });
}

const row = (id: string, overrides: Partial<NotificationRow> = {}): NotificationRow => ({
    id, app: 'Test', summary: 'Summary', body: 'Body', icon: 'test-symbolic', urgency: 1,
    transient: false, timeout: -1, actions: [{ identifier: 'open', text: 'Open' }], closed: false,
    ...overrides,
});

const palette: Palette = {
    background: '#101010', secondaryBackground: '#202020', elevated: '#303030', foreground: '#eeeeee',
    separator: '#404040', cyan: '#00ffff', pink: '#ff00ff', dimPink: '#804080', yellow: '#ffff00', red: '#ff0000',
};
const profileChoices = [
    { id: 'power-saver', label: 'Power Saver' },
    { id: 'balanced', label: 'Balanced' },
    { id: 'performance', label: 'Performance' },
] as const;

function switcher(choices: readonly { id: string; label: string }[] = profileChoices) {
    const requests: string[] = [];
    const control = new ButtonSwitcher('Profile', choices, id => requests.push(id), palette);
    const previous = control.previous as unknown as Button;
    const next = control.next as unknown as Button;
    const value = control.value as unknown as Label;
    const indicators = control.indicators as unknown as Box;
    return { control, requests, previous, next, value, indicators };
}

test('switcher constructs a native caption/frame row and exposes accessible native arrows', () => {
    const { control, previous, next, value, indicators } = switcher();
    const widget = control.widget as unknown as Box;
    assert.ok(widget instanceof Box);
    assert.equal(widget.orientation, Gtk.Orientation.HORIZONTAL);
    assert.ok(widget.children.some(child => child instanceof Label && child.label === 'Profile'));
    assert.ok(widget.children.some(child => child instanceof Overlay));
    assert.ok(previous instanceof Button);
    assert.ok(next instanceof Button);
    assert.ok(value instanceof Label);
    assert.ok(indicators instanceof Box);
    assert.equal(value.label, 'Unavailable');
    control.update('balanced');
    assert.equal(previous.accessible.get(Gtk.AccessibleProperty.LABEL), 'Previous profile');
    assert.equal(next.accessible.get(Gtk.AccessibleProperty.LABEL), 'Next profile');
    assert.equal(value.accessible.get(Gtk.AccessibleProperty.LABEL), 'Balanced');
});

test('switcher arrows wrap in declared order and skip unavailable choices', () => {
    const { control, requests, previous, next } = switcher();
    // Availability order must not reorder the declared choices; unknown IDs are ignored.
    control.update('power-saver', ['performance', 'missing', 'power-saver']);
    next.emit('clicked');
    previous.emit('clicked');
    assert.deepEqual(requests, ['performance', 'performance']);
    control.update('performance', ['performance', 'power-saver']);
    next.emit('clicked');
    previous.emit('clicked');
    assert.deepEqual(requests, ['performance', 'performance', 'power-saver', 'power-saver']);
    control.update('balanced');
    next.emit('clicked');
    previous.emit('clicked');
    assert.deepEqual(requests.slice(-2), ['performance', 'power-saver']);
    control.update('performance');
    next.emit('clicked');
    control.update('power-saver');
    previous.emit('clicked');
    assert.deepEqual(requests.slice(-2), ['power-saver', 'performance']);
});

test('switcher unknown active selects first with next and last with previous', () => {
    const { control, requests, previous, next } = switcher();
    control.update('unknown', ['performance', 'balanced']);
    assert.equal(previous.sensitive, true);
    assert.equal(next.sensitive, true);
    next.emit('clicked');
    previous.emit('clicked');
    assert.deepEqual(requests, ['balanced', 'performance']);
});

test('switcher single available choice disables current selection but can restore a different one', () => {
    const { control, requests, previous, next, value } = switcher();
    control.update('balanced', ['balanced']);
    assert.equal(value.label, 'Balanced');
    assert.equal(previous.sensitive, false);
    assert.equal(next.sensitive, false);
    // Emit directly, even though GTK would normally suppress disabled activation.
    previous.emit('clicked');
    next.emit('clicked');
    assert.deepEqual(requests, []);
    control.update('performance', ['balanced']);
    assert.equal(value.label, 'Performance');
    assert.equal(previous.sensitive, true);
    assert.equal(next.sensitive, true);
    previous.emit('clicked');
    next.emit('clicked');
    assert.deepEqual(requests, ['balanced', 'balanced']);
    control.update('balanced', ['balanced']);
    previous.emit('clicked');
    next.emit('clicked');
    assert.deepEqual(requests, ['balanced', 'balanced']);
});

test('switcher empty availability preserves native truth while empty choices are unavailable; both guard callbacks', () => {
    for (const choices of [profileChoices, []]) {
        const { control, requests, previous, next, value } = switcher(choices);
        for (const available of [[], ['missing']]) {
            control.update('balanced', available);
            assert.equal(value.label, choices.length ? 'Balanced' : 'Unavailable');
            assert.equal(previous.sensitive, false);
            assert.equal(next.sensitive, false);
            previous.emit('clicked');
            next.emit('clicked');
        }
        if (!choices.length) {
            control.update('unknown');
            assert.equal(value.label, 'Unavailable');
            assert.equal(previous.sensitive, false);
            assert.equal(next.sensitive, false);
            previous.emit('clicked');
            next.emit('clicked');
        }
        assert.deepEqual(requests, []);
    }
});

test('switcher update is read-only and clicks await authoritative state without optimistic selection', () => {
    const { control, requests, previous, next, value, indicators } = switcher();
    for (const active of ['power-saver', 'performance', 'balanced']) control.update(active);
    assert.deepEqual(requests, []);
    assert.equal(value.label, 'Balanced');
    const tickState = () => indicators.children.map(tick => [...tick.css_classes]);
    const before = tickState();
    next.emit('clicked');
    next.emit('clicked');
    previous.emit('clicked');
    assert.deepEqual(requests, ['performance', 'performance', 'power-saver']);
    assert.equal(value.label, 'Balanced');
    assert.deepEqual(tickState(), before);
    control.update('performance');
    assert.equal(value.label, 'Performance');
    assert.notDeepEqual(tickState(), before);
    next.emit('clicked');
    assert.deepEqual(requests, ['performance', 'performance', 'power-saver', 'power-saver']);
});

test('switcher ticks reflect authoritative selection and availability and reset on update', () => {
    const { control, requests, indicators } = switcher();
    const state = () => indicators.children.map(tick => ({
        classes: [...tick.css_classes].sort(), visible: tick.visible,
    }));
    control.update('balanced');
    assert.equal(indicators.children.length, profileChoices.length);
    const balanced = state();
    control.update('performance');
    const performance = state();
    assert.notDeepEqual(performance[1], balanced[1]);
    assert.notDeepEqual(performance[2], balanced[2]);
    assert.deepEqual(performance[0], balanced[0]);
    control.update('balanced', ['power-saver', 'balanced']);
    const filtered = state();
    assert.notDeepEqual(filtered[2], balanced[2]);
    assert.deepEqual(filtered[0], balanced[0]);
    assert.deepEqual(filtered[1], balanced[1]);
    control.update('balanced');
    assert.deepEqual(state(), balanced);
    assert.deepEqual(requests, []);
});

test('switcher airplane yes/no choices cycle in both directions without optimistic toggles', () => {
    const { control, requests, previous, next, value } = switcher([
        { id: 'no', label: 'No' }, { id: 'yes', label: 'Yes' },
    ]);
    control.update('no');
    previous.emit('clicked');
    next.emit('clicked');
    assert.deepEqual(requests, ['yes', 'yes']);
    assert.equal(value.label, 'No');
    control.update('yes');
    previous.emit('clicked');
    next.emit('clicked');
    assert.deepEqual(requests, ['yes', 'yes', 'no', 'no']);
    assert.equal(value.label, 'Yes');
});

test('strict JSON request envelope and command arity', () => {
    for (const command of ['status', 'toggle', 'close', 'toggleDnd', 'clearNotifications'])
        assert.deepEqual(parseRequest(JSON.stringify({ command })), { command });
    for (const monitor of ['', 'DP-1', 'HDMI-A-2'])
        assert.deepEqual(parseRequest(JSON.stringify({ command: 'toggle', monitor })), { command: 'toggle', monitor });
    for (const request of [null, [], 'status', {}, { command: 'bad' }, { command: 'toggleIdleInhibit' },
        { command: 'status', extra: true }, { command: 'status', monitor: 'DP-1' },
        { command: 'toggleDnd', monitor: '' }, { command: 'close', args: [] },
        { command: 'toggle', monitor: null }, { command: 'toggle', monitor: 4 },
        { command: 'toggle', monitor: 'a'.repeat(257) }, { command: 'toggle', monitor: 'DP-1\nclose' },
        { command: 'toggle', monitor: '\u007f' }])
        assert.throws(() => parseRequest(JSON.stringify(request)), JSON.stringify(request));
    for (const text of ['', 'status', '{} {}', '{broken']) assert.throws(() => parseRequest(text));
});

test('all missing metrics use -1, not plausible zero readings', () => {
    assert.ok(Object.values(emptyMetrics()).every(value => value === -1));
    assert.equal(parseCpu(''), null);
    assert.deepEqual(parseMemory(''), { memoryPercent: -1, memoryUsedGiB: -1, memoryTotalGiB: -1 });
    assert.deepEqual(parseDisk(''), { diskPercent: -1, diskUsedGiB: -1, diskTotalGiB: -1 });
    assert.equal(parseTemperature(''), -1);
});

test('CPU deltas include iowait as idle, exclude double-counted guest time', () => {
    const before = parseCpu('cpu  100 20 30 400 50 6 7 8 20 5\ncpu0 1 2 3 4');
    assert.deepEqual(before, { total: 621, idle: 450 });
    for (const newline of ['\n', '\r\n']) {
        assert.deepEqual(parseCpu(`cpu 1 2 3 4${newline}cpu0 malformed${newline}intr 999`), { total: 10, idle: 4 });
        assert.equal(parseCpu(`cpu malformed${newline}cpu 1 2 3 4`), null, 'only the first line is the aggregate sample');
    }
    const after = parseCpu('cpu  140 20 40 440 60 6 7 8 60 5');
    assert.equal(cpuPercent(before, after), 50);
    assert.equal(cpuPercent(null, after), -1);
    assert.equal(cpuPercent(before, before), -1);
    assert.equal(cpuPercent(after, before), -1);
    assert.equal(cpuPercent(before, null), -1);
    assert.equal(cpuPercent({ total: 10, idle: 5 }, { total: 20, idle: 16 }), -1);
    assert.equal(cpuPercent({ total: 10, idle: 5 }, { total: 20, idle: 4 }), -1);
    for (const text of ['cpu0 1 2 3 4', 'cpu 1 2 3', 'cpu 1 -2 3 4', 'cpu 1 NaN 3 4', 'cpu 1 2 3 4x'])
        assert.equal(parseCpu(text), null);
});

test('memory uses MemAvailable including zero, with old-kernel fallback', () => {
    assert.deepEqual(parseMemory('MemTotal: 2097152 kB\nMemAvailable: 524288 kB\nMemFree: 0 kB'),
        { memoryPercent: 75, memoryUsedGiB: 1.5, memoryTotalGiB: 2 });
    assert.equal(parseMemory('MemTotal: 100 kB\nMemAvailable: 0 kB\nMemFree: 80 kB').memoryPercent, 100);
    assert.equal(parseMemory('MemTotal: 100 kB\nMemFree: 10 kB\nBuffers: 10 kB\nCached: 30 kB').memoryPercent, 50);
    assert.equal(parseMemory('MemTotal: 100 kB\nMemAvailable: 200 kB').memoryPercent, 0);
    for (const text of ['MemTotal: 0 kB\nMemFree: 2 kB', 'MemTotal: 100 kB', 'MemAvailable: 100 kB', 'MemTotal: nonsense'])
        assert.equal(parseMemory(text).memoryPercent, -1);
});

test('df byte units and supplied pcent (reserved blocks) are preserved', () => {
    assert.deepEqual(parseDisk(' Used 1B-blocks Use%\n 1073741824 4294967296 30%\n'),
        { diskPercent: 30, diskUsedGiB: 1, diskTotalGiB: 4 });
    for (const text of ['header', 'header\n1 0 5%', 'header\n1 2 NaN', 'header\n1 2 5%garbage', 'header\n1 2 5%\nextra'])
        assert.equal(parseDisk(text).diskPercent, -1);
    assert.equal(parseDisk('header\n0 100 0%').diskPercent, 0);
});

test('temperature handles millidegrees and missing/malformed files', () => {
    assert.equal(parseTemperature('45678\n'), 45.678);
    assert.equal(parseTemperature(' 52.5 '), 52.5);
    for (const text of ['', '0', '-1', 'NaN', '45C', '40000\n50000']) assert.equal(parseTemperature(text), -1);
});

test('history and popup ordering is newest-first, bounded and replacement-deduplicated', () => {
    let history: NotificationRow[] = [];
    let popups: NotificationRow[] = [];
    for (let index = 0; index < 205; index++) {
        history = newestFirst(history, row(String(index)), HISTORY_LIMIT);
        popups = newestFirst(popups, row(String(index)), POPUP_LIMIT);
    }
    assert.equal(history.length, 200);
    assert.equal(history[0].id, '204');
    assert.equal(history[199].id, '5');
    assert.deepEqual(popups.map(row => row.id), ['204', '203', '202', '201', '200']);
    history = newestFirst(history, row('10', { summary: 'Replaced' }), HISTORY_LIMIT);
    assert.equal(history[0].summary, 'Replaced');
    assert.equal(history.filter(row => row.id === '10').length, 1);
});

test('timeout policy: eight-second default, milliseconds, critical and zero exemption', () => {
    assert.equal(notificationTimeout(row('1')), DEFAULT_TIMEOUT);
    assert.equal(notificationTimeout(row('1', { timeout: 350 })), 350);
    assert.equal(notificationTimeout(row('1', { timeout: 0 })), 0);
    assert.equal(notificationTimeout(row('1', { urgency: 2, timeout: 350 })), 0);
    assert.equal(notificationTimeout(row('1', { urgency: 0, timeout: 350 })), 350);
});

test('v1 archives keep existing fields, strip live actions and transient image providers', () => {
    const live = row('nonce:42', { icon: 'image://native/image' });
    const saved = serializeArchive([live, row('transient', { transient: true })], true);
    const raw = JSON.parse(saved);
    assert.deepEqual(Object.keys(raw).sort(), ['dnd', 'history', 'version']);
    assert.equal(raw.version, 1);
    assert.deepEqual(raw.history, [archived(live)]);
    assert.equal(live.closed, false);
    assert.equal(live.actions.length, 1);
    const restored = restoreArchive(saved);
    assert.equal(restored.dnd, true);
    assert.equal(restored.history[0].closed, true);
    assert.deepEqual(restored.history[0].actions, []);
    assert.equal(restored.history[0].icon, '');
    assert.equal(archived(row('file', { icon: '/tmp/cached.png' })).icon, '/tmp/cached.png');
});

test('restore tolerates malformed rows, defaults optional fields, deduplicates, excludes transients', () => {
    const restored = restoreArchive(JSON.stringify({ version: 1, dnd: 'true', history: [null, {}, row('a'), row('a'),
        { id: 'old:1', app: 'App', summary: 'Legacy', body: '', icon: 4, urgency: 9, timeout: 'bad', actions: ['bad'] },
        row('t', { transient: true }), row('', {}), { id: 4, app: '', summary: '', body: '' }] }));
    assert.equal(restored.dnd, false);
    assert.deepEqual(restored.history.map(row => row.id), ['a', 'old:1']);
    assert.deepEqual(restored.history[1], row('old:1', { app: 'App', summary: 'Legacy', body: '', icon: '', actions: [], closed: true }));
    assert.equal(restoreArchive(serializeArchive(Array.from({ length: 210 }, (_, index) => row(String(index))), false)).history.length, 200);
    assert.deepEqual(restoreArchive('  '), { history: [], dnd: false });
    for (const text of ['null', '[]', '{}', '{broken', '{"version":2,"history":[]}', '{"version":1,"history":{}}'])
        assert.throws(() => restoreArchive(text));
});

test('backend native controls, fractional battery, radio hotplug and read-only status', () => {
    const backend = new DashboardBackend();
    let changes = 0;
    const unsubscribe = backend.subscribe(() => changes++);
    assert.equal(backend.battery.percentage, 0.42);
    assert.equal(backend.battery.status, 'CHARGING');
    assert.equal(backend.airplaneMode, false);
    assert.equal(reads.length, 0);
    assert.equal(children.length, 0);
    const writesBefore = writes.length;
    for (let i = 0; i < 20; i++) assert.equal(backend.status().historyCount, 0);
    assert.equal(writes.length, writesBefore);
    assert.equal(timers.size, 0);
    backend.toggleAirplaneMode();
    assert.equal(network.client.wireless_enabled, false);
    assert.ok(bluetooth.adapters.every(adapter => !adapter.powered));
    network.client.emit('notify::wireless-enabled');
    assert.equal(backend.airplaneMode, true);
    backend.toggleAirplaneMode();
    assert.equal(network.client.wireless_enabled, true);
    assert.ok(bluetooth.adapters.every(adapter => adapter.powered));
    network.client = null;
    network.emit('notify::client');
    bluetooth.adapters = [];
    bluetooth.emit('notify::adapters');
    assert.equal(backend.airplaneMode, true);
    assert.doesNotThrow(() => backend.toggleAirplaneMode());
    const hotplugged = Object.assign(new Signals(), { powered: true });
    bluetooth.adapters.push(hotplugged);
    bluetooth.emit('notify::adapters');
    assert.equal(backend.airplaneMode, false);
    hotplugged.powered = false; hotplugged.emit('notify::powered');
    assert.equal(backend.airplaneMode, true);
    backend.setProfile('power-saver');
    assert.equal(power.active_profile, 'power-saver');
    assert.throws(() => backend.setProfile('performance'));
    native.send(new NativeNotification(1, 'Changed'));
    assert.ok(changes > 0);
    assert.equal(backend.status().historyCount, 1);
    assert.equal(backend.status().popupCount, 1);
    unsubscribe();
    const before = changes;
    native.send(new NativeNotification(2, 'Unsubscribed'));
    assert.equal(changes, before);
    backend.dispose(); backend.dispose();
    assert.equal(timers.size, 0);
});

test('status preserves battery percentage, decile glyphs, thresholds and profile tooltip', () => {
    device.state = 2;
    upower.on_battery = true;
    const backend = new DashboardBackend();
    const icons = ['󰁺', '󰁻', '󰁼', '󰁽', '󰁾', '󰁿', '󰂀', '󰂁', '󰂂', '󰁹'];
    for (let percent = 0; percent <= 100; percent++) {
        device.percentage = percent / 100;
        device.emit('notify');
        const status = backend.status();
        assert.equal(status.text, `${icons[Math.min(9, Math.floor(percent / 10))]} ${percent}%`);
        assert.equal(status.class, percent <= 15 ? 'critical' : percent <= 25 ? 'warning' : percent < 50 ? 'low' : 'discharging', `${percent}%`);
        assert.equal(status.tooltip, 'ON BATTERY | Profile: balanced | Click: Dashboard');
    }
    device.percentage = 0.426;
    device.emit('notify');
    assert.equal(backend.status().text, '󰁾 43%');
    power.active_profile = 'power-saver';
    power.emit('notify::active-profile');
    assert.equal(backend.status().tooltip, 'ON BATTERY | Profile: power-saver | Click: Dashboard');
    upower.on_battery = false;
    device.emit('notify');
    assert.equal(backend.status().tooltip, 'AC POWER | Profile: power-saver | Click: Dashboard');
    backend.dispose();
});

test('status gives charging, pending charge and full priority over percentage thresholds', () => {
    const backend = new DashboardBackend();
    for (const [state, expectedClass, expectedStatus] of [
        [1, 'charging', 'CHARGING'],
        [5, 'charging', 'CONNECTED · CHARGING SOON'],
        [4, 'full', 'FULLY CHARGED'],
    ] as const) {
        device.state = state;
        for (const percent of [0, 15, 25, 49, 50, 100]) {
            device.percentage = percent / 100;
            device.emit('notify');
            const status = backend.status();
            assert.equal(status.text, `󰂄 ${percent}%`);
            assert.equal(status.class, expectedClass);
            assert.equal(status.tooltip, `${expectedStatus} | Profile: balanced | Click: Dashboard`);
        }
    }
    backend.dispose();
});

test('status preserves clear dashboard badge for absent, unreadable or not-ready batteries', () => {
    const backend = new DashboardBackend();
    const assertClear = () => {
        const status = backend.status();
        assert.equal(status.text, '󰕮');
        assert.equal(status.class, 'clear');
        assert.equal(status.tooltip, 'Dashboard');
    };
    device.is_present = false;
    device.emit('notify');
    assertClear();
    device.is_present = true;
    for (const percentage of [NaN, Infinity]) {
        device.percentage = percentage;
        device.emit('notify');
        assertClear();
    }
    device.percentage = 0.42;
    upower.daemon_version = '';
    upower.emit('notify');
    assertClear();
    backend.dispose();
});

test('status keeps notification metadata without changing the battery badge or mutating state', () => {
    const backend = new DashboardBackend();
    native.send(new NativeNotification(1, 'History'));
    native.send(new NativeNotification(2, 'Transient', { transient: true }));
    assert.deepEqual(backend.status(), {
        text: '󰂄 42%', tooltip: 'CHARGING | Profile: balanced | Click: Dashboard', class: 'charging',
        historyCount: 1, popupCount: 2, dnd: false, airplaneMode: false, activeProfile: 'balanced',
    });
    backend.notifications.toggleDnd();
    const expected = { ...backend.status(), historyCount: 1, popupCount: 0, dnd: true };
    assert.equal(expected.text, '󰂄 42%');
    assert.equal(expected.class, 'charging');
    const before = { writes: writes.length, reads: reads.length, children: children.length, timers: timers.size,
        settings: [...settings], native: [...native.notifications], history: backend.notifications.history,
        popups: backend.notifications.popups, resolutions: [...native.resolutions], battery: backend.battery };
    let changes = 0;
    backend.subscribe(() => changes++);
    for (let index = 0; index < 20; index++) assert.deepEqual(backend.status(), expected);
    assert.deepEqual({ writes: writes.length, reads: reads.length, children: children.length, timers: timers.size,
        settings: [...settings], native: [...native.notifications], history: backend.notifications.history,
        popups: backend.notifications.popups, resolutions: [...native.resolutions], battery: backend.battery }, before);
    assert.equal(changes, 0);
    backend.dispose();
});

test('application status serializes the native status object with panel visibility and monitor', () => {
    const backend = new DashboardBackend();
    const app = new DashboardApplication();
    app.backend = backend;
    app.panel = { opened: true, connector: 'DP-1' };
    assert.deepEqual(JSON.parse(app.handleRequest('{"command":"status"}')), {
        ...backend.status(), opened: true, monitor: 'DP-1',
    });
    app.panel = { opened: false, connector: '' };
    assert.deepEqual(JSON.parse(app.handleRequest('{"command":"status"}')), {
        ...backend.status(), opened: false, monitor: null,
    });
    backend.dispose();
});

test('notification capabilities advertise only implemented plaintext UI features through GSettings', () => {
    settings.set('server-capabilites', ['action-icons', 'actions', 'body', 'body-hyperlinks', 'body-images',
        'body-markup', 'icon-static', 'persistence', 'sound']);
    const center = new NotificationCenter();
    assert.deepEqual(settings.get('server-capabilites'), ['actions', 'body', 'icon-static', 'persistence']);
    center.dispose();
});

test('metrics cancel on close, never overlap, discard stale completions, reset CPU baseline', async () => {
    let changes = 0;
    const sampler = new MetricsSampler(() => changes++);
    sampler.setActive(true);
    assert.equal(reads.length, 3);
    assert.equal(children.length, 1);
    assert.deepEqual(children[0].argv, ['df', '-B1', '--output=used,size,pcent', '/']);
    advance(30000);
    assert.equal(reads.length, 3);
    assert.equal(children.length, 1);
    sampler.setActive(false);
    assert.ok(reads.every(read => read.io.cancelled));
    assert.equal(timers.size, 0);
    assert.ok(children[0].killed);
    const before = changes;
    sampler.setActive(true);
    assert.equal(reads.length, 3, 'reopen must wait for cancelled reads');
    completeReads(); await delay();
    assert.equal(reads.length, 3, 'new generation starts after cancellation completes');
    completeReads(); await delay();
    assert.equal(sampler.data.cpuPercent, -1);
    assert.equal(sampler.data.memoryPercent, 50);
    assert.equal(sampler.data.cpuTemperatureC, 45);
    advance(2500);
    completeReads('cpu 60 0 0 140'); await delay();
    assert.equal(sampler.data.cpuPercent, 50);
    sampler.dispose(); await delay();
    assert.equal(timers.size, 0);
    assert.ok(changes > before);
});

test('metrics failures replace old readings with -1 and df uses explicit executable path', async () => {
    environment.set('COREUTILS_DF', '/test/coreutils/df');
    const sampler = new MetricsSampler(() => {});
    sampler.setActive(true);
    assert.equal(children[0].argv[0], '/test/coreutils/df');
    completeReads(); children[0].finish('Used 1B-blocks Use%\n1073741824 2147483648 55%'); await delay();
    assert.equal(sampler.data.diskPercent, 55);
    assert.equal(sampler.data.diskUsedGiB, 1);
    advance(30000); completeReads('', '', ''); await delay();
    children[1].finish('bad'); await delay();
    assert.equal(sampler.data.cpuPercent, -1);
    assert.equal(sampler.data.memoryUsedGiB, -1);
    assert.equal(sampler.data.diskUsedGiB, -1);
    assert.equal(sampler.data.cpuTemperatureC, -1);
    sampler.dispose();
});

test('notification replacement, hover reset, critical exemption and native close semantics', () => {
    const center = new NotificationCenter();
    assert.equal(native.ignore_timeout, true);
    assert.equal(native.default_timeout, -1);
    native.send(new NativeNotification(1, 'Original', { expire_timeout: 600 }));
    const key = center.history[0].id;
    advance(400);
    native.send(new NativeNotification(1, 'Replacement', { expire_timeout: 600, time: 102 }));
    assert.equal(center.history[0].id, key);
    advance(300);
    assert.equal(center.popups.length, 1);
    center.setHovered(key, true); advance(1000);
    assert.equal(center.popups.length, 1);
    center.setHovered(key, false); advance(599);
    assert.equal(center.popups.length, 1);
    advance(1);
    assert.equal(center.popups.length, 0);
    assert.equal(center.history[0].closed, true);
    assert.deepEqual(center.history[0].actions, []);
    center.invokeNotificationAction(key, 'open');
    assert.equal(native.invoked.length, 0);
    native.send(new NativeNotification(2, 'Critical', { urgency: 2, expire_timeout: 50 }));
    advance(10000);
    assert.equal(center.popups.length, 1);
    native.resolve(2, 3);
    assert.equal(center.history.length, 1);
    center.dismissNotification(key);
    assert.equal(center.history.length, 0);
    center.dispose();
});

test('notification controls find the matching live row without acting on archived or missing rows', () => {
    const center = new NotificationCenter();
    native.send(new NativeNotification(1, 'First', { expire_timeout: 10 }));
    const first = center.history[0].id;
    native.send(new NativeNotification(2, 'Second', { resident: true }));
    const second = center.history[0].id;
    native.send(new NativeNotification(3, 'Third', { expire_timeout: 20 }));
    const third = center.history[0].id;

    center.setHovered(third, true);
    advance(20);
    assert.equal(center.history.find(row => row.id === first)!.closed, true);
    assert.ok(native.get_notification(3), 'hover must pause only the matching live notification');
    center.invokeNotificationAction(first, 'open');
    center.invokeNotificationAction('missing', 'open');
    center.invokeNotificationAction(second, 'missing');
    assert.deepEqual(native.invoked, []);
    center.invokeNotificationAction(second, 'open');
    assert.deepEqual(native.invoked, [[2, 'open']]);

    native.send(new NativeNotification(2, 'Replacement', { resident: true, actions: [{ id: 'reply', label: 'Reply' }] }));
    center.invokeNotificationAction(second, 'open');
    center.invokeNotificationAction(second, 'reply');
    assert.deepEqual(native.invoked, [[2, 'open'], [2, 'reply']]);
    center.dismissNotification(second);
    assert.equal(native.get_notification(2), undefined);
    assert.deepEqual(native.resolutions, [[1, 1], [2, 2]]);
    center.dismissNotification(first);
    center.dismissNotification('missing');
    center.setHovered('missing', true);
    assert.deepEqual(center.history.map(row => row.id), [third]);
    center.setHovered(third, false);
    advance(20);
    assert.equal(native.get_notification(3), undefined);
    center.dispose();
});

test('DND is native truth, clears popups, releases transients and mirrors archive', () => {
    files.set(archivePath, JSON.stringify({ version: 1, dnd: true, history: [] }));
    settings.set('dont-disturb', false);
    const center = new NotificationCenter();
    assert.equal(center.dnd, false, 'explicit native false must override archive true');
    native.send(new NativeNotification(1, 'Normal'));
    native.send(new NativeNotification(2, 'Transient', { transient: true }));
    assert.equal(center.history.length, 1);
    assert.equal(center.popups.length, 2);
    native.dont_disturb = true;
    assert.equal(center.popups.length, 0);
    assert.equal(native.get_notification(2), undefined);
    native.send(new NativeNotification(3, 'Quiet transient', { transient: true }));
    assert.equal(native.get_notification(3), undefined);
    assert.equal(center.history.length, 1);
    assert.equal(JSON.parse(files.get(archivePath)!).dnd, true);
    assert.ok(writes.every(write => write.flags === 3));
    center.toggleDnd();
    assert.equal(native.dont_disturb, false);
    assert.equal(center.popups.length, 0);
    center.dispose();
});

test('reload retains ordering/actions and never pops up restored native notifications', () => {
    let center = new NotificationCenter();
    native.send(new NativeNotification(1, 'First'));
    native.send(new NativeNotification(2, 'Second'));
    native.send(new NativeNotification(1, 'Replacement later', { time: 105 }));
    const keys = center.history.map(row => row.id);
    center.dispose();
    center = new NotificationCenter();
    assert.equal(center.popups.length, 0);
    assert.deepEqual(center.history.map(row => row.id), keys);
    assert.ok(center.history.every(row => !row.closed && row.actions.length === 1));
    center.invokeNotificationAction(keys[0], 'open');
    assert.deepEqual(native.invoked, [[1, 'open']]);
    center.clearNotifications();
    center.dispose();
    // Simulate a native cache flush which had not yet observed clear.
    native.notifications = [new NativeNotification(2, 'Second')];
    center = new NotificationCenter();
    assert.deepEqual(center.history, []);
    assert.deepEqual(center.popups, []);
    assert.equal(native.notifications.length, 0);
    center.dispose();
});

test('archive migration is once-only, supports override paths and survives corrupt restore', () => {
    environment.set('NOTIFICATION_HISTORY', '/test/custom/history.json');
    environment.set('LEGACY_NOTIFICATION_HISTORY', '/test/legacy.json');
    files.set('/test/legacy.json', JSON.stringify({ version: 1, dnd: true, history: [
        { id: 'quickshell:1', app: 'App', summary: 'Legacy', body: '', actions: [{ identifier: 'open', text: 'Open' }] },
    ] }));
    let center = new NotificationCenter();
    assert.equal(center.dnd, true);
    assert.equal(center.history.length, 1);
    assert.equal(center.history[0].closed, true);
    assert.deepEqual(center.history[0].actions, []);
    center.clearNotifications(); center.dispose();
    center = new NotificationCenter();
    assert.deepEqual(center.history, []);
    center.dispose();
    files.set('/test/custom/history.json', '{broken');
    center = new NotificationCenter();
    assert.deepEqual(center.history, []);
    assert.equal(JSON.parse(files.get('/test/custom/history.json')!).version, 1);
    center.dispose();
});

test('limits release evicted native objects and unpause evicted hovered popups', () => {
    const center = new NotificationCenter();
    native.send(new NativeNotification(1, 'Hovered oldest', { expire_timeout: 500 }));
    center.setHovered(center.popups[0].id, true);
    for (let id = 2; id <= 6; id++) native.send(new NativeNotification(id, `Row ${id}`));
    assert.equal(center.popups.length, 5);
    advance(500);
    assert.equal(native.get_notification(1), undefined, 'evicted popup remained hovered forever');
    for (let id = 7; id <= 202; id++) native.send(new NativeNotification(id, `Row ${id}`));
    assert.equal(center.history.length, 200);
    assert.equal(center.history[0].summary, 'Row 202');
    assert.equal(native.get_notification(2), undefined);
    center.clearNotifications();
    for (let id = 300; id <= 305; id++) native.send(new NativeNotification(id, `Transient ${id}`, { transient: true }));
    assert.equal(center.history.length, 0);
    assert.equal(center.popups.length, 5);
    assert.equal(native.get_notification(300), undefined);
    center.dispose();
    assert.equal(timers.size, 0);
});
