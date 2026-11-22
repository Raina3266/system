import Gio from 'gi://Gio';
import GLib from 'gi://GLib';

const PRIORITY = GLib.PRIORITY_DEFAULT;
const CHUNK_BYTES = 8192;
const MAX_LINE_BYTES = 1024 * 1024;
const MAX_QUEUE_BYTES = 1024 * 1024;
const MAX_QUEUE_LENGTH = 128;
const MAX_PENDING = 64;
const REQUEST_TIMEOUT_MS = 30000;
const START_TIMEOUT_MS = 10000;
const WRITE_TIMEOUT_MS = 5000;
const encoder = new TextEncoder();
const decoder = new TextDecoder('utf-8', { fatal: true });
const copy = value => value == null ? value : JSON.parse(JSON.stringify(value));
const object = value => value !== null && typeof value === 'object' && !Array.isArray(value);
const owns = (value, key) => Object.prototype.hasOwnProperty.call(value, key);
const failure = (message, code = 'transport') => Object.assign(new Error(message), { code, authentication: code === 'AUTHENTICATION' });

// This adapter owns only IPC and snapshots. Rust owns NetworkManager policy,
// authorization, scanning, profile construction and every native operation.
export class NetworkBackend {
    constructor({ onChange = () => {}, launch = (argv, flags) => Gio.Subprocess.new(argv, flags) } = {}) {
        this.onChange = onChange;
        this._launch = launch;
        this._started = this._disposed = false;
        this._opened = false;
        this._child = null;
        this._generation = 0;
        this._serial = 0;
        this._pending = new Map();
        this._queue = [];
        this._queuedBytes = 0;
        this._writing = null;
        this._buffer = new Uint8Array();
        this._startupTimer = this._writeTimer = 0;
        this._failure = null;
        this._clearState();
    }

    _clearState() {
        this._rows = [];
        this._identities = this._details = {};
        this._available = this._wifiAvailable = this._wifiEnabled = this._wifiHardwareEnabled = false;
    }

    get rows() { return copy(this._rows); }
    get available() { return this._available; }
    get wifiAvailable() { return this._wifiAvailable; }
    get wifiEnabled() { return this._wifiEnabled; }
    get wifiHardwareEnabled() { return this._wifiHardwareEnabled; }

    start() {
        if (this._started || this._disposed) return;
        this._started = true;
        const generation = ++this._generation;
        try {
            this._io = new Gio.Cancellable();
            this._child = this._launch([GLib.getenv('PANEL_BACKEND') || 'panel-launcher', 'backend', 'network'],
                Gio.SubprocessFlags.STDIN_PIPE | Gio.SubprocessFlags.STDOUT_PIPE | Gio.SubprocessFlags.STDERR_SILENCE);
            this._input = this._child.get_stdin_pipe();
            this._output = this._child.get_stdout_pipe();
            this._startupTimer = GLib.timeout_add(PRIORITY, START_TIMEOUT_MS, () => {
                this._startupTimer = 0;
                this._failed(failure('The network backend did not provide its initial state.', 'timeout'));
                return GLib.SOURCE_REMOVE;
            });
            this._child.wait_async(null, (child, result) => {
                try { child.wait_finish(result); } catch (_) { /* Never expose subprocess diagnostics. */ }
                if (generation === this._generation) this._failed(failure('The network backend exited.'));
            });
            this._read(generation);
            this._send({ method: 'setOpened', args: [this._opened] });
        } catch (_) { this._failed(failure('Could not start the network backend.')); }
    }

    setOpened(opened) {
        if (this._disposed) return;
        this._opened = Boolean(opened);
        if (this._child) this._send({ method: 'setOpened', args: [this._opened] });
    }

    getIdentity(key) {
        return !this._disposed && owns(this._identities, key) ? copy(this._identities[key]) : null;
    }

    details(key) {
        if (this._disposed) throw failure('The network panel is closed.', 'cancelled');
        if (!owns(this._details, key) || this._details[key] === null)
            throw this._failure ?? failure('Network details are not available. Reopen network information.');
        return copy(this._details[key]);
    }

    async toggleWifi(cancellable = null) { return this._request('toggleWifi', [], cancellable); }
    async readSecret(key, identity, cancellable = null, { expectedStamp } = {}) { return this._request('readSecret', [key, identity], cancellable, expectedStamp); }
    async readSavedSecret(key, profileUuid, cancellable = null, { expectedStamp } = {}) { return this._request('readSavedSecret', [key, profileUuid], cancellable, expectedStamp); }
    async activate(key, { profileUuid = '', password = null, expectedStamp, cancellable = null } = {}) {
        return this._request('activate', [key, { profileUuid, password }], cancellable, expectedStamp);
    }
    async disconnect(key, cancellable = null) { return this._request('disconnect', [key], cancellable); }
    async forget(key, cancellable = null, { expectedStamp } = {}) { return this._request('forget', [key], cancellable, expectedStamp); }

