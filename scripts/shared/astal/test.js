import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { loadIPC } from './ipc-test-loader.js';
import { cutCornerPath, drawCutCornerFrame, hexRgb } from './drawing.js';

const encode = text => new TextEncoder().encode(text);
const { LineDecoder } = loadIPC();

test('shared infrastructure groups Astal utilities, panel-launcher and packaging together', () => {
    const shared = new URL('../', import.meta.url);
    assert.deepEqual(readdirSync(shared).sort(), ['astal', 'default.nix', 'panel-launcher']);
    for (const directory of ['astal/', 'panel-launcher/'])
        assert.ok(statSync(new URL(directory, shared)).isDirectory());
    for (const file of ['ipc.js', 'live-style.js', 'theme.js', 'socket.js', 'drawing.js', 'ipc-test-loader.js', 'package.json', 'test.js'])
        assert.ok(statSync(new URL(file, import.meta.url)).isFile(), file);
    for (const file of ['Cargo.toml', 'src/main.rs', 'src/tests.rs'])
        assert.ok(statSync(new URL(`panel-launcher/${file}`, shared)).isFile(), file);
    const recipe = readFileSync(new URL('default.nix', shared), 'utf8');
    assert.match(recipe, /workspaceRoot \? \.\.\//);
    assert.match(recipe, /workspaceMember = "shared\/panel-launcher"/);
    assert.ok(recipe.includes('/shared/astal/ipc.js'));
    for (const file of ['live-style.js', 'theme.js', 'socket.js', 'drawing.js'])
        assert.ok(recipe.includes(`/shared/astal/${file}`));

    const workspace = readFileSync(new URL('../../Cargo.toml', import.meta.url), 'utf8');
    assert.ok(workspace.includes('"shared/panel-launcher"'));
    assert.doesNotMatch(workspace, /^\s*"panel-launcher",?$/m);
});

// Exercise the real GI modules with native doubles, not duplicate implementations.
function sharedModule(file, names, bindings) {
    const source = readFileSync(new URL(file, import.meta.url), 'utf8')
        .replace(/^import .*;\r?\n/gm, '').replace(/^export /gm, '');
    return new Function(...Object.keys(bindings), `${source}\nreturn { ${names.join(', ')} };`)(...Object.values(bindings));
}

function themeFixture() {
    const calls = [], provider = {};
    let contents = JSON.stringify({ yellow: '#ffee00', cyan: '#00ccFF', extra: '#123456' }), readable = true;
    const options = { entry: '/panel/frontend.js', palettePath: '/palette.json', required: ['yellow', 'cyan'],
        selector: '.test-panel', prefix: 'tp', label: 'Test' };
    const theme = sharedModule('theme.js', ['loadPanelTheme', 'paletteDefinitions'], {
        GLib: { file_get_contents(path) { calls.push(['read', path]); return [readable, encode(contents)]; } },
        loadPanelStyle(value) { calls.push(['style', value]); return provider; },
    });
    return { ...theme, options, calls, provider, setContents(value) { contents = value; }, setReadable(value) { readable = value; } };
}

test('shared theme validates before installation and preserves namespace, extras and provider ownership', () => {
    const h = themeFixture(), result = h.loadPanelTheme(h.options);
    assert.deepEqual(result.palette, { yellow: '#ffee00', cyan: '#00ccFF', extra: '#123456' });
    assert.equal(result.provider, h.provider);
    assert.deepEqual(h.calls, [['read', '/palette.json'], ['style', {
        entry: '/panel/frontend.js', label: 'Test',
        definitions: '.test-panel {\n    --tp_yellow: #ffee00;\n    --tp_cyan: #00ccFF;\n    --tp_extra: #123456;\n}',
    }]]);
    result.palette.yellow = '#000000';
    assert.ok(h.calls[1][1].definitions.includes('#ffee00'), 'installed definitions are a restart-time snapshot');
});

test('palette definitions admit only alphabetic property names and full hexadecimal colors', () => {
    const h = themeFixture();
    assert.equal(h.paletteDefinitions({
        valid: '#aAbBcc', 'bad-name': '#123456', 'x;injected': '#123456', short: '#fff',
        named: 'red', injected: '#123456; color: red', number: 123456, nil: null,
    }, '.scope', 'sc'), '.scope {\n    --sc_valid: #aAbBcc;\n}');
});

for (const invalid of [null, [], {}, { yellow: '#ffffff' }, { yellow: '#fff', cyan: '#000000' },
    { yellow: '#ffffff', cyan: 'red' }, { yellow: '#ffffff', cyan: '#000000;evil' }]) {
    test(`invalid required palette values never install CSS: ${JSON.stringify(invalid)}`, () => {
        const h = themeFixture(); h.setContents(JSON.stringify(invalid));
        assert.throws(() => h.loadPanelTheme(h.options), /Invalid test palette colour: (yellow|cyan)/);
        assert.equal(h.calls.filter(([operation]) => operation === 'style').length, 0);
    });
}

test('theme rejects missing files, failed reads and malformed JSON without installing CSS', () => {
    const h = themeFixture();
    assert.throws(() => h.loadPanelTheme({ ...h.options, palettePath: null }), /Run through test-panel/);
    assert.deepEqual(h.calls, []);
    h.setReadable(false);
    assert.throws(() => h.loadPanelTheme(h.options), /Could not read test theme: \/palette.json/);
    h.setReadable(true); h.setContents('{broken');
    assert.throws(() => h.loadPanelTheme(h.options), SyntaxError);
    assert.ok(h.calls.every(([operation]) => operation === 'read'));
});

test('theme permits per-panel diagnostics and required color sets without broadening them', () => {
    const h = themeFixture(), options = { ...h.options, errors: {
        missing: 'Missing clipboard palette', read: 'Cannot load the clipboard theme.', invalid: 'Invalid clipboard palette',
    } };
    assert.throws(() => h.loadPanelTheme({ ...options, palettePath: '' }), /Missing clipboard palette/);
    h.setReadable(false);
    assert.throws(() => h.loadPanelTheme(options), /Cannot load the clipboard theme\./);
    h.setReadable(true); h.setContents('{"yellow":"#abcdef"}');
    assert.throws(() => h.loadPanelTheme(options), /Invalid clipboard palette: cyan/);
    assert.deepEqual(h.loadPanelTheme({ ...options, required: ['yellow'] }).palette, { yellow: '#abcdef' });
});

function socketFixture({ writeError = false, finishError = false, closeError = false, closeFinishError = false, inline = false } = {}) {
    const events = [], errors = [];
    let writeCallback, closeCallback;
    const connection = {
        close(cancellable) { events.push(['close', cancellable]); if (closeError) throw new Error('close failed'); },
        close_async(priority, cancellable, callback) {
            events.push(['close_async', priority, cancellable]);
            if (closeError) throw new Error('close failed');
            closeCallback = callback;
        },
        close_finish(result) { events.push(['close_finish', result]); if (closeFinishError) throw new Error('close finish failed'); },
    };
    const { replyAndClose } = sharedModule('socket.js', ['replyAndClose'], {
        GLib: { PRIORITY_DEFAULT: 0 },
        AstalIO: {
            write_sock(target, response, callback) {
                assert.equal(target, connection); events.push(['write', response]); writeCallback = callback;
                if (inline) callback(null, 'inline');
                if (writeError) throw new Error('write failed');
            },
            write_sock_finish(result) { events.push(['write_finish', result]); if (finishError) throw new Error('finish failed'); },
        },
        console: { warn(message) { errors.push(['warning', message]); } },
    });
    return { events, errors, connection, replyAndClose,
        onError(phase, error) { errors.push([phase, error.message]); },
        finishWrite() { writeCallback(null, 'written'); },
        finishClose() { closeCallback(connection, 'closed'); },
    };
}

for (const asyncClose of [false, true]) {
    test(`socket reply finishes writing before closing, preserving close mode (${asyncClose})`, () => {
        const h = socketFixture();
        h.replyAndClose(h.connection, 'exact response\n', { asyncClose, onError: h.onError });
        assert.deepEqual(h.events, [['write', 'exact response\n']]);
        h.finishWrite();
        assert.deepEqual(h.events, [['write', 'exact response\n'], ['write_finish', 'written'],
            asyncClose ? ['close_async', 0, null] : ['close', null]]);
        if (asyncClose) { h.finishClose(); assert.deepEqual(h.events.at(-1), ['close_finish', 'closed']); }
        assert.deepEqual(h.errors, []);
    });
    for (const failure of ['writeError', 'finishError', 'closeError', 'closeFinishError']) {
        if (!asyncClose && failure === 'closeFinishError') continue;
        test(`socket ${failure} reports without losing connection cleanup (${asyncClose})`, () => {
            const h = socketFixture({ [failure]: true });
            assert.doesNotThrow(() => h.replyAndClose(h.connection, 'sensitive response', { asyncClose, onError: h.onError }));
            if (failure !== 'writeError') h.finishWrite();
            assert.equal(h.events.filter(([operation]) => operation === (asyncClose ? 'close_async' : 'close')).length, 1);
            if (asyncClose && failure !== 'closeError') h.finishClose();
            assert.equal(h.errors.length, 1);
            assert.equal(h.errors[0][0], failure.startsWith('close') ? 'close' : 'reply');
            assert.ok(!JSON.stringify(h.errors).includes('sensitive response'));
        });
    }
}

test('socket closes only once when a native write completes inline and then throws', () => {
    const h = socketFixture({ inline: true, writeError: true });
    h.replyAndClose(h.connection, 'ok', { onError: h.onError });
    assert.equal(h.events.filter(([operation]) => operation === 'write_finish').length, 1);
    assert.equal(h.events.filter(([operation]) => operation === 'close_async').length, 1);
    h.finishClose();
    assert.deepEqual(h.errors, [['reply', 'write failed']]);
});

test('a failing error reporter cannot prevent socket cleanup', () => {
    const h = socketFixture({ finishError: true });
    h.replyAndClose(h.connection, 'ok', { onError() { throw new Error('logger failed'); } });
    assert.doesNotThrow(h.finishWrite);
    assert.equal(h.events.filter(([operation]) => operation === 'close_async').length, 1);
    h.finishClose();
});

test('default socket diagnostics are labelled and never include response bodies', () => {
    const h = socketFixture({ writeError: true });
    h.replyAndClose(h.connection, 'secret response', { label: 'Example' });
    assert.deepEqual(h.errors, [['warning', 'Example request reply: write failed']]);
});

function drawingFixture() {
    const calls = [];
    const cr = Object.fromEntries(['moveTo', 'lineTo', 'closePath', 'setSourceRGBA', 'fillPreserve', 'setLineWidth', 'stroke']
        .map(name => [name, (...args) => calls.push([name, ...args])]));
    return { calls, cr };
}

test('cut-corner geometry retains caller offsets, dimensions and exact unclamped cut', () => {
    const h = drawingFixture();
    cutCornerPath(h.cr, { x: 4.5, y: 6.5, width: 99, height: 59, cut: 10 });
    assert.deepEqual(h.calls, [['moveTo', 14.5, 6.5], ['lineTo', 103.5, 6.5], ['lineTo', 103.5, 55.5],
        ['lineTo', 93.5, 65.5], ['lineTo', 4.5, 65.5], ['lineTo', 4.5, 16.5], ['closePath']]);
    h.calls.length = 0;
    cutCornerPath(h.cr, { x: 0.5, y: 0.5, width: 2, height: 1, cut: 10 });
    assert.deepEqual(h.calls[0], ['moveTo', 10.5, 0.5], 'fixed-cut callers are not silently clamped');
    assert.deepEqual(h.calls[2], ['lineTo', 2.5, -8.5]);
});

for (const opacity of [0.9, 0.95]) test(`frame painting preserves the neutral surface, accent and opacity ${opacity}`, () => {
    const h = drawingFixture();
    drawCutCornerFrame(h.cr, { width: 99, height: 59, color: '#fCeE0A', opacity });
    assert.deepEqual(h.calls.slice(0, 7), [['moveTo', 10.5, 0.5], ['lineTo', 99.5, 0.5], ['lineTo', 99.5, 49.5],
        ['lineTo', 89.5, 59.5], ['lineTo', 0.5, 59.5], ['lineTo', 0.5, 10.5], ['closePath']]);
    assert.deepEqual(h.calls.slice(7), [['setSourceRGBA', 37 / 255, 37 / 255, 47 / 255, opacity], ['fillPreserve'],
        ['setSourceRGBA', 252 / 255, 238 / 255, 10 / 255, 1], ['setLineWidth', 1], ['stroke']]);
});

test('frame defaults do not retain earlier geometry or color across redraws', () => {
    const h = drawingFixture();
    drawCutCornerFrame(h.cr, { x: 8, y: 9, width: 20, height: 30, cut: 0, color: '#ff0000', opacity: 0.95 });
    h.calls.length = 0;
    drawCutCornerFrame(h.cr, { width: 99, height: 59, color: '#00ff00' });
    assert.deepEqual(h.calls[0], ['moveTo', 10.5, 0.5]);
    assert.deepEqual(h.calls[7], ['setSourceRGBA', 37 / 255, 37 / 255, 47 / 255, 0.9]);
    assert.deepEqual(h.calls[9], ['setSourceRGBA', 0, 1, 0, 1]);
    assert.deepEqual(hexRgb('#AbCdEf'), [171 / 255, 205 / 255, 239 / 255]);
});

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
