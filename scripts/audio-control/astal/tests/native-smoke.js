// Run with the audio package's AGS wrapper; no UI, discovery, or device writes.
import GLib from 'gi://GLib';
import GObject from 'gi://GObject';
import System from 'system';
import { AudioBackend, HelperProcess } from '../backend.js';

function assert(condition, message) { if (!condition) throw new Error(message); }
const node = GLib.find_program_in_path('node');
if (!node) throw new Error('Node.js is required for the pipe smoke test.');
const run = (code, options = {}) => new Promise((resolve, reject) => {
    new HelperProcess([node, '-e', code], { ...options,
        onExit: (error, output) => error ? reject(error) : resolve(output) });
});

async function smoke() {
    assert(await run('console.log("metadata")') === 'metadata\n', 'helper stdout');
    assert(await run('process.stderr.write("diagnostic")') === 'diagnostic', 'helper stderr');
    const chunks = [];
    const unicode = await run('const fs = require("node:fs"), data = Buffer.from("🎧"); '
        + 'fs.writeSync(1, data.subarray(0, 2)); setTimeout(() => fs.writeSync(1, data.subarray(2)), 50);',
        { onChunk: chunk => chunks.push(chunk) });
    assert(unicode === '🎧' && chunks.join('') === unicode, 'UTF-8 split across reads');
    await new Promise((resolve, reject) => {
        let worker;
        worker = new HelperProcess([node, '-e', 'require("node:readline").createInterface({ input: process.stdin })'
            + '.once("line", line => { require("node:fs").writeSync(1, line.trim() + "\\n"); process.exit(0); }); console.log("PIN:");'], {
            interactive: true,
            onChunk: text => { if (text.includes('PIN:')) worker.send('042311\n'); },
            onExit: (error, output) => {
                if (error) reject(error);
                else { try { assert(output.includes('042311'), 'interactive answer'); resolve(); } catch (failure) { reject(failure); } }
            },
        });
    });
    for (const [code, options, expected] of [
        ['require("node:fs").writeSync(1, "bad request\\n"); process.exit(2);', {}, 'bad request'],
        ['setTimeout(() => {}, 30000);', { timeout: 50 }, 'timed out'],
        ['require("node:fs").writeFileSync(1, "x".repeat(1100000));', {}, 'too large'],
    ]) {
        let error;
        try { await run(code, options); } catch (failure) { error = failure; }
        assert(error?.message.includes(expected), `expected helper failure: ${expected}`);
    }
    const backend = new AudioBackend();
    try {
        backend.start();
        await new Promise(resolve => GLib.timeout_add(GLib.PRIORITY_DEFAULT, 1800, () => { resolve(); return GLib.SOURCE_REMOVE; }));
        const model = backend.snapshot();
        assert(typeof JSON.parse(backend.status()).tooltip === 'string', 'Waybar response');
        assert(!model.output.loading && !model.input.loading, 'metadata helpers completed');
        assert(!model.output.error && !model.input.error, `metadata: ${model.output.error || model.input.error}`);
        const adapter = backend.adapter;
        if (adapter) {
            const path = new GObject.Value();
            path.init(GObject.TYPE_STRING);
            adapter.get_property('object-path', path);
            const expected = backend.bluetooth.devices.filter(device => device.adapter === path.get_string());
            assert(expected.length === model.bluetooth.devices.length, 'native adapter device filtering');
        }
        for (const kind of ['output', 'input']) {
            if (!model[kind].available) continue;
            assert(model[kind].nodeKey && model[kind].choices.length, `${kind} native state`);
            assert(model[kind].choices.some(choice => choice.default), `${kind} live default selection`);
        }
        print('Native audio smoke passed: bounded pipes, UTF-8, interactive input, timeout/error handling, Astal state, and read-only status.');
    } finally { backend.dispose(); }
}

const loop = new GLib.MainLoop(null, false);
let failed = false;
smoke().catch(error => { failed = true; logError(error); }).finally(() => loop.quit());
loop.run();
System.exit(failed ? 1 : 0);
