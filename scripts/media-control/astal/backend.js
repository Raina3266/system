import GLib from 'gi://GLib';
import Gio from 'gi://Gio';
import Mpris from 'gi://AstalMpris';
import Wp from 'gi://AstalWp';
import { badge } from './logic.js';

// Planner

const MAX_PENDING = 8;
const MAX_ACTION_AGE = 5;
const KINDS = new Set(['toggle', 'next', 'previous', 'seek', 'pauseAll', 'togglePauseAll']);
const OPERATIONS = new Set(['play', 'pause', 'next', 'previous', 'seek']);
const PLAYER_TEXT = ['bus', 'identity', 'desktopEntry', 'title', 'artist', 'album', 'url', 'artUrl'];
const PLAYER_FLAGS = ['playing', 'stopped', 'canPlay', 'canPause', 'canSeek', 'canNext', 'canPrevious'];
const has = (object, key) => Object.prototype.hasOwnProperty.call(object, key);
const fail = message => { throw new Error(message); };
const message = error => error instanceof Error ? error.message : String(error);
const defaultNow = () => (globalThis.performance?.now() ?? Date.now()) / 1000;

function text(value, name) {
    if (typeof value !== 'string') fail(`${name} must be a string.`);
    return value;
}

function serial(value, name) {
    if (!(typeof value === 'string' && value.length)
        && !(Number.isSafeInteger(value) && value >= 0))
        fail(`${name} must be a stable string or nonnegative integer serial.`);
    return value;
}

// Detach native snapshots and sort object keys, so mutation/property insertion
// order cannot change an existing request or spuriously invalidate its stamp.
function json(value) {
    if (value === null || typeof value === 'string' || typeof value === 'boolean') return value;
    if (typeof value === 'number' && Number.isFinite(value)) return value;
    if (Array.isArray(value)) return value.map(json);
    if (value && Object.getPrototypeOf(value) === Object.prototype) {
        return Object.fromEntries(Object.keys(value).sort().map(key => [key, json(value[key])]));
    }
    fail('Snapshot properties and browserHint must contain only finite JSON values.');
}

function snapshot(raw) {
    if (!raw || !Array.isArray(raw.players) || !Array.isArray(raw.streams))
        fail('capture() must return {players: [], streams: []}.');
    const buses = new Set();
    const players = raw.players.map(player => {
        const copy = Object.fromEntries(PLAYER_TEXT.map(key => [key, text(player[key], `Player ${key}`)]));
        if (!copy.bus || buses.has(copy.bus)) fail('Player buses must be nonempty and unique.');
        buses.add(copy.bus);
        for (const key of PLAYER_FLAGS) {
            if (typeof player[key] !== 'boolean') fail(`Player ${key} must be a boolean.`);
            copy[key] = player[key];
        }
        copy.browserHint = json(player.browserHint ?? null);
        copy.length = Number.isFinite(player.length) && player.length > 0 ? player.length : null;
        copy.instance = serial(player.instance, 'Player instance');
        copy.trackId = text(player.trackId, 'Player trackId');
        if (!Number.isSafeInteger(player.seekRevision) || player.seekRevision < 0)
            fail('Player seekRevision must be a nonnegative integer.');
        copy.seekRevision = player.seekRevision;
        if (typeof player.positionSupported !== 'boolean') fail('Player positionSupported must be a boolean.');
        copy.positionSupported = player.positionSupported;
        if (!Number.isFinite(player.rate)) fail('Player rate must be finite.');
        copy.rate = player.rate;
        copy.position = player.positionSupported ? player.position : 0;
        if (!Number.isFinite(copy.position) || copy.position < 0)
            fail('Supported player positions must be finite and nonnegative.');
        return copy;
    });
    const ids = new Set();
    const streams = raw.streams.map(stream => {
        if (!Number.isInteger(stream.id) || stream.id < 0 || stream.id > 0xffffffff)
            fail('Stream id must be an unsigned 32-bit integer.');
        if (stream.id !== 0 && ids.has(stream.id)) fail('Bound stream IDs must be unique.');
        ids.add(stream.id);
        if (typeof stream.ready !== 'boolean') fail('Stream ready must be a boolean.');
        const properties = json(stream.properties);
        if (!properties || Array.isArray(properties) || typeof properties !== 'object')
            fail('Stream properties must be a JSON object.');
        return {
            id: stream.id, name: text(stream.name, 'Stream name'),
            nickname: text(stream.nickname, 'Stream nickname'), ready: stream.ready,
            properties, instance: serial(stream.instance, 'Stream instance'),
        };
    });
    return { players, streams };
}

