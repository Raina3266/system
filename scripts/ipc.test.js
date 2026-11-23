import test from 'node:test';
import assert from 'node:assert/strict';
import { loadIPC } from './ipc-test-loader.js';

const encode = text => new TextEncoder().encode(text);
const { LineDecoder } = loadIPC();

test('JSONL decoding spans reads and UTF-8 scalars with per-frame, not lifetime, limits', () => {
    const frames = new LineDecoder(4), lines = [];
    for (const byte of encode('🎧\na\nb\n🎧\n')) frames.push(Uint8Array.of(byte), line => lines.push(line));
    assert.deepEqual(lines, ['🎧', 'a', 'b', '🎧']);
    assert.equal(frames.size, 0);
    assert.ok(frames.buffer.length <= frames.maxBytes);
});

for (const includeNewline of [false, true]) for (const eof of [false, true]) {
    test(`one-MiB frames retain capped storage under one-byte fragmentation (newline=${includeNewline}, EOF=${eof})`, () => {
        const limit = 1024 * 1024, frames = new LineDecoder(limit, { includeNewline });
        const fragment = encode('x'), length = limit - (includeNewline && !eof ? 1 : 0);
        let buffer = frames.buffer, allocations = 0, allocatedBytes = 0;
        for (let index = 0; index < length; index++) {
            frames.push(fragment, () => assert.fail('unterminated frame'));
            if (frames.buffer !== buffer) {
                buffer = frames.buffer; allocations++; allocatedBytes += buffer.length;
                assert.ok(buffer.length <= limit, 'capacity must never exceed the frame limit');
            }
        }
        assert.equal(frames.size, length);
        assert.ok(allocations <= 10, 'fragment count must not control retained allocation count');
        assert.ok(allocatedBytes <= 2 * limit, 'geometric growth must bound total allocation/copy work');
        fragment[0] = 121; // Retained bytes must not alias the incoming read's storage.
        const expected = 'x'.repeat(length) + (includeNewline && !eof ? '\n' : '');
        if (eof) assert.equal(frames.finish(), expected);
        else frames.push(encode('\n'), line => assert.equal(line, expected));
        assert.equal(frames.size, 0);
        assert.equal(frames.buffer, buffer, 'completed frames reuse their bounded capacity');
    });
}

test('frame boundaries handle empty lines, exact limits and optional newline accounting', () => {
    for (const includeNewline of [false, true]) {
        const frames = new LineDecoder(4, { includeNewline }), lines = [];
        frames.push(encode(includeNewline ? '\nabc\n' : '\nabcd\n'), line => lines.push(line));
        assert.deepEqual(lines, includeNewline ? ['\n', 'abc\n'] : ['', 'abcd']);
        frames.push(encode('abcd'), () => assert.fail('unterminated frame'));
        if (includeNewline) assert.throws(() => frames.push(encode('\n'), () => {}), /too large/);
        else assert.throws(() => frames.push(encode('e'), () => {}), /too large/);
    }
});

test('EOF flushing is explicit and validates incomplete UTF-8 without poisoning later frames', () => {
    const frames = new LineDecoder(20);
    frames.push(encode('tail 🎧'), () => assert.fail('must wait for EOF'));
    assert.equal(frames.finish(), 'tail 🎧');
    frames.push(Uint8Array.of(0xf0, 0x9f), () => {});
    assert.throws(() => frames.finish(), /encoding|encoded|UTF/i);
    frames.push(encode('ok\n'), line => assert.equal(line, 'ok'));
    assert.throws(() => frames.push(Uint8Array.of(0xff, 10), () => {}), /encoding|encoded|UTF/i);
    new LineDecoder(20, { fatal: false }).push(Uint8Array.of(0xff, 10), line => assert.equal(line, '�'));
});

