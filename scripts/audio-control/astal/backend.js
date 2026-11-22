import GLib from 'gi://GLib';
import Gio from 'gi://Gio';
import GObject from 'gi://GObject';
import Wp from 'gi://AstalWp';
import Bluetooth from 'gi://AstalBluetooth';
import { nativeEvent, rawEqual, waybarStatus } from './logic.js';

const PRIORITY = GLib.PRIORITY_DEFAULT;

function closeStream(stream) {
    if (!stream || stream.is_closed() || stream.has_pending()) return;
    stream.close_async(PRIORITY, null, (source, result) => {
        try { source.close_finish(result); } catch (_) { /* Cancelled pipes may already be closed. */ }
    });
}

// Transport limits apply per frame for the resident connection, not per lifetime.
// Waiting is never cancelled: every child is still reaped.
export class HelperProcess {
    constructor(argv, { onChunk = () => {}, onExit = () => {} } = {}) {
        Object.assign(this, { onChunk, onExit });
        this.io = new Gio.Cancellable();
        this.output = '';

        this.queue = [];
        this.done = this.exited = this.eof = this.writing = false;
        this.decoder = new TextDecoder('utf-8', { fatal: true });
        this.pendingBytes = new Uint8Array(0);
        try {
            this.child = Gio.Subprocess.new(argv, Gio.SubprocessFlags.STDOUT_PIPE | Gio.SubprocessFlags.STDERR_MERGE | Gio.SubprocessFlags.STDIN_PIPE);
            this.stdout = this.child.get_stdout_pipe();
            this.stdin = this.child.get_stdin_pipe();
            this.child.wait_async(null, (child, result) => {
                try { child.wait_finish(result); this.exited = true; this.complete(); }
                catch (error) { this.finish(error); }
            });

            this.read();
        } catch (error) {
            GLib.idle_add(PRIORITY, () => { this.finish(error); return GLib.SOURCE_REMOVE; });
        }
    }

    read() {
        this.stdout.read_bytes_async(8192, PRIORITY, this.io, (stream, result) => {
            try {
                const chunk = stream.read_bytes_finish(result);
                if (this.done) return;
                const size = chunk.get_size();

                const data = new Uint8Array(this.pendingBytes.length + size);
                data.set(this.pendingBytes);
                data.set(chunk.toArray(), this.pendingBytes.length);
                let frameStart = 0;
                for (let index = 0; index < data.length; index++) if (data[index] === 10) {
                    if (index + 1 - frameStart > 1024 * 1024) throw new Error('Audio helper frame is too large.');
                    frameStart = index + 1;
                }
                if (data.length - frameStart > 1024 * 1024) throw new Error('Audio helper frame is too large.');
                const end = size ? data.lastIndexOf(10) + 1 : data.length;
                const text = this.decoder.decode(data.subarray(0, end));
                this.pendingBytes = data.slice(end);
                this.output = (this.output + text).slice(-8192);
                if (text) this.onChunk(text);
                if (size) this.read();
                else { this.eof = true; closeStream(stream); this.complete(); }
            } catch (error) { this.finish(error); }
            finally { if (this.done) closeStream(stream); }
        });
    }

    complete() {
        if (!this.done && this.exited && this.eof)
            this.finish(this.child.get_successful() ? null : new Error(this.output.trim() || 'Audio helper failed.'));
    }

    send(text) {
        if (this.done || !this.stdin) throw new Error('The audio policy connection is closed.');
        const bytes = new TextEncoder().encode(text);
        if (bytes.length > 1024 * 1024 || this.queue.reduce((sum, item) => sum + item.length, 0) + bytes.length > 8 * 1024 * 1024)
            throw new Error('Audio policy transport queue is too large.');
        this.queue.push(bytes);
        this.write();
    }

    write() {
        if (this.done || this.writing || !this.queue.length) return;
        this.writing = true;
        // GUnixOutputStream may write inline; stay within PIPE_BUF.
        this.stdin.write_bytes_async(new GLib.Bytes(this.queue[0].subarray(0, 4096)), PRIORITY, this.io, (stream, result) => {
            try {
                const written = stream.write_bytes_finish(result);
                if (this.done) return;
                if (written <= 0) throw new Error('The audio helper stopped accepting input.');
                if (written === this.queue[0].length) this.queue.shift();
                else this.queue[0] = this.queue[0].slice(written);
                this.writing = false;
                this.write();
            } catch (error) { this.finish(error); }
            finally { if (this.done) closeStream(stream); }
        });
    }

