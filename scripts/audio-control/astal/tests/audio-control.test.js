import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync, chmodSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
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

test('retired QML and Rust status sources and slider aliases remain absent', () => {
    for (const path of ['niri/audio-control', 'scripts/audio-control/rust-custom/src/waybar.rs', 'themes/quickshell/StyledSlider.qml'])
        assert.equal(existsSync(new URL(path, root)), false, path);
    assert.doesNotMatch(read('niri/quickshell/style.nix'), /StyledSlider|"audio-control"/);
    assert.doesNotMatch(read('niri/default.nix'), /\.\/audio-control/);
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
    radio = adapter(), devices = [], cards = [],
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
    const audio = new Signals({ devices: cards, speakers: outputs, microphones: inputs,
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
    return { backend, audio, wp, bluetooth, radio, outputs, inputs, cards, helpers, timers, listing, policy, bindNode,
        snapshot: () => plain(backend.snapshot()), changes: () => changes,
        load(kind, entries) { timers.advance(); listing(kind).rows(entries); } };
}

function transactionFixture(t) {
    const selectedCard = card();
    const previousCard = card({ id: 8, serial: '80', pwName: 'alsa_card.previous' });
    const previous = endpoint({ card: previousCard });
    const input = endpoint({ id: 20, card: selectedCard, pwName: 'alsa_input.test' });
    const f = fixture(t, { outputs: [previous], inputs: [input], cards: [selectedCard, previousCard] });
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
        bluetooth: { available: false, powered: false, discovering: false, devices: [],
            pairing: false, pairingLog: '', error: '' } });
    assert.equal(JSON.parse(f.backend.status()).class, 'unavailable');
    assert.equal(f.helpers.length, 0);
    assert.equal(f.timers.sources.size, 0);
    assert.equal(f.audio.connections, 0);
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
    const headset = device({ adapter: '/org/bluez/hci12', alias: 'Desk headset' });
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
    assert.ok(f.snapshot().bluetooth.pairingLog.endsWith(`Desk headset (${headset.address}) is paired and connected.\n`));
    assert.equal(headset.cancellations, 0);
});

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
        assert.equal(f.snapshot().bluetooth.error, error.message);
        assert.equal(f.snapshot().bluetooth.pairingLog.includes('paired and connected'), false);
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
        const log = f.backend.pairingLog;
        old.operations[0].callback(old, failure ? { error: new Error('Old connect failed') } : {});
        assert.equal(old.finishes.length, 1, 'native async results are always consumed');
        assert.equal(f.backend.pairing, session);
        assert.equal(f.backend.pairingLog, log);
        assert.equal(f.backend.bluetoothError, '');
        assert.equal(f.snapshot().bluetooth.devices.find(row => row.address === old.address).busy, false);
        f.backend.sendPairingAnswer('004200');
        assert.deepEqual(newer.answers, ['004200\n']);
        worker.options.onChunk('Old agent prompt');
        worker.options.onExit(new Error('Old helper error'));
        assert.equal(f.backend.pairing, session);
        assert.equal(f.backend.pairingLog, log);
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
        assert.equal(f.backend.pairingLog.includes('paired and connected'), false);
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
        f.backend.toggleDiscovery();
        assert.deepEqual(f.radio.scanCalls, ['start']);
        f.backend.deviceAction(f.snapshot().bluetooth.devices[0].key);
        const pairing = f.helpers.at(-1);
        assert.deepEqual(pairing.argv, ['/test/audio-control', 'bluetooth-pair', 'hci0', headset.address]);
        assert.equal(pairing.options.interactive, true);
        assert.equal(pairing.options.timeout, 180000);
        pairing.options.onChunk('x'.repeat(33000));
        pairing.options.onChunk('PIN?');
        assert.equal(f.snapshot().bluetooth.pairingLog.length, 32768);
        assert.ok(f.snapshot().bluetooth.pairingLog.endsWith('PIN?'));
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
    f.backend.toggleDiscovery();
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
        f.backend.toggleDiscovery();
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