function stamp(state) {
    return JSON.stringify({
        players: state.players.map(({ position, ...player }) => player),
        streams: state.streams,
    });
}

function protocol(state) {
    return {
        players: state.players.map(({ instance, trackId, seekRevision, positionSupported, rate, ...player }) => player),
        streams: state.streams.map(({ instance, ...stream }) => stream),
    };
}

function freeze(value) {
    if (value && typeof value === 'object') {
        Object.values(value).forEach(freeze);
        Object.freeze(value);
    }
    return value;
}

/**
 * Transport/safety bridge to the Rust model; never groups players or routes audio.
 *
 * capture() returns protocol players/streams plus player {trackId, instance,
 * seekRevision, positionSupported, rate} and stream {instance}. Instance serials
 * must change on native object replacement, including reuse of a bus/stream ID.
 * Extra fields (including volume) are ignored. Unknown positions are sent as 0;
 * invalid/unknown lengths as null. The extra bridge fields never go to Rust.
 *
 * send(requestObject) synchronously dispatches ONE JSON request; the backend
 * serializes it and calls receive(parsedResponse). A throw/false means send failure.
 * apply({player,operation,value?}) is
 * synchronous DISPATCH, may throw/return false/{error}, and MUST advance that
 * player's seekRevision exactly once after dispatching a seek. It does not
 * confirm remote completion or modify the sampled native position. Promises
 * from send/apply are unsupported. onModel(groups, activePlayer) receives frozen
 * protocol groups (primary/players are bus strings); the backend resolves native objects.
 * onError(string) receives actionable errors; dispatched actions/reset clear them.
 * Model/error callbacks should not throw. now() supplies monotonic seconds.
 *
 * Starts ready; refresh() is explicit. Draining is deferred to a microtask, so
 * callbacks may safely call refresh()/action() without recursive dispatch.
 * There is one in-flight request, eight queued actions, one coalesced refresh,
 * and a five-second maximum action age (enqueue through response). Only adjacent
 * queued seeks to the same bus coalesce; intervening actions remain barriers.
 * Every action is guarded at enqueue, send, response, and each native operation.
 * Position/volume changes do not invalidate it; identity, playback, capabilities,
 * track/instance/seekRevision, selection revision, and all stream ownership fields do.
 * Only the single trailing same-target seek can be rebased after our own exact
 * synchronous seek dispatch revisions. Any other change cancels it; no blind retries.
 * Failed/interrupted native apply discards the remaining queued actions.
 *
 * refresh/action/select/cycle return whether accepted; receive returns whether
 * a fresh, well-formed response was processed (including a model policy error).
 * reset()/setReady(false) disable dispatch, discard pending work/model/selection,
 * and clear errors. IDs are never reused. setReady(true) requests a fresh model.
 * send failure also resets/disables and reports an error. The backend must reset on
 * worker EOF, malformed JSON, or timeout; Planner itself owns no timers/process I/O.
 */
