import Gio from 'gi://Gio';
import GLib from 'gi://GLib';

const PRIORITY = GLib.PRIORITY_DEFAULT;

// Call again from the pending operation's callback after cancellation.
export function closeStream(stream) {
    if (!stream || stream.is_closed() || stream.has_pending()) return;
    stream.close_async(PRIORITY, null, (source, result) => {
        try { source.close_finish(result); } catch (_) { /* Best-effort pipe cleanup. */ }
    });
}

// Limits are per frame, not per read or connection. Geometric growth bounds
// both copying and retained storage, even when reads arrive one byte at a time.
export class LineDecoder {
    constructor(maxBytes, { fatal = true, includeNewline = false, error = 'IPC frame is too large.' } = {}) {
        Object.assign(this, { maxBytes, includeNewline, error });
        this.decoder = new TextDecoder('utf-8', { fatal });
        this.buffer = new Uint8Array(0);
        this.size = 0;
    }
    push(bytes, receive) {
        let start = 0;
        while (start < bytes.length) {
            const newline = bytes.indexOf(10, start);
            const end = newline < 0 ? bytes.length : newline + (this.includeNewline ? 1 : 0);
            const size = this.size + end - start;
            if (size > this.maxBytes) throw new Error(this.error);
            if (size > this.buffer.length) {
                const buffer = new Uint8Array(Math.min(this.maxBytes, Math.max(size, this.buffer.length * 2, 4096)));
                buffer.set(this.buffer.subarray(0, this.size));
                this.buffer.fill(0);
                this.buffer = buffer;
            }
            this.buffer.set(bytes.subarray(start, end), this.size);
            this.size = size;
            if (newline < 0) break;
            const text = this.finish();
            if (receive(text) === false) return;
            start = newline + 1;
        }
    }
    // Only callers whose protocol permits an unterminated EOF frame use this.
    finish() {
        const bytes = this.buffer.subarray(0, this.size);
        this.size = 0;
        try { return this.decoder.decode(bytes); }
        finally { bytes.fill(0); }
    }
    // Wipe owned bytes only; decoded JS strings cannot be erased here.
    clear() {
        this.buffer.fill(0);
        this.size = 0;
    }
}

// Queuing, credential ownership and cancellation policy remain with the caller.
// Always finish a native callback, including those from an obsolete generation.
export function writeBytes(stream, data, cancellable, {
    chunkBytes = 4096, active = () => true, onDone = () => {}, onError = () => {},
    onProgress = () => {}, closeInactive = false,
} = {}) {
    const next = offset => {
        if (!active()) return;
        if (offset === data.length) { onDone(); return; }
        // GUnixOutputStream may write inline; the default stays within PIPE_BUF.
        const end = Math.min(data.length, offset + chunkBytes);
        try {
            stream.write_bytes_async(new GLib.Bytes(data.slice(offset, end)), PRIORITY, cancellable, (source, result) => {
                try {
                    const written = source.write_bytes_finish(result);
                    if (!active()) return;
                    if (!Number.isInteger(written) || written <= 0 || written > end - offset)
                        throw new Error('IPC input pipe stopped accepting data.');
                    onProgress(written);
                    next(offset + written);
                } catch (error) { if (active()) onError(error); }
                finally { if (closeInactive && !active()) closeStream(source); }
            });
        } catch (error) { if (active()) onError(error); }
    };
    next(0);
}

