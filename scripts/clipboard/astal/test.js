// node --test test.js
// CLIPBOARD_PANEL_IMPORT_ONLY=1 <clipboard runtime>/bin/panel-gjs -m test.js native
// Native mode requires an isolated display, D-Bus session, and temporary history.
// node test.js --ui --runtime PATH --helper PATH --palette FILE [--panel PATH]
async function isolatedUI() {
    const fs = await import('node:fs');
    const path = await import('node:path');
    const os = await import('node:os');
    const { spawn } = await import('node:child_process');
    const { parseArgs } = await import('node:util');
    const { fileURLToPath } = await import('node:url');
    const { setTimeout: delay } = await import('node:timers/promises');
    const { values } = parseArgs({ options: {
        ui: { type: 'boolean' }, runtime: { type: 'string' }, helper: { type: 'string' },
        palette: { type: 'string' }, panel: { type: 'string' }, screenshot: { type: 'string' },
    } });
    for (const name of ['runtime', 'helper', 'palette'])
        if (!values[name] || !path.isAbsolute(values[name])) throw new Error(`--${name} must be an absolute path`);
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'clipboard-ui-'));
    const env = { ...process.env };
    for (const name of ['DISPLAY', 'WAYLAND_DISPLAY', 'WAYLAND_SOCKET', 'NIRI_SOCKET', 'NIRI_CONFIG',
        'DBUS_SESSION_BUS_ADDRESS', 'DBUS_SYSTEM_BUS_ADDRESS', 'XAUTHORITY', 'LD_PRELOAD',
        'GTK_THEME', 'GTK_PATH', 'GTK_MODULES']) delete env[name];
    for (const name of ['home', 'config', 'data', 'cache', 'runtime']) fs.mkdirSync(path.join(root, name), { mode: 0o700 });
    const source = path.dirname(fileURLToPath(import.meta.url));
    Object.assign(env, {
        HOME: path.join(root, 'home'), XDG_CONFIG_HOME: path.join(root, 'config'),
        XDG_DATA_HOME: path.join(root, 'data'), XDG_CACHE_HOME: path.join(root, 'cache'),
        XDG_RUNTIME_DIR: path.join(root, 'runtime'), XDG_CURRENT_DESKTOP: 'niri',
        GSETTINGS_BACKEND: 'memory', GTK_A11Y: 'none', GIO_USE_VFS: 'local',
        DBUS_SYSTEM_BUS_ADDRESS: `unix:path=${root}/no-system-bus`,
        CLIPBOARD_PANEL_CONFIG: path.join(source, 'frontend.js'),
        CLIPBOARD_PANEL_PALETTE: values.palette, CLIPBOARD_CUSTOM_HELPER: values.helper,
        ROFI_CLIPBOARD_DATA_DIR: path.join(root, 'history'),
    });
    if (values.screenshot) env.CLIPBOARD_TEST_SCREENSHOT = path.resolve(values.screenshot);
    const processes = [];
    const interrupted = new AbortController();
    const onInterrupt = () => interrupted.abort(new Error('Native tests interrupted'));
    process.on('SIGINT', onInterrupt);
    process.on('SIGTERM', onInterrupt);
    function start(command, args, options = {}) {
        const child = spawn(command, args, { env, detached: true, stdio: ['ignore', 'pipe', 'pipe', 'pipe'], ...options });
        const record = { child, log: '', done: null };
        record.done = new Promise(resolve => {
            child.on('error', error => { record.error = error; resolve({ code: -1 }); });
            child.on('exit', (code, signal) => resolve({ code, signal }));
        });
        child.stdout.on('data', bytes => { record.log += bytes; });
        child.stderr.on('data', bytes => { record.log += bytes; });
        processes.push(record);
        return record;
    }
    async function until(check, description) {
        const deadline = Date.now() + 15000;
        while (Date.now() < deadline) {
            interrupted.signal.throwIfAborted();
            const failed = processes.find(record => record.error);
            if (failed) throw failed.error;
            const value = check();
            if (value) return value;
            await delay(50);
        }
        throw new Error(`Timed out waiting for ${description}`);
    }
    async function finish(record) {
        let timer, abort;
        try {
            const result = await Promise.race([record.done, new Promise((_, reject) => {
                timer = setTimeout(() => reject(new Error('Native test timed out')), 30000);
                abort = () => reject(interrupted.signal.reason);
                interrupted.signal.addEventListener('abort', abort, { once: true });
            })]);
            if (result.code !== 0) throw new Error(record.log || `Process exited: ${JSON.stringify(result)}`);
            return record.log;
        } finally {
            clearTimeout(timer);
            if (abort) interrupted.signal.removeEventListener('abort', abort);
        }
    }
    function kill(record, signal) {
        if (!record.child.pid) return;
        try { process.kill(-record.child.pid, signal); }
        catch (error) { if (error.code !== 'ESRCH') throw error; }
    }
    try {
        const xvfb = start('Xvfb', ['-displayfd', '3', '-screen', '0', '1280x800x24', '-nolisten', 'tcp']);
        let display = '';
        xvfb.child.stdio[3].on('data', bytes => { display += bytes; });
        await until(() => display.includes('\n'), 'private Xvfb');
        env.DISPLAY = `:${display.trim()}`;
        const busConfig = path.join(root, 'bus.conf');
        // No servicedir: tests cannot activate a real desktop service.
        fs.writeFileSync(busConfig, `<busconfig><type>session</type><listen>unix:tmpdir=${root}</listen>
            <auth>EXTERNAL</auth><policy context="default"><allow send_destination="*"/>
            <allow receive_sender="*"/><allow own="*"/></policy></busconfig>`);
        const bus = start('dbus-daemon', ['--nofork', '--nopidfile', '--print-address=1', `--config-file=${busConfig}`]);
        env.DBUS_SESSION_BUS_ADDRESS = await until(() => bus.log.match(/unix:[^\n]+/)?.[0], 'private service-free D-Bus');
        const config = path.join(root, 'niri.kdl');
        fs.writeFileSync(config, 'hotkey-overlay {\n    skip-at-startup\n}\n');
        start('niri', ['--config', config]);
        const sockets = await until(() => {
            const files = fs.readdirSync(env.XDG_RUNTIME_DIR);
            const wayland = files.find(name => /^wayland-\d+$/.test(name));
            const niri = files.find(name => name.startsWith('niri.') && name.endsWith('.sock'));
            return wayland && niri ? { wayland, niri } : null;
        }, 'nested Niri Wayland sockets');
        env.WAYLAND_DISPLAY = sockets.wayland;
        env.NIRI_SOCKET = path.join(env.XDG_RUNTIME_DIR, sockets.niri);
        env.GDK_BACKEND = 'wayland';
        env.CLIPBOARD_PANEL_IMPORT_ONLY = '1';
        console.log(await finish(start(path.join(values.runtime, 'bin/panel-gjs'), ['-m', path.join(source, 'test.js'), 'native'])));
        delete env.CLIPBOARD_PANEL_IMPORT_ONLY;
        if (values.panel) {
            const launcher = path.join(values.panel, 'bin/clipboard-panel');
            start(launcher, ['run']);
            let ready = false;
            for (let attempt = 0; attempt < 30 && !ready; attempt++) {
                const status = start(launcher, ['status']);
                const result = await status.done;
                ready = result.code === 0;
                if (!ready) await delay(100);
            }
            if (!ready) throw new Error('Bundled clipboard-panel launcher did not become ready');
            await finish(start(launcher, ['toggle']));
            await finish(start(launcher, ['close']));
            console.log('PASS: packaged AGS bundle, request socket, status, toggle, and close');
        }
    } catch (error) {
        for (const record of processes) if (record.log) console.error(record.log);
        throw error;
    } finally {
        for (const record of processes.toReversed()) kill(record, 'SIGTERM');
        await Promise.race([Promise.allSettled(processes.map(record => record.done)), delay(2000)]);
        for (const record of processes) kill(record, 'SIGKILL');
        process.off('SIGINT', onInterrupt);
        process.off('SIGTERM', onInterrupt);
        fs.rmSync(root, { recursive: true, force: true });
    }
}