export class Planner {
    constructor({ capture, send, apply, onModel = () => {}, onError = () => {}, now = defaultNow }) {
        for (const callback of [capture, send, apply, onModel, onError, now]) {
            if (typeof callback !== 'function') throw new TypeError('Planner callbacks must be functions.');
        }
        this._capture = capture;
        this._send = send;
        this._apply = apply;
        this._onModel = onModel;
        this._onError = onError;
        this._now = now;
        this._ready = true;
        this._groups = Object.freeze([]);
        this._activePlayer = '';
        this._selectedPlayer = '';
        this._selectionRevision = 0;
        this._error = '';
        this._queue = [];
        this._inFlight = null;
        this._refreshPending = false;
        this._scheduled = false;
        this._processing = false;
        this._generation = 0;
        this._nextId = 1;
    }

    get ready() { return this._ready; }
    get groups() { return this._groups; }
    get activePlayer() { return this._activePlayer; }
    get selectedPlayer() { return this._selectedPlayer; }
    get error() { return this._error; }

    _report(error) {
        if (error !== this._error) {
            this._error = error;
            this._onError(error);
        }
    }

    _time() {
        const now = this._now();
        if (!Number.isFinite(now)) fail('Planner now() must return finite monotonic seconds.');
        return now;
    }

    _guard(state) {
        return { stamp: stamp(state), selected: this.selectedPlayer, revision: this._selectionRevision };
    }

    _stale(entry, state, now) {
        if (entry.guard.selected !== this.selectedPlayer || entry.guard.revision !== this._selectionRevision)
            return 'selection changed';
        if (entry.guard.stamp !== stamp(state))
            return 'player identity, playback, capabilities, seek dispatch, or stream ownership changed';
        if (entry.action && (now < entry.created || now - entry.created > MAX_ACTION_AGE))
            return 'action expired (maximum age is 5 seconds)';
        return '';
    }

    _schedule() {
        if (!this.ready || this._scheduled) return;
        this._scheduled = true;
        const generation = this._generation;
        Promise.resolve().then(() => {
            if (generation !== this._generation) return;
            this._scheduled = false;
            this._pump();
        });
    }

    refresh() {
        if (!this.ready) return false;
        this._refreshPending = true;
        this._schedule();
        return true;
    }

    action(kind, bus = '', position = 0) {
        if (!this.ready) {
            this._report('Media worker is not ready; action was not queued.');
            return false;
        }
        if (!KINDS.has(kind) || typeof bus !== 'string'
            || !Number.isFinite(position) || (kind === 'seek' && position < 0)) {
            this._report('Invalid media action: use toggle, next, previous, seek, pauseAll, or togglePauseAll with finite seconds.');
            return false;
        }
        try {
            const state = snapshot(this._capture());
            const entry = {
                action: { kind, player: bus, position }, guard: this._guard(state), created: this._time(),
            };
            const tail = this._queue.at(-1);
            if (kind === 'seek' && tail?.action.kind === 'seek' && tail.action.player === bus) {
                // This is a new user intent, not a revalidation of the old one.
                this._queue[this._queue.length - 1] = entry;
            } else {
                if (this._queue.length >= MAX_PENDING) {
                    this._report('Media action queue is full (8 pending); action was not queued.');
                    return false;
                }
                this._queue.push(entry);
            }
            this._schedule();
            return true;
        } catch (error) {
            this._report(`Cannot capture media action: ${message(error)}`);
            return false;
        }
    }

    select(bus) {
        if (!this.ready || typeof bus !== 'string'
            || !this.groups.some(group => group.players.includes(bus))) {
            this._report('The requested media source is no longer available.');
            return false;
        }
        if (bus !== this.selectedPlayer) {
            this._selectedPlayer = bus;
            this._selectionRevision++;
        }
        return this.refresh();
    }

    cycle() {
        if (!this.ready || !this.groups.length) return false;
        let index = this.groups.findIndex(group => group.players.includes(this.selectedPlayer));
        if (index < 0) index = this.groups.findIndex(group => group.primary === this.activePlayer);
        return this.select(this.groups[(index + 1) % this.groups.length].primary);
    }

