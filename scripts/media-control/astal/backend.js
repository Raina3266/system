import GLib from 'gi://GLib';
import Gio from 'gi://Gio';
import Mpris from 'gi://AstalMpris';
import Wp from 'gi://AstalWp';
import { badge } from './logic.js';

export const clock = () => GLib.get_monotonic_time() / 1e6;
const encoder = new TextEncoder();
const decoder = new TextDecoder('utf-8', { fatal: true });
const MAX_HELPER_BYTES = 1024 * 1024;
const PIPE_CHUNK_BYTES = 8192;
const HELPER_TIMEOUT_MS = 12000;
const SERVICE_TIMEOUT_MS = 4000;
const SERVICE_RETRY_MS = 1500;
const PROPERTIES = [
    'application.name', 'application.id', 'application.process.binary',
    'media.title', 'media.filename', 'media.url', 'node.nick',
];
const meta = (player, key) => player.get_meta(key)?.deep_unpack() ?? null;
const finite = value => Number.isFinite(value) ? value : null;
const message = error => error instanceof Error ? error.message : String(error);

function canonical(value) {
    if (Array.isArray(value)) return value.map(canonical);
    if (value && typeof value === 'object')
        return Object.fromEntries(Object.keys(value).sort().map(key => [key, canonical(value[key])]));
    return value;
}

// Rust chooses the guarded fields. This is an opaque native-object/IPC check,
// not a second implementation of source selection or action policy.
export function matchesGuard(guard, snapshot) {
    return Boolean(guard && snapshot && ['players', 'streams'].every(kind =>
        Array.isArray(guard[kind]) && Array.isArray(snapshot[kind])
        && guard[kind].length === snapshot[kind].length
        && guard[kind].every((expected, index) => expected && typeof expected === 'object'
            && Object.entries(expected).every(([key, value]) =>
                JSON.stringify(canonical(snapshot[kind][index]?.[key])) === JSON.stringify(canonical(value))))));
}

// Only Astal-native objects and their raw properties live here. Discovery beyond
// Astal, compatibility normalization, timing and policy all belong to Rust.
class NativeMedia {
    constructor({ onChange = () => {} } = {}) {
        this.mpris = Mpris.get_default();
        this.audio = Wp.get_default().audio;
        this.onChange = onChange;
        this.players = new Map();
        this.streams = new Map();
        this.serial = 0;
        this.idle = 0;
        this.signals = [
            [this.mpris, this.mpris.connect('notify::players', () => this.sync())],
            [this.audio, this.audio.connect('notify::streams', () => this.sync())],
        ];
        this.sync();
    }

    changed() {
        if (this.idle) return;
        this.idle = GLib.idle_add(GLib.PRIORITY_DEFAULT_IDLE, () => {
            this.idle = 0;
            this.onChange();
            return GLib.SOURCE_REMOVE;
        });
    }

    sync() {
        this.syncObjects(this.players, this.mpris.players, player => player.bus_name, player => {
            const record = { object: player, instance: ++this.serial, seekRevision: 0, positionRevision: 0 };
            record.signal = player.connect('notify', (_, property) => {
                if (property?.name === 'position') record.positionRevision++;
                this.changed();
            });
            return record;
        });
        this.syncObjects(this.streams, this.audio.streams, stream => stream.id, stream => ({
            object: stream, instance: ++this.serial, signal: stream.connect('notify', () => this.changed()),
        }));
        this.changed();
    }

    syncObjects(records, objects, key, create) {
        const current = new Map((objects || []).map(object => [key(object), object]));
        for (const [id, record] of records) {
            if (current.get(id) === record.object) continue;
            record.object.disconnect(record.signal);
            records.delete(id);
        }
        for (const [id, object] of current)
            if (!records.has(id)) records.set(id, create(object));
    }