test('Bluetooth power-off cancels owned work, and scan toggles require an open powered Bluetooth tab', t => {
    const headset = device();
    const f = fixture(t, { devices: [headset] });
    f.backend.start();
    f.backend.toggleDiscovery();
    f.backend.setActive(true, 1);
    f.backend.toggleDiscovery();
    assert.deepEqual(f.radio.scanCalls, []);
    f.backend.setActive(true, 0);
    f.backend.toggleDiscovery();
    f.backend.toggleDiscovery();
    f.backend.toggleDiscovery();
    f.backend.deviceAction(f.snapshot().bluetooth.devices[0].key);
    f.backend.toggleBluetoothPower();
    assert.equal(f.radio.powered, false);
    assert.deepEqual(f.radio.scanCalls, ['start', 'stop', 'start', 'stop']);
    assert.equal(headset.cancellations, 1);
    f.backend.toggleDiscovery();
    assert.equal(f.radio.scanCalls.length, 4);
    f.backend.toggleBluetoothPower();
    assert.equal(f.radio.powered, true);
    assert.equal(f.radio.scanCalls.length, 4, 'powering on does not start discovery implicitly');
});

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
    backend.toggleDiscovery();
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
    f.backend.toggleDiscovery();
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
    assert.equal(f.backend.pairingLog, '');
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

// Tests consolidated from launcher.test.js.
{
const launcherPath = process.env.AUDIO_PANEL_LAUNCHER;

test('built audio launcher', {
    skip: launcherPath ? false : 'Set AUDIO_PANEL_LAUNCHER to the built audio-panel executable.',
}, async t => {
    const source = readFileSync(launcherPath, 'utf8');
    const calls = [...source.matchAll(/\bexec ([^\s]+\/bin\/ags) (run|request)\b/g)];
    const runtimes = new Set(calls.map(match => match[1]));
    assert.equal(runtimes.size, 1, 'launcher must use one explicit AGS runtime');
    assert.deepEqual(calls.map(match => match[2]).sort(), ['request', 'run']);

    const root = mkdtempSync(join(tmpdir(), 'audio-launcher-'));
    t.after(() => rmSync(root, { recursive: true, force: true }));
    const fake = join(root, 'ags');
    writeFileSync(fake, `#!${process.execPath}\n`
        + 'const keys = ["AUDIO_PANEL_CONFIG", "AUDIO_PANEL_PALETTE", "AUDIO_CONTROL_HELPER"];\n'
        + 'console.log(JSON.stringify({argv: process.argv.slice(2), '
        + 'env: Object.fromEntries(keys.map(key => [key, process.env[key]]))}));\n');
    chmodSync(fake, 0o700);
    const launcher = join(root, 'audio-panel');
    writeFileSync(launcher, source.replaceAll([...runtimes][0], () => fake));
    chmodSync(launcher, 0o700);
    const env = { ...process.env, XDG_CONFIG_HOME: join(root, 'config with spaces') };
    for (const key of ['AUDIO_PANEL_CONFIG', 'AUDIO_PANEL_PALETTE', 'AUDIO_CONTROL_HELPER']) delete env[key];

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
        return JSON.parse(result.stdout);
    }

    await t.test('validates command arity and help without invoking AGS', () => {
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
            const data = success([command]);
            assert.deepEqual(data.argv.slice(0, 4), ['request', '--instance', 'audio-panel', '--']);
            assert.equal(data.argv.length, 5);
            assert.deepEqual(JSON.parse(data.argv[4]), { command });
        }
    });

    await t.test('escapes monitor arguments without interpreting shell syntax', () => {
        for (const monitor of ['', 'eDP-1', 'Display with spaces', 'DP-\'"; touch /not-executed; 🎧']) {
            const data = success(['toggle', monitor]);
            assert.deepEqual(JSON.parse(data.argv[4]), { command: 'toggle', ...(monitor ? { monitor } : {}) });
        }
    });

    await t.test('supplies the GTK4 entry point and store-backed defaults', () => {
        const data = success(['run']);
        assert.deepEqual(data.argv, ['run', '--gtk4', '--', `${env.XDG_CONFIG_HOME}/ags/audio/frontend.js`]);
        assert.equal(data.env.AUDIO_PANEL_CONFIG, data.argv.at(-1));
        assert.match(data.env.AUDIO_PANEL_PALETTE, /^\/nix\/store\//);
        assert.match(data.env.AUDIO_CONTROL_HELPER, /\/bin\/audio-control$/);
    });

    await t.test('preserves explicit runtime overrides including paths with spaces', () => {
        const overrides = { AUDIO_PANEL_CONFIG: join(root, 'custom frontend.js'),
            AUDIO_PANEL_PALETTE: join(root, 'palette.json'), AUDIO_CONTROL_HELPER: join(root, 'custom-helper') };
        const data = success(['run'], overrides);
        assert.deepEqual(data.env, overrides);
        assert.equal(data.argv.at(-1), overrides.AUDIO_PANEL_CONFIG);
    });
});
}