    finish(error) {
        if (this.done) return;
        this.done = true;
        this.io.cancel();
        if (error && !this.exited) this.child?.force_exit();
        this.queue = [];
        closeStream(this.stdout);
        closeStream(this.stdin);
        this.onExit(error, this.output);
    }

    stop() { this.finish(new Error('Audio operation cancelled.')); }
}

const emptyAudio = () => ({ nodeKey: '', available: false, name: 'No device', statusName: '',
    volume: 0, muted: false, choices: [], busy: false, loading: false, error: '' });

export class AudioBackend {
    constructor({ onChange = () => {}, wp = Wp.get_default(), bluetooth = Bluetooth.get_default() } = {}) {
        Object.assign(this, { onChange, wp, audio: wp.audio, bluetooth });
        this.helper = GLib.getenv('AUDIO_CONTROL_HELPER') || 'audio-control';
        this.model = { output: emptyAudio(), input: emptyAudio(), bluetooth: {
            available: false, powered: false, devices: [], pairing: false, pairingMessage: null, error: '' } };
        this.records = new Map();
        this.identities = new WeakMap();
        this.signals = new Map();
        this.serial = this.rpc = this.idle = this.shutdownTimer = this.pairingEpoch = 0;
        this.defaults = { output: { key: '', epoch: '' }, input: { key: '', epoch: '' } };
        this.opened = this.started = this.closing = this.disposed = false;
        this.tab = 1;
    }

    path(adapter) {
        if (!adapter) return '';
        // Astal omits this property's convenience accessor from its GIR.
        const value = new GObject.Value();
        value.init(GObject.TYPE_STRING);
        adapter.get_property('object-path', value);
        return value.get_string() || '';
    }

    identity(object) { return { id: object.id, serial: object.get_pw_property('object.serial') || '' }; }

    bind(object, type, fields) {
        const fingerprint = JSON.stringify([type, fields.id, fields.serial, fields.address, fields.path]);
        let identity = this.identities.get(object);
        if (!identity || identity.fingerprint !== fingerprint || !this.records.has(identity.key)) {
            identity = { fingerprint, key: String(++this.serial) };
            this.identities.set(object, identity);
        }
        const previous = this.records.get(identity.key);
        const record = { object, type, key: identity.key, fields: { key: identity.key, type, ...fields }, holds: previous?.holds || 0, drops: previous?.drops || [], live: true };
        this.records.set(record.key, record);
        return record.fields;
    }

    capture() {
        for (const record of this.records.values()) record.live = false;
        const objects = [], watched = new Set([this.audio, this.wp, this.bluetooth]);
        const endpoints = [...new Set([...(this.audio.speakers || []), ...(this.audio.microphones || [])])];
        const cards = [...new Set([...(this.audio.devices || []), ...endpoints.map(node => node.get_device()).filter(Boolean)])];
        const valid = object => object.id > 0 && /^\d+$/.test(object.get_pw_property('object.serial') || '');
        for (const card of cards.filter(valid)) objects.push(this.bind(card, 'card', {
            ...this.identity(card), name: card.get_pw_property('device.name') || '', activeProfile: card.active_profile_id,
            profiles: (card.profiles || []).map(profile => ({ index: profile.index })),
        }));
        for (const kind of ['output', 'input']) {
            const nodes = (kind === 'output' ? this.audio.speakers : this.audio.microphones) || [];
            for (const node of nodes.filter(valid)) {
                const card = node.get_device();
                const binding = objects.find(item => item.type === 'card' && item.id === card?.id
                    && item.serial === card?.get_pw_property('object.serial'));
                const profile = node.get_pw_property('card.profile.device') || '';
                objects.push(this.bind(node, 'node', { ...this.identity(node), kind,
                    name: node.get_pw_property('node.name') || '', description: node.description || node.get_pw_property('node.name') || '',
                    nick: node.get_pw_property('node.nick') || '', cardId: binding?.id ?? null, cardSerial: binding?.serial || '',
                    profileDevice: /^\d+$/.test(profile) ? Number(profile) : null,
                    routeName: node.route?.name || '', routes: (node.routes || []).map(route => ({ index: route.index,
                        name: route.name || '', description: route.description || route.name || '', available: route.available !== Wp.Available.NO })),
                    volume: Number.isFinite(node.volume) ? node.volume : 0, muted: Boolean(node.mute), isDefault: Boolean(node.is_default),
                }));
            }
            const proxy = kind === 'output' ? this.audio.default_speaker : this.audio.default_microphone;
            if (proxy) watched.add(proxy);
            const current = nodes.find(node => node.id === proxy?.id);
            const key = current && objects.find(item => item.type === 'node' && item.kind === kind
                            && item.key === this.identities.get(current)?.key)?.key || '';
            if (this.defaults[kind].key !== key) this.defaults[kind] = { key, epoch: String(++this.serial) };
        }
        for (const stream of this.audio.streams || []) objects.push(this.bind(stream, 'stream', {
            ...this.identity(stream), name: stream.get_pw_property('application.name') || stream.description || 'Playback',
            targetSerial: stream.target_serial,
        }));
        this.adapter = this.bluetooth.adapter || null;
        let adapter = '';
        if (this.adapter) adapter = this.bind(this.adapter, 'adapter', { path: this.path(this.adapter), powered: Boolean(this.adapter.powered), discovering: Boolean(this.adapter.discovering) }).key;
        if (adapter) objects.push(this.records.get(adapter).fields);
        for (const device of this.bluetooth.devices || []) objects.push(this.bind(device, 'device', {
            address: device.address || '', adapter: device.adapter || '', name: device.name || '', alias: device.alias || '',
            connected: Boolean(device.connected), paired: Boolean(device.paired), trusted: Boolean(device.trusted),
            battery: Number.isFinite(device.battery_percentage) ? device.battery_percentage : null,
        }));
        for (const [key, record] of this.records) {
            if (!record.live && !record.holds) this.records.delete(key);
            else watched.add(record.object);
        }
        if (!this.disposed && !this.finishing) {
            for (const [object, ids] of this.signals) if (!watched.has(object)) {
                for (const id of ids) object.disconnect(id);
                this.signals.delete(object);
            }
            for (const object of watched) if (!this.signals.has(object)) {
                const ids = [object.connect('notify', () => this.notify())];
                if (object === this.wp) ids.push(object.connect('ready', () => this.notify()));
                this.signals.set(object, ids);
            }
        }
        return { objects, defaultOutput: this.defaults.output.key, defaultInput: this.defaults.input.key,
            outputEpoch: this.defaults.output.epoch, inputEpoch: this.defaults.input.epoch, pairingEpoch: this.pairingEpoch,
            adapter, opened: this.opened, tab: this.tab };
    }