    _pump() {
        if (!this.ready || this._inFlight || this._processing) return;
        const generation = this._generation;
        let entry;
        let state;
        try {
            while (this._queue.length) {
                entry = this._queue.shift();
                state = snapshot(this._capture());
                const reason = this._stale(entry, state, this._time());
                if (!reason) break;
                this._report(`Discarded stale queued ${entry.action.kind}: ${reason}; action was not retried.`);
                if (generation !== this._generation) return;
                this._refreshPending = true;
                entry = null;
            }
            if (!entry) {
                if (!this._refreshPending) return;
                state = snapshot(this._capture());
                entry = { action: null, guard: this._guard(state), created: this._time() };
            }
        } catch (error) {
            this._refreshPending = false;
            this._report(`Cannot capture media snapshot: ${message(error)}; action was not retried.`);
            if (this._queue.length) this._schedule();
            return;
        }
        this._refreshPending = false;
        entry.guard = this._guard(state);
        const request = freeze({
            requestId: this._nextId++, selectedPlayer: this.selectedPlayer, ...protocol(state),
            ...(entry.action ? { action: { ...entry.action } } : {}),
        });
        entry.requestId = request.requestId;
        this._inFlight = entry;
        try {
            const result = this._send(request);
            if (result === false) fail('send() returned false');
            if (result && typeof result.then === 'function') {
                Promise.resolve(result).catch(() => {});
                fail('send() must dispatch synchronously, not return a Promise');
            }
        } catch (error) {
            this.reset();
            this._report(`Media worker send failed: ${message(error)}; actions were discarded, not retried.`);
        }
    }

    _validate(response, entry, state) {
        if (!Array.isArray(response.groups) || !Array.isArray(response.actions))
            fail('Model response must contain groups and actions arrays.');
        if (has(response, 'error') && typeof response.error !== 'string') fail('Model error must be a string.');
        const byBus = new Map(state.players.map(player => [player.bus, player]));
        const used = new Set();
        const groups = response.groups.map(group => {
            if (!group || !Array.isArray(group.players) || !group.players.length)
                fail('Model group must contain native player buses.');
            const primary = text(group.primary, 'Group primary');
            const players = group.players.map(bus => {
                if (!byBus.has(bus) || used.has(bus)) fail('Model group contains an unknown or duplicate native player.');
                used.add(bus);
                return bus;
            });
            if (!players.includes(primary)) fail('Model group primary must be a member.');
            if (group.streamId !== null && !state.streams.some(stream =>
                stream.id !== 0 && stream.id === group.streamId && stream.ready))
                fail('Model group references an unavailable audio stream.');
            return {
                title: text(group.title, 'Group title'), artist: text(group.artist, 'Group artist'),
                primary, players, displayName: text(group.displayName, 'Group displayName'),
                artUrl: text(group.artUrl, 'Group artUrl'), streamId: group.streamId,
                streamReason: text(group.streamReason, 'Group streamReason'),
            };
        });
        const active = text(response.activePlayer, 'Model activePlayer');
        if (groups.length ? !groups.some(group => group.primary === active) : active !== '')
            fail('Model activePlayer must be a current group primary.');
        const selected = groups.find(group => group.players.includes(entry.guard.selected));
        if (selected && selected.primary !== active) fail('Model activePlayer does not honor the current selection.');
        if ((!entry.action || response.error) && response.actions.length)
            fail('Model returned native actions for a refresh or failed request.');

        const intent = entry.action;
        const target = intent && groups.find(group => intent.player
            ? group.players.includes(intent.player) : group.primary === active);
        if (intent && !['pauseAll', 'togglePauseAll'].includes(intent.kind) && !target && !response.error)
            fail('Model action has no current target group.');
        const seen = new Set();
        const operations = response.actions.map(operation => {
            if (!operation || Object.keys(operation).some(key => !['player', 'operation', 'value'].includes(key))
                || !OPERATIONS.has(operation.operation))
                fail('Model returned an invalid native operation (routing is not supported).');
            const player = byBus.get(operation.player);
            if (!player || seen.has(player.bus)) fail('Model operation targets an unknown or duplicate native player.');
            seen.add(player.bus);
            const op = operation.operation;
            if (op !== 'seek' && has(operation, 'value')) fail('Only a seek may carry a native value.');
            switch (intent?.kind) {
            case 'toggle': {
                const expected = target.players.some(bus => byBus.get(bus).playing) ? 'pause' : 'play';
                if (op !== expected || !target.players.includes(player.bus)) fail('Native toggle plan does not match its group.');
                break;
            }
            case 'next':
            case 'previous':
                if (op !== intent.kind || player.bus !== target.primary
                    || !player[op === 'next' ? 'canNext' : 'canPrevious'])
                    fail('Native track operation lacks the exact target/capability.');
                break;
            case 'seek':
                if (op !== 'seek' || !target.players.includes(player.bus) || !player.canSeek
                    || !Number.isFinite(operation.value) || operation.value < 0 || operation.value !== intent.position)
                    fail('Native seek plan lacks the exact target, capability, or requested value.');
                break;
            case 'pauseAll':
                if (op !== 'pause' || !player.playing) fail('Native pause-all plan targets a nonplaying player or wrong operation.');
                break;
            case 'togglePauseAll':
                if (op === 'pause' && player.playing) break;
                if (op === 'play' && groups.some(group => group.primary === active && group.players.includes(player.bus))) break;
                fail('Native toggle-pause-all plan has an invalid target/operation.');
                break;
            default:
                fail('Unrequested native operation.');
            }
            return op === 'seek'
                ? { player: player.bus, operation: op, value: operation.value }
                : { player: player.bus, operation: op };
        });
        if (intent?.kind === 'togglePauseAll' && new Set(operations.map(op => op.operation)).size > 1)
            fail('Native toggle-pause-all plan mixes play and pause.');
        return { groups: freeze(groups), active, operations, error: response.error || '' };
    }