async function unitTests() {
    const { default: test } = await import('node:test');
    const { default: assert } = await import('node:assert/strict');
    const { ClipboardModel, visibleItems, title, preferred } = await import('./logic.js');
    const item = (id, kind = 'memo', text = `item ${id}`, pinned = false) =>
        ({ id, kind, text, pinned, mime: 'text/plain;charset=utf-8' });
    function fixture(items = [item(1), item(2), item(3, 'text')]) {
        const messages = [], publications = [];
        let next = 10;
        const request = async message => {
            messages.push(message);
            const current = items.find(item => item.id === message.id);
            switch (message.command) {
            case 'prepare': return { items };
            case 'snapshot': return { items: structuredClone(items) };
            case 'memo': {
                let draft = items.find(item => item.kind === 'memo' && !item.text);
                if (!draft) { draft = item(next++, 'memo', ''); items.push(draft); }
                return { id: draft.id };
            }
            case 'save':
                if (!current) throw new Error('Item disappeared');
                if (current.text !== message.base) throw new Error('External edit conflict');
                current.text = message.text;
                return { id: current.id };
            case 'pin': current.pinned = !current.pinned; return {};
            case 'delete': items.splice(items.indexOf(current), 1); return {};
            case 'payload': return { mime: current.mime, text: current.text };
            default: throw new Error('Unknown request');
            }
        };
        const model = new ClipboardModel({ request, publish: async payload => publications.push(payload) });
        return { model, messages, items, publications };
    }
    test('filtering preserves pinned order, draft row and Unicode content', () => {
        const items = [item(1), item(2, 'memo', '你好\nworld', true), item(3, 'memo', '')];
        assert.deepEqual(visibleItems(items, 'memo', '').map(item => item.id), [2, 1, 3]);
        assert.deepEqual(visibleItems(items, 'memo', '你好 WORLD').map(item => item.id), [2, 3]);
        assert.equal(title(item(1, 'file', '/home/test/a b'), '/home/test'), '~/a b');
        assert.equal(title(item(1, 'text', '<b>not markup</b>')), '<b>not markup</b>');
        assert.equal(preferred(items, 2).id, 2);
    });
    test('switching selection saves the original item, preserving exact text', async () => {
        const { model, items, messages } = fixture();
        await model.open();
        model.edit('\t 你好\n\ncomplete text  ');
        assert.equal(await model.select(2), true);
        assert.equal(items[0].text, '\t 你好\n\ncomplete text  ');
        assert.equal(messages.find(message => message.command === 'save').id, 1);
        assert.equal(model.editor.id, 2);
    });
    test('a conflicting save keeps the editor and prevents tab changes and copying', async () => {
        const { model, items, publications } = fixture();
        await model.open();
        model.edit('local draft');
        items[0].text = 'external';
        assert.equal(await model.changeTab('text'), false);
        assert.equal(model.tab, 'memo');
        assert.equal(model.editor.text, 'local draft');
        assert.equal(await model.copy(), false);
        assert.equal(publications.length, 0);
        assert.match(model.failure, /conflict/);
        await model.refresh();
        assert.match(model.failure, /conflict/);
        await model.reload();
        assert.equal(model.editor.text, 'external');
    });
    test('search does not save, discard or switch the dirty editor', async () => {
        const { model, messages } = fixture();
        await model.open();
        model.edit('unsaved');
        model.filter('item 2');
        assert.equal(model.editor.id, 1);
        assert.equal(model.editor.text, 'unsaved');
        assert.equal(messages.some(message => message.command === 'save'), false);
        assert.deepEqual(model.rows.filter(item => item.text).map(item => item.id), [2]);
    });
    test('external refresh preserves dirty text and fails safely if its item disappeared', async () => {
        const { model, items } = fixture();
        await model.open();
        model.edit('draft');
        items.splice(0, 1);
        await model.refresh();
        assert.equal(model.editor.id, 1);
        assert.equal(model.editor.text, 'draft');
        assert.equal(await model.save(), false);
        assert.match(model.failure, /disappeared/);
    });
    test('copy saves first and publishes the complete typed payload', async () => {
        const { model, publications } = fixture();
        await model.open();
        model.edit('copy me\n你好');
        assert.equal(await model.copy(), true);
        assert.deepEqual(publications, [{ mime: 'text/plain;charset=utf-8', text: 'copy me\n你好' }]);
    });
    test('deleting a draft deliberately discards it and selects a surviving neighbor', async () => {
        const { model, items, messages } = fixture();
        await model.open();
        model.edit('discard this');
        await model.remove();
        assert.equal(items.some(item => item.id === 1), false);
        assert.equal(model.editor.id, 2);
        assert.equal(messages.some(message => message.command === 'save'), false);
    });
    test('busy transactions reject duplicate mutations and defer file-watch refresh', async () => {
        const { model, messages } = fixture();
        await model.open();
        const original = model.request;
        let unblock;
        model.request = message => message.command === 'pin'
            ? new Promise(resolve => { unblock = () => resolve(original(message)); }) : original(message);
        const pin = model.pin();
        assert.equal(await model.remove(), false);
        assert.equal(await model.refresh(), false);
        unblock();
        assert.equal(await pin, true);
        assert.equal(model.pendingRefresh, false);
        assert.equal(messages.some(message => message.command === 'delete'), false);
    });
    test('new memo saves the previous draft and clears the search', async () => {
        const { model, items } = fixture();
        await model.open();
        model.edit('saved before new memo');
        model.filter('no match');
        await model.newMemo();
        assert.equal(items[0].text, 'saved before new memo');
        assert.equal(model.query, '');
        assert.equal(model.editor.text, '');
    });
    test('failed reload preserves the unsaved draft', async () => {
        const { model } = fixture();
        await model.open();
        model.edit('keep me');
        model.request = async () => { throw new Error('Read failed'); };
        assert.equal(await model.reload(), false);
        assert.equal(model.editor.text, 'keep me');
    });
    test('typing during a read-only background refresh is never dropped or replaced', async () => {
        const { model } = fixture();
        await model.open();
        const original = model.request;
        let release;
        model.request = message => new Promise(resolve => { release = () => resolve(original(message)); });
        const refreshing = model.refresh();
        assert.equal(model.busy, true);
        model.edit('typed while reading');
        release();
        assert.equal(await refreshing, true);
        assert.equal(model.editor.text, 'typed while reading');
        assert.equal(model.dirty, true);
    });
}