    notify() {
        if (this.disposed) return;
        // Capture synchronously so A -> B -> A also changes the raw default epoch.
        this.capture();
        if (!this.idle) this.idle = GLib.idle_add(PRIORITY, () => {
            this.idle = 0;
            if (!this.disposed) this.send({ type: 'snapshot', snapshot: this.capture() });
            return GLib.SOURCE_REMOVE;
        });
    }

    send(value) {
        if (!this.worker || this.worker.done) return;
        try { this.worker.send(`${JSON.stringify(value)}\n`); }
        catch (error) { this.worker.finish(error); }
    }

    start() {
        if (this.started || this.disposed) return;
        this.started = true;
        this.worker = new HelperProcess([this.helper, 'panel'], {
            onChunk: chunk => {
                for (const line of chunk.split('\n').filter(line => line.trim())) {
                    const event = nativeEvent(line);
                    if (event.type === 'model') { this.model = event.model; if (!this.closing) this.onChange(); }
                    else if (event.type === 'closed') this.finish();
                    else {
                        if (event.id !== this.rpc + 1) throw new Error('Unexpected audio native RPC id.');
                        this.rpc = event.id;
                        Promise.resolve().then(() => event.type === 'snapshot' ? null : this.execute(event)).then(() => {
                            if (!this.disposed) this.send({ type: 'reply', id: event.id, ok: true, snapshot: this.capture() });
                        }, error => {
                            if (!this.disposed) this.send({ type: 'reply', id: event.id, ok: false, error: String(error.message || error).slice(0, 2048), snapshot: this.capture() });
                        }).catch(error => this.worker.finish(error));
                    }
                }
            },
            onExit: error => {
                if (!this.closing) {
                    const message = error?.message || 'The Rust audio policy connection closed.';
                    for (const kind of ['output', 'input']) Object.assign(this.model[kind], { busy: false, loading: false, error: message });
                    this.model.bluetooth.error = message;
                    this.onChange();
                }
                this.finish();
            },
        });
        this.notify();
    }