    receive(response) {
        const entry = this._inFlight;
        if (!this.ready || this._processing || !entry || response?.requestId !== entry.requestId) return false;
        this._inFlight = null;
        this._processing = true;
        const generation = this._generation;
        let processed = false;
        let applying = false;
        try {
            let state = snapshot(this._capture());
            const reason = this._stale(entry, state, this._time());
            if (reason && !entry.action) {
                // Discovery races with native startup/metadata notifications.
                // Recompute read-only models; only user mutations need an error.
                this._refreshPending = true;
                return false;
            }
            if (reason) fail(`Stale ${entry.action.kind} response: ${reason}; action was not retried.`);
            const model = this._validate(response, entry, state);
            const originalGuard = entry.guard;
            let guard = originalGuard;
            for (const operation of model.operations) {
                const current = snapshot(this._capture());
                const changed = this._stale({ ...entry, guard }, current, this._time());
                if (changed) fail(`Native plan interrupted: ${changed}; remaining operations were not retried.`);
                applying = true;
                const result = this._apply(freeze(operation));
                if (generation !== this._generation) return false;
                if (result && typeof result.then === 'function') {
                    Promise.resolve(result).catch(() => {});
                    fail('apply() must be synchronous; native operations were not retried.');
                }
                if (result === false || result?.error) fail(`Native ${operation.operation} failed: ${result?.error || 'apply() returned false'}.`);
                const expected = operation.operation === 'seek' ? {
                    ...current,
                    players: current.players.map(player => player.bus === operation.player
                        ? { ...player, seekRevision: player.seekRevision + 1 } : player),
                } : current;
                state = snapshot(this._capture());
                guard = { ...guard, stamp: stamp(expected) };
                const after = this._stale({ ...entry, guard }, state, this._time());
                if (after) fail(`Native plan changed during apply (${after}); remaining actions were not retried. Seek apply must advance only its own seek dispatch revision once.`);
            }
            // Never bless an external change, a different target, or a queue
            // separated by another action as the result of our own seek.
            const queued = this._queue.length === 1 ? this._queue[0] : null;
            if (entry.action?.kind === 'seek' && model.operations.length && queued?.action.kind === 'seek'
                && queued.action.player === entry.action.player
                && queued.guard.stamp === originalGuard.stamp
                && queued.guard.selected === originalGuard.selected
                && queued.guard.revision === originalGuard.revision) {
                queued.guard = { ...guard };
            }
            this._groups = model.groups;
            this._activePlayer = model.active;
            if (!model.groups.some(group => group.players.includes(this.selectedPlayer))
                && this.selectedPlayer !== model.active) {
                this._selectedPlayer = model.active;
                this._selectionRevision++;
            }
            if (model.error) this._report(model.error);
            else if (entry.action) this._report('');
            if (generation !== this._generation) return false;
            this._onModel(this.groups, this.activePlayer);
            processed = true;
        } catch (error) {
            if (generation === this._generation) {
                this._refreshPending = true;
                if (applying) this._queue = [];
                this._report(`Media plan rejected: ${message(error)}${applying ? '; pending actions were discarded, not retried.' : ''}`);
            }
        } finally {
            this._processing = false;
            if (this.ready && (this._queue.length || this._refreshPending)) this._schedule();
        }
        return processed;
    }