    capture() {
        return {
            players: [...this.players].map(([bus, record]) => {
                const player = record.object;
                return {
                    bus, instance: record.instance, trackId: player.trackid || '',
                    identity: player.identity || '', desktopEntry: player.entry || '',
                    title: player.title || '', artist: player.artist || '', album: player.album || '',
                    url: String(meta(player, 'xesam:url') || ''), artUrl: player.art_url || '',
                    browserHint: meta(player, 'mprisence:browser'),
                    playing: player.playback_status === Mpris.PlaybackStatus.PLAYING,
                    stopped: player.playback_status === Mpris.PlaybackStatus.STOPPED,
                    position: finite(player.position), positionSupported: Number.isFinite(player.position) && player.position >= 0,
                    positionRevision: record.positionRevision, seekRevision: record.seekRevision,
                    rate: finite(player.rate), length: finite(player.length), lengthMicros: meta(player, 'mpris:length'),
                    canPlay: Boolean(player.can_play), canPause: Boolean(player.can_pause), canSeek: Boolean(player.can_seek),
                    canNext: Boolean(player.can_go_next), canPrevious: Boolean(player.can_go_previous),
                    cover: player.cover_art || '', volume: finite(player.volume), available: Boolean(player.available),
                };
            }),
            streams: [...this.streams.values()].map(({ object: stream, instance }) => {
                const properties = Object.fromEntries(PROPERTIES.map(key => [key, stream.get_pw_property(key) || '']));
                properties['media.name'] = stream.name || '';
                return { id: stream.id, instance, name: stream.name || '', nickname: properties['node.nick'],
                    ready: stream.id > 0 && Number.isFinite(stream.volume) && stream.volume >= 0,
                    properties, volume: finite(stream.volume), muted: Boolean(stream.mute) };
            }),
        };
    }

    apply(operation) {
        if (operation.streamId !== undefined) {
            const stream = this.streams.get(operation.streamId)?.object;
            if (!stream) throw new Error('The audio stream disappeared.');
            if (operation.operation === 'volume') stream.volume = operation.value;
            else if (operation.operation !== 'mute') throw new Error('Unsupported native stream operation.');
            if (operation.muted !== undefined) stream.mute = operation.muted;
            return;
        }
        const record = this.players.get(operation.player);
        if (!record?.object.available) throw new Error('The media player disappeared.');
        const player = record.object;
        switch (operation.operation) {
        case 'play': player.play(); break;
        case 'pause': player.pause(); break;
        case 'next': player.next(); break;
        case 'previous': player.previous(); break;
        case 'seek': player.position = operation.value; record.seekRevision++; break;
        case 'volume': player.volume = operation.value; break;
        default: throw new Error('Unsupported native media operation.');
        }
    }

    dispose() {
        if (this.idle) GLib.source_remove(this.idle);
        this.idle = 0;
        for (const [object, signal] of this.signals) object.disconnect(signal);
        for (const records of [this.players, this.streams]) {
            for (const record of records.values()) record.object.disconnect(record.signal);
            records.clear();
        }
    }
}

