// One cross-runtime entry point; imports and fixtures stay scoped to their mode.
// Node (default): node --test scripts/audio-control/astal/test.js
// Node isolated GTK: node scripts/audio-control/astal/test.js --tooltips /absolute/path/to/panel-gjs
// GJS: panel-gjs -m scripts/audio-control/astal/test.js pipes|native|dropdown|tooltips
// pipes is service-free; native reads live state; dropdown/tooltips require their original isolated environments.

async function nodeTests() {
const { default: test } = await import('node:test');
const { default: assert } = await import('node:assert/strict');
const { existsSync, readFileSync, chmodSync, mkdirSync, mkdtempSync, rmSync, statSync, writeFileSync } = await import('node:fs');
const { spawnSync } = await import('node:child_process');
const { setImmediate: settle } = await import('node:timers/promises');
const { tmpdir } = await import('node:os');
const { join } = await import('node:path');
const { default: vm } = await import('node:vm');
const logic = await import('./logic.js');
const {
    parseRequest, nativeEvent, rawEqual, waybarStatus,
} = await import('./logic.js');


// Tests consolidated from architecture.test.js.
{
const root = new URL('../../../', import.meta.url);
const read = path => readFileSync(new URL(path, root), 'utf8');


test('audio application uses the public native request virtual and socket writer', () => {
    const source = read('scripts/audio-control/astal/frontend.js');
    assert.match(source, /extends Astal\.Application/);
    assert.match(source, /vfunc_request\(request, connection\)/);
    assert.match(source, /AstalIO\.write_sock\(connection/);
    assert.match(source, /AstalIO\.write_sock_finish\(result\)/);
    assert.match(source, /connection\.close_async/);
    assert.doesNotMatch(source, /\.prototype\.|Object\.defineProperty|gi:\/\/Gio.*SocketService/);
});

test('audio lists highlight user interaction without a persistent connected Bluetooth highlight', () => {
    const css = read('scripts/audio-control/astal/style.css');
    assert.doesNotMatch(css, /dropdown\.device-dropdown|popover|button\.device-dropdown:(?:hover|focus-visible)/);
    const trigger = css.match(/\.audio-panel button\.device-dropdown \{([^}]+)\}/)?.[1];
    assert.ok(trigger, 'the inline toggle has its own style, not control-button highlights');
    assert.match(trigger, /background: transparent;/);
    assert.match(trigger, /background-image: none;/);
    assert.match(trigger, /border: 0\.35px solid color-mix\(in srgb, var\(--ap_foreground\) 50%, transparent\);/);
    assert.doesNotMatch(trigger, /--ap_yellow|lighter\(/);
    const options = css.match(/\.audio-panel (?:scrolledwindow)?\.device-options \{([^}]+)\}/)?.[1];
    assert.ok(options, 'inline options have a dedicated container style');
    assert.match(options, /background: transparent;/);
    assert.match(options, /border: 0\.35px solid color-mix\(in srgb, var\(--ap_foreground\) 50%, transparent\);/);
    const rows = css.slice(css.indexOf('.audio-panel listview.device-list row {'), css.indexOf('.audio-panel button.icon-button {'));
    assert.match(rows, /padding: 5px 7px;\s*border: 1px solid transparent;/);
    for (const state of ['selected', 'hover', 'focus-visible', 'active'])
        assert.ok(rows.includes(`row:${state}`));
    assert.match(rows, /border-color: color-mix\(in srgb, var\(--ap_yellow\) 45%, transparent\);/);
    assert.match(rows, /row:selected \{\s*border-color: color-mix\(in srgb, var\(--ap_red\) 55%, transparent\);/);
    assert.ok(rows.indexOf('row:selected {') > rows.indexOf('row:hover,'), 'selected outline wins over hover');
    assert.match(rows, /background: color-mix\(in srgb, var\(--ap_red\) 10%, transparent\);/);
    const bluetooth = css.slice(css.indexOf('.audio-panel .bluetooth-device {'), css.indexOf('.audio-panel .pairing-card {'));
    assert.match(bluetooth, /border: 1px solid transparent;/);
    assert.match(bluetooth, /border-color: color-mix\(in srgb, var\(--ap_yellow\) 45%, transparent\);/);
    assert.doesNotMatch(bluetooth, /\.bluetooth-device\.connected/);
    assert.match(bluetooth, /\.bluetooth-device:hover,\s*\.audio-panel \.bluetooth-device:focus-within/);
    assert.match(bluetooth, /background: color-mix\(in srgb, var\(--ap_red\) 10%, transparent\);/);
});

test('audio content uses compact outer padding and page insets', () => {
    const source = read('scripts/audio-control/astal/frontend.js');
    const css = read('scripts/audio-control/astal/style.css');
    assert.match(css, /\.audio-content \{ padding: 6px; \}/);
    assert.match(source, /margin_top: 6, margin_bottom: 6, margin_start: 6, margin_end: 6/);
});

test('audio tabs use the network panel’s flat cyan/red underline styling', () => {
    const source = read('scripts/audio-control/astal/frontend.js');
    const css = read('scripts/audio-control/astal/style.css');
    const tabs = css.slice(css.indexOf('.audio-panel button.tab {'), css.indexOf('.audio-panel .volume-bar {'));
    assert.match(tabs, /border: none;\s*border-bottom: 2px solid var\(--ap_cyan\);/);
    assert.match(tabs, /border-radius: 0;/);
    assert.match(tabs, /min-height: 30px;/);
    assert.match(tabs, /font-size: 14px; letter-spacing: 0\.5px;/);
    assert.match(tabs, /button\.tab\.selected:focus-visible \{\s*border-color: var\(--ap_red\);\s*background: transparent;\s*color: var\(--ap_red\);/);
    assert.match(tabs, /button\.tab\.selected:active \{ background: transparent; \}/);
    assert.doesNotMatch(tabs, /background: (?!transparent;)/);
    assert.doesNotMatch(tabs, /--ap_pink|border-width:/);
    assert.match(source, /const tabs = box\(false, 4\);/);
        assert.match(source, /tab\.height_request = 32;/);
    assert.doesNotMatch(source, /pageBorder|pageFrame/);
});

test('audio theme generates scoped CSS variables for every stylesheet color', () => {
    const source = read('scripts/audio-control/astal/frontend.js');
    const css = read('scripts/audio-control/astal/style.css');
    const palette = Object.fromEntries(['yellow', 'cyan', 'pink', 'red', 'dimPink', 'separator',
        'foreground', 'background', 'secondaryBackground', 'chromeBackground', 'extraColor']
        .map((key, index) => [key, `#${(index + 1).toString(16).padStart(6, '0')}`]));
    const input = { ...palette, 'bad-key': '#123456', notColor: 'transparent' };
    const calls = [];
    const theme = source.match(/function loadTheme\([\s\S]*?\n\}/)?.[0];
    assert.ok(theme);
    const loadTheme = vm.runInNewContext(`${theme}\nloadTheme;`, {
        read: () => JSON.stringify(input),
        loadPanelStyle: options => calls.push(options),
    });
    const loaded = loadTheme('fixture/frontend.js', 'fixture/palette.json');
    assert.equal(JSON.stringify(loaded), JSON.stringify(input));
    assert.equal(calls.length, 1);
    assert.equal(calls[0].entry, 'fixture/frontend.js');
    assert.equal(calls[0].label, 'Audio');
    assert.equal(calls[0].definitions, '.audio-panel {\n' + Object.entries(palette)
        .map(([key, value]) => `    --ap_${key}: ${value};`).join('\n') + '\n}');
    assert.doesNotMatch(source, /@define-color/);
    assert.doesNotMatch(css, /@ap_|\balpha\(|#[a-f\d]{3,8}\b/i);
    const references = [...css.matchAll(/var\(--ap_([a-zA-Z]+)\)/g)];
    assert.ok(references.length > 0);
    for (const [, key] of references) assert.ok(Object.hasOwn(palette, key), key);
    input.cyan = 'transparent';
    assert.throws(() => loadTheme('fixture/frontend.js', 'fixture/palette.json'), /Invalid audio palette colour: cyan/);
    assert.equal(calls.length, 1, 'invalid palettes never reach the stylesheet loader');
});

test('Rust has no second native audio mutation backend or post-pair Bluetooth controls', () => {
    const manifest = read('scripts/audio-control/rust-custom/Cargo.toml');
    assert.doesNotMatch(manifest, /pulsectl|libpulse/);
    for (const name of ['audio.rs', 'devices.rs', 'policy.rs']) {
        const source = read(`scripts/audio-control/rust-custom/src/${name}`);
        assert.doesNotMatch(source, /set_card_profile|set_sink_port|set_source_port|set_default_device|SinkController|SourceController/);
        assert.doesNotMatch(source, /"(?:pactl|wpctl|pw-cli|bluetoothctl)"/);
    }
    const pairing = read('scripts/audio-control/rust-custom/src/bluetooth.rs').split('pub mod battery_provider')[0];
    assert.match(pairing, /register_agent\(pairing_agent\(\)\)/);
    assert.match(pairing, /device\.pair\(\)/);
    assert.doesNotMatch(pairing, /\.set_powered\(|\.discover_devices\(|\.set_trusted\(|device\.connect\(|default_adapter\(|device_addresses\(/);
});

test('retired panel implementations and Rust status sources remain absent', () => {
    for (const path of ['niri/audio-control', 'scripts/audio-control/rust-custom/src/waybar.rs',
        'niri/quickshell', 'themes/quickshell'])
        assert.equal(existsSync(new URL(path, root)), false, path);
    assert.doesNotMatch(read('niri/default.nix'), /\.\/(?:audio-control|quickshell)/);
});
}

// Raw native adapter tests; domain regressions are ported to Rust tests.rs.
{
const NO = 1, YES = 2;
const plain = value => structuredClone(value); // Normalize objects from the VM realm.
const GObject = {
    TYPE_STRING: Symbol('GObject.TYPE_STRING'),
    Value: class {
        init(type) { assert.equal(type, GObject.TYPE_STRING); this.type = type; }
        set_string(value) { assert.equal(this.type, GObject.TYPE_STRING); this.value = value; }
        get_string() { assert.equal(this.type, GObject.TYPE_STRING); return this.value; }
    },
};

class Signals {
    constructor(properties = {}) {
        Object.assign(this, properties);
        this.handlers = new Map();
        this.nextSignal = 1;
        this.connections = 0;
        this.disconnections = 0;
    }

    connect(name, callback) {
        const id = this.nextSignal++;
        this.handlers.set(id, { name, callback });
        this.connections++;
        return id;
    }

    disconnect(id) {
        assert.ok(this.handlers.delete(id), 'disconnect must use a live signal id');
        this.disconnections++;
    }

    emit(name, ...args) {
        for (const handler of [...this.handlers.values()])
            if (handler.name === name) handler.callback(this, ...args);
    }

    notify(name) { this.emit('notify', { name }); }
}

function clock() {
    let now = 0, serial = 0;
    const sources = new Map();
    const add = (interval, callback) => {
        const id = ++serial;
        sources.set(id, { interval, callback, due: now + interval });
        return id;
    };
    return {
        sources,
        GLib: {
            PRIORITY_DEFAULT: 0, SOURCE_REMOVE: false, SOURCE_CONTINUE: true,
            getenv: name => name === 'AUDIO_CONTROL_HELPER' ? '/test/audio-control' : null,
            idle_add: (_, callback) => add(0, callback),
            timeout_add: (_, interval, callback) => add(interval, callback),
            source_remove: id => sources.delete(id),
        },
        advance(milliseconds = 0) {
            const target = now + milliseconds;
            let iterations = 0;
            for (;;) {
                const next = [...sources].filter(([, task]) => task.due <= target)
                    .sort((a, b) => a[1].due - b[1].due || a[0] - b[0])[0];
                if (!next) break;
                assert.ok(++iterations < 1000, 'timer loop must remain bounded');
                const [id, task] = next;
                now = task.due;
                const repeat = task.callback();
                if (sources.get(id) !== task) continue;
                if (repeat) task.due = now + task.interval;
                else sources.delete(id);
            }
            now = target;
        },
    };
}

function endpoint(properties = {}) {
    const node = new Signals({
        id: 10, serial: String((properties.id ?? 10) * 10), pwName: 'alsa_output.test', name: null, description: 'Speakers', nick: 'Desk',
        card: null, profileDevice: '0', volume: 0.4, mute: false, routes: [], route: null, acceptRoute: true,
        acceptDefault: true, routeWrites: [], defaultWrites: [], defaultValue: false,
        ...properties,
    });
    // Astal's public name is media.name, not the PipeWire endpoint identity.
    node.get_pw_property = name => ({
        'object.serial': node.serial, 'node.nick': node.nick, 'node.name': node.pwName,
        'card.profile.device': node.profileDevice,
    })[name] ?? null;
    node.get_device = () => node.card;
    node.set_route = route => {
        assert.ok(node.routes.includes(route), 'set_route receives the actual native route object');
        node.routeWrites.push(route);
        if (node.acceptRoute) { node.route = route; node.notify('route'); }
    };
    Object.defineProperty(node, 'is_default', {
        get: () => node.defaultValue,
        set: value => {
            if (node.defaultError) throw node.defaultError;
            node.defaultWrites.push(value);
            if (node.acceptDefault) {
                node.defaultValue = value;
                node.onDefault?.(value);
                node.notify('is-default');
            }
        },
    });
    return node;
}

function playbackStream(properties = {}) {
    const stream = new Signals({ id: 40, serial: '400', name: 'Chrome', description: 'Chrome playback',
        target_serial: 100, acceptTarget: true, targetWrites: [], ...properties });
    stream.get_pw_property = name => ({ 'object.serial': stream.serial, 'application.name': stream.name })[name] ?? null;
    stream.set_target_endpoint = (...args) => {
        assert.equal(args.length, 1, 'set_target_endpoint(endpoint)');
        assert.equal(args[0], null, 'Astal null clears the saved app target instead of pinning another endpoint');
        stream.targetWrites.push(args[0]);
        if (stream.targetError) throw stream.targetError;
        if (stream.acceptTarget) {
            stream.target_serial = -1;
            stream.notify('target-serial');
        }
        stream.onTarget?.();
    };
    return stream;
}

function card(properties = {}) {
    const object = new Signals({ id: 7, serial: '70', pwName: 'alsa_card.test',
        active_profile_id: 0, profiles: [{ index: 0 }, { index: 1 }],
        acceptProfile: true, profileWrites: [], ...properties });
    object.get_pw_property = name => ({ 'object.serial': object.serial, 'device.name': object.pwName })[name] ?? null;
    object.set_active_profile_id = (...args) => {
        assert.equal(args.length, 1, 'set_active_profile_id(index)');
        assert.ok(object.profiles.some(profile => profile.index === args[0]));
        object.profileWrites.push(args[0]);
        if (object.acceptProfile) {
            object.active_profile_id = args[0];
            object.notify('active-profile-id');
        }
    };
    return object;
}

function adapter({ path = '/org/bluez/hci0', ...properties } = {}) {
    // The pinned GIR omits object_path; only GObject's generic getter exposes it.
    const object = new Signals({ powered: true, discovering: false,
        scanCalls: [], removed: [], propertyReads: [], ...properties });
    object.get_property = (name, value) => {
        assert.equal(name, 'object-path');
        assert.ok(value instanceof GObject.Value);
        object.propertyReads.push(name);
        value.set_string(path);
    };
    object.start_discovery = () => { object.scanCalls.push('start'); object.discovering = true; };
    object.stop_discovery = () => { object.scanCalls.push('stop'); object.discovering = false; };
    object.remove_device = device => object.removed.push(device);
    return object;
}

function device(properties = {}) {
    const object = new Signals({ address: 'AA:BB:CC:DD:EE:01', adapter: '/org/bluez/hci0', name: 'Headset',
        alias: '', connected: false, paired: false, battery_percentage: -1,
        operations: [], finishes: [], cancellations: 0, trusted: false, ...properties });
    for (const method of ['connect_device', 'disconnect_device']) {
        // AstalBluetooth takes just an async callback, not a Gio.Cancellable.
        object[method] = (...args) => {
            assert.equal(args.length, 1, `${method}(callback)`);
            assert.equal(typeof args[0], 'function');
            object.operations.push({ method, callback: args[0] });
        };
        object[`${method}_finish`] = result => {
            object.finishes.push({ method, result });
            if (result.error) throw result.error;
            object.connected = method === 'connect_device';
            object.notify('connected');
        };
    }
    object.cancel_pairing = () => { object.cancellations++; };
    return object;
}


const source = readFileSync(new URL('./backend.js', import.meta.url), 'utf8')
    .replace(/^import .*;\r?\n/gm, '')
    .replace(/export class HelperProcess[\s\S]*?(?=const emptyAudio)/, '')
    .replace(/^export /gm, '');
function fixture(t, { outputs = [endpoint()], inputs = [], cards = [], streams = [], recorders = [], radio = adapter(), devices = [] } = {}) {
    const timers = clock(), helpers = [];
    class HelperProcess {
        constructor(argv, options) { Object.assign(this, { argv, options, answers: [], done: false }); helpers.push(this); }
        send(text) { assert.ok(text.endsWith('\n')); this.answers.push(JSON.parse(text)); }
        event(value) { try { this.options.onChunk(`${JSON.stringify(value)}\n`); } catch (error) { this.finish(error); } }
        finish(error = null) { if (this.done) return; this.done = true; this.error = error; this.options.onExit(error, ''); }
        stop() { this.finish(new Error('cancelled')); }
    }
    const audio = new Signals({ devices: cards, speakers: outputs, microphones: inputs, streams, recorders,
        default_speaker: new Signals({ id: outputs[0]?.id || 0 }), default_microphone: new Signals({ id: inputs[0]?.id || 0 }) });
    const bindNode = (node, kind = 'output') => {
        node.onDefault = value => { if (!value) return; audio[kind === 'output' ? 'default_speaker' : 'default_microphone'].id = node.id; audio.notify('default'); };
        return node;
    };
    outputs.forEach(node => bindNode(node)); inputs.forEach(node => bindNode(node, 'input'));
    const wp = new Signals({ audio }), bluetooth = new Signals({ adapter: radio, devices });
    let changes = 0;
    const AudioBackend = vm.runInNewContext(`${source}\nAudioBackend;`, { ...logic, GLib: timers.GLib, Gio: {}, GObject, HelperProcess,
        Wp: { Available: { NO, YES }, get_default: () => wp }, Bluetooth: { get_default: () => bluetooth }, TextEncoder });
    const backend = new AudioBackend({ onChange: () => changes++ });
    backend.start(); timers.advance();
    t.after(() => { backend.finish(); backend.worker?.stop(); });
    const worker = helpers[0];
    const snapshot = () => plain(backend.capture());
    const raw = object => snapshot().objects.find(value => backend.records.get(value.key)?.object === object);
    const action = (operation, object, args = {}, guards = [], extra = {}) => ({ type: 'action', id: 1, operation,
        target: raw(object).key, args, guards, cleanup: false, ...extra });
    return { backend, worker, audio, wp, bluetooth, outputs, inputs, cards, streams, recorders, radio, devices, helpers, timers, snapshot, raw, action, bindNode, changes: () => changes };
}

test('native snapshots are raw, canonical Astal properties; status and visibility never select workflows in JS', t => {
    const nativeCard = card(), route = { index: 0, name: 'headphones', description: 'Headphones', available: YES };
    const output = endpoint({ card: nativeCard, routes: [route], route, name: 'Not the PipeWire name' });
    const foreign = device({ adapter: '/org/bluez/hci99' }), local = device();
    const f = fixture(t, { outputs: [output], devices: [local, foreign], streams: [playbackStream()], recorders: [playbackStream({ id: 50 })] });
    const raw = f.snapshot();
    assert.equal(raw.objects.filter(object => object.type === 'device').length, 2, 'Rust filters the native adapter, not JS');
    assert.equal(raw.objects.filter(object => object.type === 'stream').length, 1, 'recording streams are never playback action targets');
    assert.equal(f.raw(output).name, output.pwName); assert.equal(f.raw(output).routes[0].index, 0);
    assert.equal(f.raw(nativeCard).activeProfile, 0); assert.deepEqual(f.raw(nativeCard).profiles, [{ index: 0 }, { index: 1 }]);
    assert.equal(f.raw(local).battery, -1, 'battery interpretation is Rust policy');
    assert.ok(!('routesReliable' in f.raw(output)));
    for (const [opened, tab] of [[true, 0], [true, 1], [false, 0]]) { f.backend.setActive(opened, tab); f.timers.advance(); }
    f.backend.status(); f.backend.snapshot();
    assert.deepEqual(f.radio.scanCalls, []); assert.deepEqual(output.defaultWrites, []);
    assert.deepEqual(output.routeWrites, []); assert.deepEqual(local.operations, []);
    assert.deepEqual([...f.worker.argv], ['/test/audio-control', 'panel']);
    assert.deepEqual(Object.keys(f.worker.options).sort(), ['onChunk', 'onExit']);
    assert.ok(f.radio.propertyReads.length, 'adapter path uses the generic GObject getter');
});

test('raw lifetime keys and default epochs invalidate replacement, removal and A -> B -> A controls', t => {
    const a = endpoint(), b = endpoint({ id: 11 }); const f = fixture(t, { outputs: [a, b] });
    const first = f.snapshot();
    f.audio.default_speaker = new Signals({ id: a.id }); f.audio.notify('default');
    assert.equal(f.snapshot().outputEpoch, first.outputEpoch, 'new proxy is not a new default');
    f.audio.default_speaker.id = b.id; f.audio.default_speaker.notify('id');
    f.audio.default_speaker.id = a.id; f.audio.default_speaker.notify('id');
    assert.notEqual(f.snapshot().outputEpoch, first.outputEpoch);
    const oldKey = f.raw(a).key;
    const replacement = endpoint({ id: a.id, serial: a.serial });
    f.audio.speakers = [replacement, b]; f.audio.notify('speakers');
    assert.notEqual(f.raw(replacement).key, oldKey);
    const key = f.raw(replacement).key;
    f.audio.speakers = [b]; f.audio.notify('speakers');
    f.audio.speakers = [replacement, b]; f.audio.notify('speakers');
    assert.notEqual(f.raw(replacement).key, key);
});

test('generic executor calls the actual native APIs, preserving route index zero and null playback targets', async t => {
    const nativeCard = card(), route = { index: 0, name: 'port', available: YES };
    const output = endpoint({ card: nativeCard, routes: [route] }), stream = playbackStream(), headset = device();
    const f = fixture(t, { outputs: [output], streams: [stream], devices: [headset] });
    for (const [operation, object, args] of [['profile', nativeCard, { profile: 1 }], ['route', output, { route: 0 }],
        ['default', output, {}], ['volume', output, { volume: 1.5 }], ['mute', output, { muted: true }],
        ['follow', stream, {}], ['trust', headset, { trusted: true }], ['startDiscovery', f.radio, {}],
        ['stopDiscovery', f.radio, {}], ['power', f.radio, { powered: false }],
        ['forget', headset, { adapter: f.raw(f.radio).key }], ['cancelPairing', headset, {}]])
        await f.backend.execute(f.action(operation, object, args));
    assert.deepEqual(nativeCard.profileWrites, [1]); assert.deepEqual(output.routeWrites, [route]);
    assert.deepEqual(output.defaultWrites, [true]); assert.equal(output.volume, 1.5); assert.equal(output.mute, true);
    assert.deepEqual(stream.targetWrites, [null]); assert.equal(headset.trusted, true); assert.equal(headset.cancellations, 1);
    assert.deepEqual(f.radio.scanCalls, ['start', 'stop']); assert.deepEqual(f.radio.removed, [headset]);
});

test('opaque guards reject stale raw arrays, defaults and identities without semantic action authorization', async t => {
    const route = { index: 0, name: 'port', available: YES }, output = endpoint({ routes: [route] });
    const f = fixture(t, { outputs: [output] }), raw = f.raw(output);
    const guards = [{ key: raw.key, expect: { routes: raw.routes, id: raw.id, serial: raw.serial } },
        { key: null, expect: { outputEpoch: f.snapshot().outputEpoch } }];
    route.available = NO;
    await assert.rejects(f.backend.execute(f.action('route', output, { route: 0 }, guards)), /changed/);
    assert.deepEqual(output.routeWrites, []);
    route.available = YES;
    output.serial = '999';
    await assert.rejects(f.backend.execute({ ...f.action('default', output), target: raw.key, guards }), /changed/);
    assert.deepEqual(output.defaultWrites, []);
    assert.ok(rawEqual({ b: [{ x: 1, y: null }], a: true }, { a: true, b: [{ y: null, x: 1 }] }));
    assert.equal(rawEqual([1, 2], [2, 1]), false); assert.equal(rawEqual({ x: 1 }, { x: '1' }), false);
});

test('retained raw handles permit Rust-selected cleanup after adapter replacement, then release the old object', async t => {
    const f = fixture(t), old = f.radio, raw = f.raw(old);
    await f.backend.execute(f.action('retain', old));
    f.bluetooth.adapter = adapter({ path: '/org/bluez/hci12' }); f.bluetooth.notify('adapter');
    await f.backend.execute({ type: 'action', id: 1, operation: 'stopDiscovery', target: raw.key, args: {},
        guards: [{ key: raw.key, held: true, expect: { type: 'adapter' } }], cleanup: true });
    assert.deepEqual(old.scanCalls, ['stop']);
    await f.backend.execute({ type: 'action', id: 2, operation: 'release', target: raw.key, args: {}, guards: [], cleanup: true });
    f.snapshot(); assert.equal(f.backend.records.has(raw.key), false);
});

for (const method of ['connect_device', 'disconnect_device']) for (const failure of [false, true]) {
    test(`native ${method} consumes its ${failure ? 'failed' : 'successful'} callback after disposal`, async t => {
        const headset = device(), f = fixture(t, { devices: [headset] });
        const pending = f.backend.execute(f.action(method === 'connect_device' ? 'connect' : 'disconnect', headset));
        const checked = failure ? assert.rejects(pending, /native failure/) : pending;
        assert.equal(headset.operations.length, 1);
        f.backend.finish();
        headset.operations[0].callback(headset, failure ? { error: new Error('native failure') } : {});
        await checked; assert.equal(headset.finishes.length, 1);
    });
}

test('resident IPC forwards fresh snapshots and native errors with monotonic RPC ids; models are cached read-only', async t => {
    const output = endpoint(), f = fixture(t, { outputs: [output] });
    const model = plain(f.backend.snapshot()); model.output.name = 'Rust-selected label';
    f.worker.event({ type: 'model', model }); assert.equal(f.backend.snapshot().output.name, model.output.name);
    const before = f.worker.answers.length; f.backend.status(); f.backend.snapshot(); assert.equal(f.worker.answers.length, before);
    f.worker.event({ type: 'snapshot', id: 1 }); await settle();
    assert.equal(f.worker.answers.at(-1).id, 1); assert.equal(f.worker.answers.at(-1).ok, true);
    output.defaultError = new Error('native setter failed');
    f.worker.event(f.action('default', output, {}, [], { id: 2 })); await settle();
    const reply = f.worker.answers.at(-1); assert.equal(reply.id, 2); assert.equal(reply.ok, false); assert.equal(reply.error, 'native setter failed');
    f.worker.event({ type: 'snapshot', id: 4 }); assert.match(f.worker.error.message, /RPC id/);
    assert.equal(f.helpers.length, 1, 'a failed coordinator is never restarted or replaced by a backend fallback');
});

test('UI methods send only intents and cancellation epochs; no JS workflow is started', t => {
    const headset = device(), f = fixture(t, { devices: [headset] });
    f.backend.select('output', 'opaque'); f.backend.setVolume('output', 0.42, 'epoch'); f.backend.toggleMute('input', 'epoch');
    f.backend.deviceAction(f.raw(headset).key); f.backend.forget(f.raw(headset).key); f.backend.toggleBluetoothPower();
    f.backend.sendPairingAnswer('000007'); f.backend.dismissPairingMessage(); f.backend.cancelPairing();
    const intents = f.worker.answers.filter(value => value.type === 'intent');
    assert.deepEqual(intents.map(value => value.command), ['select', 'volume', 'mute', 'device', 'forget', 'bluetoothPower', 'answer', 'dismissPairing', 'cancelPairing']);
    assert.equal(intents.find(value => value.command === 'answer').answer, '000007');
    assert.equal(f.snapshot().pairingEpoch, 1); assert.deepEqual(headset.operations, []); assert.deepEqual(f.radio.scanCalls, []);
});

test('shutdown blocks ordinary writes, lets Rust request cleanup, and disconnects every subscription exactly once', async t => {
    const output = endpoint(), f = fixture(t, { outputs: [output] });
    const action = f.action('default', output), cleanup = f.action('stopDiscovery', f.radio, {}, [], { cleanup: true });
    f.backend.dispose(); assert.equal(f.worker.answers.at(-1).type, 'shutdown');
    await assert.rejects(f.backend.execute(action), /cancelled/); await f.backend.execute(cleanup);
    f.worker.event({ type: 'closed' }); f.backend.dispose();
    for (const object of [f.audio, f.wp, f.bluetooth, f.radio, output, f.audio.default_speaker, f.audio.default_microphone])
        assert.equal(object.handlers.size, 0);
    assert.equal(f.timers.sources.size, 0); assert.throws(() => f.backend.select('output', 'x'), /stopped/);
});

test('abrupt IPC death executes only opaque Rust-registered teardown plans, without a backend fallback', async t => {
    const headset = device(), f = fixture(t, { devices: [headset] });
    const stop = f.action('stopDiscovery', f.radio, {}, [], { cleanup: true });
    const cancel = f.action('cancelPairing', headset, {}, [], { cleanup: true });
    await f.backend.execute(f.action('retain', f.radio, { onDrop: [stop] }));
    await f.backend.execute(f.action('retain', headset, { onDrop: [cancel] }));
    const original = f.radio.stop_discovery;
    f.radio.stop_discovery = () => { throw new Error('stop failed'); };
    f.bluetooth.adapter = adapter({ path: '/org/bluez/hci12' }); f.bluetooth.notify('adapter');
    await assert.rejects(f.backend.execute(stop), /stop failed/);
    f.radio.stop_discovery = original;
    assert.deepEqual(f.bluetooth.adapter.scanCalls, []);
    f.worker.finish(new Error('policy died'));
    await settle();
    assert.deepEqual(f.radio.scanCalls, ['stop']); assert.equal(headset.cancellations, 1);
    assert.equal(f.backend.disposed, true); assert.equal(f.backend.records.size, 0); assert.equal(f.helpers.length, 1);
    assert.deepEqual(f.bluetooth.adapter.scanCalls, []);
    assert.deepEqual(headset.operations, []); assert.equal(headset.trusted, false);
});

function helperFixture(data, { written = 4096, exited = true, successful = true } = {}) {
    const completeSource = readFileSync(new URL('./backend.js', import.meta.url), 'utf8').replace(/^import .*;\r?\n/gm, '').replace(/^export /gm, '');
    const timers = clock(), chunks = [], exits = [], writes = [];
    const status = { waits: 0, reaped: 0, killed: 0, cancelled: 0 };
    const stream = () => ({ closed: false, pending: false,
        is_closed() { return this.closed; }, has_pending() { return this.pending; },
        close_async(_, cancellable, callback) { assert.equal(cancellable, null); this.closed = true; callback(this, {}); }, close_finish() {} });
    let cursor = 0, waitCallback, exitScheduled = false;
    const stdout = { ...stream(), read_bytes_async(size, _, __, callback) {
        this.pending = true;
        const bytes = data.slice(cursor, cursor + size); cursor += bytes.length;
        timers.GLib.idle_add(0, () => { this.pending = false; callback(this, bytes); return false; });
    }, read_bytes_finish: bytes => ({ get_size: () => bytes.length, toArray: () => bytes }) };
    const stdin = { ...stream(), write_bytes_async(bytes, _, __, callback) {
        this.pending = true;
        assert.ok(bytes.data.length <= 4096);
        const count = Math.min(bytes.data.length, written); writes.push(bytes.data.slice(0, count));
        timers.GLib.idle_add(0, () => { this.pending = false; callback(this, count); return false; });
    }, write_bytes_finish: count => count };
    const exit = () => {
        if (exitScheduled) return;
        exitScheduled = true;
        timers.GLib.idle_add(0, () => { waitCallback(child, {}); return false; });
    };
    const child = { get_stdout_pipe: () => stdout, get_stdin_pipe: () => stdin, get_successful: () => successful,
        wait_async(cancellable, callback) { assert.equal(cancellable, null); status.waits++; waitCallback = callback; if (exited) exit(); },
        wait_finish() { status.reaped++; }, force_exit() { status.killed++; exit(); } };
    const Helper = vm.runInNewContext(`${completeSource}\nHelperProcess;`, {
        GLib: { ...timers.GLib, Bytes: class { constructor(data) { this.data = data; } } },
        Gio: { Cancellable: class { cancel() { status.cancelled++; } },
            SubprocessFlags: { STDOUT_PIPE: 1, STDERR_MERGE: 2, STDIN_PIPE: 4 },
            Subprocess: { new(argv, flags) { assert.deepEqual(argv, ['fake']); assert.equal(flags, 7); return child; } } },
        TextDecoder, TextEncoder, Uint8Array,
    });
    const worker = new Helper(['fake'], { onChunk: text => chunks.push(text), onExit: error => exits.push(error) });
    return { worker, chunks, exits, writes, status, stdout, stdin, exit, advance: milliseconds => timers.advance(milliseconds) };
}

test('resident helper owns stdin and waits for child exit after EOF without a lifetime deadline', () => {
    const h = helperFixture(new TextEncoder().encode('metadata\ntrailing data'), { exited: false });
    h.advance(60000);
    assert.equal(h.worker.eof, true); assert.equal(h.worker.done, false); assert.equal(h.exits.length, 0);
    assert.equal(h.status.waits, 1); assert.equal(h.status.reaped, 0);
    h.exit(); h.advance();
    assert.deepEqual(h.exits, [null]); assert.equal(h.worker.output, 'metadata\ntrailing data');
    assert.equal(h.status.reaped, 1); assert.equal(h.status.cancelled, 1); assert.equal(h.status.killed, 0);
    assert.equal(h.stdout.closed, true); assert.equal(h.stdin.closed, true);
});

test('resident helper decodes split UTF-8 and EOF and keeps only a diagnostic tail across an unlimited stream', () => {
    const text = `${'x'.repeat(8190)}🎧\ntrailing 🎧`;
    const utf8 = helperFixture(new TextEncoder().encode(text)); utf8.advance();
    assert.equal(utf8.chunks.join(''), text); assert.deepEqual(utf8.exits, [null]);
    const output = '🎧'.repeat(200) + '\n' + 'short frame\n'.repeat(110000);
    const long = helperFixture(new TextEncoder().encode(output)); long.advance();
    assert.deepEqual(long.exits, [null]); assert.equal(long.chunks.join(''), output);
    assert.equal(long.worker.output, output.slice(-8192), 'EOF must retain the bounded diagnostic tail');
});

test('resident helper rejects oversized frames and malformed or truncated UTF-8', () => {
    for (const text of ['x'.repeat(1024 * 1024 + 1), 'x'.repeat(1024 * 1024) + '\n']) {
        const h = helperFixture(new TextEncoder().encode(text), { exited: false }); h.advance();
        assert.equal(h.exits.length, 1); assert.match(h.exits[0].message, /frame is too large/);
        assert.equal(h.status.killed, 1); assert.equal(h.status.reaped, 1);
        assert.equal(h.stdout.closed, true); assert.equal(h.stdin.closed, true);
    }
    for (const data of [new Uint8Array([255, 10]), new Uint8Array([0xf0, 0x9f])]) {
        const h = helperFixture(data); h.advance(); assert.equal(h.exits.length, 1); assert.ok(h.exits[0]);
    }
});

test('resident helper preserves queued input across partial PIPE_BUF writes', () => {
    const h = helperFixture(new Uint8Array(), { exited: false, written: 37 });
    const first = '000007\n' + 'x'.repeat(10000), second = '🎧\n';
    h.worker.send(first); h.worker.send(second); h.advance();
    assert.equal(new TextDecoder().decode(Buffer.concat(h.writes)), first + second);
    assert.equal(h.worker.queue.length, 0); assert.equal(h.exits.length, 0);
    h.worker.stop(); h.advance(); assert.equal(h.status.reaped, 1);
});

test('resident helper bounds message bytes and queued bytes; cancellation clears pending IO once and still reaps', () => {
    const h = helperFixture(new Uint8Array(), { exited: false });
    assert.throws(() => h.worker.send('🎧'.repeat(256 * 1024 + 1)), /queue is too large/);
    assert.equal(h.worker.queue.length, 0);
    for (let index = 0; index < 8; index++) h.worker.send('x'.repeat(1024 * 1024));
    assert.throws(() => h.worker.send('x'), /queue is too large/);
    assert.equal(h.worker.queue.length, 8); assert.equal(h.writes.length, 1);
    h.worker.stop(); h.worker.stop(); h.advance();
    assert.equal(h.exits.length, 1); assert.match(h.exits[0].message, /cancelled/);
    assert.equal(h.worker.queue.length, 0); assert.equal(h.writes.length, 1);
    assert.equal(h.status.cancelled, 1); assert.equal(h.status.killed, 1); assert.equal(h.status.reaped, 1);
    assert.equal(h.stdout.closed, true); assert.equal(h.stdin.closed, true);
    assert.throws(() => h.worker.send('after stop'), /connection is closed/);
});

test('resident helper fails closed when a child stops accepting input', () => {
    const h = helperFixture(new Uint8Array(), { exited: false, written: 0 });
    h.worker.send('request\n'); h.advance();
    assert.equal(h.exits.length, 1); assert.match(h.exits[0].message, /stopped accepting input/);
    assert.equal(h.status.killed, 1); assert.equal(h.status.reaped, 1);
});

test('resident helper reports nonzero exit diagnostics after draining EOF', () => {
    const h = helperFixture(new TextEncoder().encode('bad request\n'), { successful: false }); h.advance();
    assert.equal(h.exits.length, 1); assert.equal(h.exits[0].message, 'bad request');
    assert.equal(h.worker.output, 'bad request\n'); assert.equal(h.status.reaped, 1); assert.equal(h.status.killed, 0);
});

test('resident message validation is strict, bounded and structural rather than domain-specific', () => {
    const action = { type: 'action', id: 1, operation: 'route', target: 'opaque', args: { route: 0 },
        guards: [{ key: 'opaque', expect: { routes: [{ anything: 'Rust-selected raw data' }] } }], cleanup: false };
    for (const value of [action, { type: 'snapshot', id: Number.MAX_SAFE_INTEGER }, { type: 'closed' },
        { type: 'model', model: { output: {}, input: {}, bluetooth: {} } }]) assert.deepEqual(nativeEvent(JSON.stringify(value)), value);
    for (const value of [null, [], 4, {}, { ...action, id: 0 }, { ...action, id: 1.5 }, { ...action, id: Number.MAX_SAFE_INTEGER + 1 },
        { ...action, operation: 'shell' }, { ...action, command: 'pactl' }, { ...action, target: '' },
        { ...action, args: [] }, { ...action, cleanup: 'yes' }, { ...action, guards: Array(65).fill({ key: null, expect: {} }) },
        { ...action, guards: [{ key: 'a', expect: {}, unknown: true }] }, { type: 'snapshot', id: 1, action },
        { type: 'model', model: { output: {}, input: {} } }, { type: 'closed', id: 1 }]) assert.throws(() => nativeEvent(JSON.stringify(value)));
    assert.throws(() => nativeEvent('{'));
});
}

// Tests consolidated from logic.test.js.
{
function waybarState({ output = {}, input = {}, bluetooth = {} } = {}) {
    return {
        output: {
            available: true, name: 'alsa_output.pci', statusName: 'Speakers',
            volume: 0.42, muted: false, ...output,
        },
        input: {
            available: true, name: 'alsa_input.pci', statusName: 'Microphone',
            volume: 0.7, muted: false, ...input,
        },
        bluetooth: { available: true, powered: false, devices: [], ...bluetooth },
    };
}

for (const command of ['status', 'toggle', 'close', 'bluetoothPower']) {
    test(`parseRequest accepts ${command} without a monitor`, () => {
        assert.deepEqual(parseRequest(JSON.stringify({ command })), { command });
    });
}

test('parseRequest preserves optional toggle monitors, including empty and maximum-length names', () => {
    for (const monitor of ['', 'eDP-1', 'DP-2', 'Display with spaces', 'é'.repeat(256)]) {
        const request = { command: 'toggle', monitor };
        assert.deepEqual(parseRequest(` \n${JSON.stringify(request)}\t`), request);
    }
});

test('parseRequest rejects malformed JSON, non-object requests, and unknown commands', () => {
    for (const raw of ['', '{', '{"command":"status",}', '{"command":"status"}{}'])
        assert.throws(() => parseRequest(raw), SyntaxError, raw);
    for (const request of [null, false, 0, 'status', [], [{}], {},
        ...['', 'Status', 'bluetooth-power', 'volume', 'constructor', null, 1, {}, []]
            .map(command => ({ command }))]) {
        assert.throws(() => parseRequest(JSON.stringify(request)), /Unknown audio panel command/, JSON.stringify(request));
    }
});

test('parseRequest rejects extra schema fields rather than accepting an action payload', () => {
    for (const field of ['value', 'kind', 'key', 'payload', 'constructor', '__proto__']) {
        const request = { command: 'toggle', [field]: null };
        assert.throws(() => parseRequest(JSON.stringify(request)), /Unexpected audio panel request field/, field);
    }
});

test('parseRequest permits a monitor only for toggle and requires a bounded string', () => {
    for (const command of ['status', 'close', 'bluetoothPower']) {
        for (const monitor of ['', 'DP-1', null]) {
            assert.throws(() => parseRequest(JSON.stringify({ command, monitor })), /Monitor must be a connector name/, command);
        }
    }
    for (const monitor of [null, true, 1, [], {}, 'a'.repeat(257)]) {
        assert.throws(() => parseRequest(JSON.stringify({ command: 'toggle', monitor })), /Monitor must be a connector name/);
    }
    for (let code = 0; code < 32; code++) {
        const monitor = `DP-${String.fromCharCode(code)}1`;
        assert.throws(() => parseRequest(JSON.stringify({ command: 'toggle', monitor })), /Monitor must be a connector name/, `control ${code}`);
    }
});

// Selection, reconciliation, volume and pairing validation live in resident_policy (Rust).

test('waybarStatus preserves the exact legacy JSON text, tooltip lines, and class contract', () => {
    assert.equal(waybarStatus(waybarState()),
        `{"text":"<span size='large'>󰖀</span>","tooltip":"Output: Speakers (42%)\\nInput: Microphone (70%)\\nBluetooth: off","class":"active"}`);
});

for (const [volume, percent, glyph] of [
    [-0.1, 0, '󰕿'], [undefined, 0, '󰕿'], [0, 0, '󰕿'], [0.0049, 0, '󰕿'],
    [0.005, 1, '󰖀'], [0.01, 1, '󰖀'], [0.3, 30, '󰖀'], [0.5, 50, '󰖀'],
    [0.5049, 50, '󰖀'], [0.505, 51, '󰕾'], [0.51, 51, '󰕾'], [1, 100, '󰕾'], [1.5, 150, '󰕾'],
]) {
    test(`waybarStatus uses the legacy output glyph at rounded ${percent}% (volume ${volume})`, () => {
        assert.deepEqual(JSON.parse(waybarStatus(waybarState({ output: { volume } }))), {
            text: `<span size='large'>${glyph}</span>`,
            tooltip: `Output: Speakers (${percent}%)\nInput: Microphone (70%)\nBluetooth: off`,
            class: 'active',
        });
    });
}

test('waybarStatus mutes the glyph for a muted or missing output regardless of volume', () => {
    for (const volume of [0, 0.5, 1.5]) {
        assert.deepEqual(JSON.parse(waybarStatus(waybarState({ output: { volume, muted: true } }))), {
            text: "<span size='large'>󰝟</span>",
            tooltip: `Output: Speakers (${Math.round(volume * 100)}%, muted)\nInput: Microphone (70%)\nBluetooth: off`,
            class: 'muted',
        });
    }
    for (const muted of [false, true]) {
        assert.deepEqual(JSON.parse(waybarStatus(waybarState({ output: { available: false, volume: 1.5, muted } }))), {
            text: "<span size='large'>󰝟</span>",
            tooltip: 'Output: none\nInput: Microphone (70%)\nBluetooth: off',
            class: 'unavailable',
        });
    }
});

test('waybarStatus keeps output and input availability, mute, names, and volumes independent', () => {
    for (const [input, line] of [
        [{ volume: 0, muted: false }, 'Input: Microphone (0%)'],
        [{ volume: 1, muted: true }, 'Input: Microphone (100%, muted)'],
        [{ available: false, muted: true }, 'Input: none'],
        [{ statusName: '', name: 'Raw microphone', volume: 0.1249 }, 'Input: Raw microphone (12%)'],
    ]) {
        assert.deepEqual(JSON.parse(waybarStatus(waybarState({ input }))), {
            text: "<span size='large'>󰖀</span>",
            tooltip: `Output: Speakers (42%)\n${line}\nBluetooth: off`, class: 'active',
        });
    }
    assert.deepEqual(JSON.parse(waybarStatus(waybarState({
        output: { available: false }, input: { volume: 0.125, muted: true },
    }))), {
        text: "<span size='large'>󰝟</span>",
        tooltip: 'Output: none\nInput: Microphone (13%, muted)\nBluetooth: off', class: 'unavailable',
    });
    assert.deepEqual(JSON.parse(waybarStatus(waybarState({
        output: { statusName: undefined, name: 'Raw output' }, input: { available: false },
    }))), {
        text: "<span size='large'>󰖀</span>",
        tooltip: 'Output: Raw output (42%)\nInput: none\nBluetooth: off', class: 'active',
    });
});

test('waybarStatus Bluetooth classes take precedence without changing the audio glyph or lines', () => {
    const connected = [{ name: 'Headset', address: 'AA:00', connected: true }];
    for (const [output, glyph, line, audioClass] of [
        [{}, '󰖀', 'Output: Speakers (42%)', 'active'],
        [{ muted: true }, '󰝟', 'Output: Speakers (42%, muted)', 'muted'],
        [{ available: false }, '󰝟', 'Output: none', 'unavailable'],
    ]) {
        for (const [bluetooth, bluetoothLine, className] of [
            [{ available: false, powered: true, devices: connected }, 'Bluetooth: off', audioClass],
            [{ available: true, powered: false, devices: connected }, 'Bluetooth: off', audioClass],
            [{ available: true, powered: true, devices: [] }, 'Bluetooth: on, nothing connected', 'bluetooth-on'],
            [{ available: true, powered: true, devices: connected }, 'Bluetooth: Headset', 'bluetooth-connected'],
        ]) {
            assert.deepEqual(JSON.parse(waybarStatus(waybarState({ output, bluetooth }))), {
                text: `<span size='large'>${glyph}</span>`,
                tooltip: `${line}\nInput: Microphone (70%)\n${bluetoothLine}`, class: className,
            });
        }
    }
});

test('waybarStatus sorts only connected Bluetooth names, falls back to addresses, and does not mutate devices', () => {
    const devices = Object.freeze([
        { name: 'Zulu', address: 'CC:00', connected: true },
        { name: 'Ignored paired device', address: 'DD:00', paired: true, connected: false },
        { name: '', address: 'BB:00', connected: true },
        { name: 'Alpha', address: 'AA:00', connected: true },
    ].map(device => Object.freeze(device)));
    const before = structuredClone(devices);
    const result = JSON.parse(waybarStatus(waybarState({ bluetooth: { powered: true, devices } })));
    assert.deepEqual(result, {
        text: "<span size='large'>󰖀</span>",
        tooltip: 'Output: Speakers (42%)\nInput: Microphone (70%)\nBluetooth: Alpha, BB:00, Zulu',
        class: 'bluetooth-connected',
    });
    assert.deepEqual(devices, before);
    const disconnected = JSON.parse(waybarStatus(waybarState({
        bluetooth: { powered: true, devices: [{ name: 'Paired', paired: true, connected: false }] },
    })));
    assert.equal(disconnected.tooltip, 'Output: Speakers (42%)\nInput: Microphone (70%)\nBluetooth: on, nothing connected');
    assert.equal(disconnected.class, 'bluetooth-on');
});

test('waybarStatus serializes quoted, control-character, Unicode, and markup-like names as one valid JSON line', () => {
    const outputName = 'Speakers "\\\n\r\t\0\u001f Café 🎧 <A&B>';
    const inputName = 'Mic "\\\n\u0001日本語';
    const bluetoothName = 'Headset "\\\n\0 🎧';
    const raw = waybarStatus(waybarState({
        output: { statusName: outputName },
        input: { statusName: '', name: inputName, muted: true },
        bluetooth: { powered: true, devices: [{ name: bluetoothName, connected: true }] },
    }));
    const expected = {
        text: "<span size='large'>󰖀</span>",
        tooltip: `Output: ${outputName} (42%)\nInput: ${inputName} (70%, muted)\nBluetooth: ${bluetoothName}`,
        class: 'bluetooth-connected',
    };
    assert.deepEqual(JSON.parse(raw), expected);
    assert.equal(raw, JSON.stringify(expected));
    assert.doesNotMatch(raw, /[\x00-\x1f]/);
    assert.ok(raw.includes('\\"'));
    assert.ok(raw.includes('\\\\'));
    assert.ok(raw.includes('\\u0000'));
});
}

// Exercise the real renderer with widget doubles, without loading GTK/GI.
{
const frontend = readFileSync(new URL('./frontend.js', import.meta.url), 'utf8');
const accessibleDouble = () => ({
    accessibleProperties: new Map(),
    accessibleStates: new Map(),
    update_state(states, values) {
        assert.equal(states.length, values.length, 'accessible states and values are paired');
        states.forEach((state, index) => {
            assert.ok(Object.values(mockGtk.AccessibleState).includes(state), 'known native accessible state');
            assert.equal(typeof values[index], 'number', 'GTK expanded states require integer GValues');
            assert.ok([mockGtk.AccessibleTristate.FALSE, mockGtk.AccessibleTristate.TRUE].includes(values[index]));
            this.accessibleStates.set(state, values[index] === mockGtk.AccessibleTristate.TRUE);
        });
    },
    update_property(properties, values) {
        assert.equal(properties.length, values.length, 'accessible properties and values are paired');
        properties.forEach((property, index) => {
            assert.ok(Object.values(mockGtk.AccessibleProperty).includes(property), 'known native accessible property');
            assert.equal(typeof values[index], 'string');
            this.accessibleProperties.set(property, values[index]);
        });
    },
});
const labelDouble = (text = '') => ({ ...accessibleDouble(), text, label: text,
    set_label(text) { this.text = this.label = text; } });
const buttonDouble = (text, clicked, classes = []) => {
    const child = labelDouble(text || '');
    return { ...accessibleDouble(), classes, get_child: () => child, click: () => clicked?.() };
};
const boxDouble = (_vertical, _spacing, children = []) => ({
    children: [...children],
    append(child) { child.parent = this; this.children.push(child); },
    remove(child) {
        const index = this.children.indexOf(child);
        assert.notEqual(index, -1);
        this.children.splice(index, 1);
    },
    reorder_child_after(child, previous) {
        this.remove(child);
        assert.ok(previous === null || this.children.includes(previous));
        this.children.splice(previous === null ? 0 : this.children.indexOf(previous) + 1, 0, child);
    },
    add_css_class() {},
});
const INVALID_LIST_POSITION = 0xffffffff;
class SingleSelectionDouble {
    constructor(properties) {
        Object.assign(this, properties);
        this.handlers = new Map();
        this.model = null;
        this.modelWrites = [];
        this.selectionWrites = [];

        this._selected = INVALID_LIST_POSITION;
        this.visible = this.sensitive = true;
    }

    connect(name, callback) {
        if (!this.handlers.has(name)) this.handlers.set(name, []);
        this.handlers.get(name).push(callback);
    }

    notifySelected() {
        for (const callback of this.handlers.get('notify::selected') || []) callback(this);
    }

    set_model(model) {
        this.model = model;
        this.modelWrites.push(model);
        this._selected = this.autoselect && model.get_n_items() ? 0 : INVALID_LIST_POSITION;
        // Selection notifications are not ListView activation, even during render.
        this.notifySelected();
    }

    get selected() { return this._selected; }
    set selected(index) {
        this._selected = index;
        this.selectionWrites.push(index);
        this.notifySelected();
    }
}
const signalWidgetDouble = properties => ({
    ...accessibleDouble(), visible: true, sensitive: true, ...properties, handlers: new Map(), focusCalls: 0,
    connect(name, callback) {
        if (!this.handlers.has(name)) this.handlers.set(name, []);
        this.handlers.get(name).push(callback);
    },
    emit(name, ...args) {
        for (const callback of this.handlers.get(name) || []) callback(this, ...args);
    },
    grab_focus() { this.focusCalls++; return true; },
});
const stringListDouble = {
    new(strings) {
        const values = Array.from(strings);
        return {
            get_n_items: () => values.length,
            get_string: index => values[index] ?? null,
            get_item: index => index >= 0 && index < values.length
                ? { get_string: () => values[index] } : null,
        };
    },
};
const volumeDouble = () => ({
    ...accessibleDouble(), value: 0, round_digits: -1, formatWrites: 0, sensitive: true,
    is_sensitive() { return this.sensitive && (!this.parent || this.parent.is_sensitive()); },
    set draw_value(value) { this.drawValue = value; this.round_digits = 2; },
    set_value(value) { this.value = value; },
    get_value() { return this.value; },
    set_format_value_func(format) { this.format = format; this.formatWrites++; },
    get_layout() { return { get_text: () => this.format(this, this.value) }; },
    add_css_class(name) { this.cssClass = name; },
});
const mockGtk = {
    AccessibleProperty: { LABEL: 'label', DESCRIPTION: 'description' },
    AccessibleState: { EXPANDED: 'expanded' },
    AccessibleTristate: { FALSE: 0, TRUE: 1 },
    RevealerTransitionType: { SLIDE_DOWN: 1 },
    Orientation: { VERTICAL: 1, HORIZONTAL: 0 }, Align: { END: 2, FILL: 0, START: 1, CENTER: 3 },
    PositionType: { TOP: 2 }, ScrollType: { STEP_BACKWARD: 6, STEP_FORWARD: 7 },
    PolicyType: { NEVER: 0, AUTOMATIC: 1 }, PropagationPhase: { CAPTURE: 1 },
    EventControllerScrollFlags: { BOTH_AXES: 3 }, StackTransitionType: { NONE: 0 },
    Box: function(properties) {
        return Object.assign(boxDouble(), { sensitive: true,
            measure() { return [0, 48, -1, -1]; },
            is_sensitive() { return this.sensitive && (!this.parent || this.parent.is_sensitive()); },
        }, properties);
    },
    Button: function(properties) {
        const widget = { ...buttonDouble('', null, properties.css_classes), ...properties,
            get_child() { return this.child; }, add_controller() {},
            connect(_name, callback) { this.click = callback; } };
        widget.child.parent = widget;
        return widget;
    },
    ToggleButton: function(properties) {
        const widget = signalWidgetDouble(properties);
        widget.get_child = () => widget.child;
        widget.child.parent = widget;
        widget._active = false;
        Object.defineProperty(widget, 'active', {
            get() { return this._active; },
            set(value) {
                if (this._active === value) return;
                this._active = value;
                this.emit('toggled');
            },
        });
        widget.click = () => { widget.active = !widget.active; };
        return widget;
    },
    Revealer: function(properties) {
        const widget = { reveal_child: false, ...properties, get_child() { return this.child; } };
        widget.child.parent = widget;
        return widget;
    },
    ListView: function(properties) {
        return Object.assign(signalWidgetDouble(properties), {
            measureCalls: [],
            measure(orientation, forSize) {
                const model = this.model.model;
                this.measureCalls.push({ orientation, forSize, model });
                return [0, (model?.get_n_items() || 0) * 32, -1, -1];
            },
        });
    },
    Label: function(properties) { return Object.assign(labelDouble(properties.label), properties); },
    Scale: function(properties) { return Object.assign(volumeDouble(), properties, { connect() {}, add_controller() {} }); },
    Adjustment: function(properties) { return { ...properties }; },
    EventControllerKey: function() { return { connect() {} }; },
    EventControllerScroll: function() { return { connect() {} }; },
    SignalListItemFactory: function() { return { connect(name, callback) { this[name] = callback; } }; },
    INVALID_LIST_POSITION, StringList: stringListDouble, SingleSelection: SingleSelectionDouble,
    Entry: function(properties) { return { ...properties, text: '', set_text(text) { this.text = text; }, connect() {} }; },
    Separator: function(properties) { return { ...properties, measure() { return [0, 1, -1, -1]; } }; },
    ScrolledWindow: function(properties) {
        const widget = { ...properties };
        widget.child.parent = widget;
        return widget;
    },
    Overlay: function(properties) {
        return Object.assign(boxDouble(), properties, {
            set_child(child) { this.append(child); }, add_overlay(child) { this.append(child); }, set_measure_overlay() {},
        });
    },
    DrawingArea: function(properties) { return { ...properties, set_draw_func() {} }; },
    Stack: function(properties) { return Object.assign(boxDouble(), properties, { add_named(child) { this.append(child); } }); },
};
const { AudioPanel, volumeBar } = vm.runInNewContext(
    frontend.slice(frontend.indexOf('function box('), frontend.indexOf('function style('))
    + frontend.slice(frontend.indexOf('function volumeValue('), frontend.indexOf('function frame('))
    + frontend.slice(frontend.indexOf('class AudioPanel'), frontend.indexOf('\nfunction monitorFor('))
    + '\n({ AudioPanel, volumeBar });', {
        Gtk: mockGtk, Pango: { EllipsizeMode: { END: 3 }, WrapMode: { WORD_CHAR: 2 } },
        style: (widget, name, enabled) => { widget[name] = enabled; },
        message: (widget, text) => { widget.set_label(text || ''); widget.visible = Boolean(text); },
    });
function assertNoTooltip(widget) {
    assert.ok(!widget.tooltip_text && !widget.tooltip_markup && !widget.has_tooltip, 'widget has no visual tooltip');
}
function assertNoInheritedTooltip(widget) {
    for (let current = widget; current; current = current.parent) assertNoTooltip(current);
}
function assertTooltipFreeTree(widget) {
    assertNoTooltip(widget);
    for (const child of widget.children || []) assertTooltipFreeTree(child);
    if (widget.child) assertTooltipFreeTree(widget.child);
}
function viewHarness(kind = 'output') {
    const state = {}, selections = [];
    const panel = Object.assign(Object.create(AudioPanel.prototype), {
        opened: true, currentTab: 1, audio: {}, failures: {}, press() {},
        backend: { snapshot: () => state, select: (...args) => selections.push(args) },
        render() { this.renderAudio(this.audio[kind], state[kind]); },
    });
    const content = panel.buildAudio(kind), view = panel.audio[kind];
    return { view, panel, state, selections, content,
        render(model = {}) {
            state[kind] = { available: true, nodeKey: 'current',
                name: kind === 'output' ? 'Speaker' : 'Microphone', volume: 0.25, ...model };
            panel.render();
        },
        select(index) { view.deviceList.emit('activate', index); },
        labels() {
            return Array.from({ length: view.deviceSelection.model.get_n_items() },
                (_, index) => view.deviceSelection.model.get_string(index));
        },
    };
}

function bluetoothViewHarness() {
    const panel = {
        deviceRows: new Map(), cardWidth: 400, failures: {}, backend: {}, action() {}, pairing: false,
    };
    const content = AudioPanel.prototype.buildBluetooth.call(panel);
    return { panel, content, render(devices, model = {}) {
        AudioPanel.prototype.renderBluetooth.call(panel, { available: true, powered: true, devices, ...model });
    }, order() {
        const keys = new Map([...panel.deviceRows].map(([key, row]) => [row.widget, key]));
        return panel.bluetoothRows.children.filter(child => child !== panel.bluetoothSeparator || child.visible)
            .map(child => child === panel.bluetoothSeparator ? 'divider' : keys.get(child));
    } };
}

test('audio never creates visual tooltips or query handlers, including metadata labels', () => {
    assert.doesNotMatch(frontend, /tooltip|Gtk\.Settings/i);
});

test('the whole constructed panel stays tooltip-free across enabled and disabled rerenders', () => {
    const state = { bluetooth: { available: true, powered: true } };
    for (const kind of ['output', 'input']) state[kind] = {
        available: true, nodeKey: `current:${kind}`, name: `${kind} device`, volume: 0.25,
        choices: [{ key: 'default', label: 'Default', description: 'Default route', default: true },
            { key: 'other', label: 'Other', description: 'Other route' }],
    };
    const panel = Object.assign(Object.create(AudioPanel.prototype), {
        audio: {}, content: boxDouble(), opened: true, currentTab: 1, cardWidth: 400, deviceRows: new Map(), failures: {},
        backend: { snapshot: () => state }, press() {},
    });
    panel.build();
    const checkButtons = widget => {
        let count = 0;
        if (widget.css_classes?.some(name => ['control-button', 'volume-step', 'device-dropdown'].includes(name))) {
            assertNoInheritedTooltip(widget);
            assertNoTooltip(widget.get_child());
            count++;
        }
        for (const child of widget.children || []) count += checkButtons(child);
        if (widget.child) count += checkButtons(widget.child);
        return count;
    };
    assertTooltipFreeTree(panel.content);
    assert.equal(checkButtons(panel.content), 13, 'tabs, mute, steppers, device toggles and pairing buttons are actually constructed');
    for (const patch of [{}, { muted: true }, { available: false },
        { available: true, nodeKey: '' }, { nodeKey: 'replacement', muted: false }, { busy: true }]) {
        for (const kind of ['output', 'input']) state[kind] = { ...state[kind], ...patch };
        panel.render();
        assertTooltipFreeTree(panel.content);
        assert.equal(checkButtons(panel.content), 13);
        for (const kind of ['output', 'input']) {
            const view = panel.audio[kind], model = state[kind];
            const enabled = Boolean(model.available && model.nodeKey);
            assertNoInheritedTooltip(view.volume);
            assertNoInheritedTooltip(view.volumeBar.widget);
            assert.equal(view.mute.accessibleProperties.get(mockGtk.AccessibleProperty.LABEL), model.muted ? 'Unmute' : 'Mute');
            assert.equal(view.volume.accessibleProperties.get(mockGtk.AccessibleProperty.LABEL),
                kind === 'output' ? 'Output volume' : 'Input volume');
            for (const control of [view.volume, view.mute])
                assert.equal(control.accessibleProperties.get(mockGtk.AccessibleProperty.DESCRIPTION),
                    enabled ? '' : 'No default audio device is available.');
            assert.equal(view.heading.accessibleProperties.get(mockGtk.AccessibleProperty.LABEL),
                `${kind === 'output' ? 'Output' : 'Input'} · ${model.available ? model.name : 'No device'}`);
            assertNoInheritedTooltip(view.devices);
            assert.equal(view.devices.accessibleProperties.get(mockGtk.AccessibleProperty.DESCRIPTION), 'Default route',
                'dropdown metadata is preserved for assistive technology, not a tooltip');
            assert.equal(view.devices.accessibleProperties.get(mockGtk.AccessibleProperty.LABEL),
                kind === 'output' ? 'Output device' : 'Input device');
        }
    }
});

test('Bluetooth buttons retain accessible labels without tooltips through pairing, busy and renamed-device rerenders', () => {
    const h = bluetoothViewHarness();
    const device = { key: 'headset', name: 'Headset <name>', address: 'AA:01', paired: true };
    for (const [properties, model] of [
        [{}, {}],
        [{ connected: true }, { pairing: true, pairingMessage: { text: 'Pairing…', prompt: 'Enter PIN:', needsAnswer: true } }],
        [{ busy: true }, {}],
        [{ name: 'Renamed headset' }, { powered: false }],
        [{ name: '', paired: false }, {}],
        [{}, {}],
    ]) {
        const current = { ...device, ...properties };
        h.render([current], model);
        assertTooltipFreeTree(h.content);
        const row = h.panel.deviceRows.get(device.key);
        for (const control of [row.action, row.forget, h.panel.dismissPairing, h.panel.send, h.panel.cancel]) {
            assertNoInheritedTooltip(control);
            assertNoTooltip(control.get_child());
        }
        assert.equal(row.forget.accessibleProperties.get(mockGtk.AccessibleProperty.LABEL),
            `Forget ${current.name || current.address}`);
        assert.equal(h.panel.dismissPairing.accessibleProperties.get(mockGtk.AccessibleProperty.LABEL), 'Dismiss pairing message');
        assert.equal(h.panel.dismissPairing.accessibleProperties.get(mockGtk.AccessibleProperty.DESCRIPTION),
            model.pairing ? 'Answer or cancel pairing before dismissing this message' : '');
        assert.equal(row.name.accessibleProperties.get(mockGtk.AccessibleProperty.LABEL), current.name || current.address,
            'full device names remain accessible without hover tooltips');
    }
});

test('Bluetooth connected rows keep their semantic status and action without selected state', () => {
    const h = bluetoothViewHarness();
    const device = { key: 'headset', name: 'Headset', paired: true, connected: true, battery: 0.6 };
    h.render([device]);
    const row = h.panel.deviceRows.get(device.key);
    assert.equal(row.widget.connected, true, 'the semantic connected class is retained');
    assert.equal(row.status.label, 'Connected · 60%');
    assert.equal(row.action.get_child().label, 'Disconnect');
    assert.notEqual(row.widget.selected, true, 'a connected device is not a selected list row');
    h.render([{ ...device, connected: false }]);
    assert.equal(h.panel.deviceRows.get(device.key), row);
    assert.equal(row.widget.connected, false);
    assert.equal(row.status.label, 'Paired · 60%');
    assert.equal(row.action.get_child().label, 'Connect');
});

test('Bluetooth renderer keeps connected and paired devices above named unpaired devices', () => {
    const h = bluetoothViewHarness();
    const devices = Object.freeze([
        { key: 'unnamed', name: '', address: '01' },
        { key: 'paired', name: 'Paired speaker', paired: true },
        { key: 'named-z', name: 'New speaker' },
        { key: 'connected', name: 'Connected headset', connected: true },
        { key: 'address', name: 'AA:BB:CC:DD:EE:FF', address: 'AA:BB:CC:DD:EE:FF' },
        { key: 'named-a', name: 'New keyboard' },
        { key: 'address-alias', name: 'aa-bb-cc-dd-ee-ff', address: 'AA:BB:CC:DD:EE:FF' },
        { key: 'blank', name: '   ', address: '02' },
    ].map(Object.freeze));
    h.render(devices);
    assert.deepEqual(h.order(), ['connected', 'paired', 'divider', 'named-z', 'named-a',
        'unnamed', 'address', 'address-alias', 'blank']);
    assert.equal(h.panel.deviceRows.get('named-z').forget.visible, false);
    assert.equal(h.panel.deviceRows.get('named-z').action.get_child().text, 'Pair');
    assert.deepEqual(devices.map(device => device.key), ['unnamed', 'paired', 'named-z',
        'connected', 'address', 'named-a', 'address-alias', 'blank']);
    h.render(devices.filter(device => device.key.startsWith('named-')));
    assert.deepEqual(h.order(), ['named-z', 'named-a']);
    assert.equal(h.panel.bluetoothSeparator.visible, false);
});

test('Bluetooth viewport fits five unpaired rows and any remembered rows preceding them', () => {
    const h = bluetoothViewHarness();
    const available = Array.from({ length: 8 }, (_, index) => ({ key: `available-${index}` }));
    h.render(available);
    assert.equal(h.panel.bluetoothList.max_content_height, 5 * 48 + 4 * 6);
    h.render([{ key: 'remembered-a', paired: true }, { key: 'remembered-b', connected: true }, ...available]);
    assert.equal(h.panel.bluetoothList.max_content_height, 7 * 48 + 6 * 6 + 1 + 6);
    assert.equal(h.panel.bluetoothList.propagate_natural_height, true);
    assert.equal(h.panel.bluetoothPage.propagate_natural_height, true);
});

test('paired Bluetooth devices precede all unpaired devices regardless of names', () => {
    const h = bluetoothViewHarness();
    h.render([
        { key: 'unnamed-paired', paired: true, name: 'AA-BB-CC-DD-EE-FF', address: 'AA:BB:CC:DD:EE:FF' },
        { key: 'named-paired', paired: true, name: 'Speaker' },
        { key: 'other-address', name: '11-22-33-44-55-66', address: 'AA:BB:CC:DD:EE:00' },
        { key: 'named-unpaired', name: 'Keyboard' },
    ]);
    assert.deepEqual(h.order(), ['unnamed-paired', 'named-paired', 'divider', 'named-unpaired', 'other-address']);
});

test('Bluetooth name discovery retains unpaired ordering and pairing promotes the existing row', () => {
    const h = bluetoothViewHarness();
    const devices = [{ key: 'remembered', name: 'Speaker', paired: true },
        { key: 'discovered', name: 'AA-BB-CC-DD-EE-FF', address: 'AA:BB:CC:DD:EE:FF' }];
    h.render(devices);
    const row = h.panel.deviceRows.get('discovered');
    assert.deepEqual(h.order(), ['remembered', 'divider', 'discovered']);
    devices[1].name = 'New headphones';
    h.render(devices);
    assert.deepEqual(h.order(), ['remembered', 'divider', 'discovered']);
    assert.equal(h.panel.deviceRows.get('discovered'), row);
    devices[1].paired = true;
    h.render(devices);
    assert.deepEqual(h.order(), ['remembered', 'discovered']);
    assert.equal(row.forget.visible, true);
    assert.equal(row.action.get_child().text, 'Connect');
    assert.equal(h.panel.bluetoothSeparator.visible, false);
});

test('Bluetooth renderer keeps remembered devices above unnamed available devices with paired-only forget', () => {
    const h = bluetoothViewHarness();
    const devices = Object.freeze([
        { key: 'available-z' }, { key: 'paired-z', paired: true },
        { key: 'connected', connected: true }, { key: 'available-a' }, { key: 'paired-a', paired: true },
    ].map(Object.freeze));
    h.render(devices);
    assert.deepEqual(h.order(), ['connected', 'paired-z', 'paired-a', 'divider', 'available-z', 'available-a']);
    assert.equal(h.panel.bluetoothSeparator.visible, true);
    assert.deepEqual(devices.map(device => device.key), ['available-z', 'paired-z', 'connected', 'available-a', 'paired-a']);
    for (const device of devices) {
        const row = h.panel.deviceRows.get(device.key);
        assert.equal(row.forget.visible, Boolean(device.paired));
        assert.equal(row.forget.sensitive, Boolean(device.paired));
        assert.equal(row.action.get_child().text, device.connected ? 'Disconnect' : device.paired ? 'Connect' : 'Pair');
    }
    h.render(devices, { pairing: true });
    assert.equal(h.panel.deviceRows.get('paired-z').forget.visible, true);
    assert.equal(h.panel.deviceRows.get('paired-z').forget.sensitive, false);
    assert.equal(h.panel.deviceRows.get('connected').forget.visible, false);
    h.render(devices.map(device => ({ ...device, busy: true })));
    assert.equal(h.panel.deviceRows.get('paired-z').forget.sensitive, false);
});

test('Bluetooth renderer updates existing rows and divider placement through pairing changes and removals', () => {
    const h = bluetoothViewHarness();
    const devices = [{ key: 'first' }, { key: 'second', paired: true }, { key: 'third' }];
    h.render(devices);
    const first = h.panel.deviceRows.get('first'), second = h.panel.deviceRows.get('second');
    assert.deepEqual(h.order(), ['second', 'divider', 'first', 'third']);
    devices[0].paired = true;
    h.render(devices);
    assert.equal(h.panel.deviceRows.get('first'), first);
    assert.equal(first.forget.visible, true);
    assert.deepEqual(h.order(), ['first', 'second', 'divider', 'third']);
    devices[1].paired = false;
    h.render(devices);
    assert.equal(h.panel.deviceRows.get('second'), second);
    assert.equal(second.forget.visible, false);
    assert.equal(second.forget.sensitive, false);
    assert.deepEqual(h.order(), ['first', 'divider', 'second', 'third']);
    devices[0].paired = false;
    h.render(devices);
    assert.equal(h.panel.bluetoothSeparator.visible, false);
    assert.deepEqual(h.order(), ['first', 'second', 'third']);
    h.render(devices.map(device => ({ ...device, paired: true })));
    assert.equal(h.panel.bluetoothSeparator.visible, false);
    assert.deepEqual(h.order(), ['first', 'second', 'third']);
    h.render([]);
    assert.equal(h.panel.deviceRows.size, 0);
    assert.equal(h.panel.bluetoothSeparator.visible, false);
    assert.deepEqual(h.panel.bluetoothRows.children, [h.panel.bluetoothSeparator]);
});

test('Bluetooth status card has a compact dismiss button wired to notice dismissal, not cancellation', () => {
    let dismissals = 0;
    const panel = { backend: { dismissPairingMessage() { dismissals++; } }, action(_kind, callback) { callback(); } };
    AudioPanel.prototype.buildBluetooth.call(panel);
    assert.equal(panel.dismissPairing.get_child().text, '×');
    assert.ok(panel.dismissPairing.classes.includes('pairing-dismiss'));
    assertNoInheritedTooltip(panel.dismissPairing);
    assert.equal(panel.dismissPairing.accessibleProperties.get(mockGtk.AccessibleProperty.LABEL), 'Dismiss pairing message');
    assert.equal(panel.answer.visible, false, 'entry starts hidden to avoid flashing during automatic pairing');
    assert.equal(panel.send.visible, false);
    panel.dismissPairing.click();
    assert.equal(dismissals, 1);
    assert.equal(panel.pairingCard.children[0].children[0], panel.pairingStatus, 'status is directly sized, not a log scroller');
    assert.equal(panel.pairingCard.children[0].children[1], panel.dismissPairing);
    assert.doesNotMatch(frontend, /pairingLog|pairing-log/);
});

test('Bluetooth renderer shows one current pairing status and hides prompt controls after completion or dismissal', () => {
    const h = bluetoothViewHarness();
    h.render([], { pairing: true, pairingMessage: { state: 'pairing', text: 'Pairing with Headset (AA:01)…', prompt: 'Enter PIN:', needsAnswer: true } });
    assert.equal(h.panel.pairingCard.visible, true);
    assert.equal(h.panel.pairingStatus.text, 'Pairing with Headset (AA:01)…');
    assert.equal(h.panel.pairingPrompt.text, 'Enter PIN:');
    assert.equal(h.panel.pairingControls.visible, true);
    assert.equal(h.panel.answer.visible, true);
    assert.equal(h.panel.send.visible, true);
    assert.equal(h.panel.dismissPairing.sensitive, false);
    assertNoInheritedTooltip(h.panel.dismissPairing);
    assert.equal(h.panel.dismissPairing.accessibleProperties.get(mockGtk.AccessibleProperty.LABEL), 'Dismiss pairing message');
    assert.equal(h.panel.dismissPairing.accessibleProperties.get(mockGtk.AccessibleProperty.DESCRIPTION),
        'Answer or cancel pairing before dismissing this message');
    h.render([], { pairingMessage: { state: 'paired', text: 'Paired with Headset (AA:01).', prompt: '' } });
    assert.equal(h.panel.pairingCard.visible, true);
    assert.equal(h.panel.pairingStatus.text, 'Paired with Headset (AA:01).');
    assert.equal(h.panel.dismissPairing.sensitive, true);
    assertNoInheritedTooltip(h.panel.dismissPairing);
    assert.equal(h.panel.dismissPairing.accessibleProperties.get(mockGtk.AccessibleProperty.DESCRIPTION), '');
    assert.equal(h.panel.pairingPrompt.visible, false);
    assert.equal(h.panel.pairingControls.visible, false);
    h.render([], { pairingMessage: { state: 'failed', text: 'Failed to pair with Headset (AA:01): rejected', prompt: '' } });
    assert.equal(h.panel.pairingStatus.failure, true);
    assert.equal(h.panel.pairingPrompt.visible, false);
    assert.equal(h.panel.bluetoothError.visible, false);
    h.render([], { pairing: true, pairingMessage: null });
    assert.equal(h.panel.pairingCard.visible, false);
    assert.equal(h.panel.pairingControls.visible, false);
    assert.equal(h.panel.pairingStatus.text, '');
});

test('automatic pairing never shows an entry field or Send button without an authentication prompt', () => {
    const h = bluetoothViewHarness();
    h.render([], { pairing: true, pairingMessage: { state: 'pairing', text: 'Pairing with Headset…', prompt: '' } });
    assert.equal(h.panel.pairingCard.visible, true);
    assert.equal(h.panel.pairingControls.visible, false);
    assert.equal(h.panel.answer.visible, false);
    assert.equal(h.panel.send.visible, false);
    assert.equal(h.panel.send.sensitive, false);
    assert.equal(h.panel.dismissPairing.sensitive, true);
    h.render([], { pairingMessage: { state: 'paired', text: 'Paired with Headset.', prompt: '' } });
    assert.equal(h.panel.pairingControls.visible, false);
    assert.equal(h.panel.answer.visible, false);
    assert.equal(h.panel.send.visible, false);
});

for (const prompt of [
    'Please type in the pairing PIN for AA:01.',
    'Please type in the 6-digit passkey for AA:01.',
    'Does passkey 000007 match the one shown on AA:01? Type yes or no.',
    'Confirm pairing with AA:01? Type yes or no.',
]) {
    test(`pairing entry appears only while a real response is requested: ${prompt}`, () => {
        const h = bluetoothViewHarness();
        const notice = { state: 'pairing', text: 'Pairing with Headset…', prompt: '' };
        h.render([], { pairing: true, pairingMessage: notice });
        h.render([], { pairing: true, pairingMessage: { ...notice, prompt, needsAnswer: true } });
        assert.equal(h.panel.answer.visible, true);
        assert.equal(h.panel.send.visible, true);
        assert.equal(h.panel.pairingControls.visible, true);
        h.panel.answer.text = '000007';
        h.render([], { pairing: true, pairingMessage: { ...notice, prompt, needsAnswer: true } });
        assert.equal(h.panel.answer.text, '000007', 'ordinary renders preserve an answer being typed');
        assert.equal(h.panel.send.sensitive, true);
        h.render([], { pairing: true, pairingMessage: notice });
        assert.equal(h.panel.answer.visible, false);
        assert.equal(h.panel.send.visible, false);
        assert.equal(h.panel.send.sensitive, false);
        assert.equal(h.panel.pairingControls.visible, false);
        assert.equal(h.panel.answer.text, '', 'a consumed request does not retain its PIN');
    });
}

for (const prompt of [
    'Type 042311 on the device to finish pairing.',
    'Type 042311 on the device and press Enter to finish pairing.',
]) {
    test(`display-only pairing instructions do not show a panel input: ${prompt}`, () => {
        const h = bluetoothViewHarness();
        h.render([], { pairing: true, pairingMessage: { state: 'pairing', text: 'Pairing with Keyboard…', prompt: 'Enter PIN:', needsAnswer: true } });
        h.panel.answer.text = 'old PIN';
        h.render([], { pairing: true, pairingMessage: { state: 'pairing', text: 'Pairing with Keyboard…', prompt } });
        assert.equal(h.panel.pairingPrompt.text, prompt);
        assert.equal(h.panel.pairingPrompt.visible, true);
        assert.equal(h.panel.answer.visible, false);
        assert.equal(h.panel.send.visible, false);
        assert.equal(h.panel.send.sensitive, false);
        assert.equal(h.panel.answer.text, '');
        assert.equal(h.panel.pairingControls.visible, true, 'Cancel remains reachable while entering a code on the device');
    });
}

test('hidden pairing input cannot send a response, while an actual request can', () => {
    const h = bluetoothViewHarness(), answers = [];
    h.panel.backend.sendPairingAnswer = text => answers.push(text);
    h.panel.action = (_kind, callback) => callback();
    h.render([], { pairing: true, pairingMessage: { state: 'pairing', text: 'Pairing…', prompt: '' } });
    h.panel.answer.text = 'stale PIN';
    AudioPanel.prototype.sendAnswer.call(h.panel);
    assert.deepEqual(answers, []);
    h.render([], { pairing: true, pairingMessage: { state: 'pairing', text: 'Pairing…', prompt: 'Enter PIN:', needsAnswer: true } });
    h.panel.answer.text = '000123';
    AudioPanel.prototype.sendAnswer.call(h.panel);
    assert.deepEqual(answers, ['000123']);
    assert.equal(h.panel.answer.text, '');
});

test('Bluetooth frontend has no manual discovery toggle', () => {
    assert.doesNotMatch(frontend, /this\.scan\b|toggleDiscovery|Stop scan/);
});

test('volume bar keeps the native scale and its precision, stepping only through move-slider', () => {
    const scale = volumeDouble(), events = [];
    let dragging = true, volumeTarget = 'old';
    scale.emit = (...args) => events.push([dragging, volumeTarget, ...args]);
    const bar = volumeBar(scale, () => { dragging = false; volumeTarget = null; });
    const [backward, native, forward] = bar.widget.children;
    assert.equal(native, scale);
    assert.equal(scale.round_digits, -1, 'draw-value must not introduce rounding of native volume mutations');
    assert.equal(scale.drawValue, true);
    assert.equal(scale.cssClass, 'volume-track');
    assert.deepEqual(Array.from(backward.css_classes), ['volume-step']);
    assert.deepEqual(Array.from(forward.css_classes), ['volume-step']);
    assert.equal(backward.child.label, '◁');
    assert.equal(forward.child.label, '▷');
    for (const widget of [backward, native, forward, bar.widget]) assertNoInheritedTooltip(widget);
    assert.equal(backward.accessibleProperties.get(mockGtk.AccessibleProperty.LABEL), 'Decrease volume');
    assert.equal(forward.accessibleProperties.get(mockGtk.AccessibleProperty.LABEL), 'Increase volume');
    backward.click();
    dragging = true; volumeTarget = 'stale';
    forward.click();
    assert.deepEqual(events, [[false, null, 'move-slider', 6], [false, null, 'move-slider', 7]]);
    assert.doesNotMatch(frontend, /view\.percent\b|volume-slider|volume-value/);
});

for (const disabled of ['scale', 'ancestor']) {
    test(`volume bar ignores both arrows with a disabled ${disabled} before clearing drag ownership or emitting`, () => {
        const scale = volumeDouble(), events = [];
        let dragging = true, volumeTarget = 'owned-drag', beforeSteps = 0;
        scale.emit = (...args) => events.push([dragging, volumeTarget, ...args]);
        const bar = volumeBar(scale, () => {
            beforeSteps++;
            dragging = false;
            volumeTarget = null;
        });
        const [backward, , forward] = bar.widget.children;
        const target = disabled === 'scale' ? scale : bar.widget;
        target.sensitive = false;
        assert.equal(scale.sensitive, disabled === 'ancestor', 'ancestor sensitivity is distinct from the scale property');
        assert.equal(scale.is_sensitive(), false);
        backward.click();
        forward.click();
        assert.equal(beforeSteps, 0, 'disabled clicks must not run beforeStep');
        assert.equal(dragging, true);
        assert.equal(volumeTarget, 'owned-drag');
        assert.deepEqual(events, [], 'disabled clicks must not emit move-slider');
        target.sensitive = true;
        forward.click();
        assert.equal(beforeSteps, 1, 'stepping resumes when effective sensitivity is restored');
        assert.deepEqual(events, [[false, null, 'move-slider', 7]]);
    });
}

test('volume bar invalidates the native cached value only on availability changes', () => {
    const scale = volumeDouble(), bar = volumeBar(scale, () => {});
    scale.set_value(0.3749);
    scale.sensitive = true;
    bar.setAvailable(true);
    assert.equal(bar.widget.sensitive, true);
    assert.equal(scale.get_layout().get_text(), '37%');
    assert.equal(scale.formatWrites, 1);
    scale.sensitive = false;
    bar.setAvailable(false);
    assert.equal(bar.widget.sensitive, false);
    assert.equal(scale.get_layout().get_text(), '—');
    assert.equal(scale.formatWrites, 2);
    bar.setAvailable(false);
    assert.equal(scale.formatWrites, 2, 'unchanged snapshots do not reset formatting');
    scale.sensitive = true;
    bar.setAvailable(true);
    assert.equal(bar.widget.sensitive, true);
    assert.equal(scale.get_layout().get_text(), '37%');
    assert.equal(scale.formatWrites, 3, 'same-value recovery still invalidates the cached placeholder');
});

test('audio device selectors use inline native GTK lists without popup or per-device button backends', () => {
    assert.match(frontend, /new Gtk\.ToggleButton\(/);
    assert.match(frontend, /new Gtk\.SingleSelection\(/);
    assert.match(frontend, /new Gtk\.ListView\(/);
    assert.match(frontend, /Gtk\.StringList\.new\(/);
    assert.match(frontend, /new Gtk\.Revealer\(/);
    assert.doesNotMatch(frontend, /Gtk\.(?:DropDown|Popover)|view\.(?:rows|choices|updatingChoices)\b/);
});

test('opening another inline audio selector collapses the first and resetInteractions collapses both', () => {
    const h = viewHarness('output');
    h.render({ choices: [{ key: 'out-default', label: 'Speakers', default: true }, { key: 'out-other', label: 'Dock' }] });
    h.panel.buildAudio('input');
    const input = h.panel.audio.input;
    h.panel.renderAudio(input, { available: true, nodeKey: 'input', choices: [
        { key: 'in-default', label: 'Mic', default: true }, { key: 'in-other', label: 'USB mic' },
    ] });
    h.view.devices.click();
    assert.equal(h.view.deviceChoices.reveal_child, true);
    input.devices.click();
    assert.equal(input.deviceChoices.reveal_child, true);
    assert.equal(h.view.deviceChoices.reveal_child, false);
    assert.equal(h.view.devices.active, false);
    assert.equal(h.view.deviceArrow.label, '⌄');
    assert.equal(h.view.devices.accessibleStates.get(mockGtk.AccessibleState.EXPANDED), false);
    h.view.devices.click();
    assert.equal(h.view.deviceChoices.reveal_child, true);
    assert.equal(input.deviceChoices.reveal_child, false);
    for (const view of Object.values(h.panel.audio)) {
        view.dragging = true;
        view.volumeTarget = view.muteTarget = 'stale';
    }
    h.panel.resetInteractions();
    for (const view of Object.values(h.panel.audio)) {
        assert.equal(view.deviceChoices.reveal_child, false);
        assert.equal(view.devices.active, false);
        assert.equal(view.deviceArrow.label, '⌄');
        assert.equal(view.devices.accessibleStates.get(mockGtk.AccessibleState.EXPANDED), false);
        assert.equal(view.dragging, false);
        assert.equal(view.volumeTarget, null);
        assert.equal(view.muteTarget, null);
    }
    assert.deepEqual(h.selections, []);
});

for (const kind of ['output', 'input']) {
    const choices = Object.freeze([
        Object.freeze({ key: `native:${kind}:10:one`, label: 'USB <Audio> & 🎧',
            description: 'Built-in device route', default: true }),
        Object.freeze({ key: `policy:${kind}:20:two`, label: 'USB <Audio> & 🎧',
            description: 'External device route', default: false }),
    ]);

    test(`${kind} selector constructs an inline, bounded, animated native list with a separate trigger`, () => {
        const h = viewHarness(kind), { view } = h;
        h.render({ choices });
        assert.ok(view.devices.css_classes.includes('device-dropdown'));
        assert.ok(!view.devices.css_classes.includes('control-button'), 'the trigger does not inherit custom control highlights');
        assert.deepEqual(view.devices.child.children, [view.deviceLabel, view.deviceArrow]);
        assert.equal(view.deviceLabel.use_markup, false);
        assert.equal(view.deviceLabel.ellipsize, 3);
        assert.equal(view.deviceLabel.label, choices[0].label);
        assert.equal(view.deviceArrow.label, '⌄');
        assert.equal(view.deviceSelection.autoselect, false);
        assert.equal(view.deviceSelection.can_unselect, true);
        assert.equal(view.deviceSelection.selected, INVALID_LIST_POSITION, 'the default label does not select a row');
        assert.equal(view.deviceList.model, view.deviceSelection);
        assert.equal(view.deviceList.single_click_activate, true);
        assert.ok(view.deviceList.css_classes.includes('device-list'));
        assert.equal(view.deviceChoices.transition_type, mockGtk.RevealerTransitionType.SLIDE_DOWN);
        assert.equal(view.deviceChoices.transition_duration, 180);
        assert.equal(view.deviceChoices.reveal_child, false);
        const options = view.deviceChoices.child;
        assert.equal(options.child, view.deviceList);
        assert.equal(options.max_content_height, 240);
        assert.equal(options.propagate_natural_height, true);
        assert.equal(options.margin_top, 6);
        assert.ok(options.css_classes.includes('device-options'));
        assert.equal(view.devices.parent, view.deviceChoices.parent, 'the trigger and revealer share the inline layout');
        assert.equal(view.devices.parent.orientation, mockGtk.Orientation.VERTICAL);
        assert.ok(view.devices.parent.children.indexOf(view.deviceChoices) > view.devices.parent.children.indexOf(view.devices));
        for (const widget of [view.devices, view.deviceLabel, view.deviceArrow, view.deviceList, options])
            assertNoInheritedTooltip(widget);
    });

    test(`${kind} options reserve measured height up to 240px on model changes without revealing closed content`, () => {
        for (const opened of [true, false]) {
            const h = viewHarness(kind), { view } = h;
            h.panel.opened = opened;
            const options = view.deviceChoices.get_child();
            const extra = Array.from({ length: 10 }, (_, index) => ({
                key: `native:${kind}:extra:${index}`, label: `Extra device ${index}`, default: false,
            }));
            for (const [entries, expectedHeight] of [
                [choices, 64],
                [[...choices, ...extra], 240],
                [[...choices, extra[0]], 96],
                [[choices[0]], 32],
                [[], 32],
            ]) {
                const before = view.deviceList.measureCalls.length;
                h.render({ choices: entries });
                assert.equal(view.deviceList.measureCalls.length, before + 1, 'model changes remeasure the list');
                const measurement = view.deviceList.measureCalls.at(-1);
                assert.equal(measurement.orientation, mockGtk.Orientation.VERTICAL);
                assert.equal(measurement.forSize, -1);
                assert.equal(measurement.model, view.deviceSelection.model, 'measure the replacement model, not stale rows');
                assert.equal(options.min_content_height, expectedHeight, 'use natural height, capped to the viewport maximum');
                assert.ok(options.min_content_height > 0, 'the outer page cannot squeeze the options viewport to zero');
                assert.ok(options.min_content_height <= options.max_content_height);
                assert.equal(options.max_content_height, 240);
                assert.equal(view.deviceChoices.reveal_child, false, 'measurement must not expand the revealer');
                assert.equal(view.devices.active, false);
                assert.equal(view.deviceArrow.label, '⌄');
                assert.equal(view.devices.accessibleStates.get(mockGtk.AccessibleState.EXPANDED), false);
                h.render({ choices: entries, volume: 0.6, muted: true });
                assert.equal(view.deviceList.measureCalls.length, before + 1, 'unchanged models retain their measured minimum');
                assert.equal(options.min_content_height, expectedHeight);
                assert.equal(view.deviceChoices.reveal_child, false);
            }
            assert.deepEqual(h.selections, []);
        }
    });

    test(`${kind} list factory renders literal ellipsized accessible labels without tooltips, including recycled rows`, () => {
        const h = viewHarness(kind);
        h.render({ choices });
        const { factory } = h.view.deviceList;
        const item = {
            position: 0,
            set_child(child) { this.child = child; },
            get_child() { return this.child; },
            get_position() { return this.position; },
            get_item() { return h.view.deviceSelection.model.get_item(this.position); },
        };
        factory.setup(factory, item);
        for (const index of [0, 1]) {
            item.position = index;
            factory.bind(factory, item);
            assert.equal(item.child.label, choices[index].label);
            assert.equal(item.child.use_markup, false);
            assert.equal(item.child.ellipsize, 3);
            assert.equal(item.child.accessibleProperties.get(mockGtk.AccessibleProperty.DESCRIPTION), choices[index].description);
            assertNoTooltip(item.child);
        }
        h.render({ choices: choices.map(choice => ({ ...choice, description: '' })) });
        factory.bind(factory, item);
        assert.equal(item.child.accessibleProperties.get(mockGtk.AccessibleProperty.DESCRIPTION), choices[1].label);
        assertNoTooltip(item.child);
    });

    test(`${kind} toggle synchronizes expansion, chevron and native accessibility state without selecting`, () => {
        const h = viewHarness(kind), { view } = h;
        h.render({ choices });
        const check = expanded => {
            assert.equal(view.devices.active, expanded);
            assert.equal(view.deviceChoices.reveal_child, expanded);
            assert.equal(view.deviceArrow.label, expanded ? '⌃' : '⌄');
            assert.equal(view.devices.accessibleStates.get(mockGtk.AccessibleState.EXPANDED), expanded);
        };
        check(false);
        view.devices.click();
        check(true);
        h.render({ choices, volume: 0.6, muted: true });
        check(true);
        view.devices.click();
        check(false);
        h.panel.setDeviceExpanded(view, true);
        check(true);
        h.panel.setDeviceExpanded(view, false);
        check(false);
        assert.deepEqual(h.selections, []);
    });

    test(`${kind} expansion is gated and collapses when hidden, busy, closed or on the inactive tab`, () => {
        for (const [name, configure] of [
            ['hidden', h => { h.view.devices.visible = false; }],
            ['insensitive', h => { h.view.devices.sensitive = false; }],
            ['closed', h => { h.panel.opened = false; }],
            ['inactive tab', h => { h.panel.currentTab = 0; }],
        ]) {
            const h = viewHarness(kind);
            h.render({ choices });
            configure(h);
            h.panel.setDeviceExpanded(h.view, true);
            assert.equal(h.view.deviceChoices.reveal_child, false, name);
            assert.equal(h.view.devices.active, false, name);
            assert.equal(h.view.deviceArrow.label, '⌄', name);
            assert.equal(h.view.devices.accessibleStates.get(mockGtk.AccessibleState.EXPANDED), false, name);
        }
        for (const [name, update] of [
            ['no alternatives', h => h.render({ choices: [choices[0]] })],
            ['busy', h => h.render({ choices, busy: true })],
            ['closed panel', h => { h.panel.opened = false; h.panel.render(); }],
            ['inactive tab', h => { h.panel.currentTab = 0; h.panel.render(); }],
        ]) {
            const h = viewHarness(kind);
            h.render({ choices });
            h.view.devices.click();
            assert.equal(h.view.deviceChoices.reveal_child, true);
            update(h);
            assert.equal(h.view.deviceChoices.reveal_child, false, name);
            assert.equal(h.view.devices.active, false, name);
            assert.equal(h.view.deviceArrow.label, '⌄', name);
            assert.equal(h.view.devices.accessibleStates.get(mockGtk.AccessibleState.EXPANDED), false, name);
            assert.deepEqual(h.selections, [], name);
        }
    });

    test(`${kind} native selection notifications never write; only row activation uses the activated position`, () => {
        const h = viewHarness(kind);
        h.render({ choices });
        h.view.deviceSelection.selected = 1;
        h.view.deviceSelection.selected = INVALID_LIST_POSITION;
        h.view.deviceSelection.set_model(stringListDouble.new(choices.map(choice => choice.label)));
        assert.deepEqual(h.selections, [], 'model changes and keyboard selection are read-only');
        h.view.deviceSelection.selected = 0;
        h.select(1);
        assert.deepEqual(h.selections, [[kind, choices[1].key]], 'activate position wins over the selected index');
        h.view.deviceSelection.selected = 1;
        h.panel.selectAudioDevice(h.view);
        assert.deepEqual(h.selections, [[kind, choices[1].key], [kind, choices[1].key]],
            'direct callers may omit the position and use the current native selection');
    });

    test(`${kind} activating a valid row collapses choices and focuses the trigger, including the current default`, () => {
        const h = viewHarness(kind);
        h.render({ choices });
        for (const position of [0, 1]) {
            h.view.devices.click();
            assert.equal(h.view.deviceChoices.reveal_child, true);
            const before = h.view.devices.focusCalls;
            h.select(position);
            assert.equal(h.view.deviceChoices.reveal_child, false);
            assert.equal(h.view.devices.active, false);
            assert.equal(h.view.deviceArrow.label, '⌄');
            assert.equal(h.view.devices.accessibleStates.get(mockGtk.AccessibleState.EXPANDED), false);
            assert.equal(h.view.devices.focusCalls, before + 1);
            assert.equal(h.view.deviceSelection.selected, INVALID_LIST_POSITION, 'activation clears the closed list selection');
            assert.equal(h.view.deviceLabel.label, choices[0].label);
        }
        assert.deepEqual(h.selections, [[kind, choices[1].key]]);
    });

    test(`${kind} renderer hides the complete dropdown without an available alternative`, () => {
        const h = viewHarness(kind);
        h.render({ choices: [] });
        assert.equal(h.view.devicesHeading.visible, false);
        assert.equal(h.view.devices.visible, false);
        assert.equal(h.view.devices.sensitive, false);
        h.render({ choices: [choices[0]] });
        assert.equal(h.view.devicesHeading.visible, false);
        assert.equal(h.view.devices.visible, false);
        assert.equal(h.view.devices.sensitive, false);
        h.render({ choices });
        assert.equal(h.view.devicesHeading.visible, true);
        assert.equal(h.view.devices.visible, true);
        assert.equal(h.view.devices.sensitive, true);
        assert.equal(h.view.deviceSelection.model.get_n_items(), 2);
        h.render({ choices: [choices[0]] });
        assert.equal(h.view.devicesHeading.visible, false);
        assert.equal(h.view.devices.visible, false);
        assert.equal(h.view.deviceSelection.model.get_n_items(), 1);
        assert.deepEqual(h.selections, []);
    });

    test(`${kind} dropdown selects the real key despite duplicate literal labels`, () => {
        const h = viewHarness(kind);
        h.render({ choices });
        assert.deepEqual(h.labels(), choices.map(choice => choice.label));
        assert.deepEqual(Array.from(h.view.choiceItems, item => item.key), choices.map(choice => choice.key));
        assert.equal(h.view.deviceSelection.selected, INVALID_LIST_POSITION);
        assert.equal(h.view.devices.accessibleProperties.get(mockGtk.AccessibleProperty.DESCRIPTION), choices[0].description);
        h.select(0);
        assert.deepEqual(h.selections, [], 'selecting the native default is not a mutation');
        h.select(1);
        assert.deepEqual(h.selections, [[kind, choices[1].key]]);
        assert.equal(h.state[kind].choices, choices, 'GTK selection must not rewrite native state');
        assert.equal(h.view.deviceSelection.selected, INVALID_LIST_POSITION, 'activation never highlights the native default');
        assert.equal(h.view.deviceLabel.label, choices[0].label, 'the trigger still follows the native default');
    });

    test(`${kind} dropdown label follows native default changes and reordered keys without selecting a row or writing`, () => {
        const h = viewHarness(kind), notifications = [], activations = [];
        h.view.deviceSelection.connect('notify::selected', () => notifications.push(h.view.deviceSelection.selected));
        h.view.deviceList.connect('activate', (_, position) => activations.push(position));
        const entries = [choices[1], choices[0]];
        h.render({ choices: entries });
        assert.equal(h.view.deviceSelection.selected, INVALID_LIST_POSITION, 'model replacement selects neither the first item nor the native default');
        const model = h.view.deviceSelection.model;
        const switched = entries.map(choice => ({ ...choice, default: !choice.default }));
        h.render({ choices: switched });
        assert.equal(h.view.deviceSelection.model, model, 'changing only the native default keeps the string model');
        assert.equal(h.view.deviceSelection.selected, INVALID_LIST_POSITION);
        assert.equal(h.view.devices.accessibleProperties.get(mockGtk.AccessibleProperty.DESCRIPTION), choices[1].description);
        h.render({ choices: [switched[1], switched[0]] });
        assert.notEqual(h.view.deviceSelection.model, model, 'key order matters even when both labels are identical');
        assert.deepEqual(Array.from(h.view.choiceItems, item => item.key), [choices[0].key, choices[1].key]);
        assert.equal(h.view.deviceSelection.selected, INVALID_LIST_POSITION);
        assert.ok(h.view.deviceSelection.modelWrites.length >= 2);
        assert.ok(h.view.deviceSelection.selectionWrites.length >= 2);
        assert.ok(notifications.length >= 4, 'model replacement and unselection emit notifications');
        assert.ok(notifications.every(position => position === INVALID_LIST_POSITION), 'render never selects a row');
        assert.deepEqual(activations, [], 'native model/selection notifications never activate a row');
        assert.equal(Object.hasOwn(h.view, 'updatingChoices'), false);
        assert.deepEqual(h.selections, []);
        h.select(0);
        assert.deepEqual(h.selections, [[kind, choices[0].key]], 'the reordered index resolves to its stored key');
    });

    test(`${kind} open list preserves user navigation through rerenders but clears selection on replacement or collapse`, () => {
        const h = viewHarness(kind), { view, panel } = h;
        h.render({ choices });
        view.devices.click();
        assert.equal(view.deviceSelection.selected, INVALID_LIST_POSITION, 'opening does not select the native default');
        view.deviceSelection.selected = 1;
        h.render({ choices, volume: 0.6, muted: true });
        assert.equal(view.deviceSelection.selected, 1, 'unrelated refreshes preserve user keyboard selection');
        assert.equal(view.deviceLabel.label, choices[0].label);
        assert.equal(view.devices.accessibleProperties.get(mockGtk.AccessibleProperty.DESCRIPTION), choices[0].description,
            'user navigation does not change the native-default description');
        const switched = choices.map(choice => ({ ...choice, default: !choice.default }));
        view.deviceSelection.selected = 0;
        h.render({ choices: switched });
        assert.equal(view.deviceSelection.selected, 0, 'external defaults do not move the user selection');
        assert.equal(view.devices.accessibleProperties.get(mockGtk.AccessibleProperty.DESCRIPTION), choices[1].description);
        h.render({ choices: [switched[1], switched[0]] });
        assert.equal(view.deviceChoices.reveal_child, true);
        assert.equal(view.deviceSelection.selected, INVALID_LIST_POSITION, 'replacement rows do not inherit an unrelated index');
        view.deviceSelection.selected = 1;
        view.devices.click();
        assert.equal(view.deviceSelection.selected, INVALID_LIST_POSITION, 'closing clears the user selection');
        view.devices.click();
        assert.equal(view.deviceSelection.selected, INVALID_LIST_POSITION, 'reopening does not restore an old highlight');
        view.deviceSelection.selected = 1;
        panel.resetInteractions();
        assert.equal(view.deviceSelection.selected, INVALID_LIST_POSITION, 'interaction reset clears the user selection');
        assert.deepEqual(h.selections, [], 'selection navigation alone never writes native state');
    });

    test(`${kind} dropdown does not rebuild its string model for volume, mute or busy updates`, () => {
        const h = viewHarness(kind);
        h.render({ choices });
        const model = h.view.deviceSelection.model;
        h.render({ choices: choices.map(choice => ({ ...choice })), volume: 0.6, muted: true });
        assert.equal(h.view.deviceSelection.model, model);
        assert.equal(h.view.deviceSelection.modelWrites.length, 1);
        assert.equal(h.view.volume.get_layout().get_text(), '60%');
        h.render({ choices, volume: 0, busy: true });
        assert.equal(h.view.deviceSelection.model, model);
        assert.equal(h.view.deviceSelection.modelWrites.length, 1);
        assert.equal(h.view.devices.sensitive, false);
        assert.equal(h.view.volume.sensitive, true, 'selection busy state does not disable native volume');
        assert.equal(h.view.mute.sensitive, true);
        assert.deepEqual(h.selections, []);
    });

    test(`${kind} dropdown signature includes keys, labels and descriptions`, () => {
        const h = viewHarness(kind);
        let entries = choices;
        h.render({ choices: entries });
        for (const [field, value] of [['description', 'Updated accessible description'],
            ['label', '<b>Updated literal name & text</b>'], ['key', `native:${kind}:replacement`]]) {
            const previous = h.view.deviceSelection.model;
            entries = entries.map((choice, index) => index === 0 ? { ...choice, [field]: value } : choice);
            h.render({ choices: entries });
            assert.notEqual(h.view.deviceSelection.model, previous, `${field} change must replace the model`);
            assert.equal(h.view.deviceSelection.model.get_string(0), entries[0].label);
            assert.equal(h.view.deviceLabel.label, entries[0].label);
            assert.equal(h.view.choiceItems[0][field], value);
            assert.equal(h.view.devices.accessibleProperties.get(mockGtk.AccessibleProperty.DESCRIPTION), entries[0].description);
        }
        h.render({ choices: entries.map(choice => ({ ...choice, description: '' })) });
        assert.equal(h.view.devices.accessibleProperties.get(mockGtk.AccessibleProperty.DESCRIPTION), entries[0].label,
            'empty descriptions fall back to literal accessible labels');
        assert.deepEqual(h.selections, []);
    });

    test(`${kind} dropdown handles hotplug removal and an unavailable default without selecting another device`, () => {
        const h = viewHarness(kind);
        const third = { key: `native:${kind}:30:three`, label: 'Dock', description: 'Dock route', default: false };
        h.render({ choices: [...choices, third] });
        h.render({ choices: [{ ...choices[1], default: true }, third] });
        assert.deepEqual(Array.from(h.view.choiceItems, item => item.key), [choices[1].key, third.key]);
        assert.equal(h.view.deviceSelection.selected, INVALID_LIST_POSITION);
        assert.equal(h.view.devices.accessibleProperties.get(mockGtk.AccessibleProperty.DESCRIPTION), choices[1].description);
        assert.deepEqual(h.selections, []);
        h.select(1);
        assert.deepEqual(h.selections, [[kind, third.key]]);
        h.render({ choices: [{ ...choices[1], default: true }] });
        assert.equal(h.view.devices.visible, false);
        assert.equal(h.view.devices.sensitive, false);
        h.render({ choices: [], available: false, nodeKey: '' });
        assert.equal(h.view.devicesHeading.visible, false);
        assert.equal(h.view.devices.visible, false);
        assert.equal(h.view.devices.sensitive, false);
        assert.ok(h.view.choiceItems.every(item => !item.key));
        assert.deepEqual(h.selections, [[kind, third.key]], 'hotplug renders never choose a replacement');
    });

    test(`${kind} dropdown prepends a non-actionable placeholder when no choice is the native default`, () => {
        const h = viewHarness(kind);
        const entries = choices.map(choice => ({ ...choice, default: false }));
        for (const [model, placeholder] of [
            [{ name: 'Unlisted native <device> & name' }, 'Unlisted native <device> & name'],
            [{ available: false, nodeKey: '', name: 'Stale native name' }, 'Select a device…'],
            [{ name: '' }, 'Select a device…'],
        ]) {
            h.render({ choices: entries, ...model });
            assert.deepEqual(h.labels(), [placeholder, ...entries.map(choice => choice.label)]);
            assert.equal(h.view.choiceItems[0].key, '');
            assert.equal(h.view.deviceSelection.selected, INVALID_LIST_POSITION, 'even the placeholder is not preselected');
            assert.equal(h.view.deviceLabel.label, placeholder);
            assert.equal(h.view.devices.visible, true);
            assert.equal(h.view.devices.sensitive, true);
            const before = h.selections.length;
            h.select(0);
            assert.equal(h.selections.length, before, 'the placeholder must never reach backend.select');
            h.select(1);
            assert.deepEqual(h.selections.at(-1), [kind, choices[0].key]);
            assert.equal(h.selections.length, before + 1, 'the first real choice remains selectable without a default');
        }
    });

    test(`${kind} dropdown blocks native writes for busy, hidden, inactive and invalid selection events`, () => {
        const cases = [
            ['rendered busy', h => h.render({ choices, busy: true })],
            ['live busy before rerender', h => { h.state[kind] = { ...h.state[kind], busy: true }; }],
            ['missing live model', h => { delete h.state[kind]; }],
            ['insensitive widget', h => { h.view.devices.sensitive = false; }],
            ['hidden widget', h => { h.view.devices.visible = false; }],
            ['Bluetooth tab', h => { h.panel.currentTab = 0; }],
            ['closed panel', h => { h.panel.opened = false; }],
            ['GTK invalid position', () => {}, INVALID_LIST_POSITION],
            ['out-of-range position', () => {}, choices.length],
            ['negative position', () => {}, -1],
        ];
        for (const [name, configure, index = 1] of cases) {
            const h = viewHarness(kind);
            h.render({ choices });
            configure(h);
            h.select(index);
            assert.deepEqual(h.selections, [], name);
            assert.equal(h.view.error.visible, false, `${name} must be ignored rather than reported as a failure`);
        }
    });

    test(`${kind} dropdown revalidates stale keys against the live snapshot instead of using labels or indexes`, () => {
        const h = viewHarness(kind);
        h.render({ choices });
        const replacement = { ...choices[1], key: `native:${kind}:replugged` };
        h.state[kind] = { ...h.state[kind], choices: [choices[0], replacement] };
        h.select(1);
        assert.deepEqual(h.selections, [], 'a new key at the same index with the same label is not the selected device');
        assert.ok(h.panel.failures[kind]);
        assert.equal(h.view.error.visible, true);
        assert.equal(h.view.error.text, h.panel.failures[kind]);
        assert.equal(h.view.choiceItems[1].key, replacement.key, 'the failure rerender picks up the current snapshot');
        h.select(1);
        assert.deepEqual(h.selections, [[kind, replacement.key]]);
        assert.equal(h.view.error.visible, false);
    });

    test(`${kind} dropdown checks the live native default rather than the rendered default flag`, () => {
        const h = viewHarness(kind);
        h.render({ choices });
        h.state[kind] = { ...h.state[kind], choices: choices.map(choice => ({ ...choice, default: !choice.default })) };
        h.select(1);
        assert.deepEqual(h.selections, [], 'an externally changed native default needs no redundant selection');
        h.select(0);
        assert.deepEqual(h.selections, [[kind, choices[0].key]], 'a formerly default choice is selectable by its key');
    });

    test(`${kind} dropdown surfaces backend selection errors through the existing action path`, () => {
        const h = viewHarness(kind);
        h.render({ choices });
        h.panel.backend.select = (...args) => {
            h.selections.push(args);
            throw new Error('Native device selection failed');
        };
        h.select(1);
        assert.deepEqual(h.selections, [[kind, choices[1].key]]);
        assert.equal(h.panel.failures[kind], 'Native device selection failed');
        assert.equal(h.view.error.text, 'Native device selection failed');
        assert.equal(h.view.error.visible, true);
        h.panel.backend.select = (...args) => h.selections.push(args);
        h.select(1);
        assert.equal(h.panel.failures[kind], '');
        assert.equal(h.view.error.visible, false);
        h.render({ choices, error: 'Native route acknowledgement failed' });
        assert.equal(h.view.error.text, 'Native route acknowledgement failed');
        assert.equal(h.view.error.visible, true);
    });

    test(`${kind} audio renderer distinguishes unavailable defaults from a real adjustable zero volume`, () => {
        const h = viewHarness(kind);
        h.render({ available: false, nodeKey: '', volume: 0 });
        assert.equal(h.view.volume.get_layout().get_text(), '—');
        assert.equal(h.view.volume.sensitive, false);
        assert.equal(h.view.volumeBar.widget.sensitive, false);
        assertNoInheritedTooltip(h.view.volume);
        assert.equal(h.view.volume.accessibleProperties.get(mockGtk.AccessibleProperty.DESCRIPTION),
            'No default audio device is available.');
        h.render({ volume: 0 });
        assert.equal(h.view.volume.get_layout().get_text(), '0%');
        assert.equal(h.view.volume.sensitive, true);
        assert.equal(h.view.volumeBar.widget.sensitive, true);
        assert.equal(h.view.mute.sensitive, true);
        assertNoInheritedTooltip(h.view.volume);
        assert.equal(h.view.volume.accessibleProperties.get(mockGtk.AccessibleProperty.DESCRIPTION), '');
        h.render({ volume: 0.4 });
        assert.equal(h.view.volume.get_layout().get_text(), '40%');
    });
}
}

// Tests consolidated from launcher.test.js.
{
const launcherPath = process.env.AUDIO_PANEL_LAUNCHER;
const panelLauncherPath = process.env.PANEL_LAUNCHER;

test('built audio launcher', {
    skip: launcherPath && panelLauncherPath ? false
        : 'Set AUDIO_PANEL_LAUNCHER to the built audio-panel wrapper and PANEL_LAUNCHER to the shared Rust executable; both are required.',
}, async t => {
    const { readdirSync, realpathSync } = await import('node:fs');
    const { dirname } = await import('node:path');
    const root = mkdtempSync(join(tmpdir(), 'audio-launcher-'));
    t.after(() => rmSync(root, { recursive: true, force: true }));

    function invoke(executable, args, env = process.env) {
        const result = spawnSync(executable, args, { env, encoding: 'utf8',
            timeout: 5000, maxBuffer: 1024 * 1024 });
        assert.ifError(result.error);
        assert.equal(result.signal, null);
        return result;
    }

    function checkUsage(run) {
        for (const args of [[], ['unknown'], ['run', 'extra'], ['status', 'extra'], ['close', 'extra'],
            ['bluetoothPower', 'extra'], ['bluetoothPower', ''], ['toggle', 'DP-1', 'extra'], ['--help', 'extra'], ['-h', 'extra']]) {
            const result = run(args);
            assert.equal(result.status, 2, JSON.stringify(args));
            assert.match(result.stderr, /Usage:/);
            assert.equal(result.stdout, '');
        }
        for (const command of ['--help', '-h']) {
            const result = run([command]);
            assert.equal(result.status, 0, result.stderr);
            assert.match(result.stdout, /^Usage: audio-panel/);
            assert.match(result.stdout, /bluetoothPower/);
        }
    }

    await t.test('installed wrapper validates command arity and help', () => {
        checkUsage(args => invoke(launcherPath, args));
    });

    const manifestPath = join(dirname(realpathSync(launcherPath)), '..', 'share', 'audio-panel', 'launcher.json');
    const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));
    assert.equal(manifest.name, 'audio-panel');
    assert.match(manifest.bundle, /^\/nix\/store\/[a-z0-9]{32}-[^/]+(?:\/|$)/, 'the prebuilt bundle must be immutable');
    assert.ok(statSync(manifest.bundle).isFile());
    assert.match(manifest.forced_environment.AUDIO_PANEL_CONFIG,
        /^\/nix\/store\/[^/]+\/(?:scripts\/)?audio-control\/astal\/frontend\.js$/,
        'the entry must come from the packaged source, never the checkout or XDG config');
    for (const key of ['AUDIO_PANEL_PALETTE', 'AUDIO_CONTROL_HELPER', 'PANEL_CSS_FILE']) {
        assert.equal(typeof manifest.environment[key], 'string', `${key} must have a manifest default`);
        assert.notEqual(manifest.environment[key], '');
    }
    const keys = [...new Set([...Object.keys(manifest.environment), ...Object.keys(manifest.forced_environment), 'XDG_RUNTIME_DIR'])];
    const fixtureManifest = { ...manifest, astal: join(root, 'astal'), bundle: join(root, 'bundle') };
    for (const [program, fake] of [['astal', fixtureManifest.astal], ['bundle', fixtureManifest.bundle]]) {
        writeFileSync(fake, `#!${process.execPath}\n`
            + `const program = ${JSON.stringify(program)}, keys = ${JSON.stringify(keys)};\n`
            + 'const argv = process.argv.slice(2);\n'
            + 'console.log(JSON.stringify({program, argv, env: Object.fromEntries(keys.map(key => [key, process.env[key]]))}));\n');
        chmodSync(fake, 0o700);
    }
    const configPath = join(root, 'launcher fixture.json');
    writeFileSync(configPath, JSON.stringify(fixtureManifest));
    const env = { ...process.env, XDG_CONFIG_HOME: join(root, 'config with spaces'),
        XDG_RUNTIME_DIR: join(root, 'runtime with spaces') };
    for (const key of keys.filter(key => key !== 'XDG_RUNTIME_DIR')) delete env[key];
    mkdirSync(env.XDG_RUNTIME_DIR, { mode: 0o700 });
    const sharedBundle = join(env.XDG_RUNTIME_DIR, 'ags.js');
    writeFileSync(sharedBundle, 'another panel owns this shared-path sentinel');

    function runtimeState(path) {
        const stat = statSync(path);
        return { mode: stat.mode, mtimeMs: stat.mtimeMs, ctimeMs: stat.ctimeMs,
            content: stat.isDirectory()
                ? readdirSync(path).sort().map(name => [name, runtimeState(join(path, name))])
                : readFileSync(path, 'utf8') };
    }

    function run(args, overrides = {}) {
        const childEnv = { ...env, ...overrides };
        const runtimeDir = childEnv.XDG_RUNTIME_DIR;
        const before = runtimeDir ? runtimeState(runtimeDir) : null;
        const result = invoke(panelLauncherPath, ['--config', configPath, ...args], childEnv);
        if (runtimeDir) assert.deepEqual(runtimeState(runtimeDir), before, 'the launcher must not write runtime files');
        return result;
    }

    function success(args, overrides) {
        const result = run(args, overrides);
        assert.equal(result.status, 0, result.stderr);
        return result.stdout.trim().split('\n').map(line => JSON.parse(line));
    }

    function request(args, overrides = {}) {
        const records = success(args, overrides);
        assert.equal(records.length, 1, 'requests must not compile or start GJS');
        const [data] = records;
        assert.equal(data.program, 'astal');
        assert.deepEqual(data.argv.slice(0, 2), ['--instance', 'audio-panel']);
        assert.equal(data.argv.length, 3);
        assert.equal(data.env.XDG_RUNTIME_DIR, overrides.XDG_RUNTIME_DIR ?? env.XDG_RUNTIME_DIR,
            'IPC uses the original runtime directory');
        assert.equal(data.env.AUDIO_PANEL_CONFIG, manifest.forced_environment.AUDIO_PANEL_CONFIG);
        return data;
    }

    function launch(overrides = {}) {
        const records = success(['run'], overrides);
        assert.equal(records.length, 1, 'start the immutable bundle exactly once');
        const [runtime] = records;
        assert.equal(runtime.program, 'bundle');
        assert.deepEqual(runtime.argv, []);
        assert.equal(runtime.env.XDG_RUNTIME_DIR, overrides.XDG_RUNTIME_DIR ?? env.XDG_RUNTIME_DIR,
            'the native socket namespace must not change');
        assert.equal(runtime.env.AUDIO_PANEL_CONFIG, manifest.forced_environment.AUDIO_PANEL_CONFIG);
        assert.equal(readFileSync(sharedBundle, 'utf8'), 'another panel owns this shared-path sentinel');
        return runtime;
    }

    await t.test('validates command arity and help without invoking AGS or GJS', () => {
        checkUsage(run);
    });

    await t.test('targets the audio instance with typed JSON requests', () => {
        for (const command of ['status', 'close', 'bluetoothPower', 'toggle']) {
            const data = request([command]);
            assert.deepEqual(JSON.parse(data.argv[4]), { command });
        }
    });

    await t.test('escapes monitor arguments without interpreting shell syntax', () => {
        const marker = join(root, 'monitor-was-evaluated');
        for (const monitor of ['', 'eDP-1', 'Display with spaces', 'Display "quoted" \\ with\nnewlines\tand tabs',
            `DP-'"; touch ${marker}; $(touch ${marker}); 🎧`]) {
            const data = request(['toggle', monitor]);
            assert.deepEqual(JSON.parse(data.argv[4]), { command: 'toggle', ...(monitor ? { monitor } : {}) });
        }
        assert.equal(existsSync(marker), false);
        assert.equal(existsSync(join(env.XDG_RUNTIME_DIR, 'ags-panels')), false, 'requests do not create bundles');
    });

    await t.test('runs the prebuilt bundle with store-backed defaults and live CSS', () => {
        const runtime = launch();
        assert.deepEqual(runtime.env, { ...manifest.environment, ...manifest.forced_environment,
            XDG_RUNTIME_DIR: env.XDG_RUNTIME_DIR });
        assert.match(runtime.env.AUDIO_PANEL_PALETTE, /^\/nix\/store\//);
        assert.match(runtime.env.AUDIO_CONTROL_HELPER, /\/bin\/audio-control$/);
        assert.match(runtime.env.PANEL_CSS_FILE, /\/scripts\/audio-control\/astal\/style\.css$/);
        assert.doesNotMatch(runtime.env.PANEL_CSS_FILE, /^\/nix\/store\//);
    });

    await t.test('missing and empty environment variables use manifest defaults for runs and requests', () => {
        const empty = Object.fromEntries(Object.keys(manifest.environment).map(key => [key, '']));
        for (const overrides of [{}, { ...empty, AUDIO_PANEL_CONFIG: '' }]) {
            for (const data of [launch(overrides), request(['status'], overrides)]) {
                assert.deepEqual(data.env, { ...manifest.environment, ...manifest.forced_environment,
                    XDG_RUNTIME_DIR: env.XDG_RUNTIME_DIR });
            }
        }
    });

    await t.test('ignores live-source overrides but preserves literal palette, helper and CSS overrides', () => {
        const marker = join(root, 'config-was-evaluated');
        const overrides = { AUDIO_PANEL_CONFIG: join(root, `custom '"; $(touch ${marker}); frontend.js`),
            AUDIO_PANEL_PALETTE: join(root, 'palette with spaces.json'), AUDIO_CONTROL_HELPER: join(root, 'custom helper'),
            PANEL_CSS_FILE: join(root, 'custom style with spaces.css'),
            XDG_RUNTIME_DIR: join(root, 'isolated runtime $HOME with spaces') };
        mkdirSync(overrides.XDG_RUNTIME_DIR, { mode: 0o700 });
        for (const data of [launch(overrides), request(['status'], overrides)]) {
            assert.notEqual(data.env.AUDIO_PANEL_CONFIG, overrides.AUDIO_PANEL_CONFIG);
            assert.deepEqual(data.env, { ...manifest.environment, ...overrides, ...manifest.forced_environment });
        }
        assert.equal(existsSync(marker), false);
        assert.equal(existsSync(join(overrides.XDG_RUNTIME_DIR, 'ags.js')), false);
    });

    await t.test('ignores stale runtime bundles without compiling or writing runtime files', () => {
        const runtimeDir = join(root, 'stale bundle runtime');
        const bundleDir = join(runtimeDir, 'ags-panels');
        mkdirSync(bundleDir, { recursive: true, mode: 0o700 });
        writeFileSync(join(bundleDir, 'audio-panel.js'), 'throw new Error("stale runtime bundle was executed");\n');
        writeFileSync(join(runtimeDir, 'ags.js'), 'another panel owns this stale shared bundle');
        launch({ XDG_RUNTIME_DIR: runtimeDir });
    });

    await t.test('a missing or empty runtime directory fails before any AGS or GJS invocation', () => {
        for (const XDG_RUNTIME_DIR of [undefined, '']) {
            const result = run(['run'], { XDG_RUNTIME_DIR });
            assert.notEqual(result.status, 0);
            assert.match(result.stderr, /XDG_RUNTIME_DIR/);
            assert.equal(result.stdout, '');
        }
    });
});
}
}

async function gjsNativeTests(pipesOnly = false) {
// Run with the audio package's AGS wrapper; no UI, discovery, or device writes.
const { default: GLib } = await import('gi://GLib');
const { default: GObject } = await import('gi://GObject');
const { AudioBackend, HelperProcess } = await import('./backend.js');

function assert(condition, message) { if (!condition) throw new Error(message); }
const node = GLib.find_program_in_path('node');
if (!node) throw new Error('Node.js is required for the pipe smoke test.');
const run = (code, onChunk = () => {}) => new Promise((resolve, reject) => {
    new HelperProcess([node, '-e', code], { onChunk,
        onExit: (error, output) => error ? reject(error) : resolve(output) });
});

async function smoke() {
    assert(await run('console.log("metadata")') === 'metadata\n', 'helper stdout');
    assert(await run('process.stderr.write("diagnostic")') === 'diagnostic', 'helper stderr');
    const chunks = [];
    const unicode = await run('const fs = require("node:fs"), data = Buffer.from("🎧"); '
        + 'fs.writeSync(1, data.subarray(0, 2)); setTimeout(() => fs.writeSync(1, data.subarray(2)), 50);',
        chunk => chunks.push(chunk));
    assert(unicode === '🎧' && chunks.join('') === unicode, 'UTF-8 split across reads');
    await new Promise((resolve, reject) => {
        let worker;
        worker = new HelperProcess([node, '-e', 'require("node:readline").createInterface({ input: process.stdin })'
            + '.once("line", line => { require("node:fs").writeSync(1, line.trim() + "\\n"); process.exit(0); }); console.log("PIN:");'], {
            onChunk: text => { if (text.includes('PIN:')) worker.send('042311\n'); },
            onExit: (error, output) => {
                if (error) reject(error);
                else { try { assert(output.includes('042311'), 'interactive answer'); resolve(); } catch (failure) { reject(failure); } }
            },
        });
    });
    const line = JSON.stringify({ text: '🎧'.repeat(200) }) + '\n';
    let streamedBytes = 0;
    const tail = await run(`const fs = require("node:fs"), line = ${JSON.stringify(line)}; `
        + 'for (let index = 0; index < 1400; index++) fs.writeSync(1, line);',
        text => { streamedBytes += new TextEncoder().encode(text).length; });
    assert(streamedBytes > 1024 * 1024 && tail === line.repeat(1400).slice(-8192), 'resident stream with a bounded diagnostic tail');
    let cancellations = 0;
    await new Promise((resolve, reject) => {
        let worker;
        worker = new HelperProcess([node, '-e', 'console.log("ready"); setInterval(() => {}, 1000);'], {
            onChunk: text => { if (text.includes('ready')) { worker.stop(); worker.stop(); } },
            onExit: error => {
                cancellations++;
                try { assert(error?.message.includes('cancelled'), 'resident cancellation'); resolve(); }
                catch (failure) { reject(failure); }
            },
        });
    });
    assert(cancellations === 1, 'resident cancellation completes once');
    for (const [code, expected] of [
        ['require("node:fs").writeSync(1, "bad request\\n"); process.exit(2);', 'bad request'],
        ['require("node:fs").writeFileSync(1, "x".repeat(1100000));', 'too large'],
    ]) {
        let error;
        try { await run(code); } catch (failure) { error = failure; }
        assert(error?.message.includes(expected), `expected helper failure: ${expected}`);
    }
    print('Native audio pipe smoke passed: UTF-8, resident streaming, bounded frames/tail, interactive input, cancellation and errors.');
    if (pipesOnly) return;
    const backend = new AudioBackend();
    try {
        backend.start();
        await new Promise(resolve => GLib.timeout_add(GLib.PRIORITY_DEFAULT, 1800, () => { resolve(); return GLib.SOURCE_REMOVE; }));
        const model = backend.snapshot();
        assert(typeof JSON.parse(backend.status()).tooltip === 'string', 'Waybar response');
        assert(!model.output.loading && !model.input.loading, 'metadata helpers completed');
        assert(!model.output.error && !model.input.error, `metadata: ${model.output.error || model.input.error}`);
        const adapter = backend.adapter;
        if (adapter) {
            const path = new GObject.Value();
            path.init(GObject.TYPE_STRING);
            adapter.get_property('object-path', path);
            const expected = backend.bluetooth.devices.filter(device => device.adapter === path.get_string());
            assert(expected.length === model.bluetooth.devices.length, 'native adapter device filtering');
        }
        for (const kind of ['output', 'input']) {
            if (!model[kind].available) continue;
            assert(model[kind].nodeKey && model[kind].choices.length, `${kind} native state`);
            assert(model[kind].choices.some(choice => choice.default), `${kind} live default selection`);
        }
        print('Native audio state smoke passed: Astal state and read-only status.');
    } finally { backend.dispose(); }
}

// A nested MainLoop inside this async mode would block promise continuations.
await smoke();
}

async function gjsDropdownTests() {
// Offline inline-selector/volume GTK4 integration smoke. Run with the audio launcher's AGS --gtk4
// wrapper, AUDIO_PANEL_CONFIG/PALETTE, and AUDIO_DROPDOWN_ISOLATED=1 in private
// Xvfb + service-free D-Bus (SYSTEM and SESSION addresses must be identical).
// Optional AUDIO_DROPDOWN_TEST_PYTHON / AUDIO_DROPDOWN_TEST_X11 select Python
// and libX11 for real XTEST mouse/keyboard input. No AudioBackend is imported.
const { default: GLib } = await import('gi://GLib');
const { default: Gio } = await import('gi://Gio');
const { default: Gtk } = await import('gi://Gtk?version=4.0');
const { default: Gdk } = await import('gi://Gdk?version=4.0');
const { default: GdkX11 } = await import('gi://GdkX11?version=4.0');
const { default: Pango } = await import('gi://Pango');
const { default: Astal } = await import('gi://Astal?version=4.0');
const { default: System } = await import('system');
const { loadPanelStyle } = await import('../../live-style.js');

if (GLib.getenv('AUDIO_DROPDOWN_ISOLATED') !== '1'
    || GLib.getenv('GDK_BACKEND') !== 'x11'
    || !GLib.getenv('DBUS_SESSION_BUS_ADDRESS')
    || GLib.getenv('DBUS_SYSTEM_BUS_ADDRESS') !== GLib.getenv('DBUS_SESSION_BUS_ADDRESS'))
    throw new Error('Use private Xvfb and service-free D-Bus, not the desktop session.');

let checks = 0;
function assert(condition, message) {
    checks++;
    if (!condition) throw new Error(message);
}
function equal(actual, expected, message) {
    assert(JSON.stringify(actual) === JSON.stringify(expected),
        `${message}: ${JSON.stringify(actual)} != ${JSON.stringify(expected)}`);
}
// Fractional CSS borders produce float bounds beside integer GTK allocations.
// Accept subpixel rounding only, never a full pixel of layout drift.
function geometryEqual(actual, expected, message) {
    assert(Math.abs(actual - expected) < 1, `${message}: ${actual} != ${expected} (tolerance <1px)`);
}
function boundsEqual(actual, expected, message) {
    for (const key of ['x', 'y', 'width', 'height', 'bottom'])
        geometryEqual(actual[key], expected[key], `${message}: ${key}`);
}
function wait(milliseconds = 100) {
    return new Promise(resolve => GLib.timeout_add(GLib.PRIORITY_DEFAULT, milliseconds,
        () => { resolve(); return GLib.SOURCE_REMOVE; }));
}
function read(path) {
    if (!path) throw new Error('AUDIO_PANEL_CONFIG and AUDIO_PANEL_PALETTE are required.');
    const [ok, bytes] = GLib.file_get_contents(path);
    if (!ok) throw new Error(`Cannot read ${path}`);
    return new TextDecoder().decode(bytes);
}
function walk(widget) {
    const result = [widget];
    for (let child = widget.get_first_child(); child; child = child.get_next_sibling())
        result.push(...walk(child));
    return result;
}
function noTooltip(widget, message) {
    assert(!widget.has_tooltip && !widget.tooltip_text && !widget.tooltip_markup, message);
}
function noInheritedTooltip(widget, message) {
    for (let current = widget; current; current = current.get_parent()) noTooltip(current, message);
}
function bounds(widget, relative = panel.card) {
    const [ok, rect] = widget.compute_bounds(relative);
    assert(ok && rect.get_width() > 0 && rect.get_height() > 0, 'Widget has real GTK bounds');
    return { x: rect.get_x(), y: rect.get_y(), width: rect.get_width(), height: rect.get_height(),
        bottom: rect.get_y() + rect.get_height() };
}
const accessibleStates = new WeakMap();
const updateAccessibleState = Gtk.ToggleButton.prototype.update_state;
function trackAccessibleState(states, values) {
    states.forEach((state, index) => {
        if (state === Gtk.AccessibleState.EXPANDED) {
            assert(typeof values[index] === 'number' && [0, 1].includes(values[index]),
                'Expanded accessibility states must marshal integer GValues, not booleans');
            accessibleStates.set(this, values[index]);
        }
    });
    return updateAccessibleState.call(this, states, values);
}
function expanded(view, value, message) {
    equal([view.devices.active, view.deviceChoices.reveal_child, view.deviceArrow.label],
        [value, value, value ? '⌃' : '⌄'], message);
    equal(accessibleStates.get(view.devices), value ? Gtk.AccessibleTristate.TRUE : Gtk.AccessibleTristate.FALSE,
        `${message}: real GTK receives numeric accessibility state`);
}
function focused(widget, message) {
    const focus = host.get_focus();
    assert(focus === widget || (focus && focus.is_ancestor(widget)), message);
}
function listLabels(view) {
    return walk(view.deviceList).filter(widget => widget instanceof Gtk.Label
        && widget.get_mapped() && view.choiceItems.some(item => item.label === widget.label));
}
async function scrollIntoView(widget, viewport) {
    const rect = bounds(widget, viewport), adjustment = viewport.get_vadjustment();
    if (rect.y < 0) adjustment.value += rect.y;
    else if (rect.bottom > viewport.get_height()) adjustment.value += rect.bottom - viewport.get_height();
    await wait();
}
function noDevicePopups() {
    assert(!walk(panel.root).some(widget => widget instanceof Gtk.DropDown || widget instanceof Gtk.Popover),
        'Audio device selectors contain no native dropdown or popup');
}
function owner(name) {
    return Gio.DBus.session.call_sync('org.freedesktop.DBus', '/org/freedesktop/DBus',
        'org.freedesktop.DBus', 'NameHasOwner', new GLib.Variant('(s)', [name]),
        new GLib.VariantType('(b)'), Gio.DBusCallFlags.NONE, 1000, null).deep_unpack()[0];
}

// Only evaluate the production helpers and AudioPanel, as the Node renderer
// tests do. The application/bootstrap below monitorFor must never execute.
const entry = GLib.getenv('AUDIO_PANEL_CONFIG');
const source = read(entry);
const boundary = source.indexOf('\nfunction monitorFor(');
assert(boundary > source.indexOf('export class AudioPanel'), 'Production extraction boundary exists');
const extracted = source.slice(0, boundary)
    .replace(/^import .*;[ \t]*$/gm, '')
    .replace('export class AudioPanel', 'class AudioPanel');
assert(!/^\s*(?:import|export)\b/m.test(extracted), 'All imports/exports stripped before evaluation');
const { AudioPanel, loadTheme } = new Function('Gtk', 'Gdk', 'Pango', 'Astal', 'GLib', 'loadPanelStyle',
    `${extracted}\nreturn { AudioPanel, loadTheme };`)(Gtk, Gdk, Pango, Astal, GLib, loadPanelStyle);

// XTEST uses the existing libX11 request allocator, so no xdotool, python-xlib,
// libXtst, shell, or extra on-disk input harness is needed. All input is sent
// exclusively to the authenticated private DISPLAY checked above.
const inputCode = `
import ctypes as C, ctypes.util, os, struct, sys
assert os.environ['AUDIO_DROPDOWN_ISOLATED'] == '1'
x = C.CDLL(os.environ.get('AUDIO_DROPDOWN_TEST_X11') or ctypes.util.find_library('X11'))
def api(name, result, *args):
    f = getattr(x, name); f.restype = result; f.argtypes = args; return f
P, I, U, L = C.c_void_p, C.c_int, C.c_uint, C.c_ulong
open_display = api('XOpenDisplay', P, C.c_char_p)
d = open_display(None)
assert d, 'Cannot open private Xvfb'
try:
    root = api('XDefaultRootWindow', L, P)(d)
    opcode, event, error = I(), I(), I()
    assert api('XQueryExtension', I, P, C.c_char_p, C.POINTER(I), C.POINTER(I), C.POINTER(I))(
        d, b'XTEST', C.byref(opcode), C.byref(event), C.byref(error)), 'XTEST unavailable'
    request = api('_XGetRequest', P, P, C.c_ubyte, C.c_size_t)
    def send(kind, detail=0, px=0, py=0):
        data = struct.pack('=BBHBBHII8xhh8x', opcode.value, 2, 9, kind, detail, 0, 0, root, px, py)
        C.memmove(request(d, opcode.value, len(data)), data, len(data))
    mode, xid = sys.argv[1], int(sys.argv[2])
    if mode == 'key':
        api('XSetInputFocus', I, P, L, I, L)(d, xid, 2, 0)
        symbol = api('XStringToKeysym', L, C.c_char_p)(sys.argv[3].encode())
        code = api('XKeysymToKeycode', C.c_ubyte, P, L)(d, symbol)
        assert code, 'Missing X11 keycode'
        send(2, code); send(3, code)
    elif mode == 'pixels':
        import hashlib
        left, top, width, height = map(int, sys.argv[3:7])
        image = api('XGetImage', P, P, L, I, I, U, U, L, I)(d, xid, left, top, width, height, L(-1).value, 2)
        assert image, 'Cannot read private widget pixels'
        try:
            pixel = api('XGetPixel', L, P, I, I)
            data = b''.join(pixel(image, col, row).to_bytes(8, 'little') for row in range(height) for col in range(width))
            print(hashlib.sha256(data).hexdigest())
        finally:
            api('XDestroyImage', I, P)(image)
    else:
        px, py, child = I(), I(), L()
        assert api('XTranslateCoordinates', I, P, L, L, I, I, C.POINTER(I), C.POINTER(I), C.POINTER(L))(
            d, xid, root, int(sys.argv[3]), int(sys.argv[4]), C.byref(px), C.byref(py), C.byref(child))
        send(6, px=px.value, py=py.value)
        if mode == 'click': send(4, 1); send(5, 1)
        elif mode == 'press': send(4, 1)
        elif mode == 'release': send(5, 1)
        elif mode == 'scroll': send(4, 4); send(5, 4)
    api('XSync', I, P, I)(d, 0)
finally:
    api('XCloseDisplay', I, P)(d)
`;
function input(mode, widget, key = '') {
    const native = widget.get_native();
    const surface = native.get_surface();
    assert(surface instanceof GdkX11.X11Surface, 'Input targets a private real X11 surface');
    const args = [mode, String(surface.get_xid())];
    if (mode === 'key') args.push(key);
    else {
        const [ok, rect] = widget.compute_bounds(native);
        assert(ok && rect.get_width() > 0 && rect.get_height() > 0, 'Input widget is allocated');
        const [sx, sy] = native.get_surface_transform();
        if (mode === 'pixels') args.push(String(Math.round(rect.get_x() + sx)),
            String(Math.round(rect.get_y() + sy)), String(Math.floor(rect.get_width())), String(Math.floor(rect.get_height())));
        else args.push(String(Math.round(rect.get_x() + rect.get_width() / 2 + sx)),
            String(Math.round(rect.get_y() + rect.get_height() / 2 + sy)));
    }
    const process = Gio.Subprocess.new([GLib.getenv('AUDIO_DROPDOWN_TEST_PYTHON')
        || GLib.find_program_in_path('python3'), '-c', inputCode, ...args],
    Gio.SubprocessFlags.STDOUT_PIPE | Gio.SubprocessFlags.STDERR_PIPE);
    const [, stdout, stderr] = process.communicate_utf8(null, null);
    assert(process.get_successful(), `Private XTEST input: ${stdout}${stderr}`);
    return stdout.trim();
}

function choices(kind) {
    const duplicate = `<b>${kind} USB & 🎧</b> ` + 'very long literal device name '.repeat(24);
    return [
        { key: `native:${kind}:a`, label: duplicate, description: `<i>${kind} route A & literal</i>`, default: false },
        { key: `policy:${kind}:b`, label: duplicate, description: `<i>${kind} route B & literal</i>`, default: true },
        { key: `native:${kind}:c`, label: `${kind} Dock`, description: '', default: false },
    ];
}
class FakeBackend {
    constructor() {
        this.calls = [];
        this.activity = [];
        this.starts = 0;
        this.state = { bluetooth: { available: false, powered: false, devices: [] } };
        for (const kind of ['output', 'input']) this.state[kind] = {
            available: true, nodeKey: `current:${kind}`, name: `<b>${kind} native default</b>`,
            volume: 0.25, muted: false, busy: false, choices: choices(kind),
        };
    }
    start() { this.starts++; throw new Error('A fake-only UI smoke must never start a backend'); }
    snapshot() { return this.state; }
    select(...args) {
        this.calls.push(['select', ...args]);
        if (this.selectionError) throw new Error(this.selectionError);
    }
    setVolume(kind, value, key) {
        this.calls.push(['volume', kind, value, key]);
        this.state[kind] = { ...this.state[kind], volume: value };
    }
    toggleMute(kind, key) {
        this.calls.push(['mute', kind, key]);
        this.state[kind] = { ...this.state[kind], muted: !this.state[kind].muted };
    }
    setActive(...args) { this.activity.push(args); }
}

let panel, host, app;
const backend = new FakeBackend();
function allocation(message) {
    equal(panel.card.get_width(), 400, `${message}: constrained width`);
    const [, naturalHeight] = panel.content.measure(Gtk.Orientation.VERTICAL, 400);
    equal(panel.card.get_height(), naturalHeight, `${message}: content-sized height`);
    assert(panel.card.get_height() <= 520, `${message}: height stays within the monitor budget`);
    equal([host.get_width(), host.get_height()], [1280, 800], 'Long names do not enlarge the host');
}
async function testCompactLayout() {
    const before = backend.calls.length, original = backend.state.output;
    const monitor = panel.monitor;
    for (const view of Object.values(panel.audio)) expanded(view, false, 'Compact selectors start collapsed');
    const rect = widget => {
        const [ok, value] = widget.compute_bounds(panel.card);
        assert(ok, 'Compact layout has native widget bounds');
        return value;
    };
    const compactHeight = panel.card.get_height();
    assert(compactHeight < 520, 'Audio card shrinks below the old fixed height');
    const sections = walk(panel.audioPage).filter(widget => widget.has_css_class('audio-section'));
    equal(sections.length, 2, 'One audio viewport contains both natural-height sections');
    for (const [index, kind] of ['output', 'input'].entries()) {
        const section = rect(sections[index]), trigger = rect(panel.audio[kind].devices);
        geometryEqual(section.get_y() + section.get_height(), trigger.get_y() + trigger.get_height(),
            `${kind}: no expanding gap below the collapsed selector`);
    }
    const last = rect(panel.audio.input.devices);
    const padding = panel.content.get_style_context().get_padding();
    geometryEqual(panel.card.get_height() - last.get_y() - last.get_height(), padding.bottom + panel.pages.margin_bottom,
        'Only normal panel padding remains below the collapsed input selector');
    backend.state.output = { ...original, choices: original.choices.filter(item => item.default) };
    panel.render();
    await wait();
    assert(panel.card.get_height() < compactHeight, 'Hiding an unnecessary selector also shrinks the card');
    backend.state.output = { ...original, error: 'A wrapped device error with useful details. '.repeat(60) };
    panel.render();
    await wait();
    assert(panel.card.get_height() <= 520, 'Wrapped errors cannot enlarge the card beyond the budget');
    let adjustment = panel.audioPage.get_vadjustment();
    assert(adjustment.upper > adjustment.page_size, 'Long errors scroll within the single audio viewport');
    backend.state.output = original;
    panel.render();
    await wait();
    equal(panel.card.get_height(), compactHeight, 'Clearing errors restores the compact height');
    panel.setTab(0);
    await wait();
    assert(panel.card.get_height() < 520, 'Bluetooth shrinks to its visible rows instead of reserving the old fixed height');
    panel.setTab(1);
    await wait();
    equal(panel.card.get_height(), compactHeight, 'Returning to Audio restores content-sized height');
    panel.monitor = { get_geometry: () => ({ width: 340, height: 300 }) };
    panel.render();
    await wait();
    equal(panel.card.get_width(), 324, 'Short/narrow monitor preserves horizontal clearance');
    assert(panel.card.get_height() <= 236, 'Short monitor preserves the existing vertical clearance');
    adjustment = panel.audioPage.get_vadjustment();
    assert(adjustment.upper > adjustment.page_size, 'Short monitor scrolls the compact audio page');
    adjustment.value = adjustment.upper - adjustment.page_size;
    await wait();
    const [ok, inputBounds] = panel.audio.input.devices.compute_bounds(panel.audioPage);
    assert(ok && inputBounds.get_y() >= 0
        && inputBounds.get_y() + inputBounds.get_height() <= panel.audioPage.get_height(),
    'Input selector remains reachable at the bottom of a short monitor');
    panel.monitor = monitor;
    panel.render();
    await wait();
    equal(panel.card.get_height(), compactHeight, 'Restoring monitor geometry restores compact height');
    equal(backend.calls.length, before, 'Layout checks never mutate audio devices');
    print(`LAYOUT compact audio card=400x${compactHeight}; collapsed selector gaps removed; short-monitor scrolling verified`);
}
async function testKind(kind) {
    const view = panel.audio[kind], trigger = view.devices, selection = view.deviceSelection;
    const initial = choices(kind);
    const update = properties => {
        backend.state[kind] = { ...backend.state[kind], ...properties };
        panel.render();
    };
    assert(trigger instanceof Gtk.ToggleButton && trigger.has_css_class('device-dropdown'),
        `${kind}: selector trigger is the native toggle itself`);
    assert(trigger.get_child() instanceof Gtk.Box && view.deviceLabel instanceof Gtk.Label
        && view.deviceArrow instanceof Gtk.Label, `${kind}: trigger owns a label and arrow box`);
    assert(view.deviceLabel.get_parent() === trigger.get_child()
        && view.deviceArrow.get_parent() === trigger.get_child(), `${kind}: both labels belong to the trigger box`);
    assert(selection instanceof Gtk.SingleSelection && !selection.autoselect && selection.can_unselect,
        `${kind}: native single selection does not autoselect`);
    assert(view.deviceList instanceof Gtk.ListView && view.deviceList.model === selection
        && view.deviceList.single_click_activate && view.deviceList.has_css_class('device-list'),
        `${kind}: native list activates on one click`);
    const factory = view.deviceList.factory, bindings = [];
    assert(factory instanceof Gtk.SignalListItemFactory, `${kind}: native row factory`);
    const bindSignal = factory.connect('bind', (_, item) => {
        const widget = item.get_child(), choice = view.choiceItems[item.get_position()];
        const ancestors = [];
        for (let current = widget; current; current = current.get_parent()) ancestors.push(current);
        // Record inside GTK signals; assert outside them so GJS cannot swallow a failure.
        bindings.push({ literal: widget instanceof Gtk.Label && !widget.use_markup && widget.label === choice?.label
            && widget.ellipsize === Pango.EllipsizeMode.END,
            tooltipFree: ancestors.every(value => !value.has_tooltip && !value.tooltip_text && !value.tooltip_markup) });
    });
    assert(view.deviceChoices instanceof Gtk.Revealer, `${kind}: inline choices use a native revealer`);
    equal([view.deviceChoices.transition_type, view.deviceChoices.transition_duration],
        [Gtk.RevealerTransitionType.SLIDE_DOWN, 180], `${kind}: native slide-down animation`);
    const options = view.deviceChoices.get_child();
    assert(options instanceof Gtk.ScrolledWindow && options.get_child() === view.deviceList
        && options.propagate_natural_height && options.max_content_height === 240
        && options.margin_top === 6 && options.has_css_class('device-options'),
        `${kind}: inline list has a naturally sized, capped scroller`);
    noDevicePopups();
    for (let widget = view.deviceChoices; widget && widget !== panel.audioPage; widget = widget.get_parent())
        assert(!(widget instanceof Gtk.Overlay), `${kind}: inline choices are not hosted in an overlay`);
    expanded(view, false, `${kind}: starts collapsed`);
    // Replace metadata after observing the actual factory lifecycle.
    update({ choices: initial.map(item => ({ ...item,
        description: item.description ? item.description + ' [bound]' : '' })) });
    await wait();
    const entries = backend.state[kind].choices;
    assert(selection.model instanceof Gtk.StringList, `${kind}: real native string model`);
    equal(Array.from({ length: selection.model.get_n_items() }, (_, i) => selection.model.get_string(i)),
        entries.map(item => item.label), `${kind}: literal ordered model without an unnecessary placeholder`);
    equal(selection.selected, Gtk.INVALID_LIST_POSITION, `${kind}: native default label does not preselect a row`);
    assert(view.deviceLabel.label === entries[1].label && !view.deviceLabel.use_markup
        && view.deviceLabel.ellipsize === Pango.EllipsizeMode.END,
        `${kind}: collapsed trigger shows literal ellipsized default text`);
    for (const widget of walk(trigger)) noInheritedTooltip(widget, `${kind}: trigger has no tooltip`);
    input('move', view.devicesHeading);
    await wait(250);
    const restingPixels = input('pixels', trigger);
    input('move', trigger);
    await wait(250);
    assert(trigger.get_state_flags() & Gtk.StateFlags.PRELIGHT, `${kind}: real pointer enters native toggle`);
    equal(input('pixels', trigger), restingPixels, `${kind}: closed native toggle pixels are hover-invariant`);
    input('move', view.devicesHeading);
    await wait(250);
    assert(!view.heading.use_markup && view.heading.label.includes('<b>'), `${kind}: heading is literal`);
    assert(!('rows' in view) && !('choices' in view), `${kind}: old per-device button rows are absent`);
    assert(!walk(view.deviceList).some(widget => widget instanceof Gtk.Button), `${kind}: choices are native list rows, not buttons`);
    allocation(`${kind}: long collapsed labels retain the compact frame`);
    let before = backend.calls.length;
    const model = selection.model;
    update({ volume: 0.61, muted: true });
    assert(selection.model === model, `${kind}: volume/mute render preserves the model`);
    equal(view.volume.get_layout().get_text(), '61%', `${kind}: native volume snapshot rendered`);
    selection.selected = 0;
    selection.notify('selected');
    equal(backend.calls.length, before, `${kind}: selection notification is read-only`);
    panel.render();
    equal(selection.selected, Gtk.INVALID_LIST_POSITION, `${kind}: closed render clears selection without highlighting the default`);
    panel.setDeviceExpanded(view, true);
    view.deviceList.emit('activate', 0);
    equal(backend.calls.slice(before), [['select', kind, entries[0].key]], `${kind}: activation selects the first duplicate by key`);
    equal(selection.selected, Gtk.INVALID_LIST_POSITION, `${kind}: activation clears the closed list selection`);
    equal(view.deviceLabel.label, entries[1].label, `${kind}: pending activation cannot invent a native default label`);
    expanded(view, false, `${kind}: activation collapses the list`);
    focused(trigger, `${kind}: activation focuses the trigger`);
    before = backend.calls.length;
    update({ choices: entries.map((item, i) => ({ ...item, default: i === 0 })) });
    assert(selection.model === model && selection.selected === Gtk.INVALID_LIST_POSITION,
        `${kind}: external default leaves selection empty without rebuilding`);
    equal(view.deviceLabel.label, entries[0].label, `${kind}: the trigger follows the external native default`);
    equal(backend.calls.length, before, `${kind}: external native default change never mutates`);
    panel.setDeviceExpanded(view, true);
    view.deviceList.emit('activate', 1);
    equal(backend.calls.slice(before), [['select', kind, entries[1].key]], `${kind}: second duplicate retains a distinct key`);
    before = backend.calls.length;
    panel.setDeviceExpanded(view, true);
    update({ busy: true });
    assert(!trigger.sensitive && view.volume.sensitive && view.mute.sensitive, `${kind}: busy disables only device selection`);
    expanded(view, false, `${kind}: busy render collapses choices`);
    panel.setDeviceExpanded(view, true);
    expanded(view, false, `${kind}: busy cannot expand choices`);
    view.deviceList.emit('activate', 2);
    equal(backend.calls.length, before, `${kind}: busy activation is ignored`);
    update({ busy: false });
    await wait(260);

    const compact = bounds(panel.card, panel.root), inputBefore = bounds(panel.audio.input.heading);
    const triggerBefore = bounds(trigger);
    input('click', trigger);
    await wait(260);
    expanded(view, true, `${kind}: real pointer expands inline choices`);
    equal(selection.selected, Gtk.INVALID_LIST_POSITION, `${kind}: pointer expansion does not select a default row`);
    const grown = bounds(panel.card, panel.root), listBounds = bounds(options);
    assert(grown.height > compact.height, `${kind}: expansion grows the actual card`);
    boundsEqual(bounds(trigger), triggerBefore, `${kind}: expansion does not move the trigger inside the card`);
    assert(bindings.length >= entries.length, `${kind}: real inline factory binds all three rows`);
    const labels = listLabels(view);
    equal(labels.length, entries.length, `${kind}: all small-list choices have real native labels`);
    for (const widget of labels) {
        const row = bounds(widget.get_parent());
        assert(row.y >= listBounds.y && row.bottom <= listBounds.bottom,
            `${kind}: actual native row lies inside the inline scroller`);
        if (kind === 'output') assert(row.bottom <= bounds(panel.audio.input.heading).y,
            'Actual output row bounds do not overlap input controls');
        noInheritedTooltip(widget, `${kind}: expanded labels and ancestors have no tooltip`);
    }
    if (kind === 'output') {
        geometryEqual(bounds(panel.audio.input.heading).y - inputBefore.y, grown.height - compact.height,
            'Output list pushes the actual input section down by the card growth');
        assert(bounds(panel.audio.input.heading).y >= listBounds.bottom,
            'Output choice rows end before the input section, never overlay it');
    } else {
        boundsEqual(bounds(panel.audio.input.heading), inputBefore, 'Input expansion leaves the upper input controls in place');
        assert(grown.y + grown.height > compact.y + compact.height, 'Input choices expand the lower frame edge');
    }
    allocation(`${kind}: inline expansion stays content-sized`);
    input('click', trigger);
    await wait(260);
    expanded(view, false, `${kind}: clicking the active toggle collapses choices`);
    equal(bounds(panel.card, panel.root), compact, `${kind}: toggling closed exactly restores the compact frame`);
    boundsEqual(bounds(panel.audio.input.heading), inputBefore, `${kind}: toggling closed restores input placement within subpixel rounding`);
    equal(backend.calls.length, before, `${kind}: toggling choices never selects a device`);
    input('click', trigger);
    await wait(260);
    input('click', listLabels(view)[2]);
    await wait(260);
    equal(backend.calls.slice(before), [['select', kind, entries[2].key]], `${kind}: real pointer activates the exact row key`);
    expanded(view, false, `${kind}: real pointer selection collapses choices`);
    focused(trigger, `${kind}: pointer selection returns focus to the trigger`);
    equal(bounds(panel.card, panel.root), compact, `${kind}: closing exactly restores the compact frame`);
    boundsEqual(bounds(panel.audio.input.heading), inputBefore, `${kind}: closing restores input placement within subpixel rounding`);
    assert(panel.opened, `${kind}: selecting an inline row does not dismiss the panel`);

    // Description-only replacements must rebind native rows, not leave stale metadata.
    before = backend.calls.length;
    const oldModel = selection.model, oldBinds = bindings.length;
    const rebound = backend.state[kind].choices.map(item => ({ ...item, description: `Rebound <b>${item.key}</b>` }));
    update({ choices: rebound });
    assert(selection.model !== oldModel, `${kind}: metadata replacement replaces the native string model`);
    equal(backend.calls.length, before, `${kind}: replacing model and clearing selection is read-only`);
    equal(selection.selected, Gtk.INVALID_LIST_POSITION, `${kind}: replacement model starts unselected`);
    input('click', trigger);
    await wait(260);
    assert(bindings.length > oldBinds, `${kind}: metadata replacement rebinds the actual list factory`);
    noInheritedTooltip(view.deviceLabel, `${kind}: rebound trigger remains tooltip-free`);
    assert(listLabels(view)[0].get_parent().grab_focus(), `${kind}: native first row takes keyboard focus`);
    // Use actual Gtk.ListView keyboard navigation, not selection notifications.
    input('key', host, 'Home');
    await wait();
    input('key', host, 'Down');
    await wait();
    equal(selection.selected, 1, `${kind}: real keyboard navigation selects the second row`);
    update({ volume: 0.62 });
    await wait();
    equal(selection.selected, 1, `${kind}: snapshot refresh preserves open keyboard selection`);
    equal(view.deviceLabel.label, rebound[0].label, `${kind}: keyboard navigation does not change the default label`);
    equal(backend.calls.length, before, `${kind}: real keyboard navigation alone never activates a device`);
    input('key', host, 'Return');
    await wait(260);
    equal(backend.calls.slice(before), [['select', kind, rebound[1].key]], `${kind}: real keyboard picks the duplicate by key`);
    expanded(view, false, `${kind}: keyboard selection collapses inline choices`);
    equal(selection.selected, Gtk.INVALID_LIST_POSITION, `${kind}: keyboard activation clears selection after collapse`);
    focused(trigger, `${kind}: keyboard selection returns focus to the trigger`);
    equal(bounds(panel.card, panel.root), compact, `${kind}: keyboard selection exactly restores the compact frame`);
    assert(panel.opened, `${kind}: keyboard activation leaves the panel open`);
    before = backend.calls.length;
    input('click', trigger);
    await wait(260);
    input('click', listLabels(view)[0]);
    await wait(260);
    equal(backend.calls.length, before, `${kind}: choosing the existing default does not mutate`);
    expanded(view, false, `${kind}: choosing the existing default still collapses`);
    focused(trigger, `${kind}: existing-default selection focuses the trigger`);

    for (const [properties, placeholder] of [
        [{ available: true, name: 'Unlisted <native> & name' }, 'Unlisted <native> & name'],
        [{ available: false, nodeKey: '', name: 'Stale name' }, 'Select a device…'],
    ]) {
        update({ ...properties, choices: rebound.map(item => ({ ...item, default: false })) });
        equal(selection.model.get_n_items(), rebound.length + 1, `${kind}: missing default prepends one placeholder`);
        equal([selection.selected, selection.model.get_string(0), view.choiceItems[0].key, view.deviceLabel.label],
            [Gtk.INVALID_LIST_POSITION, placeholder, '', placeholder], `${kind}: placeholder labels the trigger but stays unselected and non-actionable`);
        selection.notify('selected');
        view.deviceList.emit('activate', 0);
        equal(backend.calls.length, before, `${kind}: placeholder cannot call select`);
    }
    update({ available: true, nodeKey: `current:${kind}`, choices: initial });
    panel.setDeviceExpanded(view, true);
    update({ choices: [initial[1]] });
    assert(!trigger.visible && !view.devicesHeading.visible && !trigger.sensitive,
        `${kind}: sole default hides selector and Devices heading`);
    expanded(view, false, `${kind}: hotplug leaving no alternatives collapses choices`);
    panel.setDeviceExpanded(view, true);
    expanded(view, false, `${kind}: hidden selector cannot expand`);
    update({ available: false, nodeKey: '', choices: [] });
    assert(!trigger.visible && !view.devicesHeading.visible && !view.mute.sensitive && !view.volume.sensitive,
        `${kind}: empty hardware leaves no old rows or active controls`);
    equal(selection.model.get_n_items(), 1, `${kind}: removed choices are absent from the native model`);
    equal(backend.calls.length, before, `${kind}: hotplug/placeholder renders never mutate`);
    update({ available: true, nodeKey: `current:${kind}`, choices: initial, name: `${kind} default` });
    await wait(260);
    assert(bindings.length > 0 && bindings.every(item => item.literal && item.tooltipFree),
        `${kind}: every factory binding is literal, ellipsized and tooltip-free, including after rebinding`);
    factory.disconnect(bindSignal);
}

async function testVolume(kind) {
    const view = panel.audio[kind], scale = view.volume, bar = view.volumeBar.widget;
    const backward = bar.get_first_child(), forward = bar.get_last_child();
    const original = { ...backend.state[kind] };
    const noVolumeTooltips = message => {
        for (const control of [view.mute, backward, scale, forward, bar])
            noInheritedTooltip(control, `${kind}: ${message} has no control or ancestor tooltip`);
        for (const control of [view.mute, backward, forward])
            for (const child of walk(control)) noTooltip(child, `${kind}: ${message} has no button-child tooltip`);
    };
    const update = properties => {
        backend.state[kind] = { ...backend.state[kind], ...properties };
        panel.render();
    };
    const geometry = message => {
        equal([bar.get_allocated_height(), scale.get_height(), backward.get_height(), forward.get_height()],
            [20, 20, 20, 20], `${kind}: ${message} keeps the compact flat 20px bar`);
        const [start, end] = scale.get_slider_range();
        const [x, y] = scale.get_layout_offsets();
        const [, text] = scale.get_layout().get_pixel_extents();
        equal(end - start, 43, `${kind}: ${message} retains the compact native thumb with the thin button-matched outline`);
        assert(x >= start && x + text.width <= end && y >= 0 && y + text.height <= scale.get_height(),
            `${kind}: ${message} keeps the native percent inside the native thumb`);
    };
    assert(scale instanceof Gtk.Scale && backward instanceof Gtk.Button && forward instanceof Gtk.Button,
        `${kind}: existing native scale between native stepper buttons`);
    assert(backward.get_next_sibling() === scale && scale.get_next_sibling() === forward,
        `${kind}: no standalone percent widget`);
    assert(!('percent' in view), `${kind}: obsolete view.percent is removed`);
    noVolumeTooltips('constructed volume bar');
    for (const control of [backward, forward]) {
        assert(control.has_css_class('volume-step') && !control.has_css_class('control-button'),
            `${kind}: arrows do not inherit unrelated button sizing`);
        equal(control.get_width(), 20, `${kind}: arrow is 20px wide`);
    }
    equal([backward.get_child().label, forward.get_child().label], ['◁', '▷'], `${kind}: hollow arrows`);
    equal(scale.get_adjustment().step_increment, 0.01, `${kind}: native 1% increment is unchanged`);
    equal([scale.get_adjustment().lower, scale.get_adjustment().upper], [0, view.maximum], `${kind}: native volume limits`);
    equal(scale.round_digits, -1, `${kind}: displaying the percent does not quantize native volume`);
    let before = backend.calls.length;
    // Every percentage, including both endpoints and output amplification, uses
    // only GTK's value layout and slider allocation, never custom geometry.
    for (let percent = 0; percent <= view.maximum * 100; percent++) {
        update({ volume: percent / 100 });
        await wait(20);
        equal(scale.get_layout().get_text(), `${percent}%`, `${kind}: native label at ${percent}%`);
        geometry(`${percent}%`);
    }
    update({ volume: 0.3749 });
    equal(scale.get_value(), 0.3749, `${kind}: native fractional volume is preserved`);
    equal(scale.get_layout().get_text(), '37%', `${kind}: only displayed text rounds`);
    for (const available of [false, true, false, true]) {
        update({ available });
        await wait(30);
        equal(scale.get_layout().get_text(), available ? '37%' : '—', `${kind}: same-value availability invalidates cached text`);
        equal([bar.sensitive, backward.is_sensitive(), forward.is_sensitive()],
            [available, available, available], `${kind}: availability covers the entire bar`);
        geometry(available ? 'available' : 'disabled');
        noVolumeTooltips(available ? 'enabled rerender' : 'disabled rerender');
    }
    for (const flags of [Gtk.StateFlags.PRELIGHT, Gtk.StateFlags.ACTIVE]) {
        scale.set_state_flags(flags, false);
        await wait(30);
        geometry(`native state ${flags}`);
        scale.unset_state_flags(flags);
    }
    scale.grab_focus();
    await wait(30);
    geometry('focused');
    view.dragging = true;
    view.volumeTarget = view.nodeKey;
    scale.set_value(0.63);
    update({ volume: 0.42 });
    equal(scale.get_value(), 0.63, `${kind}: native updates remain suppressed during an owned drag`);
    equal(scale.get_layout().get_text(), '63%', `${kind}: native label follows the live drag, not the suppressed snapshot`);
    equal(backend.calls.length, before, `${kind}: all volume rendering and allocation checks are read-only`);
    view.dragging = false;
    view.volumeTarget = null;
    update({ volume: 0.4 });
    let moves = 0;
    const moveSignal = scale.connect('move-slider', () => { moves++; });
    for (const [name, disabled] of [['scale', scale], ['ancestor', bar.get_parent()]]) {
        const value = scale.get_value(), target = `disabled:${kind}:${name}`;
        view.dragging = true;
        view.volumeTarget = target;
        disabled.sensitive = false;
        try {
            noVolumeTooltips(`disabled ${name}`);
            equal(scale.sensitive, name === 'ancestor', `${kind}: disabling ${name} preserves the scale's own sensitivity semantics`);
            assert(!scale.is_sensitive(), `${kind}: disabled ${name} makes the native scale effectively insensitive`);
            backward.emit('clicked');
            forward.emit('clicked');
            equal(moves, 0, `${kind}: disabled ${name} blocks both native move-slider emissions`);
            equal([view.dragging, view.volumeTarget], [true, target], `${kind}: disabled ${name} cannot clear drag ownership`);
            equal(scale.get_value(), value, `${kind}: disabled ${name} cannot change the native value`);
            equal(backend.calls.length, before, `${kind}: disabled ${name} cannot mutate the backend`);
        } finally { disabled.sensitive = true; }
    }
    scale.disconnect(moveSignal);
    view.dragging = false;
    view.volumeTarget = null;
    const expectVolume = (value, message) => {
        const calls = backend.calls.slice(before);
        equal(calls.length, 1, `${kind}: ${message} makes one guarded mutation`);
        equal([calls[0][0], calls[0][1], calls[0][3]], ['volume', kind, view.nodeKey], `${kind}: ${message} uses the current native identity`);
        assert(Math.abs(calls[0][2] - value) < 1e-9, `${kind}: ${message} uses the native step increment`);
        before = backend.calls.length;
    };
    for (const [control, value, description] of [[forward, 0.41, 'Increase volume'], [backward, 0.4, 'Decrease volume']]) {
        view.dragging = true;
        view.volumeTarget = 'stale-drag-owner';
        control.emit('clicked');
        expectVolume(value, description);
        assert(!view.dragging && view.volumeTarget === null, `${kind}: step clears old drag ownership before the native signal`);
    }
    input('click', forward);
    await wait();
    expectVolume(0.41, 'real pointer step');
    scale.grab_focus();
    view.dragging = true;
    view.volumeTarget = 'stale-keyboard-owner';
    input('key', host, 'Right');
    await wait();
    expectVolume(0.42, 'real keyboard step');
    view.dragging = true;
    view.volumeTarget = 'stale-scroll-owner';
    input('scroll', scale);
    await wait();
    assert(backend.calls.length > before && backend.calls.slice(before).every(call =>
        call[0] === 'volume' && call[1] === kind && call[3] === view.nodeKey), `${kind}: native scroll uses the guarded current identity`);
    assert(!view.dragging && view.volumeTarget === null, `${kind}: scroll clears stale drag ownership`);
    before = backend.calls.length;
    input('click', view.mute);
    await wait();
    equal(backend.calls.slice(before), [['mute', kind, view.nodeKey]], `${kind}: native mute control is unchanged`);
    noVolumeTooltips('mute rerender');
    before = backend.calls.length;
    for (const configure of [
        () => { panel.opened = false; },
        () => { panel.currentTab = 0; },
        () => { backend.state[kind] = { ...original, available: false }; },
        () => { backend.state[kind] = { ...original, nodeKey: 'unrendered-replacement' }; },
    ]) {
        update(original);
        configure();
        forward.emit('clicked');
        equal(backend.calls.length, before, `${kind}: arrow cannot bypass nodeCurrent availability/ownership/lifecycle guards`);
        panel.opened = true;
        panel.currentTab = 1;
    }
    update({ ...original, volume: 0.4 });
    input('press', scale);
    await wait();
    assert(view.dragging && view.volumeTarget === view.nodeKey, `${kind}: real native drag captures its original node`);
    before = backend.calls.length;
    update({ nodeKey: 'replacement-during-drag', volume: 0.2 });
    equal(scale.get_value(), 0.2, `${kind}: new native identity is not suppressed by the old drag`);
    input('move', forward);
    input('release', forward);
    await wait();
    equal(backend.calls.length, before, `${kind}: old native drag cannot mutate the replacement node`);
    assert(!view.dragging, `${kind}: native release ends dragging`);
    view.volumeTarget = null;
    for (const [value, control] of [[0, backward], [view.maximum, forward]]) {
        update({ volume: value });
        before = backend.calls.length;
        control.emit('clicked');
        equal(scale.get_value(), value, `${kind}: native step clamps at ${value * 100}%`);
        assert(backend.calls.slice(before).every(call => call[2] === value), `${kind}: step never writes outside the native range`);
    }
    update(original);
    await wait();
    allocation(`${kind}: volume bar does not resize the frame`);
}

function testButtonTooltips() {
    const original = backend.state.bluetooth;
    const device = { key: 'fake-headset', name: 'Fake <headset>', address: 'AA:01', paired: true };
    const before = backend.calls.length;
    try {
        for (const model of [
            {},
            { devices: [{ ...device, connected: true }] },
            { pairing: true, pairingMessage: { text: 'Pairing…', prompt: 'Enter PIN:', needsAnswer: true } },
            { devices: [{ ...device, busy: true }] },
            { powered: false, devices: [{ ...device, name: 'Renamed fake headset' }] },
            {},
        ]) {
            backend.state.bluetooth = { available: true, powered: true, devices: [device], ...model };
            panel.render();
            const row = panel.deviceRows.get(device.key);
            for (const control of [...panel.tabs, panel.dismissPairing, panel.send, panel.cancel, row.action, row.forget]) {
                noInheritedTooltip(control, 'Constructed button and its ancestors have no tooltip after rerender');
                for (const child of walk(control)) noTooltip(child, 'Button children do not introduce tooltips');
            }
            equal(row.name.label, backend.state.bluetooth.devices[0].name, 'Full Bluetooth name remains literal label text');
            if (backend.state.bluetooth.devices[0].connected) {
                assert(row.widget.has_css_class('connected'), 'Connected Bluetooth device keeps its semantic class');
                equal(row.status.label, 'Connected', 'Connected Bluetooth status remains explicit');
                equal(row.action.get_child().label, 'Disconnect', 'Connected Bluetooth action stays unchanged');
                assert(!(row.widget.get_state_flags() & Gtk.StateFlags.SELECTED), 'Connected Bluetooth row is not selected');
            }
            for (const widget of walk(panel.root))
                noTooltip(widget, 'Every constructed native widget stays tooltip-free after rerender');
        }
        equal(backend.calls.length, before, 'Button tooltip regression checks never invoke backend operations');
    } finally {
        backend.state.bluetooth = original;
        panel.render();
    }
}

async function testSelectorGuards() {
    const view = panel.audio.output, other = panel.audio.input;
    const original = backend.state.output;
    const before = backend.calls.length;
    panel.setDeviceExpanded(view, true);
    panel.setDeviceExpanded(other, true);
    expanded(view, false, 'Opening input collapses output');
    expanded(other, true, 'Only the input list remains expanded');
    panel.setDeviceExpanded(view, true);
    expanded(other, false, 'Opening output collapses input');
    panel.setTab(0);
    for (const item of Object.values(panel.audio)) expanded(item, false, 'Tab reset collapses every list');
    panel.setDeviceExpanded(view, true);
    expanded(view, false, 'Bluetooth tab cannot expand an audio selector');
    view.deviceList.emit('activate', 0);
    panel.setTab(1);
    for (const item of Object.values(panel.audio)) expanded(item, false, 'Returning to Audio does not reopen choices');
    panel.setDeviceExpanded(view, true);
    panel.close();
    for (const item of Object.values(panel.audio)) expanded(item, false, 'Panel close resets every selector');
    panel.setDeviceExpanded(view, true);
    expanded(view, false, 'Closed panel cannot expand choices');
    view.deviceList.emit('activate', 0);
    panel.opened = true;
    panel.render();
    equal(backend.calls.length, before, 'Expansion, tab and closed-panel guards never mutate audio');

    // Keep the rendered keys stale deliberately: activation must validate the live snapshot.
    panel.setDeviceExpanded(view, true);
    const stale = view.choiceItems[0].key;
    backend.state.output = { ...original, choices: original.choices.filter(item => item.key !== stale) };
    view.deviceList.emit('activate', 0);
    equal(backend.calls.length, before, 'A removed native key never reaches backend.select');
    assert(view.error.visible && /no longer available/i.test(view.error.label),
        'Stale native keys report through the existing action error path');
    expanded(view, false, 'Stale selection collapses choices');
    focused(view.devices, 'Stale selection returns focus to the trigger');
    backend.state.output = original;
    panel.failures.output = '';
    panel.render();
    panel.setDeviceExpanded(view, true);
    backend.state.output = { ...original, busy: true };
    view.deviceList.emit('activate', 0);
    equal(backend.calls.length, before, 'Live busy state blocks activation before a rerender');
    backend.state.output = original;
    panel.render();
    panel.setDeviceExpanded(view, true);
    backend.state.output = { ...original, choices: original.choices.map((item, i) => ({ ...item, default: i === 0 })) };
    view.deviceList.emit('activate', 0);
    equal(backend.calls.length, before, 'A live default change prevents a redundant stale-view mutation');
    expanded(view, false, 'A newly authoritative default still collapses choices');
    focused(view.devices, 'A newly authoritative default returns focus to the trigger');
    backend.state.output = original;
    panel.render();
    panel.setDeviceExpanded(view, true);
    backend.selectionError = 'Fake native device selection failed';
    try {
        view.deviceList.emit('activate', 0);
        equal(backend.calls.slice(before), [['select', 'output', original.choices[0].key]],
            'A failing selection dispatches the validated native key once');
        assert(view.error.visible && view.error.label === backend.selectionError,
            'Backend selection errors use the existing visible action failure');
        expanded(view, false, 'A failed selection collapses choices');
        focused(view.devices, 'A failed selection returns focus to the trigger');
    } finally {
        backend.selectionError = '';
        panel.failures.output = '';
        panel.render();
    }
    for (const position of [Gtk.INVALID_LIST_POSITION, original.choices.length + 10])
        view.deviceList.emit('activate', position);
    equal(backend.calls.length, before + 1, 'Invalid native list positions never dispatch mutations');
    await wait(260);
}

async function testLargeInlineLists() {
    const monitor = panel.monitor, originals = { output: backend.state.output, input: backend.state.input };
    const before = backend.calls.length;
    try {
        for (const kind of ['output', 'input']) {
            const view = panel.audio[kind], seen = new Set(), bindings = [];
            const factory = view.deviceList.factory;
            const signal = factory.connect('bind', (_, item) => {
                const widget = item.get_child();
                seen.add(widget.label);
                bindings.push({ literal: !widget.use_markup && widget.ellipsize === Pango.EllipsizeMode.END,
                    tooltipFree: !widget.has_tooltip && !widget.tooltip_text && !widget.tooltip_markup });
            });
            try {
                for (const short of [false, true]) {
                    const entries = Array.from({ length: 40 }, (_, i) => ({ key: `native:${kind}:inline:${i}`,
                        label: `Inline ${kind} choice ${i} <b>& literal</b> ` + 'long name '.repeat(12),
                        description: `Inline ${kind} route ${i} <literal>`, default: i === 1 }));
                    seen.clear();
                    panel.monitor = short ? { get_geometry: () => ({ width: 340, height: 300 }) } : monitor;
                    backend.state[kind] = { ...originals[kind], choices: entries };
                    panel.render();
                    await wait(260);
                    await scrollIntoView(view.devices, panel.audioPage);
                    input('click', view.devices);
                    await wait(260);
                    expanded(view, true, `${kind}: large list expands on a ${short ? 'short' : 'normal'} monitor`);
                    const options = view.deviceChoices.get_child(), adjustment = options.get_vadjustment();
                    equal(options.max_content_height, 240, `${kind}: large-list viewport retains its native 240px cap`);
                    assert(adjustment.page_size > 0 && adjustment.page_size <= 240
                        && adjustment.upper > adjustment.page_size,
                                            `${kind}: many devices scroll inside the capped list (upper=${adjustment.upper}, page=${adjustment.page_size}, options=${options.get_height()}, card=${panel.card.get_height()})`);
                    assert(panel.card.get_height() <= (short ? 236 : 520), `${kind}: expanded list respects the monitor budget`);
                    equal(panel.card.get_width(), short ? 324 : 400, `${kind}: long inline names cannot widen the card`);
                    if (short) {
                        const page = panel.audioPage.get_vadjustment();
                        assert(page.upper > page.page_size, `${kind}: short monitor also scrolls the whole audio page`);
                    }
                    // Visit the entire native scroll range, including recycled factory rows.
                    const maximum = adjustment.upper - adjustment.page_size;
                    for (let value = 0; value < maximum; value += Math.max(1, adjustment.page_size / 2)) {
                        adjustment.value = value;
                        await wait(30);
                        for (const widget of listLabels(view)) noInheritedTooltip(widget, `${kind}: large-list labels remain tooltip-free`);
                    }
                    adjustment.value = maximum;
                    await wait();
                    const last = listLabels(view).find(widget => widget.label === entries.at(-1).label);
                    assert(last, `${kind}: native scrolling reaches the last device`);
                    const row = bounds(last.get_parent(), options);
                    assert(row.y >= 0 && row.bottom <= options.get_height(), `${kind}: last native row is allocated inside the capped viewport`);
                    assert(entries.every(item => seen.has(item.label)), `${kind}: every large-list label was bound without a tooltip`);
                    panel.setDeviceExpanded(view, false);
                    await wait(260);
                    expanded(view, false, `${kind}: large list collapses within the frame`);
                    noDevicePopups();
                    backend.state[kind] = originals[kind];
                    panel.render();
                    await wait(260);
                }
                assert(bindings.length > 0 && bindings.every(item => item.literal && item.tooltipFree),
                    `${kind}: all large-list and recycled bindings are literal, ellipsized and tooltip-free`);
            } finally { factory.disconnect(signal); }
        }
    } finally {
        panel.monitor = monitor;
        for (const kind of ['output', 'input']) {
            backend.state[kind] = originals[kind];
            panel.setDeviceExpanded(panel.audio[kind], false);
        }
        panel.render();
    }
    await wait(260);
    equal(backend.calls.length, before, 'Large-list layout and scrolling never mutate devices');
    allocation('Large-list cleanup restores the compact frame');
}

async function testRootDismissal() {
    const view = panel.audio.output, before = backend.calls.length;
    const compact = bounds(panel.card, panel.root);
    const activity = backend.activity.length;
    input('click', view.devices);
    await wait(260);
    assert(listLabels(view)[0].get_parent().grab_focus(), 'Escape starts with focus inside a real list row');
    input('key', host, 'Escape');
    await wait(260);
    expanded(view, false, 'First real Escape collapses the open list');
    focused(view.devices, 'First Escape focuses the trigger');
    assert(panel.opened && backend.activity.length === activity, 'First Escape is consumed before panel close');
    equal(bounds(panel.card, panel.root), compact, 'Escape exactly restores compact geometry');
    input('key', host, 'Escape');
    await wait(260);
    assert(!panel.opened && backend.activity.length === activity + 1 && backend.activity.at(-1)?.[0] === false,
        'Second real Escape reaches the hosted root controller and closes the panel');
    for (const item of Object.values(panel.audio)) expanded(item, false, 'Escape close leaves every selector collapsed');
    panel.opened = true;
    panel.render();
    await wait(260);
    input('click', panel.root);
    await wait(260);
    assert(!panel.opened && backend.activity.at(-1)?.[0] === false,
        'An actual root click outside the card still closes the panel');
    equal(backend.calls.length, before, 'Escape and outside dismissal never mutate audio');
}

async function smoke() {
    const services = ['org.bluez', 'org.PulseAudio1', 'org.freedesktop.NetworkManager'];
    assert(services.every(name => !owner(name)), 'Private bus has no live audio/Bluetooth/network services');
    Gtk.init();
    GLib.unsetenv('LD_PRELOAD');
    const palette = loadTheme(entry, GLib.getenv('AUDIO_PANEL_PALETTE'));
    assert(Boolean(palette.yellow), 'Actual custom CSS parses in GTK without parsing errors');
    const css = read(GLib.build_filenamev([GLib.path_get_dirname(entry), 'style.css']));
    assert(/background(?:-color)?:\s*alpha\(\s*shade\(\s*@ap_red\s*,\s*0?\.45\s*\)\s*,\s*0?\.5\s*\)/.test(css),
        'Inline selector stylesheet uses the requested half-transparent dark-red shade');
    app = new Astal.Application({ instance_name: 'audio-dropdown-offline' });
    app.register(null);
    assert(!app.get_is_remote(), 'Application instance belongs to the private bus');
    panel = new AudioPanel({ app, backend, palette });
    assert(!panel.window.visible && backend.starts === 0 && backend.calls.length === 0,
        'Real AudioPanel constructs hidden with fake snapshot only');
    // Never present the layer-shell window on X11; retain the production root
    // and its capture-phase key/click-outside controllers for real XTEST input.
    panel.window.set_child(null);
    host = new Gtk.Window({ child: panel.root, decorated: false, default_width: 1280,
        default_height: 800, css_classes: ['audio-panel'] });
    panel.opened = true;
    panel.monitor = { get_geometry: () => ({ width: 1280, height: 800 }) };
    panel.render();
    host.present();
    await wait(200);
    for (const tab of panel.tabs) {
        const rgb = (tab.has_css_class('selected') ? palette.cyan : palette.pink)
            .slice(1).match(/../g).map(value => parseInt(value, 16) / 255);
        for (const state of [Gtk.StateFlags.NORMAL, Gtk.StateFlags.PRELIGHT, Gtk.StateFlags.FOCUS_VISIBLE]) {
            tab.set_state_flags(state, false);
            await wait(150);
            const color = tab.get_child().get_color();
            assert([color.red, color.green, color.blue].every((value, index) => Math.abs(value - rgb[index]) < 0.01),
                'Selected and inactive tabs must retain their text colors at rest, on hover and keyboard focus');
            tab.unset_state_flags(state);
        }
    }
    await testCompactLayout();
    for (const kind of ['output', 'input']) await testKind(kind);
    await testSelectorGuards();
    await testLargeInlineLists();
    assert(backend.calls.every(call => call[0] === 'select'), 'Selector rendering never invokes volume/mute mutations');
    for (const kind of ['output', 'input']) await testVolume(kind);
    testButtonTooltips();
    await testRootDismissal();
    allocation('Final real GTK frame remains content-sized');
    assert(backend.starts === 0 && !panel.window.visible, 'Backend never started and Astal window stayed hidden');

    assert(services.every(name => !owner(name)), 'No live service appeared during the fake-only test');
    print(`PASS audio inline-selector/volume GTK smoke: ${checks} assertions; fake backend only; actual source, CSS, native scale geometry, inline factories, capped scrolling and root XTEST Escape`);
    print('LIMITATION Xvfb does not validate Wayland layer-shell placement, exclusive key grabs, accessibility announcements, or tooltip painting.');
}

let exitCode = 0;
Gtk.ToggleButton.prototype.update_state = trackAccessibleState;
try { await smoke(); }
catch (error) { console.error(`FAIL audio inline-selector/volume GTK smoke after ${checks} assertions`, error); exitCode = 1; }
finally {
    Gtk.ToggleButton.prototype.update_state = updateAccessibleState;
    host?.destroy();
    panel?.dispose();
    app?.quit();
}
System.exit(exitCode);
}

// node test.js --tooltips /absolute/path/to/panel-gjs
// Real GTK widget trees; private Xvfb/service-free D-Bus; in-memory models only.
// Never import a panel backend or use the system clipboard, devices or history.
async function isolatedTooltips(runtime) {
    const fs = await import('node:fs');
    const os = await import('node:os');
    const path = await import('node:path');
    const { fileURLToPath } = await import('node:url');
    const { spawn } = await import('node:child_process');
    const { setTimeout: delay } = await import('node:timers/promises');
    if (!runtime || !path.isAbsolute(runtime)) throw new Error('Pass an absolute path to panel-gjs.');
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'panel-tooltip-smoke-'));
    const env = { ...process.env };
    for (const name of ['DISPLAY', 'WAYLAND_DISPLAY', 'WAYLAND_SOCKET', 'DBUS_SESSION_BUS_ADDRESS',
        'DBUS_SYSTEM_BUS_ADDRESS', 'XAUTHORITY', 'LD_PRELOAD', 'GTK_THEME', 'GTK_PATH', 'GTK_MODULES',
        'GIO_EXTRA_MODULES', 'GI_TYPELIB_PATH']) delete env[name];
    for (const name of ['home', 'config', 'data', 'cache', 'runtime']) fs.mkdirSync(path.join(root, name), { mode: 0o700 });
    Object.assign(env, { HOME: path.join(root, 'home'), XDG_CONFIG_HOME: path.join(root, 'config'),
        XDG_DATA_HOME: path.join(root, 'data'), XDG_CACHE_HOME: path.join(root, 'cache'),
        XDG_RUNTIME_DIR: path.join(root, 'runtime'), GDK_BACKEND: 'x11', GSETTINGS_BACKEND: 'memory',
        GTK_A11Y: 'none', GIO_USE_VFS: 'local', GSK_RENDERER: 'cairo', PANEL_TOOLTIP_ISOLATED: '1' });
    const children = [];
    function start(command, args, options = {}) {
        const child = spawn(command, args, { env, stdio: ['ignore', 'pipe', 'pipe'], ...options });
        const record = { child, log: '' };
        child.stdout.on('data', bytes => { record.log += bytes; });
        child.stderr.on('data', bytes => { record.log += bytes; });
        record.done = new Promise(resolve => {
            child.on('error', error => { record.error = error; resolve(-1); });
            child.on('exit', code => { record.exited = true; resolve(code); });
        });
        children.push(record);
        return record;
    }
    async function until(check, description) {
        const deadline = Date.now() + 5000;
        while (Date.now() < deadline) {
            const failed = children.find(record => record.error || record.exited);
            if (failed) throw failed.error || new Error(failed.log);
            const result = check();
            if (result) return result;
            await delay(25);
        }
        throw new Error(`Timed out waiting for ${description}\n${children.map(record => record.log).join('\n')}`);
    }
    try {
        const display = start('Xvfb', ['-displayfd', '3', '-screen', '0', '1024x768x24', '-nolisten', 'tcp'],
            { stdio: ['ignore', 'pipe', 'pipe', 'pipe'] });
        let displayNumber = '';
        display.child.stdio[3].on('data', bytes => { displayNumber += bytes; });
        env.DISPLAY = ':' + (await until(() => displayNumber.includes('\n') && displayNumber, 'private display')).trim();
        const config = path.join(root, 'bus.conf');
        fs.writeFileSync(config, `<busconfig><type>session</type><listen>unix:tmpdir=${root}</listen>
            <auth>EXTERNAL</auth><policy context="default"><allow send_destination="*"/>
            <allow receive_sender="*"/><allow own="*"/></policy></busconfig>`);
        const bus = start('dbus-daemon', ['--nofork', '--nopidfile', '--print-address=1', `--config-file=${config}`]);
        env.DBUS_SESSION_BUS_ADDRESS = await until(() => bus.log.match(/unix:[^\n]+/)?.[0], 'private bus');
        env.DBUS_SYSTEM_BUS_ADDRESS = env.DBUS_SESSION_BUS_ADDRESS;
        const smoke = start(runtime, ['-m', fileURLToPath(import.meta.url), 'tooltips'], { timeout: 20000 });
        const code = await smoke.done;
        if (code !== 0) throw smoke.error || new Error(smoke.log || `GTK smoke exited ${code}`);
        console.log(smoke.log.trim());
    } finally {
        children.forEach(({ child }) => child.kill('SIGTERM'));
        await Promise.race([Promise.all(children.map(record => record.done)), delay(1000)]);
        children.forEach(({ child, exited }) => { if (!exited) child.kill('SIGKILL'); });
        fs.rmSync(root, { recursive: true, force: true });
    }
}

async function gjsTooltipTests() {
    const { default: GLib } = await import('gi://GLib');
    const { default: Gtk } = await import('gi://Gtk?version=4.0');
    const { default: Gdk } = await import('gi://Gdk?version=4.0');
    const { default: Pango } = await import('gi://Pango');
    const scripts = GLib.canonicalize_filename('../..', GLib.path_get_dirname(GLib.filename_from_uri(import.meta.url)[0]));
    const { TABS, editable, title } = await import('../../clipboard/astal/logic.js');
    const { formatTime } = await import('../../media-control/astal/logic.js');
    if (GLib.getenv('PANEL_TOOLTIP_ISOLATED') !== '1' || GLib.getenv('GDK_BACKEND') !== 'x11'
        || !GLib.getenv('DBUS_SESSION_BUS_ADDRESS')
        || GLib.getenv('DBUS_SYSTEM_BUS_ADDRESS') !== GLib.getenv('DBUS_SESSION_BUS_ADDRESS'))
        throw new Error('Run through the Node isolation wrapper, not the desktop session.');
    GLib.unsetenv('LD_PRELOAD');
    Gtk.init();
    class Artwork {
        constructor() { this.widget = new Gtk.Box(); }
        set() {}
        dispose() {}
    }
    class Credit { set() {} }
    const bindings = { Gtk, Gdk, Pango, TABS, editable, title, formatTime, Artwork, Credit };
    function panelClass(scope, name, boundary) {
        const [ok, bytes] = GLib.file_get_contents(GLib.build_filenamev([scripts, scope, 'astal', 'frontend.js']));
        if (!ok) throw new Error(`Cannot read ${scope} frontend`);
        const source = new TextDecoder().decode(bytes);
        const start = source.indexOf(`class ${name}`), end = source.indexOf(boundary);
        if (start < 0 || end < start) throw new Error(`Missing ${scope} extraction boundary`);
        const code = source.slice(source.indexOf('function box('), source.indexOf('function frame('))
            + source.slice(start, end);
        return new Function(...Object.keys(bindings), `${code}\nreturn ${name};`)(...Object.values(bindings));
    }
    const wait = () => new Promise(resolve => GLib.timeout_add(GLib.PRIORITY_DEFAULT, 60,
        () => { resolve(); return GLib.SOURCE_REMOVE; }));
    let checks = 0;
    function checkTree(widget) {
        checks++;
        if (widget.has_tooltip || widget.tooltip_text || widget.tooltip_markup)
            throw new Error(`Visual tooltip on ${widget.constructor.name}: ${widget.tooltip_text || widget.tooltip_markup}`);
        for (let child = widget.get_first_child(); child; child = child.get_next_sibling()) checkTree(child);
    }
    async function exercise(name, panel, renderCases) {
        const host = new Gtk.Window({ child: panel.content, default_width: 500, default_height: 650 });
        try {
            host.present();
            for (const update of renderCases) {
                update();
                panel.render();
                await wait();
                checkTree(host);
            }
            print(`PASS ${name}: real GTK tree has no tooltip text, markup or enabled hover tips across rerenders`);
        } finally { host.destroy(); }
    }
    const AudioPanel = panelClass('audio-control', 'AudioPanel', '\nfunction monitorFor(');
    const audioState = { bluetooth: { available: true, powered: true, devices: [] } };
    for (const kind of ['output', 'input']) audioState[kind] = {
        available: true, nodeKey: kind, name: `Fake ${kind}`, volume: 0.4,
        choices: [{ key: 'one', label: 'Default <literal>', description: 'Full route name', default: true },
            { key: 'two', label: 'Other route', description: 'Another full route name' }],
    };
    const audio = Object.assign(Object.create(AudioPanel.prototype), {
        content: new Gtk.Box({ orientation: Gtk.Orientation.VERTICAL }), audio: {}, deviceRows: new Map(),
        failures: {}, opened: true, currentTab: 1, cardWidth: 400, backend: { snapshot: () => audioState },
    });
    audio.build({ cyan: '#00ffff' });
    await exercise('audio', audio, [() => {}, () => { audio.setDeviceExpanded(audio.audio.output, true); },
        () => { audioState.output = { ...audioState.output, name: 'Renamed output', muted: true }; },
        () => { audioState.input = { ...audioState.input, available: false, nodeKey: '' }; },
        ...['Fake <headset>', 'Renamed headset', ''].map(name => () => {
            audio.currentTab = 0;
            audioState.bluetooth.devices = [{ key: 'fake', address: 'AA:01', name, paired: true }];
        }), () => { audioState.bluetooth.devices = []; }]);

    {
        const [, cssBytes] = GLib.file_get_contents(GLib.build_filenamev([scripts, 'audio-control', 'astal', 'style.css']));
        const colors = { cyan: '#00ffff', yellow: '#ffff00', red: '#ff0000', pink: '#ff00ff',
            dimPink: '#330022', foreground: '#eeeeee', separator: '#777777',
            background: '#25252f', chromeBackground: '#25252f' };
        const provider = new Gtk.CssProvider();
        provider.load_from_string('.audio-panel {' + Object.entries(colors)
            .map(([key, value]) => `--ap_${key}: ${value};`).join('') + '}\n' + new TextDecoder().decode(cssBytes));
        Gtk.StyleContext.add_provider_for_display(Gdk.Display.get_default(), provider, Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION);
        let monitorHeight = 800;
        const available = Array.from({ length: 8 }, (_, index) => ({
            key: `nearby-${index}`, name: `AA-BB-CC-DD-EE-${String(index).padStart(2, '0')}`,
            address: `AA:BB:CC:DD:EE:${String(index).padStart(2, '0')}`,
        }));
        const state = { ...audioState, bluetooth: { available: true, powered: true, devices: [
            { key: 'paired-one', name: 'Headphones', paired: true },
            { key: 'paired-two', name: 'Mouse', paired: true }, ...available,
        ] } };
        const panel = Object.assign(Object.create(AudioPanel.prototype), {
            content: new Gtk.Box({ orientation: Gtk.Orientation.VERTICAL, css_classes: ['audio-content'] }),
            card: new Gtk.Overlay({ width_request: 400, halign: Gtk.Align.START, valign: Gtk.Align.START,
                css_classes: ['audio-card'] }),
            audio: {}, deviceRows: new Map(), failures: {}, opened: true, currentTab: 0, cardWidth: 400,
            backend: { snapshot: () => state }, monitor: { get_geometry: () => ({ width: 1280, height: monitorHeight }) },
        });
        panel.card.set_child(new Gtk.Box());
        panel.card.add_overlay(panel.content);
        panel.card.set_measure_overlay(panel.content, true);
        panel.build();
        const host = new Gtk.Window({ child: panel.card, default_width: 500, default_height: 650,
            css_classes: ['audio-panel'] });
        try {
            panel.render(); host.present(); await wait();
            panel.render(); await wait();
            const viewport = panel.bluetoothList;
            for (const device of available.slice(0, 5)) {
                const [ok, bounds] = panel.deviceRows.get(device.key).widget.compute_bounds(viewport);
                if (!ok || bounds.get_y() < -1 || bounds.get_y() + bounds.get_height() > viewport.get_height() + 1)
                    throw new Error(`${device.key}: five unpaired devices must fit below remembered rows`);
                checks++;
            }
            const [ok, sixth] = panel.deviceRows.get(available[5].key).widget.compute_bounds(viewport);
            if (!ok || sixth.get_y() < viewport.get_height() - 1)
                throw new Error('The sixth unpaired row must scroll rather than enlarge the viewport');
            if (panel.card.get_height() >= 520) throw new Error('Bluetooth must shrink below its old fixed height');
            monitorHeight = 300;
            panel.render(); await wait();
            if (panel.card.get_height() > 236) throw new Error('Bluetooth must stay inside the short-monitor budget');
            checkTree(host);
            print('PASS Bluetooth: five unpaired rows fit below remembered devices; extra rows scroll; short monitors stay bounded');
        } finally {
            host.destroy();
            Gtk.StyleContext.remove_provider_for_display(Gdk.Display.get_default(), provider);
        }
    }

    const MediaPanel = panelClass('media-control', 'MediaPanel', '\nlet backend, panel;');
    let enabled = true;
    const media = Object.assign(Object.create(MediaPanel.prototype), {
        content: new Gtk.Box({ orientation: Gtk.Orientation.VERTICAL }), groups: [], activePlayer: '', rows: new Map(),
        opened: true, failure: '', backend: { current: () => enabled,
            transport: () => ({ enabled, reason: enabled ? '' : 'Unavailable' }),
            progress: () => null,
            player: bus => media.groups.find(group => group.primary.bus === bus)?.primary },
    });
    media.build();
    const group = (bus, text) => ({ displayName: `Fake ${bus}`, title: text,
        primary: { bus, title: text, canSeek: true, length: 90 } });
    await exercise('media', media, [() => {}, () => {
        media.groups = [group('one', 'Long <literal> title & 🎵'.repeat(10)), group('two', 'Other track')];
        media.activePlayer = 'one';
        media.sources.reveal_child = true;
    }, () => { media.groups[0].primary.title = 'Renamed track'; enabled = false; },
    () => { media.groups = [group('two', '')]; media.activePlayer = 'two'; enabled = true; },
    () => { media.groups = []; }]);

    const ClipboardPanel = panelClass('clipboard', 'ClipboardPanel', '\nexport function createApplication(');
    const model = { rows: [], editor: null, tab: 'memo', query: '', busy: false, failure: '',
        filter(query) { this.query = query; } };
    const clipboard = Object.assign(Object.create(ClipboardPanel.prototype), {
        content: new Gtk.Box({ orientation: Gtk.Orientation.VERTICAL }), model, opened: false,
        viewRows: [], rowTitles: [], titleCache: new WeakMap(), home: '/fixture', bufferText: '',
    });
    clipboard.build();
    await exercise('clipboard', clipboard, [() => {},
        ...[[1, 'memo', false, ''], [2, 'file', true, '/fixture/local.txt'], [3, 'text', false, '']]
            .map(([id, kind, pinned, name]) => () => {
                model.rows = [{ id, kind, pinned, name, text: `Fake entry ${id}` }];
                model.tab = kind;
                model.query = id % 2 ? 'fixture' : '';
            }), () => { model.busy = true; }, () => { model.rows = []; }]);
    print(`PASS ${checks} native widget checks; only fixture strings and model doubles; no backends or clipboard access`);
    print('LIMITATION: Xvfb checks GTK tooltip properties, not Wayland placement, tooltip painting or screen-reader announcements.');
}

const usage = 'Usage:\n'
    + '  node --test scripts/audio-control/astal/test.js (Node-only units)\n'
    + '  node scripts/audio-control/astal/test.js --tooltips /absolute/path/to/panel-gjs\n'
    + '  panel-gjs -m scripts/audio-control/astal/test.js pipes|native|dropdown|tooltips\n'
    + 'pipes is service-free; native reads live services; dropdown requires AUDIO_DROPDOWN_ISOLATED=1 and private X11/D-Bus; '
    + 'tooltips requires the Node isolation wrapper.';

if (globalThis.process?.versions?.node) {
    const args = process.argv.slice(2);
    if (!args.length) await nodeTests();
    else if (args.length === 2 && args[0] === '--tooltips') await isolatedTooltips(args[1]);
    else throw new Error(`Unknown or invalid Node test mode: ${args.join(' ')}\n${usage}`);
} else {
    const args = typeof ARGV === 'undefined' ? [] : ARGV;
    if (args.length !== 1) throw new Error(usage);
    switch (args[0]) {
    case 'pipes': await gjsNativeTests(true); break;
    case 'native': await gjsNativeTests(); break;
    case 'dropdown': await gjsDropdownTests(); break;
    case 'tooltips': await gjsTooltipTests(); break;
    default: throw new Error(`Unknown GJS test mode: ${args[0]}\n${usage}`);
    }
}