test('completed frames wipe consumed bytes before delivery or EOF return while preserving decoded text', () => {
    for (const eof of [false, true]) {
        const frames = new LineDecoder(100), input = encode('secret 🎧');
        frames.push(input, assert.fail);
        const buffer = frames.buffer;
        const check = text => {
            assert.equal(text, 'secret 🎧');
            assert.equal(frames.size, 0);
            assert.ok(buffer.every(byte => byte === 0));
        };
        if (eof) check(frames.finish()); else frames.push(encode('\n'), check);
        assert.equal(new TextDecoder().decode(input), 'secret 🎧', 'only decoder-owned bytes are wiped');
        frames.push(encode('ok\n'), text => assert.equal(text, 'ok'));
        assert.equal(frames.buffer, buffer);
        assert.ok(buffer.every(byte => byte === 0));
    }
});

test('invalid and incomplete UTF-8 wipe consumed bytes even when decoding throws', () => {
    for (const eof of [false, true]) {
        const frames = new LineDecoder(100);
        frames.push(encode('secret '), assert.fail);
        frames.push(eof ? Uint8Array.of(0xf0, 0x9f) : Uint8Array.of(0xff), assert.fail);
        const buffer = frames.buffer;
        assert.throws(() => eof ? frames.finish() : frames.push(encode('\n'), assert.fail), /encoding|encoded|UTF/i);
        assert.equal(frames.size, 0);
        assert.ok(buffer.every(byte => byte === 0));
        frames.push(encode('ok\n'), text => assert.equal(text, 'ok'));
        assert.ok(buffer.every(byte => byte === 0));
    }
});

test('growing a frame wipes the replaced buffer after copying its live bytes', () => {
    const frames = new LineDecoder(8192);
    frames.push(encode('s'.repeat(4096)), assert.fail);
    const oldBuffer = frames.buffer;
    frames.push(encode('ecret'), assert.fail);
    assert.notEqual(frames.buffer, oldBuffer);
    assert.ok(oldBuffer.every(byte => byte === 0));
    assert.equal(frames.finish(), 's'.repeat(4096) + 'ecret');
    assert.ok(frames.buffer.every(byte => byte === 0));
});

test('clear wipes an incomplete frame, resets accounting and permits safe reuse', () => {
    const frames = new LineDecoder(16);
    frames.push(encode('partial secret'), assert.fail);
    const buffer = frames.buffer;
    frames.clear(); frames.clear();
    assert.equal(frames.size, 0);
    assert.equal(frames.buffer, buffer);
    assert.ok(buffer.every(byte => byte === 0));
    assert.equal(frames.finish(), '');
    frames.push(encode('new\n'), text => assert.equal(text, 'new'));
    assert.ok(buffer.every(byte => byte === 0));
});

test('a consumer can stop a read batch after disposal without delivering later frames', () => {
    const frames = new LineDecoder(20), lines = [];
    frames.push(encode('first\nsecond\n'), line => { lines.push(line); return false; });
    assert.deepEqual(lines, ['first']);
});

