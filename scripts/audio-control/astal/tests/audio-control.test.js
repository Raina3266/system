import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync, chmodSync, mkdirSync, mkdtempSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { setImmediate as settle } from 'node:timers/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import vm from 'node:vm';
import * as logic from '../logic.js';
import {
    parseRequest, volumeValue, parseChoices, policyEvent, selectionPlan, deviceChoices,
    bluetoothDevices, pairingAnswer, waybarStatus,
} from '../logic.js';


// Tests consolidated from architecture.test.js.
{
const root = new URL('../../../../', import.meta.url);
const read = path => readFileSync(new URL(path, root), 'utf8');

test('audio packaging uses unmodified Astal with only declared runtime dependencies', () => {
    const source = read('niri/panels/audio.nix');
    assert.match(source, /pkgs\.astal\.wireplumber/);
    assert.match(source, /pkgs\.astal\.bluetooth/);
    assert.doesNotMatch(source, /overrideAttrs|substituteInPlace|astalGjs|postPatch|patches\s*=/);
    assert.doesNotMatch(source, /astal\s*=\s*pkgs\.astal/);
});

test('audio application uses the public native request virtual and socket writer', () => {
    const source = read('scripts/audio-control/astal/frontend.js');
    assert.match(source, /extends Astal\.Application/);
    assert.match(source, /vfunc_request\(request, connection\)/);
    assert.match(source, /AstalIO\.write_sock\(connection/);
    assert.match(source, /AstalIO\.write_sock_finish\(result\)/);
    assert.match(source, /connection\.close_async/);
    assert.doesNotMatch(source, /\.prototype\.|Object\.defineProperty|gi:\/\/Gio.*SocketService/);
});

test('inline audio device choices have a quiet dark-red half-opacity trigger and row-only highlights', () => {
    const css = read('scripts/audio-control/astal/style.css');
    assert.doesNotMatch(css, /dropdown\.device-dropdown|popover|button\.device-dropdown:(?:hover|focus-visible)/);
    const trigger = css.match(/\.audio-panel button\.device-dropdown \{([^}]+)\}/)?.[1];
    assert.ok(trigger, 'the inline toggle has its own style, not control-button highlights');
    assert.match(trigger, /background: alpha\(shade\(@ap_red, 0\.45\), 0\.5\);/);
    assert.match(trigger, /border: 0\.35px solid alpha\(@ap_foreground, 0\.5\);/);
    assert.doesNotMatch(trigger, /@ap_yellow|lighter\(/);
    const options = css.match(/\.audio-panel (?:scrolledwindow)?\.device-options \{([^}]+)\}/)?.[1];
    assert.ok(options, 'inline options have a dedicated container style');
    assert.match(options, /background: transparent;/);
    assert.match(options, /border: 0\.35px solid alpha\(@ap_foreground, 0\.5\);/);
    assert.match(css, /\.audio-panel listview\.device-list row:selected \{\s*background: alpha\(@ap_dimPink, 0\.7\);\s*\}/);
    assert.match(css, /\.audio-panel listview\.device-list row:hover \{\s*background: alpha\(@ap_pink, 0\.333\);\s*\}/);
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

// Tests consolidated from backend.test.js.
{
// Exercise the real coordinator and logic, replacing only GI imports and the
// subprocess/pipe implementation. Each fixture gets an isolated module context.
const source = readFileSync(new URL('../backend.js', import.meta.url), 'utf8')
    .replace(/^import .*;\r?\n/gm, '')
    .replace(/export class HelperProcess[\s\S]*?(?=export class PolicyProcess)/, '')
    .replace(/^export /gm, '');
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

function choice({ node, nativeCard = node?.card, ...properties } = {}) {
    return { key: 'card:port', label: 'Headphones', description: 'Built-in Audio — Headphones',
        name: node?.pwName ?? 'alsa_output.test', portName: 'headphones', card: nativeCard?.pwName || '',
        port: true, portActive: false, nodeId: node?.id ?? 10, nodeSerial: node?.serial ?? '100',
        cardId: nativeCard?.id ?? null, cardSerial: nativeCard?.serial ?? '', routeId: 0, ...properties };
}

const identity = object => ({ id: object.id, serial: object.serial });
const action = (operation, object, extra = {}) => ({ type: 'action', id: 1, operation,
    [operation === 'profile' ? 'card' : 'node']: identity(object), ...extra });

function fixture(t, {
    outputs = [endpoint()],
    inputs = [endpoint({ id: 20, serial: '200', pwName: 'alsa_input.test', description: 'Microphone', nick: '', volume: 0.7 })],
    radio = adapter(), devices = [], cards = [], streams = [], recorders = [],
} = {}) {
    const timers = clock();
    const helpers = [];
    class HelperProcess {
        constructor(argv, options = {}) {
            this.argv = [...argv];
            this.options = options;
            this.done = false;
            this.stops = 0;
            this.answers = [];
            this.output = '';
            helpers.push(this);
        }

        emit(text) {
            assert.equal(this.done, false, 'only live helpers emit pipe data');
            this.output += text;
            // The real pipe catches parser/callback failures and finishes the child.
            try { this.options.onChunk?.(text); } catch (error) { this.finish(error); }
        }

        event(event) { this.emit(`${JSON.stringify(event)}\n`); }

        result(event) {
            this.event({ type: 'result', ...event });
            if (!this.done) this.finish();
        }

        rows(rows) { this.result({ rows }); }

        finish(error = null) {
            if (this.done) return;
            this.done = true;
            this.options.onExit?.(error, this.output);
        }

        stop() {
            this.stops++;
            if (!this.done) this.finish(new Error('Audio operation cancelled.'));
        }

        send(text) {
            assert.equal(this.done, false);
            assert.equal(this.options.interactive, true);
            this.answers.push(text);
            if (this.argv[1] === 'policy') {
                assert.ok(text.endsWith('\n'));
                assert.equal(text.split('\n').length, 2, 'one JSON record per send');
                const value = JSON.parse(text);
                if (!this.request) this.request = value;
            }
        }
    }
    // Defaults are separate, stable proxy objects, as in AstalWp. Resolve them to
    // live endpoints by id; never accidentally mutate a default proxy's volume.
    const audio = new Signals({ devices: cards, speakers: outputs, microphones: inputs, streams, recorders,
        default_speaker: new Signals({ id: outputs[0]?.id || 0 }),
        default_microphone: new Signals({ id: inputs[0]?.id || 0 }) });
    const bindNode = (node, kind = 'output') => {
        const proxy = kind === 'output' ? 'speaker' : 'microphone';
        node.defaultValue = audio[`default_${proxy}`].id === node.id;
        node.onDefault = value => {
            if (!value) return;
            for (const other of kind === 'output' ? audio.speakers : audio.microphones)
                other.defaultValue = other === node;
            audio[`default_${proxy}`].id = node.id;
            audio.notify(`default-${proxy}`);
        };
        return node;
    };
    outputs.forEach(node => bindNode(node));
    inputs.forEach(node => bindNode(node, 'input'));
    const wp = new Signals({ audio });
    const bluetooth = new Signals({ adapter: radio, devices });
    let changes = 0;
    const { AudioBackend, PolicyProcess } = vm.runInNewContext(`${source}\n({ AudioBackend, PolicyProcess });`, {
        ...logic, GLib: timers.GLib, Gio: {}, GObject, HelperProcess, TextEncoder,
        Wp: { Available: { NO, YES }, get_default: () => wp },
        Bluetooth: { get_default: () => bluetooth },
    }, { filename: 'backend.js' });
    const backend = new AudioBackend({ onChange: () => changes++ });
    t.after(() => backend.dispose());
    const listing = kind => backend.metadata[kind].listing?.worker;
    const policy = (request, callbacks = {}) => {
        const exits = [], actions = [], snapshots = [];
        const process = new PolicyProcess('/test/audio-control', request, {
            onSnapshot: () => { const snapshot = backend.nativeSnapshot(); snapshots.push(snapshot); return snapshot; },
            onAction: event => { actions.push(event); },
            onExit: (error, result) => exits.push({ error, result }), ...callbacks,
        });
        t.after(() => { if (!process.stopped) process.stop(); });
        return { process, worker: process.worker, exits, actions, snapshots };
    };
    return { backend, audio, wp, bluetooth, radio, outputs, inputs, cards, streams, recorders, helpers, timers, listing, policy, bindNode,
        snapshot: () => plain(backend.snapshot()), changes: () => changes,
        async tick(milliseconds = 100) { timers.advance(milliseconds); await settle(); },
        load(kind, entries) { timers.advance(); listing(kind).rows(entries); } };
}

function transactionFixture(t, { streams = [], recorders = [] } = {}) {
    const selectedCard = card();
    const previousCard = card({ id: 8, serial: '80', pwName: 'alsa_card.previous' });
    const previous = endpoint({ card: previousCard });
    const input = endpoint({ id: 20, card: selectedCard, pwName: 'alsa_input.test' });
    const f = fixture(t, { outputs: [previous], inputs: [input], cards: [selectedCard, previousCard], streams, recorders });
    f.backend.start();
    const entry = choice({ nativeCard: selectedCard, name: '', nodeId: null, nodeSerial: '' });
    f.load('output', [entry]);
    const context = { kind: 'output', entry, initial: plain(f.backend.nativeSnapshot()) };
    const addOutput = (properties = {}) => {
        const node = f.bindNode(endpoint({ id: 30, card: selectedCard, pwName: 'alsa_output.new',
            routes: [{ index: 0, name: 'headphones', available: YES }], ...properties }));
        f.audio.speakers.push(node);
        f.audio.notify('speakers');
        return node;
    };
    return { ...f, selectedCard, previousCard, previous, input, entry, context, addOutput };
}

for (const failRoute of [false, true]) {
    test(`inactive output policy executes native profile/route/default ${failRoute ? 'and Rust-requested recovery after an API error' : 'forward actions'}`, async t => {
        const f = transactionFixture(t);
        f.selectedCard.acceptProfile = false;
        f.backend.select('output', f.entry.key);
        const worker = f.helpers.at(-1);
        f.timers.advance();
        assert.deepEqual(worker.request, { command: 'select', kind: 'output', key: f.entry.key, snapshot: f.context.initial });
        worker.event({ type: 'snapshot', id: 1 });
        assert.deepEqual(JSON.parse(worker.answers.at(-1)), { id: 1, snapshot: f.context.initial });
        worker.event(action('profile', f.selectedCard, { id: 2, profile: 1 }));
        await settle();
        assert.deepEqual(f.selectedCard.profileWrites, [1]);
        assert.equal(worker.answers.length, 2, 'profile acknowledgement waits for native convergence');
        f.timers.advance(100);
        await settle();
        assert.equal(worker.answers.length, 2);
        f.selectedCard.active_profile_id = 1;
        f.selectedCard.notify('active-profile-id');
        const target = f.addOutput();
        f.timers.advance(100);
        await settle();
        assert.deepEqual(JSON.parse(worker.answers.at(-1)), { id: 2, ok: true });
        worker.event({ type: 'snapshot', id: 3 });
        assert.ok(JSON.parse(worker.answers.at(-1)).snapshot.nodes.some(node => node.id === target.id && node.cardSerial === '70'));
        if (failRoute) target.set_route = () => { throw new Error('Astal route write failed'); };
        worker.event(action('route', target, { id: 4, route: 0 }));
        await settle();
        assert.deepEqual(JSON.parse(worker.answers.at(-1)), failRoute
            ? { id: 4, ok: false, error: 'Astal route write failed' } : { id: 4, ok: true });
        assert.equal(f.backend.metadata.output.busy, true);
        assert.equal(worker.done, false, 'native errors leave Rust alive to decide rollback');
        worker.event({ type: 'snapshot', id: 5 });
        if (failRoute) {
            f.selectedCard.acceptProfile = true;
            worker.event(action('profile', f.selectedCard, { id: 6, profile: 0 }));
            await settle();
            f.timers.advance(100);
            await settle();
            assert.deepEqual(JSON.parse(worker.answers.at(-1)), { id: 6, ok: true });
            assert.deepEqual(f.selectedCard.profileWrites, [1, 0]);
            worker.event(action('default', f.previous, { id: 7 }));
        } else {
            assert.deepEqual(target.routeWrites, [target.routes[0]]);
            assert.deepEqual(target.defaultWrites, []);
            worker.event(action('default', target, { id: 6 }));
        }
        await settle();
        const count = worker.answers.length;
        assert.equal(f.backend.waiters.size, 1);
        f.timers.advance(100);
        await settle();
        assert.equal(worker.answers.length, count + 1);
        assert.deepEqual(JSON.parse(worker.answers.at(-1)), { id: failRoute ? 7 : 6, ok: true });
        worker.result(failRoute ? { ok: false, error: 'Selection failed; previous profile/default restored' } : { ok: true });
        await settle();
        assert.equal(f.snapshot().output.busy, false);
        assert.equal(f.snapshot().output.error, failRoute ? 'Selection failed; previous profile/default restored' : '');
        assert.deepEqual((failRoute ? f.previous : target).defaultWrites, [true]);
        assert.deepEqual(f.previousCard.profileWrites, []);
        assert.equal(f.backend.waiters.size, 0);
        assert.ok(f.helpers.every(helper => helper.argv[1] === 'policy'), 'no native mutation CLI exists');
    });
}

test('policy route acknowledgement does not trust the card-wide route getter or wait for it', async t => {
    const f = transactionFixture(t);
    const target = f.addOutput({ acceptRoute: false, route: { name: 'other-card-wide-route' } });
    await f.backend.applyPolicyAction(action('route', target, { route: 0 }), f.context);
    assert.deepEqual(target.routeWrites, [target.routes[0]]);
    assert.equal(target.route.name, 'other-card-wide-route');
    assert.equal(f.backend.waiters.size, 0, 'Rust verifies omitted Route.device associations');
    assert.deepEqual(target.defaultWrites, []);
});

for (const [label, arrange, expected] of [
    ['wrong-card profile', f => action('profile', f.previousCard, { profile: 1 }), /unrelated profile/],
    ['input profile', f => { f.context.kind = 'input'; return action('profile', f.selectedCard, { profile: 1 }); }, /unrelated profile/],
    ['plain-selection profile', f => { f.context.entry.port = false; return action('profile', f.selectedCard, { profile: 1 }); }, /unrelated profile/],
    ['missing profile', f => action('profile', f.selectedCard, { profile: 99 }), /profile is no longer available/],
    ['missing selected card', f => { f.audio.devices = [f.previousCard]; f.input.card = null; f.target.card = null; return action('profile', f.selectedCard, { profile: 1 }); }, /sound card disappeared/],
    ['changed card serial', f => { const event = action('profile', f.selectedCard, { profile: 1 }); f.selectedCard.serial = '999'; return event; }, /sound card disappeared/],
    ['wrong-card route', f => action('route', f.previous, { route: 0 }), /unrelated physical audio port/],
    ['input node for output', f => action('route', f.input, { route: 0 }), /endpoint disappeared/],
    ['missing route index', f => action('route', f.target, { route: 9 }), /route is no longer available/],
    ['wrong route name', f => { f.target.routes[0].name = 'speakers'; return action('route', f.target, { route: 0 }); }, /route is no longer available/],
    ['unavailable route', f => { f.target.routes[0].available = NO; return action('route', f.target, { route: 0 }); }, /route is no longer available/],
    ['plain-selection route', f => { f.context.entry.port = false; return action('route', f.target, { route: 0 }); }, /unrelated physical audio port/],
    ['reused node id', f => action('default', f.target, { node: { id: f.target.id, serial: '999' } }), /endpoint disappeared/],
    ['new unrelated default', f => { f.target.card = f.previousCard; return action('default', f.target); }, /unrelated default audio endpoint/],
    ['input default for output', f => action('default', f.input), /endpoint disappeared/],
]) {
    test(`policy executor rejects ${label} before any native write`, async t => {
        const f = transactionFixture(t);
        f.target = f.addOutput();
        const event = arrange(f);
        await assert.rejects(f.backend.applyPolicyAction(event, f.context), expected);
        for (const node of [f.previous, f.input, f.target]) {
            assert.deepEqual(node.routeWrites, []);
            assert.deepEqual(node.defaultWrites, []);
        }
        assert.deepEqual(f.selectedCard.profileWrites, []);
        assert.deepEqual(f.previousCard.profileWrites, []);
        assert.equal(f.backend.waiters.size, 0);
    });
}

for (const operation of ['profile', 'default']) {
    test(`policy ${operation} convergence is bounded and reports failure back to Rust`, async t => {
        const f = transactionFixture(t);
        const target = f.addOutput({ acceptDefault: false });
        f.selectedCard.acceptProfile = false;
        f.backend.select('output', f.entry.key);
        const worker = f.helpers.at(-1);
        f.timers.advance();
        worker.event(operation === 'profile' ? action('profile', f.selectedCard, { profile: 1 }) : action('default', target));
        await settle();
        const before = worker.answers.length;
        f.timers.advance(2900);
        await settle();
        assert.equal(worker.answers.length, before);
        f.timers.advance(100);
        await settle();
        assert.deepEqual(JSON.parse(worker.answers.at(-1)), { id: 1, ok: false, error: 'The audio device did not accept the change.' });
        assert.equal(f.backend.waiters.size, 0);
        assert.equal(worker.done, false);
        worker.result({ ok: false, error: 'Native convergence failed' });
        await settle();
        assert.equal(f.snapshot().output.error, 'Native convergence failed');
        assert.equal(f.snapshot().output.busy, false);
        assert.equal(f.helpers.length, 3);
    });
}

for (const change of ['card replacement', 'card serial', 'node replacement', 'node serial', 'node removal']) {
    test(`policy wait rejects ${change} instead of acknowledging an identity-reused object`, async t => {
        const f = transactionFixture(t);
        const target = f.addOutput();
        const profile = change.startsWith('card');
        const promise = f.backend.applyPolicyAction(profile
            ? action('profile', f.selectedCard, { profile: 1 }) : action('default', target), f.context);
        const rejected = assert.rejects(promise, profile ? /sound card disappeared/ : /endpoint disappeared/);
        if (change === 'card replacement') {
            const replacement = card();
            f.audio.devices = [replacement, f.previousCard];
            f.input.card = replacement;
            target.card = replacement;
        } else if (change === 'card serial') f.selectedCard.serial = '999';
        else if (change === 'node replacement') f.audio.speakers = [f.previous, endpoint({ id: target.id, serial: target.serial, card: f.selectedCard })];
        else if (change === 'node serial') target.serial = '999';
        else f.audio.speakers = [f.previous];
        f.timers.advance(100);
        await rejected;
        assert.equal(f.backend.waiters.size, 0);
    });
}

test('policy recovery may default an initial unrelated node or a recreated endpoint on the selected card, not a new unrelated one', async t => {
    const f = transactionFixture(t);
    const recreated = f.addOutput();
    for (const node of [recreated, f.previous]) {
        const promise = f.backend.applyPolicyAction(action('default', node), f.context);
        assert.deepEqual(node.defaultWrites, [true]);
        f.timers.advance(100);
        await promise;
    }
    const unrelated = f.addOutput({ id: 40, card: f.previousCard });
    await assert.rejects(f.backend.applyPolicyAction(action('default', unrelated), f.context), /unrelated default/);
    assert.deepEqual(unrelated.defaultWrites, []);
    f.backend.dispose();
    await assert.rejects(f.backend.applyPolicyAction(action('profile', f.selectedCard, { profile: 1 }), f.context), /panel has stopped/);
});

test('disposing a pending policy action cancels its waiter and suppresses the late acknowledgement', async t => {
    const f = transactionFixture(t);
    f.selectedCard.acceptProfile = false;
    f.backend.select('output', f.entry.key);
    const worker = f.helpers.at(-1);
    f.timers.advance();
    worker.event(action('profile', f.selectedCard, { profile: 1 }));
    await settle();
    assert.equal(f.backend.waiters.size, 1);
    f.backend.dispose();
    await settle();
    f.timers.advance();
    assert.equal(f.backend.waiters.size, 0);
    assert.equal(f.timers.sources.size, 0);
    assert.equal(worker.stops, 1);
    assert.equal(worker.answers.length, 1);
    assert.equal(f.backend.metadata.output.busy, false);
});

test('canonical snapshot uses native cards, identities, profile indexes and route availability only', t => {
    const nativeCard = card({ profiles: [{ index: 0, name: 'off' }, { index: 9, description: 'Duplex' }], active_profile_id: 9 });
    const inactive = card({ id: 8, serial: '80', pwName: 'alsa_card.inactive' });
    const routes = [{ index: 0, name: 'headphones', description: 'Headphones', available: YES },
        { index: 1, name: 'speaker', description: '', available: NO },
        { index: 2, name: 'unknown', available: 0 }];
    const output = endpoint({ card: nativeCard, profileDevice: '0', routes, route: routes[0] });
    const input = endpoint({ id: 20, card: nativeCard, profileDevice: 'not-a-number', pwName: 'alsa_input.test', description: '', nick: '' });
    const f = fixture(t, { outputs: [output], inputs: [input], cards: [nativeCard, inactive, nativeCard] });
    f.backend.start();
    const expected = {
        cards: [{ id: 7, serial: '70', name: 'alsa_card.test', activeProfile: 9, profiles: [{ index: 0 }, { index: 9 }] },
            { id: 8, serial: '80', name: 'alsa_card.inactive', activeProfile: 0, profiles: [{ index: 0 }, { index: 1 }] }],
        nodes: [{ id: 10, serial: '100', kind: 'output', name: 'alsa_output.test', description: 'Speakers',
            cardId: 7, cardSerial: '70', profileDevice: 0, routeName: 'headphones', routes: [
                { index: 0, name: 'headphones', description: 'Headphones', available: true },
                { index: 1, name: 'speaker', description: 'speaker', available: false },
                { index: 2, name: 'unknown', description: 'unknown', available: true },
            ] }, { id: 20, serial: '200', kind: 'input', name: 'alsa_input.test', description: 'alsa_input.test',
            cardId: 7, cardSerial: '70', profileDevice: null, routeName: '', routes: [] }],
        defaultOutput: { id: 10, serial: '100' }, defaultInput: { id: 20, serial: '200' },
    };
    assert.deepEqual(plain(f.backend.nativeSnapshot()), expected, 'matches the deny_unknown_fields Rust model');
    f.timers.advance();
    for (const worker of f.helpers) assert.deepEqual(worker.request.snapshot, expected);
    assert.equal(nativeCard.handlers.size, 1);
    assert.equal(inactive.handlers.size, 1, 'audio.devices includes cards without an active endpoint');
    inactive.notify('profiles');
    assert.ok(f.backend.refreshTimer);
    f.audio.devices = [nativeCard];
    f.audio.notify('devices');
    assert.equal(inactive.handlers.size, 0);
    assert.deepEqual(nativeCard.profileWrites, []);
});

test('snapshot discovers endpoint cards and omits unresolved native defaults and identities', t => {
    const nativeCard = card();
    const output = endpoint({ card: nativeCard, profileDevice: '2' });
    const invalid = endpoint({ id: 11, serial: '' });
    const f = fixture(t, { outputs: [output, invalid], inputs: [] });
    f.backend.start();
    assert.equal(f.audio.devices.length, 0);
    assert.equal(f.backend.nativeSnapshot().cards.length, 1);
    assert.equal(f.backend.nativeSnapshot().nodes.length, 1);
    f.audio.default_speaker.id = 999;
    assert.equal(f.backend.nativeSnapshot().defaultOutput, null);
    assert.equal(f.backend.nativeSnapshot().defaultInput, null);
});

test('PolicyProcess defers and frames the canonical rows request and requires a typed streamed result', t => {
    const f = fixture(t);
    const request = { command: 'rows', kind: 'output', snapshot: plain(f.backend.nativeSnapshot()) };
    const p = f.policy(request);
    assert.deepEqual(p.worker.answers, []);
    f.timers.advance();
    assert.deepEqual(p.worker.argv, ['/test/audio-control', 'policy']);
    assert.equal(p.worker.options.interactive, true);
    assert.equal(p.worker.options.timeout, 12000);
    assert.deepEqual(p.worker.answers, [`${JSON.stringify(request)}\n`]);
    const rows = [choice({ port: false, portName: '', routeId: null })];
    p.worker.event({ type: 'result', rows });
    assert.equal(p.exits.length, 0, 'a result does not imply child exit');
    p.worker.finish();
    assert.equal(p.exits.length, 1);
    assert.equal(p.exits[0].error, null);
    assert.deepEqual(plain(p.exits[0].result), { type: 'result', rows });
});

test('PolicyProcess returns fresh snapshots and acknowledges native actions with sequential RPC ids', async t => {
    const f = fixture(t);
    f.backend.start();
    const p = f.policy({ command: 'select', kind: 'output', key: 'selected', snapshot: plain(f.backend.nativeSnapshot()) });
    f.timers.advance();
    p.worker.event({ type: 'snapshot', id: 1 });
    assert.deepEqual(JSON.parse(p.worker.answers[1]), { id: 1, snapshot: plain(f.backend.nativeSnapshot()) });
    f.outputs[0].description = 'Updated';
    p.worker.event({ type: 'snapshot', id: 2 });
    assert.equal(JSON.parse(p.worker.answers[2]).snapshot.nodes[0].description, 'Updated');
    p.worker.event(action('default', f.outputs[0], { id: 3 }));
    assert.equal(p.worker.answers.length, 3, 'action acknowledgement waits for its promise');
    await settle();
    assert.deepEqual(JSON.parse(p.worker.answers[3]), { id: 3, ok: true });
    assert.equal(p.actions.length, 1);
    p.worker.result({ ok: true });
    assert.equal(p.exits[0].error, null);
    assert.equal(p.process.rpc, 3);
});

for (const [label, event] of [
    ['malformed JSON', '{'], ['untyped rows', '[]'], ['unknown type', { type: 'write', id: 1 }],
    ['extra field', { type: 'snapshot', id: 1, command: 'set-default' }],
    ['zero id', { type: 'snapshot', id: 0 }], ['skipped id', { type: 'snapshot', id: 2 }],
    ['fractional id', { type: 'snapshot', id: 1.5 }], ['string id', { type: 'snapshot', id: '1' }],
    ['out-of-range id', { type: 'snapshot', id: 257 }],
    ['invalid identity', { type: 'action', id: 1, operation: 'default', node: { id: 10, serial: '' } }],
    ['unknown native operation', { type: 'action', id: 1, operation: 'volume', node: { id: 10, serial: '100' } }],
    ['invalid rows', { type: 'result', rows: [{ key: 'invalid' }] }],
    ['invalid result', { type: 'result', ok: false, error: '' }],
    ['result extra field', { type: 'result', ok: true, mutation: 'default' }],
]) {
    test(`PolicyProcess rejects ${label} without dispatching a mutation`, async t => {
        const f = fixture(t);
        const p = f.policy({ command: 'select', kind: 'output', key: 'selected', snapshot: plain(f.backend.nativeSnapshot()) });
        f.timers.advance();
        p.worker.emit(`${typeof event === 'string' ? event : JSON.stringify(event)}\n`);
        await settle();
        assert.equal(p.worker.done, true);
        assert.equal(p.exits.length, 1);
        assert.ok(p.exits[0].error);
        assert.deepEqual(p.actions, []);
        assert.equal(p.worker.answers.length, 1);
    });
}

for (const event of [{ type: 'snapshot', id: 1 }, action('default', { id: 10, serial: '100' })]) {
    test(`rows policy cannot request a ${event.type} RPC`, async t => {
        const f = fixture(t);
        const p = f.policy({ command: 'rows', kind: 'output', snapshot: plain(f.backend.nativeSnapshot()) });
        f.timers.advance();
        p.worker.event(event);
        await settle();
        assert.match(p.exits[0].error.message, /Unexpected audio policy request/);
        assert.equal(p.actions.length, 0);
    });
}

for (const duplicate of ['rpc', 'result']) {
    test(`PolicyProcess rejects duplicate ${duplicate} records`, t => {
        const f = fixture(t);
        const p = f.policy({ command: 'select', kind: 'output', key: 'selected', snapshot: plain(f.backend.nativeSnapshot()) });
        f.timers.advance();
        const event = duplicate === 'rpc' ? { type: 'snapshot', id: 1 } : { type: 'result', ok: true };
        p.worker.emit(`${JSON.stringify(event)}\n${JSON.stringify(event)}\n`);
        assert.ok(p.exits[0].error);
    });
}

for (const next of [{ type: 'snapshot', id: 2 }, { type: 'result', ok: true }]) {
    test(`PolicyProcess rejects ${next.type} while a native action is pending`, async t => {
        const f = fixture(t);
        let resolve;
        const pending = new Promise(done => { resolve = done; });
        const p = f.policy({ command: 'select', kind: 'output', key: 'selected', snapshot: plain(f.backend.nativeSnapshot()) },
            { onAction: () => pending });
        f.timers.advance();
        p.worker.event(action('default', f.outputs[0]));
        await settle();
        p.worker.event(next);
        assert.match(p.exits[0].error.message, /overlapping/);
        resolve();
        await settle();
        assert.equal(p.worker.answers.length, 1, 'no acknowledgement after transport failure');
        assert.equal(p.exits.length, 1);
    });
}

for (const reject of [false, true]) {
    test(`PolicyProcess ignores a late ${reject ? 'rejection' : 'acknowledgement'} after cancellation`, async t => {
        const f = fixture(t);
        let complete;
        const pending = new Promise((resolve, failure) => { complete = reject ? failure : resolve; });
        const p = f.policy({ command: 'select', kind: 'output', key: 'selected', snapshot: plain(f.backend.nativeSnapshot()) },
            { onAction: () => pending });
        f.timers.advance();
        p.worker.event(action('default', f.outputs[0]));
        await settle();
        p.process.stop();
        complete(new Error('Late native failure'));
        await settle();
        assert.equal(p.worker.answers.length, 1);
        assert.equal(p.exits.length, 1);
        assert.match(p.exits[0].error.message, /cancelled/);
    });
}

for (const command of ['rows', 'select']) {
    for (const result of [null, command === 'rows' ? { ok: true } : { rows: [] }]) {
        test(`${command} rejects ${result ? 'the wrong result variant' : 'exit before a typed result'}`, t => {
            const f = fixture(t);
            const p = f.policy({ command, kind: 'output', ...(command === 'select' ? { key: 'selected' } : {}), snapshot: plain(f.backend.nativeSnapshot()) });
            f.timers.advance();
            if (result) p.worker.result(result); else p.worker.finish();
            assert.match(p.exits[0].error.message, /Incomplete audio policy response/);
        });
    }
}

test('PolicyProcess bounds UTF-8 request bytes, reports send failures and does not send after an early stop', t => {
    const f = fixture(t);
    f.backend.start();
    const snapshot = plain(f.backend.nativeSnapshot());
    snapshot.nodes[0].description = 'é'.repeat(524288);
    const p = f.policy({ command: 'rows', kind: 'output', snapshot });
    assert.equal(p.exits.length, 0);
    f.timers.advance();
    assert.match(p.exits[0].error.message, /request is too large/);
    assert.deepEqual(p.worker.answers, []);
    const stopped = f.policy({ command: 'rows', kind: 'output', snapshot: plain(f.backend.nativeSnapshot()) });
    stopped.process.stop();
    f.timers.advance();
    assert.deepEqual(stopped.worker.answers, []);
    const broken = f.policy({ command: 'rows', kind: 'output', snapshot: plain(f.backend.nativeSnapshot()) });
    broken.worker.send = () => { throw new Error('Pipe closed'); };
    f.timers.advance();
    assert.equal(broken.exits[0].error.message, 'Pipe closed');
});

test('PolicyProcess accepts sequential ids through 256 and rejects an RPC beyond the transaction limit', t => {
    const f = fixture(t);
    const p = f.policy({ command: 'select', kind: 'output', key: 'selected', snapshot: plain(f.backend.nativeSnapshot()) });
    f.timers.advance();
    for (let id = 1; id <= 256; id++) {
        p.worker.event({ type: 'snapshot', id });
        assert.equal(JSON.parse(p.worker.answers.at(-1)).id, id);
    }
    assert.equal(p.process.rpc, 256);
    p.worker.event({ type: 'snapshot', id: 257 });
    assert.equal(p.worker.answers.length, 257, 'no reply to an invalid id');
    assert.ok(p.exits[0].error);
});

test('PolicyProcess reports snapshot callback and child-exit failures even after receiving a result', t => {
    const f = fixture(t);
    const request = { command: 'select', kind: 'output', key: 'selected', snapshot: plain(f.backend.nativeSnapshot()) };
    const p = f.policy(request, { onSnapshot: () => { throw new Error('Native snapshot unavailable'); } });
    f.timers.advance();
    p.worker.event({ type: 'snapshot', id: 1 });
    assert.equal(p.exits[0].error.message, 'Native snapshot unavailable');
    const exited = f.policy(request);
    f.timers.advance();
    exited.worker.event({ type: 'result', ok: true });
    exited.worker.finish(new Error('Child failed after producing output'));
    assert.equal(exited.exits[0].error.message, 'Child failed after producing output');
});

test('PolicyProcess forwards bounded native execution errors so Rust can request recovery', async t => {
    const f = fixture(t);
    const p = f.policy({ command: 'select', kind: 'output', key: 'selected', snapshot: plain(f.backend.nativeSnapshot()) },
        { onAction: () => { throw new Error('x'.repeat(9000)); } });
    f.timers.advance();
    p.worker.event(action('default', f.outputs[0]));
    await settle();
    assert.deepEqual(JSON.parse(p.worker.answers[1]), { id: 1, ok: false, error: 'x'.repeat(8192) });
    assert.equal(p.worker.done, false, 'a rejected action does not kill Rust recovery policy');
    p.worker.event({ type: 'snapshot', id: 2 });
    assert.equal(JSON.parse(p.worker.answers[2]).id, 2);
    p.worker.result({ ok: false, error: 'Selection failed after recovery' });
    assert.equal(p.exits[0].error.message, 'Selection failed after recovery');
});

test('snapshot represents missing hardware without inventing defaults or doing work', t => {
    const f = fixture(t, { outputs: [], inputs: [], radio: null });
    const emptyAudio = { nodeKey: '', available: false, name: 'No device', statusName: '',
        volume: 0, muted: false, choices: [], busy: false, loading: false, error: '' };
    assert.deepEqual(f.snapshot(), { output: emptyAudio, input: emptyAudio,
        bluetooth: { available: false, powered: false, devices: [],
            pairing: false, pairingMessage: null, error: '' } });
    assert.equal(JSON.parse(f.backend.status()).class, 'unavailable');
    assert.equal(f.helpers.length, 0);
    assert.equal(f.timers.sources.size, 0);
    assert.equal(f.audio.connections, 0);
});

test('snapshot omits discovery state without starting or stopping native discovery', t => {
    const f = fixture(t);
    f.backend.syncBluetooth();
    for (const discovering of [false, true]) {
        f.radio.discovering = discovering;
        const bluetooth = f.snapshot().bluetooth;
        assert.equal(bluetooth.available, true);
        assert.equal('discovering' in bluetooth, false);
        assert.equal(f.radio.discovering, discovering);
        assert.deepEqual(f.radio.scanCalls, []);
    }
});

test('start is idempotent, subscribes to native objects and coalesces change/refresh notifications', t => {
    const headset = device();
    const f = fixture(t, { devices: [headset] });
    const { backend, audio, wp, bluetooth, radio, timers } = f;
    backend.start();
    backend.start();
    for (const object of [audio, bluetooth, radio, headset, ...f.outputs, ...f.inputs,
        audio.default_speaker, audio.default_microphone]) assert.equal(object.handlers.size, 1);
    assert.equal(wp.handlers.size, 2);
    assert.deepEqual(f.helpers.map(helper => helper.argv), [
        ['/test/audio-control', 'policy'], ['/test/audio-control', 'policy'],
    ]);
    assert.equal(f.snapshot().output.loading, true);
    assert.equal(f.changes(), 0);
    timers.advance();
    assert.equal(f.changes(), 1);
    assert.deepEqual(f.helpers.map(helper => helper.request.command), ['rows', 'rows']);
    assert.deepEqual(f.helpers.map(helper => helper.request.kind), ['output', 'input']);
    for (const helper of f.helpers) {
        assert.equal(helper.options.interactive, true);
        assert.equal(helper.options.timeout, 12000);
        assert.deepEqual(helper.request.snapshot, plain(backend.nativeSnapshot()));
    }
    for (const property of ['volume', 'mute', 'volume-icon', 'channels', 'lock-channels'])
        f.outputs[0].notify(property);
    timers.advance();
    assert.equal(f.changes(), 2);
    assert.equal(backend.refreshTimer, 0, 'meter/control notifications do not query metadata');
    f.load('output', []);
    f.load('input', []);
    f.outputs[0].notify('description');
    audio.notify('speakers');
    wp.emit('ready');
    const refresh = backend.refreshTimer;
    assert.ok(refresh);
    assert.equal([...timers.sources.values()].filter(task => task.interval === 150).length, 1);
    timers.advance(150);
    assert.equal(f.helpers.length, 4);
    assert.equal(backend.refreshTimer, 0);
});

test('resolved unavailable endpoints stay omitted without disabling native default volume', t => {
    const nativeCard = card();
    const route = { index: 3, name: 'speaker', available: 0 };
    const speaker = endpoint({ card: nativeCard, routes: [route], route });
    const hdmi = endpoint({ id: 11, card: nativeCard, pwName: 'alsa_output.hdmi', routes: [route], route });
    const f = fixture(t, { outputs: [speaker, hdmi], cards: [nativeCard] });
    f.backend.start();
    const before = f.snapshot().output;
    const hiddenKey = before.choices.find(row => row.nodeId === hdmi.id).key;
    const row = choice({ node: speaker, key: 'speaker', portName: 'speaker', routeId: 3, portActive: true });
    f.load('output', [row]);
    const model = f.snapshot().output;
    assert.deepEqual(model.choices.map(row => row.key), ['speaker']);
    assert.equal(model.available, true);
    assert.equal(model.nodeKey, before.nodeKey);
    assert.equal(model.volume, 0.4);
    assert.throws(() => f.backend.select('output', hiddenKey), /no longer available/);
    f.backend.setVolume('output', 0.55, model.nodeKey);
    assert.equal(speaker.volume, 0.55);
    assert.equal(hdmi.volume, 0.4);

    const replacement = endpoint({ id: hdmi.id, serial: '111', pwName: hdmi.pwName });
    f.audio.speakers = [speaker, replacement];
    f.audio.notify('speakers');
    assert.ok(f.snapshot().output.choices.some(row => row.nodeId === replacement.id && row.nodeSerial === '111'),
        'an unseen native identity still has a fallback while its metadata loads');
});

test('stale helper omissions cannot suppress current native fallback choices', t => {
    const f = fixture(t);
    f.backend.start();
    f.timers.advance();
    const listing = f.listing('output');
    f.outputs[0].notify('description');
    listing.rows([]);
    assert.deepEqual(plain(f.backend.metadata.output.resolvedNodes), []);
    assert.equal(f.snapshot().output.choices.length, 1);
    assert.match(f.snapshot().output.choices[0].key, /^native:/);
});

test('default proxy resolution keeps keys stable, but A -> B -> A invalidates old control keys', t => {
    const a = endpoint(), b = endpoint({ id: 11, serial: '101', pwName: 'other' });
    const f = fixture(t, { outputs: [a, b] });
    f.backend.start();
    const first = f.snapshot();
    const endpointKey = f.backend.endpointKey(a);
    assert.equal(first.output.name, 'Desk');
    a.volume = 0.63;
    a.notify('volume');
    assert.equal(f.snapshot().output.nodeKey, first.output.nodeKey);
    assert.equal(f.snapshot().output.volume, 0.63);
    assert.equal(f.audio.default_speaker.volume, undefined);
    // Replacing the proxy must not invalidate a key for the same live endpoint.
    const oldProxy = f.audio.default_speaker;
    f.audio.default_speaker = new Signals({ id: a.id });
    f.audio.notify('default-speaker');
    assert.equal(oldProxy.handlers.size, 0);
    assert.equal(f.snapshot().output.nodeKey, first.output.nodeKey);
    f.audio.default_speaker.id = b.id;
    f.audio.default_speaker.notify('id');
    const second = f.snapshot().output.nodeKey;
    assert.notEqual(second, first.output.nodeKey);
    f.audio.default_speaker.id = a.id;
    f.audio.default_speaker.notify('id');
    const third = f.snapshot();
    assert.notEqual(third.output.nodeKey, first.output.nodeKey);
    assert.notEqual(third.output.nodeKey, second);
    assert.equal(f.backend.endpointKey(a), endpointKey);
    assert.equal(third.input.nodeKey, first.input.nodeKey);
    for (const key of [first.output.nodeKey, second]) {
        assert.throws(() => f.backend.setVolume('output', 0.9, key), /default audio device changed/);
        assert.throws(() => f.backend.toggleMute('output', key), /default audio device changed/);
    }
    assert.equal(a.volume, 0.63);
    assert.equal(b.volume, 0.4);
    assert.equal(a.mute, false);
});

test('endpoint id/serial reuse and removal invalidate both control and native selection keys', t => {
    const f = fixture(t);
    f.backend.start();
    const old = f.outputs[0], before = f.snapshot().output;
    const oldEndpointKey = f.backend.endpointKey(old);
    // Even an identical PipeWire id and serial cannot identify a new GI object.
    const replacement = endpoint({ id: old.id, serial: old.serial });
    f.audio.speakers = [replacement];
    f.audio.notify('speakers');
    assert.equal(old.handlers.size, 0);
    assert.equal(replacement.handlers.size, 1);
    const replacementKey = f.backend.endpointKey(replacement);
    assert.notEqual(replacementKey, oldEndpointKey);
    assert.notEqual(f.snapshot().output.nodeKey, before.nodeKey);
    assert.throws(() => f.backend.setVolume('output', 1, before.nodeKey), /default audio device changed/);
    assert.throws(() => f.backend.select('output', before.choices[0].key), /no longer available/);
    f.audio.speakers = [];
    f.audio.notify('speakers');
    assert.equal(replacement.handlers.size, 0);
    assert.equal(f.snapshot().output.available, false);
    assert.equal(f.snapshot().output.nodeKey, '');
    f.audio.speakers = [replacement];
    f.audio.notify('speakers');
    assert.notEqual(f.backend.endpointKey(replacement), replacementKey);
    const readded = f.snapshot().output;
    replacement.serial = '999';
    replacement.notify('serial');
    assert.notEqual(f.snapshot().output.nodeKey, readded.nodeKey);
    assert.throws(() => f.backend.setVolume('output', 1, readded.nodeKey), /default audio device changed/);
    const key = f.snapshot().output.nodeKey;
    for (const id of [0, -1, 999]) {
        f.audio.default_speaker.id = id;
        f.audio.default_speaker.notify('id');
        assert.equal(f.snapshot().output.available, false);
        assert.throws(() => f.backend.toggleMute('output', key), /default audio device changed/);
    }
});

for (const [kind, max] of [['output', 1.5], ['input', 1]]) {
    test(`${kind} volume/mute affect only the current endpoint and enforce limits and keys`, t => {
        const f = fixture(t);
        f.backend.start();
        const model = f.snapshot(), node = kind === 'output' ? f.outputs[0] : f.inputs[0];
        const key = model[kind].nodeKey;
        for (const [value, expected] of [[-5, 0], [10, max], [0.12345, 0.12345]]) {
            f.backend.setVolume(kind, value, key);
            assert.equal(node.volume, expected);
        }
        f.backend.toggleMute(kind, key);
        assert.equal(node.mute, true);
        f.backend.toggleMute(kind, key);
        assert.equal(node.mute, false);
        for (const value of [NaN, Infinity, -Infinity, '0.5', null, undefined])
            assert.throws(() => f.backend.setVolume(kind, value, key), /Invalid audio volume/);
        for (const stale of ['', 'stale', undefined, model[kind === 'output' ? 'input' : 'output'].nodeKey]) {
            assert.throws(() => f.backend.setVolume(kind, 0.8, stale), /default audio device changed/);
            assert.throws(() => f.backend.toggleMute(kind, stale), /default audio device changed/);
        }
        assert.throws(() => f.backend.setVolume('invalid', 1, key), /Invalid audio volume/);
        assert.throws(() => f.backend.toggleMute('invalid', key), /default audio device changed/);
        assert.equal(node.volume, 0.12345);
        assert.equal(node.mute, false);
        assert.equal((kind === 'output' ? f.inputs[0] : f.outputs[0]).volume, kind === 'output' ? 0.7 : 0.4);
        f.backend.dispose();
        assert.throws(() => f.backend.setVolume(kind, 0.5, key), /panel has stopped/);
        assert.throws(() => f.backend.toggleMute(kind, key), /panel has stopped/);
    });
}

for (const kind of ['output', 'input']) {
    test(`${kind} native route selection preserves route index zero and waits before setting the default`, async t => {
        const route = { index: 0, name: 'headphones', available: YES };
        const node = endpoint({ id: 11, pwName: `${kind}.target`, routes: [route], route: { name: 'speakers' } });
        const initial = endpoint({ id: 12, pwName: `${kind}.initial` });
        const f = fixture(t, kind === 'output' ? { outputs: [initial, node] } : { inputs: [initial, node] });
        f.backend.start();
        const before = f.snapshot()[kind].nodeKey;
        const row = choice({ node, routeId: route.index });
        f.load(kind, [row]);
        assert.equal(f.snapshot()[kind].choices.find(entry => entry.key === row.key).default, false);
        f.backend.select(kind, row.key);
        assert.deepEqual(node.routeWrites, [route]);
        assert.deepEqual(node.defaultWrites, []);
        assert.equal(f.snapshot()[kind].busy, true);
        f.backend.select(kind, 'ignored-while-busy');
        assert.equal(f.helpers.length, 2, 'native actions do not launch a policy selection');
        f.timers.advance(100);
        await settle();
        assert.deepEqual(node.defaultWrites, [true]);
        assert.equal(f.snapshot()[kind].busy, true);
        f.timers.advance(100);
        await settle();
        assert.equal(f.snapshot()[kind].busy, false);
        assert.equal(f.snapshot()[kind].error, '');
        assert.equal(f.snapshot()[kind].choices.find(entry => entry.key === row.key).default, true);
        assert.notEqual(f.snapshot()[kind].nodeKey, before);
        assert.deepEqual(initial.defaultWrites, []);
        assert.equal(f.backend.waiters.size, 0);
    });
}

for (const operation of ['route', 'default']) {
    test(`native ${operation} API exceptions report an error without delegating a write to Rust`, async t => {
        const route = { index: 0, name: 'headphones', available: YES };
        const node = endpoint({ routes: [route] });
        const f = fixture(t, { outputs: [node] });
        f.backend.start();
        const row = choice({ node, port: operation === 'route', portName: operation === 'route' ? 'headphones' : '',
            routeId: operation === 'route' ? 0 : null });
        f.load('output', [row]);
        const error = new Error(`Astal ${operation} failed`);
        if (operation === 'route') node.set_route = () => { throw error; };
        else node.defaultError = error;
        f.backend.select('output', row.key);
        await settle();
        assert.equal(f.snapshot().output.error, error.message);
        assert.equal(f.snapshot().output.busy, false);
        assert.equal(f.helpers.length, 2);
        assert.deepEqual(node.defaultWrites, []);
        assert.equal(f.backend.waiters.size, 0);
    });
}

for (const reliable of [false, true]) {
    test(`defaults come from Astal, with ${reliable ? 'native' : 'policy-associated'} route activity`, t => {
        const nativeCard = card();
        const route = { index: 0, name: 'headphones', available: YES };
        const first = endpoint({ card: nativeCard, routes: [route], route });
        const second = endpoint({ id: 11, card: reliable ? null : nativeCard, routes: [route], route });
        const f = fixture(t, { outputs: [first, second] });
        f.backend.start();
        const rows = [choice({ node: first, key: 'first', portActive: false, default: true }),
            choice({ node: second, key: 'second', portActive: true, default: true })];
        f.load('output', rows);
        assert.deepEqual(f.snapshot().output.choices.map(row => row.default), [reliable, false]);
        assert.equal('default' in f.backend.metadata.output.entries[0], false, 'Choice stores portActive, not a Rust default');
        f.backend.metadata.output.entries[0].portActive = true;
        assert.deepEqual(f.snapshot().output.choices.map(row => row.default), [true, false]);
        f.audio.default_speaker.id = second.id;
        f.audio.default_speaker.notify('id');
        assert.deepEqual(f.snapshot().output.choices.map(row => row.default), [false, true], 'changing Astal default requires no policy result');
        assert.equal(f.snapshot().output.statusName, 'Headphones');
        for (const node of [first, second]) assert.deepEqual(node.defaultWrites, []);
    });
}

for (const publicName of [null, 'Application media title']) {
    test(`native route selection uses PipeWire node.name when public name is ${JSON.stringify(publicName)}`, async t => {
        const route = { index: 4, name: 'headphones', available: YES };
        const node = endpoint({ pwName: 'alsa_output.pinned', name: publicName, routes: [route] });
        const f = fixture(t, { outputs: [node] });
        f.backend.start();
        const row = choice({ node, routeId: route.index });
        f.load('output', [row]);
        assert.notEqual(node.name, node.get_pw_property('node.name'));
        assert.equal(f.backend.endpointModels('output')[0].name, node.pwName);
        assert.deepEqual(f.snapshot().output.choices.map(entry => entry.key), [row.key]);
        f.backend.select('output', row.key);
        assert.deepEqual(node.routeWrites, [route]);
        assert.equal(f.helpers.length, 2, 'native routing never falls back to a Rust write');
        f.timers.advance(100);
        await settle();
        assert.deepEqual(node.defaultWrites, [true]);
        f.timers.advance(100);
        await settle();
        assert.equal(f.snapshot().output.busy, false);
        assert.equal(f.snapshot().output.error, '');
        assert.equal(f.snapshot().output.choices[0].default, true);
    });
}

for (const pwName of ['', 'alsa_output.duplicate']) {
    test(`native fallback rows select by key even with ${pwName ? 'duplicate' : 'empty'} PipeWire names`, async t => {
        const nativeCard = card();
        const initial = endpoint({ id: 31, pwName, card: nativeCard });
        const target = endpoint({ id: 32, pwName, card: nativeCard });
        const f = fixture(t, { outputs: [initial, target] });
        f.backend.start();
        const choices = f.snapshot().output.choices;
        assert.equal(choices.length, 2);
        assert.equal(choices[1].key, `native:${f.backend.endpointKey(target)}`);
        assert.deepEqual(choices.map(entry => entry.name), [pwName, pwName]);
        f.backend.select('output', choices[1].key);
        assert.equal(f.helpers.length, 2, 'native fallback rows do not launch policy selections');
        assert.deepEqual(initial.defaultWrites, []);
        assert.deepEqual(target.defaultWrites, [true]);
        assert.deepEqual(target.routeWrites, []);
        f.timers.advance(100);
        await settle();
        assert.equal(f.snapshot().output.busy, false);
        assert.equal(f.snapshot().output.error, '');
        assert.equal(f.snapshot().output.choices.find(entry => entry.key === choices[1].key).default, true);
    });
}

for (const kind of ['output', 'input']) {
    for (const port of [false, true]) {
        test(`${kind} same-card ${port ? 'association uses policy RPC with native writes' : 'plain selection stays native'}`, async t => {
            const nativeCard = card();
            const route = { index: 0, name: 'headphones', available: YES };
            const initial = endpoint({ id: 31, card: nativeCard, routes: [route], route });
            const target = endpoint({ id: 32, card: nativeCard, routes: [route], route });
            const f = fixture(t, kind === 'output' ? { outputs: [initial, target] } : { inputs: [initial, target] });
            f.backend.start();
            assert.deepEqual(plain(f.backend.endpointModels(kind).map(node => node.routesReliable)), [false, false]);
            const rows = [choice({ node: initial, key: 'first', portActive: false, default: false }),
                choice({ node: target, key: 'second', port, portName: port ? 'headphones' : '',
                    routeId: port ? 0 : null, portActive: true, default: true })];
            f.load(kind, rows);
            assert.deepEqual(f.snapshot()[kind].choices.map(row => row.default), [false, false],
                'Rust cannot mark a non-native-default node as default');
            f.backend.select(kind, 'second');
            if (port) {
                const worker = f.helpers.at(-1);
                f.timers.advance();
                assert.deepEqual(worker.argv, ['/test/audio-control', 'policy']);
                assert.equal(worker.options.timeout, 45000);
                assert.deepEqual(worker.request, { command: 'select', kind, key: 'second', snapshot: plain(f.backend.nativeSnapshot()) });
                assert.deepEqual(target.routeWrites, []);
                worker.event(action('route', target, { route: 0 }));
                await settle();
                assert.deepEqual(target.routeWrites, [route]);
                assert.deepEqual(JSON.parse(worker.answers.at(-1)), { id: 1, ok: true });
                worker.event(action('default', target, { id: 2 }));
                await settle();
                f.timers.advance(100);
                await settle();
                assert.deepEqual(JSON.parse(worker.answers.at(-1)), { id: 2, ok: true });
                worker.result({ ok: true });
            } else {
                assert.equal(f.helpers.length, 2);
                f.timers.advance(100);
            }
            await settle();
            assert.deepEqual(target.defaultWrites, [true]);
            assert.deepEqual(initial.defaultWrites, []);
            assert.deepEqual(nativeCard.profileWrites, []);
            assert.equal(f.snapshot()[kind].busy, false);
            assert.equal(f.snapshot()[kind].error, '');
            assert.equal(f.snapshot()[kind].choices.find(row => row.key === 'second').default, true);
        });
    }
}

test('native-only device selection remains usable while metadata is unavailable', async t => {
    const f = fixture(t);
    f.backend.start();
    const key = f.snapshot().output.choices[0].key;
    assert.match(key, /^native:/);
    const listing = f.listing('output');
    f.backend.select('output', key);
    assert.equal(listing.stops, 1);
    assert.deepEqual(f.outputs[0].routeWrites, []);
    assert.deepEqual(f.outputs[0].defaultWrites, [true]);
    assert.equal(f.helpers.length, 2, 'no policy selection process for a live native device');
    f.timers.advance(100);
    await settle();
    assert.equal(f.snapshot().output.busy, false);
    assert.equal(f.snapshot().output.error, '');
});

for (const sameDefault of [false, true]) {
    test(`output follow-default: ${sameDefault ? 'reselecting the current output' : 'a converged native output choice'} clears existing app pins, never capture`, async t => {
        const speaker = endpoint();
        const headphones = endpoint({ id: 11, pwName: 'alsa_output.headphones' });
        const selected = sameDefault ? speaker : headphones;
        selected.acceptDefault = false;
        const streams = [playbackStream(), playbackStream({ id: 41, name: 'Elisa', target_serial: 110 }),
            playbackStream({ id: 42, name: 'Other player', target_serial: 999 })];
        const recorder = playbackStream({ id: 50, name: 'Capture', target_serial: 200 });
        const f = fixture(t, { outputs: [speaker, headphones], streams, recorders: [recorder] });
        f.backend.start();
        f.backend.select('output', `native:${f.backend.endpointKey(selected)}`);
        assert.deepEqual(selected.defaultWrites, [true]);
        for (const stream of [...streams, recorder]) assert.deepEqual(stream.targetWrites, []);
        await f.tick();
        if (!sameDefault) {
            assert.equal(f.snapshot().output.busy, true);
            for (const stream of streams) assert.deepEqual(stream.targetWrites, [], 'no reroute before default convergence');
            selected.defaultValue = true;
            selected.onDefault(true);
            selected.notify('is-default');
            await f.tick();
        }
        await f.tick();
        for (const stream of streams) {
            assert.deepEqual(stream.targetWrites, [null]);
            assert.equal(stream.target_serial, -1, 'clear the saved target, not an explicit pin to the chosen endpoint');
        }
        assert.deepEqual(recorder.targetWrites, []);
        assert.equal(recorder.target_serial, 200);
        assert.equal(f.audio.default_speaker.id, selected.id);
        assert.equal(f.snapshot().output.busy, false);
        assert.equal(f.snapshot().output.error, '');
        assert.equal(f.backend.waiters.size, 0);
        assert.equal(f.helpers.filter(helper => helper.request?.command === 'select').length, 0);
    });
}

test('output follow-default: later output choices retain follow mode and clear pins on newly live playback streams', async t => {
    const speaker = endpoint();
    const headphones = endpoint({ id: 11, pwName: 'alsa_output.headphones' });
    const chrome = playbackStream();
    const f = fixture(t, { outputs: [speaker, headphones], streams: [chrome] });
    f.backend.start();
    f.backend.select('output', `native:${f.backend.endpointKey(headphones)}`);
    await f.tick();
    await f.tick();
    assert.equal(chrome.target_serial, -1);
    assert.deepEqual(chrome.targetWrites, [null]);
    const elisa = playbackStream({ id: 41, name: 'Elisa', target_serial: Number(headphones.serial) });
    f.audio.streams = [chrome, elisa];
    f.audio.notify('streams');
    f.backend.select('output', `native:${f.backend.endpointKey(speaker)}`);
    await f.tick();
    await f.tick();
    assert.equal(f.audio.default_speaker.id, speaker.id);
    assert.equal(chrome.target_serial, -1);
    assert.ok(chrome.targetWrites.every(target => target === null), 'future choices must not repin an application');
    assert.deepEqual(elisa.targetWrites, [null]);
    assert.equal(elisa.target_serial, -1);
    assert.equal(f.snapshot().output.busy, false);
    assert.equal(f.snapshot().output.error, '');
});

for (const policy of [false, true]) {
    test(`output follow-default: ${policy ? 'policy' : 'native'} input selection does not reroute playback or capture`, async t => {
        const nativeCard = card();
        const route = { index: 0, name: 'headphones', available: YES };
        const initial = endpoint({ id: 20, card: nativeCard, pwName: 'alsa_input.initial', routes: [route] });
        const selected = endpoint({ id: 21, card: nativeCard, pwName: 'alsa_input.selected', routes: [route] });
        const chrome = playbackStream(), recorder = playbackStream({ id: 50, target_serial: 200 });
        const f = fixture(t, { inputs: [initial, selected], streams: [chrome], recorders: [recorder] });
        f.backend.start();
        const row = choice({ node: selected, port: policy, portName: policy ? 'headphones' : '', routeId: policy ? 0 : null });
        f.load('input', [row]);
        f.backend.select('input', row.key);
        if (policy) {
            const worker = f.helpers.at(-1);
            f.timers.advance();
            worker.event(action('default', selected));
            await settle();
            await f.tick();
            assert.deepEqual(JSON.parse(worker.answers.at(-1)), { id: 1, ok: true });
            worker.result({ ok: true });
            await settle();
        } else await f.tick();
        assert.equal(f.audio.default_microphone.id, selected.id);
        assert.equal(f.snapshot().input.busy, false);
        assert.equal(f.snapshot().input.error, '');
        assert.deepEqual(chrome.targetWrites, []);
        assert.deepEqual(recorder.targetWrites, []);
        assert.equal(chrome.target_serial, 100);
        assert.equal(recorder.target_serial, 200);
    });
}

test('output follow-default: status, refresh, polling and volume/mute controls never clear app or capture targets', async t => {
    const chrome = playbackStream(), recorder = playbackStream({ id: 50, target_serial: 200 });
    const f = fixture(t, { streams: [chrome], recorders: [recorder] });
    f.backend.start();
    await f.tick(0);
    f.backend.status();
    f.backend.snapshot();
    f.backend.refresh();
    for (const kind of ['output', 'input']) {
        const key = f.snapshot()[kind].nodeKey;
        f.backend.setVolume(kind, 0.5, key);
        f.backend.toggleMute(kind, key);
    }
    f.backend.setActive(true, 1);
    await f.tick(2000);
    f.backend.status();
    f.backend.setActive(false, 1);
    assert.deepEqual(chrome.targetWrites, []);
    assert.deepEqual(recorder.targetWrites, []);
    assert.equal(chrome.target_serial, 100);
    assert.equal(recorder.target_serial, 200);
    for (const node of [...f.outputs, ...f.inputs]) {
        assert.deepEqual(node.routeWrites, []);
        assert.deepEqual(node.defaultWrites, []);
    }
});

for (const failure of ['route error', 'route timeout', 'default error', 'default timeout']) {
    test(`output follow-default: native ${failure} leaves app pins untouched`, async t => {
        const route = { index: 0, name: 'headphones', available: YES };
        const selected = endpoint({ id: 11, routes: [route] });
        const chrome = playbackStream(), recorder = playbackStream({ id: 50, target_serial: 200 });
        const f = fixture(t, { outputs: [endpoint(), selected], streams: [chrome], recorders: [recorder] });
        f.backend.start();
        const routed = failure.startsWith('route');
        const row = choice({ node: selected, port: routed, portName: routed ? 'headphones' : '', routeId: routed ? 0 : null });
        f.load('output', [row]);
        if (failure === 'route error') selected.set_route = () => { throw new Error('Native route rejected'); };
        if (failure === 'route timeout') selected.acceptRoute = false;
        if (failure === 'default error') selected.defaultError = new Error('Native default rejected');
        if (failure === 'default timeout') selected.acceptDefault = false;
        f.backend.select('output', row.key);
        await settle();
        await f.tick(3000);
        assert.match(f.snapshot().output.error, /rejected|did not accept the change/);
        assert.equal(f.snapshot().output.busy, false);
        assert.deepEqual(chrome.targetWrites, []);
        assert.deepEqual(recorder.targetWrites, []);
        assert.equal(chrome.target_serial, 100);
        assert.equal(f.backend.waiters.size, 0);
    });
}

test('output follow-default: a policy default action returns the canonical endpoint without rerouting by itself', async t => {
    const chrome = playbackStream();
    const f = transactionFixture(t, { streams: [chrome] });
    const selected = f.addOutput();
    const pending = f.backend.applyPolicyAction(action('default', selected), f.context);
    await f.tick();
    assert.equal(await pending, selected, 'forward the live native node, not the stable default proxy or a snapshot');
    assert.deepEqual(chrome.targetWrites, []);
});

test('output follow-default: policy success reroutes once, using the last successful default only after the helper exits successfully', async t => {
    const chrome = playbackStream({ acceptTarget: false });
    const recorder = playbackStream({ id: 50, target_serial: 200 });
    const f = transactionFixture(t, { streams: [chrome], recorders: [recorder] });
    const selected = f.addOutput();
    const followed = [], follow = f.backend.followDefaultOutput;
    f.backend.followDefaultOutput = node => { followed.push(node); return follow.call(f.backend, node); };
    f.backend.select('output', f.entry.key);
    const worker = f.helpers.at(-1);
    f.timers.advance();
    for (const [index, node] of [f.previous, selected].entries()) {
        worker.event(action('default', node, { id: index + 1 }));
        await settle();
        await f.tick();
        assert.deepEqual(JSON.parse(worker.answers.at(-1)), { id: index + 1, ok: true });
        assert.deepEqual(followed, []);
        assert.deepEqual(chrome.targetWrites, []);
    }
    f.previous.defaultError = new Error('Later default rejected');
    worker.event(action('default', f.previous, { id: 3 }));
    await settle();
    assert.deepEqual(JSON.parse(worker.answers.at(-1)), { id: 3, ok: false, error: 'Later default rejected' });
    worker.event({ type: 'result', ok: true });
    await settle();
    assert.deepEqual(followed, [], 'a typed result alone does not prove successful process completion');
    assert.deepEqual(chrome.targetWrites, []);
    worker.finish();
    await settle();
    assert.deepEqual(followed, [selected]);
    assert.deepEqual(chrome.targetWrites, [null]);
    assert.equal(f.snapshot().output.busy, true, 'selection stays busy until native playback targets converge');
    await f.tick();
    assert.equal(f.snapshot().output.busy, true);
    chrome.target_serial = -1;
    chrome.notify('target-serial');
    await f.tick();
    assert.equal(f.snapshot().output.busy, false);
    assert.equal(f.snapshot().output.error, '');
    assert.deepEqual(followed, [selected]);
    assert.deepEqual(recorder.targetWrites, []);
    assert.equal(f.backend.waiters.size, 0);
});

for (const failure of ['typed failure', 'transport failure', 'rollback']) {
    test(`output follow-default: policy ${failure} never reroutes even after a successful default action`, async t => {
        const chrome = playbackStream(), recorder = playbackStream({ id: 50, target_serial: 200 });
        const f = transactionFixture(t, { streams: [chrome], recorders: [recorder] });
        const selected = f.addOutput();
        f.backend.select('output', f.entry.key);
        const worker = f.helpers.at(-1);
        f.timers.advance();
        worker.event(action('default', selected));
        await settle();
        await f.tick();
        assert.deepEqual(JSON.parse(worker.answers.at(-1)), { id: 1, ok: true });
        assert.deepEqual(chrome.targetWrites, []);
        if (failure === 'rollback') {
            worker.event(action('default', f.previous, { id: 2 }));
            await settle();
            await f.tick();
            assert.deepEqual(JSON.parse(worker.answers.at(-1)), { id: 2, ok: true });
            assert.equal(f.audio.default_speaker.id, f.previous.id);
            assert.deepEqual(chrome.targetWrites, [], 'recovery defaults must not clear application policy');
        }
        if (failure === 'transport failure') {
            worker.event({ type: 'result', ok: true });
            worker.finish(new Error('Policy process failed'));
        } else worker.result({ ok: false, error: failure === 'rollback' ? 'Selection failed; previous default restored' : 'Selection failed' });
        await settle();
        assert.match(f.snapshot().output.error, /failed/);
        assert.equal(f.snapshot().output.busy, false);
        assert.deepEqual(chrome.targetWrites, []);
        assert.deepEqual(recorder.targetWrites, []);
        assert.equal(chrome.target_serial, 100);
        assert.equal(f.backend.waiters.size, 0);
    });
}

test('output follow-default: policy success without an acknowledged default does not invent a playback reroute', async t => {
    const chrome = playbackStream();
    const f = transactionFixture(t, { streams: [chrome] });
    f.backend.select('output', f.entry.key);
    f.timers.advance();
    f.helpers.at(-1).result({ ok: true });
    await settle();
    assert.deepEqual(chrome.targetWrites, []);
    assert.equal(f.snapshot().output.busy, false);
    assert.equal(f.snapshot().output.error, '');
});

for (const change of ['removal', 'same-identity replacement', 'default replacement', 'disposal']) {
    test(`output follow-default: ${change} after default convergence prevents stale playback writes`, async t => {
        const selected = endpoint(), other = endpoint({ id: 11 });
        const chrome = playbackStream();
        const f = fixture(t, { outputs: [selected, other], streams: [chrome] });
        f.backend.start();
        f.backend.select('output', `native:${f.backend.endpointKey(selected)}`);
        f.timers.advance(100); // Resolve native convergence, but do not run its async continuation yet.
        if (change === 'disposal') f.backend.dispose();
        else if (change === 'default replacement') {
            f.audio.default_speaker.id = other.id;
            f.audio.default_speaker.notify('id');
        } else {
            f.audio.speakers = change === 'removal' ? [other] : [f.bindNode(endpoint({ id: selected.id, serial: selected.serial })), other];
            f.audio.notify('speakers');
        }
        await settle();
        await f.tick();
        assert.deepEqual(chrome.targetWrites, []);
        assert.equal(chrome.target_serial, 100);
        assert.equal(f.snapshot().output.busy, false);
        if (change !== 'disposal') assert.match(f.snapshot().output.error, /disappeared|replaced|changed|no longer/);
        assert.equal(f.backend.waiters.size, 0);
    });
}

for (const change of ['removal', 'same-identity replacement', 'serial change', 'default replacement', 'disposal']) {
    test(`output follow-default: ${change} during the native target wait rejects stale success`, async t => {
        const selected = endpoint(), other = endpoint({ id: 11 });
        const chrome = playbackStream({ acceptTarget: false });
        const f = fixture(t, { outputs: [selected, other], streams: [chrome] });
        f.backend.start();
        const pending = f.backend.followDefaultOutput(selected);
        const rejected = assert.rejects(pending, /disappeared|replaced|changed|no longer|cancelled|stopped/);
        assert.deepEqual(chrome.targetWrites, [null]);
        if (change === 'disposal') f.backend.dispose();
        else if (change === 'default replacement') {
            f.audio.default_speaker.id = other.id;
            f.audio.default_speaker.notify('id');
        } else if (change === 'serial change') selected.serial = '999';
        else {
            f.audio.speakers = change === 'removal' ? [other] : [f.bindNode(endpoint({ id: selected.id, serial: selected.serial })), other];
            f.audio.notify('speakers');
        }
        chrome.target_serial = -1; // Even a late successful native write cannot validate a stale selection.
        chrome.notify('target-serial');
        await f.tick();
        await rejected;
        assert.deepEqual(chrome.targetWrites, [null]);
        assert.equal(f.backend.waiters.size, 0);
        if (change === 'disposal') assert.equal(f.timers.sources.size, 0);
    });
}

test('output follow-default: a synchronous default change during a setter prevents writes to later streams', async t => {
    const selected = endpoint(), other = endpoint({ id: 11 });
    const first = playbackStream(), later = playbackStream({ id: 41, name: 'Elisa' });
    const f = fixture(t, { outputs: [selected, other], streams: [first, later] });
    f.backend.start();
    first.onTarget = () => {
        f.audio.default_speaker.id = other.id;
        f.audio.default_speaker.notify('id');
    };
    const rejected = assert.rejects(f.backend.followDefaultOutput(selected), /default.*changed|selected.*changed|no longer/);
    await f.tick();
    await rejected;
    assert.deepEqual(first.targetWrites, [null]);
    assert.deepEqual(later.targetWrites, []);
    assert.equal(f.backend.waiters.size, 0);
});

test('output follow-default: a stream removed before its setter is reached is skipped', async t => {
    const first = playbackStream(), removed = playbackStream({ id: 41, name: 'Elisa' });
    const f = fixture(t, { streams: [first, removed] });
    f.backend.start();
    first.onTarget = () => { f.audio.streams = [first]; f.audio.notify('streams'); };
    const pending = f.backend.followDefaultOutput(f.outputs[0]);
    await f.tick();
    await pending;
    assert.deepEqual(first.targetWrites, [null]);
    assert.deepEqual(removed.targetWrites, []);
    assert.equal(f.backend.waiters.size, 0);
});

test('output follow-default: a removed stream does not block the wait for remaining live playback', async t => {
    const removed = playbackStream({ acceptTarget: false });
    const live = playbackStream({ id: 41, name: 'Elisa', acceptTarget: false });
    const f = fixture(t, { streams: [removed, live] });
    f.backend.start();
    const pending = f.backend.followDefaultOutput(f.outputs[0]);
    f.audio.streams = [live];
    f.audio.notify('streams');
    await f.tick();
    assert.deepEqual(live.targetWrites, [null]);
    assert.equal(f.backend.waiters.size, 1);
    live.target_serial = -1;
    live.notify('target-serial');
    await f.tick();
    await pending;
    assert.deepEqual(removed.targetWrites, [null]);
    assert.equal(f.backend.waiters.size, 0);
});

test('output follow-default: selection waits for target_serial -1, not a pin to the selected endpoint', async t => {
    const selected = endpoint();
    const chrome = playbackStream({ acceptTarget: false, target_serial: Number(selected.serial) });
    const f = fixture(t, { outputs: [selected], streams: [chrome] });
    f.backend.start();
    f.backend.select('output', `native:${f.backend.endpointKey(selected)}`);
    await f.tick();
    assert.deepEqual(chrome.targetWrites, [null]);
    await f.tick();
    assert.equal(f.snapshot().output.busy, true, 'an explicit target on the selected output is still an app pin');
    chrome.target_serial = -1;
    chrome.notify('target-serial');
    await f.tick();
    assert.equal(f.snapshot().output.busy, false);
    assert.equal(f.snapshot().output.error, '');
    assert.equal(f.backend.waiters.size, 0);
});

test('output follow-default: a failed setter does not block other playback resets and is reported after their convergence', async t => {
    const chrome = playbackStream({ targetError: new Error('Native target rejected') });
    const elisa = playbackStream({ id: 41, name: 'Elisa', acceptTarget: false });
    const other = playbackStream({ id: 42, name: 'Other player' });
    const recorder = playbackStream({ id: 50, target_serial: 200 });
    const f = fixture(t, { streams: [chrome, elisa, other], recorders: [recorder] });
    f.backend.start();
    f.backend.select('output', `native:${f.backend.endpointKey(f.outputs[0])}`);
    await f.tick();
    for (const stream of [chrome, elisa, other]) assert.deepEqual(stream.targetWrites, [null]);
    assert.equal(chrome.target_serial, 100);
    assert.equal(other.target_serial, -1);
    assert.equal(f.snapshot().output.busy, true);
    assert.equal(f.snapshot().output.error, '');
    await f.tick();
    assert.equal(f.snapshot().output.busy, true, 'a setter error must not abandon other live streams awaiting convergence');
    assert.equal(f.snapshot().output.error, '');
    elisa.target_serial = -1;
    elisa.notify('target-serial');
    await f.tick();
    const model = f.snapshot().output;
    assert.equal(model.busy, false);
    assert.match(model.error, /Chrome/);
    assert.match(model.error, /Native target rejected/);
    assert.equal(elisa.target_serial, -1);
    for (const stream of [chrome, elisa, other]) assert.deepEqual(stream.targetWrites, [null]);
    assert.deepEqual(recorder.targetWrites, []);
    assert.equal(recorder.target_serial, 200);
    assert.equal(f.backend.waiters.size, 0);
    assert.equal(f.helpers.filter(helper => helper.request?.command === 'select').length, 0);
});

for (const failure of ['setter error', 'timeout']) {
    test(`output follow-default: native target ${failure} is actionable instead of silent selection success`, async t => {
        const chrome = playbackStream({ acceptTarget: false,
            ...(failure === 'setter error' ? { targetError: new Error('Astal refused the Chrome playback target') } : {}) });
        const recorder = playbackStream({ id: 50, target_serial: 200 });
        const f = fixture(t, { streams: [chrome], recorders: [recorder] });
        f.backend.start();
        f.backend.select('output', `native:${f.backend.endpointKey(f.outputs[0])}`);
        await f.tick();
        assert.deepEqual(chrome.targetWrites, [null]);
        if (failure === 'timeout') {
            assert.equal(f.snapshot().output.busy, true);
            await f.tick(2900);
            assert.equal(f.snapshot().output.busy, true, 'native target convergence gets a bounded opportunity to finish');
            await f.tick();
        }
        const model = f.snapshot().output;
        assert.equal(model.busy, false);
        assert.match(model.error, /Chrome|playback|stream|application/i);
        assert.match(model.error, /refused|did not|not.*follow|tim(?:e|ed)[ -]?out|timeout|failed|accept/i);
        assert.equal(chrome.target_serial, 100);
        assert.deepEqual(recorder.targetWrites, []);
        assert.equal(f.backend.waiters.size, 0);
        assert.equal(f.helpers.filter(helper => helper.request?.command === 'select').length, 0);
    });
}

for (const reason of ['stale serial', 'missing node', 'missing route', 'unavailable route', 'ambiguous route']) {
    test(`selection rejects ${reason} without starting a Rust mutation fallback`, t => {
        const route = { index: 0, name: 'headphones', available: YES };
        const node = endpoint({ routes: [route] });
        const f = fixture(t, { outputs: [node] });
        f.backend.start();
        const row = choice({ node });
        if (reason === 'stale serial') row.nodeSerial = '999';
        if (reason === 'missing node') row.nodeId = 999;
        if (reason === 'missing route') node.routes = [];
        if (reason === 'unavailable route') route.available = NO;
        if (reason === 'ambiguous route') node.routes.push({ ...route });
        f.load('output', [row]);
        assert.throws(() => f.backend.select('output', row.key), /no longer available/);
        assert.equal(f.helpers.length, 2);
        assert.deepEqual(node.routeWrites, []);
        assert.deepEqual(node.defaultWrites, []);
    });
}

for (const transportError of [false, true]) {
    test(`inactive-profile policy ${transportError ? 'transport' : 'typed'} failure releases busy state`, async t => {
        const nativeCard = card();
        const f = fixture(t, { cards: [nativeCard] });
        f.backend.start();
        const row = choice({ nativeCard, name: '', nodeId: null, nodeSerial: '' });
        f.load('output', [row]);
        f.backend.select('output', row.key);
        const worker = f.helpers.at(-1);
        f.timers.advance();
        assert.equal(worker.request.key, row.key);
        if (transportError) worker.finish(new Error('Policy unavailable'));
        else worker.result({ ok: false, error: 'Profile switch failed' });
        await settle();
        assert.equal(f.snapshot().output.error, transportError ? 'Policy unavailable' : 'Profile switch failed');
        assert.equal(f.snapshot().output.busy, false);
        assert.equal(f.backend.metadata.output.selection, null);
        assert.deepEqual(nativeCard.profileWrites, []);
        assert.deepEqual(f.outputs[0].defaultWrites, []);
    });
}

test('native selection never defaults a replacement endpoint that reuses the selected id', async t => {
    const route = { index: 3, name: 'headphones', available: YES };
    const old = endpoint({ routes: [route], acceptRoute: false });
    const f = fixture(t, { outputs: [old] });
    f.backend.start();
    f.load('output', [choice({ node: old, routeId: route.index })]);
    f.backend.select('output', 'card:port');
    const replacement = endpoint({ id: old.id, serial: old.serial, routes: [route] });
    f.audio.speakers = [replacement];
    f.audio.notify('speakers');
    f.timers.advance(100);
    await settle();
    assert.match(f.snapshot().output.error, /device disappeared/);
    assert.equal(f.snapshot().output.busy, false);
    assert.deepEqual(old.defaultWrites, []);
    assert.deepEqual(replacement.defaultWrites, []);
    assert.equal(f.helpers.length, 2, 'a disappeared endpoint cannot trigger a mutation fallback');
    assert.equal(f.backend.waiters.size, 0);
});

test('native route acknowledgement is bounded and failure does not set a default', async t => {
    const node = endpoint({ routes: [{ index: 1, name: 'headphones', available: YES }], acceptRoute: false });
    const f = fixture(t, { outputs: [node] });
    f.backend.start();
    f.load('output', [choice({ node, routeId: 1 })]);
    f.backend.select('output', 'card:port');
    f.timers.advance(3000);
    await settle();
    assert.match(f.snapshot().output.error, /did not accept the change/);
    assert.equal(f.snapshot().output.busy, false);
    assert.deepEqual(node.defaultWrites, []);
    assert.equal(f.backend.waiters.size, 0);
});

test('overlapping metadata refreshes coalesce to one follow-up without flickering loading off', t => {
    const f = fixture(t);
    f.backend.start();
    const first = f.listing('output');
    f.backend.refresh('output');
    f.backend.refresh('output');
    assert.equal(f.helpers.length, 2);
    first.rows([choice({ port: false, portName: '', routeId: null, label: 'First' })]);
    const second = f.listing('output');
    assert.notEqual(first, second);
    assert.equal(f.helpers.length, 3);
    assert.equal(f.snapshot().output.loading, true);
    assert.equal(f.snapshot().output.choices[0].label, 'First');
    second.rows([choice({ port: false, portName: '', routeId: null, label: 'Latest' })]);
    assert.equal(f.helpers.length, 3);
    assert.equal(f.snapshot().output.loading, false);
    assert.equal(f.snapshot().output.choices[0].label, 'Latest');
    assert.equal(f.snapshot().input.loading, true, 'input request has independent loading state');
});

for (const staleError of [false, true]) {
    test(`metadata revision changes discard stale ${staleError ? 'errors' : 'successes'} and keep the replacement loading`, t => {
        const f = fixture(t);
        f.backend.start();
        f.load('output', [choice({ port: false, portName: '', routeId: null, label: 'Known good' })]);
        f.load('input', []);
        f.backend.refresh('output');
        const stale = f.listing('output');
        f.backend.scheduleRefresh();
        f.backend.scheduleRefresh();
        f.timers.advance(150);
        if (staleError) stale.finish(new Error('Obsolete failure'));
        else stale.rows([choice({ port: false, portName: '', routeId: null, label: 'Obsolete result' })]);
        const latest = f.listing('output');
        assert.notEqual(latest, stale);
        assert.equal(f.snapshot().output.loading, true);
        assert.equal(f.snapshot().output.choices[0].label, 'Known good');
        assert.equal(f.snapshot().output.error, '');
        latest.rows([choice({ port: false, portName: '', routeId: null, label: 'Fresh result' })]);
        assert.equal(f.snapshot().output.loading, false);
        assert.equal(f.snapshot().output.choices[0].label, 'Fresh result');
    });
}

test('selection invalidates old metadata callbacks, including callbacks after a newer listing begins', async t => {
    const f = fixture(t);
    f.backend.start();
    const row = choice({ port: false, portName: '', routeId: null, label: 'Selected device' });
    f.load('output', [row]);
    f.backend.refresh('output');
    const stale = f.listing('output');
    stale.event({ type: 'result', rows: [{ ...row, label: 'Stale' }] });
    f.backend.select('output', row.key);
    assert.equal(stale.stops, 1);
    assert.equal(f.snapshot().output.loading, false);
    // Simulate a queued completion delivered after cancellation.
    stale.options.onExit(null);
    assert.equal(f.snapshot().output.choices[0].label, 'Selected device');
    f.timers.advance(100);
    await settle();
    f.timers.advance(150);
    const latest = f.listing('output');
    assert.notEqual(latest, stale);
    assert.equal(f.snapshot().output.loading, true);
    stale.options.onExit(new Error('Late cancellation'));
    assert.equal(f.backend.metadata.output.listing.worker, latest);
    assert.equal(f.snapshot().output.loading, true);
    assert.equal(f.snapshot().output.error, '');
    latest.rows([row]);
    assert.equal(f.snapshot().output.loading, false);
});

test('metadata failures preserve native controls and last good rows, and recovery clears the error', t => {
    const f = fixture(t);
    f.backend.start();
    f.listing('output').finish(new Error('Helper unavailable'));
    assert.match(f.snapshot().output.error, /Helper unavailable/);
    assert.match(f.snapshot().output.choices[0].key, /^native:/);
    assert.equal(f.snapshot().output.available, true);
    const key = f.snapshot().output.nodeKey;
    f.backend.setVolume('output', 0.8, key);
    assert.equal(f.outputs[0].volume, 0.8);
    f.backend.refresh('output');
    f.load('output', [choice({ port: false, portName: '', routeId: null, label: 'Recovered' })]);
    assert.equal(f.snapshot().output.error, '');
    f.backend.refresh('output');
    f.listing('output').rows([{ key: 'invalid-row' }]);
    assert.match(f.snapshot().output.error, /Invalid audio device entry/);
    assert.equal(f.snapshot().output.choices[0].label, 'Recovered');
    assert.equal(f.snapshot().output.loading, false);
});

test('pairing binds the explicit native hci adapter and address, then trusts and connects only through Astal', t => {
    const radio = adapter({ path: '/org/bluez/hci12' });
    const headset = device({ adapter: '/org/bluez/hci12', name: 'Desk headset' });
    const f = fixture(t, { radio, devices: [headset] });
    f.backend.start();
    f.backend.setActive(true, 0);
    f.backend.deviceAction(f.snapshot().bluetooth.devices[0].key);
    const worker = f.helpers.at(-1);
    assert.deepEqual(worker.argv, ['/test/audio-control', 'bluetooth-pair', 'hci12', headset.address]);
    assert.equal(worker.options.interactive, true);
    assert.equal(worker.options.timeout, 180000);
    assert.equal(headset.trusted, false);
    assert.deepEqual(headset.operations, []);
    worker.emit('Please type the six-digit passkey.\n');
    f.backend.sendPairingAnswer('000007');
    assert.deepEqual(worker.answers, ['000007\n']);
    worker.finish();
    assert.equal(headset.trusted, true);
    assert.equal(headset.connected, false, 'the helper cannot connect the native device');
    assert.equal(headset.operations[0].method, 'connect_device');
    assert.equal(f.backend.pairing.process, null);
    assert.equal(f.snapshot().bluetooth.pairing, false, 'no more agent prompts after Pair completes');
    assert.equal(f.snapshot().bluetooth.devices[0].busy, true);
    assert.throws(() => f.backend.sendPairingAnswer('yes'), /completed; connecting through Astal/);
    f.backend.deviceAction(f.snapshot().bluetooth.devices[0].key);
    f.backend.forget(f.snapshot().bluetooth.devices[0].key);
    assert.equal(headset.operations.length, 1);
    assert.deepEqual(radio.removed, []);
    const result = { token: 'native connection result' };
    headset.operations[0].callback(headset, result);
    assert.deepEqual(headset.finishes, [{ method: 'connect_device', result }]);
    assert.equal(headset.connected, true);
    assert.equal(f.backend.pairing, null);
    assert.equal(f.snapshot().bluetooth.devices[0].busy, false);
    assert.equal(f.snapshot().bluetooth.error, '');
    assert.deepEqual(f.snapshot().bluetooth.pairingMessage,
        { state: 'paired', text: `Paired with Desk headset (${headset.address}).`, prompt: '' });
    assert.equal(headset.cancellations, 0);
});

test('pairing replaces progress with one named status, retaining only the latest complete prompt', t => {
    const headset = device({ name: "Raina's Pixel Buds Pro" });
    const label = `${headset.name} (${headset.address})`;
    const f = fixture(t, { devices: [headset] });
    f.backend.start();
    f.backend.setActive(true, 0);
    f.backend.deviceAction(f.snapshot().bluetooth.devices[0].key);
    const worker = f.helpers.at(-1);
    const status = () => f.snapshot().bluetooth.pairingMessage;
    assert.deepEqual(status(), { state: 'pairing', text: `Pairing with ${label}…`, prompt: '' });
    worker.emit(`Pairing with ${headset.address} on hci0…\n`);
    assert.equal(status().prompt, '');
    worker.emit('Does passkey 000');
    assert.equal(status().prompt, '', 'partial lines are not shown');
    worker.emit(`007 match the one shown on ${headset.address}? Type yes or no.\n`);
    assert.equal(status().prompt, `Does passkey 000007 match the one shown on ${label}? Type yes or no.`);
    worker.emit(`Enter PIN for ${headset.address}:\n\n`);
    assert.equal(status().prompt, `Enter PIN for ${label}:`);
    assert.equal(status().prompt.includes('000007'), false, 'new prompts replace old ones');
    f.backend.sendPairingAnswer('000123');
    assert.equal(status().prompt, '');
    worker.emit(`${headset.address} is paired.\n`);
    assert.equal(status().text, `Pairing with ${label}…`, 'stdout progress is not an extra message');
    worker.finish();
    assert.deepEqual(status(), { state: 'paired', text: `Paired with ${label}.`, prompt: '' });
    headset.operations[0].callback(headset, {});
    assert.deepEqual(status(), { state: 'paired', text: `Paired with ${label}.`, prompt: '' });
    assert.doesNotMatch(status().text, /[\r\n]/);
});

for (const [properties, expected] of [
    [{ name: '', alias: 'Desk buds' }, 'Desk buds (AA:BB:CC:DD:EE:01)'],
    [{ name: '', alias: '' }, 'AA:BB:CC:DD:EE:01'],
    [{ name: 'AA:BB:CC:DD:EE:01' }, 'AA:BB:CC:DD:EE:01'],
]) {
    test(`pairing status uses the native name/alias with an address fallback: ${expected}`, t => {
        const f = fixture(t, { devices: [device(properties)] });
        f.backend.start();
        f.backend.setActive(true, 0);
        f.backend.deviceAction(f.snapshot().bluetooth.devices[0].key);
        assert.equal(f.snapshot().bluetooth.pairingMessage.text, `Pairing with ${expected}…`);
    });
}

test('pairing failure replaces progress with one named error without a duplicate Bluetooth error or trailing line', t => {
    const headset = device({ name: "Raina's Pixel Buds Pro" });
    const label = `${headset.name} (${headset.address})`;
    const f = fixture(t, { devices: [headset] });
    f.backend.start();
    f.backend.setActive(true, 0);
    f.backend.deviceAction(f.snapshot().bluetooth.devices[0].key);
    f.helpers.at(-1).finish(new Error(`Pairing with ${headset.address} on hci0…\n`
        + `audio-control: Incorrect pairing code for ${headset.address}.\n`));
    assert.deepEqual(f.snapshot().bluetooth.pairingMessage,
        { state: 'failed', text: `Pairing failed: Incorrect pairing code for ${label}.`, prompt: '' });
    assert.equal(f.snapshot().bluetooth.error, '');
    f.backend.dismissPairingMessage();
    assert.equal(f.snapshot().bluetooth.pairingMessage, null);
    assert.equal(headset.cancellations, 0);
});

test('dismissing a pairing notice does not cancel pairing or revive it on completion; new prompts remain accessible', t => {
    const headset = device();
    const f = fixture(t, { devices: [headset] });
    f.backend.start();
    f.backend.setActive(true, 0);
    const key = f.snapshot().bluetooth.devices[0].key;
    f.backend.deviceAction(key);
    const worker = f.helpers.at(-1), session = f.backend.pairing;
    f.backend.dismissPairingMessage();
    assert.equal(f.snapshot().bluetooth.pairingMessage, null);
    assert.equal(f.backend.pairing, session);
    assert.equal(worker.stops, 0);
    assert.equal(headset.cancellations, 0);
    worker.emit(`Pairing with ${headset.address} on hci0…\n`);
    assert.equal(f.snapshot().bluetooth.pairingMessage, null);
    worker.emit(`Does passkey 000007 match the one shown on ${headset.address}? Type yes or no.\n`);
    assert.match(f.snapshot().bluetooth.pairingMessage.prompt, /000007/);
    f.backend.sendPairingAnswer('yes');
    f.backend.dismissPairingMessage();
    worker.finish();
    headset.operations[0].callback(headset, {});
    assert.equal(f.snapshot().bluetooth.pairingMessage, null);
    assert.equal(headset.cancellations, 0);
    assert.deepEqual(f.radio.scanCalls, ['start']);
    headset.connected = false;
    f.backend.deviceAction(key);
    assert.equal(f.snapshot().bluetooth.pairingMessage.state, 'pairing', 'a new request gets a new visible notice');
});

for (const response of ['answer', 'cancel']) {
    test(`an unanswered pairing prompt cannot be dismissed and remains reachable to ${response}`, t => {
        const headset = device();
        const f = fixture(t, { devices: [headset] });
        f.backend.start();
        f.backend.setActive(true, 0);
        f.backend.deviceAction(f.snapshot().bluetooth.devices[0].key);
        const worker = f.helpers.at(-1);
        worker.emit(`Enter PIN for ${headset.address}:\n`);
        const notice = f.snapshot().bluetooth.pairingMessage;
        f.backend.dismissPairingMessage();
        assert.deepEqual(f.snapshot().bluetooth.pairingMessage, notice);
        assert.equal(f.snapshot().bluetooth.pairing, true);
        assert.equal(worker.stops, 0);
        if (response === 'answer') {
            f.backend.sendPairingAnswer('000123');
            assert.deepEqual(worker.answers, ['000123\n']);
            f.backend.dismissPairingMessage();
            assert.equal(f.snapshot().bluetooth.pairingMessage, null);
        } else {
            f.backend.cancelPairing();
            assert.equal(f.snapshot().bluetooth.pairingMessage, null);
            assert.equal(headset.cancellations, 1);
            assert.equal(worker.stops, 1);
        }
    });
}

for (const stage of ['agent', 'trust', 'connect start', 'connect finish']) {
    test(`pairing ${stage} failure reports the error and releases native busy/session state`, t => {
        const headset = device();
        const f = fixture(t, { devices: [headset] });
        f.backend.start();
        f.backend.setActive(true, 0);
        f.backend.deviceAction(f.snapshot().bluetooth.devices[0].key);
        const worker = f.helpers.at(-1);
        const error = new Error(`${stage} failed`);
        if (stage === 'trust') Object.defineProperty(headset, 'trusted', { set: () => { throw error; } });
        if (stage === 'connect start') headset.connect_device = () => { throw error; };
        worker.finish(stage === 'agent' ? error : null);
        if (stage === 'connect finish') headset.operations[0].callback(headset, { error });
        assert.equal(f.backend.pairing, null);
        assert.equal(f.snapshot().bluetooth.devices[0].busy, false);
        assert.equal(f.snapshot().bluetooth.error, '', 'pairing errors appear once in the dismissible status card');
        const notice = f.snapshot().bluetooth.pairingMessage;
        assert.equal(notice.state, 'failed');
        assert.ok(notice.text.includes(error.message));
        assert.match(notice.text, stage === 'agent' ? /^Failed to pair with Headset/ : /^Paired with Headset.*but setup failed/);
        assert.equal(notice.prompt, '');
        if (stage === 'agent') {
            assert.equal(headset.trusted, false);
            assert.deepEqual(headset.operations, []);
        }
        if (stage === 'connect finish') assert.equal(headset.finishes.length, 1);
    });
}

for (const path of ['/org/bluez/not-hci', '/org/bluez/hci1/device', 'hci0', '']) {
    test(`pairing rejects an unavailable or non-adapter object path ${JSON.stringify(path)}`, t => {
        const radio = adapter({ path });
        const headset = device({ adapter: path });
        const f = fixture(t, { radio, devices: [headset] });
        f.backend.start();
        f.backend.setActive(true, 0);
        assert.throws(() => f.backend.deviceAction(f.snapshot().bluetooth.devices[0].key), /native Bluetooth adapter is no longer available/);
        assert.equal(f.backend.pairing, null);
        assert.equal(f.helpers.length, 2);
        assert.equal(headset.trusted, false);
        assert.deepEqual(headset.operations, []);
    });
}

for (const failure of [false, true]) {
    test(`cancelled session's late native connect ${failure ? 'failure' : 'success'} is finished without changing a newer pairing`, t => {
        const old = device();
        const current = device({ address: 'AA:BB:CC:DD:EE:02' });
        const f = fixture(t, { devices: [old, current] });
        f.backend.start();
        f.backend.setActive(true, 0);
        const keyFor = address => f.snapshot().bluetooth.devices.find(row => row.address === address).key;
        f.backend.deviceAction(keyFor(old.address));
        const worker = f.helpers.at(-1);
        worker.finish();
        assert.equal(old.operations.length, 1);
        f.backend.setActive(false, 0);
        assert.equal(old.cancellations, 1);
        assert.equal(worker.stops, 0, 'already-exited agent is not stopped again');
        f.backend.setActive(true, 0);
        f.backend.deviceAction(keyFor(current.address));
        const newer = f.helpers.at(-1);
        newer.emit('New session PIN?\n');
        const session = f.backend.pairing;
        const message = plain(f.backend.pairingMessage);
        old.operations[0].callback(old, failure ? { error: new Error('Old connect failed') } : {});
        assert.equal(old.finishes.length, 1, 'native async results are always consumed');
        assert.equal(f.backend.pairing, session);
        assert.deepEqual(plain(f.backend.pairingMessage), message);
        assert.equal(f.backend.bluetoothError, '');
        assert.equal(f.snapshot().bluetooth.devices.find(row => row.address === old.address).busy, false);
        f.backend.sendPairingAnswer('004200');
        assert.deepEqual(newer.answers, ['004200\n']);
        message.prompt = '';
        worker.options.onChunk('Old agent prompt');
        worker.options.onExit(new Error('Old helper error'));
        assert.equal(f.backend.pairing, session);
        assert.deepEqual(plain(f.backend.pairingMessage), message);
        assert.equal(f.backend.bluetoothError, '');
    });
}

for (const cause of ['power off', 'device removed', 'dispose']) {
    test(`native connection completion after ${cause} still calls finish without reviving pairing`, t => {
        const headset = device();
        const f = fixture(t, { devices: [headset] });
        f.backend.start();
        f.backend.setActive(true, 0);
        f.backend.deviceAction(f.snapshot().bluetooth.devices[0].key);
        f.helpers.at(-1).finish();
        if (cause === 'power off') f.backend.toggleBluetoothPower();
        else if (cause === 'device removed') { f.bluetooth.devices = []; f.bluetooth.notify('devices'); }
        else f.backend.dispose();
        assert.equal(f.backend.pairing, null);
        headset.operations[0].callback(headset, { error: new Error('Late connection error') });
        assert.equal(headset.finishes.length, 1);
        assert.equal(f.backend.pairing, null);
        assert.equal(f.backend.bluetoothError, '');
        assert.equal(f.backend.pairingMessage, null);
    });
}

test('Bluetooth filters devices using the generic adapter path getter when object_path is absent', t => {
    const radio = adapter({ path: '/org/bluez/hci9' });
    const local = device({ adapter: '/org/bluez/hci9', paired: true });
    const foreign = device({ address: 'AA:02', adapter: '/org/bluez/hci0' });
    const f = fixture(t, { radio, devices: [local, foreign] });
    assert.equal(radio.object_path, undefined);
    assert.equal('object_path' in radio, false);
    f.backend.start();
    assert.deepEqual(radio.propertyReads, ['object-path']);
    assert.deepEqual(f.snapshot().bluetooth.devices.map(entry => entry.address), [local.address]);
    assert.equal(local.handlers.size, 1);
    assert.equal(foreign.handlers.size, 0);
    f.backend.deviceAction(f.snapshot().bluetooth.devices[0].key);
    assert.equal(local.operations[0].method, 'connect_device');
    f.bluetooth.adapter = null;
    f.bluetooth.notify('adapter');
    assert.equal(radio.handlers.size, 0);
    assert.equal(local.handlers.size, 0);
    assert.equal(f.snapshot().bluetooth.available, false);
    assert.deepEqual(f.snapshot().bluetooth.devices, []);
    assert.deepEqual(radio.propertyReads, ['object-path'], 'no getter call without an adapter');
});

test('Bluetooth snapshots filter the active adapter, order devices, fall back names and normalize battery fractions', t => {
    const devices = [
        device({ address: '01', name: 'Zulu', connected: true, battery_percentage: 0.45 }),
        device({ address: '02', name: 'Alpha', connected: true, battery_percentage: 0 }),
        device({ address: '03', name: 'Paired', paired: true, battery_percentage: 1.2 }),
        device({ address: '04', name: '', alias: 'Alias', battery_percentage: NaN }),
        device({ address: '05', name: '', battery_percentage: -0.1 }),
        device({ address: '06', name: 'Infinity', battery_percentage: Infinity }),
        device({ address: '07', name: 'String', battery_percentage: '0.5' }),
        device({ address: '08', name: 'Missing', battery_percentage: undefined }),
        device({ address: 'foreign', name: 'Wrong adapter', connected: true, adapter: '/org/bluez/hci1' }),
    ];
    const f = fixture(t, { devices });
    f.backend.start();
    const model = f.snapshot().bluetooth;
    assert.equal(model.available, true);
    assert.equal(model.powered, true);
    assert.deepEqual(model.devices.map(entry => entry.name),
        ['Alpha', 'Zulu', 'Paired', '05', 'Alias', 'Infinity', 'Missing', 'String']);
    assert.deepEqual(Object.fromEntries(model.devices.map(entry => [entry.address, entry.battery])),
        { '01': 0.45, '02': 0, '03': 1, '04': null, '05': null, '06': null, '07': null, '08': null });
    assert.equal(devices.at(-1).connections, 0);
    const key = model.devices.find(entry => entry.address === '01').key;
    devices[0].battery_percentage = 0.8;
    devices[0].notify('battery-percentage');
    assert.equal(f.snapshot().bluetooth.devices.find(entry => entry.address === '01').key, key);
    assert.equal(devices[0].battery_percentage, 0.8);
    assert.deepEqual(f.bluetooth.devices, devices, 'snapshot sorting must not mutate the native list');
});

for (const connected of [false, true]) {
    const method = connected ? 'disconnect_device' : 'connect_device';
    test(`Bluetooth ${method} uses callback-only async/finish APIs and guards duplicate actions`, t => {
        const headset = device({ paired: true, connected });
        const f = fixture(t, { devices: [headset] });
        f.backend.start();
        const key = f.snapshot().bluetooth.devices[0].key;
        f.backend.deviceAction(key);
        f.backend.deviceAction(key);
        f.backend.forget(key);
        assert.equal(headset.operations.length, 1);
        assert.equal(headset.operations[0].method, method);
        assert.equal(f.snapshot().bluetooth.devices[0].busy, true);
        assert.deepEqual(f.radio.removed, []);
        const result = { token: 'native async result' };
        headset.operations[0].callback(headset, result);
        assert.deepEqual(headset.finishes, [{ method, result }]);
        assert.equal(headset.connected, !connected);
        assert.equal(f.snapshot().bluetooth.devices[0].busy, false);
        assert.equal(f.snapshot().bluetooth.error, '');
        f.backend.deviceAction(key);
        const failed = headset.operations[1];
        failed.callback(headset, { error: new Error('BlueZ rejected operation') });
        assert.equal(f.snapshot().bluetooth.error, 'BlueZ rejected operation');
        assert.equal(f.snapshot().bluetooth.devices[0].busy, false);
        f.backend.forget(key);
        assert.deepEqual(f.radio.removed, [headset]);
        assert.equal(f.snapshot().bluetooth.error, '');
        f.radio.powered = false;
        f.backend.deviceAction(key);
        assert.equal(headset.operations.length, 2);
    });
}

test('Bluetooth address reuse invalidates keys and a removed device completion cannot leak an error', t => {
    const old = device({ paired: true });
    const f = fixture(t, { devices: [old] });
    f.backend.start();
    const key = f.snapshot().bluetooth.devices[0].key;
    f.backend.deviceAction(key);
    const replacement = device({ address: old.address, paired: true });
    f.bluetooth.devices = [replacement];
    f.bluetooth.notify('devices');
    assert.equal(old.handlers.size, 0);
    assert.equal(replacement.handlers.size, 1);
    assert.notEqual(f.snapshot().bluetooth.devices[0].key, key);
    assert.throws(() => f.backend.deviceAction(key), /no longer available/);
    assert.throws(() => f.backend.forget(key), /no longer available/);
    old.operations[0].callback(old, { error: new Error('Obsolete failure') });
    assert.equal(f.snapshot().bluetooth.error, '');
    assert.equal(f.snapshot().bluetooth.devices[0].busy, false);
});

for (const [label, opened, tab] of [['leaving the Bluetooth tab', true, 1], ['closing the panel', false, 0]]) {
    test(`${label} stops owned scanning, cancels pairing and ignores delayed helper output`, t => {
        const headset = device();
        const f = fixture(t, { devices: [headset] });
        f.backend.start();
        f.backend.setActive(true, 0);
        assert.deepEqual(f.radio.scanCalls, ['start']);
        f.backend.deviceAction(f.snapshot().bluetooth.devices[0].key);
        const pairing = f.helpers.at(-1);
        assert.deepEqual(pairing.argv, ['/test/audio-control', 'bluetooth-pair', 'hci0', headset.address]);
        assert.equal(pairing.options.interactive, true);
        assert.equal(pairing.options.timeout, 180000);
        pairing.options.onChunk('x'.repeat(33000));
        assert.equal(f.backend.pairing.pending.length, 32768);
        pairing.options.onChunk(`\nEnter PIN for ${headset.address}:\n`);
        const notice = f.snapshot().bluetooth.pairingMessage;
        assert.equal(notice.text, `Pairing with Headset (${headset.address})…`);
        assert.equal(notice.prompt, `Enter PIN for Headset (${headset.address}):`);
        f.backend.setActive(opened, tab);
        f.backend.setActive(opened, tab);
        assert.deepEqual(f.radio.scanCalls, ['start', 'stop']);
        assert.equal(headset.cancellations, 1);
        assert.equal(pairing.stops, 1);
        assert.equal(f.snapshot().bluetooth.pairing, false);
        assert.equal(f.snapshot().bluetooth.error, '', 'intentional cancellation is not a failure');
        f.timers.advance();
        const before = f.snapshot(), changes = f.changes();
        pairing.options.onChunk('Delayed PIN prompt');
        pairing.options.onExit(new Error('Delayed pairing failure'), '');
        f.timers.advance();
        assert.deepEqual(f.snapshot(), before);
        assert.equal(f.changes(), changes);
        assert.throws(() => f.backend.sendPairingAnswer('yes'), /No pairing request/);
        f.backend.setActive(true, 0);
        f.backend.deviceAction(f.snapshot().bluetooth.devices[0].key);
        const current = f.helpers.at(-1);
        current.options.onChunk('New session PIN?');
        f.timers.advance();
        const reopened = f.snapshot(), reopenedChanges = f.changes();
        pairing.options.onChunk('Old session PIN');
        pairing.options.onExit(new Error('Old session failure'), '');
        f.timers.advance();
        assert.deepEqual(f.snapshot(), reopened, 'old callbacks cannot affect a new pairing session');
        assert.equal(f.changes(), reopenedChanges);
        f.backend.sendPairingAnswer('000123');
        assert.deepEqual(current.answers, ['000123\n']);
    });
}

test('pairing input guards preserve PINs, reject injection, and do not start pairing off-tab', t => {
    const headset = device();
    const f = fixture(t, { devices: [headset] });
    f.backend.start();
    const key = f.snapshot().bluetooth.devices[0].key;
    f.backend.deviceAction(key);
    f.backend.syncDiscovery();
    f.backend.setActive(true, 1);
    f.backend.deviceAction(key);
    assert.equal(f.helpers.some(helper => helper.argv[1] === 'bluetooth-pair'), false);
    assert.deepEqual(f.radio.scanCalls, []);
    assert.throws(() => f.backend.sendPairingAnswer('yes'), /No pairing request/);
    f.backend.setActive(true, 0);
    f.backend.deviceAction(key);
    const pairing = f.helpers.at(-1);
    const helperCount = f.helpers.length;
    f.backend.deviceAction(key);
    f.backend.forget(key);
    assert.equal(f.helpers.length, helperCount);
    assert.deepEqual(f.radio.removed, []);
    for (const invalid of ['', '  ', 'yes\nno', 'yes\r', 'a\0b', 'x'.repeat(513), null, 123, undefined])
        assert.throws(() => f.backend.sendPairingAnswer(invalid), /Enter one PIN\/passkey/);
    assert.deepEqual(pairing.answers, []);
    for (const answer of ['000123', 'yes', ' no ', 'x'.repeat(512)]) f.backend.sendPairingAnswer(answer);
    assert.deepEqual(pairing.answers, ['000123\n', 'yes\n', ' no \n', `${'x'.repeat(512)}\n`]);
    pairing.options.onChunk('Confirmed');
    pairing.finish();
    assert.equal(headset.trusted, true);
    assert.equal(headset.operations.length, 1);
    assert.equal(f.snapshot().bluetooth.devices[0].busy, true);
    assert.throws(() => f.backend.sendPairingAnswer('yes'), /completed; connecting through Astal/);
    headset.operations[0].callback(headset, {});
    assert.equal(f.snapshot().bluetooth.pairing, false);
    assert.equal(f.snapshot().bluetooth.error, '');
    assert.throws(() => f.backend.sendPairingAnswer('yes'), /No pairing request/);
});

for (const cause of ['adapter replacement', 'adapter powered off', 'device removed']) {
    test(`${cause} cancels active pairing and cleans up native subscriptions`, t => {
        const headset = device();
        const f = fixture(t, { devices: [headset] });
        f.backend.start();
        f.backend.setActive(true, 0);
        f.backend.deviceAction(f.snapshot().bluetooth.devices[0].key);
        const pairing = f.helpers.at(-1), oldRadio = f.radio;
        if (cause === 'adapter replacement') {
            f.bluetooth.adapter = adapter({ path: '/org/bluez/hci1' });
            f.bluetooth.notify('adapter');
            assert.equal(oldRadio.handlers.size, 0);
            assert.equal(headset.handlers.size, 0);
            assert.equal(f.bluetooth.adapter.handlers.size, 1);
            assert.deepEqual(f.snapshot().bluetooth.devices, []);
        } else if (cause === 'adapter powered off') {
            oldRadio.powered = false;
            oldRadio.notify('powered');
            assert.equal(f.snapshot().bluetooth.powered, false);
        } else {
            f.bluetooth.devices = [];
            f.bluetooth.notify('devices');
            assert.equal(headset.handlers.size, 0);
            // Discovery can continue after a disappearing device, until closed.
            f.backend.setActive(false, 0);
        }
        assert.equal(headset.cancellations, 1);
        assert.equal(pairing.stops, 1);
        assert.equal(f.snapshot().bluetooth.pairing, false);
        assert.deepEqual(oldRadio.scanCalls, ['start', 'stop']);
    });
}

test('Bluetooth discovery automatically follows visibility, tab, power, and disposal', t => {
    const headset = device();
    const f = fixture(t, { devices: [headset] });
    f.backend.start();
    f.backend.setActive(false, 0);
    f.backend.setActive(true, 1);
    f.backend.syncDiscovery();
    assert.deepEqual(f.radio.scanCalls, []);
    f.backend.setActive(true, 0);
    f.backend.setActive(true, 0);
    f.bluetooth.notify('devices');
    assert.deepEqual(f.radio.scanCalls, ['start'], 'repeated reconciliation must not toggle scanning');
    f.backend.deviceAction(f.snapshot().bluetooth.devices[0].key);
    f.backend.toggleBluetoothPower();
    assert.equal(f.radio.powered, false);
    assert.deepEqual(f.radio.scanCalls, ['start', 'stop']);
    assert.equal(headset.cancellations, 1);
    f.backend.syncDiscovery();
    assert.equal(f.radio.scanCalls.length, 2);
    f.backend.toggleBluetoothPower();
    assert.equal(f.radio.powered, true);
    assert.deepEqual(f.radio.scanCalls, ['start', 'stop', 'start']);
    f.backend.setActive(true, 1);
    f.backend.setActive(true, 1);
    assert.deepEqual(f.radio.scanCalls, ['start', 'stop', 'start', 'stop']);
    f.backend.setActive(true, 0);
    f.backend.setActive(false, 0);
    f.backend.setActive(false, 0);
    assert.deepEqual(f.radio.scanCalls.slice(-2), ['start', 'stop']);
    f.backend.setActive(true, 0);
    const count = f.radio.scanCalls.length;
    f.backend.dispose();
    f.backend.dispose();
    assert.equal(f.radio.scanCalls.length, count + 1);
    assert.equal(f.radio.scanCalls.at(-1), 'stop');
});

test('Bluetooth discovery starts for late adapters and native power-on while the tab is visible', t => {
    const f = fixture(t, { radio: null });
    f.backend.start();
    f.backend.setActive(true, 0);
    const radio = adapter({ powered: false });
    f.bluetooth.adapter = radio;
    f.bluetooth.notify('adapter');
    assert.deepEqual(radio.scanCalls, []);
    radio.powered = true;
    radio.notify('powered');
    assert.deepEqual(radio.scanCalls, ['start']);
    radio.powered = false;
    radio.notify('powered');
    assert.deepEqual(radio.scanCalls, ['start', 'stop']);
    radio.powered = true;
    radio.notify('powered');
    const replacement = adapter({ path: '/org/bluez/hci1' });
    f.bluetooth.adapter = replacement;
    f.bluetooth.notify('adapter');
    assert.deepEqual(radio.scanCalls, ['start', 'stop', 'start', 'stop']);
    assert.deepEqual(replacement.scanCalls, ['start']);
    f.bluetooth.adapter = null;
    f.bluetooth.notify('adapter');
    assert.deepEqual(replacement.scanCalls, ['start', 'stop']);
});

test('Bluetooth discovery owns a session without stopping another client scanning', t => {
    const radio = adapter({ discovering: true });
    let owned = 0;
    radio.start_discovery = () => {
        radio.scanCalls.push('start');
        owned++;
        radio.discovering = true;
        radio.notify('discovering');
    };
    radio.stop_discovery = () => {
        radio.scanCalls.push('stop');
        assert.equal(owned, 1);
        owned--;
        radio.discovering = true; // Another client's discovery reference remains active.
        radio.notify('discovering');
    };
    const f = fixture(t, { radio });
    f.backend.start();
    f.backend.setActive(false, 0);
    assert.deepEqual(radio.scanCalls, []);
    f.backend.setActive(true, 0);
    f.backend.setActive(true, 0);
    assert.equal(owned, 1);
    assert.deepEqual(radio.scanCalls, ['start']);
    f.backend.setActive(true, 1);
    assert.equal(owned, 0);
    assert.equal(radio.discovering, true);
    f.backend.dispose();
    assert.deepEqual(radio.scanCalls, ['start', 'stop']);
});

test('synchronous discovery notifications cannot restart scanning during a delayed power-off', t => {
    const radio = adapter();
    let powered = true, requested = null;
    Object.defineProperty(radio, 'powered', {
        get: () => powered,
        set(value) { requested = value; radio.notify('discoverable'); },
    });
    radio.start_discovery = () => {
        radio.scanCalls.push('start'); radio.discovering = true; radio.notify('discovering');
    };
    radio.stop_discovery = () => {
        radio.scanCalls.push('stop'); radio.discovering = false; radio.notify('discovering');
    };
    const f = fixture(t, { radio });
    f.backend.start();
    f.backend.setActive(true, 0);
    assert.deepEqual(radio.scanCalls, ['start']);
    f.backend.toggleBluetoothPower();
    assert.equal(requested, false);
    assert.equal(radio.powered, true, 'native powered notification has not arrived yet');
    radio.notify('discovering');
    assert.deepEqual(radio.scanCalls, ['start', 'stop']);
    powered = false;
    radio.notify('powered');
    powered = true;
    radio.notify('powered');
    assert.deepEqual(radio.scanCalls, ['start', 'stop', 'start']);
    f.backend.setActive(false, 0);
    assert.deepEqual(radio.scanCalls, ['start', 'stop', 'start', 'stop']);
});

test('failed discovery starts report errors without spinning or stopping an unowned session', t => {
    const radio = adapter();
    let fail = true;
    radio.start_discovery = () => {
        radio.scanCalls.push('start');
        if (fail) throw new Error('Discovery unavailable');
        radio.discovering = true;
    };
    const f = fixture(t, { radio });
    f.backend.start();
    f.backend.setActive(true, 0);
    assert.equal(f.snapshot().bluetooth.error, 'Discovery unavailable');
    f.timers.advance();
    assert.deepEqual(radio.scanCalls, ['start']);
    f.backend.setActive(false, 0);
    assert.deepEqual(radio.scanCalls, ['start']);
    fail = false;
    f.backend.setActive(true, 0);
    assert.deepEqual(radio.scanCalls, ['start', 'start']);
    assert.equal(f.snapshot().bluetooth.error, '');
    f.backend.setActive(false, 0);
    assert.deepEqual(radio.scanCalls, ['start', 'start', 'stop']);
});

for (const leave of ['close', 'Audio tab']) {
    test(`leaving Bluetooth via ${leave} recovers discovery after a silently failed power-off`, t => {
        const radio = adapter();
        const writes = [];
        Object.defineProperty(radio, 'powered', {
            get: () => true,
            set(value) { writes.push(value); radio.notify('powered'); },
        });
        const f = fixture(t, { radio });
        f.backend.start();
        f.backend.setActive(true, 0);
        f.backend.toggleBluetoothPower();
        assert.deepEqual(writes, [false]);
        radio.notify('discovering');
        assert.deepEqual(radio.scanCalls, ['start', 'stop']);
        f.backend.setActive(leave !== 'close', leave === 'close' ? 0 : 1);
        assert.deepEqual(radio.scanCalls, ['start', 'stop']);
        f.backend.setActive(true, 0);
        assert.deepEqual(radio.scanCalls, ['start', 'stop', 'start']);
    });
}

test('nested adapter replacement during discovery cleanup starts only the final adapter', t => {
    const f = fixture(t);
    const intermediate = adapter({ path: '/org/bluez/hci1' });
    const final = adapter({ path: '/org/bluez/hci2' });
    f.radio.stop_discovery = () => {
        f.radio.scanCalls.push('stop');
        f.radio.discovering = false;
        assert.equal(f.radio.handlers.size, 0, 'old adapter notify must be disconnected before stopping');
        f.radio.notify('discovering');
        f.bluetooth.adapter = final;
        f.bluetooth.notify('adapter');
    };
    f.backend.start();
    f.backend.setActive(true, 0);
    f.bluetooth.adapter = intermediate;
    f.bluetooth.notify('adapter');
    assert.equal(f.backend.adapter, final);
    assert.equal(f.backend.discoveryAdapter, final);
    assert.deepEqual(f.radio.scanCalls, ['start', 'stop']);
    assert.deepEqual(intermediate.scanCalls, []);
    assert.deepEqual(final.scanCalls, ['start']);
    assert.equal(intermediate.handlers.size, 0);
    assert.equal(final.handlers.size, 1);
});

test('disposal during a discovery stop cannot install an Audio poll afterwards', t => {
    const f = fixture(t);
    f.radio.stop_discovery = () => {
        f.radio.scanCalls.push('stop');
        f.radio.discovering = false;
        f.radio.notify('discovering');
    };
    f.backend.start();
    f.backend.setActive(true, 0);
    f.radio.connect('notify', (_, property) => {
        if (property.name === 'discovering' && !f.radio.discovering) f.backend.dispose();
    });
    f.backend.setActive(true, 1);
    assert.equal(f.backend.disposed, true);
    assert.equal(f.backend.poll, 0);
    f.timers.advance(); // Drain stopped policy workers' deferred initial sends.
    assert.equal(f.timers.sources.size, 0);
    assert.deepEqual(f.radio.scanCalls, ['start', 'stop']);
});

for (const cause of ['disposal', 'adapter replacement']) {
    test(`${cause} during discovery cleanup prevents a stale Bluetooth power write`, t => {
        const f = fixture(t);
        const writes = [];
        const replacement = adapter({ path: '/org/bluez/hci1' });
        Object.defineProperty(f.radio, 'powered', {
            get: () => true,
            set(value) { writes.push(value); },
        });
        f.radio.stop_discovery = () => {
            f.radio.scanCalls.push('stop');
            f.radio.discovering = false;
            f.radio.notify('discovering');
        };
        f.backend.start();
        f.backend.setActive(true, 0);
        f.radio.connect('notify', (_, property) => {
            if (property.name !== 'discovering' || f.radio.discovering) return;
            if (cause === 'disposal') f.backend.dispose();
            else {
                f.bluetooth.adapter = replacement;
                f.bluetooth.notify('adapter');
            }
        });
        f.backend.toggleBluetoothPower();
        assert.deepEqual(writes, []);
        assert.deepEqual(f.radio.scanCalls, ['start', 'stop']);
        if (cause === 'disposal') {
            f.timers.advance();
            assert.equal(f.timers.sources.size, 0);
        } else {
            assert.equal(replacement.powered, true);
            assert.deepEqual(replacement.scanCalls, ['start']);
        }
    });
}

for (const retry of ['close', 'dispose']) {
    test(`failed discovery stops retain cleanup ownership until a later ${retry}`, t => {
        const f = fixture(t);
        let fail = true;
        f.radio.stop_discovery = () => {
            f.radio.scanCalls.push('stop');
            if (fail) throw new Error('Discovery stop unavailable');
            f.radio.discovering = false;
            f.radio.notify('discovering');
        };
        f.backend.start();
        f.backend.setActive(true, 0);
        f.backend.setActive(false, 0);
        assert.equal(f.snapshot().bluetooth.error, 'Discovery stop unavailable');
        assert.equal(f.backend.discoveryAdapter, f.radio);
        f.timers.advance();
        assert.deepEqual(f.radio.scanCalls, ['start', 'stop'], 'no background cleanup retry loop');
        fail = false;
        if (retry === 'dispose') f.backend.dispose();
        else f.backend.setActive(false, 0);
        assert.deepEqual(f.radio.scanCalls, ['start', 'stop', 'stop']);
        assert.equal(f.backend.discoveryAdapter, null);
        if (retry === 'close') {
            f.backend.setActive(true, 0);
            assert.deepEqual(f.radio.scanCalls, ['start', 'stop', 'stop', 'start']);
        } else assert.equal(f.timers.sources.size, 0);
    });
}

for (const connected of [false, true]) {
    test(`unpaired ${connected ? 'connected' : 'discovered'} Bluetooth devices cannot be forgotten`, t => {
        const candidate = device({ connected, paired: false });
        const f = fixture(t, { devices: [candidate] });
        f.backend.start();
        const key = f.snapshot().bluetooth.devices[0].key;
        f.backend.forget(key);
        assert.deepEqual(f.radio.removed, []);
        candidate.paired = true;
        candidate.notify('paired');
        f.backend.forget(key);
        assert.deepEqual(f.radio.removed, [candidate]);
    });
}

test('status is read-only: no refresh, polling, discovery, pairing or native writes', t => {
    const headset = device({ connected: true, name: 'Headset' });
    const f = fixture(t, { devices: [headset] });
    f.backend.start();
    f.timers.advance();
    const helperCount = f.helpers.length, changes = f.changes();
    for (let i = 0; i < 5; i++) {
        const status = JSON.parse(f.backend.status());
        assert.equal(status.class, 'bluetooth-connected');
        assert.equal(status.tooltip, 'Output: Speakers (40%)\nInput: Microphone (70%)\nBluetooth: Headset');
    }
    assert.equal(f.helpers.length, helperCount);
    assert.equal(f.changes(), changes);
    assert.equal(f.timers.sources.size, 0);
    assert.equal(f.backend.opened, false);
    assert.equal(f.radio.powered, true);
    assert.deepEqual(f.radio.scanCalls, []);
    assert.deepEqual(f.radio.removed, []);
    assert.deepEqual(headset.operations, []);
    assert.equal(headset.cancellations, 0);
    for (const node of [...f.outputs, ...f.inputs]) {
        assert.deepEqual(node.routeWrites, []);
        assert.deepEqual(node.defaultWrites, []);
    }
});

test('audio-tab polling is bounded by active tab and close lifecycle', t => {
    const f = fixture(t);
    f.backend.start();
    f.load('output', []);
    f.load('input', []);
    f.backend.setActive(true, 1);
    assert.ok(f.backend.poll);
    f.load('output', []);
    f.load('input', []);
    const count = f.helpers.length;
    f.timers.advance(2000);
    assert.equal(f.helpers.length, count + 2);
    f.backend.setActive(true, 0);
    assert.equal(f.backend.poll, 0);
    f.load('output', []);
    f.load('input', []);
    f.timers.advance(4000);
    assert.equal(f.helpers.length, count + 2);
    f.backend.setActive(false, 1);
    assert.equal(f.backend.poll, 0);
});

test('dispose cancels native waiters, helper selections/listings and every subscription/timer exactly once', async t => {
    const route = { index: 1, name: 'headphones', available: YES };
    const headset = device();
    const inputCard = card();
        const f = fixture(t, { outputs: [endpoint({ routes: [route], acceptRoute: false })], cards: [inputCard], devices: [headset] });
    const { backend, audio, wp, bluetooth, radio } = f;
    backend.start();
    f.load('output', [choice({ routeId: 1 })]);
    f.load('input', [choice({ nativeCard: inputCard, key: 'inactive-input', name: '', nodeId: null, nodeSerial: '' })]);
    backend.setActive(true, 1);
    const outputListing = f.listing('output'), inputListing = f.listing('input');
    backend.select('output', 'card:port'); // Pending native acknowledgement.
    backend.select('input', 'inactive-input'); // Pending helper selection.
    const selection = f.helpers.at(-1);
    backend.scheduleRefresh();
    assert.equal(backend.waiters.size, 1);
    assert.ok(backend.poll);
    assert.ok(backend.refreshTimer);
    const objects = [audio, wp, bluetooth, radio, headset, inputCard, ...f.outputs, ...f.inputs,
        audio.default_speaker, audio.default_microphone];
    backend.dispose();
    backend.dispose();
    await settle();
    f.timers.advance(); // Drain the policy's deferred initial sends; stopped requests send nothing.
    assert.equal(backend.waiters.size, 0);
    assert.equal(f.timers.sources.size, 0);
    for (const object of objects) {
        assert.equal(object.handlers.size, 0);
        assert.equal(object.connections, object.disconnections);
    }
    for (const helper of [outputListing, inputListing, selection]) assert.equal(helper.stops, 1);
    assert.ok(f.helpers.every(helper => helper.done));
    assert.equal(backend.records.size, 0);
    assert.equal(backend.bluetoothRecords.size, 0);
    const count = f.helpers.length, changes = f.changes();
    backend.start();
    backend.setActive(true, 1);
    backend.refresh();
    backend.changed();
    backend.syncDiscovery();
    backend.toggleBluetoothPower();
    audio.notify('speakers');
    wp.emit('ready');
    bluetooth.notify('devices');
    f.timers.advance(5000);
    await settle();
    assert.equal(f.helpers.length, count);
    assert.equal(f.changes(), changes);
    assert.equal(f.timers.sources.size, 0);
    assert.equal(radio.powered, true);
});

test('dispose during Bluetooth activity cancels scanning/pairing and ignores late native/helper callbacks', t => {
    const paired = device({ address: 'AA:02', paired: true }), unpaired = device();
    const f = fixture(t, { devices: [paired, unpaired] });
    f.backend.start();
    f.backend.setActive(true, 0);
    const keyFor = address => f.snapshot().bluetooth.devices.find(entry => entry.address === address).key;
    f.backend.deviceAction(keyFor(paired.address));
    f.backend.deviceAction(keyFor(unpaired.address));
    const pairing = f.helpers.at(-1);
    f.backend.dispose();
    f.timers.advance();
    assert.equal(f.timers.sources.size, 0);
    const changes = f.changes();
    paired.operations[0].callback(paired, { error: new Error('Late native failure') });
    pairing.options.onChunk('Late PIN');
    pairing.options.onExit(new Error('Late helper failure'), '');
    assert.equal(f.backend.bluetoothError, '');
    assert.equal(f.backend.pairingMessage, null);
    assert.equal(f.changes(), changes);
    assert.equal(f.timers.sources.size, 0);
    assert.equal(unpaired.cancellations, 1);
    assert.equal(pairing.stops, 1);
    assert.deepEqual(f.radio.scanCalls, ['start', 'stop']);
    for (const object of [paired, unpaired, f.radio]) assert.equal(object.handlers.size, 0);
});
}

// Tests consolidated from logic.test.js.
{
function choice(extra = {}) {
    return {
        key: 'card-output:01:port:02', label: 'Headphones',
        description: 'Built-in Audio — Headphones', name: 'alsa_output.pci',
        portName: 'analog-output-headphones', card: 'alsa_card.pci',
        port: true, portActive: false, nodeId: 42, nodeSerial: '1001',
        cardId: 10, cardSerial: '1000', routeId: 7, ...extra,
    };
}

function endpoint(extra = {}) {
    return {
        key: 'output:42', id: 42, serial: '1001', name: 'alsa_output.pci', description: 'Built-in Audio',
        cardId: 10, cardSerial: '1000', isDefault: false, routesReliable: true,
        routeName: 'analog-output-headphones',
        routes: [{ name: 'analog-output-headphones', index: 7, available: true }],
        ...extra,
    };
}

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

test('volumeValue clamps finite numbers to 150% output and 100% input without quantizing', () => {
    for (const [value, output, input] of [
        [-Number.MAX_VALUE, 0, 0], [-0.1, 0, 0], [-0, 0, 0], [0, 0, 0],
        [Number.MIN_VALUE, Number.MIN_VALUE, Number.MIN_VALUE], [0.375, 0.375, 0.375],
        [1, 1, 1], [1.25, 1.25, 1], [1.5, 1.5, 1], [1.5001, 1.5, 1],
        [Number.MAX_VALUE, 1.5, 1],
    ]) {
        assert.equal(volumeValue('output', value), output, `output ${value}`);
        assert.equal(volumeValue('input', value), input, `input ${value}`);
    }
});

test('volumeValue rejects non-finite values and coercible non-numbers for both kinds', () => {
    for (const kind of ['output', 'input']) {
        for (const value of [NaN, Infinity, -Infinity, undefined, null, true, false, '0.5', '', [], [0.5], {}, 1n, new Number(0.5)]) {
            assert.throws(() => volumeValue(kind, value), /Invalid audio volume/, `${kind}: ${String(value)}`);
        }
    }
});

test('volumeValue rejects unsupported kinds even with a valid numeric volume', () => {
    for (const kind of ['sink', 'source', 'Output', 'INPUT', '', undefined, null, 0, {}, new String('output')])
        assert.throws(() => volumeValue(kind, 0.5), /Invalid audio volume/);
});

test('parseChoices accepts exact native route, device-only, input, and inactive-profile bindings', () => {
    const entries = [
        choice({ portActive: true }),
        choice({ key: 'sink:usb', name: 'alsa_output.usb', port: false, portName: '', routeId: null,
            nodeId: 43, nodeSerial: '1002', card: '', cardId: null, cardSerial: '' }),
        choice({ key: 'source:01:port:02', name: 'alsa_input.pci', label: 'Microphone',
            portName: 'analog-input-mic', nodeId: 9, nodeSerial: '1003', routeId: 0 }),
        choice({ key: 'card-output:01:port:03', name: '', label: 'Speakers',
            portName: 'analog-output-speaker', nodeId: null, nodeSerial: '', routeId: 1 }),
    ];
    for (const raw of [entries, `${JSON.stringify(entries)}\n`]) {
        const parsed = parseChoices(raw);
        assert.deepEqual(parsed, entries);
        assert.notEqual(parsed, entries);
        parsed.forEach((row, index) => assert.notEqual(row, entries[index]));
    }
    assert.deepEqual(parseChoices('[]\n'), []);
});

test('parseChoices keeps only choice metadata, never helper-provided native state or defaults', () => {
    const row = choice({ portName: '[Out] Headphones', card: 'alsa_card.pci-0000_00_1f.3' });
    for (const isDefault of [false, true, 'true', null]) {
        assert.deepEqual(parseChoices(JSON.stringify([
            { ...row, default: isDefault, isDefault: true, kind: 'output', volume: 50,
                muted: false, future: { version: 2 }, ['__proto__']: { default: true } },
        ])), [row]);
    }
});

test('parseChoices preserves JSON-escaped Unicode metadata and permits empty optional strings', () => {
    const text = '"\\\n\r\t\u001fCafé 🎧 <A&B>';
    const row = choice(Object.fromEntries(['key', 'label', 'description', 'name', 'portName', 'card'].map(field => [field, text])));
    assert.deepEqual(parseChoices(JSON.stringify([row])), [row]);
    const empty = choice({ label: '', description: '', name: '', portName: '', card: '',
        port: false, routeId: null, cardId: null, cardSerial: '' });
    assert.deepEqual(parseChoices(JSON.stringify([empty])), [empty]);
});

test('parseChoices requires an array of at most 4096 entries, preserving row order', () => {
    for (const raw of ['', '[', '[{}', '[] []']) assert.throws(() => parseChoices(raw), SyntaxError);
    for (const value of [null, false, 1, '[]', {}, { entries: [] }])
        assert.throws(() => parseChoices(JSON.stringify(value)), /Invalid audio device list/);
    const rows = Array.from({ length: 4096 }, (_, index) => choice({ key: `row:${4095 - index}` }));
    assert.deepEqual(parseChoices(rows), rows);
    assert.deepEqual(parseChoices(JSON.stringify(rows)), rows);
    rows.push(choice({ key: 'excess' }));
    for (const raw of [rows, JSON.stringify(rows)])
        assert.throws(() => parseChoices(raw), /Invalid audio device list/);
});

test('parseChoices rejects non-object rows and does not silently drop malformed entries', () => {
    for (const invalid of [null, false, 1, 'device', [], {}]) {
        assert.throws(() => parseChoices(JSON.stringify([choice(), invalid])), /Invalid audio device entry/, JSON.stringify(invalid));
    }
});

for (const field of ['key', 'label', 'description', 'name', 'portName', 'card']) {
    test(`parseChoices validates required string field ${field}, NUL, and the 8192-character bound`, () => {
        const missing = choice();
        delete missing[field];
        assert.throws(() => parseChoices(JSON.stringify([missing])), /Invalid audio device entry/, 'missing');
        for (const value of [null, false, 1, [], {}, 'before\0after', 'x'.repeat(8193)]) {
            assert.throws(() => parseChoices(JSON.stringify([choice({ [field]: value })])), /Invalid audio device entry/, String(value).slice(0, 40));
        }
        const boundary = choice({ [field]: 'x'.repeat(8192) });
        assert.deepEqual(parseChoices(JSON.stringify([boundary])), [boundary]);
    });
}

test('parseChoices requires real port/portActive booleans and consistent route shapes', () => {
    for (const field of ['portActive', 'port']) {
        const missing = choice();
        delete missing[field];
        assert.throws(() => parseChoices([missing]), /Invalid audio device entry/, `missing ${field}`);
        for (const value of [null, 0, 1, 'true', 'false', [], {}])
            assert.throws(() => parseChoices([choice({ [field]: value })]), /Invalid audio device entry/, field);
    }
    for (const row of [choice({ port: false }), choice({ portName: '' }), choice({ routeId: null }),
        choice({ port: false, portName: '' }), choice({ port: false, routeId: null })])
        assert.throws(() => parseChoices([row]), /Invalid audio device entry/);
    for (const portActive of [false, true]) {
        for (const extra of [{}, { port: false, portName: '', routeId: null }]) {
            const row = choice({ ...extra, portActive });
            assert.deepEqual(parseChoices([row]), [row]);
        }
    }
});

for (const [idField, serialField] of [['nodeId', 'nodeSerial'], ['cardId', 'cardSerial']]) {
    test(`parseChoices requires a positive uint32 ${idField} and a decimal string ${serialField}`, () => {
        for (const value of [undefined, false, true, 0, -1, 1.5, 0x100000000, NaN, Infinity, '42', [], {}])
            assert.throws(() => parseChoices([choice({ [idField]: value })]), /Invalid audio device entry/, String(value));
        for (const value of [undefined, null, false, 1001, '', '-1', '+1', '1.0', '1e3', ' 1', '1\n',
            '１２', '1\0', '1'.repeat(21), [], {}])
            assert.throws(() => parseChoices([choice({ [serialField]: value })]), /Invalid audio device entry/, String(value));
        for (const id of [1, 0xffffffff]) {
            for (const serial of ['0', '0001', '18446744073709551615']) {
                const row = choice({ [idField]: id, [serialField]: serial });
                assert.deepEqual(parseChoices(JSON.stringify([row])), [row]);
            }
        }
        for (const field of [idField, serialField]) {
            const missing = choice();
            delete missing[field];
            assert.throws(() => parseChoices([missing]), /Invalid audio device entry/, `missing ${field}`);
        }
    });
}

test('parseChoices permits null bindings only with empty serials and a card-bound inactive route', () => {
    const inactive = choice({ nodeId: null, nodeSerial: '', name: '' });
    const cardless = choice({ cardId: null, cardSerial: '', card: '' });
    assert.deepEqual(parseChoices([inactive, { ...cardless, key: 'cardless' }]), [inactive, { ...cardless, key: 'cardless' }]);
    for (const row of [choice({ nodeId: null }), choice({ nodeSerial: '' }), choice({ cardId: null }),
        choice({ cardSerial: '' }), { ...inactive, cardId: null, cardSerial: '' },
        { ...inactive, port: false, portName: '', routeId: null }])
        assert.throws(() => parseChoices([row]), /Invalid audio device entry/);
});

test('parseChoices requires an explicit null routeId or a nonnegative int32 route index', () => {
    for (const value of [undefined, false, true, -1, 0.5, 0x80000000, NaN, Infinity, '7', [], {}])
        assert.throws(() => parseChoices([choice({ routeId: value })]), /Invalid audio device entry/, String(value));
    const missing = choice();
    delete missing.routeId;
    assert.throws(() => parseChoices([missing]), /Invalid audio device entry/);
    for (const routeId of [0, 0x7fffffff]) {
        const row = choice({ routeId });
        assert.deepEqual(parseChoices(JSON.stringify([row])), [row]);
    }
});

test('parseChoices requires nonempty, unique stable keys, not unique labels or native identities', () => {
    assert.throws(() => parseChoices([choice({ key: '' })]), /Invalid audio device entry/);
    assert.throws(() => parseChoices([choice(), choice({ label: 'Different label', name: 'different-node' })]), /Invalid audio device entry/);
    const rows = ['__proto__', 'constructor', 'toString', 'distinct-key'].map(key => choice({ key }));
    assert.deepEqual(parseChoices(JSON.stringify(rows)), rows);
});

function policyMessages() {
    return [
        { type: 'snapshot', id: 1 },
        { type: 'action', id: 2, operation: 'profile', card: { id: 10, serial: '1000' }, profile: 0 },
        { type: 'action', id: 3, operation: 'route', node: { id: 42, serial: '1001' }, route: 7 },
        { type: 'action', id: 4, operation: 'default', node: { id: 42, serial: '1001' } },
        { type: 'result', rows: [choice({ key: 'z' }), choice({ key: 'a' })] },
        { type: 'result', ok: true },
        { type: 'result', ok: false, error: 'The selected device disappeared.' },
    ];
}

test('policyEvent accepts only typed snapshot, profile/route/default action, and result shapes', () => {
    for (const event of [...policyMessages(), { type: 'result', rows: [] }]) {
        assert.deepEqual(policyEvent(`${JSON.stringify(event)}\n`), event);
        const reordered = Object.fromEntries(Object.entries(event).reverse());
        assert.deepEqual(policyEvent(JSON.stringify(reordered)), event);
    }
});

test('policyEvent rejects malformed JSON, non-object envelopes, and unknown message types', () => {
    for (const raw of ['', '{', '{"type":"snapshot",}', '{}\n{}'])
        assert.throws(() => policyEvent(raw), SyntaxError, raw);
    for (const event of [null, false, 0, 'snapshot', [], [{}], {},
        ...['', 'Snapshot', 'rows', 'select', 'constructor', null, 1, {}, []].map(type => ({ type }))])
        assert.throws(() => policyEvent(JSON.stringify(event)), /audio policy message/, JSON.stringify(event));
});

test('policyEvent requires bounded integer RPC IDs only for snapshots and actions', () => {
    for (const event of policyMessages().filter(event => event.type !== 'result')) {
        const missing = { ...event };
        delete missing.id;
        assert.throws(() => policyEvent(JSON.stringify(missing)), /audio policy message fields/);
        for (const id of [null, false, true, 0, -1, 1.5, 257, Number.MAX_SAFE_INTEGER,
            Number.MAX_SAFE_INTEGER + 1, '1', [], {}])
            assert.throws(() => policyEvent(JSON.stringify({ ...event, id })), /audio policy message fields/, String(id));
        for (const id of [1, 256])
            assert.deepEqual(policyEvent(JSON.stringify({ ...event, id })), { ...event, id });
    }
    for (const event of policyMessages().filter(event => event.type === 'result'))
        assert.throws(() => policyEvent(JSON.stringify({ ...event, id: 1 })), /audio policy message fields/);
});

test('policyEvent rejects unknown fields and excess payloads instead of expanding native operations', () => {
    for (const event of policyMessages()) {
        for (const field of ['payload', 'value', 'volume', 'mute', 'command', 'constructor', '__proto__']) {
            assert.throws(() => policyEvent(JSON.stringify({ ...event, [field]: null })), /audio policy message fields/, `${event.type}: ${field}`);
        }
    }
    for (const event of [
        { type: 'snapshot', id: 1, rows: [] },
        { type: 'snapshot', id: 1, snapshot: {} },
        { type: 'action', id: 1, operation: 'default', node: { id: 42, serial: '1001' }, route: 7 },
        { type: 'action', id: 1, operation: 'route', node: { id: 42, serial: '1001' }, route: 7, card: { id: 10, serial: '1000' } },
        { type: 'action', id: 1, operation: 'profile', card: { id: 10, serial: '1000' }, profile: 0, node: { id: 42, serial: '1001' } },
    ]) assert.throws(() => policyEvent(JSON.stringify(event)), /audio policy message fields/);
});

test('policyEvent requires complete actions and does not accept unsupported or mistyped operations', () => {
    for (const event of policyMessages().filter(event => event.type === 'action')) {
        for (const field of Object.keys(event)) {
            const missing = { ...event };
            delete missing[field];
            assert.throws(() => policyEvent(JSON.stringify(missing)), /audio (policy message|action)/, `missing ${field}`);
        }
        for (const operation of ['', 'Profile', 'volume', 'mute', 'select', 'constructor', null, false, 1, [], {}])
            assert.throws(() => policyEvent(JSON.stringify({ ...event, operation })), /Invalid native audio action/, String(operation));
    }
});

test('policyEvent validates exact two-field native identities without ID or serial coercion', () => {
    const invalidIdentities = [null, false, 42, '42', [], {}, { id: 42 }, { serial: '1001' },
        { id: 42, serial: '1001', name: 'alsa_output.pci' },
        { id: 42, serial: '1001', ['__proto__']: {} },
        ...[null, false, true, 0, -1, 1.5, 0x100000000, '42', [], {}].map(id => ({ id, serial: '1001' })),
        ...[null, false, 1001, '', '-1', '+1', '1.0', '1e3', ' 1', '1\n', '１２', '1\0', '1'.repeat(21), [], {}]
            .map(serial => ({ id: 42, serial }))];
    for (const event of policyMessages().filter(event => event.type === 'action')) {
        const field = event.operation === 'profile' ? 'card' : 'node';
        for (const value of invalidIdentities)
            assert.throws(() => policyEvent(JSON.stringify({ ...event, [field]: value })), /Invalid native audio action/, JSON.stringify(value));
        for (const id of [1, 0xffffffff]) {
            for (const serial of ['0', '0001', '18446744073709551615']) {
                const valid = { ...event, [field]: { id, serial } };
                assert.deepEqual(policyEvent(JSON.stringify(valid)), valid);
            }
        }
    }
});

test('policyEvent validates route and profile indices, including zero and int32 boundaries', () => {
    for (const event of policyMessages().filter(event => ['profile', 'route'].includes(event.operation))) {
        const field = event.operation;
        for (const value of [null, false, true, -1, 0.5, 0x80000000, '0', [], {}])
            assert.throws(() => policyEvent(JSON.stringify({ ...event, [field]: value })), /Invalid native audio action/, String(value));
        for (const value of [0, 0x7fffffff]) {
            const valid = { ...event, [field]: value };
            assert.deepEqual(policyEvent(JSON.stringify(valid)), valid);
        }
    }
});

test('policyEvent enforces mutually exclusive row, success, and failure result payloads', () => {
    for (const event of [
        { type: 'result' }, { type: 'result', ok: false }, { type: 'result', error: 'failed' },
        ...[null, 0, 1, 'true', 'false', [], {}].map(ok => ({ type: 'result', ok })),
        { type: 'result', ok: true, error: '' }, { type: 'result', ok: true, error: 'failed' },
        { type: 'result', rows: [], ok: true }, { type: 'result', rows: [], ok: false, error: 'failed' },
        { type: 'result', rows: [], error: 'failed' },
    ]) assert.throws(() => policyEvent(JSON.stringify(event)), /Invalid audio policy (result|message fields)/, JSON.stringify(event));
    for (const error of [null, false, 1, '', 'before\0after', 'x'.repeat(8193), [], {}])
        assert.throws(() => policyEvent(JSON.stringify({ type: 'result', ok: false, error })), /Invalid audio policy result/);
    for (const error of ['failed', 'x'.repeat(8192), 'Café 🎧 "\\\n\r\t']) {
        const event = { type: 'result', ok: false, error };
        assert.deepEqual(policyEvent(JSON.stringify(event)), event);
    }
});

test('policyEvent validates every result row instead of accepting incomplete or duplicate entries', () => {
    for (const rows of [null, false, {}, [null], [choice(), {}], [choice(), choice()],
        [choice({ nodeId: '42' })], [choice({ portActive: 1 })], [choice({ routeId: null })]])
        assert.throws(() => policyEvent(JSON.stringify({ type: 'result', rows })), /Invalid audio device (list|entry)/);
});

test('policyEvent requires result rows to be an array, not a JSON-encoded string', () => {
    for (const rows of ['[]', JSON.stringify([choice()])])
        assert.throws(() => policyEvent(JSON.stringify({ type: 'result', rows })), /Invalid audio/);
});

test('policyEvent accepts 4096 ordered result rows and rejects excessive row payloads', () => {
    const rows = Array.from({ length: 4096 }, (_, index) => choice({ key: `row:${4095 - index}` }));
    const event = { type: 'result', rows };
    assert.deepEqual(policyEvent(JSON.stringify(event)), event);
    rows.push(choice({ key: 'excess' }));
    assert.throws(() => policyEvent(JSON.stringify(event)), /Invalid audio device list/);
});

test('selectionPlan uses exact ID/serial identity, not native names or the current default', () => {
    for (const isDefault of [false, true]) {
        const row = choice({ port: false, portName: '', routeId: null });
        const node = endpoint({ isDefault, routes: [], routesReliable: false, name: 'renamed-node' });
        const distractors = [endpoint({ key: 'reused-id', serial: '1002' }),
            endpoint({ key: 'same-name', id: 43, isDefault: !isDefault })];
        for (const nodes of [[...distractors, node], [node, ...distractors.reverse()]])
            assert.deepEqual(selectionPlan(row, nodes), { type: 'native', nodeKey: node.key, route: null });
    }
});

test('selectionPlan uses exact route index and name, preserving route index zero', () => {
    for (const [name, portName] of [['alsa_output.pci', 'analog-output-headphones'], ['alsa_input.pci', 'analog-input-mic']]) {
        for (const index of [0, 7, 0x7fffffff]) {
            const row = choice({ name, portName, routeId: index });
            const routes = [
                { name: 'unrelated', index, available: true },
                { name: portName, index: index === 7 ? 8 : 7, available: true },
                { name: portName, index, available: true },
            ];
            for (const ordered of [routes, [...routes].reverse()]) {
                const node = endpoint({ name, routeName: 'other-active-route', routes: ordered });
                assert.deepEqual(selectionPlan(row, [node]), { type: 'native', nodeKey: node.key, route: index });
            }
        }
    }
});

test('selectionPlan throws for missing, stale, or ambiguous endpoints instead of mutation fallback', () => {
    for (const nodes of [[], [endpoint({ id: 43 })], [endpoint({ serial: '1002' })],
        [endpoint({ id: '42' })], [endpoint({ serial: 1001 })],
        [endpoint({ key: 'first', isDefault: true }), endpoint({ key: 'second' })]]) {
        for (const entry of [choice(), choice({ port: false, portName: '', routeId: null })])
            assert.throws(() => selectionPlan(entry, nodes), /selected audio device disappeared or was replaced/);
    }
});

test('selectionPlan throws for missing, stale, unavailable, or ambiguous routes even with an unreliable getter', () => {
    const row = choice();
    const route = { name: row.portName, index: row.routeId, available: true };
    for (const routes of [[], [{ ...route, name: 'different-route' }], [{ ...route, name: row.portName.toUpperCase() }],
        [{ ...route, index: 8 }], [{ ...route, index: '7' }], [{ ...route, available: false }],
        [{ name: route.name, index: route.index }], [route, { ...route }]]) {
        for (const routesReliable of [true, false])
            assert.throws(() => selectionPlan(row, [endpoint({ routes, routesReliable })]), /selected audio port is no longer available/);
    }
    assert.deepEqual(selectionPlan(row, [endpoint({ routes: [
        { ...route, available: false }, route, { ...route, index: 8 },
    ] })]), { type: 'native', nodeKey: 'output:42', route: 7 });
});

test('selectionPlan never borrows a route from another endpoint on the same card', () => {
    assert.throws(() => selectionPlan(choice(), [endpoint({ routes: [] }), endpoint({ key: 'other', id: 43 })]),
        /selected audio port is no longer available/);
});

test('selectionPlan delegates only inactive profiles and ambiguous card-wide route getters to policy', () => {
    const inactive = choice({ nodeId: null, nodeSerial: '', name: '' });
    for (const nodes of [[], [endpoint()], [endpoint({ key: 'empty-name', name: '' })]])
        assert.deepEqual(selectionPlan(inactive, nodes), { type: 'policy', key: inactive.key });
    const row = Object.freeze(choice({ portActive: true }));
    const node = Object.freeze(endpoint({ routesReliable: false }));
    assert.deepEqual(selectionPlan(row, [node]), { type: 'policy', key: row.key });
    assert.deepEqual(selectionPlan({ ...row, portActive: false }, [node]), { type: 'policy', key: row.key });
    assert.deepEqual(selectionPlan(row, [endpoint()]), { type: 'native', nodeKey: node.key, route: 7 });
});

test('deviceChoices derives defaults from native endpoints and reliable active-route state without mutation', () => {
    const rows = Object.freeze([
        Object.freeze(choice({ key: 'speakers', portName: 'analog-output-speaker', routeId: 0, portActive: false, default: false })),
        Object.freeze(choice({ key: 'headphones', portActive: true, default: true })),
        Object.freeze(choice({ key: 'whole-device', port: false, portName: '', routeId: null, default: false })),
        Object.freeze(choice({ key: 'usb', name: 'alsa_output.usb', nodeId: 43, nodeSerial: '1002', default: true })),
        Object.freeze(choice({ key: 'inactive', nodeId: null, nodeSerial: '', name: '', portActive: true, default: true })),
    ]);
    const nodes = Object.freeze([
        Object.freeze(endpoint({ isDefault: true, routeName: 'analog-output-speaker', routes: [
            { name: 'analog-output-speaker', index: 0, available: true }, ...endpoint().routes,
        ] })),
        Object.freeze(endpoint({ key: 'output:43', id: 43, serial: '1002', name: 'alsa_output.usb' })),
    ]);
    const cards = Object.freeze([Object.freeze({ id: 10, serial: '1000' })]);
    const before = structuredClone({ rows, nodes, cards });
    const result = deviceChoices(rows, nodes, cards);
    assert.deepEqual(result, rows.map((row, index) => ({ ...row, default: index === 0 || index === 2 })));
    assert.deepEqual({ rows, nodes, cards }, before);
    assert.notEqual(result, rows);
    result.forEach((row, index) => assert.notEqual(row, rows[index]));
    const changed = deviceChoices(rows, [{ ...nodes[0], routeName: 'analog-output-headphones' }, nodes[1]], cards);
    assert.deepEqual(changed.map(row => row.default), [false, true, true, false, false]);
    const noDefault = deviceChoices(rows, nodes.map(node => ({ ...node, isDefault: false })), cards);
    assert.ok(noDefault.every(row => row.default === false));
});

test('deviceChoices resolves live input defaults by exact binding without card metadata', () => {
    const rows = [
        choice({ key: 'mic', name: 'alsa_input.pci', portName: 'analog-input-mic', routeId: 0,
            nodeId: 9, nodeSerial: '1003', card: '', cardId: null, cardSerial: '', portActive: false }),
        choice({ key: 'line', name: 'alsa_input.pci', portName: 'analog-input-linein', routeId: 1,
            nodeId: 9, nodeSerial: '1003', card: '', cardId: null, cardSerial: '', portActive: true }),
    ];
    const node = endpoint({ key: 'input:9', id: 9, serial: '1003', name: 'alsa_input.pci',
        cardId: null, cardSerial: '', isDefault: true, routeName: 'analog-input-mic', routes: [
            { name: 'analog-input-mic', index: 0, available: true },
            { name: 'analog-input-linein', index: 1, available: true },
        ] });
    assert.deepEqual(deviceChoices(rows, [node]), rows.map((row, index) => ({ ...row, default: index === 0 })));
});

test('deviceChoices removes ghosts and stale native/card bindings, falling back for unrepresented endpoints', () => {
    const live = choice({ key: 'live' });
    const ghosts = [
        choice({ key: 'missing-node', nodeId: 99 }), choice({ key: 'reused-node', nodeSerial: '9999' }),
        choice({ key: 'wrong-name', name: 'disconnected' }), choice({ key: 'wrong-card', cardId: 11 }),
        choice({ key: 'reused-card', cardSerial: '9999' }), choice({ key: 'wrong-route-index', routeId: 8 }),
        choice({ key: 'wrong-route-name', portName: 'absent' }),
    ];
    assert.deepEqual(deviceChoices([...ghosts, live], [endpoint()]), [{ ...live, default: false }]);
    const fallback = deviceChoices([], [endpoint({ isDefault: true })]);
    for (const ghost of ghosts) {
        assert.deepEqual(deviceChoices([ghost], [endpoint({ isDefault: true })]), fallback, ghost.key);
        assert.deepEqual(deviceChoices([ghost], []), [], ghost.key);
    }
    for (const routes of [[], [{ ...endpoint().routes[0], available: false }]])
        assert.deepEqual(deviceChoices([live], [endpoint({ routes })]), deviceChoices([], [endpoint({ routes })]));
    assert.deepEqual(deviceChoices([live], []), []);
});

test('deviceChoices respects omissions only for identities resolved by the helper snapshot', () => {
    const node = endpoint({ isDefault: true });
    const resolved = [{ id: node.id, serial: node.serial }];
    assert.deepEqual(deviceChoices([], [node], [], resolved), []);
    const row = choice();
    assert.equal(deviceChoices([row], [node], [], resolved).length, 1);
    assert.equal(deviceChoices([], [node], [], [{ id: node.id, serial: 'replaced' }]).length, 1);
    assert.equal(deviceChoices([], [node], [], [{ id: node.id + 1, serial: node.serial }]).length, 1);
    assert.equal(deviceChoices([], [node]).length, 1, 'failed/missing metadata retains native fallback');
});

test('deviceChoices keeps inactive profiles only while the exact native card is live', () => {
    const inactive = choice({ key: 'inactive', nodeId: null, nodeSerial: '', name: '', portActive: true, default: true });
    for (const cards of [[], [{ id: 11, serial: '1000' }], [{ id: 10, serial: '1002' }],
        [{ id: '10', serial: '1000' }], [{ id: 10, serial: 1000 }]])
        assert.deepEqual(deviceChoices([inactive], [], cards), []);
    assert.deepEqual(deviceChoices([inactive], [], [{ id: 10, serial: '1000' }]), [{ ...inactive, default: false }]);
    assert.deepEqual(deviceChoices([inactive], []), []);
});

test('deviceChoices preserves policy row order across native endpoint and profile changes', () => {
    const active = choice({ key: 'active' });
    const whole = choice({ key: 'whole', port: false, portName: '', routeId: null });
    const inactive = choice({ key: 'inactive', nodeId: null, nodeSerial: '', name: '' });
    const secondInactive = { ...inactive, key: 'second-inactive', routeId: 8 };
    const ghost = choice({ key: 'ghost', nodeSerial: '9999' });
    const other = endpoint({ key: 'output:99', id: 99, serial: '1009', name: 'alsa_output.usb', description: 'USB', isDefault: true });
    const result = deviceChoices([inactive, whole, ghost, secondInactive, active], [other, endpoint()], [{ id: 10, serial: '1000' }]);
    assert.deepEqual(result.map(row => row.key), ['inactive', 'whole', 'second-inactive', 'active', 'native:output:99']);
    assert.deepEqual(result.map(row => row.default), [false, false, false, false, true]);
});

test('deviceChoices native fallback remains selectable when helper metadata is unavailable', () => {
    const nodes = [
        endpoint({ isDefault: true, routesReliable: false }),
        endpoint({ key: 'input:9', id: 9, serial: '1003', name: 'alsa_input.usb', description: '',
            cardId: null, cardSerial: '', routes: [] }),
    ];
    const choices = deviceChoices([], nodes);
    assert.deepEqual(choices, [
        {
            key: 'native:output:42', label: 'Built-in Audio', description: 'Built-in Audio',
            name: 'alsa_output.pci', portName: '', card: '', port: false, portActive: false,
            nodeId: 42, nodeSerial: '1001', cardId: 10, cardSerial: '1000', routeId: null, default: true,
        },
        {
            key: 'native:input:9', label: 'alsa_input.usb', description: 'alsa_input.usb',
            name: 'alsa_input.usb', portName: '', card: '', port: false, portActive: false,
            nodeId: 9, nodeSerial: '1003', cardId: null, cardSerial: '', routeId: null, default: false,
        },
    ]);
    choices.forEach((row, index) => assert.deepEqual(selectionPlan(row, nodes), {
        type: 'native', nodeKey: nodes[index].key, route: null,
    }));
    assert.deepEqual(deviceChoices([], []), []);
});

test('deviceChoices never lets helper defaults or portActive choose the native default endpoint', () => {
    const row = choice({ default: true, isDefault: true, portActive: true });
    for (const routesReliable of [true, false]) {
        const nodes = [endpoint({ routesReliable }), endpoint({ key: 'output:99', id: 99, serial: '1009', isDefault: true })];
        const result = deviceChoices([row], nodes);
        assert.deepEqual(result.map(row => row.default), [false, true]);
        const parsed = parseChoices(JSON.stringify([row]));
        assert.deepEqual(deviceChoices(parsed, nodes).map(row => row.default), [false, true]);
    }
});

test('deviceChoices ignores ambiguous card-wide route getters and uses only each endpoint portActive', () => {
    const routes = [
        { name: 'analog-output-speaker', index: 0, available: true },
        { name: 'analog-output-headphones', index: 7, available: true },
    ];
    const rows = [
        choice({ key: 'first-speaker', portName: routes[0].name, routeId: 0, portActive: false }),
        choice({ key: 'first-headphones', portActive: true }),
        choice({ key: 'second-speaker', nodeId: 43, nodeSerial: '1002', portName: routes[0].name, routeId: 0, portActive: true }),
        choice({ key: 'second-headphones', nodeId: 43, nodeSerial: '1002', portActive: false }),
    ];
    const nodes = [endpoint({ routes, routesReliable: false, routeName: routes[0].name, isDefault: true }),
        endpoint({ key: 'output:43', id: 43, serial: '1002', routes, routesReliable: false, routeName: routes[0].name })];
    assert.deepEqual(deviceChoices(rows, nodes).map(row => row.default), [false, true, false, false]);
    const switched = nodes.map((node, index) => ({ ...node, isDefault: index === 1, routeName: routes[1].name }));
    assert.deepEqual(deviceChoices(rows, switched).map(row => row.default), [false, false, true, false]);
    assert.ok(deviceChoices(rows.map(row => ({ ...row, portActive: false })), nodes).every(row => !row.default));
});

test('bluetoothDevices orders connected, then paired, then discovered devices by name without mutation', () => {
    const devices = Object.freeze([
        { address: '1', name: 'Alpha', connected: false, paired: false },
        { address: '2', name: 'Zulu', connected: false, paired: true },
        { address: '3', name: 'Zulu', connected: true, paired: false },
        { address: '4', name: 'Alpha', connected: true, paired: true },
        { address: '5', name: 'Alpha', connected: false, paired: true },
        { address: '6', name: 'Zulu', connected: false, paired: false },
        { address: '7', name: 'Alpha', connected: true, paired: false },
    ].map(device => Object.freeze(device)));
    const before = structuredClone(devices);
    const sorted = bluetoothDevices(devices);
    assert.deepEqual(sorted.map(device => device.address), ['4', '7', '3', '5', '2', '1', '6']);
    assert.notEqual(sorted, devices);
    assert.deepEqual(devices, before);
    for (const device of sorted) assert.equal(device, devices.find(original => original.address === device.address));
    const empty = [];
    assert.deepEqual(bluetoothDevices(empty), []);
    assert.notEqual(bluetoothDevices(empty), empty);
    assert.deepEqual(bluetoothDevices([devices[0]]), [devices[0]]);
});

test('pairingAnswer preserves leading zeros and surrounding or embedded spaces, adding exactly one newline', () => {
    for (const answer of [
        '0', '000000', '001234', 'yes', 'no', 'aB09', ' 000123 ', ' PIN with spaces ', '\t000123\t',
        'x'.repeat(512), `${' '.repeat(511)}0`, `0${' '.repeat(511)}`,
    ]) assert.equal(pairingAnswer(answer), `${answer}\n`);
});

test('pairingAnswer rejects empty, multiline, NUL-containing, excessive, and non-string answers', () => {
    for (const answer of [
        '', ' ', '\t', '\u2003', '\r', '\n', 'yes\n', '\nyes', 'y\res', 'yes\r\nno',
        '\0yes', 'ye\0s', 'yes\0', 'x'.repeat(513), `0${' '.repeat(512)}`,
        undefined, null, true, false, 0, 123456, [], {}, new String('000123'),
    ]) assert.throws(() => pairingAnswer(answer), /Enter one PIN\/passkey or yes\/no answer/);
});

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
const frontend = readFileSync(new URL('../frontend.js', import.meta.url), 'utf8');
const accessibleDouble = () => ({
    accessibleProperties: new Map(),
    accessibleStates: new Map(),
    update_state(states, values) {
        assert.equal(states.length, values.length, 'accessible states and values are paired');
        states.forEach((state, index) => {
            assert.ok(Object.values(mockGtk.AccessibleState).includes(state), 'known native accessible state');
            assert.equal(typeof values[index], 'boolean');
            this.accessibleStates.set(state, values[index]);
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
    RevealerTransitionType: { SLIDE_DOWN: 1 },
    Orientation: { VERTICAL: 1, HORIZONTAL: 0 }, Align: { END: 2, FILL: 0, START: 1, CENTER: 3 },
    PositionType: { TOP: 2 }, ScrollType: { STEP_BACKWARD: 6, STEP_FORWARD: 7 },
    PolicyType: { NEVER: 0, AUTOMATIC: 1 }, PropagationPhase: { CAPTURE: 1 },
    EventControllerScrollFlags: { BOTH_AXES: 3 }, StackTransitionType: { NONE: 0 },
    Box: function(properties) {
        return Object.assign(boxDouble(), { sensitive: true,
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
    Separator: function(properties) { return { ...properties }; },
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
    AudioPanel.prototype.buildBluetooth.call(panel);
    return { panel, render(devices, model = {}) {
        AudioPanel.prototype.renderBluetooth.call(panel, { available: true, powered: true, devices, ...model });
    }, order() {
        const keys = new Map([...panel.deviceRows].map(([key, row]) => [row.widget, key]));
        return panel.bluetoothRows.children.filter(child => child !== panel.bluetoothSeparator || child.visible)
            .map(child => child === panel.bluetoothSeparator ? 'divider' : keys.get(child));
    } };
}

test('audio creates tooltips only for preserved non-button metadata, never volume ancestors', () => {
    assert.deepEqual(Array.from(frontend.matchAll(/^\s*([\w.]+)\.tooltip_text\s*=/gm), match => match[1]),
        ['view.heading', 'row.name']);
    assert.doesNotMatch(frontend, /tooltip_text\s*:|tooltip_markup|has_tooltip|set_tooltip|Gtk\.Settings/);
});

test('constructed panel buttons and volume controls stay tooltip-free across enabled and disabled rerenders', () => {
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
    panel.build({ cyan: '#00ffff' });
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
    assert.equal(checkButtons(panel.content), 13, 'tabs, mute, steppers, device toggles and pairing buttons are actually constructed');
    for (const patch of [{}, { muted: true }, { available: false },
        { available: true, nodeKey: '' }, { nodeKey: 'replacement', muted: false }, { busy: true }]) {
        for (const kind of ['output', 'input']) state[kind] = { ...state[kind], ...patch };
        panel.render();
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
            assert.equal(view.heading.tooltip_text, `${kind === 'output' ? 'Output' : 'Input'} · ${model.available ? model.name : 'No device'}`);
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
        [{ connected: true }, { pairing: true, pairingMessage: { text: 'Pairing…', prompt: 'Enter PIN:' } }],
        [{ busy: true }, {}],
        [{ name: 'Renamed headset' }, { powered: false }],
        [{ name: '', paired: false }, {}],
        [{}, {}],
    ]) {
        const current = { ...device, ...properties };
        h.render([current], model);
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
        assert.equal(row.name.tooltip_text, current.name || current.address, 'non-button device-name metadata is preserved');
    }
});

test('Bluetooth renderer groups remembered devices before available devices with a divider and paired-only forget', () => {
    const h = bluetoothViewHarness();
    const devices = Object.freeze([
        { key: 'available-z' }, { key: 'paired-z', paired: true },
        { key: 'connected', connected: true }, { key: 'available-a' }, { key: 'paired-a', paired: true },
    ].map(Object.freeze));
    h.render(devices);
    assert.deepEqual(h.order(), ['paired-z', 'connected', 'paired-a', 'divider', 'available-z', 'available-a']);
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
    h.render([], { pairing: true, pairingMessage: { state: 'pairing', text: 'Pairing with Headset (AA:01)…', prompt: 'Enter PIN:' } });
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
        h.render([], { pairing: true, pairingMessage: { ...notice, prompt } });
        assert.equal(h.panel.answer.visible, true);
        assert.equal(h.panel.send.visible, true);
        assert.equal(h.panel.pairingControls.visible, true);
        h.panel.answer.text = '000007';
        h.render([], { pairing: true, pairingMessage: { ...notice, prompt } });
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
        h.render([], { pairing: true, pairingMessage: { state: 'pairing', text: 'Pairing with Keyboard…', prompt: 'Enter PIN:' } });
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
    h.render([], { pairing: true, pairingMessage: { state: 'pairing', text: 'Pairing…', prompt: 'Enter PIN:' } });
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
            assert.equal(h.view.deviceSelection.selected, 0, 'native default remains authoritative until backend confirmation');
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
        assert.equal(h.view.deviceSelection.selected, 0);
        assert.equal(h.view.devices.accessibleProperties.get(mockGtk.AccessibleProperty.DESCRIPTION), choices[0].description);
        h.select(0);
        assert.deepEqual(h.selections, [], 'selecting the native default is not a mutation');
        h.select(1);
        assert.deepEqual(h.selections, [[kind, choices[1].key]]);
        assert.equal(h.state[kind].choices, choices, 'GTK selection must not rewrite native state');
        assert.equal(h.view.deviceSelection.selected, 0, 'the renderer still follows the native default');
    });

    test(`${kind} dropdown follows native default changes and reordered keys without native writes`, () => {
        const h = viewHarness(kind), notifications = [], activations = [];
        h.view.deviceSelection.connect('notify::selected', () => notifications.push(h.view.deviceSelection.selected));
        h.view.deviceList.connect('activate', (_, position) => activations.push(position));
        const entries = [choices[1], choices[0]];
        h.render({ choices: entries });
        assert.equal(h.view.deviceSelection.selected, 1, 'model replacement must not select the first non-default item');
        const model = h.view.deviceSelection.model;
        const switched = entries.map(choice => ({ ...choice, default: !choice.default }));
        h.render({ choices: switched });
        assert.equal(h.view.deviceSelection.model, model, 'changing only the native default keeps the string model');
        assert.equal(h.view.deviceSelection.selected, 0);
        assert.equal(h.view.devices.accessibleProperties.get(mockGtk.AccessibleProperty.DESCRIPTION), choices[1].description);
        h.render({ choices: [switched[1], switched[0]] });
        assert.notEqual(h.view.deviceSelection.model, model, 'key order matters even when both labels are identical');
        assert.deepEqual(Array.from(h.view.choiceItems, item => item.key), [choices[0].key, choices[1].key]);
        assert.equal(h.view.deviceSelection.selected, 1);
        assert.ok(h.view.deviceSelection.modelWrites.length >= 2);
        assert.ok(h.view.deviceSelection.selectionWrites.length >= 2);
        assert.ok(notifications.length >= 4, 'model replacement and selected assignments emit notifications');
        assert.deepEqual(activations, [], 'native model/selection notifications never activate a row');
        assert.equal(Object.hasOwn(h.view, 'updatingChoices'), false);
        assert.deepEqual(h.selections, []);
        h.select(0);
        assert.deepEqual(h.selections, [[kind, choices[0].key]], 'the reordered index resolves to its stored key');
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
        assert.equal(h.view.deviceSelection.selected, 0);
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
            assert.equal(h.view.deviceSelection.selected, 0);
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

test('built audio launcher', {
    skip: launcherPath ? false : 'Set AUDIO_PANEL_LAUNCHER to the built audio-panel executable.',
}, async t => {
    const source = readFileSync(launcherPath, 'utf8');
    const calls = [...source.matchAll(/(?:^|\s)(\/[^\s"']+\/bin\/ags) (bundle|request|run)\b/g)];
    const runtimes = new Set(calls.map(match => match[1]));
    assert.equal(runtimes.size, 1, 'launcher must use one explicit AGS runtime');
    assert.deepEqual(calls.map(match => match[2]).sort(), ['bundle', 'request']);
    const [ags] = runtimes;
    const gjs = ags.replace(/\/ags$/, '/panel-gjs');
    assert.ok(source.includes(`exec ${gjs} -m "$bundle"`), 'run must exec the companion GJS wrapper');

    const root = mkdtempSync(join(tmpdir(), 'audio-launcher-'));
    t.after(() => rmSync(root, { recursive: true, force: true }));
    const keys = ['AUDIO_PANEL_CONFIG', 'AUDIO_PANEL_PALETTE', 'AUDIO_CONTROL_HELPER', 'XDG_RUNTIME_DIR'];
    const fakes = new Map();
    for (const [program, path] of [['ags', ags], ['panel-gjs', gjs]]) {
        const fake = join(root, program);
        writeFileSync(fake, `#!${process.execPath}\n`
            + 'const fs = require("node:fs");\n'
            + `const program = ${JSON.stringify(program)}, keys = ${JSON.stringify(keys)};\n`
            + 'const argv = process.argv.slice(2);\n'
            + 'const data = {program, argv, env: Object.fromEntries(keys.map(key => [key, process.env[key]]))};\n'
            + 'if (program === "ags" && argv[0] === "bundle") {\n'
            + '  console.log(JSON.stringify(data));\n'
            + '  if (process.env.PANEL_TEST_BUNDLE_STATUS) {\n'
            + '    console.error("fixture compiler failed");\n'
            + '    process.exit(Number(process.env.PANEL_TEST_BUNDLE_STATUS));\n'
            + '  }\n'
            + '  fs.writeFileSync(argv.at(-1), JSON.stringify({config: argv.at(-2)}));\n'
            + '} else {\n'
            + '  if (program === "panel-gjs") data.bundle = JSON.parse(fs.readFileSync(argv.at(-1), "utf8"));\n'
            + '  console.log(JSON.stringify(data));\n'
            + '}\n');
        chmodSync(fake, 0o700);
        fakes.set(path, fake);
    }
    const launcher = join(root, 'audio-panel');
    writeFileSync(launcher, source.replaceAll(gjs, () => fakes.get(gjs)).replaceAll(ags, () => fakes.get(ags)));
    chmodSync(launcher, 0o700);
    const env = { ...process.env, XDG_CONFIG_HOME: join(root, 'config with spaces'),
        XDG_RUNTIME_DIR: join(root, 'runtime with spaces') };
    mkdirSync(env.XDG_RUNTIME_DIR, { mode: 0o700 });
    const sharedBundle = join(env.XDG_RUNTIME_DIR, 'ags.js');
    writeFileSync(sharedBundle, 'another panel owns this shared-path sentinel');
    for (const key of [...keys.filter(key => key !== 'XDG_RUNTIME_DIR'), 'PANEL_TEST_BUNDLE_STATUS']) delete env[key];

    function run(args, overrides = {}) {
        const result = spawnSync(launcher, args, { env: { ...env, ...overrides }, encoding: 'utf8',
            timeout: 5000, maxBuffer: 1024 * 1024 });
        assert.ifError(result.error);
        assert.equal(result.signal, null);
        return result;
    }

    function success(args, overrides) {
        const result = run(args, overrides);
        assert.equal(result.status, 0, result.stderr);
        return result.stdout.trim().split('\n').map(line => JSON.parse(line));
    }

    function request(args) {
        const records = success(args);
        assert.equal(records.length, 1, 'requests must not compile or start GJS');
        const [data] = records;
        assert.equal(data.program, 'ags');
        assert.equal(data.env.XDG_RUNTIME_DIR, env.XDG_RUNTIME_DIR, 'IPC uses the original runtime directory');
        return data;
    }

    function launch(overrides = {}) {
        const records = success(['run'], overrides);
        assert.equal(records.length, 2, 'compile once, then start GJS once');
        const [compiler, runtime] = records;
        assert.equal(compiler.program, 'ags');
        assert.equal(runtime.program, 'panel-gjs');
        const runtimeDir = overrides.XDG_RUNTIME_DIR ?? env.XDG_RUNTIME_DIR;
        const config = overrides.AUDIO_PANEL_CONFIG ?? `${env.XDG_CONFIG_HOME}/ags/audio/frontend.js`;
        const bundle = join(runtimeDir, 'ags-panels', 'audio-panel.js');
        assert.deepEqual(compiler.argv, ['bundle', '--gtk', '4', '--', config, bundle]);
        assert.deepEqual(runtime.argv, ['-m', bundle]);
        assert.deepEqual(runtime.bundle, { config }, 'GJS reads the output of this compilation');
        assert.equal(statSync(join(runtimeDir, 'ags-panels')).mode & 0o777, 0o700);
        for (const data of records) {
            assert.equal(data.env.XDG_RUNTIME_DIR, runtimeDir, 'bundling must not change the IPC namespace');
            assert.equal(data.env.AUDIO_PANEL_CONFIG, config);
        }
        assert.equal(readFileSync(sharedBundle, 'utf8'), 'another panel owns this shared-path sentinel');
        return { compiler, runtime };
    }

    await t.test('validates command arity and help without invoking AGS or GJS', () => {
        for (const args of [[], ['unknown'], ['run', 'extra'], ['status', 'extra'], ['close', 'extra'],
            ['bluetoothPower', 'extra'], ['toggle', 'DP-1', 'extra'], ['--help', 'extra'], ['-h', 'extra']]) {
            const result = run(args);
            assert.equal(result.status, 2, JSON.stringify(args));
            assert.match(result.stderr, /Usage:/);
            assert.equal(result.stdout, '');
        }
        for (const command of ['--help', '-h']) {
            const result = run([command]);
            assert.equal(result.status, 0);
            assert.match(result.stdout, /Usage:/);
        }
    });

    await t.test('targets the audio instance with typed JSON requests', () => {
        for (const command of ['status', 'close', 'bluetoothPower', 'toggle']) {
            const data = request([command]);
            assert.deepEqual(data.argv.slice(0, 4), ['request', '--instance', 'audio-panel', '--']);
            assert.equal(data.argv.length, 5);
            assert.deepEqual(JSON.parse(data.argv[4]), { command });
        }
    });

    await t.test('escapes monitor arguments without interpreting shell syntax', () => {
        const marker = join(root, 'monitor-was-evaluated');
        for (const monitor of ['', 'eDP-1', 'Display with spaces', `DP-'"; touch ${marker}; $(touch ${marker}); 🎧`]) {
            const data = request(['toggle', monitor]);
            assert.deepEqual(JSON.parse(data.argv[4]), { command: 'toggle', ...(monitor ? { monitor } : {}) });
        }
        assert.equal(existsSync(marker), false);
        assert.equal(existsSync(join(env.XDG_RUNTIME_DIR, 'ags-panels')), false, 'requests do not create bundles');
    });

    await t.test('bundles GTK4 to the audio-specific file before executing GJS with store-backed defaults', () => {
        const { compiler, runtime } = launch();
        for (const data of [compiler, runtime]) {
            assert.match(data.env.AUDIO_PANEL_PALETTE, /^\/nix\/store\//);
            assert.match(data.env.AUDIO_CONTROL_HELPER, /\/bin\/audio-control$/);
        }
    });

    await t.test('preserves explicit overrides and passes paths as native arguments without shell evaluation', () => {
        const marker = join(root, 'config-was-evaluated');
        const overrides = { AUDIO_PANEL_CONFIG: join(root, `custom '"; $(touch ${marker}); frontend.js`),
            AUDIO_PANEL_PALETTE: join(root, 'palette with spaces.json'), AUDIO_CONTROL_HELPER: join(root, 'custom helper'),
            XDG_RUNTIME_DIR: join(root, 'isolated runtime $HOME with spaces') };
        mkdirSync(overrides.XDG_RUNTIME_DIR, { mode: 0o700 });
        const { compiler, runtime } = launch(overrides);
        for (const data of [compiler, runtime]) assert.deepEqual(data.env, overrides);
        assert.equal(existsSync(marker), false);
        assert.equal(existsSync(join(overrides.XDG_RUNTIME_DIR, 'ags.js')), false);
    });

    await t.test('compiler failure cannot execute GJS, even when a stale audio bundle exists', () => {
        const runtimeDir = join(root, 'failed compilation runtime');
        const bundleDir = join(runtimeDir, 'ags-panels');
        mkdirSync(bundleDir, { recursive: true, mode: 0o700 });
        const bundle = join(bundleDir, 'audio-panel.js');
        writeFileSync(bundle, JSON.stringify({ config: 'stale frontend.js' }));
        const result = run(['run'], { XDG_RUNTIME_DIR: runtimeDir, PANEL_TEST_BUNDLE_STATUS: '41' });
        assert.equal(result.status, 41, result.stderr);
        assert.match(result.stderr, /fixture compiler failed/);
        const records = result.stdout.trim().split('\n').map(line => JSON.parse(line));
        assert.equal(records.length, 1, 'a stale bundle is never executed after compilation fails');
        assert.equal(records[0].program, 'ags');
        assert.equal(records[0].argv[0], 'bundle');
        assert.equal(records[0].env.XDG_RUNTIME_DIR, runtimeDir);
    });
});
}
