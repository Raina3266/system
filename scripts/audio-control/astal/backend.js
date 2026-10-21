import GLib from 'gi://GLib';
import Gio from 'gi://Gio';
import GObject from 'gi://GObject';
import Wp from 'gi://AstalWp';
import Bluetooth from 'gi://AstalBluetooth';
import { parseChoices, policyEvent, selectionPlan, deviceChoices, bluetoothDevices, volumeValue, pairingAnswer, waybarStatus } from './logic.js';

const KINDS = ['output', 'input'];
const PRIORITY = GLib.PRIORITY_DEFAULT;

function closeStream(stream) {
    if (!stream || stream.is_closed() || stream.has_pending()) return;
    stream.close_async(PRIORITY, null, (source, result) => {
        try { source.close_finish(result); } catch (_) { /* A cancelled pipe may already be closed. */ }
    });
}

// Bounded, asynchronous pipes keep both discovery replies and interactive pairing
// off the GTK thread. Waiting is never cancelled: every child is still reaped.
export class HelperProcess {
    constructor(argv, { interactive = false, timeout = 12000, onChunk = () => {}, onExit = () => {} } = {}) {
        this.onChunk = onChunk;
        this.onExit = onExit;
        this.io = new Gio.Cancellable();
        this.output = '';
        this.bytes = 0;
        this.queue = [];
        this.done = false;
        this.exited = false;
        this.eof = false;
        this.writing = false;
        this.timer = 0;
        this.decoder = new TextDecoder('utf-8', { fatal: true });
        this.pendingBytes = new Uint8Array(0);
        try {
            this.child = Gio.Subprocess.new(argv, Gio.SubprocessFlags.STDOUT_PIPE | Gio.SubprocessFlags.STDERR_MERGE
                | (interactive ? Gio.SubprocessFlags.STDIN_PIPE : 0));
            this.stdout = this.child.get_stdout_pipe();
            this.stdin = interactive ? this.child.get_stdin_pipe() : null;
            this.child.wait_async(null, (child, result) => {
                try {
                    child.wait_finish(result);
                    this.exited = true;
                    this.complete();
                } catch (error) { this.finish(error); }
            });
            this.timer = GLib.timeout_add(PRIORITY, timeout, () => {
                this.timer = 0;
                this.finish(new Error('Audio helper timed out.'));
                return GLib.SOURCE_REMOVE;
            });
            this.read();
        } catch (error) {
            // Deliver startup failures after the caller has stored this operation.
            GLib.idle_add(PRIORITY, () => { this.finish(error); return GLib.SOURCE_REMOVE; });
        }
    }

    read() {
        this.stdout.read_bytes_async(8192, PRIORITY, this.io, (stream, result) => {
            try {
                const chunk = stream.read_bytes_finish(result);
                if (this.done) return;
                const size = chunk.get_size();
                this.bytes += size;
                if (this.bytes > 1024 * 1024) throw new Error('Audio helper response is too large.');
                // GJS has no streaming TextDecoder. Helper replies and prompts
                // are line-oriented; buffer incomplete lines (including split
                // UTF-8 sequences), and decode the final remainder at EOF.
                const data = new Uint8Array(this.pendingBytes.length + size);
                data.set(this.pendingBytes);
                data.set(chunk.toArray(), this.pendingBytes.length);
                const end = size ? data.lastIndexOf(10) + 1 : data.length;
                const text = this.decoder.decode(data.subarray(0, end));
                this.pendingBytes = data.slice(end);
                this.output += text;
                if (text) this.onChunk(text);
                if (size) this.read();
                else { this.eof = true; closeStream(stream); this.complete(); }
            } catch (error) { this.finish(error); }
            finally { if (this.done) closeStream(stream); }
        });
    }

    complete() {
        if (this.done || !this.exited || !this.eof) return;
        this.finish(this.child.get_successful() ? null : new Error(this.output.trim() || 'Audio helper failed.'));
    }