    _request(method, args, cancellable = null, expectedStamp) {
        if (this._disposed) return Promise.reject(failure('The network panel is closed.', 'cancelled'));
        if (cancellable?.is_cancelled()) return Promise.reject(failure('The network request was cancelled.', 'cancelled'));
        if (!this._child) return Promise.reject(this._failure ?? failure('The network backend is not running.'));
        if (this._pending.size >= MAX_PENDING) return Promise.reject(failure('Too many network requests are pending.'));
        const id = ++this._serial;
        return new Promise((resolve, reject) => {
            const request = { resolve, reject, cancellable, signal: 0, timer: 0 };
            this._pending.set(id, request);
            try {
                request.timer = GLib.timeout_add(PRIORITY, REQUEST_TIMEOUT_MS, () => {
                    request.timer = 0;
                    this._cancel(id, failure('The network request timed out. Try again.', 'timeout'));
                    return GLib.SOURCE_REMOVE;
                });
                if (cancellable) request.signal = cancellable.connect(() => {
                    // Gio.Cancellable.disconnect must not run inside its own handler.
                    Promise.resolve().then(() => this._cancel(id, failure('The network request was cancelled.', 'cancelled')));
                });
                if (cancellable?.is_cancelled()) this._cancel(id, failure('The network request was cancelled.', 'cancelled'));
                else this._send({ id, method, args, expectedStamp });
            } catch (_) {
                this._settle(id, failure('Could not send the network request.'));
            }
        });
    }

    _settle(id, error, result) {
        const request = this._pending.get(id);
        if (!request) return;
        this._pending.delete(id);
        if (request.timer) GLib.source_remove(request.timer);
        if (request.signal) request.cancellable.disconnect(request.signal);
        if (error) request.reject(error);
        else request.resolve(result);
    }

    _cancel(id, error) {
        if (!this._pending.has(id)) return;
        // Drop an unsent credential-bearing frame. If writing has begun, finish
        // its JSONL frame before sending cancel so the stream remains parseable.
        const index = this._queue.findIndex(frame => frame.id === id);
        if (index !== -1) {
            const [frame] = this._queue.splice(index, 1);
            this._queuedBytes -= frame.data.length;
            frame.data.fill(0);
        } else if (this._child) this._send({ method: 'cancel', args: [id] });
        this._settle(id, error);
    }

    _send(value) {
        if (!this._child || this._disposed) return;
        let data;
        try { data = encoder.encode(JSON.stringify(value) + '\n'); }
        catch (_) { throw failure('The network request is not serializable.'); }
        if (data.length > MAX_LINE_BYTES || this._queuedBytes + data.length > MAX_QUEUE_BYTES
            || this._queue.length >= MAX_QUEUE_LENGTH) {
            data.fill(0);
            this._failed(failure('The network backend input queue is full.'));
            return;
        }
        this._queue.push({ id: value.id, data });
        this._queuedBytes += data.length;
        this._write();
    }

    _write() {
        if (this._writing || !this._queue.length || !this._child) return;
        const frame = this._queue.shift();
        this._writing = frame;
        const generation = this._generation;
        // Bound the whole frame, not just each partial write: a worker consuming
        // a byte at a time must not keep a credential-bearing write alive forever.
        this._writeTimer = GLib.timeout_add(PRIORITY, WRITE_TIMEOUT_MS, () => {
            this._writeTimer = 0;
            this._failed(failure('The network backend stopped accepting requests.', 'timeout'));
            return GLib.SOURCE_REMOVE;
        });
        const next = offset => {
            if (generation !== this._generation) return;
            if (offset === frame.data.length) {
                if (this._writeTimer) GLib.source_remove(this._writeTimer);
                this._writeTimer = 0;
                this._queuedBytes -= frame.data.length;
                frame.data.fill(0);
                this._writing = null;
                this._write();
                return;
            }
            try {
                this._input.write_bytes_async(new GLib.Bytes(frame.data.slice(offset, offset + CHUNK_BYTES)), PRIORITY, this._io,
                    (stream, result) => {
                        try {
                            const written = stream.write_bytes_finish(result);
                            if (generation !== this._generation) return;
                            if (!Number.isInteger(written) || written <= 0 || written > Math.min(CHUNK_BYTES, frame.data.length - offset))
                                throw new Error();
                            next(offset + written);
                        } catch (_) {
                            if (generation === this._generation) this._failed(failure('Could not write to the network backend.'));
                        }
                    });
            } catch (_) { this._failed(failure('Could not write to the network backend.')); }
        };
        next(0);
    }