    reset() {
        this._generation++;
        this._ready = false;
        this._queue = [];
        this._inFlight = null;
        this._refreshPending = false;
        this._scheduled = false;
        this._groups = Object.freeze([]);
        this._activePlayer = '';
        this._selectedPlayer = '';
        this._selectionRevision++;
        this._report('');
        this._onModel(this.groups, this.activePlayer);
    }

    setReady(ready) {
        if (!ready) this.reset();
        else if (!this.ready) {
            this._ready = true;
            this.refresh();
        }
    }
}


export const clock = () => GLib.get_monotonic_time() / 1e6;
const bounded = value => Math.min(1, Math.max(0, value));
const PROPERTIES = [
    'application.name', 'application.id', 'application.process.binary',
    'media.title', 'media.filename', 'media.url', 'node.nick',
];

function meta(player, key) {
    const value = player.get_meta(key);
    return value ? value.deep_unpack() : null;
}

function validTrackId(value) {
    return typeof value === 'string' && GLib.Variant.is_object_path(value)
        && value !== '/org/mpris/MediaPlayer2/TrackList/NoTrack';
}

function identity(player) {
    if (validTrackId(player.trackid)) return player.trackid;
    return JSON.stringify([player.title, player.artist, player.album, meta(player, 'xesam:url')]);
}

