import GLib from 'gi://GLib';
import Gio from 'gi://Gio';
import Mpris from 'gi://AstalMpris';
import Wp from 'gi://AstalWp';
import { Timeline, Planner, badge } from './logic.js';

export const clock = () => GLib.get_monotonic_time() / 1e6;
const bounded = value => Math.min(1, Math.max(0, value));
const PROPERTIES = [
    'application.name', 'application.id', 'application.process.binary',
    'media.name', 'media.title', 'media.filename', 'media.url', 'node.nick',
];

function meta(player, key) {
    const value = player.get_meta(key);
    return value ? value.deep_unpack() : null;
}

function identity(player) {
    return JSON.stringify([player.trackid, player.title, player.artist, player.album, meta(player, 'xesam:url')]);
}

// Volume must use a fresh ownership decision, not a recycled native node ID.
function ownershipStamp(data) {
    return JSON.stringify({
        players: data.players.map(({ position, timelineEpoch, rate, positionSupported, cover, ...player }) => player),
        streams: data.streams,
    });
}

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
            const record = { object: player, instance: ++this.serial, timeline: new Timeline() };
            const update = sampled => {
                const result = record.timeline.update({
                    position: player.position, playing: player.playback_status === Mpris.PlaybackStatus.PLAYING,
                    rate: player.rate, length: player.length, identity: identity(player), sampled,
                }, clock());
                if (result.error) console.warn(result.error);
            };
            update(true);
            record.signal = player.connect('notify', (_, property) => {
                if (['position', 'metadata', 'playback-status', 'rate'].includes(property.name))
                    update(property.name === 'position');
                this.changed();
            });
            return record;
        });
        this.syncObjects(this.streams, this.audio.streams, stream => stream.id, stream => ({
            object: stream, instance: ++this.serial,
            signal: stream.connect('notify', () => this.changed()),
        }));
        this.changed();
    }

    syncObjects(records, objects, key, create) {
        const current = new Map((objects || []).map(object => [key(object), object]));
        for (const [id, record] of records) {
            if (current.get(id) !== record.object) {
                record.object.disconnect(record.signal);
                records.delete(id);
            }
        }
        for (const [id, object] of current)
            if (!records.has(id)) records.set(id, create(object));
    }

    player(bus) {
        const record = this.players.get(bus);
        if (!record) return null;
        const player = record.object;
        const timeline = record.timeline;
        const rate = Number.isFinite(player.rate) ? player.rate : 1;
        return {
            bus, instance: record.instance, trackId: player.trackid || '',
            identity: player.identity || '', desktopEntry: player.entry || '',
            title: player.title || '', artist: player.artist || '', album: player.album || '',
            url: String(meta(player, 'xesam:url') || ''), artUrl: player.art_url || '',
            browserHint: meta(player, 'mprisence:browser'),
            playing: player.playback_status === Mpris.PlaybackStatus.PLAYING,
            stopped: player.playback_status === Mpris.PlaybackStatus.STOPPED,
            position: timeline.value(clock()), positionSupported: timeline.supported,
            timelineEpoch: timeline.epoch, rate,
            length: player.length > 0 && Number.isFinite(player.length) ? player.length : null,
            canSeek: Boolean(player.can_seek && timeline.supported && player.trackid?.startsWith('/')),
            canNext: Boolean(player.can_go_next), canPrevious: Boolean(player.can_go_previous),
            cover: this.coverPath(player),
        };
    }

    coverPath(player) {
        const url = player.art_url || '';
        if (!url) return '';
        // Astal's asynchronous loader can complete after a track change. Only
        // accept the cache path belonging to the CURRENT advertised artwork.
        const local = Gio.File.new_for_uri(url).get_path();
        const expected = local || GLib.build_filenamev([
            GLib.get_user_cache_dir(), 'astal', 'mpris',
            GLib.compute_checksum_for_string(GLib.ChecksumType.SHA1, url, -1),
        ]);
        return player.cover_art === expected ? expected : '';
    }

    capture() {
        return {
            players: [...this.players.keys()].map(bus => this.player(bus)),
            streams: [...this.streams.values()].map(({ object: stream, instance }) => {
                const properties = Object.fromEntries(PROPERTIES.map(key => [key, stream.get_pw_property(key) || '']));
                return {
                    id: stream.id, instance, name: stream.name || '', nickname: properties['node.nick'],
                    ready: stream.id > 0 && Number.isFinite(stream.volume) && stream.volume >= 0,
                    properties,
                };
            }),
        };
    }

    apply(operation) {
        const record = this.players.get(operation.player);
        if (!record?.object.available) throw new Error('The media player disappeared.');
        const player = record.object;
        switch (operation.operation) {
        case 'play': player.play(); break;
        // Deliberately do not gate pause on CanPause: pause-all includes players
        // that advertise it incorrectly. The remote player can still refuse.
        case 'pause': player.pause(); break;
        case 'next': player.next(); break;
        case 'previous': player.previous(); break;
        case 'seek': {
            if (!this.player(operation.player).canSeek) throw new Error('Seeking is unavailable for this track.');
            const result = record.timeline.seek(operation.value, clock());
            if (result.error) throw new Error(result.error);
            player.position = operation.value;
            this.changed();
            break;
        }
        default: throw new Error('Unsupported native media operation.');
        }
    }

    volume(group, stamp) {
        const player = group?.primary;
        const record = player && this.players.get(player.bus);
        const valid = () => record && this.players.get(player.bus) === record
            && record.instance === player.instance && record.object.available
            && ownershipStamp(this.capture()) === stamp;
        const mprisVolume = () => record && Number.isFinite(record.object.volume) && record.object.volume >= 0;
        if (mprisVolume()) {
            return {
                enabled: Boolean(valid()), value: bounded(record.object.volume), key: `player:${record.instance}`,
                reason: 'Media volume (0–100%)',
                set: value => {
                    if (!Number.isFinite(value) || !valid() || !mprisVolume()) return false;
                    record.object.volume = bounded(value);
                    return true;
                },
            };
        }
        const stream = this.streams.get(group?.streamId);
        if (stream && stream.object.id > 0 && Number.isFinite(stream.object.volume) && stream.object.volume >= 0) {
            return {
                enabled: Boolean(valid()), value: stream.object.mute ? 0 : bounded(stream.object.volume),
                key: `stream:${stream.instance}`, reason: 'Application stream volume (0–100%)',
                set: value => {
                    if (!Number.isFinite(value) || !valid() || mprisVolume()
                        || this.streams.get(group.streamId) !== stream) return false;
                    stream.object.volume = bounded(value);
                    if (value > 0) stream.object.mute = false;
                    return true;
                },
            };
        }
        return { enabled: false, value: 0, key: '', reason: group?.streamReason || 'Independent media volume is unavailable.', set: () => false };
    }

    dispose() {
        if (this.idle) GLib.source_remove(this.idle);
        this.idle = 0;
        this.signals.forEach(([object, signal]) => object.disconnect(signal));
        for (const records of [this.players, this.streams]) {
            for (const record of records.values()) record.object.disconnect(record.signal);
            records.clear();
        }
    }
}