export function runHelper(argv, input = null, cancellable = new Gio.Cancellable()) {
    return new Promise((resolve, reject) => {
        const io = new Gio.Cancellable();
        let process = null, output = null, stdin = null;
        let settled = false, exited = false, outputDone = false, inputDone = input === null;
        let timer = 0, cancelId = 0, length = 0;
        let chunks = [];
        const close = stream => {
            if (!stream || stream.is_closed() || stream.has_pending()) return;
            stream.close_async(GLib.PRIORITY_DEFAULT, null, (source, result) => {
                try { source.close_finish(result); } catch (_) { /* Best-effort pipe cleanup. */ }
            });
        };
        const settle = (error, value) => {
            if (settled) return;
            settled = true;
            if (timer) GLib.source_remove(timer);
            timer = 0;
            if (cancelId) {
                // Disconnecting inside the cancellable's own handler can deadlock.
                GLib.idle_add(GLib.PRIORITY_DEFAULT, () => {
                    cancellable.disconnect(cancelId);
                    cancelId = 0;
                    return GLib.SOURCE_REMOVE;
                });
            }
            if (error) {
                io.cancel();
                if (!exited) process?.force_exit();
            }
            chunks = [];
            close(output);
            close(stdin);
            if (error) reject(error);
            else resolve(value);
        };
        const complete = () => {
            if (settled || !exited || !outputDone || !inputDone) return;
            try {
                const bytes = new Uint8Array(length);
                let offset = 0;
                for (const chunk of chunks) {
                    const data = chunk.toArray();
                    bytes.set(data, offset);
                    offset += data.length;
                }
                settle(null, decoder.decode(bytes));
            } catch (error) { settle(error); }
        };
        const read = () => output.read_bytes_async(PIPE_CHUNK_BYTES, GLib.PRIORITY_DEFAULT, io, (stream, result) => {
            try {
                const bytes = stream.read_bytes_finish(result);
                if (settled) return;
                const size = bytes.get_size();
                if (!size) {
                    outputDone = true;
                    close(stream);
                    complete();
                } else if (length + size > MAX_HELPER_BYTES) {
                    settle(new Error('Media helper response is too large.'));
                } else {
                    length += size;
                    chunks.push(bytes);
                    read();
                }
            } catch (error) { settle(error); }
            finally { if (settled) close(stream); }
        });
        const write = (data, offset = 0) => {
            if (offset === data.length) {
                stdin.close_async(GLib.PRIORITY_DEFAULT, io, (stream, result) => {
                    try {
                        stream.close_finish(result);
                        inputDone = true;
                        complete();
                    } catch (error) { settle(error); }
                    finally { if (settled) close(stream); }
                });
                return;
            }
            const end = Math.min(data.length, offset + 4096);
            const bytes = new GLib.Bytes(data.slice(offset, end));
            stdin.write_bytes_async(bytes, GLib.PRIORITY_DEFAULT, io, (stream, result) => {
                try {
                    const written = stream.write_bytes_finish(result);
                    if (!settled) {
                        if (written <= 0) throw new Error('Media helper input pipe stopped accepting data.');
                        write(data, offset + written);
                    }
                } catch (error) { settle(error); }
                finally { if (settled) close(stream); }
            });
        };
        try {
            if (cancellable.is_cancelled()) throw new Error('Media lookup cancelled.');
            if (input !== null && typeof input !== 'string') throw new TypeError('Media helper input must be a string or null.');
            if (input !== null && input.length > MAX_HELPER_BYTES) throw new Error('Media helper input is too large.');
            const data = input === null ? null : encoder.encode(input);
            if (data && data.length > MAX_HELPER_BYTES) throw new Error('Media helper input is too large.');
            process = Gio.Subprocess.new(argv, Gio.SubprocessFlags.STDOUT_PIPE | (input === null ? 0 : Gio.SubprocessFlags.STDIN_PIPE));
            output = process.get_stdout_pipe();
            stdin = data === null ? null : process.get_stdin_pipe();
            // Waiting is never cancelled: rejected lookups still reap their child.
            process.wait_async(null, (child, result) => {
                try {
                    child.wait_finish(result);
                    exited = true;
                    if (settled) return;
                    if (!child.get_successful()) throw new Error('Media helper failed; check the user-service journal.');
                    complete();
                } catch (error) { settle(error); }
            });
            cancelId = cancellable.connect(() => settle(new Error('Media lookup cancelled.')));
            if (settled) return;
            timer = GLib.timeout_add(GLib.PRIORITY_DEFAULT, HELPER_TIMEOUT_MS, () => {
                timer = 0;
                settle(new Error('Media helper timed out.'));
                return GLib.SOURCE_REMOVE;
            });
            read();
            if (stdin) write(data);
        } catch (error) { settle(error); }
    });
}

