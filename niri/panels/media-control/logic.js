// Planner

const MAX_PENDING = 8;
const MAX_ACTION_AGE = 5;
const KINDS = new Set(['toggle', 'next', 'previous', 'seek', 'pauseAll', 'togglePauseAll']);
const OPERATIONS = new Set(['play', 'pause', 'next', 'previous', 'seek']);
const PLAYER_TEXT = ['bus', 'identity', 'desktopEntry', 'title', 'artist', 'album', 'url', 'artUrl'];
const PLAYER_FLAGS = ['playing', 'stopped', 'canSeek', 'canNext', 'canPrevious'];
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
        if (!Number.isSafeInteger(player.timelineEpoch) || player.timelineEpoch < 0)
            fail('Player timelineEpoch must be a nonnegative integer.');
        copy.timelineEpoch = player.timelineEpoch;
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
        players: state.players.map(({ instance, trackId, timelineEpoch, positionSupported, rate, ...player }) => player),
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
 * timelineEpoch, positionSupported, rate} and stream {instance}. Instance serials
 * must change on native object replacement, including reuse of a bus/stream ID.
 * Extra fields (including volume) are ignored. Unknown positions are sent as 0;
 * invalid/unknown lengths as null. The extra bridge fields never go to Rust.
 *
 * send(requestObject) synchronously dispatches ONE JSON request; the backend
 * serializes it and calls receive(parsedResponse). A throw/false means send failure.
 * apply({player,operation,value?}) is
 * synchronous, may throw/return false/{error}, and MUST call Timeline.seek() for
 * a seek, advancing that player's epoch exactly once before returning. Promises
 * from send/apply are unsupported. onModel(groups, activePlayer) receives frozen
 * protocol groups (primary/players are bus strings); the backend resolves native objects.
 * onError(string) receives actionable errors; successful actions/reset clear them.
 * Model/error callbacks should not throw. now() supplies monotonic seconds.
 *
 * Starts ready; refresh() is explicit. Draining is deferred to a microtask, so
 * callbacks may safely call refresh()/action() without recursive dispatch.
 * There is one in-flight request, eight queued actions, one coalesced refresh,
 * and a five-second maximum action age (enqueue through response). Only adjacent
 * queued seeks to the same bus coalesce; intervening actions remain barriers.
 * Every action is guarded at enqueue, send, response, and each native operation.
 * Position/volume changes do not invalidate it; identity, playback, capabilities,
 * track/instance/epoch, selection revision, and all stream ownership fields do.
 * Only the single trailing same-target seek can be rebased after our own exact
 * synchronous seek epoch changes. Any other change cancels it; no blind retries.
 * Failed/interrupted native apply discards the remaining queued actions.
 *
 * refresh/action/select/cycle return whether accepted; receive returns whether
 * a fresh, well-formed response was processed (including a model policy error).
 * reset()/setReady(false) disable dispatch, discard pending work/model/selection,
 * and clear errors. IDs are never reused. setReady(true) requests a fresh model.
 * send failure also resets/disables and reports an error. The backend must reset on
 * worker EOF, malformed JSON, or timeout; this module has no timers/process I/O.
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
            return 'player identity, playback, capabilities, timeline, or stream ownership changed';
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
                if (op !== 'seek' || !target.players.includes(player.bus) || !player.canSeek || !player.positionSupported
                    || !Number.isFinite(operation.value) || operation.value < 0 || operation.value !== intent.position)
                    fail('Native seek plan lacks the exact target, supported position, capability, or requested value.');
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
            if (reason) fail(`Stale ${entry.action?.kind ?? 'model'} response: ${reason}; action was not retried.`);
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
                        ? { ...player, timelineEpoch: player.timelineEpoch + 1 } : player),
                } : current;
                state = snapshot(this._capture());
                guard = { ...guard, stamp: stamp(expected) };
                const after = this._stale({ ...entry, guard }, state, this._time());
                if (after) fail(`Native plan changed during apply (${after}); remaining actions were not retried. Seek apply must advance only its own timeline epoch once.`);
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

// Timeline

const SAMPLE_TOLERANCE = 1.25;
const SEEK_GRACE_SECONDS = 1.5;
const validPosition = value => Number.isFinite(value) && value >= 0;
const validLength = value => Number.isFinite(value) && value > 0 ? value : null;
const validRate = value => Number.isFinite(value) ? value : 1;

function time(seconds) {
    if (!Number.isFinite(seconds))
        throw new TypeError('Timeline time must be finite monotonic seconds.');
    return seconds;
}

