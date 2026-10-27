// Deterministic pure/lifecycle coverage without GI, a display, D-Bus, or host telemetry.
import assert from 'node:assert/strict';
import { beforeEach, test } from 'node:test';
import { registerHooks } from 'node:module';
import path from 'node:path';
import type { NotificationRow, Palette } from './notification.ts';
import type { DashboardPanel } from './power.ts';

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
    constructor(properties: Record<string, any> = {}) {
        super();
        for (const [name, value] of Object.entries(properties))
            if (value === undefined) throw new TypeError(`Invalid value 'undefined' for property ${name}`);
        Object.assign(this, properties);
    }
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
const syncReads: string[] = [];
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
    shell_quote: (text: string) => `'${text.replaceAll("'", "'\\''")}'`,
};

class Child {
    argv: string[];
    flags: number;
    killed = false;
    done = false;
    signals: number[] = [];
    waitCancellable: any = undefined;
    reaped = 0;
    wait: ((child: Child, result: any) => void) | null = null;
    communicate: ((child: Child, result: any) => void) | null = null;
    constructor(argv: string[], flags = 0) { this.argv = argv; this.flags = flags; children.push(this); }
    wait_async(io: any, callback: any) { this.waitCancellable = io; this.wait = callback; }
    wait_finish() { this.reaped++; return true; }
    communicate_utf8_async(_input: any, _io: any, callback: any) { this.communicate = callback; }
    communicate_utf8_finish(result: any) { return [true, result, '']; }
    get_successful() { return !this.killed; }
    send_signal(signal: number) { this.signals.push(signal); }
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
    SubprocessFlags: { STDOUT_PIPE: 1, STDERR_SILENCE: 2, STDOUT_SILENCE: 4, STDERR_PIPE: 8 },
    SubprocessLauncher: class {
        flags: number;
        constructor(options: { flags: number }) { this.flags = options.flags; }
        setenv() {}
        spawnv(argv: string[]) { return new Child(argv, this.flags); }
    },
    File: { new_for_path(filename: string) {
        return {
            query_info() { if (!files.has(filename)) throw missing(); return { get_size: () => new TextEncoder().encode(files.get(filename)!).length }; },
            load_contents() { syncReads.push(filename); if (!files.has(filename)) throw missing(); return [true, new TextEncoder().encode(files.get(filename)!)]; },
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
let availableProfiles = [{ profile: 'balanced' }, { profile: 'power-saver' }];
const power = Object.assign(new Signals(), { get_profiles: () => availableProfiles, active_profile: 'balanced' });
const mockGI: Record<string, any> = {
    GLib, Gio, AstalNotifd: { get_default: () => native, ClosedReason: { EXPIRED: 1 } },
    AstalBattery: { get_default: () => device, UPower: class { constructor() { return upower; } },
        State: { CHARGING: 1, PENDING_CHARGE: 5, FULLY_CHARGED: 4 } },
    AstalPowerProfiles: { get_default: () => power },
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
    notificationTimeout, POPUP_LIMIT, restoreArchive, serializeArchive, invoke, iconButton,
} = await import('./notification.ts');
const {
    MetricsSampler, cpuPercent, emptyMetrics, parseCpu, parseDisk, parseMemory, parseTemperature,
} = await import('./parameters.ts');
const {
    ButtonSwitcher, DashboardApplication, DashboardBackend, parseRequest, SCREEN_TIMEOUT_CHOICES, ScreenTimeoutController,
} = await import('./power.ts');
hooks.deregister();

beforeEach(() => {
    clock = 0; serial = 0; timers.clear(); files.clear(); writes.length = 0; settings.clear(); environment.clear();
    reads.length = 0; syncReads.length = 0; children.length = 0; native = new Native();
    environment.set('XDG_STATE_HOME', '/test/state');
    device.is_present = true; device.percentage = 0.42; device.state = 1;
    upower.daemon_version = '1'; upower.on_battery = false;
    availableProfiles = [{ profile: 'balanced' }, { profile: 'power-saver' }]; power.active_profile = 'balanced';
});

const archivePath = '/test/state/dashboard/notifications.json';
const screenTimeoutPath = '/test/state/dashboard/screen-timeout.json';
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

test('icon buttons omit absent tooltips instead of passing undefined to GObject', () => {
    let clicked = 0;
    for (const tooltip of [null, '', 'Clear notifications']) {
        const button = iconButton('×', tooltip, () => { clicked++; }) as unknown as Button;
        assert.equal(Object.hasOwn(button, 'tooltip_text'), tooltip !== null);
        if (tooltip !== null) assert.equal(button.tooltip_text, tooltip);
        assert.equal((button.get_child() as Label).label, '×');
        button.emit('clicked');
    }
    assert.equal(clicked, 3);
});

test('switcher has no outside caption, keeps its frame transparent and exposes accessible arrows', () => {
    const { control, previous, next, value, indicators } = switcher();
    const widget = control.widget as unknown as Box;
    assert.ok(widget instanceof Box);
    assert.equal(widget.orientation, Gtk.Orientation.HORIZONTAL);
    assert.equal(widget.hexpand, true);
    assert.equal(widget.children.length, 1);
    assert.equal(widget.children.some(child => child instanceof Label), false);
    const frame = widget.children[0] as Overlay;
    assert.ok(frame instanceof Overlay);
    assert.equal(frame.hexpand, true);
    const drawing = frame.child as DrawingArea;
    const calls: string[] = [];
    const paint = new Map<string, number[]>();
    const context = Object.fromEntries(['moveTo', 'lineTo', 'closePath', 'setSourceRGBA', 'setLineWidth', 'stroke', 'fill', 'fillPreserve']
        .map(method => [method, (...args: number[]) => { calls.push(method); paint.set(method, args); }]));
    drawing.draw!(drawing, context, 320, 42);
    assert.ok(calls.includes('stroke'));
    assert.deepEqual(paint.get('setSourceRGBA'), [238 / 255, 238 / 255, 238 / 255, 0.5]);
    assert.deepEqual(paint.get('setLineWidth'), [0.35]);
    assert.equal(calls.includes('fill') || calls.includes('fillPreserve'), false);
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

test('sleep timer choices have exact labels and switcher arrows wrap without optimistic selection', () => {
    assert.deepEqual(SCREEN_TIMEOUT_CHOICES, [
        { id: '0', label: 'Sleep after: None' },
        { id: '10', label: 'Sleep after: 10 min' },
        { id: '30', label: 'Sleep after: 30 min' },
        { id: '60', label: 'Sleep after: 1h' },
        { id: '120', label: 'Sleep after: 2h' },
        { id: '180', label: 'Sleep after: 3h' },
        { id: '360', label: 'Sleep after: 6h' },
    ]);
    const { control, requests, previous, next, value, indicators } = switcher(SCREEN_TIMEOUT_CHOICES);
    assert.equal(indicators.children.length, 7);
    for (const [index, choice] of SCREEN_TIMEOUT_CHOICES.entries()) {
        control.update(choice.id);
        previous.emit('clicked');
        next.emit('clicked');
        assert.deepEqual(requests.splice(0), [
            SCREEN_TIMEOUT_CHOICES[(index + 6) % 7].id,
            SCREEN_TIMEOUT_CHOICES[(index + 1) % 7].id,
        ]);
        assert.equal(value.label, choice.label);
        assert.equal(value.accessible.get(Gtk.AccessibleProperty.LABEL), choice.label);
    }
});

test('screen timeout defaults to None without writes, a child or change notifications', () => {
    let changes = 0;
    const controller = new ScreenTimeoutController(() => changes++);
    assert.equal(controller.minutes, 0);
    controller.setMinutes(0);
    controller.setMinutes(0);
    controller.dispose(); controller.dispose();
    assert.equal(changes, 0);
    assert.equal(writes.length, 0);
    assert.equal(children.length, 0);
    assert.equal(timers.size, 0);
});

for (const [minutes, seconds] of [[10, 600], [30, 1800], [60, 3600], [120, 7200], [180, 10800], [360, 21600]]) {
    test(`sleep timer ${minutes} minutes requests suspend after ${seconds} idle seconds and persists synchronously`, () => {
        const observed: number[] = [];
        const controller = new ScreenTimeoutController(() => observed.push(controller.minutes));
        controller.setMinutes(minutes);
        assert.equal(controller.minutes, minutes);
        assert.deepEqual(observed, [minutes]);
        assert.deepEqual(JSON.parse(files.get(screenTimeoutPath)!), { version: 1, minutes });
        assert.deepEqual(writes, [{ path: screenTimeoutPath,
            flags: Gio.FileCreateFlags.PRIVATE | Gio.FileCreateFlags.REPLACE_DESTINATION }]);
        assert.equal(children.length, 1);
        const child = children[0];
        assert.deepEqual(child.argv, ['swayidle', '-w', '-C', '/dev/null', 'timeout', String(seconds),
            "exec 'systemctl' suspend"]);
        assert.equal(child.flags, Gio.SubprocessFlags.STDOUT_SILENCE, 'stderr must inherit, not be silenced or piped');
        assert.equal(child.waitCancellable, null);
        assert.equal(typeof child.wait, 'function');
        controller.setMinutes(minutes);
        assert.equal(children.length, 1, 'identical active selection must not spawn again');
        assert.equal(writes.length, 1);
        assert.deepEqual(observed, [minutes]);
        controller.dispose();
        assert.deepEqual(child.signals, [15]);
        assert.equal(child.killed, false);
        child.finish('');
        assert.equal(child.reaped, 1);
    });
}

test('sleep timer honors executable overrides and shell-quotes systemctl', () => {
    environment.set('SWAYIDLE', '/test/sway idle');
    environment.set('SYSTEMCTL', "/test/system'ctl with spaces");
    const controller = new ScreenTimeoutController(() => {});
    controller.setMinutes(30);
    assert.deepEqual(children[0].argv, ['/test/sway idle', '-w', '-C', '/dev/null', 'timeout', '1800',
        "exec '/test/system'\\''ctl with spaces' suspend"]);
    controller.dispose();
});

test('screen timeout rejects values outside the exact numeric choices without side effects', () => {
    let changes = 0;
    const controller = new ScreenTimeoutController(() => changes++);
    controller.setMinutes(10);
    for (const invalid of [-1, 1, 5, 10.5, 20, 61, 361, NaN, Infinity, -Infinity, '10', null, undefined, true]) {
        assert.throws(() => controller.setMinutes(invalid as number), String(invalid));
        assert.equal(controller.minutes, 10);
    }
    assert.equal(changes, 1);
    assert.equal(children.length, 1);
    assert.equal(writes.length, 1);
    assert.deepEqual(children[0].signals, []);
    assert.deepEqual(JSON.parse(files.get(screenTimeoutPath)!), { version: 1, minutes: 10 });
    controller.dispose();
});

for (const minutes of [0, 10, 30, 60, 120, 180, 360]) {
    test(`sleep timer reuses saved screen timeout of ${minutes} minutes without rewriting the preference`, () => {
        const saved = JSON.stringify({ version: 1, minutes });
        files.set(screenTimeoutPath, saved);
        const controller = new ScreenTimeoutController(() => {});
        assert.equal(controller.minutes, minutes);
        assert.equal(children.length, minutes === 0 ? 0 : 1);
        if (minutes) {
            assert.equal(children[0].argv[5], String(minutes * 60));
            assert.equal(children[0].argv[6], "exec 'systemctl' suspend");
            assert.equal(children[0].waitCancellable, null);
        }
        assert.deepEqual(syncReads, [screenTimeoutPath]);
        assert.equal(writes.length, 0);
        assert.equal(files.get(screenTimeoutPath), saved);
        controller.dispose();
    });
}

test('screen timeout ignores corrupt, unsupported and oversized saved preferences without loading oversized files', t => {
    t.mock.method(console, 'warn', () => {});
    for (const text of ['{broken', 'null', '[]', '{}', '{"minutes":10}',
        '{"version":2,"minutes":10}', '{"version":1}', '{"version":1,"minutes":"10"}',
        '{"version":1,"minutes":null}', '{"version":1,"minutes":true}',
        '{"version":1,"minutes":-1}', '{"version":1,"minutes":20}', '{"version":1,"minutes":10.5}',
        JSON.stringify({ version: 1, minutes: 10 }).padEnd(1025)]) {
        files.set(screenTimeoutPath, text);
        syncReads.length = 0;
        const controller = new ScreenTimeoutController(() => {});
        assert.equal(controller.minutes, 0, text);
        assert.equal(children.length, 0);
        assert.equal(writes.length, 0);
        assert.equal(files.get(screenTimeoutPath), text);
        if (text.length > 1024) assert.deepEqual(syncReads, [], 'size must be checked before reading contents');
        controller.dispose();
    }
});

test('screen timeout accepts a saved preference at the 1024-byte size boundary', () => {
    files.set(screenTimeoutPath, JSON.stringify({ version: 1, minutes: 60 }).padEnd(1024));
    const controller = new ScreenTimeoutController(() => {});
    assert.equal(controller.minutes, 60);
    assert.equal(children.length, 1);
    assert.deepEqual(syncReads, [screenTimeoutPath]);
    assert.equal(writes.length, 0);
    controller.dispose();
});

test('screen timeout saves and restores under ~/.local/state when XDG_STATE_HOME is absent', () => {
    environment.delete('XDG_STATE_HOME');
    const filename = '/test/home/.local/state/dashboard/screen-timeout.json';
    let controller = new ScreenTimeoutController(() => {});
    assert.equal(writes.length, 0);
    assert.equal(children.length, 0);
    controller.setMinutes(120);
    assert.deepEqual(JSON.parse(files.get(filename)!), { version: 1, minutes: 120 });
    assert.equal(writes[0].path, filename);
    controller.dispose();
    controller = new ScreenTimeoutController(() => {});
    assert.equal(controller.minutes, 120);
    assert.equal(children.length, 2);
    assert.equal(children[1].argv[5], '7200');
    assert.equal(writes.length, 1);
    controller.dispose();
});

test('sleep timer None stops the watcher without a power or lock command and can re-enable before its stale exit', async t => {
    const warnings = t.mock.method(console, 'warn', () => {});
    const observed: number[] = [];
    const controller = new ScreenTimeoutController(() => observed.push(controller.minutes));
    controller.setMinutes(10);
    const old = children[0];
    controller.setMinutes(0);
    assert.equal(controller.minutes, 0);
    assert.deepEqual(JSON.parse(files.get(screenTimeoutPath)!), { version: 1, minutes: 0 });
    assert.deepEqual(old.signals, [15]);
    assert.equal(old.killed, false, 'watchers should shut down gracefully');
    assert.equal(children.length, 1, 'None must not launch a suspend, resume or lock command');
    assert.equal(old.argv.includes('resume'), false, 'stopping the watcher must not trigger a power action');
    assert.equal(old.waitCancellable, null);
    controller.setMinutes(0);
    assert.equal(writes.length, 2);
    assert.equal(children.length, 1);
    controller.setMinutes(30);
    assert.equal(controller.minutes, 30);
    assert.equal(children.length, 2);
    old.finish(''); await delay();
    assert.equal(old.reaped, 1);
    assert.equal(controller.minutes, 30);
    assert.deepEqual(observed, [10, 0, 30]);
    assert.equal(warnings.mock.callCount(), 0);
    assert.deepEqual(children[1].signals, []);
    controller.dispose();
});

test('screen timeout replacement spawns and saves before committing and terminating the old child', async t => {
    let changes = 0;
    const controller = new ScreenTimeoutController(() => changes++);
    controller.setMinutes(10);
    const old = children[0];
    const open = Gio.File.new_for_path;
    const saving = t.mock.method(Gio.File, 'new_for_path', (filename: string) => {
        const file = open(filename);
        if (filename !== screenTimeoutPath) return file;
        return { ...file, replace_contents(...args: Parameters<typeof file.replace_contents>) {
            assert.equal(controller.minutes, 10, 'selection must not commit before persistence');
            assert.equal(children.length, 2, 'replacement must exist before persistence');
            assert.deepEqual(old.signals, []);
            return file.replace_contents(...args);
        } };
    });
    const signal = old.send_signal.bind(old);
    t.mock.method(old, 'send_signal', (number: number) => {
        assert.equal(controller.minutes, 30);
        assert.deepEqual(JSON.parse(files.get(screenTimeoutPath)!), { version: 1, minutes: 30 });
        signal(number);
    });
    controller.setMinutes(30);
    saving.mock.restore();
    assert.equal(controller.minutes, 30);
    assert.equal(changes, 2);
    assert.deepEqual(old.signals, [15]);
    assert.equal(old.killed, false);
    assert.deepEqual(children[1].signals, []);
    old.finish(''); await delay();
    assert.equal(old.reaped, 1);
    assert.equal(controller.minutes, 30);
    assert.equal(changes, 2, 'stale replacement exit must not emit a change');
    controller.dispose();
});

test('screen timeout unexpected active exit resets live state, warns and retains the preference for restart', async t => {
    const warnings = t.mock.method(console, 'warn', () => {});
    let changes = 0;
    const controller = new ScreenTimeoutController(() => changes++);
    controller.setMinutes(60);
    const child = children[0];
    child.finish(''); await delay();
    assert.equal(child.reaped, 1);
    assert.equal(controller.minutes, 0);
    assert.equal(changes, 2);
    assert.ok(warnings.mock.callCount() > 0, 'unexpected exit must log a warning');
    assert.deepEqual(JSON.parse(files.get(screenTimeoutPath)!), { version: 1, minutes: 60 });
    assert.equal(writes.length, 1);
    assert.equal(children.length, 1, 'unexpected exit must not enter an automatic restart loop');
    controller.dispose();
    assert.deepEqual(child.signals, []);
    const restarted = new ScreenTimeoutController(() => {});
    assert.equal(restarted.minutes, 60);
    assert.equal(children.length, 2);
    assert.equal(writes.length, 1);
    restarted.dispose();
});

test('screen timeout failed spawn preserves the active child, preference and notifications', t => {
    let changes = 0;
    const controller = new ScreenTimeoutController(() => changes++);
    controller.setMinutes(10);
    const spawn = t.mock.method(Gio.SubprocessLauncher.prototype, 'spawnv', () => { throw new Error('spawn failed'); });
    assert.throws(() => controller.setMinutes(30), /spawn failed/);
    assert.equal(spawn.mock.callCount(), 1);
    assert.equal(controller.minutes, 10);
    assert.equal(changes, 1);
    assert.equal(children.length, 1);
    assert.deepEqual(children[0].signals, []);
    assert.equal(children[0].killed, false);
    assert.deepEqual(JSON.parse(files.get(screenTimeoutPath)!), { version: 1, minutes: 10 });
    assert.equal(writes.length, 1);
    spawn.mock.restore();
    controller.setMinutes(30);
    assert.equal(controller.minutes, 30);
    assert.deepEqual(children[0].signals, [15]);
    controller.dispose();
});

test('screen timeout failed save terminates only the candidate and failed None preserves the active child', async t => {
    let changes = 0;
    const controller = new ScreenTimeoutController(() => changes++);
    controller.setMinutes(10);
    const old = children[0];
    const open = Gio.File.new_for_path;
    const saving = t.mock.method(Gio.File, 'new_for_path', (filename: string) => {
        const file = open(filename);
        return filename !== screenTimeoutPath ? file : { ...file,
            replace_contents() { throw new Error('save failed'); } };
    });
    assert.throws(() => controller.setMinutes(30), /save failed/);
    assert.equal(children.length, 2);
    const candidate = children[1];
    assert.deepEqual(candidate.signals, [15]);
    assert.equal(candidate.killed, false);
    assert.equal(candidate.waitCancellable, null);
    candidate.finish(''); await delay();
    assert.equal(candidate.reaped, 1);
    assert.equal(controller.minutes, 10);
    assert.equal(changes, 1, 'discarded candidate exit must not change active state');
    assert.throws(() => controller.setMinutes(0), /save failed/);
    assert.equal(controller.minutes, 10);
    assert.equal(changes, 1);
    assert.deepEqual(old.signals, []);
    assert.equal(old.killed, false);
    assert.deepEqual(JSON.parse(files.get(screenTimeoutPath)!), { version: 1, minutes: 10 });
    assert.equal(writes.length, 1);
    assert.equal(children.length, 2, 'failed None must not spawn a child');
    saving.mock.restore();
    controller.setMinutes(30);
    assert.equal(controller.minutes, 30);
    assert.deepEqual(old.signals, [15]);
    controller.dispose();
});

test('screen timeout failed restore spawn leaves None live without erasing the saved preference', t => {
    const warnings = t.mock.method(console, 'warn', () => {});
    const saved = JSON.stringify({ version: 1, minutes: 30 });
    files.set(screenTimeoutPath, saved);
    const spawn = t.mock.method(Gio.SubprocessLauncher.prototype, 'spawnv', () => { throw new Error('restore spawn failed'); });
    const controller = new ScreenTimeoutController(() => {});
    assert.equal(controller.minutes, 0);
    assert.equal(children.length, 0);
    assert.equal(writes.length, 0);
    assert.equal(files.get(screenTimeoutPath), saved);
    assert.equal(spawn.mock.callCount(), 1);
    assert.ok(warnings.mock.callCount() > 0);
    spawn.mock.restore();
    controller.setMinutes(30);
    assert.equal(controller.minutes, 30);
    assert.equal(children.length, 1);
    controller.dispose();
});

test('screen timeout disposal is idempotent, reaps with SIGTERM and makes all later setters inert', async () => {
    let changes = 0;
    const controller = new ScreenTimeoutController(() => changes++);
    controller.setMinutes(180);
    const child = children[0];
    controller.dispose(); controller.dispose();
    const disposedMinutes = controller.minutes;
    for (const value of [0, 30, NaN]) assert.doesNotThrow(() => controller.setMinutes(value));
    assert.equal(controller.minutes, disposedMinutes);
    assert.deepEqual(child.signals, [15]);
    assert.equal(child.killed, false);
    assert.equal(child.waitCancellable, null);
    assert.equal(writes.length, 1, 'disposal must not erase the saved preference');
    assert.deepEqual(JSON.parse(files.get(screenTimeoutPath)!), { version: 1, minutes: 180 });
    assert.equal(children.length, 1);
    const beforeExit = changes;
    child.finish(''); await delay();
    assert.equal(child.reaped, 1);
    assert.equal(controller.minutes, disposedMinutes);
    assert.equal(changes, beforeExit);
    controller.dispose();
    assert.deepEqual(child.signals, [15]);
});

test('dashboard actions safely report thrown and rejected non-Error values', async () => {
    const warnings: string[] = [];
    const warn = console.warn;
    console.warn = (message: unknown) => { warnings.push(String(message)); };
    try {
        for (const error of [new Error('failed'), 'failed', null, undefined]) {
            invoke(() => { throw error; });
            invoke(() => Promise.reject(error));
        }
        await delay();
        assert.deepEqual(warnings, [
            'Dashboard action: Error: failed', 'Dashboard action: failed',
            'Dashboard action: null', 'Dashboard action: undefined',
            'Dashboard action: Error: failed', 'Dashboard action: failed',
            'Dashboard action: null', 'Dashboard action: undefined',
        ]);
    } finally { console.warn = warn; }
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

test('backend native controls, fractional battery, screen timeout and read-only status', () => {
    const backend = new DashboardBackend();
    let changes = 0;
    const unsubscribe = backend.subscribe(() => changes++);
    assert.equal(backend.battery.percentage, 0.42);
    assert.equal(backend.battery.status, 'CHARGING');
    assert.equal(backend.screenTimeoutMinutes, 0);
    assert.equal('airplaneMode' in backend, false);
    assert.equal('toggleAirplaneMode' in backend, false);
    assert.equal(reads.length, 0);
    assert.equal(children.length, 0);
    const writesBefore = writes.length;
    for (let i = 0; i < 20; i++) assert.equal(backend.status().historyCount, 0);
    assert.equal(writes.length, writesBefore);
    assert.equal(timers.size, 0);
    backend.setScreenTimeout(10);
    assert.equal(backend.screenTimeoutMinutes, 10);
    assert.equal(backend.status().screenTimeoutMinutes, 10);
    assert.ok(changes > 0, 'timeout selection must synchronously notify subscribers');
    assert.equal(children.length, 1);
    assert.deepEqual(JSON.parse(files.get(screenTimeoutPath)!), { version: 1, minutes: 10 });
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
    assert.deepEqual(children[0].signals, [15]);
    assert.equal(children[0].killed, false);
    assert.equal(timers.size, 0);
});

test('backend keeps restored screen timeout running across panel close and status reads have no side effects', async () => {
    files.set(screenTimeoutPath, JSON.stringify({ version: 1, minutes: 30 }));
    const backend = new DashboardBackend();
    assert.equal(backend.screenTimeoutMinutes, 30);
    assert.equal(children.length, 1, 'saved timeout must run even before the panel opens');
    const child = children[0];
    const descriptor = Object.getOwnPropertyDescriptor(DashboardBackend.prototype, 'screenTimeoutMinutes')!;
    assert.equal(typeof descriptor.get, 'function');
    assert.equal(descriptor.set, undefined);
    backend.setActive(true);
    assert.equal(children.length, 2);
    const metricsChild = children[1];
    backend.setActive(false);
    completeReads(); await delay();
    assert.equal(metricsChild.killed, true, 'panel close still stops metrics');
    assert.equal(child.killed, false);
    assert.deepEqual(child.signals, [], 'panel close must not terminate swayidle');
    assert.equal(backend.screenTimeoutMinutes, 30);
    assert.equal(backend.status().screenTimeoutMinutes, 30);
    assert.equal('airplaneMode' in backend.status(), false);
    let changes = 0;
    backend.subscribe(() => changes++);
    const state = () => ({
        files: [...files], writes: [...writes], reads: reads.length, syncReads: [...syncReads],
        children: [...children], signals: [...child.signals], timers: timers.size, settings: [...settings],
        battery: backend.battery, history: backend.notifications.history, popups: backend.notifications.popups,
        resolutions: [...native.resolutions], notifications: [...native.notifications],
    });
    const before = state();
    const status = backend.status();
    for (let index = 0; index < 20; index++) {
        assert.equal(backend.screenTimeoutMinutes, 30);
        assert.deepEqual(backend.status(), status);
    }
    assert.deepEqual(state(), before);
    assert.equal(changes, 0);
    backend.dispose(); backend.dispose();
    assert.deepEqual(child.signals, [15]);
    assert.equal(child.killed, false);
    child.finish(''); await delay();
    assert.equal(child.reaped, 1);
    const afterDispose = { writes: writes.length, children: children.length, changes };
    backend.setScreenTimeout(60);
    assert.deepEqual({ writes: writes.length, children: children.length, changes }, afterDispose);
});

test('backend publishes unexpected timeout exit synchronously through its getter and subscribers', async t => {
    t.mock.method(console, 'warn', () => {});
    const backend = new DashboardBackend();
    const observed: number[] = [];
    backend.subscribe(() => observed.push(backend.screenTimeoutMinutes));
    backend.setScreenTimeout(60);
    assert.deepEqual(observed, [60]);
    children[0].finish(''); await delay();
    assert.equal(backend.screenTimeoutMinutes, 0);
    assert.equal(backend.status().screenTimeoutMinutes, 0);
    assert.deepEqual(observed, [60, 0]);
    assert.deepEqual(JSON.parse(files.get(screenTimeoutPath)!), { version: 1, minutes: 60 });
    backend.dispose();
});

test('power profile switcher uses the native getter and cycles all available profiles', () => {
    availableProfiles = profileChoices.map(choice => ({ profile: choice.id }));
    power.active_profile = 'power-saver';
    assert.equal('profiles' in power, false);
    const backend = new DashboardBackend();
    const control = new ButtonSwitcher('Power profile', profileChoices, profile => backend.setProfile(profile), palette);
    const render = () => control.update(backend.activeProfile, backend.profiles);
    const unsubscribe = backend.subscribe(render);
    try {
        render();
        assert.deepEqual(backend.profiles, ['power-saver', 'balanced', 'performance']);
        assert.equal(control.previous.sensitive, true);
        assert.equal(control.next.sensitive, true);
        for (const expected of ['balanced', 'performance', 'power-saver']) {
            (control.next as unknown as Button).emit('clicked');
            assert.equal(power.active_profile, expected);
            power.emit('notify::active-profile');
            assert.equal(backend.activeProfile, expected);
            assert.equal(control.value.label, profileChoices.find(choice => choice.id === expected)!.label);
        }
        (control.previous as unknown as Button).emit('clicked');
        assert.equal(power.active_profile, 'performance');
        power.emit('notify::active-profile');
        assert.equal(control.value.label, 'Performance');
        availableProfiles = [{ profile: 'balanced' }];
        power.active_profile = 'balanced';
        power.emit('notify::active-profile');
        assert.deepEqual(backend.profiles, ['balanced']);
        assert.equal(control.previous.sensitive, false);
        assert.equal(control.next.sensitive, false);
    } finally {
        unsubscribe();
        backend.dispose();
    }
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
        historyCount: 1, popupCount: 2, dnd: false, screenTimeoutMinutes: 0, activeProfile: 'balanced',
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
    const panelDouble = (state: Pick<DashboardPanel, 'opened' | 'connector'>): DashboardPanel => state as DashboardPanel;
    const backend = new DashboardBackend();
    const app = new DashboardApplication();
    app.backend = backend;
    app.panel = panelDouble({ opened: true, connector: 'DP-1' });
    assert.deepEqual(JSON.parse(app.handleRequest('{"command":"status"}')), {
        ...backend.status(), opened: true, monitor: 'DP-1',
    });
    app.panel = panelDouble({ opened: false, connector: '' });
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