async function nativeTests() {
    const { default: GLib } = await import('gi://GLib');
    const { default: Gtk } = await import('gi://Gtk?version=4.0');
    const { default: Gdk } = await import('gi://Gdk?version=4.0');
    const { default: Gio } = await import('gi://Gio');
    const { ClipboardPanel, createApplication, loadTheme } = await import('./frontend.js');
    const { ClipboardModel } = await import('./logic.js');
    const { ClipboardBackend } = await import('./backend.js');
    function assert(value, message) { if (!value) throw new Error(message); }
    const wait = milliseconds => new Promise(resolve => GLib.timeout_add(GLib.PRIORITY_DEFAULT, milliseconds,
        () => { resolve(); return GLib.SOURCE_REMOVE; }));
    const idle = async () => {
        for (let attempt = 0; attempt < 100 && model.busy; attempt++) await wait(10);
        assert(!model.busy, 'Background history refresh did not settle');
    };
    const externalRead = async mime => {
        const paste = Gio.Subprocess.new(['wl-paste', '--no-newline', '--type', mime],
            Gio.SubprocessFlags.STDOUT_PIPE | Gio.SubprocessFlags.STDERR_PIPE);
        return new Promise((resolve, reject) => paste.communicate_async(null, null, (source, result) => {
            try {
                const [, stdout, stderr] = source.communicate_finish(result);
                assert(source.get_successful(), `External Wayland clipboard read failed: ${new TextDecoder().decode(stderr.get_data())}`);
                resolve(stdout.get_data());
            } catch (error) { reject(error); }
        }));
    };
    Gtk.init();
    GLib.unsetenv('LD_PRELOAD');
    const root = GLib.getenv('ROFI_CLIPBOARD_DATA_DIR');
    if (!root || !root.includes('clipboard-ui-')) throw new Error('Native tests require --ui isolation.');
    GLib.mkdir_with_parents(`${root}/images`, 0o700);
    const png = GLib.base64_decode('iVBORw0KGgoAAAANSUhEUgAAACAAAAAgCAIAAAD8GO2jAAAAKklEQVR4nO3NsQkAAAgDsD7q/w84i1c4CIHsSfWcikAgEAgEAoFAIPgSLI/kPIhK7EgvAAAAAElFTkSuQmCC');
    GLib.file_set_contents(`${root}/images/3.png`, png);
    GLib.file_set_contents(`${root}/report one.txt`, 'file reference fixture');
    const fixture = (id, kind, text, extra = {}) => ({ id, kind, text, image_file: null,
        mime: 'text/plain;charset=utf-8', pinned: false, created_at: id, digest: `fixture-${id}`, ...extra });
    GLib.file_set_contents(`${root}/history.json`, JSON.stringify({ version: 1, next_id: 4, items: [
        fixture(1, 'text', '<not markup>\nA complete text entry'),
        fixture(2, 'file', `${root}/report one.txt`, { name: `${root}/report one.txt` }),
        fixture(3, 'file', null, { image_file: '3.png', mime: 'image/png' }),
    ] }));
    const app = createApplication();
    app.register(null);
    const palette = loadTheme(GLib.getenv('CLIPBOARD_PANEL_CONFIG'), GLib.getenv('CLIPBOARD_PANEL_PALETTE'));
    let panel;
    const backend = new ClipboardBackend({ onChange: () => void model.refresh() });
    const model = new ClipboardModel({ request: message => backend.request(message),
        publish: payload => backend.publish(payload), onChange: () => panel?.render() });
    panel = new ClipboardPanel({ app, model, backend, palette });
    try {
        const monitor = app.get_monitors()[0];
        assert(await panel.toggle(monitor), model.failure);
        assert(model.editor?.kind === 'memo', 'Memo draft was not created');
        await wait(250);
        model.edit('native GTK clipboard\n你好');
        assert(panel.saveButton.sensitive, 'Save button did not follow dirty state');
        assert(await model.save(), model.failure);
        assert(await model.copy(), model.failure);
        const text = await new Promise((resolve, reject) => {
            const clipboard = Gdk.Display.get_default().get_clipboard();
            clipboard.read_text_async(null, (source, result) => {
                try { resolve(source.read_text_finish(result)); } catch (error) { reject(error); }
            });
        });
        assert(text === 'native GTK clipboard\n你好', 'GDK clipboard text roundtrip failed');
        assert(await panel.close(), model.failure);
        assert(!panel.opened, 'Save and close did not hide the panel');
        assert(new TextDecoder().decode(await externalRead('text/plain;charset=utf-8')) === text,
            'External text clipboard read after hiding changed the contents');
        const [, bytes] = GLib.file_get_contents(`${GLib.getenv('ROFI_CLIPBOARD_DATA_DIR')}/history.json`);
        assert(new TextDecoder().decode(bytes).includes('native GTK clipboard'), 'Real helper did not persist the memo');
        assert(await panel.toggle(monitor), model.failure);
        await wait(100);
        await idle();
        assert(await model.changeTab('file'), model.failure);
        assert(await model.select(2), model.failure);
        assert(!panel.editor.editable, 'File references must be read-only');
        assert(await model.copy(), model.failure);
        const uri = await backend.request({ command: 'payload', id: 2 });
        assert(uri.mime === 'text/uri-list' && uri.text.endsWith('/report%20one.txt\n'), 'File URI policy was not preserved');
        assert(new TextDecoder().decode(await externalRead('text/uri-list')) === uri.text, 'Native file URI publication changed');
        assert(await model.select(3), model.failure);
        await wait(200);
        await idle();
        assert(panel.preview.visible_child_name === 'image' && panel.picture.get_paintable(), 'PNG preview did not load');
        assert(await model.copy(), model.failure);
        panel.hide();
        // Read through another Wayland client while our window is hidden.
        const copied = await externalRead('image/png');
        assert(GLib.base64_encode(copied) === GLib.base64_encode(png), 'Native image clipboard bytes changed after hiding');
        assert(await panel.toggle(monitor), model.failure);
        await model.changeTab('memo');
        await wait(250);
        if (GLib.getenv('CLIPBOARD_TEST_SCREENSHOT')) {
            const capture = Gio.Subprocess.new(['niri', 'msg', 'action', 'screenshot-screen',
                '--path', GLib.getenv('CLIPBOARD_TEST_SCREENSHOT')], Gio.SubprocessFlags.NONE);
            await new Promise((resolve, reject) => capture.wait_check_async(null, (source, result) => {
                try { source.wait_check_finish(result); resolve(); } catch (error) { reject(error); }
            }));
        }
        print('PASS: nested Wayland layer-shell, GTK/CSS, editor, Rust persistence, tabs, file URI policy, PNG preview, and cross-client clipboard bytes after hiding');
    } finally {
        backend.dispose();
        panel.dispose();
    }
}

if (typeof process !== 'undefined' && process.versions?.node) {
    if (process.argv.includes('--ui')) await isolatedUI();
    else await unitTests();
}
else await nativeTests();