    async execute(action) {
        if (this.disposed || (this.closing && !action.cleanup)) throw new Error('Audio operation cancelled.');
        const snapshot = this.capture();
        for (const guard of action.guards) {
            const record = guard.key === null ? null : this.records.get(guard.key);
            let actual = guard.key === null ? snapshot : record && (record.live || guard.held) ? record.fields : null;
            if (record && guard.held && !record.live) {
                // Retained GObjects can outlive inventory membership. Read their
                // identity again rather than validating a cached cleanup target.
                actual = { ...record.fields };
                if (Object.hasOwn(actual, 'id')) Object.assign(actual, this.identity(record.object));
                if (Object.hasOwn(actual, 'address')) actual.address = record.object.address;
                if (Object.hasOwn(actual, 'adapter')) actual.adapter = record.object.adapter;
                if (Object.hasOwn(actual, 'path')) actual.path = this.path(record.object);
            }
            if (!actual || Object.entries(guard.expect).some(([key, value]) => !rawEqual(actual[key] ?? null, value)))
                throw new Error('Native object changed before the guarded operation.');
        }
        const record = this.records.get(action.target);
        if (action.operation === 'release') {
            if (record) { record.holds = Math.max(0, record.holds - 1); record.drops.pop(); }
            return;
        }
        if (!record || (!record.live && !action.cleanup)) throw new Error('The native object disappeared or was replaced.');
        const object = record.object, args = action.args;
        const asyncCall = method => new Promise((resolve, reject) => {
            object[method]((source, result) => {
                // Always consume the native result, including after cancellation/disposal.
                try { source[`${method}_finish`](result); resolve(); } catch (error) { reject(error); }
            });
        });
        switch (action.operation) {
        case 'retain':
            record.drops.push((args.onDrop || []).map(plan => nativeEvent(JSON.stringify(plan))));
            record.holds++;
            break;
        case 'profile': object.set_active_profile_id(args.profile); break;
        case 'route': object.set_route((object.routes || []).find(route => route.index === args.route)); break;
        case 'default': object.is_default = true; break;
        case 'volume': object.volume = args.volume; break;
        case 'mute': object.mute = args.muted; break;
        case 'follow': object.set_target_endpoint(null); break;
        case 'power': object.powered = args.powered; break;
        case 'startDiscovery': object.start_discovery(); break;
        case 'stopDiscovery': object.stop_discovery(); break;
        case 'trust': object.trusted = args.trusted; break;
        case 'connect': await asyncCall('connect_device'); break;
        case 'disconnect': await asyncCall('disconnect_device'); break;
        case 'forget': this.records.get(args.adapter).object.remove_device(object); break;
        case 'cancelPairing': object.cancel_pairing(); break;
        default: throw new Error('Unsupported native operation.');
        }
    }

    snapshot() { return this.model; }
    status() { return waybarStatus(this.model); }
    intent(command, fields = {}) {
        if (this.closing || this.disposed) throw new Error('The audio panel has stopped.');
        this.send({ type: 'snapshot', snapshot: this.capture() });
        this.send({ type: 'intent', command, ...fields });
    }
    setActive(opened, tab) {
        if (this.closing || this.disposed) return;
        this.opened = Boolean(opened); this.tab = tab;
        this.intent('active');
    }
    select(kind, key) { this.intent('select', { kind, key }); }
    setVolume(kind, value, key) { this.intent('volume', { kind, key, value }); }
    toggleMute(kind, key) { this.intent('mute', { kind, key }); }
    toggleBluetoothPower() { this.intent('bluetoothPower'); }
    deviceAction(key) { this.intent('device', { key }); }
    forget(key) { this.intent('forget', { key }); }
    sendPairingAnswer(answer) { this.intent('answer', { answer }); }
    dismissPairingMessage() { this.intent('dismissPairing'); }
    cancelPairing() { this.pairingEpoch++; this.intent('cancelPairing'); }

    dispose() {
        if (this.closing || this.disposed) return;
        this.closing = true;
        this.send({ type: 'shutdown' });
        if (this.disposed) return;
        // Only transport teardown is timed here; Rust selects all device cleanup.
        this.shutdownTimer = GLib.timeout_add(PRIORITY, 6500, () => { this.shutdownTimer = 0; this.worker?.stop(); this.finish(); return GLib.SOURCE_REMOVE; });
    }

    finish() {
        if (this.disposed || this.finishing) return;
        this.finishing = this.closing = true;
        for (const source of [this.idle, this.shutdownTimer]) if (source) GLib.source_remove(source);
        this.idle = this.shutdownTimer = 0;
        for (const [object, ids] of this.signals) for (const id of ids) object.disconnect(id);
        this.signals.clear();
        // These are opaque teardown calls registered by Rust before acquiring
        // native handles, not a JS discovery/pairing fallback. They also cover
        // abrupt IPC death, when Rust can no longer send its normal cleanup.
        const plans = [...this.records.values()].flatMap(record => record.drops.flat());
        const complete = () => { this.records.clear(); this.disposed = true; };
        if (!plans.length) { complete(); return; }
        (async () => {
            for (const plan of plans) {
                try { await this.execute(plan); } catch (_) { /* Best-effort native handle teardown. */ }
            }
        })().finally(complete);
    }
}