const encoder = new TextEncoder();
const MAX_HELPER_BYTES = 1024 * 1024;
const HELPER_TIMEOUT_MS = 12000;

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
                settle(null, new TextDecoder('utf-8', { fatal: true }).decode(bytes));
            } catch (error) { settle(error); }
        };
        const read = () => output.read_bytes_async(8192, GLib.PRIORITY_DEFAULT, io, (stream, result) => {
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
            // GUnixOutputStream may do the first async write inline. Stay within
            // Linux PIPE_BUF so a writable pipe cannot block the main loop.
            const end = Math.min(data.length, offset + 4096);
            // GBytes owns the buffer throughout the async call, across JS GC.
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

            if (input !== null && typeof input !== 'string')
                throw new TypeError('Media helper input must be a string or null.');
            if (input !== null && input.length > MAX_HELPER_BYTES)
                throw new Error('Media helper input is too large.');
            const data = input === null ? null : encoder.encode(input);
            if (data && data.length > MAX_HELPER_BYTES) throw new Error('Media helper input is too large.');
            process = Gio.Subprocess.new(argv, Gio.SubprocessFlags.STDOUT_PIPE
                | (input === null ? 0 : Gio.SubprocessFlags.STDIN_PIPE));
            output = process.get_stdout_pipe();
            stdin = data === null ? null : process.get_stdin_pipe();
            // Never cancel the wait: even an already-rejected lookup must reap its child.
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

// One request in flight, asynchronous pipes, bounded replies, and a watchdog.
// Failures discard pending actions before a new worker is started.
class ModelWorker {
    constructor(helper, { onReady, onResponse, onFailure }) {
        this.helper = helper;
        this.onReady = onReady;
        this.onResponse = onResponse;
        this.onFailure = onFailure;
        this.child = null;
        this.generation = 0;
        this.pending = null;
        this.watchdog = 0;
        this.retry = 0;
        this.disposed = false;
    }

    start() {
        if (this.disposed || this.child) return;
        const generation = ++this.generation;
        try {
            this.cancel = new Gio.Cancellable();
            this.child = Gio.Subprocess.new([this.helper, 'model'],
                Gio.SubprocessFlags.STDIN_PIPE | Gio.SubprocessFlags.STDOUT_PIPE);
            this.output = this.child.get_stdout_pipe();
            this.buffer = new Uint8Array();
            this.read(generation);
            this.child.wait_async(null, () => {
                if (generation === this.generation) this.failed('Media helper exited.');
            });
            this.onReady();
        } catch (error) {
            this.failed(`Could not start media-control: ${error.message}`);
        }
    }

    send(request) {
        if (!this.child || this.pending !== null) throw new Error('Media worker is not ready.');
        const data = encoder.encode(JSON.stringify(request) + '\n');
        if (data.length > 1024 * 1024) throw new Error('Media snapshot is too large.');
        this.pending = request.requestId;
        const generation = this.generation;
        this.watchdog = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 4000, () => {
            this.watchdog = 0;
            this.failed('Media helper timed out; pending actions were discarded.');
            return GLib.SOURCE_REMOVE;
        });
        this.child.get_stdin_pipe().write_all_async(data, GLib.PRIORITY_DEFAULT, this.cancel, (stream, result) => {
            try { stream.write_all_finish(result); }
            catch (error) { if (generation === this.generation) this.failed(error.message); }
        });
    }

    read(generation) {
        this.output.read_bytes_async(8192, GLib.PRIORITY_DEFAULT, this.cancel, (stream, result) => {
            if (generation !== this.generation) return;
            try {
                const bytes = stream.read_bytes_finish(result).toArray();
                if (!bytes.length) throw new Error('Media helper closed its output.');
                const joined = new Uint8Array(this.buffer.length + bytes.length);
                joined.set(this.buffer);
                joined.set(bytes, this.buffer.length);
                if (joined.length > 1024 * 1024) throw new Error('Media model response is too large.');
                let start = 0;
                for (let index = 0; index < joined.length; index++) {
                    if (joined[index] !== 10) continue;
                    const response = JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(joined.subarray(start, index)));
                    if (this.pending === null || response.requestId !== this.pending)
                        throw new Error('Unexpected media model response.');
                    this.pending = null;
                    if (this.watchdog) GLib.source_remove(this.watchdog);
                    this.watchdog = 0;
                    this.onResponse(response);
                    if (generation !== this.generation) return;
                    start = index + 1;
                }
                this.buffer = joined.slice(start);
                this.read(generation);
            } catch (error) {
                this.failed(`Media helper protocol failed: ${error.message}`);
            }
        });
    }

    failed(reason) {
        if (this.disposed) return;
        this.stop();
        this.onFailure(reason);
        this.retry = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 1500, () => {
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
        this.pending = null;
    }

    dispose() {
        this.disposed = true;
        this.stop();
    }
}

export class MediaBackend {
    constructor({ onChange = () => {} } = {}) {
        this.helper = GLib.getenv('MEDIA_CONTROL') || 'media-control';
        this.groups = [];
        this.activePlayer = '';
        this.modelStamp = '';
        this.failure = '';
        this.native = new NativeMedia({ onChange: () => { this.planner?.refresh(); onChange(); } });
        this.planner = new Planner({
            capture: () => this.native.capture(), send: request => this.worker.send(request),
            apply: operation => this.native.apply(operation), now: clock,
            onModel: (nextGroups, nextActive) => {
                this.groups = nextGroups;
                this.activePlayer = nextActive;
                this.modelStamp = ownershipStamp(this.native.capture());
                onChange();
            },
            onError: error => { this.failure = error; onChange(); },
        });
        this.planner.setReady(false);
        this.worker = new ModelWorker(this.helper, {
            onReady: () => { this.failure = ''; this.planner.setReady(true); },
            onResponse: response => { this.planner.receive(response); },
            onFailure: error => {
                this.planner.reset();
                this.failure = error;
                onChange();
            },
        });
    }

    start() {
        this.worker.start();
    }

    resolveGroups() {
        return this.groups.map(group => ({ ...group, primary: this.native.player(group.primary) }))
            .filter(group => group.primary !== null);
    }

    player(bus) { return this.native.player(bus); }

    volume(group) { return this.native.volume(group, this.modelStamp); }

    current() { return this.modelStamp === ownershipStamp(this.native.capture()); }

    refresh() { return this.planner.refresh(); }

    action(kind, bus = '', position = 0) { return this.planner.action(kind, bus, position); }

    select(bus) { return this.planner.select(bus); }

    cycle() { return this.planner.cycle(); }

    status() {
        this.planner.refresh();
        const group = this.resolveGroups().find(candidate => candidate.primary.bus === this.activePlayer);
        return JSON.stringify(badge(group));
    }

    closeNetwork() {
        try {
            const process = Gio.Subprocess.new([GLib.getenv('QUICKSHELL') || 'quickshell',
                'ipc', '--config', 'media', 'call', 'network', 'close'], Gio.SubprocessFlags.NONE);
            process.wait_check_async(null, (child, result) => {
                try { child.wait_check_finish(result); }
                catch (error) { console.warn(`Could not dismiss the network panel: ${error.message}`); }
            });
        } catch (error) { console.warn(`Could not dismiss the network panel: ${error.message}`); }
    }

    dispose() {
        this.worker.dispose();
        this.native.dispose();
    }
}