function fixture() {
    let serial = 0;
    const timers = new Map(), idles = [], children = [];
    class Cancellable {
        cancelled = false;
        handlers = new Map();
        connect(callback) {
            if (this.cancelled) { this.invoke(callback); return 0; }
            const id = ++serial; this.handlers.set(id, callback); return id;
        }
        invoke(callback) {
            this.inCallback = true;
            try { callback(); } finally { this.inCallback = false; }
        }
        disconnect(id) { assert.ok(!this.inCallback, 'disconnect must not run inside a cancellation handler'); this.handlers.delete(id); }
        is_cancelled() { return this.cancelled; }
        cancel() {
            if (this.cancelled) return;
            this.cancelled = true;
            for (const callback of this.handlers.values()) this.invoke(callback);
        }
    }
    const GLib = {
        PRIORITY_DEFAULT: 0, SOURCE_REMOVE: false,
        Bytes: class { constructor(data) { this.data = data; } },
        timeout_add(priority, delay, callback) { const id = ++serial; timers.set(id, { delay, callback }); return id; },
        source_remove(id) { assert.ok(timers.delete(id)); },
        idle_add(priority, callback) { idles.push(callback); return ++serial; },
    };
    function stream() {
        return {
            closed: false, pending: false, closes: 0, finishes: 0, writes: [],
            is_closed() { return this.closed; }, has_pending() { return this.pending; },
            close_async(priority, cancellable, callback) {
                assert.equal(this.pending, false); this.closes++; this.closed = true; callback(this, {});
            },
            close_finish() {},
            read_bytes_async(size, priority, cancel, callback) { this.pending = true; this.read = { cancel, callback }; },
            read_bytes_finish(result) { this.finishes++; if (result.error) throw result.error; return { toArray: () => result.bytes }; },
            feed(bytes, error = null) {
                const { callback } = this.read; this.pending = false;
                callback(this, { bytes: typeof bytes === 'string' ? encode(bytes) : bytes, error });
            },
            write_bytes_async(bytes, priority, cancel, callback) {
                if (this.throwWrite) throw new Error('sync write failure');
                this.pending = true; this.writes.push({ data: bytes.data, cancel, callback });
            },
            write_bytes_finish(result) { this.finishes++; if (result.error) throw result.error; return result.count; },
            written(count = this.writes.at(-1).data.length, error = null) {
                this.pending = false; this.writes.at(-1).callback(this, { count, error });
            },
        };
    }
    const Gio = { Cancellable, SubprocessFlags: { STDIN_PIPE: 1, STDOUT_PIPE: 2 }, Subprocess: {
        new(argv, flags) {
            if (h.spawnError) throw new Error('spawn failure');
            const child = { argv, flags, input: stream(), output: stream(), killed: 0, reaped: 0, successful: true,
                get_stdin_pipe() { this.inputAcquired = true; return this.input; },
                get_stdout_pipe() { this.outputAcquired = true; return this.output; },
                get_successful() { return this.successful; }, force_exit() { this.killed++; },
                wait_async(cancel, callback) { assert.equal(cancel, null); this.wait = callback; },
                wait_finish() { this.reaped++; }, exit() { this.wait(this, {}); },
                communicate_utf8_async(input, cancel, callback) {
                    this.output.pending = true;
                    if (input !== null) this.input.pending = true;
                    this.native = { input, cancel, callback };
                },
                communicate_utf8_finish(result) { this.finished = true; if (result.error) throw result.error; return [true, result.text]; },
                reply(text, error = null) {
                    this.output.pending = this.input.pending = false;
                    this.native.callback(this, { text, error });
                },
            };
            children.push(child); return child;
        },
    } };
    const h = { ...loadIPC(GLib, Gio), Gio, timers, children, stream,
        flush() { while (idles.length) idles.shift()(); },
        timeout() { const [id, { callback }] = [...timers][0]; timers.delete(id); callback(); },
    };
    return h;
}

test('partial writes stay bounded and ordered, keep caller buffers intact and drain once', () => {
    const h = fixture(), pipe = h.stream(), data = encode('abcdefgh');
    let done = 0;
    const progress = [];
    h.writeBytes(pipe, data, null, { chunkBytes: 3, onProgress: count => progress.push(count), onDone: () => done++, onError: assert.fail });
    pipe.written(1); pipe.written(2); pipe.written(3); pipe.written(2);
    assert.deepEqual(pipe.writes.map(w => new TextDecoder().decode(w.data)), ['abc', 'bcd', 'def', 'gh']);
    assert.equal(done, 1); assert.deepEqual(progress, [1, 2, 3, 2]);
    assert.equal(new TextDecoder().decode(data), 'abcdefgh');
});

for (const count of [0, -1, 1.5, NaN, 4]) test(`invalid write progress fails once: ${count}`, () => {
    const h = fixture(), pipe = h.stream(), errors = [];
    h.writeBytes(pipe, encode('abc'), null, { onDone: assert.fail, onError: error => errors.push(error) });
    pipe.written(count);
    assert.equal(errors.length, 1); assert.equal(pipe.writes.length, 1);
});

test('synchronous write errors reach the owner and empty input completes without a native write', () => {
    const h = fixture(), pipe = h.stream(); pipe.throwWrite = true;
    let done = 0, errors = 0;
    h.writeBytes(pipe, encode('abc'), null, { onError: () => errors++ });
    h.writeBytes(pipe, new Uint8Array(), null, { onDone: () => done++ });
    assert.equal(errors, 1); assert.equal(done, 1);
});