// Bounded JSONL transport only. Rust owns the intent queue and all media policy.
class ServiceWorker {
    constructor(helper, { onMessage = () => {}, onFailure = () => {} } = {}) {
        Object.assign(this, { helper, onMessage, onFailure });
        this.generation = 0;
        this.child = null;
        this.ready = this.disposed = false;
        this.watchdog = this.retry = 0;
        this.queue = [];
        this.pendingBytes = 0;
        this.writing = false;
    }
    start() {
        if (this.disposed || this.child) return;
        const generation = ++this.generation;
        try {
            this.cancel = new Gio.Cancellable();
            this.child = Gio.Subprocess.new([this.helper, 'service'], Gio.SubprocessFlags.STDIN_PIPE | Gio.SubprocessFlags.STDOUT_PIPE);
            this.output = this.child.get_stdout_pipe();
            this.buffer = new Uint8Array();
            this.read(generation);
            this.child.wait_async(null, (child, result) => {
                try { child.wait_finish(result); }
                catch (error) { console.warn(`Could not wait for media helper: ${message(error)}`); }
                if (generation === this.generation) this.failed('Media helper exited.');
            });
            this.armWatchdog();
        } catch (error) { this.failed(`Could not start media-control: ${message(error)}`); }
    }
    armWatchdog() {
        if (this.watchdog) return;
        this.watchdog = GLib.timeout_add(GLib.PRIORITY_DEFAULT, SERVICE_TIMEOUT_MS, () => {
            this.watchdog = 0;
            this.failed('Media helper timed out; pending actions were discarded.');
            return GLib.SOURCE_REMOVE;
        });
    }
    send(value) {
        if (!this.child || !this.ready) return false;
        const data = encoder.encode(JSON.stringify(value) + '\n');
        if (data.length > MAX_HELPER_BYTES) throw new Error('Media snapshot is too large.');
        const tail = this.queue.at(-1);
        const replace = value.type === 'snapshot' && tail?.type === 'snapshot';
        const pending = this.pendingBytes + data.length - (replace ? tail.data.length : 0);
        if (pending > MAX_HELPER_BYTES * 8) throw new Error('Media helper input queue is full.');
        this.pendingBytes = pending;
        if (replace) this.queue[this.queue.length - 1] = { type: value.type, data };
        else this.queue.push({ type: value.type, data });
        this.armWatchdog();
        this.write();
        return true;
    }
    write() {
        if (this.writing || !this.queue.length || !this.child) return;
        const { data } = this.queue.shift();
        const generation = this.generation;
        const input = this.child.get_stdin_pipe();
        this.writing = true;
        const next = offset => {
            if (generation !== this.generation) return;
            if (offset === data.length) {
                this.pendingBytes -= data.length;
                this.writing = false;
                this.write();
                return;
            }
            const bytes = new GLib.Bytes(data.slice(offset, Math.min(data.length, offset + 4096)));
            input.write_bytes_async(bytes, GLib.PRIORITY_DEFAULT, this.cancel, (stream, result) => {
                try {
                    const written = stream.write_bytes_finish(result);
                    if (written <= 0) throw new Error('Media helper input pipe stopped accepting data.');
                    next(offset + written);
                } catch (error) { if (generation === this.generation) this.failed(message(error)); }
            });
        };
        next(0);
    }
    read(generation) {
        this.output.read_bytes_async(PIPE_CHUNK_BYTES, GLib.PRIORITY_DEFAULT, this.cancel, (stream, result) => {
            if (generation !== this.generation) return;
            try {
                const bytes = stream.read_bytes_finish(result).toArray();
                if (!bytes.length) throw new Error('Media helper closed its output.');
                const joined = new Uint8Array(this.buffer.length + bytes.length);
                joined.set(this.buffer);
                joined.set(bytes, this.buffer.length);
                let start = 0;
                for (let index = 0; index < joined.length; index++) {
                    if (joined[index] !== 10) continue;
                    if (index - start > MAX_HELPER_BYTES) throw new Error('Media service response is too large.');
                    const value = JSON.parse(decoder.decode(joined.subarray(start, index)));
                    if (!value || typeof value.type !== 'string') throw new Error('Invalid media service response.');
                    if (this.watchdog) GLib.source_remove(this.watchdog);
                    this.watchdog = 0;
                    if (value.type === 'ready') this.ready = true;
                    this.onMessage(value);
                    if (generation !== this.generation) return;
                    start = index + 1;
                }
                if (joined.length - start > MAX_HELPER_BYTES) throw new Error('Media service response is too large.');
                this.buffer = joined.slice(start);
                this.read(generation);
            } catch (error) { this.failed(`Media helper protocol failed: ${message(error)}`); }
        });
    }
    failed(reason) {
        if (this.disposed) return;
        this.stop();
        this.onFailure(reason);
        this.retry = GLib.timeout_add(GLib.PRIORITY_DEFAULT, SERVICE_RETRY_MS, () => {
            this.retry = 0;
            this.start();
            return GLib.SOURCE_REMOVE;
        });
    }
    stop() {
        this.generation++;
        if (this.watchdog) GLib.source_remove(this.watchdog);
        if (this.retry) GLib.source_remove(this.retry);
        this.watchdog = this.retry = 0;
        this.cancel?.cancel();
        this.child?.force_exit();
        this.child = null;
        this.ready = false;
        this.queue = [];
        this.pendingBytes = 0;
        this.writing = false;
    }
    dispose() { this.disposed = true; this.stop(); }
}