/**
 * A sampled native position, not a poller. All positions/times are seconds.
 *
 * update(fields, now): omitted fields are retained; sampled defaults to true.
 * Pass sampled:false for notifications other than a genuine position sample.
 * identity is a stable track key (track ID plus a metadata fallback), not the
 * application's Identity. A changed identity without a sample becomes unknown.
 * Negative/nonfinite positions, including Astal's -1, are unsupported; value()
 * returns 0 for them. Nonpositive/invalid length means unknown; invalid rate is 1.
 * Finite rates, including zero and reverse playback, are honored while playing.
 *
 * The first update establishes epoch 0. Later identity, playing, rate, support,
 * or position discontinuities advance it once per update. Samples within 1.25
 * seconds of playback (scaled by absolute rates above 1x) do not advance it;
 * repeated samples inside that tolerance preserve the anchor. Sub-tolerance
 * external seeks cannot be distinguished from
 * native sampling jitter. seek() always advances it on success.
 *
 * update()/seek() return {position, supported, epoch, seek, error}. seek is null,
 * 'pending', 'acknowledged', 'timeout', or 'cancelled'; error is null or a string.
 * A successful seek is optimistic. For 1.5s, recognizable pre-seek samples are
 * ignored, including an advancing old sample. A different external position
 * cancels it immediately. After the grace period, the next genuine sample is
 * authoritative; timeout is reported if it does not acknowledge the seek.
 * No sample means no timeout-induced rollback. value() never mutates state.
 */
export class Timeline {
    constructor() {
        this._initialized = false;
        this._identity = undefined;
        this._playing = false;
        this._rate = 1;
        this._length = null;
        this._supported = false;
        this._position = 0;
        this._time = 0;
        this._epoch = 0;
        this._lastSample = null;
        this._pending = null;
    }

    get supported() { return this._supported; }
    get epoch() { return this._epoch; }

    _clamp(position) {
        return Math.max(0, this._length === null ? position : Math.min(position, this._length));
    }

    _project(anchor, now) {
        return anchor.position + (anchor.playing ? Math.max(0, now - anchor.time) * anchor.rate : 0);
    }

    _near(left, right) {
        return Math.abs(left - right) <= SAMPLE_TOLERANCE * Math.max(1, Math.abs(this._rate));
    }

    _result(now, seek = this._pending ? 'pending' : null, error = null) {
        return { position: this.value(now), supported: this.supported, epoch: this.epoch, seek, error };
    }

    value(nowSeconds) {
        const now = time(nowSeconds);
        if (!this._supported)
            return 0;
        return this._clamp(this._project({
            position: this._position, time: this._time, playing: this._playing, rate: this._rate,
        }, now));
    }

    update(fields, nowSeconds) {
        const now = time(nowSeconds);
        const position = this.value(now);
        const identity = has(fields, 'identity') ? fields.identity : this._identity;
        const playing = has(fields, 'playing') ? Boolean(fields.playing) : this._playing;
        const rate = has(fields, 'rate') ? validRate(fields.rate) : this._rate;
        const changedIdentity = this._initialized && identity !== this._identity;
        let discontinuity = this._initialized
            && (changedIdentity || playing !== this._playing || rate !== this._rate);
        let seek = this._pending ? 'pending' : null;
        let error = null;

        // Preserve both trajectories through a playback/rate notification. Its
        // position property is often the cached value from before our seek.
        if (this._pending) {
            for (const old of this._pending.old) {
                old.position = this._project(old, now);
                old.time = now;
                old.playing = playing;
                old.rate = rate;
            }
        }
        this._position = position;
        this._time = now;
        this._identity = identity;
        this._playing = playing;
        this._rate = rate;
        if (has(fields, 'length'))
            this._length = validLength(fields.length);
        else if (changedIdentity)
            this._length = null;

        if (changedIdentity) {
            if (this._pending) {
                seek = 'cancelled';
                error = 'Seek cancelled because the media identity changed.';
            }
            this._pending = null;
            this._lastSample = null;
            this._supported = false;
            this._position = 0;
        }

        const sampled = fields.sampled !== false && has(fields, 'position');
        if (sampled) {
            const sample = fields.position;
            if (!validPosition(sample)) {
                discontinuity ||= this._initialized && this._supported;
                if (this._pending) {
                    seek = 'cancelled';
                    error = 'Seek cancelled because native position is unsupported.';
                }
                this._pending = null;
                this._lastSample = null;
                this._supported = false;
                this._position = 0;
            } else {
                let accept = true;
                let acknowledged = false;
                let authoritative = false;
                if (this._pending) {
                    if (this._near(this._clamp(sample), this.value(now))) {
                        seek = 'acknowledged';
                        acknowledged = true;
                        this._pending = null;
                    } else {
                        const oldSample = this._pending.old.some(old =>
                            sample === old.sample || this._near(sample, this._project(old, now)));
                        if (now < this._pending.deadline && oldSample) {
                            accept = false;
                        } else {
                            authoritative = true;
                            seek = now >= this._pending.deadline ? 'timeout' : 'cancelled';
                            error = seek === 'timeout'
                                ? 'Seek was not acknowledged within 1.5 seconds.'
                                : 'Seek superseded by an external position change.';
                            this._pending = null;
                        }
                    }
                }
                if (accept) {
                    const repeated = !acknowledged && !authoritative
                        && this._lastSample === sample && this._supported
                        && this._near(this._clamp(sample), this.value(now));
                    if (!acknowledged && !repeated && this._initialized) {
                        discontinuity ||= !this._supported
                            || !this._near(this._clamp(sample), this.value(now));
                    }
                    if (!repeated || changedIdentity)
                        this._position = this._clamp(sample);
                    this._supported = true;
                    this._lastSample = sample;
                }
            }
        }
        if (discontinuity)
            this._epoch++;
        this._initialized = true;
        return this._result(now, seek, error);
    }

