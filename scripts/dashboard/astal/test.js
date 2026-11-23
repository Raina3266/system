// Deterministic pure/lifecycle coverage without GI, a display, D-Bus, or host telemetry.
import assert from 'node:assert/strict';
import { beforeEach, test } from 'node:test';
import { registerHooks } from 'node:module';
import { readFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';
import { pathToFileURL } from 'node:url';
import * as drawingHelpers from '../../shared/astal/drawing.js';

test('dashboard entry, local imports and npm test use JavaScript directly', () => {
    const packageJson = JSON.parse(readFileSync(new URL('./package.json', import.meta.url), 'utf8'));
    assert.equal(packageJson.type, 'module');
    assert.equal(packageJson.scripts.test, 'node --test test.js');
    for (const name of ['frontend', 'backend', 'logic', 'test']) {
        const url = new URL(`./${name}.js`, import.meta.url);
        const source = readFileSync(url, 'utf8');
        assert.doesNotMatch(source, /\.(?:ts|tsx)\b/, `${name}.js`);
        for (const [, specifier] of source.matchAll(/(?:\bfrom\s*|\bimport\s*\(\s*)['"](\.[^'"]+)['"]/g)) {
            assert.match(specifier, /\.js$/, specifier);
            assert.ok(readFileSync(new URL(specifier, url), 'utf8').length, specifier);
        }
    }
});

test('notification history uses dim yellow interactions, a later dim red focus override and borderless popups', () => {
    const css = readFileSync(new URL('./style.css', import.meta.url), 'utf8');
    const rules = [...css.matchAll(/([^{}]+)\{([^{}]*)\}/g)];
    const matchingRules = (...selectors) => rules.filter(([, group]) => group.split(',').some((value) => selectors.includes(value.trim())));
    const rule = (selector) => {
        const [match] = matchingRules(selector);
        assert.ok(match, `Missing rule for ${selector}`);
        return match[2];
    };
    const base = rule('.dashboard-ui .notification-entry');
    assert.match(base, /border: 1px solid color-mix\(in srgb, var\(--dp_cyan\) 42%, transparent\);/);
    assert.match(base, /background: transparent;/);
    for (const state of ['hover', 'focus-visible', 'active']) {
        const entry = rule(`.dashboard-ui .notification-entry:${state}`);
        assert.match(entry, /border-color: color-mix\(in srgb, var\(--dp_yellow\) 45%, transparent\);/);
        assert.match(entry, /background: color-mix\(in srgb, var\(--dp_red\) 10%, transparent\);/);
        assert.doesNotMatch(entry, /(?:border(?:-width)?|padding|margin|min-height|min-width):/);
    }
    const focus = '.dashboard-ui .notification-entry:focus-visible';
    const focusRules = matchingRules(focus);
    assert.equal(focusRules.length, 2);
    assert.equal(focusRules[1][1].trim(), focus);
    assert.equal(focusRules[1][2].trim(), 'border-color: color-mix(in srgb, var(--dp_red) 55%, transparent);');
    for (const state of ['', ':hover', ':active']) {
        // Equal specificity makes the later focus rule win, even while hovered/pressed.
        const colors = matchingRules(focus, `.dashboard-ui .notification-entry${state}`).flatMap(([, , body]) =>
            [...body.matchAll(/border-color:\s*([^;]+);/g)].map((match) => match[1])
        );
        assert.deepEqual(colors, [
            'color-mix(in srgb, var(--dp_yellow) 45%, transparent)',
            'color-mix(in srgb, var(--dp_red) 55%, transparent)',
        ], `Focus must win over ${state || 'idle'}`);
    }
    for (const state of ['', '.hovered', ':focus-visible']) {
        const popupRules = matchingRules(`.dashboard-ui .popup-entry${state}`);
        assert.equal(popupRules.length, 1);
        assert.ok(rules.indexOf(popupRules[0]) > rules.indexOf(focusRules[1]), 'Popup override must follow the focus override');
        assert.equal(popupRules[0][2].trim(), 'background: transparent; border: none;');
    }
});

class Signals {
    handlers = new Map();
    serial = 0;

    connect(signal, callback) {
        const id = ++this.serial;
        this.handlers.set(id, { signal, callback });
        return id;
    }

    disconnect(id) {
        this.handlers.delete(id);
    }

    emit(signal, ...args) {
        for (const handler of [...this.handlers.values()]) if (handler.signal === signal) handler.callback(this, ...args);
    }
}

class Widget extends Signals {
    sensitive = true;
    visible = true;
    parent = null;
    controllers = [];

    get_next_sibling() {
        const siblings = this.parent?.children || [];
        return siblings[siblings.indexOf(this) + 1] || null;
    }

    get_prev_sibling() {
        const siblings = this.parent?.children || [];
        return siblings[siblings.indexOf(this) - 1] || null;
    }

    css_classes = [];
    accessible = new Map();

    constructor(properties = {}) {
        super();
        for (const [name, value] of Object.entries(properties)) {
            if (value === undefined) throw new TypeError(`Invalid value 'undefined' for property ${name}`);
        }
        Object.assign(this, properties);
    }

    add_css_class(name) {
        if (!this.css_classes.includes(name)) this.css_classes.push(name);
    }

    remove_css_class(name) {
        this.css_classes = this.css_classes.filter((value) => value !== name);
    }

    has_css_class(name) {
        return this.css_classes.includes(name);
    }

    update_property(properties, values) {
        properties.forEach((property, index) => this.accessible.set(property, values[index]));
    }

    get_parent() {
        return this.parent;
    }

    set_child(child) {
        this.child = child;
        child.parent = this;
    }

    set_focusable(focusable) {
        this.focusable = focusable;
    }

    add_controller(controller) {
        this.controllers.push(controller);
    }

    queue_draw() {}

    destroy() {}
}

class Box extends Widget {
    children = [];
    reorderCalls = 0;

    append(child) {
        child.parent = this;
        this.children.push(child);
    }

    remove(child) {
        this.children = this.children.filter((value) => value !== child);
        child.parent = null;
    }

    get_first_child() {
        return this.children[0] || null;
    }

    reorder_child_after(child, sibling) {
        this.reorderCalls++;
        this.children = this.children.filter((value) => value !== child);
        this.children.splice(sibling ? this.children.indexOf(sibling) + 1 : 0, 0, child);
    }
}

class Label extends Widget {
    label = '';

    constructor(properties = {}) {
        super(properties);
        Object.assign(this, properties);
    }
}

class Button extends Widget {
    get_child() {
        return this.child || null;
    }
}

class Overlay extends Widget {
    overlays = [];
    measured = new Map();

    add_overlay(child) {
        this.overlays.push(child);
    }

    set_measure_overlay(child, measure) {
        this.measured.set(child, measure);
    }
}

class DrawingArea extends Widget {
    draw = null;

    set_draw_func(callback) {
        this.draw = callback;
    }
}

class FlowBox extends Box {
    insert(child, position) {
        const wrapper = new Box();
        wrapper.append(child);
        wrapper.parent = this;
        this.children.splice(position < 0 ? this.children.length : position, 0, wrapper);
    }
}

class Image extends Widget {
    clear() {}

    set_from_gicon(icon) {
        this.gicon = icon;
    }
}

const Gtk = {
    init() {},
    Box,
    Label,
    Button,
    Overlay,
    DrawingArea,
    FlowBox,
    Image,
    ScrolledWindow: Widget,
    ProgressBar: class extends Widget {
        fraction = 0;
    },
    EventControllerKey: Widget,
    EventControllerMotion: Widget,
    GestureClick: Widget,
    Orientation: { HORIZONTAL: 0, VERTICAL: 1 },
    Align: { FILL: 0, START: 1, END: 2, CENTER: 3 },
    Overflow: { VISIBLE: 0, HIDDEN: 1 },
    SelectionMode: { NONE: 0 },
    PolicyType: { NEVER: 0, AUTOMATIC: 1 },
    PropagationPhase: { CAPTURE: 0, BUBBLE: 1 },
    AccessibleProperty: { LABEL: 0, DESCRIPTION: 1 },
};
let serial = 0;
const settings = new Map();
const environment = new Map();
const delay = () => new Promise((resolve) => setImmediate(resolve));

// Native I/O must be explicitly doubled by the test, never delegated to the host.
function unexpectedHostIO(operation) {
    assert.fail(`Unexpected host I/O: ${operation}`);
}

const GLib = {
    unsetenv(_name) {},
    PRIORITY_DEFAULT: 0,
    SOURCE_REMOVE: false,
    SOURCE_CONTINUE: true,
    getenv(name) {
        return environment.get(name) ?? null;
    },
    uuid_string_random: () => `uuid-${++serial}`,
    file_get_contents(_filename) {
        return unexpectedHostIO('GLib.file_get_contents');
    },
};

class Settings {
    static sync() {}

    constructor(_options) {}

    get_user_value(key) {
        return settings.get(key) ?? null;
    }

    set_strv(key, value) {
        settings.set(key, [...value]);
        return true;
    }
}

const Gio = {
    Settings,
    Cancellable: class {
        cancelled = false;

        cancel() {
            this.cancelled = true;
        }
    },
    Subprocess: {
        new(_argv, _flags) {
            return unexpectedHostIO('Gio.Subprocess.new');
        },
    },
    SubprocessFlags: { STDIN_PIPE: 16, STDOUT_PIPE: 1, STDERR_SILENCE: 2, STDOUT_SILENCE: 4, STDERR_PIPE: 8 },
    File: {
        new_for_path(_filename) {
            return unexpectedHostIO('Gio.File.new_for_path');
        },
        new_for_uri(_uri) {
            return unexpectedHostIO('Gio.File.new_for_uri');
        },
    },
};

class NativeNotification {
    time = 100;
    app_name = 'Test';
    app_icon = 'test-symbolic';
    image = '';

    body = 'Body';
    urgency = 1;
    transient = false;
    resident = false;
    expire_timeout = 0;
    actions = [{ id: 'open', label: 'Open' }];

    constructor(id, summary, overrides = {}) {
        this.id = id;
        this.summary = summary;
        Object.assign(this, overrides);
    }

    expire() {
        native.resolve(this.id, 1);
    }

    dismiss() {
        native.resolve(this.id, 2);
    }

    invoke(action) {
        native.invoked.push([this.id, action]);
        if (!this.resident) native.resolve(this.id, 3);
    }
}

class Native extends Signals {
    notifications = [];
    invoked = [];
    resolutions = [];
    ignore_timeout = false;
    default_timeout = 42;

    get dont_disturb() {
        return settings.get('dont-disturb') ?? false;
    }

    set dont_disturb(value) {
        settings.set('dont-disturb', value);
        this.emit('notify::dont-disturb');
    }

    get_notification(id) {
        return this.notifications.find((row) => row.id === id);
    }

    send(row) {
        const replaced = !!this.get_notification(row.id);
        this.notifications = [row, ...this.notifications.filter((existing) => existing.id !== row.id)];
        this.emit('notified', row.id, replaced);
    }

    resolve(id, reason) {
        if (!this.get_notification(id)) return;
        this.notifications = this.notifications.filter((row) => row.id !== id);
        this.resolutions.push([id, reason]);
        this.emit('resolved', id, reason);
    }
}

let native = new Native();
const device = Object.assign(new Signals(), { is_present: true, percentage: 0.42, state: 1 });
const upower = Object.assign(new Signals(), { daemon_version: '1', on_battery: false });
let availableProfiles = [{ profile: 'balanced' }, { profile: 'power-saver' }];
const power = Object.assign(new Signals(), { get_profiles: () => availableProfiles, active_profile: 'balanced' });
const mockGI = {
    GLib,
    Gio,
    AstalNotifd: { get_default: () => native, ClosedReason: { EXPIRED: 1 } },
    AstalBattery: {
        get_default: () => device,
        UPower: class {
            constructor() {
                return upower;
            }
        },
        State: { CHARGING: 1, PENDING_CHARGE: 5, FULLY_CHARGED: 4 },
    },
    AstalPowerProfiles: { get_default: () => power },
    // Native widget doubles exercise controls without starting the application.
    GObject: { registerClass: (_metadata, implementation) => implementation },
    Astal: {
        Application: class extends Signals {
            acquire_socket() {}

            hold() {}

            quit() {
                this.emit('shutdown');
            }

            async runAsync(_args) {
                this.emit('activate');
                await Promise.resolve();
                return 0;
            }
        },
        Window: Widget,
        WindowAnchor: { TOP: 1, BOTTOM: 2, LEFT: 4, RIGHT: 8 },
        Exclusivity: { IGNORE: 0 },
        Layer: { OVERLAY: 0 },
        Keymode: { NONE: 0 },
    },
    AstalIO: { write_sock: unexpectedHostIO, write_sock_finish: unexpectedHostIO },
    Gtk,
    Gdk: { BUTTON_PRIMARY: 1 },
    Pango: { EllipsizeMode: { END: 0 }, WrapMode: { WORD_CHAR: 0 } },
    Gtk4LayerShell: {},
    system: { programPath: '' },
};
globalThis[Symbol.for('dashboard-test-gi')] = mockGI;

function nativeHooks() {
    return registerHooks({
        resolve(specifier, context, next) {
            if (specifier.startsWith('gi://') || specifier === 'system') {
                const name = specifier === 'system' ? 'system' : specifier.slice(5).split('?')[0];
                return { url: `dashboard-test:${name}`, shortCircuit: true };
            }
            return next(specifier, context);
        },
        load(url, context, next) {
            if (url.startsWith('dashboard-test:')) {
                return { format: 'module', shortCircuit: true,
                    source: `export default globalThis[Symbol.for('dashboard-test-gi')][${JSON.stringify(url.slice(15))}];` };
            }
            return next(url, context);
        },
    });
}

const hooks = nativeHooks();
const { NotificationCenter, DashboardPipe, DashboardBackend } = await import('./backend.js');
const { emptyMetrics, parseRequest, SCREEN_TIMEOUT_CHOICES } = await import('./logic.js');
const {
    NotificationEntry,
    NotificationStack,
    invoke,
    iconButton,
    ButtonSwitcher,
    DashboardApplication,
    DashboardPanel: DashboardPanelWidget,
    main,
    isMainModule,
} = await import('./frontend.js');
hooks.deregister();

beforeEach(() => {
    serial = 0;
    settings.clear();
    environment.clear();
    native = new Native();
    device.is_present = true;
    device.percentage = 0.42;
    device.state = 1;
    upower.daemon_version = '1';
    upower.on_battery = false;
    availableProfiles = [{ profile: 'balanced' }, { profile: 'power-saver' }];
    power.active_profile = 'balanced';
});

test('native doubles reject unexpected host file and process I/O', () => {
    for (
        const [operation, action] of [
            ['GLib.file_get_contents', () => GLib.file_get_contents('/test/unexpected')],
            ['Gio.File.new_for_path', () => Gio.File.new_for_path('/test/unexpected')],
            ['Gio.File.new_for_uri', () => Gio.File.new_for_uri('file:///test/unexpected')],
            ['Gio.Subprocess.new', () => Gio.Subprocess.new(['unexpected-helper'], 0)],
        ]
    ) assert.throws(action, { message: `Unexpected host I/O: ${operation}` });
});

test('GJS entry detection compares frontend.js or bundled file identity, not just the filename', (t) => {
    const previous = mockGI.system.programPath;
    const fromURI = t.mock.method(Gio.File, 'new_for_uri', (uri) => ({
        equal(file) {
            return uri === file.uri;
        },
    }));
    const fromPath = t.mock.method(Gio.File, 'new_for_path', (path) => ({ uri: pathToFileURL(path).href }));
    try {
        assert.equal(isMainModule('file:///test/dashboard/astal/frontend.js'), false);
        assert.equal(fromURI.mock.callCount(), 0);
        assert.equal(fromPath.mock.callCount(), 0);
        for (
            const path of ['/test/dashboard/astal/frontend.js', '/test/dashboard with spaces/astal/frontend.js', '/nix/store/test-dashboard/dashboard-panel.js']
        ) {
            mockGI.system.programPath = path;
            const entry = pathToFileURL(path);
            assert.equal(isMainModule(entry.href), true);
            assert.equal(isMainModule(new URL('./backend.js', entry).href), false);
            assert.equal(isMainModule('file:///test/unrelated/frontend.js'), false);
        }
    } finally {
        mockGI.system.programPath = previous;
    }
});

test('import-only frontend evaluation does not initialize GTK or start a notification owner even as the entry', async (t) => {
    const previous = mockGI.system.programPath;
    mockGI.system.programPath = '/test/dashboard/astal/frontend.js';
    environment.set('DASHBOARD_PANEL_IMPORT_ONLY', '1');
    const fromURI = t.mock.method(Gio.File, 'new_for_uri', () => ({ equal: () => true }));
    t.mock.method(Gio.File, 'new_for_path', () => ({}));
    const gtk = t.mock.method(Gtk, 'init');
    const notifd = t.mock.method(mockGI.AstalNotifd, 'get_default');
    const battery = t.mock.method(mockGI.AstalBattery, 'get_default');
    const profiles = t.mock.method(mockGI.AstalPowerProfiles, 'get_default');
    const run = t.mock.method(mockGI.Astal.Application.prototype, 'runAsync');
    const imports = nativeHooks();
    try {
        const frontend = await import(new URL('./frontend.js?import-only', import.meta.url).href);
        assert.equal(typeof frontend.main, 'function');
        assert.equal(fromURI.mock.callCount(), 1, 'Exercise the positive entry-file check before the import-only guard');
        for (const method of [gtk, notifd, battery, profiles, run]) assert.equal(method.mock.callCount(), 0);
        assert.equal(native.ignore_timeout, false);
        assert.equal(native.default_timeout, 42);
        assert.deepEqual([...settings], []);
    } finally {
        imports.deregister();
        mockGI.system.programPath = previous;
        environment.delete('DASHBOARD_PANEL_IMPORT_ONLY');
    }
});

test('backend imports only native bindings, shared Gio transport and pure logic without reapplying policy', async (t) => {
    const backendURL = new URL('./backend.js?native-boundary', import.meta.url).href;
    const logicURL = new URL('./logic.js?native-boundary', import.meta.url).href;
    const allowed = new Set(['Gio', 'GLib', 'AstalBattery', 'AstalPowerProfiles', 'AstalNotifd']);
    const notifd = t.mock.method(mockGI.AstalNotifd, 'get_default');
    const imports = registerHooks({
        resolve(specifier, context, next) {
            if (context.parentURL === logicURL) assert.fail(`Pure display logic imported ${specifier}`);
            if (context.parentURL === backendURL) {
                if (specifier === './logic.js') return { url: logicURL, shortCircuit: true };
                if (specifier === '../../shared/astal/ipc.js') return next(specifier, context);
                const name = specifier.startsWith('gi://') ? specifier.slice(5).split('?')[0] : specifier;
                assert.ok(allowed.has(name), `Native backend must not depend on widgets or non-Gio I/O adapters: ${specifier}`);
                return { url: `dashboard-test:${name}`, shortCircuit: true };
            }
            return next(specifier, context);
        },
    });
    let backend;
    try {
        const module = await import(backendURL);
        assert.equal(notifd.mock.callCount(), 0, 'Importing the backend must not acquire the notification owner');
        backend = new module.DashboardBackend(connect);
        assert.equal(notifd.mock.callCount(), 1);
        // Deliberately cross the Rust retention/admission limits: JS must consume
        // the authoritative arrays verbatim, not enforce another copy of policy.
        const history = Array.from({ length: 205 }, (_, i) => row(String(i), { transient: i % 2 === 0 }));
        const popups = history.slice(0, 6);
        deliver({ event: 'notifications', history, popups });
        assert.equal(backend.notifications.history, history);
        assert.equal(backend.notifications.popups, popups);
        deliver({ event: 'timeout', minutes: 77 });
        assert.equal(backend.screenTimeoutMinutes, 77);
        assert.equal(backend.status().historyCount, history.length);
        assert.equal(backend.status().popupCount, popups.length);
        backend.dispose();
        const count = sent.length;
        native.send(new NativeNotification(1, 'after disposal'));
        power.active_profile = 'power-saver';
        power.emit('notify::active-profile');
        assert.equal(sent.length, count);
        assert.equal(backend.activeProfile, 'balanced');
    } finally {
        backend?.dispose();
        imports.deregister();
    }
});

// Widget tests supply authoritative events explicitly, not a second policy implementation.
let deliver;
const sent = [];

function connect(receive) {
    deliver = receive;
    sent.length = 0;
    return {
        send(frame) {
            sent.push(frame);
        },
        whenDrained(cb) {
            queueMicrotask(cb);
        },
        dispose() {
            receive({ event: 'offline' });
        },
    };
}

const row = (id, overrides = {}) => ({ id, app: 'Test', summary: 'Summary', body: 'Body', icon: 'test-symbolic', urgency: 1,
    transient: false, timeout: -1, actions: [{ identifier: 'open', text: 'Open' }], closed: false, ...overrides });
const palette = { background: '#101010', secondaryBackground: '#202020', elevated: '#303030', foreground: '#eeeeee', separator: '#404040',
    cyan: '#00ffff', pink: '#ff00ff', dimPink: '#804080', yellow: '#ffff00', red: '#ff0000' };

test('dashboard palette defines scoped CSS variables and preserves palette validation', () => {
    const source = readFileSync(new URL('./frontend.js', import.meta.url), 'utf8');
    const definition = source.match(/function loadTheme\([^]*?\n\}/)?.[0];
    assert.ok(definition);
    let contents = JSON.stringify({ ...palette, accent: '#123ABC', bad_key: '#112233', invalid: 'red' });
    const reads = [];
    const styles = [];
    const provider = {};
    let readable = true;
    const sharedTheme = readFileSync(new URL('../../shared/astal/theme.js', import.meta.url), 'utf8').replace(/^import .*\n/gm, '').replace(
        /^export /gm,
        '',
    );
    const loadTheme = runInNewContext(sharedTheme + '\n' + definition + '\nloadTheme;', {
        TextDecoder,
        GLib: {
            file_get_contents(path) {
                reads.push(path);
                return [readable, new TextEncoder().encode(contents)];
            },
        },
        loadPanelStyle(style) {
            styles.push(style);
            return provider;
        },
    });
    const result = loadTheme('/dashboard/astal/frontend.js', '/dashboard/palette.json');
    assert.deepEqual(reads, ['/dashboard/palette.json']);
    assert.equal(JSON.stringify(result.palette), contents);
    assert.equal(result.provider, provider);
    assert.equal(styles.length, 1);
    assert.equal(styles[0].entry, '/dashboard/astal/frontend.js');
    assert.equal(styles[0].label, 'Dashboard');
    const definitions = styles[0].definitions;
    assert.match(definitions, /^\.dashboard-ui \{\n[^]*\n\}$/);
    for (const [key, value] of Object.entries({ ...palette, accent: '#123ABC' })) {
        assert.ok(definitions.includes(`    --dp_${key}: ${value};`), key);
    }
    assert.doesNotMatch(definitions, /bad_key|invalid|@define-color|@dp_|:root/);
    const declared = new Set([...definitions.matchAll(/(--[^\s:]+):/g)].map((match) => match[1]));
    assert.equal(declared.size, Object.keys(palette).length + 1);
    const css = readFileSync(new URL('./style.css', import.meta.url), 'utf8');
    const referenced = [...css.matchAll(/\bvar\(\s*(--[^,\s)]+)/g)];
    assert.ok(referenced.length);
    for (const [, name] of referenced) assert.ok(declared.has(name), `Undefined CSS variable: ${name}`);
    assert.doesNotMatch(css, /@dp_|@define-color|\balpha\(/);
    assert.match(css, /color-mix\(in srgb, var\(--dp_pink\) 33\.3%, transparent\)/);
    for (const key of Object.keys(palette)) {
        contents = JSON.stringify({ ...palette, [key]: 'invalid' });
        assert.throws(() => loadTheme('/dashboard/astal/frontend.js', '/dashboard/palette.json'), { message: `Invalid dashboard palette colour: ${key}` });
    }
    contents = '{';
    assert.throws(() => loadTheme('/dashboard/astal/frontend.js', '/dashboard/palette.json'), { name: 'SyntaxError' });
    assert.throws(() => loadTheme('/dashboard/astal/frontend.js', null), { message: 'Run through dashboard-panel, or set DASHBOARD_PANEL_CONFIG and DASHBOARD_PANEL_PALETTE.' });
    readable = false;
    assert.throws(() => loadTheme('/dashboard/astal/frontend.js', '/dashboard/palette.json'), { message: 'Could not read dashboard theme: /dashboard/palette.json' });
    assert.equal(styles.length, 1, 'Invalid palettes must not reach the stylesheet loader');
});

test('dashboard frames keep glow allocation, small-frame cut clamping and urgency colours', () => {
    const source = readFileSync(new URL('./frontend.js', import.meta.url), 'utf8');
    const definition = source.match(/class CutCornerFrame \{[^]*?\n\}/)?.[0];
    assert.ok(definition);
    const Frame = runInNewContext(definition + '\nCutCornerFrame;', { Gtk, ...drawingHelpers, rgb: drawingHelpers.hexRgb });
    for (const glow of [0, 3]) {
        const child = new Widget();
        const frame = new Frame(palette, child, { padding: 4, glow, width: 100 });
        assert.equal(frame.widget.width_request, 100 + glow * 2);
        for (const side of ['top', 'bottom', 'start', 'end']) assert.equal(child[`margin_${side}`], 4 + glow);
        for (const [allocatedWidth, allocatedHeight] of [[100, 80], [14, 10], [1, 1]]) {
            const calls = [];
            const cr = Object.fromEntries(
                ['moveTo', 'lineTo', 'closePath', 'setSourceRGBA', 'fillPreserve', 'setLineWidth', 'stroke', 'save', 'translate', 'restore']
                    .map((name) => [name, (...args) => calls.push([name, ...args])]),
            );
            frame.drawing.draw(frame.drawing, cr, allocatedWidth, allocatedHeight);
            const x = glow + 0.5, y = glow + 0.5;
            const width = allocatedWidth - glow * 2 - 1, height = allocatedHeight - glow * 2 - 1;
            if (width <= 0 || height <= 0) {
                assert.deepEqual(calls, []);
                continue;
            }
            const cut = Math.min(10, width / 2, height / 2);
            const path = [
                ['moveTo', x + cut, y],
                ['lineTo', x + width, y],
                ['lineTo', x + width, y + height - cut],
                ['lineTo', x + width - cut, y + height],
                ['lineTo', x, y + height],
                ['lineTo', x, y + cut],
                ['closePath'],
            ];
            const expected = [];
            if (glow) {
                expected.push(['save'], ['translate', 0, 3]);
                for (let radius = glow; radius > 0; radius--) {
                    expected.push(...path, ['setSourceRGBA', 1, 1, 0, 0.025], ['setLineWidth', radius * 2], ['stroke']);
                }
                expected.push(['restore']);
            }
            expected.push(...path, ['setSourceRGBA', 37 / 255, 37 / 255, 47 / 255, 0.9], ['fillPreserve'], ['setSourceRGBA', 1, 1, 0, 1], ['setLineWidth', 1], ['stroke']);
            assert.deepEqual(calls, expected);
            calls.length = 0;
            frame.setUrgency(2, palette);
            frame.drawing.draw(frame.drawing, cr, allocatedWidth, allocatedHeight);
            assert.deepEqual(calls.filter(([name]) => name === 'setSourceRGBA').at(-1), ['setSourceRGBA', 1, 0, 0, 1]);
            frame.setUrgency(0, palette);
        }
    }
});

for (const failed of [false, true]) {
    test(`dashboard socket replies synchronously and preserves String(error) diagnostics (${failed ? 'failure' : 'success'})`, (t) => {
        const app = new DashboardApplication();
        const events = [], writes = [], warnings = [];
        let complete;
        t.mock.method(app, 'handleRequest', () => {
            events.push('handle');
            return 'ok';
        });
        t.mock.method(mockGI.AstalIO, 'write_sock', (_connection, response, callback) => {
            events.push('write');
            writes.push(response);
            complete = callback;
        });
        t.mock.method(mockGI.AstalIO, 'write_sock_finish', () => {
            events.push('write-finish');
            if (failed) throw new Error('write failed');
        });
        t.mock.method(console, 'warn', (warning) => warnings.push(warning));
        const connection = {
            close_async(priority, cancellable, callback) {
                assert.equal(priority, GLib.PRIORITY_DEFAULT);
                assert.equal(cancellable, null);
                events.push('close');
                callback(null, {});
            },
            close_finish() {
                events.push('close-finish');
                if (failed) throw new Error('close failed');
            },
        };
        assert.equal(app.vfunc_request('{}', connection), undefined);
        assert.deepEqual(events, ['handle', 'write']);
        assert.deepEqual(writes, ['ok']);
        complete(null, {});
        assert.deepEqual(events, ['handle', 'write', 'write-finish', 'close', 'close-finish']);
        assert.deepEqual(
            warnings,
            failed ? ['Dashboard request reply: Error: write failed', 'Dashboard request close: Error: close failed'] : [],
        );
    });
}

const profileChoices = [{ id: 'power-saver', label: 'Power Saver' }, { id: 'balanced', label: 'Balanced' }, { id: 'performance', label: 'Performance' }];

function switcher(choices = profileChoices) {
    const requests = [];
    const control = new ButtonSwitcher('Profile', choices, (id) => requests.push(id), palette);
    const previous = control.previous;
    const next = control.next;
    const value = control.value;
    const indicators = control.indicators;
    return { control, requests, previous, next, value, indicators };
}

function assertNoTooltips(widget) {
    for (const property of ['tooltip_text', 'tooltip_markup', 'has_tooltip']) {
        assert.equal(Object.hasOwn(widget, property), false, `${widget.constructor.name}.${property}`);
    }
    for (const child of [...widget.children || [], ...widget.overlays || [], ...widget.child ? [widget.child] : []]) {
        assertNoTooltips(child);
    }
}

test('dashboard widget sources never configure or activate visual tooltips', () => {
    const source = readFileSync(new URL('./frontend.js', import.meta.url), 'utf8');
    assert.doesNotMatch(
        source,
        /\b(?:tooltip[_-](?:text|markup)|has[_-]tooltip|set_tooltip(?:_\w+)?|set_has_tooltip|trigger_tooltip_query|query-tooltip)\b/,
        'frontend.js',
    );
});

test('icon buttons keep optional accessible labels and callbacks without visual tooltips', () => {
    let clicked = 0;
    for (const accessibleLabel of [null, '', 'Clear notifications']) {
        const button = iconButton('×', accessibleLabel, () => {
            clicked++;
        });
        assertNoTooltips(button);
        assert.equal(button.accessible.has(Gtk.AccessibleProperty.LABEL), accessibleLabel !== null);
        if (accessibleLabel !== null) assert.equal(button.accessible.get(Gtk.AccessibleProperty.LABEL), accessibleLabel);
        assert.equal(button.get_child().label, '×');
        button.emit('clicked');
    }
    assert.equal(clicked, 3);
});

test('dashboard controls keep sleep descriptions and dynamic DND labels without visual tooltips', () => {
    const backend = new DashboardBackend(connect);
    const panel = new DashboardPanelWidget({ app: new DashboardApplication(), backend, palette });
    const sleep = panel.screenTimeout.widget;
    const dnd = panel.dnd;
    const clear = panel.clear;
    assert.equal(
        sleep.accessible.get(Gtk.AccessibleProperty.DESCRIPTION),
        'Suspend the computer after inactivity. None disables automatic sleep. Locking is unchanged; keyboard/mouse wake depends on hardware.',
    );
    assert.equal(panel.screenTimeout.previous.accessible.get(Gtk.AccessibleProperty.LABEL), 'Previous sleep timer');
    assert.equal(panel.screenTimeout.next.accessible.get(Gtk.AccessibleProperty.LABEL), 'Next sleep timer');
    assert.equal(clear.accessible.get(Gtk.AccessibleProperty.LABEL), 'Clear all');
    assert.equal(dnd.accessible.get(Gtk.AccessibleProperty.LABEL), 'Enable do not disturb');
    for (const enabled of [false, true, false]) {
        if (backend.notifications.dnd !== enabled) {
            dnd.emit('clicked');
            assert.equal(sent.at(-1).command.kind, 'toggle');
            deliver({ event: 'toggle' });
        }
        panel.render();
        assert.equal(backend.notifications.dnd, enabled);
        assert.equal(dnd.accessible.get(Gtk.AccessibleProperty.LABEL), enabled ? 'Disable do not disturb' : 'Enable do not disturb');
        assert.equal(dnd.get_child().label, enabled ? '󰂛' : '󰂚');
        assertNoTooltips(panel.window);
    }
    panel.dispose();
    backend.dispose();
});

test('notification stacks skip unchanged row updates and already-correct ordering', () => {
    const stack = new NotificationStack({
        setHovered() {},
        dismissNotification() {},
        invokeNotificationAction() {},
    }, palette);
    const first = row('first', { icon: '' });
    const second = row('second', { icon: '' });
    stack.sync([first, second]);
    const widget = stack.widget;
    const firstEntry = stack.rows.get('first').entry;
    let updates = 0;
    firstEntry.update = () => {
        updates++;
    };
    widget.reorderCalls = 0;
    stack.sync([first, second]);
    assert.equal(updates, 0);
    assert.equal(widget.reorderCalls, 0);
    stack.sync([{ ...first, body: 'Updated' }, second]);
    assert.equal(updates, 1);
    assert.equal(widget.reorderCalls, 0);
    stack.sync([second, first]);
    assert.equal(widget.reorderCalls, 1);
    assert.deepEqual(widget.children, [stack.rows.get('second').widget, stack.rows.get('first').widget]);
    stack.dispose();
});

for (const popup of [false, true]) {
    test(`notification ${popup ? 'popup' : 'history'} controls keep full accessible labels and actions without visual tooltips`, () => {
        const invoked = [];
        const dismissed = [];
        const hovered = [];
        const notification = row('live', {
            icon: '',
            actions: [{ identifier: 'default', text: 'Default' }, { identifier: 'open', text: 'Open the full notification details & preferences' }],
        });
        const entry = new NotificationEntry(notification, {
            setHovered: (id, value) => {
                hovered.push([id, value]);
            },
            dismissNotification: (id) => {
                dismissed.push(id);
            },
            invokeNotificationAction: (id, identifier) => {
                invoked.push([id, identifier]);
            },
        }, popup);
        const actionButton = () => entry.actions.children[0].children[0];
        const button = actionButton();
        assert.equal(entry.actions.children.length, 1);
        assert.equal(button.accessible.get(Gtk.AccessibleProperty.LABEL), notification.actions[1].text);
        assert.equal(button.get_child().label, notification.actions[1].text);
        assert.equal(button.get_child().use_markup, false);
        assert.equal(entry.close.accessible.get(Gtk.AccessibleProperty.LABEL), 'Dismiss notification');
        assertNoTooltips(entry.widget);
        button.emit('clicked');
        assert.deepEqual(invoked, [['live', 'open']]);
        entry.update({ ...notification, body: 'Updated body' });
        assert.equal(actionButton(), button);
        entry.update({ ...notification, actions: [{ identifier: 'reply', text: 'Reply to this notification' }] });
        assert.notEqual(actionButton(), button);
        assert.equal(actionButton().accessible.get(Gtk.AccessibleProperty.LABEL), 'Reply to this notification');
        assertNoTooltips(entry.widget);
        actionButton().emit('clicked');
        entry.close.emit('clicked');
        assert.deepEqual(invoked, [['live', 'open'], ['live', 'reply']]);
        assert.deepEqual(dismissed, ['live']);
        const motion = entry.widget.controllers.at(-1);
        motion.emit('enter');
        entry.dispose();
        assert.deepEqual(hovered, popup ? [['live', true], ['live', false]] : []);
    });
}

test('switcher has no outside caption, keeps its frame transparent and exposes accessible arrows', () => {
    const { control, previous, next, value, indicators } = switcher();
    const widget = control.widget;
    assert.ok(widget instanceof Box);
    assert.equal(widget.orientation, Gtk.Orientation.HORIZONTAL);
    assert.equal(widget.hexpand, true);
    assert.equal(widget.children.length, 1);
    assert.equal(widget.children.some((child) => child instanceof Label), false);
    const frame = widget.children[0];
    assert.ok(frame instanceof Overlay);
    assert.equal(frame.hexpand, true);
    const drawing = frame.child;
    const calls = [];
    const paint = new Map();
    const context = Object.fromEntries(
        ['moveTo', 'lineTo', 'closePath', 'setSourceRGBA', 'setLineWidth', 'stroke', 'fill', 'fillPreserve'].map((method) => [
            method,
            (...args) => {
                calls.push(method);
                paint.set(method, args);
            },
        ]),
    );
    drawing.draw(drawing, context, 320, 42);
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
    // Availability order must not reorder the declared choices; unknown IDs are ignored.
    const { control, requests, previous, next } = switcher();
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
    // Emit directly, even though GTK would normally suppress disabled activation.
    const { control, requests, previous, next, value } = switcher();
    control.update('balanced', ['balanced']);
    assert.equal(value.label, 'Balanced');
    assert.equal(previous.sensitive, false);
    assert.equal(next.sensitive, false);
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
    const tickState = () => indicators.children.map((tick) => [...tick.css_classes]);
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
    const state = () => indicators.children.map((tick) => ({ classes: [...tick.css_classes].sort(), visible: tick.visible }));
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
        assert.deepEqual(requests.splice(0), [SCREEN_TIMEOUT_CHOICES[(index + 6) % 7].id, SCREEN_TIMEOUT_CHOICES[(index + 1) % 7].id]);
        assert.equal(value.label, choice.label);
        assert.equal(value.accessible.get(Gtk.AccessibleProperty.LABEL), choice.label);
    }
});

test('dashboard actions safely report thrown and rejected non-Error values', async () => {
    const warnings = [];
    const warn = console.warn;
    console.warn = (message) => {
        warnings.push(String(message));
    };
    try {
        for (const error of [new Error('failed'), 'failed', null, undefined]) {
            invoke(() => {
                throw error;
            });
            invoke(() => Promise.reject(error));
        }
        await delay();
        assert.deepEqual(warnings, [
            'Dashboard action: Error: failed',
            'Dashboard action: failed',
            'Dashboard action: null',
            'Dashboard action: undefined',
            'Dashboard action: Error: failed',
            'Dashboard action: failed',
            'Dashboard action: null',
            'Dashboard action: undefined',
        ]);
    } finally {
        console.warn = warn;
    }
});

test('strict JSON request envelope and command arity', () => {
    for (const command of ['status', 'toggle', 'close', 'toggleDnd', 'clearNotifications']) {
        assert.deepEqual(parseRequest(JSON.stringify({ command })), { command });
    }
    for (const monitor of ['', 'DP-1', 'HDMI-A-2']) {
        assert.deepEqual(parseRequest(JSON.stringify({ command: 'toggle', monitor })), { command: 'toggle', monitor });
    }
    for (
        const request of [
            null,
            [],
            'status',
            {},
            { command: 'bad' },
            { command: 'toggleIdleInhibit' },
            { command: 'status', extra: true },
            { command: 'status', monitor: 'DP-1' },
            { command: 'toggleDnd', monitor: '' },
            { command: 'close', args: [] },
            { command: 'toggle', monitor: null },
            { command: 'toggle', monitor: 4 },
            { command: 'toggle', monitor: 'a'.repeat(257) },
            { command: 'toggle', monitor: 'DP-1\nclose' },
            { command: 'toggle', monitor: '\u007f' },
        ]
    ) assert.throws(() => parseRequest(JSON.stringify(request)), JSON.stringify(request));
    for (const text of ['', 'status', '{} {}', '{broken']) assert.throws(() => parseRequest(text));
});

test('power profile switcher uses the native getter and cycles all available profiles', () => {
    availableProfiles = profileChoices.map((choice) => ({ profile: choice.id }));
    power.active_profile = 'power-saver';
    assert.equal('profiles' in power, false);
    const backend = new DashboardBackend(connect);
    const control = new ButtonSwitcher('Power profile', profileChoices, (profile) => backend.setProfile(profile), palette);
    const render = () => control.update(backend.activeProfile, backend.profiles);
    const unsubscribe = backend.subscribe(render);
    try {
        render();
        assert.deepEqual(backend.profiles, ['power-saver', 'balanced', 'performance']);
        assert.equal(control.previous.sensitive, true);
        assert.equal(control.next.sensitive, true);
        for (const expected of ['balanced', 'performance', 'power-saver']) {
            control.next.emit('clicked');
            assert.equal(power.active_profile, expected);
            power.emit('notify::active-profile');
            assert.equal(backend.activeProfile, expected);
            assert.equal(control.value.label, profileChoices.find((choice) => choice.id === expected).label);
        }
        control.previous.emit('clicked');
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
    const backend = new DashboardBackend(connect);
    const icons = ['󰁺', '󰁻', '󰁼', '󰁽', '󰁾', '󰁿', '󰂀', '󰂁', '󰂂', '󰁹'];
    for (let percent = 0; percent <= 100; percent++) {
        device.percentage = percent / 100;
        device.emit('notify');
        const status = backend.status();
        assert.equal(status.text, `${icons[Math.min(9, Math.floor(percent / 10))]} ${percent}%`);
        assert.equal(
            status.class,
            percent <= 15 ? 'critical' : percent <= 25 ? 'warning' : percent < 50 ? 'low' : 'discharging',
            `${percent}%`,
        );
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
    const backend = new DashboardBackend(connect);
    for (
        const [state, expectedClass, expectedStatus] of [[1, 'charging', 'CHARGING'], [5, 'charging', 'CONNECTED · CHARGING SOON'], [4, 'full', 'FULLY CHARGED']]
    ) {
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
    const backend = new DashboardBackend(connect);
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
    const backend = new DashboardBackend(connect);
    native.send(new NativeNotification(1, 'History'));
    native.send(new NativeNotification(2, 'Transient', { transient: true }));
    deliver({ event: 'notifications', history: [row('one')], popups: [row('two'), row('one')] });
    assert.deepEqual(backend.status(), { text: '󰂄 42%', tooltip: 'CHARGING | Profile: balanced | Click: Dashboard', class: 'charging',
        historyCount: 1, popupCount: 2, dnd: false, screenTimeoutMinutes: 0, activeProfile: 'balanced' });
    backend.notifications.toggleDnd();
    deliver({ event: 'toggle' });
    deliver({ event: 'notifications', history: [row('one')], popups: [] });
    const expected = { ...backend.status(), historyCount: 1, popupCount: 0, dnd: true };
    assert.equal(expected.text, '󰂄 42%');
    assert.equal(expected.class, 'charging');
    const before = { frames: sent.length, settings: [...settings], native: [...native.notifications], history: backend.notifications.history,
        popups: backend.notifications.popups, resolutions: [...native.resolutions], battery: backend.battery };
    let changes = 0;
    backend.subscribe(() => changes++);
    for (let index = 0; index < 20; index++) assert.deepEqual(backend.status(), expected);
    assert.deepEqual({ frames: sent.length, settings: [...settings], native: [...native.notifications],
        history: backend.notifications.history, popups: backend.notifications.popups, resolutions: [...native.resolutions],
        battery: backend.battery }, before);
    assert.equal(changes, 0);
    backend.dispose();
});

test('application status serializes the native status object with panel visibility and monitor', () => {
    const panelDouble = (state) => state;
    const backend = new DashboardBackend(connect);
    const app = new DashboardApplication();
    app.backend = backend;
    app.panel = panelDouble({ opened: true, connector: 'DP-1' });
    assert.deepEqual(JSON.parse(app.handleRequest('{"command":"status"}')), { ...backend.status(), opened: true, monitor: 'DP-1' });
    app.panel = panelDouble({ opened: false, connector: '' });
    assert.deepEqual(JSON.parse(app.handleRequest('{"command":"status"}')), { ...backend.status(), opened: false, monitor: null });
    backend.dispose();
});

test('notification capabilities advertise only implemented plaintext UI features through GSettings', () => {
    settings.set('server-capabilites', ['action-icons', 'actions', 'body', 'body-hyperlinks', 'body-images', 'body-markup', 'icon-static', 'persistence', 'sound']);
    const center = new NotificationCenter();
    center.start(connect(() => {}));
    assert.deepEqual(settings.get('server-capabilites'), ['actions', 'body', 'icon-static', 'persistence']);
    center.dispose();
});

test('metrics and timeout render authoritative events only; hide, reopen and EOF invalidate stale work', () => {
    const backend = new DashboardBackend(connect);
    assert.ok(Object.values(backend.metrics).every((v) => v === -1));
    assert.equal(sent.length, 1);
    assert.equal(sent[0].command.kind, 'init');
    const values = { ...emptyMetrics(), cpuPercent: 50 };
    backend.setActive(true);
    backend.setActive(true);
    assert.deepEqual(sent.at(-1), { op: 'active', active: true, generation: 1 });
    deliver({ event: 'metrics', generation: 1, data: values });
    assert.deepEqual(backend.metrics, values);
    backend.setActive(false);
    backend.setActive(true);
    deliver({ event: 'metrics', generation: 1, data: values });
    assert.deepEqual(backend.metrics, emptyMetrics());
    let changes = 0;
    const unsubscribe = backend.subscribe(() => changes++);
    backend.setScreenTimeout(30);
    assert.equal(changes, 0);
    assert.equal(backend.screenTimeoutMinutes, 0);
    assert.deepEqual(sent.at(-1), { op: 'timeout', minutes: 30 });
    deliver({ event: 'timeout', minutes: 30 });
    assert.equal(changes, 1);
    assert.equal(backend.status().screenTimeoutMinutes, 30);
    const frames = sent.length;
    for (let i = 0; i < 20; i++) backend.status();
    assert.equal(sent.length, frames);
    assert.equal(changes, 1);
    backend.setActive(false);
    assert.equal(backend.screenTimeoutMinutes, 30);
    deliver({ event: 'offline' });
    assert.equal(backend.screenTimeoutMinutes, 0);
    unsubscribe();
    backend.dispose();
    backend.setScreenTimeout(60);
    assert.equal(sent.length, frames + 1);
});

test('native operations require both the selected token and the current Astal instance', () => {
    const backend = new DashboardBackend(connect);
    assert.equal(native.ignore_timeout, true);
    assert.equal(native.default_timeout, -1);
    native.send(new NativeNotification(1, 'old'));
    const old = sent.at(-1).command;
    native.send(new NativeNotification(1, 'new', { resident: true }));
    const current = sent.at(-1).command;
    assert.notEqual(old.token, current.token);
    assert.equal(current.row.summary, 'new');
    const op = (operation, token) => deliver({ event: 'native', native: 1, token, operation, argument: 'open' });
    for (const operation of ['expire', 'dismiss', 'invoke']) op(operation, old.token);
    assert.deepEqual(native.resolutions, []);
    assert.deepEqual(native.invoked, []);
    op('invoke', current.token);
    assert.deepEqual(native.invoked, [[1, 'open']]);
    op('dismiss', current.token);
    assert.deepEqual(native.resolutions, [[1, 2]]);
    assert.deepEqual(sent.at(-1).command, { kind: 'resolved', token: current.token, reason: 2 });
    native.notifications = [new NativeNotification(2, 'pending')];
    backend.notifications.clearNotifications();
    assert.equal(sent.at(-1).command.kind, 'clear');
    assert.equal(sent.at(-1).command.instances[0].native, 2);
    backend.dispose();
});

test('unexpected EOF is fatal once, drops pending restores, and ignores later helper events', async () => {
    let fatal = 0;
    native.notifications = [new NativeNotification(1, 'pending')];
    const backend = new DashboardBackend(connect, () => {
        fatal++;
    });
    deliver({ event: 'offline' });
    deliver({ event: 'offline' });
    deliver({ event: 'toggle' });
    native.send(new NativeNotification(2, 'after EOF'));
    await Promise.resolve();
    assert.equal(fatal, 1);
    assert.equal(native.dont_disturb, false);
    assert.deepEqual(sent.map((f) => f.command.kind), ['init']);
    backend.dispose();
    await Promise.resolve();
    assert.equal(fatal, 1);
});

test('planned disposal suppresses both transport close and a still-deferred fatal callback', async () => {
    for (const pending of [false, true]) {
        let fatal = 0;
        const backend = new DashboardBackend(connect, () => {
            fatal++;
        });
        if (pending) deliver({ event: 'offline' });
        backend.dispose();
        deliver({ event: 'offline' });
        await Promise.resolve();
        assert.equal(fatal, 0);
    }
});

for (const [name, value, executable] of [
    ['unset', undefined, 'dashboard-control'],
    ['empty', '', 'dashboard-control'],
    ['literal path with spaces', '/test/dashboard control/bin/dashboard-control', '/test/dashboard control/bin/dashboard-control'],
]) {
    test(`DashboardPipe routes ${name} DASHBOARD_CONTROL to one literal executable`, (t) => {
        environment.set('PANEL_BACKEND', '/test/legacy panel-launcher');
        if (value !== undefined) environment.set('DASHBOARD_CONTROL', value);
        let inputCloses = 0, outputCloses = 0;
        const spawn = t.mock.method(Gio.Subprocess, 'new', () => ({
            wait_async() {},
            get_stdin_pipe: () => ({
                close() {
                    inputCloses++;
                },
            }),
            get_stdout_pipe: () => ({
                read_bytes_async() {},
                close() {
                    outputCloses++;
                },
            }),
        }));
        const events = [];
        const pipe = new DashboardPipe((event) => events.push(event));
        assert.deepEqual(spawn.mock.calls[0].arguments, [[executable], Gio.SubprocessFlags.STDIN_PIPE | Gio.SubprocessFlags.STDOUT_PIPE]);
        assert.deepEqual(events, []);
        pipe.dispose();
        pipe.dispose();
        assert.deepEqual(events, [{ event: 'offline' }]);
        assert.deepEqual([inputCloses, outputCloses, spawn.mock.callCount()], [1, 1, 1]);
    });
}

test('synchronous init pipe failure quits the assigned notification owner and main returns failure', async (t) => {
    let attempts = 0, quits = 0, writes = 0;
    const input = {
        close() {},
        write_all_async() {
            writes++;
            throw new Error('closed');
        },
    };
    t.mock.method(Gio.Subprocess, 'new', (argv, flags) => {
        assert.deepEqual(argv, ['dashboard-control']);
        assert.equal(flags, Gio.SubprocessFlags.STDIN_PIPE | Gio.SubprocessFlags.STDOUT_PIPE);
        attempts++;
        return {
            wait_async() {},
            get_stdin_pipe: () => input,
            get_stdout_pipe: () => ({
                read_bytes_async() {},
                close() {},
            }),
        };
    });
    const quit = mockGI.Astal.Application.prototype.quit;
    t.mock.method(mockGI.Astal.Application.prototype, 'quit', function () {
        quits++;
        assert.equal(this.backend?.offline, true, 'quit must run after assignment');
        quit.call(this);
        assert.equal(this.backend, undefined, 'shutdown must dispose the owner');
    });
    t.mock.method(console, 'error', () => {});
    native.notifications = [new NativeNotification(1, 'pending')];
    assert.equal(await main(), 1);
    native.send(new NativeNotification(2, 'after shutdown'));
    assert.deepEqual([attempts, quits, writes], [1, 1, 1]);
});

test('offline during connect or init cannot start native restoration or keep signal work alive', () => {
    for (const phase of ['connect', 'init']) {
        const frames = [];
        native.notifications = [new NativeNotification(1, 'old')];
        const backend = new DashboardBackend((receive) => {
            if (phase === 'connect') receive({ event: 'offline' });
            return {
                send(frame) {
                    frames.push(frame);
                    receive({ event: 'offline' });
                },
                whenDrained() {
                    assert.fail('Scheduled restore after EOF');
                },
                dispose() {},
            };
        });
        native.send(new NativeNotification(2, 'new'));
        assert.deepEqual(frames.map((f) => f.command.kind), phase === 'init' ? ['init'] : []);
        assert.deepEqual(backend.notifications.history, []);
        backend.dispose();
    }
});

test('stalled pipe restores over 512 large native snapshots without starving controls or overflowing either limit', (t) => {
    const count = 600, large = '\u0001'.repeat(15000);
    let snapshots = 0, total = 0, largest = 0, closes = 0;
    native.notifications = Array.from({ length: count }, (_, i) => {
        const obj = new NativeNotification(i, large, { app_name: large, image: large, actions: Array.from({ length: 16 }, () => ({ id: large.slice(0, 128), label: large })) });
        Object.defineProperty(obj, 'body', {
            get() {
                snapshots++;
                return large;
            },
        });
        return obj;
    });
    let complete;
    const frames = [];
    const input = {
        write_all_async(bytes, _priority, _io, cb) {
            assert.equal(complete, undefined, 'only one outstanding write');
            const f = JSON.parse(new TextDecoder().decode(bytes));
            frames.push({ name: f.command?.kind ?? f.op, id: f.command?.native, token: f.command?.token, restore: f.command?.restore });
            largest = Math.max(largest, bytes.length);
            total += bytes.length;
            complete = () => {
                complete = undefined;
                cb(input, bytes.length);
            };
        },
        write_all_finish(n) {
            return [true, n];
        },
        close() {
            closes++;
        },
    };
    t.mock.method(Gio.Subprocess, 'new', (argv, flags) => {
        assert.deepEqual(argv, ['dashboard-control']);
        assert.equal(flags, Gio.SubprocessFlags.STDIN_PIPE | Gio.SubprocessFlags.STDOUT_PIPE);
        return {
            wait_async() {},
            get_stdin_pipe: () => input,
            get_stdout_pipe: () => ({
                read_bytes_async() {},
                close() {},
            }),
        };
    });
    const warn = t.mock.method(console, 'warn', () => {});
    let transport;
    const backend = new DashboardBackend((cb) => transport = new DashboardPipe(cb));
    assert.equal(snapshots, 0, 'stalled init must not marshal any bodies');
    complete();
    assert.equal(snapshots, 1);
    transport.send({ op: 'notify', command: { kind: 'action', id: '', token: '', identifier: 'x'.repeat(131072) } });
    assert.equal(warn.mock.callCount(), 1);
    backend.setScreenTimeout(10);
    complete();
    assert.equal(frames.at(-1).name, 'timeout');
    assert.equal(snapshots, 1);
    // Signals overtaking bulk restore still preserve instance history and order.
    native.send(new NativeNotification(599, 'replacement'));
    native.resolve(598, 3);
    while (complete) complete();
    const restored = frames.filter((f) => f.restore);
    assert.deepEqual(restored.map((f) => f.id).sort((a, b) => a - b), Array.from({ length: count }, (_, i) => i));
    const replaced = frames.filter((f) => f.id === 599);
    assert.deepEqual(replaced.map((f) => f.restore), [true, false]);
    assert.notEqual(replaced[0].token, replaced[1].token);
    assert.equal(frames.find((f) => f.name === 'resolved').token, restored.find((f) => f.id === 598).token);
    assert.equal(snapshots, count);
    assert.ok(total > 8 * 1024 * 1024);
    assert.ok(largest <= 131072);
    assert.equal(closes, 0);
    assert.deepEqual(backend.notifications.popups, []);
    backend.dispose();
    assert.equal(closes, 1);
});

test('reattach snapshots do not synthesize notified signals; seed cannot override a newly explicit native preference', async () => {
    native.notifications = [new NativeNotification(1, 'restored')];
    const backend = new DashboardBackend(connect);
    await Promise.resolve();
    assert.equal(sent[0].command.user, false);
    assert.equal(sent[1].command.restore, true);
    assert.equal(backend.status().popupCount, 0);
    settings.set('dont-disturb', false);
    deliver({ event: 'seed', value: true });
    assert.equal(native.dont_disturb, false);
    assert.deepEqual(sent.at(-1).command, { kind: 'seedAck', value: false });
    backend.dispose();
});