// Volume must use a fresh ownership decision, not a recycled native node ID.
function ownershipStamp(data) {
    return JSON.stringify({
        players: data.players.map(({ position, seekRevision, rate, positionSupported, cover, ...player }) => player),
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
            const record = { object: player, instance: ++this.serial, seekRevision: 0 };
            record.signal = player.connect('notify', () => this.changed());
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
        const positionSupported = Number.isFinite(player.position) && player.position >= 0;
        const rate = Number.isFinite(player.rate) ? player.rate : 1;
        return {
            bus, instance: record.instance, trackId: player.trackid || '', trackKey: identity(player),
            identity: player.identity || '', desktopEntry: player.entry || '',
            title: player.title || '', artist: player.artist || '', album: player.album || '',
            url: String(meta(player, 'xesam:url') || ''), artUrl: player.art_url || '',
            browserHint: meta(player, 'mprisence:browser'),
            playing: player.playback_status === Mpris.PlaybackStatus.PLAYING,
            stopped: player.playback_status === Mpris.PlaybackStatus.STOPPED,
            position: positionSupported ? player.position : 0, positionSupported,
            seekRevision: record.seekRevision, rate,
            length: player.length > 0 && Number.isFinite(player.length) ? player.length : null,
            canPlay: Boolean(player.can_play), canPause: Boolean(player.can_pause),
            canSeek: Boolean(player.can_seek),
            canNext: Boolean(player.can_go_next), canPrevious: Boolean(player.can_go_previous),
            cover: player.cover_art || '',
        };
    }


    capture() {
        return {
            players: [...this.players.keys()].map(bus => this.player(bus)),
            streams: [...this.streams.values()].map(({ object: stream, instance }) => {
                const properties = Object.fromEntries(PROPERTIES.map(key => [key, stream.get_pw_property(key) || '']));
                properties['media.name'] = stream.name || '';
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
            if (!player.can_seek || !validTrackId(player.trackid))
                throw new Error('Seeking is unavailable for this track.');
            if (!Number.isFinite(operation.value) || operation.value < 0)
                throw new Error('Seek position must be finite and nonnegative.');
            player.position = operation.value;
            // Astal dispatches asynchronously. This guards our dispatch only;
            // position/support continue to come exclusively from native state.
            record.seekRevision++;
            break;
        }
        default: throw new Error('Unsupported native media operation.');
        }
    }

    volume(group, stamp, allowed = () => true) {
        const { bus, instance } = group?.primary || {};
        const streamId = group?.streamId;
        const record = this.players.get(bus);
        const valid = () => Boolean(allowed() && record && this.players.get(bus) === record
            && record.instance === instance && record.object.available
            && ownershipStamp(this.capture()) === stamp);
        // mprisence's web bridge advertises a writable constant Volume=1, but
        // its browser extension has no volume command. Use an owned Astal audio
        // stream instead; never present that placeholder as working tab volume.
        const webBridge = bus?.startsWith('org.mpris.MediaPlayer2.mprisence_web.');
        const mprisVolume = () => !webBridge && record
            && Number.isFinite(record.object.volume) && record.object.volume >= 0;
        const stream = this.streams.get(streamId);
        const streamReady = () => Boolean(stream && stream.object.id > 0
            && Number.isFinite(stream.object.volume) && stream.object.volume >= 0);
        const streamValid = () => valid() && this.streams.get(streamId) === stream && streamReady();
        const setMuted = muted => {
            if (typeof muted !== 'boolean' || !streamValid()) return false;
            stream.object.mute = muted;
            return true;
        };
        // MPRIS volume and the Rust-owned stream's native mute are independent.
        const muteEnabled = streamValid();
        const mute = {
            muteEnabled, muted: muteEnabled ? Boolean(stream.object.mute) : false,
            muteReason: muteEnabled ? 'Application stream mute' : !valid()
                ? 'Media source or audio stream ownership is no longer current.'
                : 'Native mute requires a ready, uniquely owned application audio stream.'
                    + (group?.streamReason ? ` ${group.streamReason}` : ''),
            setMuted, toggleMute: () => streamValid() && setMuted(!stream.object.mute),
        };
        if (mprisVolume()) {
            return {
                enabled: valid(), value: bounded(record.object.volume),
                key: `player:${record.instance}`, reason: 'Media volume (0–100%)',
                set: value => {
                    if (!Number.isFinite(value) || !valid() || !mprisVolume()) return false;
                    record.object.volume = bounded(value);
                    return true;
                },
                ...mute,
            };
        }
        if (streamReady()) {
            return {
                enabled: streamValid(), value: bounded(stream.object.volume),
                key: `stream:${stream.instance}`, reason: 'Application stream volume (0–100%)',
                set: value => {
                    if (!Number.isFinite(value) || !streamValid() || mprisVolume()) return false;
                    stream.object.volume = bounded(value);
                    if (value > 0) stream.object.mute = false;
                    return true;
                },
                ...mute,
            };
        }
        return {
            enabled: false, value: 0, key: '',
            reason: (webBridge ? 'This browser bridge does not support tab volume. ' : '')
                + (group?.streamReason || 'Independent media volume is unavailable.'),
            set: () => false, ...mute,
        };
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
            this.child.wait_async(null, (child, result) => {
                try { child.wait_finish(result); }
                catch (error) { console.warn(`Could not wait for media helper: ${error.message}`); }
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
        this.disposed = false;
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
            onError: error => {
                if (error) console.warn(`Media action: ${error}`);
                this.failure = error;
                onChange();
            },
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

    transport(group) {
        const primary = group?.primary;
        const model = this.groups.find(model => model.primary === primary?.bus);
        if (this.disposed || !this.planner.ready || !model || !this.current()
            || !this.native.players.get(primary.bus)?.object.available
            || this.player(primary.bus)?.instance !== primary.instance)
            return { enabled: false, reason: 'Media source changed or is not ready. Refresh and try again.' };
        const members = model.players.map(bus => this.player(bus));
        if (members.some(player => !player || !this.native.players.get(player.bus)?.object.available))
            return { enabled: false, reason: 'A media group member is unavailable. Refresh and try again.' };
        const playing = members.some(player => player.playing);
        const supported = members.some(player => playing ? player.canPause : player.canPlay);
        const operation = playing ? 'pause' : 'play';
        return {
            enabled: supported,
            reason: supported ? (playing ? 'Pause' : 'Play')
                : primary.bus.startsWith('org.mpris.MediaPlayer2.mprisence_web.')
                    ? `The browser bridge does not expose ${operation} control for this source. Use the browser's playback controls.`
                    : `This media source does not advertise ${operation} support.`,
        };
    }

    /** Fresh descriptor; setters report guarded dispatch, not remote completion. */
    volume(group) {
        const bus = group?.primary?.bus;
        const streamId = group?.streamId;
        const allowed = () => !this.disposed && this.planner.ready
            && this.groups.some(model => model.primary === bus && model.streamId === streamId);
        return this.native.volume(group, this.modelStamp, allowed);
    }

    _checkLookup(cancellable) {
        if (this.disposed) throw new Error('The media backend was disposed.');
        if (cancellable.is_cancelled()) throw new Error('Media lookup cancelled.');
    }

    /** Local-track fallback only. Advertised artwork uses the native snapshot's cover. */
    async resolveArtwork(trackUrl, cancellable = null) {
        cancellable ??= new Gio.Cancellable();
        this._checkLookup(cancellable);
        if (typeof trackUrl !== 'string' || !trackUrl.length || trackUrl.includes('\0'))
            throw new TypeError('Artwork requires a track URL or absolute path.');
        const output = await runHelper([this.helper, 'artwork', trackUrl], null, cancellable);
        this._checkLookup(cancellable);
        const path = output.replace(/\r?\n$/, '');
        if (!path.startsWith('/') || /[\x00-\x1f\x7f]/.test(path))
            throw new Error('Media artwork helper returned an invalid absolute path.');
        return path;
    }

    /** The view owns cancellation; Rust owns credit lookup and fallback policy. */
    async resolveCredit(metadata, cancellable = null) {
        cancellable ??= new Gio.Cancellable();
        this._checkLookup(cancellable);
        const request = Object.fromEntries(['url', 'artist', 'album'].map(key =>
            [key, text(metadata?.[key], `Credit ${key}`)]));
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

    current() { return this.modelStamp === ownershipStamp(this.native.capture()); }

    refresh() { return this.planner.refresh(); }

    // Recovery refreshes preserve view errors; a successful action clears them.
    reportFailure(error) { this.planner._report(error); }

    action(kind, bus = '', position = 0) { return this.planner.action(kind, bus, position); }

    select(bus) { return this.planner.select(bus); }

    cycle() { return this.planner.cycle(); }

    status() {
        this.planner.refresh();
        const group = this.resolveGroups().find(candidate => candidate.primary.bus === this.activePlayer);
        return JSON.stringify(badge(group));
    }

    dispose() {
        if (this.disposed) return;
        this.disposed = true;
        this.worker.dispose();
        this.native.dispose();
    }
}