    seek(position, nowSeconds) {
        const now = time(nowSeconds);
        if (!validPosition(position))
            return this._result(now, undefined, 'Seek position must be finite and nonnegative.');
        if (!this.supported)
            return this._result(now, undefined, 'Native position is unsupported; seeking is unavailable.');

        const old = [...(this._pending?.old ?? []), {
            position: this.value(now), time: now, playing: this._playing,
            rate: this._rate, sample: this._lastSample,
        }];
        this._position = this._clamp(position);
        this._time = now;
        this._epoch++;
        // Retain a few earlier optimistic targets during a rapid seek gesture.
        this._pending = { deadline: now + SEEK_GRACE_SECONDS, old: old.slice(-4) };
        return this._result(now);
    }
}

// Status formatting

const ICON = '\u{f040a}';

function isCjkIdeograph(code) {
    return (code >= 0x3400 && code <= 0x4dbf)
        || (code >= 0x4e00 && code <= 0x9fff)
        || (code >= 0xf900 && code <= 0xfaff)
        || (code >= 0x20000 && code <= 0x2ebef)
        || (code >= 0x30000 && code <= 0x3134f);
}

/** Original badge limits: 50 Unicode scalars, at most 35 CJK ideographs.
 * Latin/kana may follow the 35th ideograph until the scalar limit is reached.
 * Ellipsis is added only when a character would actually exceed a limit.
 */
export function truncateTitle(title) {
    let output = '';
    let characters = 0;
    let ideographs = 0;
    for (const character of title ?? '') {
        const cjk = isCjkIdeograph(character.codePointAt(0));
        if (characters === 50 || (cjk && ideographs === 35))
            return output + '…';
        output += character;
        characters++;
        if (cjk) ideographs++;
    }
    return output;
}

/** group.primary is an Astal snapshot OBJECT, not the Rust model's bus string.
 * Main resolves that bus before calling. Only {title, artist, identity, playing}
 * are read; no native/UI objects are required. Text and full tooltip are plain
 * unescaped strings: the eventual UI must not interpret them as markup.
 */
export function badge(group) {
    const player = group?.primary;
    if (!player || typeof player !== 'object')
        return { text: ICON, class: 'inactive', alt: 'inactive', tooltip: 'No media playing' };
    const title = (player.title || '').trim() || (player.identity || '').trim() || 'Unknown title';
    const artist = (player.artist || '').trim();
    const state = player.playing ? 'playing' : 'paused';
    return {
        text: ICON + '  ' + truncateTitle(title), class: state, alt: state,
        tooltip: title + (artist ? ' — ' + artist : ''),
    };
}

/** Floor finite nonnegative seconds to m:ss or h:mm:ss; unknown/-1 is '--:--'. */
export function formatTime(seconds) {
    if (!Number.isFinite(seconds) || seconds < 0) return '--:--';
    const total = Math.floor(seconds);
    const hours = Math.floor(total / 3600);
    const minutes = Math.floor(total / 60) % 60;
    const remainder = String(total % 60).padStart(2, '0');
    return hours
        ? `${hours}:${String(minutes).padStart(2, '0')}:${remainder}`
        : `${minutes}:${remainder}`;
}