test('obsolete write callbacks are finished but neither continued nor reported; cleanup waits for pending I/O', () => {
    const h = fixture(), pipe = h.stream(); let active = true;
    h.writeBytes(pipe, encode('abc'), null, { active: () => active, closeInactive: true, onDone: assert.fail, onError: assert.fail });
    h.closeStream(pipe); assert.equal(pipe.closes, 0);
    active = false; pipe.written(0, new Error('cancelled'));
    assert.equal(pipe.finishes, 1); assert.equal(pipe.closes, 1); assert.equal(pipe.writes.length, 1);
    h.closeStream(pipe); assert.equal(pipe.closes, 1);
});

test('bounded one-shots wait for stdin close, stdout EOF and exit, preserving split UTF-8', async () => {
    const h = fixture(), settled = [];
    const promise = h.communicate(['/literal path/helper'], { input: 'ab', maxBytes: 4, onSettled: child => settled.push(child) });
    const child = h.children[0];
    assert.deepEqual(child.argv, ['/literal path/helper']); assert.equal(child.flags, 3);
    child.output.feed(Uint8Array.of(0xf0, 0x9f)); child.output.feed(Uint8Array.of(0x8e, 0xa7));
    child.output.feed(new Uint8Array()); child.exit();
    assert.equal(settled.length, 0);
    child.input.written(1); child.input.written(1);
    assert.equal(await promise, '🎧');
    assert.equal(child.reaped, 1); assert.equal(child.killed, 0); assert.equal(h.timers.size, 0);
    assert.equal(child.input.closed, true); assert.equal(child.output.closed, true);
    assert.deepEqual(settled, [child]);
});

for (const input of ['12345', '界界', 1]) test(`invalid/oversized one-shot input fails before spawning: ${input}`, async () => {
    const h = fixture();
    await assert.rejects(h.communicate(['helper'], { input, maxBytes: 4 }));
    assert.equal(h.children.length, 0); assert.equal(h.timers.size, 0);
});

for (const mode of ['limit', 'utf8', 'exit', 'read', 'write', 'spawn']) test(`bounded one-shot failure cleans up and reaps: ${mode}`, async () => {
    const h = fixture(); h.spawnError = mode === 'spawn';
    const promise = h.communicate(['helper'], { input: 'x', maxBytes: 4 });
    const child = h.children[0];
    if (child) {
        if (mode === 'limit') child.output.feed('12345');
        if (mode === 'utf8') { child.output.feed(Uint8Array.of(0xff)); child.output.feed(new Uint8Array()); child.input.written(); child.exit(); }
        if (mode === 'exit') { child.successful = false; child.exit(); }
        if (mode === 'read') child.output.feed(null, new Error('read failure'));
        if (mode === 'write') child.input.written(0);
    }
    await assert.rejects(promise);
    assert.equal(h.timers.size, 0);
    if (child) {
        if (!child.reaped) child.exit();
        if (child.input.pending) child.input.written(0, new Error('cancelled'));
        if (child.output.pending) child.output.feed(null, new Error('cancelled'));
        assert.equal(child.reaped, 1); assert.equal(child.input.closed, true); assert.equal(child.output.closed, true);
    }
});

for (const maxBytes of [4, Infinity]) {
    for (const reason of ['cancel', 'timeout']) test(`${maxBytes}: one-shot ${reason} settles once, finishes late I/O and reaps`, async () => {
        const h = fixture(), cancellable = new h.Gio.Cancellable(); let settled = 0;
        const promise = h.communicate(['helper'], { cancellable, maxBytes, onSettled: () => settled++,
            errors: { cancelled: 'domain cancelled', timeout: 'domain timeout' } });
        const child = h.children[0];
        if (reason === 'cancel') cancellable.cancel(); else h.timeout();
        await assert.rejects(promise, new RegExp(`domain ${reason}`));
        assert.equal(child.killed, 1); assert.equal(h.timers.size, 0);
        if (Number.isFinite(maxBytes)) {
            assert.equal(child.output.read.cancel.is_cancelled(), true);
            child.output.feed(null, new Error('late cancellation'));
        } else {
            assert.equal(child.native.cancel.is_cancelled(), true);
            child.reply('late success'); assert.equal(child.finished, true);
        }
        child.exit(); h.flush();
        assert.equal(child.output.closes, 1, 'late native callbacks must close previously pending pipes');
        assert.equal(child.reaped, 1); assert.equal(settled, 1); assert.equal(cancellable.handlers.size, 0);
    });
}