export class MediaBackend {
    constructor({ onChange = () => {}, onProgress = onChange } = {}) {
        this.helper = GLib.getenv('MEDIA_CONTROL') || 'media-control';
        this.onChange = onChange;
        this.onProgress = onProgress;
        this.disposed = this.ready = false;
        this.groups = [];
        this.players = new Map();
        this.positions = new Map();
        this.transports = {};

        this.activePlayer = this.failure = '';
        this.guard = null;
        this.native = new NativeMedia({ onChange: () => this.refresh() });
        this.worker = new ServiceWorker(this.helper, {
            onMessage: value => this._receive(value), onFailure: error => this._failed(error),
        });
    }
    start() { this.worker.start(); }
    resolveGroups() {
        return this.groups.map(group => ({ ...group, primary: this.player(group.primary) })).filter(group => group.primary);
    }
    player(bus) { return this.players.get(bus) || null; }
    progress(bus) { return this.positions.get(bus) || null; }
    current() { return Boolean(!this.disposed && this.ready && matchesGuard(this.guard, this.native.capture())); }
    transport(group) {
        if (!this.current() || !group?.primary || this.player(group.primary.bus)?.instance !== group.primary.instance)
            return { enabled: false, reason: 'Media source changed or is not ready. Refresh and try again.' };
        return this.transports[group.primary.bus] || { enabled: false, reason: 'A media group member is unavailable. Refresh and try again.' };
    }
    _send(value) {
        if (this.disposed) return false;
        try { return this.worker.send(value); }
        catch (error) { this.failure = message(error); this.onChange(); return false; }
    }
    refresh() { return this._send({ type: 'snapshot', snapshot: this.native.capture() }); }
    tick() { return this._send({ type: 'tick' }); }
    action(kind, bus = '', position = 0) {
        return this._send({ type: 'action', action: { kind, player: bus, position },
            createdAt: clock(), snapshot: this.native.capture() });
    }
    select(player) { return this._send({ type: 'select', player, snapshot: this.native.capture() }); }
    cycle() { return this._send({ type: 'cycle', snapshot: this.native.capture() }); }
    reportFailure(error) { this.failure = error; this._send({ type: 'report', error }); }
    status() {
        return JSON.stringify(badge(this.resolveGroups().find(group => group.primary.bus === this.activePlayer)));
    }