// One-shot lifecycle shared by bounded streaming and Gio's UTF-8 communication.
// Waiting is never cancelled: even rejected/cancelled calls reap their child.
export function communicate(argv, {
    input = null, cancellable = new Gio.Cancellable(), maxBytes = Infinity, timeoutMs = 10000,
    flags = 0, errors = {}, onSpawn = () => {}, onSettled = () => {},
} = {}) {
    const errorFor = key => new Error(errors[key] || `IPC helper ${key}.`);
    return new Promise((resolve, reject) => {
        const io = new Gio.Cancellable();
        let child, output, stdin, timer = 0, cancelId = 0, length = 0;
        let settled = false, exited = false, outputDone = false, inputDone = input === null;
        let chunks = [];
        const settle = (error, value) => {
            if (settled) return;
            settled = true;
            if (timer) GLib.source_remove(timer);
            timer = 0;
            // Disconnecting inside the cancellable's own handler can deadlock.
            if (cancelId) GLib.idle_add(PRIORITY, () => {
                cancellable.disconnect(cancelId); cancelId = 0; return GLib.SOURCE_REMOVE;
            });
            if (error) { io.cancel(); if (!exited) child?.force_exit(); }
            chunks = [];
            closeStream(output); closeStream(stdin);
            try { onSettled(child); } catch (hookError) { error ||= hookError; }
            if (error) reject(error); else resolve(value);
        };
        const complete = () => {
            if (settled || !exited || !outputDone || !inputDone) return;
            try {
                const bytes = new Uint8Array(length);
                let offset = 0;
                for (const chunk of chunks) { bytes.set(chunk, offset); offset += chunk.length; }
                settle(null, new TextDecoder('utf-8', { fatal: true }).decode(bytes));
            } catch (error) { settle(error); }
        };
        const read = () => output.read_bytes_async(8192, PRIORITY, io, (stream, result) => {
            try {
                const bytes = stream.read_bytes_finish(result).toArray();
                if (settled) return;
                if (!bytes.length) { outputDone = true; closeStream(stream); complete(); }
                else if (length + bytes.length > maxBytes) settle(errorFor('responseLimit'));
                else { length += bytes.length; chunks.push(bytes); read(); }
            } catch (error) { settle(error); }
            finally { if (settled) closeStream(stream); }
        });
        try {
            if (cancellable.is_cancelled()) throw errorFor('cancelled');
            if (input !== null && typeof input !== 'string') throw new TypeError(errors.invalidInput || 'IPC helper input must be a string or null.');
            if (input !== null && input.length > maxBytes) throw errorFor('inputLimit');
            const bounded = Number.isFinite(maxBytes);
            const data = input === null || !bounded ? null : new TextEncoder().encode(input);
            if (data && data.length > maxBytes) throw errorFor('inputLimit');
            child = Gio.Subprocess.new(argv, flags | Gio.SubprocessFlags.STDOUT_PIPE
                | (input === null ? 0 : Gio.SubprocessFlags.STDIN_PIPE));
            // Establish ownership before user hooks can cancel or throw. Native
            // communication may return on cancellation before the child exits.
            child.wait_async(null, (source, result) => {
                try {
                    source.wait_finish(result); exited = true;
                    if (bounded && !settled) {
                        if (!source.get_successful()) throw errorFor('failed');
                        complete();
                    }
                } catch (error) { if (bounded) settle(error); }
            });
            output = child.get_stdout_pipe();
            stdin = input === null ? null : child.get_stdin_pipe();
            onSpawn(child);
            if (settled) return;
            cancelId = cancellable.connect(() => settle(errorFor('cancelled')));
            if (settled) {
                // connect can call back inline, before its handler ID is assigned.
                if (cancelId) { cancellable.disconnect(cancelId); cancelId = 0; }
                return;
            }
            timer = GLib.timeout_add(PRIORITY, timeoutMs, () => {
                timer = 0; settle(errorFor('timeout')); return GLib.SOURCE_REMOVE;
            });
            if (!bounded) {

                child.communicate_utf8_async(input, io, (source, result) => {
                    try {
                        const [, text] = source.communicate_utf8_finish(result);
                        if (!settled) {
                            if (!source.get_successful()) throw errorFor('failed');
                            settle(null, text);
                        }
                    } catch (error) { settle(error); }
                    finally { if (settled) { closeStream(output); closeStream(stdin); } }
                });
            } else {
                read();
                if (stdin) writeBytes(stdin, data, io, {
                    active: () => !settled, closeInactive: true, onError: settle,
                    onDone: () => stdin.close_async(PRIORITY, io, (stream, result) => {
                        try { stream.close_finish(result); inputDone = true; complete(); }
                        catch (error) { settle(error); }
                        finally { if (settled) closeStream(stream); }
                    }),
                });
            }
        } catch (error) { settle(error); }
    });
}