    send(text) {
        if (this.done || !this.stdin) throw new Error('No pairing request is waiting for an answer.');
        if (this.queue.length >= 8) throw new Error('Wait for the pairing request to accept the previous answer.');
        this.queue.push(new TextEncoder().encode(text));
        this.write();
    }

    write() {
        if (this.done || this.writing || !this.queue.length) return;
        this.writing = true;
        // GUnixOutputStream may perform its first write inline. Stay within
        // PIPE_BUF even when a canonical native snapshot is a large JSON line.
        const bytes = new GLib.Bytes(this.queue[0].subarray(0, 4096));
        this.stdin.write_bytes_async(bytes, PRIORITY, this.io, (stream, result) => {
            try {
                const written = stream.write_bytes_finish(result);
                if (this.done) return;
                if (written <= 0) throw new Error('The pairing helper stopped accepting input.');
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
        if (this.timer) GLib.source_remove(this.timer);
        this.timer = 0;
        this.io.cancel();
        if (error && !this.exited) this.child?.force_exit();
        this.queue = [];
        closeStream(this.stdout);
        closeStream(this.stdin);
        this.onExit(error, this.output);
    }

    stop() { this.finish(new Error('Audio operation cancelled.')); }
}

export class PolicyProcess {
    constructor(helper, request, { onSnapshot, onAction, onExit }) {
        this.result = null;
        this.rpc = 0;
        this.pending = false;
        this.stopped = false;
        this.worker = new HelperProcess([helper, 'policy'], { interactive: true,
            timeout: request.command === 'select' ? 45000 : 12000,
            onChunk: chunk => {
                for (const line of chunk.split('\n').filter(line => line.trim())) {
                    const event = policyEvent(line);
                    if (this.result || this.pending) throw new Error('Unexpected overlapping audio policy reply.');
                    if (event.type === 'result') { this.result = event; continue; }
                    if (request.command !== 'select' || event.id !== this.rpc + 1)
                        throw new Error('Unexpected audio policy request.');
                    this.rpc = event.id;
                    if (event.type === 'snapshot') this.reply({ id: event.id, snapshot: onSnapshot() });
                    else {
                        this.pending = true;
                        Promise.resolve().then(() => onAction(event)).then(() => {
                            this.pending = false;
                            this.reply({ id: event.id, ok: true });
                        }, error => {
                            this.pending = false;
                            this.reply({ id: event.id, ok: false, error: String(error.message || error).slice(0, 8192) });
                        }).catch(error => this.worker.finish(error));
                    }
                }
            },
            onExit: error => {
                this.stopped = true;
                const result = this.result;
                if (error) onExit(error);
                else if (result?.ok === false) onExit(new Error(result.error));
                else if (!result || (request.command === 'rows' ? !Array.isArray(result.rows) : typeof result.ok !== 'boolean'))
                    onExit(new Error('Incomplete audio policy response.'));
                else onExit(null, result);
            },
        });
        // A constructor must not call the caller's exit callback before it has
        // stored this operation. Normal read/write callbacks are asynchronous.
        GLib.idle_add(PRIORITY, () => {
            if (this.stopped) return GLib.SOURCE_REMOVE;
            try { this.reply(request); } catch (error) { this.worker.finish(error); }
            return GLib.SOURCE_REMOVE;
        });
    }

    reply(value) {
        if (this.stopped) return;
        const line = `${JSON.stringify(value)}\n`;
        if (new TextEncoder().encode(line).length > 1024 * 1024) throw new Error('Audio policy request is too large.');
        this.worker.send(line);
    }

    stop() { this.worker.stop(); }
}

export class AudioBackend {
    constructor({ onChange = () => {}, wp = Wp.get_default(), bluetooth = Bluetooth.get_default() } = {}) {
        this.onChange = onChange;
        this.wp = wp;
        this.audio = wp.audio;
        this.bluetooth = bluetooth;
        this.helper = GLib.getenv('AUDIO_CONTROL_HELPER') || 'audio-control';
        this.records = new Map();
        this.cardRecords = new Map();
        this.signals = [];
        this.bluetoothRecords = new Map();
        this.adapter = null;
        this.adapterSignal = 0;
        this.discoveryAdapter = null;
        this.serial = 0;
        this.idle = 0;
        this.refreshTimer = 0;
        this.poll = 0;
        this.started = false;
        this.disposed = false;
        this.opened = false;
        this.tab = 1;
        this.pairing = null;
        this.pairingLog = '';
        this.bluetoothError = '';
        this.metadata = {};
        this.defaults = {};
        this.waiters = new Set();
        for (const kind of KINDS) {
            this.metadata[kind] = { entries: [], loading: false, error: '', listing: null, selection: null,
                busy: false, dirty: false, revision: 0 };
            this.defaults[kind] = { object: null, identity: '', key: '' };
        }
    }

    changed() {
        if (this.disposed || this.idle) return;
        this.idle = GLib.idle_add(PRIORITY, () => {
            this.idle = 0;
            this.onChange();
            return GLib.SOURCE_REMOVE;
        });
    }

    start() {
        if (this.started || this.disposed) return;
        this.started = true;
        this.signals.push([this.audio, this.audio.connect('notify', (_, property) => {
            if (['devices', 'speakers', 'microphones', 'default-speaker', 'default-microphone'].includes(property.name)) {
                this.syncAudio();
                this.scheduleRefresh();
            }
        })]);
        this.signals.push([this.wp, this.wp.connect('ready', () => { this.syncAudio(); this.scheduleRefresh(); })]);
        this.signals.push([this.wp, this.wp.connect('notify::devices', () => { this.syncAudio(); this.scheduleRefresh(); })]);
        this.signals.push([this.bluetooth, this.bluetooth.connect('notify', () => this.syncBluetooth())]);
        this.syncAudio();
        this.syncBluetooth();
        this.refresh();
    }

    syncAudio() {
        const cards = new Set(this.cards());
        for (const [card, signal] of this.cardRecords) {
            if (cards.has(card)) continue;
            card.disconnect(signal);
            this.cardRecords.delete(card);
        }
        for (const card of cards) {
            if (!this.cardRecords.has(card)) this.cardRecords.set(card, card.connect('notify', () => {
                this.scheduleRefresh();
                this.changed();
            }));
        }
        const objects = new Set([...(this.audio.speakers || []), ...(this.audio.microphones || []),
            this.audio.default_speaker, this.audio.default_microphone].filter(Boolean));
        for (const [object, record] of this.records) {
            if (objects.has(object)) continue;
            object.disconnect(record.signal);
            this.records.delete(object);
        }
        for (const object of objects) {
            if (this.records.has(object)) continue;
            const record = { instance: ++this.serial };
            record.signal = object.connect('notify', (_, property) => {
                this.updateDefaults();
                if (!['volume', 'mute', 'volume-icon', 'channels', 'lock-channels'].includes(property.name))
                    this.scheduleRefresh();
                this.changed();
            });
            this.records.set(object, record);
        }
        this.updateDefaults();
        this.changed();
    }

    endpoints(kind) {
        if (!KINDS.includes(kind)) throw new Error('Unknown audio kind.');
        return (kind === 'output' ? this.audio.speakers : this.audio.microphones) || [];
    }

    endpointKey(endpoint) {
        const record = this.records.get(endpoint);
        return record && endpoint.id > 0 ? `${record.instance}:${endpoint.id}:${endpoint.get_pw_property('object.serial') || ''}` : '';
    }

    updateDefaults() {
        for (const kind of KINDS) {
            const proxy = kind === 'output' ? this.audio.default_speaker : this.audio.default_microphone;
            const object = proxy?.id > 0 ? this.endpoints(kind).find(endpoint => endpoint.id === proxy.id) || null : null;
            const identity = object ? this.endpointKey(object) : '';
            const state = this.defaults[kind];
            if (object !== state.object || identity !== state.identity) {
                state.object = object;
                state.identity = identity;
                state.key = object ? `${++this.serial}:${identity}` : '';
            }
        }
    }

    cards() {
        return [...new Set([...(this.audio.devices || []),
            ...KINDS.flatMap(kind => this.endpoints(kind).map(endpoint => endpoint.get_device()).filter(Boolean))])]
            .filter(card => card.id > 0 && /^\d+$/.test(card.get_pw_property('object.serial') || ''));
    }

    nativeIdentity(object) {
        return object ? { id: object.id, serial: object.get_pw_property('object.serial') || '' } : null;
    }

    nativeSnapshot() {
        this.updateDefaults();
        const cards = this.cards().map(card => ({ ...this.nativeIdentity(card), name: card.get_pw_property('device.name') || '',
            activeProfile: card.active_profile_id, profiles: (card.profiles || []).map(profile => ({ index: profile.index })) }));
        const nodes = KINDS.flatMap(kind => this.endpoints(kind).filter(node => node.id > 0
            && /^\d+$/.test(node.get_pw_property('object.serial') || '')).map(node => {
            const card = node.get_device();
            const binding = card && cards.find(item => item.id === card.id && item.serial === card.get_pw_property('object.serial'));
            const profileDevice = node.get_pw_property('card.profile.device') || '';
            return { ...this.nativeIdentity(node), kind, name: node.get_pw_property('node.name') || '',
                description: node.description || node.get_pw_property('node.name') || '',
                cardId: binding?.id ?? null, cardSerial: binding?.serial || '',
                profileDevice: /^\d+$/.test(profileDevice) ? Number(profileDevice) : null,
                routeName: node.route?.name || '', routes: (node.routes || []).map(route => ({
                    index: route.index, name: route.name || '', description: route.description || route.name || '',
                    available: route.available !== Wp.Available.NO,
                })),
            };
        }));
        const current = kind => {
            const identity = this.nativeIdentity(this.defaults[kind].object);
            return identity && nodes.some(node => node.id === identity.id && node.serial === identity.serial) ? identity : null;
        };
        return { cards, nodes, defaultOutput: current('output'), defaultInput: current('input') };
    }

    endpointModels(kind) {
        const snapshot = this.nativeSnapshot();
        return snapshot.nodes.filter(node => node.kind === kind).map(node => {
            const endpoint = this.endpoints(kind).find(endpoint => endpoint.id === node.id);
            return { ...node, key: this.endpointKey(endpoint), isDefault: this.defaults[kind].object === endpoint,
                routesReliable: node.cardId === null || snapshot.nodes.filter(other => other.kind === kind && other.cardId === node.cardId).length === 1 };
        });
    }

    choices(kind) {
        return deviceChoices(this.metadata[kind].entries, this.endpointModels(kind), this.nativeSnapshot().cards);
    }

    audioModel(kind) {
        const state = this.metadata[kind], current = this.defaults[kind], node = current.object;
        const choices = this.choices(kind);
        return { nodeKey: current.key, available: Boolean(node), name: node
            ? node.get_pw_property('node.nick') || node.description || node.get_pw_property('node.name') || 'No device' : 'No device',
        statusName: choices.find(choice => choice.default)?.label || node?.description || node?.get_pw_property('node.name') || '',
        volume: Number.isFinite(node?.volume) ? node.volume : 0, muted: Boolean(node?.mute),
        choices, busy: state.busy, loading: state.loading, error: state.error };
    }

    syncBluetooth() {
        const adapter = this.bluetooth.adapter || null;
        if (adapter !== this.adapter) {
            if (this.discoveryAdapter) this.stopDiscovery();
            this.cancelPairing();
            if (this.adapterSignal) this.adapter.disconnect(this.adapterSignal);
            this.adapter = adapter;
            this.adapterSignal = adapter ? adapter.connect('notify', () => {
                if (!adapter.powered) { this.cancelPairing(); this.stopDiscovery(); }
                this.changed();
            }) : 0;
        }
        // Astal's internal object-path property is omitted from its GIR, so it
        // cannot be read as adapter.object_path in GJS.
        let adapterPath = '';
        if (adapter) {
            const value = new GObject.Value();
            value.init(GObject.TYPE_STRING);
            adapter.get_property('object-path', value);
            adapterPath = value.get_string();
        }
        const devices = new Set((this.bluetooth.devices || []).filter(device => adapter && device.adapter === adapterPath));
        for (const [device, record] of this.bluetoothRecords) {
            if (devices.has(device)) continue;
            if (this.pairing?.device === device) this.cancelPairing();
            device.disconnect(record.signal);
            this.bluetoothRecords.delete(device);
        }
        for (const device of devices) {
            if (this.bluetoothRecords.has(device)) continue;
            this.bluetoothRecords.set(device, { key: `bluetooth:${++this.serial}:${device.address}`, busy: false,
                signal: device.connect('notify', () => this.changed()) });
        }
        this.changed();
    }

    snapshot() {
        this.updateDefaults();
        return { output: this.audioModel('output'), input: this.audioModel('input'), bluetooth: {
            available: Boolean(this.adapter), powered: Boolean(this.adapter?.powered), discovering: Boolean(this.adapter?.discovering),
            devices: bluetoothDevices([...this.bluetoothRecords].map(([device, record]) => ({
                key: record.key, address: device.address, name: device.name || device.alias || device.address,
                connected: device.connected, paired: device.paired, busy: record.busy,
                battery: Number.isFinite(device.battery_percentage) && device.battery_percentage >= 0
                    ? Math.min(1, device.battery_percentage) : null,
            }))), pairing: Boolean(this.pairing?.process), pairingLog: this.pairingLog, error: this.bluetoothError,
        } };
    }

    status() { return waybarStatus(this.snapshot()); }

    setActive(opened, tab) {
        if (this.disposed) return;
        this.opened = Boolean(opened);
        this.tab = tab;
        if (!this.opened || tab !== 0) { this.stopDiscovery(); this.cancelPairing(); }
        if (this.poll) GLib.source_remove(this.poll);
        this.poll = 0;
        if (this.opened && tab === 1) {
            this.refresh();
            this.poll = GLib.timeout_add(PRIORITY, 2000, () => { this.refresh(); return GLib.SOURCE_CONTINUE; });
        }
    }

    scheduleRefresh() {
        if (this.disposed) return;
        for (const kind of KINDS) this.metadata[kind].revision++;
        if (this.refreshTimer) return;
        this.refreshTimer = GLib.timeout_add(PRIORITY, 150, () => {
            this.refreshTimer = 0;
            this.refresh();
            return GLib.SOURCE_REMOVE;
        });
    }

    refresh(onlyKind = null) {
        if (this.disposed) return;
        for (const kind of onlyKind ? [onlyKind] : KINDS) {
            const state = this.metadata[kind];
            if (state.loading || state.busy) { state.dirty = true; continue; }
            state.loading = true;
            state.dirty = false;
            const revision = state.revision;
            const listing = new PolicyProcess(this.helper, { command: 'rows', kind, snapshot: this.nativeSnapshot() }, { onExit: (error, result) => {
                if (this.disposed || state.listing !== listing) return;
                state.listing = null;
                state.loading = false;
                if (revision === state.revision) {
                    try {
                        if (error) throw error;
                        state.entries = parseChoices(result.rows);
                        state.error = '';
                    } catch (failure) { state.error = failure.message; }
                } else state.dirty = true;
                this.changed();
                if (state.dirty) this.refresh(kind);
            } });
            state.listing = listing;
        }
        this.changed();
    }

    currentNode(kind, key) {
        if (this.disposed) throw new Error('The audio panel has stopped.');
        this.updateDefaults();
        const state = this.defaults[kind];
        if (!state?.object || !key || key !== state.key) throw new Error('The default audio device changed. Try again.');
        return state.object;
    }

    setVolume(kind, value, key) {
        const next = volumeValue(kind, value);
        this.currentNode(kind, key).volume = next;
        this.changed();
    }

    toggleMute(kind, key) {
        const node = this.currentNode(kind, key);
        node.mute = !node.mute;
        this.changed();
    }

    waitFor(predicate) {
        return new Promise((resolve, reject) => {
            let attempts = 0;
            const waiter = { timer: 0, cancel: () => finish(new Error('Audio operation cancelled.')) };
            const finish = error => {
                if (waiter.timer) GLib.source_remove(waiter.timer);
                waiter.timer = 0;
                this.waiters.delete(waiter);
                if (error) reject(error); else resolve();
            };
            const check = () => {
                try {
                    if (this.disposed) throw new Error('The audio panel has stopped.');
                    if (predicate()) { finish(); return GLib.SOURCE_REMOVE; }
                    if (++attempts >= 30) throw new Error('The audio device did not accept the change.');
                } catch (error) { finish(error); return GLib.SOURCE_REMOVE; }
                return GLib.SOURCE_CONTINUE;
            };
            this.waiters.add(waiter);
            waiter.timer = GLib.timeout_add(PRIORITY, 100, check);
        });
    }

    async applyPolicyAction(action, { kind, entry, initial }) {
        if (this.disposed) throw new Error('The audio panel has stopped.');
        const same = (left, right) => left?.id === right?.id && left?.serial === right?.serial;
        const cardIdentity = entry.cardId === null ? null : { id: entry.cardId, serial: entry.cardSerial };
        const card = cardIdentity && this.cards().find(card => same(this.nativeIdentity(card), cardIdentity));
        const validCard = () => {
            if (!card || !this.cards().includes(card) || !same(this.nativeIdentity(card), cardIdentity))
                throw new Error('The selected sound card disappeared or was replaced.');
            return true;
        };
        if (action.operation === 'profile') {
            if (kind !== 'output' || !entry.port || !same(action.card, cardIdentity))
                throw new Error('Policy requested an unrelated profile change.');
            validCard();
            if (!card.profiles.some(profile => profile.index === action.profile))
                throw new Error('The requested native audio profile is no longer available.');
            card.set_active_profile_id(action.profile);
            await this.waitFor(() => validCard() && card.active_profile_id === action.profile);
            return;
        }
        const node = this.endpoints(kind).find(node => same(this.nativeIdentity(node), action.node));
        if (!node) throw new Error('The requested native audio endpoint disappeared or was replaced.');
        const validNode = () => {
            if (!this.endpoints(kind).includes(node) || !same(this.nativeIdentity(node), action.node))
                throw new Error('The requested native audio endpoint disappeared or was replaced.');
            return true;
        };
        const onCard = cardIdentity && same(this.nativeIdentity(node.get_device()), cardIdentity);
        if (action.operation === 'route') {
            if (!entry.port || !onCard) throw new Error('Policy requested an unrelated physical audio port.');
            validCard();
            const route = node.routes.find(route => route.index === action.route && route.name === entry.portName
                && route.available !== Wp.Available.NO);
            if (!route) throw new Error('The requested native audio route is no longer available.');
            validNode();
            node.set_route(route);
            // Astal's route getter collapses multiple endpoints into card-wide
            // state. Rust verifies this write using the omitted Route.device
            // association; it never performs the native write itself.
            return;
        }
        if (action.operation !== 'default' || (!onCard && !initial.nodes.some(item => item.kind === kind && same(item, action.node))))
            throw new Error('Policy requested an unrelated default audio endpoint.');
        if (onCard) validCard();
        validNode();
        node.is_default = true;
        await this.waitFor(() => validNode() && node.is_default);
    }

    select(kind, key) {
        const state = this.metadata[kind];
        if (!state || this.disposed || state.busy) return;
        const entry = this.choices(kind).find(choice => choice.key === key);
        if (!entry) throw new Error('The selected audio device is no longer available.');
        const plan = selectionPlan(entry, this.endpointModels(kind));
        state.busy = true;
        state.error = '';
        state.revision++;
        // An old metadata response must never overwrite the result of this action.
        const listing = state.listing;
        state.listing = null;
        state.loading = false;
        listing?.stop();
        const apply = async () => {
            if (plan.type === 'policy') {
                const initial = this.nativeSnapshot();
                const context = { kind, entry, initial };
                await new Promise((resolve, reject) => {
                    state.selection = new PolicyProcess(this.helper, { command: 'select', kind, key: entry.key, snapshot: initial }, {
                        onSnapshot: () => this.nativeSnapshot(),
                        onAction: action => this.applyPolicyAction(action, context),
                        onExit: error => error ? reject(error) : resolve(),
                    });
                });
                return;
            }
            const endpoint = this.endpoints(kind).find(node => this.endpointKey(node) === plan.nodeKey);
            if (!endpoint) throw new Error('The selected audio device disappeared.');
            const valid = () => {
                if (!this.endpoints(kind).includes(endpoint) || this.endpointKey(endpoint) !== plan.nodeKey)
                    throw new Error('The selected audio device disappeared.');
                return true;
            };
            if (plan.route !== null) {
                const route = endpoint.routes.find(route => route.index === plan.route && route.name === entry.portName
                    && route.available !== Wp.Available.NO);
                if (!route) throw new Error('The selected audio port is no longer available.');
                endpoint.set_route(route);
                await this.waitFor(() => valid() && endpoint.route?.name === entry.portName);
            }
            if (this.disposed) return;
            valid();
            endpoint.is_default = true;
            await this.waitFor(() => valid() && endpoint.is_default);
        };
        apply().catch(error => { if (!this.disposed) state.error = error.message; }).finally(() => {
            state.selection = null;
            state.busy = false;
            this.scheduleRefresh();
            this.changed();
        });
        this.changed();
    }

    toggleBluetoothPower() {
        if (!this.adapter || this.disposed) return;
        this.bluetoothError = '';
        if (this.adapter.powered) { this.cancelPairing(); this.stopDiscovery(); }
        this.adapter.powered = !this.adapter.powered;
        this.changed();
    }

    toggleDiscovery() {
        if (!this.opened || this.tab !== 0 || !this.adapter?.powered || this.disposed) return;
        this.bluetoothError = '';
        if (this.discoveryAdapter) this.stopDiscovery();
        else { this.adapter.start_discovery(); this.discoveryAdapter = this.adapter; }
        this.changed();
    }

    stopDiscovery() {
        const adapter = this.discoveryAdapter;
        this.discoveryAdapter = null;
        if (!adapter) return;
        try { adapter.stop_discovery(); }
        catch (error) { if (!this.disposed) this.bluetoothError = error.message; }
        this.changed();
    }

    deviceFor(key) {
        const found = [...this.bluetoothRecords].find(([, record]) => record.key === key);
        if (!found || this.disposed) throw new Error('The Bluetooth device is no longer available.');
        return found;
    }

    deviceAction(key) {
        const [device, record] = this.deviceFor(key);
        if (record.busy || this.pairing || !this.adapter?.powered) return;
        this.bluetoothError = '';
        if (!device.connected && !device.paired) { this.pair(device); return; }
        const method = device.connected ? 'disconnect_device' : 'connect_device';
        record.busy = true;
        try {
            device[method]((source, result) => {
                try { source[`${method}_finish`](result); }
                catch (error) { if (!this.disposed && this.bluetoothRecords.has(device)) this.bluetoothError = error.message; }
                finally { record.busy = false; this.changed(); }
            });
        } catch (error) {
            record.busy = false;
            throw error;
        }
        this.changed();
    }

    forget(key) {
        const [device, record] = this.deviceFor(key);
        if (record.busy || this.pairing || !this.adapter) return;
        this.bluetoothError = '';
        this.adapter.remove_device(device);
        this.changed();
    }

    pair(device) {
        if (!this.opened || this.tab !== 0 || this.disposed) return;
        this.pairingLog = '';
        const pairing = { device, process: null };
        this.pairing = pairing;
        const value = new GObject.Value();
        value.init(GObject.TYPE_STRING);
        this.adapter.get_property('object-path', value);
        const adapter = value.get_string().match(/^\/org\/bluez\/(hci\d+)$/)?.[1];
        if (!adapter) { this.pairing = null; throw new Error('The native Bluetooth adapter is no longer available.'); }
        pairing.process = new HelperProcess([this.helper, 'bluetooth-pair', adapter, device.address], {
            interactive: true, timeout: 180000,
            onChunk: text => {
                if (this.pairing !== pairing) return;
                this.pairingLog = (this.pairingLog + text).slice(-32768);
                this.changed();
            },
            onExit: error => {
                if (this.pairing !== pairing) return;
                pairing.process = null;
                if (error) {
                    this.pairing = null;
                    this.bluetoothError = error.message;
                    this.changed();
                    return;
                }
                const record = this.bluetoothRecords.get(device);
                if (!record || !this.adapter?.powered) { this.cancelPairing(); return; }
                record.busy = true;
                // Only the request-local agent and its associated Pair call
                // belong in Rust. These are existing Astal Bluetooth APIs.
                try {
                    device.trusted = true;
                    device.connect_device((source, result) => {
                        try {
                            source.connect_device_finish(result);
                            if (this.pairing === pairing)
                                this.pairingLog += `${device.alias || device.address} (${device.address}) is paired and connected.\n`;
                        } catch (failure) {
                            if (this.pairing === pairing) this.bluetoothError = failure.message;
                        } finally {
                            record.busy = false;
                            if (this.pairing === pairing) this.pairing = null;
                            this.changed();
                        }
                    });
                } catch (failure) {
                    record.busy = false;
                    this.pairing = null;
                    this.bluetoothError = failure.message;
                }
                this.changed();
            },
        });
        this.changed();
    }

    sendPairingAnswer(text) {
        if (!this.pairing) throw new Error('No pairing request is waiting for an answer.');
        if (!this.pairing.process) throw new Error('Pairing has completed; connecting through Astal.');
        this.pairing.process.send(pairingAnswer(text));
    }

    cancelPairing() {
        const pairing = this.pairing;
        this.pairing = null;
        if (!pairing) return;
        try { pairing.device.cancel_pairing(); }
        catch (_) { /* BlueZ may already have completed or cancelled this request. */ }
        pairing.process?.stop();
        this.changed();
    }

    dispose() {
        if (this.disposed) return;
        this.disposed = true;
        this.cancelPairing();
        this.stopDiscovery();
        for (const source of [this.idle, this.poll, this.refreshTimer]) if (source) GLib.source_remove(source);
        this.idle = this.poll = this.refreshTimer = 0;
        for (const waiter of [...this.waiters]) waiter.cancel();
        for (const kind of KINDS) {
            this.metadata[kind].listing?.stop();
            this.metadata[kind].selection?.stop();
        }
        for (const [object, signal] of this.signals) object.disconnect(signal);
        for (const [object, record] of this.records) object.disconnect(record.signal);
        for (const [card, signal] of this.cardRecords) card.disconnect(signal);
        for (const [device, record] of this.bluetoothRecords) device.disconnect(record.signal);
        if (this.adapterSignal) this.adapter.disconnect(this.adapterSignal);
        this.signals = [];
        this.records.clear();
        this.cardRecords.clear();
        this.bluetoothRecords.clear();
    }
}