    _receive(value) {
        if (this.disposed) return;
        switch (value.type) {
        case 'ack': break;
        case 'ready': this.refresh(); break;
        case 'capture': this.refresh(); break;
        case 'state':
            if (!Array.isArray(value.groups) || !Array.isArray(value.players) || !Array.isArray(value.progress)
                || typeof value.activePlayer !== 'string' || typeof value.failure !== 'string' || !value.guard)
                throw new Error('Invalid media service state.');
            this.ready = value.ready === true;
            this.groups = value.groups;
            this.players = new Map(value.players.map(player => [player.bus, player]));
            this.positions = new Map(value.progress.map(player => [player.bus, player]));
            this.transports = value.transports || {};

            this.activePlayer = value.activePlayer;
            this.failure = value.failure;
            this.guard = value.guard;
            this.onChange();
            break;
        case 'progress':
            if (!Array.isArray(value.progress)) throw new Error('Invalid media progress response.');
            this.positions = new Map(value.progress.map(player => [player.bus, player]));
            this.onProgress();
            break;
        case 'apply': {
            let ok = false, error;
            try {
                if (!Number.isFinite(value.expiresAt) || clock() > value.expiresAt) throw new Error('The native operation expired.');
                if (!matchesGuard(value.guard, this.native.capture())) throw new Error('The native snapshot changed before dispatch.');
                this.native.apply(value.operation);
                ok = true;
            } catch (failure) { error = message(failure); }
            this._send({ type: 'result', id: value.id, ok, ...(error ? { error } : {}), snapshot: this.native.capture() });
            break;
        }
        default: throw new Error('Unknown media service response.');
        }
    }
    _failed(error) {
        this.ready = false;
        this.groups = [];
        this.players.clear();
        this.positions.clear();
        this.transports = {};
        this.guard = null;
        this.activePlayer = '';
        this.failure = error;
        this.onChange();
    }
    _checkLookup(cancellable) {
        if (this.disposed) throw new Error('The media backend was disposed.');
        if (cancellable.is_cancelled()) throw new Error('Media lookup cancelled.');
    }
    async resolveArtwork(trackUrl, cancellable = null) {
        cancellable ??= new Gio.Cancellable();
        this._checkLookup(cancellable);
        if (typeof trackUrl !== 'string' || !trackUrl.length || trackUrl.includes('\0'))
            throw new TypeError('Artwork requires a track URL or absolute path.');
        const output = await runHelper([this.helper, 'artwork', trackUrl], null, cancellable);
        this._checkLookup(cancellable);
        const path = output.replace(/\r?\n$/, '');
        if (!path.startsWith('/') || /[\x00-\x1f\x7f]/.test(path)) throw new Error('Media artwork helper returned an invalid absolute path.');
        return path;
    }
    async resolveCredit(metadata, cancellable = null) {
        cancellable ??= new Gio.Cancellable();
        this._checkLookup(cancellable);
        const request = Object.fromEntries(['url', 'artist', 'album'].map(key => {
            if (typeof metadata?.[key] !== 'string') throw new Error(`Credit ${key} must be a string.`);
            return [key, metadata[key]];
        }));
        const output = await runHelper([this.helper, 'credit'], JSON.stringify(request), cancellable);
        this._checkLookup(cancellable);
        let reply;
        try { reply = JSON.parse(output); }
        catch (_) { throw new Error('Media credit helper returned invalid JSON.'); }
        if (!reply || Array.isArray(reply) || typeof reply.credit !== 'string'
            || Object.keys(request).some(key => reply[key] !== request[key]))
            throw new Error('Media credit helper returned an invalid or mismatched reply.');
        return reply.credit;
    }
    dispose() {
        if (this.disposed) return;
        this.disposed = true;
        this.ready = false;
        this.guard = null;
        this.worker.dispose();
        this.native.dispose();
    }
}