test('native communication preserves exact input, flags and output, and failed/spawned children clean up', async () => {
    for (const mode of ['success', 'failed', 'native-error', 'spawn']) {
        const h = fixture(); h.spawnError = mode === 'spawn';
        const promise = h.communicate(['helper'], { input: '  界\n', flags: 8 });
        const child = h.children[0];
        if (child) {
            assert.equal(child.flags, 11); assert.equal(child.native.input, '  界\n');
            child.successful = mode !== 'failed'; child.exit();
            child.reply('exact\n', mode === 'native-error' ? new Error('native failure') : null);
        }
        if (mode === 'success') assert.equal(await promise, 'exact\n'); else await assert.rejects(promise);
        assert.equal(h.timers.size, 0); if (child) assert.equal(child.reaped, 1);
    }
});

for (const maxBytes of [4, Infinity]) {
    for (const mode of ['cancel', 'throw', 'cancel-and-throw']) test(`${maxBytes}: spawn hook ${mode} closes owned pipes and still reaps`, async () => {
        const h = fixture(), cancellable = new h.Gio.Cancellable();
        const hookError = new Error('spawn hook failed');
        let settled = 0;
        const promise = h.communicate(['helper'], { input: 'x', maxBytes, cancellable,
            onSpawn: child => {
                assert.equal(typeof child.wait, 'function', 'the uncancelled wait must precede hooks');
                assert.equal(child.inputAcquired, true); assert.equal(child.outputAcquired, true);
                if (mode !== 'throw') cancellable.cancel();
                if (mode !== 'cancel') throw hookError;
            },
            onSettled: () => settled++, errors: { cancelled: 'hook cancelled' },
        });
        await assert.rejects(promise, mode === 'cancel' ? /hook cancelled/ : error => error === hookError);
        const child = h.children[0];
        assert.equal(child.killed, 1); assert.equal(child.input.closes, 1); assert.equal(child.output.closes, 1);
        assert.equal(child.input.writes.length, 0); assert.equal(child.output.read, undefined); assert.equal(child.native, undefined);
        assert.equal(h.timers.size, 0); assert.equal(settled, 1);
        child.exit(); h.flush();
        assert.equal(child.reaped, 1); assert.equal(cancellable.handlers.size, 0);
    });
    for (const failed of [false, true]) test(`${maxBytes}: a throwing settlement hook cannot strand the promise or override a primary failure (${failed})`, async () => {
        const h = fixture(), hookError = new Error('settlement hook failed');
        const promise = h.communicate(['helper'], { input: 'x', maxBytes, onSettled: () => { throw hookError; }, errors: { failed: 'primary failure' } });
        const child = h.children[0]; child.successful = !failed;
        if (Number.isFinite(maxBytes)) {
            child.input.written(); child.output.feed('ok'); child.output.feed(new Uint8Array()); child.exit();
        } else { child.exit(); child.reply('ok'); }
        await assert.rejects(promise, failed ? /primary failure/ : error => error === hookError);
        assert.equal(h.timers.size, 0); assert.equal(child.reaped, 1);
        assert.equal(child.input.closes, 1); assert.equal(child.output.closes, 1);
    });
}

test('already-cancelled connect invokes its callback inline without retaining a handler', () => {
    const h = fixture(), cancellable = new h.Gio.Cancellable(); cancellable.cancel();
    let called = false;
    assert.equal(cancellable.connect(() => { called = true; }), 0);
    assert.equal(called, true); assert.equal(cancellable.handlers.size, 0);
});

test('pre-cancelled one-shots never spawn or arm a timeout', async () => {
    const h = fixture(), cancellable = new h.Gio.Cancellable(); cancellable.cancel();
    await assert.rejects(h.communicate(['helper'], { cancellable }));
    assert.equal(h.children.length, 0); assert.equal(h.timers.size, 0);
});