    _read(generation) {
        try {
            this._output.read_bytes_async(CHUNK_BYTES, PRIORITY, this._io, (stream, result) => {
                try {
                    // Finish callbacks even after cancellation or process teardown.
                    const bytes = stream.read_bytes_finish(result).toArray();
                    if (generation !== this._generation) return;
                    if (!bytes.length) throw failure('The network backend closed its output.');
                    const joined = new Uint8Array(this._buffer.length + bytes.length);
                    joined.set(this._buffer);
                    joined.set(bytes, this._buffer.length);
                    let start = 0;
                    for (let index = 0; index < joined.length; index++) {
                        if (joined[index] !== 10) continue;
                        if (index - start > MAX_LINE_BYTES) throw new Error();
                        this._receive(JSON.parse(decoder.decode(joined.subarray(start, index))));
                        if (generation !== this._generation) return;
                        start = index + 1;
                    }
                    if (joined.length - start > MAX_LINE_BYTES) throw new Error();
                    this._buffer = joined.slice(start);
                    this._read(generation);
                } catch (_) {
                    if (generation === this._generation) this._failed(failure('The network backend returned an invalid or incomplete response.'));
                }
            });
        } catch (_) { this._failed(failure('Could not read from the network backend.')); }
    }

    _receive(value) {
        if (!object(value)) throw new Error();
        if (value.type === 'state') {
            if (!Array.isArray(value.rows) || !object(value.identities) || !object(value.details)
                || !['available', 'wifiAvailable', 'wifiEnabled', 'wifiHardwareEnabled'].every(key => typeof value[key] === 'boolean')
                || !value.rows.every(row => object(row) && typeof row.key === 'string' && typeof row.selectionStamp === 'string' && row.selectionStamp.length > 0 && Array.isArray(row.profiles)
                    && ['canActivate', 'canDisconnect', 'canForget', 'canShare', 'acceptsPassword', 'requiresPassword'].every(key => typeof row[key] === 'boolean'))
                || !Object.values(value.identities).every(identity => identity === null || object(identity))
                || !Object.values(value.details).every(details => details === null || object(details))) throw new Error();
            if (this._startupTimer) GLib.source_remove(this._startupTimer);
            this._startupTimer = 0;
            this._rows = value.rows;
            this._identities = value.identities;
            this._details = value.details;
            this._available = value.available;
            this._wifiAvailable = value.wifiAvailable;
            this._wifiEnabled = value.wifiEnabled;
            this._wifiHardwareEnabled = value.wifiHardwareEnabled;
            this.onChange();
        } else {
            if (!Number.isSafeInteger(value.id) || value.id <= 0 || (owns(value, 'result') === owns(value, 'error'))) throw new Error();
            if (owns(value, 'error') && (!object(value.error) || typeof value.error.message !== 'string'
                || typeof value.error.code !== 'string')) throw new Error();
            const request = this._pending.get(value.id);
            if (!request) return; // Cancelled/timed-out replies cannot revive a request or its secret.
            if (request.cancellable?.is_cancelled()) {
                this._cancel(value.id, failure('The network request was cancelled.', 'cancelled'));
            } else this._settle(value.id, value.error ? failure(value.error.message, value.error.code) : null, value.result);
        }
    }

    _stop(error) {
        this._generation++;
        for (const timer of [this._startupTimer, this._writeTimer]) if (timer) GLib.source_remove(timer);
        this._startupTimer = this._writeTimer = 0;
        for (const id of this._pending.keys()) this._settle(id, error);
        this._io?.cancel();
        try { this._child?.force_exit(); } catch (_) { /* It may already have exited. */ }
        this._child = this._input = this._output = null;
        for (const frame of this._queue) frame.data.fill(0);
        this._writing?.data.fill(0);
        this._queue = [];
        this._queuedBytes = 0;
        this._writing = null;
        this._buffer = new Uint8Array();
        this._clearState();
    }

    _failed(error) {
        if (this._disposed || this._failure) return;
        this._failure = error;
        this._stop(error);
        this.onChange();
        // Never replay requests, restart the worker, or fall back to a JS backend.
    }

    dispose() {
        if (this._disposed) return;
        this._disposed = true;
        this._opened = false;
        this._stop(failure('The network panel is closed.', 'cancelled'));
        this.onChange = () => {};
    }
}

function spawn(argv) {
    const child = Gio.Subprocess.new(argv, Gio.SubprocessFlags.STDOUT_SILENCE | Gio.SubprocessFlags.STDERR_SILENCE);
    child.wait_async(null, (source, result) => {
        try { source.wait_finish(result); } catch (_) { /* Other panels may be shutting down. */ }
    });
}

export function dismissOtherPanels() {
    for (const argv of [[GLib.getenv('DASHBOARD_PANEL') || 'dashboard-panel', 'close'],
        [GLib.getenv('MEDIA_PANEL') || 'media-panel', 'close']]) {
        try { spawn(argv); } catch (_) { console.warn('Could not dismiss another panel.'); }
    }
}

export function controllerRuntime() {
    return { createCancellable: () => new Gio.Cancellable() };
}
