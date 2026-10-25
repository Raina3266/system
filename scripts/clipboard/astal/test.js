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
        benchmark: { type: 'boolean' },
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
        CLIPBOARD_DATA_DIR: path.join(root, 'history'),
    });
    if (values.screenshot) env.CLIPBOARD_TEST_SCREENSHOT = path.resolve(values.screenshot);
    if (values.benchmark) env.CLIPBOARD_TEST_BENCHMARK = '1';
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
    const { ClipboardModel, localFilePath, visibleItems, title, preferred } = await import('./logic.js');
    const item = (id, kind = 'memo', text = `item ${id}`, pinned = false) =>
        ({ id, kind, text, pinned, mime: 'text/plain;charset=utf-8' });
    function fixture(items = [item(1), item(2), item(3, 'text')], options = {}) {
        const messages = [], publications = [];
        let next = 10;
        const snapshot = () => ({ items: structuredClone(items) });
        const memo = () => {
            let draft = items.find(item => item.kind === 'memo' && !item.text);
            if (!draft) { draft = item(next++, 'memo', ''); items.push(draft); }
            return draft;
        };
        const request = async message => {
            messages.push(message);
            const current = items.find(item => item.id === message.id);
            switch (message.command) {
            case 'prepare': memo(); return snapshot();
            case 'snapshot': return snapshot();
            case 'memo': {
                return { id: memo().id, ...snapshot() };
            }
            case 'save':
                if (!current) throw new Error('Item disappeared');
                if (current.text !== message.base) throw new Error('External edit conflict');
                current.text = message.text;
                if (current.kind === 'memo') memo();
                return { id: current.id, ...snapshot() };
            case 'pin': current.pinned = !current.pinned; return snapshot();
            case 'delete':
                items.splice(items.indexOf(current), 1);
                if (current.kind === 'memo') memo();
                return snapshot();
            case 'payload': return { mime: current.mime, text: current.text };
            default: throw new Error('Unknown request');
            }
        };
        const model = new ClipboardModel({ request, publish: async payload => publications.push(payload), ...options });
        return { model, messages, items, publications };
    }
    function autosaveScheduler() {
        const tasks = [];
        return {
            tasks,
            schedule: callback => {
                const task = { callback, cancelled: false, fired: false };
                tasks.push(task);
                return () => { task.cancelled = true; };
            },
            get pending() { return tasks.filter(task => !task.cancelled && !task.fired); },
            fire(task = this.pending[0]) {
                assert.ok(task, 'expected a scheduled autosave');
                if (task.cancelled || task.fired) return;
                task.fired = true;
                return task.callback();
            },
        };
    }
    for (const kind of ['memo', 'text']) {
        test(`${kind} preview autosave debounces edits and preserves exact Unicode and whitespace`, async () => {
            const scheduler = autosaveScheduler();
            const { model, messages, items } = fixture([item(1, kind)], { scheduleAutosave: scheduler.schedule });
            await model.open();
            await model.changeTab(kind);
            await model.select(1);
            assert.equal(scheduler.tasks.length, 0);
            messages.length = 0;
            model.edit('intermediate');
            const first = scheduler.pending[0];
            const text = '\t 你好 😀 e\u0301\r\n\n\u00a0complete text  \t\n';
            model.edit(text);
            assert.equal(first.cancelled, true);
            assert.equal(scheduler.tasks.length, 2);
            assert.equal(scheduler.pending.length, 1);
            assert.deepEqual(messages, []);
            await scheduler.fire(first);
            assert.deepEqual(messages, []);
            assert.equal(await scheduler.fire(), true);
            assert.deepEqual(messages, [{ command: 'save', id: 1, base: 'item 1', text }]);
            assert.equal(items.find(item => item.id === 1).text, text);
            assert.equal(model.editor.text, text);
            assert.equal(model.editor.base, text);
            assert.equal(model.dirty, false);
            assert.equal(scheduler.pending.length, 0);
        });
    }
    test('typing during a slow autosave is preserved and the next save uses the committed base', async () => {
        const scheduler = autosaveScheduler();
        const { model, messages, items } = fixture(undefined, { scheduleAutosave: scheduler.schedule });
        await model.open();
        await model.select(1);
        messages.length = 0;
        const request = model.request;
        const saves = [];
        let release;
        model.request = message => {
            if (message.command === 'save') {
                saves.push({ ...message });
                if (saves.length === 1)
                    return new Promise(resolve => { release = () => resolve(request(message)); });
            }
            return request(message);
        };
        const first = '\tfirst saved version 你好\n';
        const latest = `${first}\nnew typing 😀  `;
        model.edit(first);
        const saving = scheduler.fire();
        assert.equal(model.busy, true);
        assert.equal(model.activity, 'save');
        model.edit(latest);
        assert.equal(model.editor.text, latest);
        assert.equal(await model.select(2), false);
        assert.equal(await model.changeTab('text'), false);
        assert.equal(await model.remove(), false);
        assert.equal(await model.pin(), false);
        assert.equal(await model.copy(), false);
        await scheduler.fire();
        assert.equal(model.pendingAutosave, true);
        assert.equal(saves.length, 1);
        release();
        assert.equal(await saving, true);
        assert.equal(model.busy, false);
        assert.equal(model.editor.id, 1);
        assert.equal(model.editor.base, first);
        assert.equal(model.editor.text, latest);
        assert.equal(model.dirty, true);
        assert.equal(items[0].text, first);
        assert.equal(scheduler.pending.length, 1);
        assert.equal(await scheduler.fire(), true);
        assert.deepEqual(saves, [
            { command: 'save', id: 1, base: 'item 1', text: first },
            { command: 'save', id: 1, base: first, text: latest },
        ]);
        assert.deepEqual(messages, saves);
        assert.equal(items[0].text, latest);
        assert.equal(model.editor.base, latest);
        assert.equal(model.editor.text, latest);
        assert.equal(model.dirty, false);
        assert.equal(scheduler.pending.length, 0);
        assert.equal(model.pendingAutosave, false);
    });
    for (const deferred of [false, true]) {
        const state = deferred ? 'deferred' : 'scheduled';
        test(`disposing during a slow autosave prevents ${state} newer typing from scheduling again`, async () => {
            const scheduler = autosaveScheduler();
            const { model, messages, items } = fixture(undefined, { scheduleAutosave: scheduler.schedule });
            await model.open();
            await model.select(1);
            messages.length = 0;
            const request = model.request;
            let release;
            model.request = message => message.command === 'save' && !release
                ? new Promise(resolve => { release = () => resolve(request(message)); }) : request(message);
            const first = 'version already being saved';
            const latest = '\tnewer typing before disposal 你好\n';
            model.edit(first);
            const saving = scheduler.fire();
            assert.equal(model.activity, 'save');
            model.edit(latest);
            const timer = scheduler.pending[0];
            if (deferred) {
                await scheduler.fire(timer);
                assert.equal(model.pendingAutosave, true);
            }
            const scheduled = scheduler.tasks.length;
            model.dispose();
            assert.equal(model.scheduleAutosave, null);
            assert.equal(model.pendingAutosave, false);
            assert.equal(scheduler.pending.length, 0);
            if (!deferred) assert.equal(timer.cancelled, true);
            release();
            assert.equal(await saving, true);
            assert.equal(model.busy, false);
            assert.equal(items[0].text, first);
            assert.equal(model.editor.base, first);
            assert.equal(model.editor.text, latest);
            assert.equal(model.dirty, true);
            assert.equal(model.pendingAutosave, false);
            assert.equal(scheduler.pending.length, 0);
            assert.equal(scheduler.tasks.length, scheduled);
            await scheduler.fire(timer);
            model.edit('even a later edit must not rearm a disposed model');
            model.dispose();
            assert.equal(scheduler.tasks.length, scheduled);
            assert.equal(scheduler.pending.length, 0);
            assert.equal(model.pendingAutosave, false);
            assert.deepEqual(messages, [{ command: 'save', id: 1, base: 'item 1', text: first }]);
        });
        test(`a queued refresh failure after a successful save preserves ${state} autosave for newer typing`, async () => {
            const scheduler = autosaveScheduler();
            const { model, items } = fixture(undefined, { scheduleAutosave: scheduler.schedule });
            await model.open();
            await model.select(1);
            const request = model.request;
            const requests = [];
            let release;
            model.request = message => {
                requests.push({ ...message });
                if (message.command === 'snapshot') return Promise.reject(new Error('Queued snapshot failed'));
                if (message.command === 'save' && !release)
                    return new Promise(resolve => { release = () => resolve(request(message)); });
                return request(message);
            };
            const first = 'successfully committed first version';
            const latest = '\tnewer typing survives refresh failure 😀\n';
            model.edit(first);
            const saving = scheduler.fire();
            assert.equal(model.activity, 'save');
            model.edit(latest);
            const timer = scheduler.pending[0];
            if (deferred) {
                await scheduler.fire(timer);
                assert.equal(model.pendingAutosave, true);
            }
            assert.equal(await model.refresh(), false);
            assert.equal(model.pendingRefresh, true);
            release();
            assert.equal(await saving, false);
            assert.match(model.failure, /Queued snapshot failed/);
            assert.equal(model.busy, false);
            assert.equal(model.pendingRefresh, false);
            assert.equal(items[0].text, first);
            assert.equal(model.editor.id, 1);
            assert.equal(model.editor.base, first);
            assert.equal(model.editor.text, latest);
            assert.equal(model.dirty, true);
            assert.deepEqual(requests, [
                { command: 'save', id: 1, base: 'item 1', text: first },
                { command: 'snapshot' },
            ]);
            assert.equal(model.pendingAutosave, false);
            assert.equal(scheduler.pending.length, 1);
            if (!deferred) {
                assert.equal(timer.cancelled, false);
                assert.equal(scheduler.pending[0], timer);
            }
            assert.equal(await scheduler.fire(), true);
            assert.deepEqual(requests[2], { command: 'save', id: 1, base: first, text: latest });
            assert.equal(requests.length, 3);
            assert.equal(items[0].text, latest);
            assert.equal(model.editor.base, latest);
            assert.equal(model.editor.text, latest);
            assert.equal(model.dirty, false);
            assert.equal(model.failure, '');
            assert.equal(model.pendingAutosave, false);
            assert.equal(scheduler.pending.length, 0);
        });
    }
    test('undoing to the original base during a slow autosave saves the restored text against the new base', async () => {
        const scheduler = autosaveScheduler();
        const { model, messages, items } = fixture(undefined, { scheduleAutosave: scheduler.schedule });
        await model.open();
        await model.select(1);
        messages.length = 0;
        const original = model.editor.base;
        const first = '\tversion already being saved 你好\n';
        const request = model.request;
        let release;
        model.request = message => message.command === 'save' && !release
            ? new Promise(resolve => { release = () => resolve(request(message)); }) : request(message);
        model.edit(first);
        const saving = scheduler.fire();
        assert.equal(model.activity, 'save');
        model.edit('another edit before undo');
        const timer = scheduler.pending[0];
        model.edit(original);
        assert.equal(timer.cancelled, true);
        assert.equal(model.editor.text, original);
        assert.equal(model.dirty, false);
        assert.equal(scheduler.pending.length, 0);
        assert.equal(model.pendingAutosave, false);
        release();
        assert.equal(await saving, true);
        assert.equal(items[0].text, first);
        assert.equal(model.editor.text, original);
        assert.equal(model.editor.base, first);
        assert.equal(model.dirty, true);
        assert.equal(scheduler.pending.length, 1);
        assert.equal(await scheduler.fire(), true);
        assert.deepEqual(messages, [
            { command: 'save', id: 1, base: original, text: first },
            { command: 'save', id: 1, base: first, text: original },
        ]);
        assert.equal(items[0].text, original);
        assert.equal(model.editor.text, original);
        assert.equal(model.editor.base, original);
        assert.equal(model.dirty, false);
        assert.equal(scheduler.pending.length, 0);
        assert.equal(model.pendingAutosave, false);
    });
    test('autosave due during a background refresh waits for the refresh and preserves typing', async () => {
        const scheduler = autosaveScheduler();
        const { model, messages } = fixture(undefined, { scheduleAutosave: scheduler.schedule });
        await model.open();
        await model.select(1);
        messages.length = 0;
        const request = model.request;
        let release;
        model.request = message => message.command === 'snapshot'
            ? new Promise(resolve => { release = () => resolve(request(message)); }) : request(message);
        const refreshing = model.refresh();
        assert.equal(model.activity, 'refresh');
        model.edit('typed while refreshing\n你好');
        await scheduler.fire();
        assert.equal(model.pendingAutosave, true);
        assert.equal(scheduler.pending.length, 0);
        assert.deepEqual(messages, []);
        release();
        assert.equal(await refreshing, true);
        assert.equal(model.busy, false);
        assert.equal(model.editor.text, 'typed while refreshing\n你好');
        assert.equal(model.editor.base, 'item 1');
        assert.equal(model.dirty, true);
        assert.equal(model.pendingAutosave, false);
        assert.equal(scheduler.pending.length, 1);
        assert.equal(await scheduler.fire(), true);
        assert.deepEqual(messages, [
            { command: 'snapshot' },
            { command: 'save', id: 1, base: 'item 1', text: 'typed while refreshing\n你好' },
        ]);
        assert.equal(model.dirty, false);
        assert.equal(scheduler.pending.length, 0);
    });
    for (const failure of ['conflict', 'disk']) {
        test(`autosave ${failure} failures preserve the draft and error without automatic retries`, async () => {
            const scheduler = autosaveScheduler();
            const { model, items } = fixture(undefined, { scheduleAutosave: scheduler.schedule });
            await model.open();
            await model.select(1);
            const request = model.request;
            let attempts = 0;
            model.request = message => {
                if (message.command === 'save') {
                    attempts++;
                    if (failure === 'disk') return Promise.reject(new Error('Disk write failed'));
                }
                return request(message);
            };
            model.edit('\tlocal unsaved draft 你好\n');
            if (failure === 'conflict') items[0].text = 'external edit';
            assert.equal(await scheduler.fire(), false);
            assert.equal(model.editor.id, 1);
            assert.equal(model.editor.base, 'item 1');
            assert.equal(model.editor.text, '\tlocal unsaved draft 你好\n');
            assert.equal(model.dirty, true);
            assert.match(model.failure, failure === 'conflict' ? /conflict/i : /Disk write failed/);
            const error = model.failure;
            assert.equal(model.busy, false);
            assert.equal(model.pendingAutosave, false);
            assert.equal(scheduler.pending.length, 0);
            assert.equal(await model.refresh(), true);
            model.filter('search without retrying');
            assert.equal(model.failure, error);
            assert.equal(model.editor.text, '\tlocal unsaved draft 你好\n');
            assert.equal(model.dirty, true);
            assert.equal(attempts, 1);
            assert.equal(scheduler.pending.length, 0);
            model.edit('another edit permits one new attempt');
            assert.equal(scheduler.pending.length, 1);
            assert.equal(await scheduler.fire(), false);
            assert.equal(attempts, 2);
            assert.equal(model.editor.text, 'another edit permits one new attempt');
            assert.equal(model.dirty, true);
            assert.equal(scheduler.pending.length, 0);
            assert.equal(model.pendingAutosave, false);
        });
    }
    test('a failed slow autosave cancels deferred retries and preserves newer typing', async () => {
        const scheduler = autosaveScheduler();
        const { model } = fixture(undefined, { scheduleAutosave: scheduler.schedule });
        await model.open();
        await model.select(1);
        const request = model.request;
        let rejectSave, attempts = 0;
        model.request = message => {
            if (message.command !== 'save') return request(message);
            attempts++;
            return new Promise((resolve, reject) => { rejectSave = reject; });
        };
        model.edit('first draft');
        const saving = scheduler.fire();
        model.edit('newer draft while saving');
        await scheduler.fire();
        assert.equal(model.pendingAutosave, true);
        rejectSave(new Error('Disk full'));
        assert.equal(await saving, false);
        assert.equal(model.editor.base, 'item 1');
        assert.equal(model.editor.text, 'newer draft while saving');
        assert.equal(model.dirty, true);
        assert.match(model.failure, /Disk full/);
        assert.equal(model.pendingAutosave, false);
        assert.equal(scheduler.pending.length, 0);
        assert.equal(attempts, 1);
        model.edit('edited after failure');
        assert.equal(scheduler.pending.length, 1);
        model.cancelAutosave();
    });
    for (const action of ['save', 'select', 'remove']) {
        test(`${action} cancels the old autosave timer without saving a stale target`, async () => {
            const scheduler = autosaveScheduler();
            const { model, messages, items } = fixture(undefined, { scheduleAutosave: scheduler.schedule });
            await model.open();
            await model.select(1);
            messages.length = 0;
            model.edit('pending draft');
            const timer = scheduler.pending[0];
            const request = model.request;
            model.request = message => {
                if (message.command === 'save') assert.equal(timer.cancelled, true);
                return request(message);
            };
            assert.equal(await (action === 'select' ? model.select(2) : model[action]()), true);
            assert.equal(timer.cancelled, true);
            assert.equal(scheduler.pending.length, 0);
            assert.equal(model.pendingAutosave, false);
            assert.equal(model.dirty, false);
            assert.deepEqual(messages, action === 'remove'
                ? [{ command: 'delete', id: 1 }]
                : [{ command: 'save', id: 1, base: 'item 1', text: 'pending draft' }]);
            if (action === 'remove') assert.equal(items.some(item => item.id === 1), false);
            else assert.equal(items[0].text, 'pending draft');
            const id = action === 'save' ? 1 : 2;
            assert.equal(model.editor.id, id);
            await scheduler.fire(timer);
            assert.equal(messages.length, 1);
            const base = model.editor.base;
            model.edit('next target draft');
            assert.equal(await scheduler.fire(), true);
            assert.deepEqual(messages[1], { command: 'save', id, base, text: 'next target draft' });
            assert.equal(model.dirty, false);
            assert.equal(scheduler.pending.length, 0);
        });
    }
    test('undoing edits back to the saved base cancels autosave without writing', async () => {
        const scheduler = autosaveScheduler();
        const { model, messages } = fixture(undefined, { scheduleAutosave: scheduler.schedule });
        await model.open();
        await model.select(1);
        messages.length = 0;
        model.edit('temporary change');
        const timer = scheduler.pending[0];
        model.edit(model.editor.base);
        assert.equal(model.dirty, false);
        assert.equal(timer.cancelled, true);
        assert.equal(scheduler.pending.length, 0);
        assert.equal(model.pendingAutosave, false);
        await scheduler.fire(timer);
        assert.deepEqual(messages, []);
    });
    test('no selection and read-only file previews never schedule autosave', async () => {
        const scheduler = autosaveScheduler();
        const { model, messages } = fixture([{ ...item(1, 'file'), name: '/tmp/source.txt' }],
            { scheduleAutosave: scheduler.schedule });
        await model.open();
        model.edit('no selected target');
        assert.equal(model.editor, null);
        await model.changeTab('file');
        await model.select(1);
        const editor = { ...model.editor };
        messages.length = 0;
        model.edit('must not edit the source file');
        assert.deepEqual(model.editor, editor);
        assert.equal(model.dirty, false);
        assert.equal(scheduler.tasks.length, 0);
        assert.deepEqual(messages, []);
    });
    test('cancelAutosave cleans up timers and deferred work without discarding the draft', async () => {
        const scheduler = autosaveScheduler();
        const { model, messages } = fixture(undefined, { scheduleAutosave: scheduler.schedule });
        await model.open();
        await model.select(1);
        messages.length = 0;
        model.edit('keep on cleanup');
        const timer = scheduler.pending[0];
        model.cancelAutosave();
        model.cancelAutosave();
        assert.equal(timer.cancelled, true);
        assert.equal(scheduler.pending.length, 0);
        assert.equal(model.pendingAutosave, false);
        assert.equal(model.editor.text, 'keep on cleanup');
        assert.equal(model.dirty, true);
        await scheduler.fire(timer);
        assert.deepEqual(messages, []);
        const request = model.request;
        let release;
        model.request = message => message.command === 'snapshot'
            ? new Promise(resolve => { release = () => resolve(request(message)); }) : request(message);
        const refreshing = model.refresh();
        model.edit('keep deferred draft on cleanup');
        await scheduler.fire();
        assert.equal(model.pendingAutosave, true);
        model.cancelAutosave();
        assert.equal(model.pendingAutosave, false);
        release();
        assert.equal(await refreshing, true);
        assert.equal(scheduler.pending.length, 0);
        assert.equal(model.editor.text, 'keep deferred draft on cleanup');
        assert.equal(model.dirty, true);
        assert.deepEqual(messages, [{ command: 'snapshot' }]);
        model.edit('a later edit rearms autosave');
        assert.equal(scheduler.pending.length, 1);
        assert.equal(await scheduler.fire(), true);
        assert.equal(model.dirty, false);
    });
    test('filtering preserves pinned order, draft row and Unicode content', () => {
        const items = [item(1), item(2, 'memo', '你好\nworld', true), item(3, 'memo', '')];
        assert.deepEqual(visibleItems(items, 'memo', '').map(item => item.id), [2, 1, 3]);
        assert.deepEqual(visibleItems(items, 'memo', '你好 WORLD').map(item => item.id), [2, 3]);
        assert.equal(title(item(1, 'file', '/home/test/a b'), '/home/test'), '~/a b');
        assert.equal(title(item(1, 'text', '<b>not markup</b>')), '<b>not markup</b>');
        assert.equal(preferred(items, 2).id, 2);
    });
    test('opening leaves pinned and unpinned history unselected, including after refresh', async () => {
        for (const entries of [[item(1), item(2, 'memo', 'pinned', true)],
            [item(1, 'memo', 'pinned', true)]]) {
            const { model, items } = fixture(entries);
            assert.equal(await model.open(), true);
            assert.equal(model.editor, null);
            assert.equal(Boolean(model.dirty), false);
            model.filter('pinned');
            assert.equal(await model.refresh(), true);
            assert.equal(model.editor, null);
            items.push(item(4, 'memo', 'pinned new arrival'));
            assert.equal(await model.refresh(), true);
            assert.equal(model.editor, null);
            assert.equal(await model.copy(), false);
            assert.equal(await model.pin(), false);
            assert.equal(await model.remove(), false);
        }
    });
    test('reopening clears a prior selection while preserving explicitly edited text', async () => {
        const { model, items } = fixture();
        await model.open();
        assert.equal(await model.select(1), true);
        model.edit('save before reopening');
        assert.equal(await model.open(), true);
        assert.equal(items[0].text, 'save before reopening');
        assert.equal(model.editor, null);
        assert.equal(await model.select(2), true);
        assert.equal(await model.refresh(), true);
        assert.equal(model.editor.id, 2);
        assert.equal(await model.open(), true);
        assert.equal(model.editor, null);
    });
    test('closing and reopening reloads externally changed content without selecting it', async () => {
        const { model, items, messages } = fixture();
        await model.open();
        await model.select(1);
        assert.equal(await model.save(), true);
        items[0].text = 'updated while the panel was closed';
        messages.length = 0;
        assert.equal(await model.open(), true);
        assert.deepEqual(messages.map(message => message.command), ['prepare']);
        assert.equal(model.editor, null);
        assert.equal(await model.select(1), true);
        assert.equal(model.editor.text, 'updated while the panel was closed');
    });
    test('reopening from Text or Files always returns to Memo without selecting an item', async () => {
        const { model } = fixture([item(1, 'text'), item(2, 'file')]);
        await model.open();
        for (const tab of ['text', 'file']) {
            assert.equal(await model.changeTab(tab), true);
            assert.equal(await model.open(), true);
            assert.equal(model.tab, 'memo');
            assert.equal(model.editor, null);
            assert.equal(model.rows.every(item => item.kind === 'memo'), true);
            assert.equal(await model.refresh(), true);
            assert.equal(model.tab, 'memo');
            assert.equal(model.editor, null);
        }
    });
    test('a failed save on reopen preserves the selected unsaved draft', async () => {
        const { model, items } = fixture();
        await model.open();
        await model.select(1);
        model.edit('keep this draft');
        items[0].text = 'external edit';
        assert.equal(await model.open(), false);
        assert.equal(model.editor.id, 1);
        assert.equal(model.editor.text, 'keep this draft');
        assert.match(model.failure, /conflict/);
    });
    test('switching selection saves the original item, preserving exact text', async () => {
        const { model, items, messages } = fixture();
        await model.open();
        await model.select(1);
        model.edit('\t 你好\n\ncomplete text  ');
        assert.equal(await model.select(2), true);
        assert.equal(items[0].text, '\t 你好\n\ncomplete text  ');
        assert.equal(messages.find(message => message.command === 'save').id, 1);
        assert.equal(model.editor.id, 2);
    });
    test('a conflicting save keeps the editor and prevents tab changes and copying', async () => {
        const { model, items, publications } = fixture();
        await model.open();
        await model.select(1);
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
        model.edit(model.editor.base);
        assert.equal(await model.open(), true);
        assert.equal(model.editor, null);
        assert.equal(await model.select(1), true);
        assert.equal(model.editor.text, 'external');
    });
    test('search does not save, discard or switch the dirty editor', async () => {
        const { model, messages } = fixture();
        await model.open();
        await model.select(1);
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
        await model.select(1);
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
        await model.select(1);
        model.edit('copy me\n你好');
        assert.equal(await model.copy(), true);
        assert.deepEqual(publications, [{ mime: 'text/plain;charset=utf-8', text: 'copy me\n你好' }]);
    });
    test('deleting a draft deliberately discards it and selects a surviving neighbor', async () => {
        const { model, items, messages } = fixture();
        await model.open();
        await model.select(1);
        model.edit('discard this');
        await model.remove();
        assert.equal(items.some(item => item.id === 1), false);
        assert.equal(model.editor.id, 2);
        assert.equal(messages.some(message => message.command === 'save'), false);
    });
    test('local deletion targets only explicit absolute file names, including named images', () => {
        const entry = { ...item(1, 'file'), name: '/tmp/source file.png', image_path: '/cache/1.png' };
        assert.equal(localFilePath(entry), '/tmp/source file.png');
        for (const candidate of [null, item(1, 'file', '/tmp/not-a-file-reference'),
            { ...entry, kind: 'text' }, { ...entry, name: 'relative.txt' },
            { ...entry, name: 'file:///tmp/source.png' }, { ...entry, name: '/tmp/a\0b' }])
            assert.equal(localFilePath(candidate), null);
    });

    test('file deletion immediately trashes the source before removing history without confirmation', async () => {
        const { model, items, messages } = fixture([{ ...item(1, 'file'), name: '/tmp/file.txt' }, item(2, 'file')]);
        await model.open();
        await model.select(1);
        await model.changeTab('file');
        const order = [], request = model.request;
        model.request = async message => { order.push(message.command); return request(message); };

        model.trashFile = async path => { order.push('trash'); assert.equal(path, '/tmp/file.txt'); return true; };
        messages.length = 0;
        assert.equal(await model.remove(), true);
        assert.deepEqual(order, ['snapshot', 'trash', 'delete']);
        assert.equal(items.some(item => item.id === 1), false);
        assert.equal(model.editor.id, 2);
        assert.equal(messages.some(message => message.command === 'save'), false);
    });
    test('Trash failures preserve the file entry and expose the error', async () => {
        const { model, items, messages } = fixture([{ ...item(1, 'file'), name: '/tmp/file.txt' }]);
        await model.open();
        await model.select(1);
        await model.changeTab('file');
        model.trashFile = async () => { throw new Error('Trash is unavailable'); };
        assert.equal(await model.remove(), false);
        assert.match(model.failure, /Trash is unavailable/);
        assert.equal(model.editor.id, 1);
        assert.equal(items.some(item => item.id === 1), true);
        assert.equal(messages.some(message => message.command === 'delete'), false);
    });
    test('a stale file path is not deleted', async () => {
        const { model, items, messages } = fixture([{ ...item(1, 'file'), name: '/tmp/old.txt' }]);
        await model.open();
        await model.select(1);
        await model.changeTab('file');
        items[0].name = '/tmp/replacement.txt';
        model.trashFile = async () => assert.fail('A changed file reference must not touch disk');
        assert.equal(await model.remove(), false);
        assert.match(model.failure, /changed in clipboard history/);
        assert.equal(messages.some(message => message.command === 'delete'), false);
    });
    test('file deletion serializes selection, repeated Delete and background refresh', async () => {
        const { model, messages } = fixture([{ ...item(1, 'file'), name: '/tmp/file.txt' }, item(2, 'file')]);
        await model.open();
        await model.select(1);
        await model.changeTab('file');
        let finishTrash;
        model.trashFile = () => new Promise(resolve => { finishTrash = () => resolve(true); });
        const removing = model.remove();
        await new Promise(resolve => setImmediate(resolve));
        assert.equal(await model.select(2), false);
        assert.equal(await model.remove(), false);
        assert.equal(await model.refresh(), false);
        finishTrash();
        assert.equal(await removing, true);
        assert.equal(messages.filter(message => message.command === 'delete').length, 1);
        assert.equal(model.editor.id, 2);
        assert.equal(model.pendingRefresh, false);
    });
    test('history failure after trashing reports partial success and supports retrying a missing file', async () => {
        const { model, items } = fixture([{ ...item(1, 'file'), name: '/tmp/file.txt' }]);
        await model.open();
        await model.select(1);
        await model.changeTab('file');
        model.trashFile = async () => true;
        const request = model.request;
        model.request = async message => {
            if (message.command === 'delete') throw new Error('History is read-only');
            return request(message);
        };
        assert.equal(await model.remove(), false);
        assert.match(model.failure, /moved to Trash, but its clipboard entry could not be removed/);
        assert.equal(items.some(item => item.id === 1), true);
        model.request = request;
        model.trashFile = async () => false;
        assert.equal(await model.remove(), true);
        assert.equal(items.some(item => item.id === 1), false);
    });
    test('cached-only images and text entries never invoke local file deletion', async () => {
        const { model } = fixture([item(1, 'text', '/tmp/not-a-file-reference'),
            { ...item(2, 'file'), image_path: '/cache/2.png' }]);
        await model.open();
        await model.select(1);

        model.trashFile = async () => assert.fail('No source file means no filesystem mutation');
        for (const tab of ['text', 'file']) {
            assert.equal(await model.changeTab(tab), true);
            assert.equal(await model.remove(), true);
        }
    });
    test('busy transactions reject duplicate mutations and defer file-watch refresh', async () => {
        const { model, messages } = fixture();
        await model.open();
        await model.select(1);
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
        await model.select(1);
        model.edit('saved before new memo');
        model.filter('no match');
        await model.newMemo();
        assert.equal(items[0].text, 'saved before new memo');
        assert.equal(model.query, '');
        assert.equal(model.editor.text, '');
    });
    test('failed background refresh preserves the unsaved draft', async () => {
        const { model } = fixture();
        await model.open();
        await model.select(1);
        model.edit('keep me');
        model.request = async () => { throw new Error('Read failed'); };
        assert.equal(await model.refresh(), false);
        assert.equal(model.editor.text, 'keep me');
    });
    test('typing during a read-only background refresh is never dropped or replaced', async () => {
        const { model } = fixture();
        await model.open();
        await model.select(1);
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
    test('file invalidation prunes missing entries without changing the tab, search or unsaved text', async () => {
        const { model, items, messages } = fixture([item(1), { ...item(4, 'file'), name: '/tmp/missing.txt' }]);
        await model.open();
        await model.select(1);
        model.edit('keep this unsaved memo');
        model.filter('missing');
        const request = model.request;
        model.request = async message => {
            if (message.command === 'prepare') items.splice(items.findIndex(item => item.id === 4), 1);
            return request(message);
        };
        messages.length = 0;
        assert.equal(await model.refresh({ pruneMissing: true }), true);
        assert.deepEqual(messages.map(message => message.command), ['prepare']);
        assert.equal(model.items.some(item => item.id === 4), false);
        assert.equal(model.tab, 'memo');
        assert.equal(model.query, 'missing');
        assert.equal(model.editor.id, 1);
        assert.equal(model.editor.text, 'keep this unsaved memo');
        assert.equal(model.dirty, true);
    });
    test('file invalidation stays queued across a busy action and a later history-only refresh', async () => {
        const { model, messages } = fixture();
        await model.open();
        await model.select(1);
        const request = model.request;
        let finishPin;
        model.request = message => message.command === 'pin'
            ? new Promise(resolve => { finishPin = () => resolve(request(message)); }) : request(message);
        messages.length = 0;
        const pinning = model.pin();
        await new Promise(resolve => setImmediate(resolve));
        assert.equal(await model.refresh({ pruneMissing: true }), false);
        assert.equal(await model.refresh(), false);
        finishPin();
        assert.equal(await pinning, true);
        assert.deepEqual(messages.map(message => message.command), ['pin', 'prepare']);
        assert.equal(model.pendingRefresh, false);
        assert.equal(model.pendingPrune, false);
    });
    test('file invalidation arriving during a queued snapshot is not lost', async () => {
        const { model, messages } = fixture();
        await model.open();
        await model.select(1);
        const request = model.request;
        let finishPin, finishSnapshot;
        model.request = message => {
            if (message.command === 'pin') return new Promise(resolve => { finishPin = () => resolve(request(message)); });
            if (message.command === 'snapshot') return new Promise(resolve => { finishSnapshot = () => resolve(request(message)); });
            return request(message);
        };
        messages.length = 0;
        const pinning = model.pin();
        await new Promise(resolve => setImmediate(resolve));
        await model.refresh();
        finishPin();
        await new Promise(resolve => setImmediate(resolve));
        await model.refresh({ pruneMissing: true });
        finishSnapshot();
        assert.equal(await pinning, true);
        assert.deepEqual(messages.map(message => message.command), ['pin', 'snapshot', 'prepare']);
        assert.equal(model.pendingRefresh, false);
        assert.equal(model.pendingPrune, false);
    });
    test('a failed missing-file cleanup keeps the current history and draft intact', async () => {
        const { model } = fixture();
        await model.open();
        await model.select(1);
        model.edit('keep this draft');
        const items = model.items;
        model.request = async () => { throw new Error('Cannot read history'); };
        assert.equal(await model.refresh({ pruneMissing: true }), false);
        assert.equal(model.items, items);
        assert.equal(model.editor.text, 'keep this draft');
        assert.equal(model.dirty, true);
        assert.match(model.failure, /Cannot read history/);
    });
    test('clean tabs, selection and closing use the loaded history without helper calls', async () => {
        const { model, messages } = fixture();
        await model.open();
        await model.select(1);
        assert.deepEqual(messages.map(message => message.command), ['prepare']);
        messages.length = 0;
        for (const tab of ['text', 'file', 'memo', 'memo']) assert.equal(await model.changeTab(tab), true);
        await model.select(2);
        await model.select(2);
        await model.save();
        assert.deepEqual(messages, []);
    });
    test('mutations consume their committed snapshot instead of rereading history', async () => {
        const { model, messages } = fixture();
        await model.open();
        await model.select(1);
        messages.length = 0;
        model.edit('save while switching');
        await model.changeTab('text');
        assert.deepEqual(messages.map(message => message.command), ['save']);
        messages.length = 0;
        await model.pin();
        assert.deepEqual(messages.map(message => message.command), ['pin']);
        messages.length = 0;
        await model.changeTab('memo');
        await model.remove();
        assert.deepEqual(messages.map(message => message.command), ['delete']);
    });
    test('visible rows are cached across editor and activity-only changes', async () => {
        const { model } = fixture();
        await model.open();
        await model.select(1);
        const rows = model.rows;
        model.edit('draft');
        assert.equal(model.rows, rows);
        model.filter('');
        assert.equal(model.rows, rows);
        model.filter('item 2');
        assert.notEqual(model.rows, rows);
        const filtered = model.rows;
        assert.equal(model.rows, filtered);
        await model.refresh();
        assert.notEqual(model.rows, filtered);
        assert.equal(model.editor.text, 'draft');
    });
    test('row previews are Unicode-safe and bounded while search sees the complete note', () => {
        const text = `${'😀 '.repeat(10000)}needle at the end`;
        const entry = item(1, 'text', text);
        const preview = title(entry);
        assert(Array.from(preview).length <= 161);
        assert(preview.endsWith('…'));
        assert(!preview.includes('\ufffd'));
        assert.equal(visibleItems([entry], 'text', 'needle end').length, 1);
        assert.equal(entry.text, text);
    });
    rowActionTests(test, assert, ClipboardModel);
    await backendTests(test, assert);
}

function rowActionTests(test, assert, ClipboardModel) {
    const item = (id, kind = 'memo', text = `item ${id}`, extra = {}) =>
        ({ id, kind, text, pinned: false, mime: 'text/plain;charset=utf-8', ...extra });

    async function fixture(entries = [item(1), item(2), item(3, 'memo', 'pinned', { pinned: true })]) {
        const items = structuredClone(entries), messages = [], publications = [], events = [];
        const snapshot = () => ({ items: structuredClone(items) });
        const model = new ClipboardModel({
            request: async message => {
                messages.push(message);
                events.push(message.command);
                if (message.command === 'prepare') {
                    if (!items.some(entry => entry.kind === 'memo' && !entry.text)) items.push(item(100, 'memo', ''));
                    return snapshot();
                }
                if (message.command === 'snapshot') return snapshot();
                const current = items.find(entry => entry.id === message.id);
                if (!current) throw new Error('This clipboard item no longer exists.');
                switch (message.command) {
                case 'save':
                    if (current.text !== message.base) throw new Error('External edit conflict');
                    current.text = message.text;
                    return snapshot();
                case 'pin': current.pinned = !current.pinned; return snapshot();
                case 'delete': items.splice(items.indexOf(current), 1); return snapshot();
                case 'payload': return { mime: current.mime, text: current.text };
                default: throw new Error(`Unexpected command: ${message.command}`);
                }
            },
            publish: async payload => { events.push('publish'); publications.push(payload); },
            trashFile: async () => assert.fail('Unexpected source file deletion'),
        });
        await model.open();
        messages.length = 0;
        events.length = 0;
        return { model, items, messages, publications, events };
    }

    for (const action of ['pin', 'remove', 'copy']) {
        test(`unselected row ${action} targets its immutable ID after filtering and reordering`, async () => {
            const { model, items, messages, publications } = await fixture();
            assert.equal(model.tab, 'memo');
            assert.equal(model.editor, null);
            const id = model.rows.find(entry => entry.id === 2).id;
            items.reverse();
            items.find(entry => entry.id === 1).pinned = true;
            await model.refresh();
            model.filter('item 1');
            assert.equal(model.rows.some(entry => entry.id === id), false);
            messages.length = 0;

            assert.equal(await model[action](id), true);
            assert.deepEqual(messages, [{ command: { pin: 'pin', remove: 'delete', copy: 'payload' }[action], id }]);
            assert.equal(model.editor?.id ?? null, action === 'copy' ? id : null);
            assert.equal(items.find(entry => entry.id === 1).text, 'item 1');
            if (action === 'pin') assert.equal(items.find(entry => entry.id === id).pinned, true);
            if (action === 'remove') assert.equal(items.some(entry => entry.id === id), false);
            assert.deepEqual(publications, action === 'copy'
                ? [{ mime: 'text/plain;charset=utf-8', text: 'item 2' }] : []);
        });
    }

    test('selected-item defaults remain no-ops when opening on Memo without a selection', async () => {
        const { model, messages } = await fixture();
        for (const action of ['pin', 'remove', 'copy']) assert.equal(await model[action](), false);
        assert.equal(model.tab, 'memo');
        assert.equal(model.editor, null);
        assert.deepEqual(messages, []);
    });

    for (const dirty of [false, true]) {
        for (const target of [1, 2]) {
            test(`row pin preserves a filtered-out ${dirty ? 'dirty' : 'clean'} selection (target ${target})`, async () => {
                const { model, items, messages } = await fixture();
                await model.select(1);
                const text = '\t complete edited text\n你好  ';
                if (dirty) model.edit(text);
                model.filter('item 2');
                assert.equal(model.rows.some(entry => entry.id === 1), false);

                assert.equal(await model.pin(target), true);
                assert.equal(model.editor.id, 1);
                assert.equal(model.editor.text, dirty ? text : 'item 1');
                assert.equal(model.dirty, false);
                assert.equal(items.find(entry => entry.id === target).pinned, true);
                assert.deepEqual(messages, dirty
                    ? [{ command: 'save', id: 1, base: 'item 1', text }, { command: 'pin', id: target }]
                    : [{ command: 'pin', id: target }]);
                await model.refresh();
                assert.equal(model.editor.id, 1);
            });
        }
    }

    for (const dirty of [false, true]) {
        test(`deleting another row preserves the exact ${dirty ? 'dirty' : 'clean'} editor`, async () => {
            const { model, items, messages } = await fixture();
            await model.select(1);
            if (dirty) model.edit('never save or discard this draft');
            model.filter('item 2');
            const editor = model.editor;

            assert.equal(await model.remove(2), true);
            assert.equal(model.editor, editor);
            assert.equal(model.editor.id, 1);
            assert.equal(model.dirty, dirty);
            assert.equal(items.find(entry => entry.id === 1).text, 'item 1');
            assert.equal(items.some(entry => entry.id === 2), false);
            assert.deepEqual(messages, [{ command: 'delete', id: 2 }]);
        });
    }

    test('deleting the selected row explicitly discards edits and selects its ordered neighbor', async () => {
        const { model, items, messages } = await fixture();
        await model.select(1);
        model.edit('explicitly discard');
        assert.deepEqual(model.rows.map(entry => entry.id), [3, 1, 2, 100]);
        assert.equal(await model.remove(1), true);
        assert.equal(model.editor.id, 2);
        assert.equal(model.dirty, false);
        assert.equal(items.some(entry => entry.id === 1), false);
        assert.deepEqual(messages, [{ command: 'delete', id: 1 }]);
    });

    test('explicit copy saves the previous dirty item, then selects and publishes the exact row in one act', async () => {
        const { model, items, messages, publications, events } = await fixture();
        await model.select(1);
        const text = '\t previous dirty item\n你好  ';
        model.edit(text);
        model.filter('pinned');
        let acts = 0;
        const act = model.act.bind(model);
        model.act = (...args) => { acts++; return act(...args); };
        model.select = async () => assert.fail('Copy must not nest a selection transaction');
        model.publish = async payload => {
            assert.equal(model.busy, true);
            assert.equal(model.editor.id, 2);
            events.push('publish');
            publications.push(payload);
        };

        assert.equal(await model.copy(2), true);
        assert.equal(acts, 1);
        assert.equal(items.find(entry => entry.id === 1).text, text);
        assert.equal(model.editor.id, 2);
        assert.equal(model.editor.text, 'item 2');
        assert.equal(model.dirty, false);
        assert.deepEqual(messages, [
            { command: 'save', id: 1, base: 'item 1', text },
            { command: 'payload', id: 2 },
        ]);
        assert.deepEqual(events, ['save', 'payload', 'publish']);
        assert.deepEqual(publications, [{ mime: 'text/plain;charset=utf-8', text: 'item 2' }]);
    });

    for (const action of ['pin', 'remove', 'copy']) {
        test(`stale row IDs cannot ${action} a fallback item or save the dirty editor`, async () => {
            const { model, items, messages, publications } = await fixture();
            await model.select(1);
            model.edit('keep unsaved');
            const staleId = 2;
            items.splice(items.findIndex(entry => entry.id === staleId), 1);
            await model.refresh();
            model.filter('pinned');
            const editor = model.editor, before = structuredClone(items);
            messages.length = 0;

            for (const id of [staleId, 999, '1']) {
                assert.equal(await model[action](id), false);
                assert.match(model.failure, /no longer exists/);
                assert.equal(model.editor, editor);
                assert.equal(model.dirty, true);
                assert.equal(model.busy, false);
            }
            assert.deepEqual(messages, []);
            assert.deepEqual(items, before);
            assert.deepEqual(publications, []);
        });
    }

    for (const action of ['pin', 'copy']) {
        test(`${action} revalidates the target after committing the previous editor`, async () => {
            const { model, items, messages, publications } = await fixture();
            await model.select(1);
            model.edit('save this previous item');
            items.splice(items.findIndex(entry => entry.id === 2), 1);
            assert.equal(await model[action](2), false);
            assert.match(model.failure, /no longer exists/);
            assert.equal(model.editor.id, 1);
            assert.equal(items.find(entry => entry.id === 1).text, 'save this previous item');
            assert.deepEqual(messages.map(message => message.command), ['save']);
            assert.deepEqual(publications, []);
        });

        test(`a conflicting save prevents row ${action} and preserves selection and edits`, async () => {
            const { model, items, messages, publications } = await fixture();
            await model.select(1);
            model.edit('local dirty text');
            items.find(entry => entry.id === 1).text = 'external edit';
            assert.equal(await model[action](2), false);
            assert.match(model.failure, /conflict/);
            assert.equal(model.editor.id, 1);
            assert.equal(model.editor.text, 'local dirty text');
            assert.equal(model.dirty, true);
            assert.deepEqual(messages.map(message => message.command), ['save']);
            assert.deepEqual(publications, []);
        });
    }

    for (const action of ['pin', 'remove', 'copy']) {
        test(`busy row ${action} rejects other actions and serializes deferred refresh`, async () => {
            const { model, messages, publications } = await fixture();
            const request = model.request;
            const command = { pin: 'pin', remove: 'delete', copy: 'payload' }[action];
            let release;
            model.request = message => message.command === command
                ? new Promise(resolve => { release = () => resolve(request(message)); }) : request(message);
            const operation = model[action](2);
            await new Promise(resolve => setImmediate(resolve));
            assert.equal(model.busy, true);
            assert.equal(await model.pin(1), false);
            assert.equal(await model.remove(1), false);
            assert.equal(await model.copy(1), false);
            assert.equal(await model.select(1), false);
            assert.equal(await model.refresh(), false);
            assert.deepEqual(messages, []);
            release();
            assert.equal(await operation, true);
            assert.deepEqual(messages, [{ command, id: 2 }, { command: 'snapshot' }]);
            assert.equal(model.pendingRefresh, false);
            assert.equal(model.busy, false);
            assert.equal(model.editor?.id ?? null, action === 'copy' ? 2 : null);
            assert.equal(publications.length, action === 'copy' ? 1 : 0);
        });
    }

    test('row copy remains busy until publication completes', async () => {
        const { model, messages } = await fixture();
        let release;
        model.publish = () => new Promise(resolve => { release = resolve; });
        const copying = model.copy(2);
        await new Promise(resolve => setImmediate(resolve));
        assert.equal(model.busy, true);
        assert.equal(model.editor.id, 2);
        assert.equal(await model.remove(2), false);
        assert.equal(await model.copy(1), false);
        release();
        assert.equal(await copying, true);
        assert.equal(model.busy, false);
        assert.deepEqual(messages, [{ command: 'payload', id: 2 }]);
    });

    test('unselected file deletion immediately revalidates and trashes only the target source', async () => {
        const { model, items, messages, events } = await fixture([
            item(1, 'file', '', { name: '/tmp/other.txt' }),
            item(2, 'file', '', { name: '/tmp/target.png', image_path: '/cache/2.png' }),
        ]);
        model.trashFile = async path => {
            assert.equal(model.busy, true);
            assert.equal(path, '/tmp/target.png');
            events.push('trash');
            return true;
        };
        assert.equal(await model.remove(2), true);
        assert.deepEqual(events, ['snapshot', 'trash', 'delete']);
        assert.deepEqual(messages, [{ command: 'snapshot' }, { command: 'delete', id: 2 }]);
        assert.equal(items.some(entry => entry.id === 1), true);
        assert.equal(items.some(entry => entry.id === 2), false);
        assert.equal(model.editor, null);
    });

    test('row file deletion rejects a changed snapshot path without saving the dirty editor', async () => {
        const { model, items, messages } = await fixture([
            item(1), item(2, 'file', '', { name: '/tmp/old.txt' }),
        ]);
        await model.select(1);
        model.edit('keep dirty');
        const editor = model.editor;
        items.find(entry => entry.id === 2).name = '/tmp/replacement.txt';
        assert.equal(await model.remove(2), false);
        assert.match(model.failure, /changed in clipboard history/);
        assert.equal(model.editor, editor);
        assert.equal(model.dirty, true);
        assert.deepEqual(messages, [{ command: 'snapshot' }]);
    });

    test('row trash failures preserve both the target and another dirty editor', async () => {
        const { model, items, messages } = await fixture([
            item(1), item(2, 'file', '', { name: '/tmp/target.txt' }),
        ]);
        await model.select(1);
        model.edit('unsaved');
        const editor = model.editor;
        model.trashFile = async () => { throw new Error('Trash unavailable'); };
        assert.equal(await model.remove(2), false);
        assert.match(model.failure, /Trash unavailable/);
        assert.equal(model.editor, editor);
        assert.equal(model.dirty, true);
        assert.equal(items.some(entry => entry.id === 2), true);
        assert.deepEqual(messages, [{ command: 'snapshot' }]);
    });

    test('text rows and cached-only images never delete source files', async () => {
        const { model, messages } = await fixture([
            item(1, 'text', '/tmp/text-is-not-a-source.txt', { name: '/tmp/not-a-source.txt' }),
            item(2, 'file', '', { image_path: '/cache/2.png' }),
            item(3, 'memo', '/tmp/memo-is-not-a-source.txt', { name: '/tmp/not-a-source.txt' }),
        ]);
        for (const id of [1, 2, 3]) {
            assert.equal(await model.remove(id), true);
            assert.equal(model.editor, null);
        }
        assert.deepEqual(messages, [1, 2, 3].map(id => ({ command: 'delete', id })));
    });
}

async function backendTests(test, assert) {
    const { registerHooks } = await import("node:module");
    const { default: path } = await import("node:path/posix");

    let state;
    const events = Object.fromEntries(['CHANGED', 'CHANGES_DONE_HINT', 'DELETED', 'CREATED',
        'ATTRIBUTE_CHANGED', 'PRE_UNMOUNT', 'UNMOUNTED', 'MOVED', 'RENAMED', 'MOVED_IN', 'MOVED_OUT']
        .map((name, index) => [name, index]));
    const flags = { NONE: 0, WATCH_MOVES: 8 };

    class MockFile {
        constructor(name) { this.path = path.resolve(name); }
        get_path() { return this.path; }
        get_basename() { return path.basename(this.path); }
        get_parent() { return this.path === '/' ? null : new MockFile(path.dirname(this.path)); }
        monitor_directory(flag, cancellable) {
            state.attempts.push({ path: this.path, flag, cancellable });
            if (state.failures.has(this.path)) throw new Error('Private path must not be logged');
            const monitor = {
                path: this.path,
                cancellations: 0,
                connect(signal, callback) {
                    assert.equal(signal, 'changed');
                    this.callback = callback;
                    if (state.connectFailures.has(this.path)) throw new Error('Monitor connection failed');
                },
                cancel() { this.cancellations++; },
                // Also allow queued callbacks after cancel to exercise the lifecycle guard.
                emit(event, file = null, other = null) {
                    this.callback(this, file === null ? null : new MockFile(file),
                        other === null ? null : new MockFile(other), events[event]);
                },
            };
            state.monitors.push(monitor);
            return monitor;
        }
    }

    const gi = {
        'gi://Gio': {
            SubprocessFlags: { STDIN_PIPE: 1, STDOUT_PIPE: 2, STDERR_PIPE: 4 },
            Subprocess: { new: args => {
                const child = {
                    communicate_utf8_async(input, cancellable, callback) {
                        state.requests.push({ args, input: JSON.parse(input),
                            finish: () => callback(child, {}) });
                    },
                    communicate_utf8_finish() { return [true, JSON.stringify(state.reply)]; },
                    get_successful() { return true; },
                    force_exit() {},
                };
                return child;
            } },
            File: { new_for_path: name => { state.fileObjects++; return new MockFile(name); } },
            FileMonitorFlags: flags,
            FileMonitorEvent: events,
        },
        'gi://GLib': {
            PRIORITY_DEFAULT: 0,
            SOURCE_REMOVE: false,
            getenv: name => state.env[name] || null,
                        get_user_data_dir: () => '/data',
                        build_filenamev: parts => path.join(...parts),
            timeout_add(priority, delay, callback) {
                assert.equal(priority, 0);
                assert.ok([100, 10000].includes(delay));
                const id = ++state.nextTimer;
                state.timers.set(id, { due: state.now + delay, callback });
                return id;
            },
            source_remove(id) { assert.equal(state.timers.delete(id), true); },
        },
        'gi://Gdk?version=4.0': {},
    };
    const key = Symbol.for('clipboard.backend.test.GI');
    globalThis[key] = gi;
    const hooks = registerHooks({
        resolve(specifier, context, nextResolve) {
            return Object.hasOwn(gi, specifier)
                ? { url: specifier, shortCircuit: true } : nextResolve(specifier, context);
        },
        load(url, context, nextLoad) {
            return Object.hasOwn(gi, url) ? {
                format: 'module', shortCircuit: true,
                source: `export default globalThis[Symbol.for('clipboard.backend.test.GI')][${JSON.stringify(url)}];`,
            } : nextLoad(url, context);
        },
    });
    let ClipboardBackend, historyRoot;
    try {
        ({ ClipboardBackend, historyRoot } = await import('./backend.js'));
    } finally {
        hooks.deregister();
        delete globalThis[key];
    }

    const linked = name => ({ kind: 'file', name });
    function fixture(t) {
        state = { attempts: [], monitors: [], failures: new Set(), connectFailures: new Set(),
            fileObjects: 0, timers: new Map(), nextTimer: 0, now: 0,
            env: { CLIPBOARD_DATA_DIR: '/history' }, requests: [], reply: { items: [] } };
        const changes = [];
        const backend = new ClipboardBackend({ onChange: change => changes.push(change) });
        t.after(() => backend.dispose());
        const monitor = directory => state.monitors.findLast(entry => entry.path === directory);
        const advance = milliseconds => {
            const until = state.now + milliseconds;
            while (true) {
                const next = [...state.timers].sort((a, b) => a[1].due - b[1].due)[0];
                if (!next || next[1].due > until) break;
                state.now = next[1].due;
                state.timers.delete(next[0]);
                assert.equal(next[1].callback(), false);
            }
            state.now = until;
        };
        return { backend, changes, monitor, advance };
    }

    test('history paths use the new environment or XDG clipboard directory', t => {
        fixture(t);
        assert.equal(historyRoot(), '/history');
        state.env = {};
        assert.equal(historyRoot(), '/data/clipboard');
        state.env.CLIPBOARD_DATA_DIR = '';
        assert.equal(historyRoot(), '/data/clipboard');
    });

    test('helper discovery finishes before monitoring; the initial snapshot gap is reconciled once', async t => {
        const { backend, monitor, changes, advance } = fixture(t);
        const pending = backend.request({ command: 'prepare' });
        assert.equal(state.attempts.length, 0);
        assert.deepEqual(state.requests[0].args, ['clipboard-custom']);
        state.requests[0].finish();
        assert.deepEqual(await pending, { items: [] });
        assert.ok(monitor('/history'));
        assert.equal(state.timers.size, 1);
        advance(100);
        assert.deepEqual(changes, [{ pruneMissing: false }]);
        const snapshot = backend.request({ command: 'snapshot' });
        state.requests[1].finish();
        await snapshot;
        advance(100);
        assert.equal(state.attempts.length, 1);
        assert.equal(changes.length, 1);
    });

    test('failed helper discovery never attaches a monitor to an unmigrated path', async t => {
        const { backend } = fixture(t);
        state.reply = { error: 'Migration failed' };
        const pending = backend.request({ command: 'prepare' });
        state.requests[0].finish();
        await assert.rejects(pending, /Migration failed/);
        assert.equal(state.attempts.length, 0);
    });

    test('a failed history monitor connection is canceled and can be retried', t => {
        const { backend, monitor } = fixture(t);
        state.connectFailures.add('/history');
        assert.doesNotThrow(() => backend.watch());
        assert.equal(monitor('/history').cancellations, 1);
        assert.equal(backend.monitor, undefined);
        state.connectFailures.clear();
        backend.watch();
        assert.ok(backend.monitor);
    });

    for (const failure of ['failures', 'connectFailures']) {
        test(`monitor ${failure} cannot reject a committed save and discovery is retried`, async t => {
            const { backend, changes, advance } = fixture(t);
            state[failure].add('/history');
            const committed = { id: 1, items: [{ id: 1, kind: 'memo', text: 'saved text' }] };
            state.reply = committed;
            const save = backend.request({ command: 'save', id: 1, base: '', text: 'saved text' });
            state.requests[0].finish();
            assert.deepEqual(await save, committed);
            assert.equal(backend.monitor, undefined);
            assert.equal(state.timers.size, 0);
            state[failure].clear();
            const snapshot = backend.request({ command: 'snapshot' });
            state.requests[1].finish();
            assert.deepEqual(await snapshot, committed);
            assert.ok(backend.monitor);
            advance(100);
            assert.deepEqual(changes, [{ pruneMissing: false }]);
        });
    }

    test('groups canonical linked paths by parent and deduplicates files', t => {
        const { backend, monitor, changes, advance } = fixture(t);
        backend.watchFiles([
            linked('/files/one'), linked('/files/one'), linked('/files/nested/../one'),
            linked('/files//two'), linked('/elsewhere/three'),
            { kind: 'text', name: '/text/not-linked' }, { kind: 'memo', name: '/memo/not-linked' },
            { kind: 'file', image_path: '/cache/thumbnail.png' }, linked('relative/file'),
            linked('file:///remote/file'), linked('/invalid\0file'), linked('/'),
        ]);
        assert.deepEqual(state.attempts, ['/files', '/elsewhere'].map(path =>
            ({ path, flag: flags.WATCH_MOVES, cancellable: null })));
        assert.deepEqual([...backend.fileWatches.get('/files').paths].sort(), ['/files/one', '/files/two']);
        assert.deepEqual(changes, []);
        assert.equal(state.timers.size, 0);
        monitor('/files').emit('DELETED', '/files/./one');
        advance(100);
        assert.deepEqual(changes, [{ pruneMissing: true }]);
    });

    test('same immutable array skips all path work; new snapshots reuse and synchronize monitors', t => {
        const { backend, monitor, changes, advance } = fixture(t);
        const items = Object.freeze([linked('/kept/old'), linked('/removed/file')]);
        backend.watchFiles(items);
        const kept = monitor('/kept'), removed = monitor('/removed');
        const fileObjects = state.fileObjects;
        for (let i = 0; i < 10; i++) backend.watchFiles(items);
        assert.equal(state.fileObjects, fileObjects);
        assert.equal(state.attempts.length, 2);
        backend.watchFiles([linked('/kept/new'), linked('/added/file')]);
        assert.equal(monitor('/kept'), kept);
        assert.equal(kept.cancellations, 0);
        assert.equal(removed.cancellations, 1);
        assert.deepEqual(state.attempts.map(entry => entry.path), ['/kept', '/removed', '/added']);
        kept.emit('DELETED', '/kept/old');
        removed.emit('DELETED', '/removed/file');
        advance(100);
        assert.deepEqual(changes, []);
        kept.emit('DELETED', '/kept/new');
        advance(100);
        assert.deepEqual(changes, [{ pruneMissing: true }]);
        backend.watchFiles([]);
        assert.equal(kept.cancellations, 1);
        assert.equal(monitor('/added').cancellations, 1);
        assert.equal(backend.fileWatches.size, 0);
    });

    for (const event of ['DELETED', 'MOVED', 'RENAMED', 'MOVED_OUT']) {
        test(`${event} invalidates tracked files or the monitored parent, not unrelated paths`, t => {
            const { backend, monitor, changes, advance } = fixture(t);
            backend.watchFiles([linked('/files/linked')]);
            const watch = monitor('/files');
            watch.emit(event, '/files/unrelated', '/files/also-unrelated');
            watch.emit(event, '/elsewhere/linked');
            watch.emit(event);
            advance(100);
            assert.deepEqual(changes, []);
            watch.emit(event, '/files/linked', '/elsewhere/new-name');
            advance(100);
            watch.emit(event, '/files/unrelated', '/files/./linked');
            advance(100);
            watch.emit(event, '/files/.', '/renamed-parent');
            advance(100);
            assert.deepEqual(changes, Array.from({ length: 3 }, () => ({ pruneMissing: true })));
        });
    }

    for (const event of ['PRE_UNMOUNT', 'UNMOUNTED']) {
        test(`${event} invalidates the entire directory watch even without an event file`, t => {
            const { backend, monitor, changes, advance } = fixture(t);
            backend.watchFiles([linked('/files/linked')]);
            monitor('/files').emit(event);
            advance(100);
            assert.deepEqual(changes, [{ pruneMissing: true }]);
        });
    }

    test('content, creation, attribute, and incoming move events do not request pruning', t => {
        const { backend, monitor, changes, advance } = fixture(t);
        backend.watchFiles([linked('/files/linked')]);
        for (const event of ['CHANGED', 'CHANGES_DONE_HINT', 'CREATED', 'ATTRIBUTE_CHANGED', 'MOVED_IN'])
            monitor('/files').emit(event, '/files/linked');
        advance(1000);
        assert.deepEqual(changes, []);
        assert.equal(state.timers.size, 0);
    });

    test('history-only delivery has pruneMissing false and ignores unrelated history events', t => {
        const { backend, monitor, changes, advance } = fixture(t);
        backend.watch();
        backend.watch();
        assert.deepEqual(state.attempts, [{ path: '/history', flag: flags.NONE, cancellable: null }]);
        monitor('/history').emit('CREATED', '/history/other.json');
        advance(100);
        assert.deepEqual(changes, []);
        monitor('/history').emit('RENAMED', '/history/temporary', '/history/history.json');
        advance(99);
        assert.deepEqual(changes, []);
        advance(1);
        assert.deepEqual(changes, [{ pruneMissing: false }]);
    });

    for (const fileFirst of [true, false]) {
        test(`file and history invalidations share a resetting debounce (file first: ${fileFirst})`, t => {
            const { backend, monitor, changes, advance } = fixture(t);
            backend.watch();
            backend.watchFiles([linked('/files/linked')]);
            const fileEvent = () => monitor('/files').emit('DELETED', '/files/linked');
            const historyEvent = () => monitor('/history').emit('CHANGED', '/history/history.json');
            (fileFirst ? fileEvent : historyEvent)();
            advance(50);
            (fileFirst ? historyEvent : fileEvent)();
            assert.equal(state.timers.size, 1);
            advance(99);
            assert.deepEqual(changes, []);
            advance(1);
            assert.deepEqual(changes, [{ pruneMissing: true }]);
            historyEvent();
            advance(100);
            assert.deepEqual(changes, [{ pruneMissing: true }, { pruneMissing: false }]);
        });
    }

    test('failed directory setup is isolated and retried only with a new snapshot', t => {
        const { backend, monitor, changes, advance } = fixture(t);
        state.failures.add('/missing');
        state.connectFailures.add('/broken');
        const items = [linked('/missing/file'), linked('/working/file'), linked('/broken/file')];
        assert.doesNotThrow(() => backend.watchFiles(items));
        assert.deepEqual([...backend.fileWatches.keys()], ['/working']);
        assert.equal(monitor('/broken').cancellations, 1);
        backend.watchFiles(items);
        assert.equal(state.attempts.length, 3);
        assert.deepEqual(changes, []);
        state.failures.clear();
        state.connectFailures.clear();
        backend.watchFiles([...items]);
        assert.equal(state.attempts.length, 5);
        assert.equal(backend.fileWatches.size, 3);
        monitor('/working').emit('DELETED', '/working/file');
        advance(100);
        assert.deepEqual(changes, [{ pruneMissing: true }]);
    });

    test('unwatchFiles cancels all file monitors, ignores stale signals, and permits the same array again', t => {
        const { backend, monitor, changes, advance } = fixture(t);
        const items = [linked('/files/linked'), linked('/other/linked')];
        backend.watch();
        backend.watchFiles(items);
        const old = monitor('/files');
        backend.unwatchFiles();
        backend.unwatchFiles();
        assert.equal(backend.fileWatches.size, 0);
        assert.equal(old.cancellations, 1);
        assert.equal(monitor('/other').cancellations, 1);
        assert.equal(monitor('/history').cancellations, 0);
        old.emit('DELETED', '/files/linked');
        advance(100);
        assert.deepEqual(changes, []);
        backend.watchFiles(items);
        assert.notEqual(monitor('/files'), old);
        assert.equal(state.attempts.length, 5);
    });

    test('dispose cancels pending delivery, monitors, and processes and prevents new work', t => {
        const { backend, monitor, changes, advance } = fixture(t);
        backend.watch();
        backend.watchFiles([linked('/files/linked')]);
        let exits = 0;
        backend.processes.add({ force_exit: () => exits++ });
        monitor('/files').emit('DELETED', '/files/linked');
        monitor('/history').emit('CHANGED', '/history/history.json');
        assert.equal(state.timers.size, 1);
        backend.dispose();
        assert.equal(state.timers.size, 0);
        assert.equal(backend.fileWatches.size, 0);
        assert.equal(backend.processes.size, 0);
        assert.equal(exits, 1);
        assert.equal(backend.pruneMissing, false);
        assert.ok(state.monitors.every(entry => entry.cancellations === 1));
        monitor('/files').emit('UNMOUNTED');
        monitor('/history').emit('DELETED', '/history/history.json');
        backend.watch();
        backend.watchFiles([linked('/new/file')]);
        advance(1000);
        assert.deepEqual(changes, []);
        assert.equal(state.attempts.length, 2);
    });
}

async function nativeTests() {
    const { default: GLib } = await import('gi://GLib');
    const { default: Gtk } = await import('gi://Gtk?version=4.0');
    const { default: Gdk } = await import('gi://Gdk?version=4.0');
    const { default: Gio } = await import('gi://Gio');
    const { ClipboardPanel, createApplication, loadTheme, scheduleAutosave } = await import('./frontend.js');
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
    const root = GLib.getenv('CLIPBOARD_DATA_DIR');
    if (!root || !root.includes('clipboard-ui-')) throw new Error('Native tests require --ui isolation.');
    GLib.mkdir_with_parents(`${root}/images`, 0o700);
    const png = GLib.base64_decode('iVBORw0KGgoAAAANSUhEUgAAACAAAAAgCAIAAAD8GO2jAAAAKklEQVR4nO3NsQkAAAgDsD7q/w84i1c4CIHsSfWcikAgEAgEAoFAIPgSLI/kPIhK7EgvAAAAAElFTkSuQmCC');
    GLib.file_set_contents(`${root}/images/3.png`, png);
    GLib.file_set_contents(`${root}/images/4.png`, png);
    GLib.file_set_contents(`${root}/source screenshot.png`, png);
    GLib.file_set_contents(`${root}/report one.txt`, 'file reference fixture');
    GLib.file_set_contents(`${root}/images/5.png`, png);
    GLib.file_set_contents(`${root}/external source.png`, png);
    GLib.file_set_contents(`${root}/move me.txt`, 'moved file fixture');
    GLib.file_set_contents(`${root}/replace me.txt`, 'original file fixture');
    const fixture = (id, kind, text, extra = {}) => ({ id, kind, text, image_file: null,
        mime: 'text/plain;charset=utf-8', pinned: false, created_at: id, digest: `fixture-${id}`, ...extra });
    const benchmark = GLib.getenv('CLIPBOARD_TEST_BENCHMARK') === '1';
    const extraItems = benchmark ? Array.from({ length: 1800 }, (_, index) =>
        fixture(index + 100, 'text', `Performance fixture ${index}\n${'Clipboard history text. '.repeat(20)}`)) : [];
    GLib.file_set_contents(`${root}/history.json`, JSON.stringify({ version: 1, next_id: 2000, items: [
        fixture(1, 'text', '<not markup>\nA complete text entry'),
        fixture(2, 'file', `${root}/report one.txt`, { name: `${root}/report one.txt` }),
        fixture(3, 'file', null, { image_file: '3.png', mime: 'image/png' }),
        fixture(4, 'file', null, { image_file: '4.png', mime: 'image/png', name: `${root}/source screenshot.png` }),
        fixture(5, 'file', null, { image_file: '5.png', mime: 'image/png', name: `${root}/external source.png`, pinned: true }),
        fixture(6, 'file', `${root}/move me.txt`, { name: `${root}/move me.txt` }),
        fixture(7, 'file', `${root}/replace me.txt`, { name: `${root}/replace me.txt` }),
        ...extraItems,
    ] }));
    const app = createApplication();
    app.register(null);
    const palette = loadTheme(GLib.getenv('CLIPBOARD_PANEL_CONFIG'), GLib.getenv('CLIPBOARD_PANEL_PALETTE'));
    let panel;
    let requests = 0, publications = 0;
    const backend = new ClipboardBackend({ onChange: options => { if (panel?.opened) void model.refresh(options); } });
    const model = new ClipboardModel({ request: message => { requests++; return backend.request(message); },
        publish: payload => { publications++; return backend.publish(payload); },
        trashFile: path => backend.trashFile(path), onChange: () => panel?.render() });
    panel = new ClipboardPanel({ app, model, backend, palette });
    const historyItems = () => {
        const items = [];
        const visit = widget => {
            if (widget.has_css_class('history-item')) items.push(widget);
            for (let child = widget.get_first_child(); child; child = child.get_next_sibling()) visit(child);
        };
        visit(panel.list);
        return items;
    };
    const historyRow = id => {
        const position = panel.viewRows.findIndex(item => item.id === id);
        return historyItems().find(row => row.get_first_child().label === panel.strings.get_string(position));
    };
    try {
        const monitor = app.get_monitors()[0];
        assert(await panel.toggle(monitor), model.failure);
        assert(model.editor === null, 'Opening must not select an existing item or memo draft');
        assert(model.items.some(item => item.kind === 'memo' && !item.text), 'Memo draft was not created');
        await wait(250);
        const assertUnselected = () => {
            assert(model.tab === 'memo' && panel.tabButtons.every((tab, index) => tab.has_css_class('selected') === (index === 0)),
                'Every panel opening must return to the Memo tab');
            assert(model.editor === null && panel.selection.selected === Gtk.INVALID_LIST_POSITION,
                'The panel must open with no history row selected');
            assert(panel.preview.visible_child_name === 'empty' && !panel.picture.get_paintable()
                && panel.bufferText === '', 'An unselected panel must not display a previous preview');
            assert(historyItems().every(row => row.get_last_child().get_first_child().sensitive
                && row.get_last_child().get_last_child().sensitive), 'Row actions must work without selecting an item');
        };
        assertUnselected();
        await idle();
        assert(await model.refresh(), model.failure);
        assertUnselected();
        if (benchmark) {
            const paint = () => new Promise(resolve => {
                let frames = 0;
                panel.window.add_tick_callback(() => {
                    if (++frames < 2) return GLib.SOURCE_CONTINUE;
                    resolve();
                    return GLib.SOURCE_REMOVE;
                });
            });
            for (const tab of ['text', 'memo', 'text']) {
                await wait(150);
                await idle();
                const before = requests, start = GLib.get_monotonic_time();
                assert(await model.changeTab(tab), model.failure || 'Tab switch was blocked');
                await paint();
                const milliseconds = (GLib.get_monotonic_time() - start) / 1000;
                let widgets = 0;
                for (let child = panel.list.get_first_child(); child; child = child.get_next_sibling()) widgets++;
                print(`BENCHMARK ${tab}: ${milliseconds.toFixed(1)}ms, ${requests - before} helper requests, ${model.rows.length} items, ${widgets} list widgets`);
                assert(requests === before, 'Clean tab switches must not launch the history helper');
                assert(model.rows.length < 1000 || widgets < 300, 'GTK must virtualize large histories');
            }
            const last = model.rows.at(-1);
            assert(await model.select(last.id), model.failure);
            await paint();
            assert(panel.selection.selected === model.rows.length - 1
                && panel.listScroll.vadjustment.value > 0, 'Keyboard selection must scroll beyond the recycled viewport');
            for (const query of ['Performance fixture 1799', '']) {
                const start = GLib.get_monotonic_time();
                model.filter(query);
                await paint();
                print(`BENCHMARK filter ${query ? 'match' : 'clear'}: ${((GLib.get_monotonic_time() - start) / 1000).toFixed(1)}ms`);
            }
            let updates = 0;
            panel.strings.connect('items-changed', () => { updates++; });
            const before = requests, start = GLib.get_monotonic_time();
            for (let index = 0; index < 20; index++) panel.editor.buffer.insert_at_cursor('x', -1);
            await paint();
            assert(updates === 0 && requests === before, 'Typing must not rebuild history or launch the helper');
            print(`BENCHMARK 20 keystrokes: ${((GLib.get_monotonic_time() - start) / 1000).toFixed(1)}ms, no history updates or helper calls`);
            return;
        }
        assert(panel.card.get_width() === 450, 'Clipboard panel must be half its original 900px width');
                const [positioned, cardBounds] = panel.card.compute_bounds(panel.root);
                assert(positioned && Math.abs(cardBounds.get_x() + cardBounds.get_width() - (panel.root.get_width() - 16)) <= 1
                    && Math.abs(cardBounds.get_y() - 40) <= 1,
                    'Clipboard panel must sit 16px from the right edge and 40px from the top');
        const sections = [];
        for (let child = panel.content.get_first_child(); child; child = child.get_next_sibling()) sections.push(child);
        assert(sections.length === 3 && sections[0] === panel.search && sections[2] === panel.failure,
            'The panel must have search, tabbed content and inline errors, with no bottom toolbar');
        assert(panel.body.orientation === Gtk.Orientation.VERTICAL, 'Compact history and preview must stack');
        const assertHistoryHeight = () => {
            const listHeight = panel.listScroll.get_allocated_height();
            const previewHeight = panel.preview.get_parent().get_allocated_height();
            const rowHeight = historyItems()[0].get_parent().get_allocated_height();
            assert(!panel.body.homogeneous && rowHeight === 40 && listHeight === 5 * rowHeight && previewHeight > 0,
                `History must fit five complete 40px rows, got ${listHeight}px with ${rowHeight}px rows`);
            assert(panel.body.spacing === 0, 'History and preview must meet without an extra gap');
                        const originalHeight = Math.max(280, Math.min(600, monitor.get_geometry().height - 80));
            const fixedHeight = panel.card.get_height() - previewHeight;
            assert(Math.abs(previewHeight - (originalHeight - fixedHeight) * 0.64) <= 1,
                'Only the preview must shrink by two successive 20% reductions');
            assert(panel.listScroll.get_allocated_width() === panel.preview.get_parent().get_allocated_width(),
                'History and preview must have the same width');
        };
        assertHistoryHeight();
        const outerBorder = panel.body.get_parent().get_style_context().get_border();
        assert(['top', 'right', 'bottom', 'left'].every(side => outerBorder[side] === 0),
            'The tab container must not add a cyan border around inactive tabs');
        const pageBorder = panel.body.get_style_context().get_border();
        assert(pageBorder.top === 0 && pageBorder.left === 1 && pageBorder.right === 1 && pageBorder.bottom === 1,
            'The cyan content outline must join the tabs below their own pink/cyan outlines');
        assert(panel.tabs.homogeneous && panel.tabs.spacing === 0, 'Audio-style tabs must be equal-width and connected');
        const tabWidths = panel.tabButtons.map(tab => tab.get_width());
        assert(Math.max(...tabWidths) - Math.min(...tabWidths) <= 1, 'Tab widths must only differ by pixel rounding');
        const matchesColor = (widget, hex) => {
            const actual = widget.get_color();
            const rgb = hex.slice(1).match(/../g).map(value => parseInt(value, 16) / 255);
            return [actual.red, actual.green, actual.blue].every((value, index) => Math.abs(value - rgb[index]) < 0.01);
        };
        panel.tabButtons.forEach((tab, index) => {
            assert(tab.label === ['MEMO', 'TEXT', 'FILES'][index] && tab.has_css_class('tab'),
                'Tabs must use the audio panel’s text-only uppercase labels');
            // get_height() is the content box; include CSS borders here.
            assert(tab.get_allocated_height() === 40,
                `Audio-style tabs must allocate 40px, got ${tab.get_allocated_height()}`);
            assert(tab.has_css_class('selected') === (index === 0), 'Only the current tab may be selected');
            assert(matchesColor(tab.get_child(), index === 0 ? palette.cyan : palette.pink),
                'Selected tabs must be cyan and inactive tabs pink');
        });
        for (const tab of panel.tabButtons) {
            const color = tab.has_css_class('selected') ? palette.cyan : palette.pink;
            for (const state of [Gtk.StateFlags.PRELIGHT, Gtk.StateFlags.FOCUS_VISIBLE]) {
                tab.set_state_flags(state, false);
                await wait(150);
                assert(matchesColor(tab.get_child(), color), 'Hover and keyboard focus must not change tab text colors');
                tab.unset_state_flags(state);
            }
        }
        assert(!panel.list.single_click_activate, 'Copying a row must require double-click activation');
        for (const row of historyItems()) {
            const actions = row.get_last_child();
            for (const control of [actions.get_first_child(), actions.get_last_child()]) {
                const [ok, bounds] = control.compute_bounds(panel.card);
                assert(ok && bounds.get_x() >= 0 && bounds.get_x() + bounds.get_width() <= panel.card.get_width(),
                    'Inline row actions must fit inside the compact frame');
                assert(control.get_allocated_width() === 22 && control.get_allocated_height() === 22,
                    'Pin and delete controls must stay compact at 22px square');
            }
        }
        await idle();
        assert(panel.key(Gdk.KEY_n, Gdk.ModifierType.CONTROL_MASK), 'Ctrl+N must remain available without the header button');
        await idle();
        assert(model.editor.kind === 'memo' && panel.window.get_focus() === panel.editor,
            'Ctrl+N must focus the memo editor');
        let listUpdates = 0;
        const listSignal = panel.strings.connect('items-changed', () => { listUpdates++; });
        panel.editor.buffer.set_text('native GTK clipboard\n你好', -1);
        assert(listUpdates === 0, 'Editing must not rebuild the history list');
        assert(model.dirty, 'Editing must mark the memo dirty');
        assert(panel.key(Gdk.KEY_s, Gdk.ModifierType.CONTROL_MASK), 'Ctrl+S must save without a toolbar button');
        await idle();
        assert(listUpdates === 1, 'Saving must update only the changed list range');
        panel.strings.disconnect(listSignal);
        const memoId = model.editor.id;
        model.setEditor(null);
        model.changed();
        await wait(100);
        await idle();
        historyRow(memoId).get_last_child().get_first_child().emit('clicked');
        await wait(100);
        await idle();
        const pinnedRow = historyItems().find(row => row.has_css_class('pinned'));
        const badge = pinnedRow?.get_first_child().get_next_sibling();
        assert(badge?.visible && badge.label === 'PINNED', 'Pinned rows must keep their separate text badge');
        assert(matchesColor(badge, palette.red), 'Pin badges must use the red accent');
        assert(pinnedRow.get_first_child().label === panel.strings.get_string(0), 'Pin badges must not alter item titles');
        const unpin = pinnedRow.get_last_child().get_first_child();
        assert(unpin.has_css_class('pinned') && unpin.tooltip_text === 'Unpin this item',
            'Pinned rows must expose a distinct red Unpin action');
        assert(model.editor === null && publications === 0 && panel.opened,
            'Pinning an unselected row must not select it, copy it or close the panel');
        unpin.grab_focus();
        assert(panel.key(Gdk.KEY_Return, 0) === false, 'Enter on a row action must reach the button, not copy the row');
        unpin.emit('clicked');
        await wait(100);
        await idle();
        assert(historyItems().every(row => !row.has_css_class('pinned') && !row.get_first_child().get_next_sibling().visible
            && !row.get_last_child().get_first_child().has_css_class('pinned')),
            'Unpinning must clear badges and active pin buttons, including recycled rows');
        assert(model.editor === null && publications === 0 && panel.opened, 'Unpinning must not copy or select a row');
        panel.list.emit('activate', panel.viewRows.findIndex(item => item.id === memoId));
        await idle();
        await wait(30);
        assert(model.editor?.id === memoId && publications === 1 && !panel.opened,
            'Double-click activation must copy the exact row and close even without a prior selection');
        const text = await new Promise((resolve, reject) => {
            const clipboard = Gdk.Display.get_default().get_clipboard();
            clipboard.read_text_async(null, (source, result) => {
                try { resolve(source.read_text_finish(result)); } catch (error) { reject(error); }
            });
        });
        assert(text === 'native GTK clipboard\n你好', 'GDK clipboard text roundtrip failed');
        await idle();
        assert(panel.key(Gdk.KEY_Escape, 0), 'Escape must remain available without the header close button');
        await idle();
        assert(!panel.opened, 'Save and close did not hide the panel');
        assert(new TextDecoder().decode(await externalRead('text/plain;charset=utf-8')) === text,
            'External text clipboard read after hiding changed the contents');
        const [, bytes] = GLib.file_get_contents(`${GLib.getenv('CLIPBOARD_DATA_DIR')}/history.json`);
        assert(new TextDecoder().decode(bytes).includes('native GTK clipboard'), 'Real helper did not persist the memo');
        assert(await panel.toggle(monitor), model.failure);
        await wait(100);
        await idle();
        assertUnselected();
        assert(panel.key(Gdk.KEY_Down, 0), 'Down must select an entry from an unselected panel');
        await idle();
        assert(model.editor?.id === model.rows[0]?.id && panel.selection.selected === 0,
            'Explicit keyboard navigation must still select the first row');
        assert(await model.changeTab('file'), model.failure);
        assert(panel.tabButtons.every((tab, index) => tab.has_css_class('selected') === (index === 2)),
            'Switching tabs must transfer the selected outline to Files');
        assert(await model.select(2), model.failure);
        assert(!panel.editor.editable, 'File references must be read-only');
        assert(await model.copy(), model.failure);
        const uri = await backend.request({ command: 'payload', id: 2 });
        assert(uri.mime === 'text/uri-list' && uri.text.endsWith('/report%20one.txt\n'), 'File URI policy was not preserved');
        assert(new TextDecoder().decode(await externalRead('text/uri-list')) === uri.text, 'Native file URI publication changed');
        panel.selection.selected = panel.viewRows.findIndex(item => item.id === 3);
        await idle();
        assert(model.editor.id === 3, 'Native list selection must resolve the correct clipboard ID');
        await wait(200);
        await idle();
        assert(panel.preview.visible_child_name === 'image' && panel.picture.get_paintable(), 'PNG preview did not load');
        assertHistoryHeight();
        assert(await model.copy(), model.failure);
        panel.hide();
        // Read through another Wayland client while our window is hidden.
        const copied = await externalRead('image/png');
        assert(GLib.base64_encode(copied) === GLib.base64_encode(png), 'Native image clipboard bytes changed after hiding');
        assert(await panel.toggle(monitor), model.failure);
        await idle();
        assertUnselected();
        assert(await model.changeTab('file'), model.failure);
        const until = async (check, message) => {
            for (let attempt = 0; attempt < 100 && !check(); attempt++) await wait(25);
            assert(check(), message);
            await idle();
        };
        await wait(150);
        await idle();
        assert(backend.fileWatches.size === 1, 'Linked files in one directory must share one native monitor');
        assert(await model.select(5), model.failure);
        Gio.File.new_for_path(`${root}/external source.png`).delete(null);
        await until(() => !model.items.some(item => item.id === 5), 'External deletion must remove even a pinned named image while open');
        assert(model.tab === 'file' && model.editor?.id !== 5 && panel.opened,
            'Missing-file cleanup must preserve Files mode and stop previewing the removed file');
        assert(!GLib.file_test(`${root}/images/5.png`, GLib.FileTest.EXISTS), 'External source deletion must clean its cached preview');
        assert(model.items.some(item => item.id === 3) && GLib.file_test(`${root}/images/3.png`, GLib.FileTest.EXISTS),
            'Cached-only images must survive unrelated source deletion');
        assert(!(await backend.request({ command: 'snapshot' })).items.some(item => item.id === 5),
            'Missing files must be removed from persisted history, not merely hidden');
        assert(await model.newMemo(), model.failure);
        panel.editor.buffer.set_text('unsaved text during external file removal', -1);
        Gio.File.new_for_path(`${root}/move me.txt`).move(Gio.File.new_for_path(`${root}/moved away.txt`),
            Gio.FileCopyFlags.NONE, null, null);
        await until(() => !model.items.some(item => item.id === 6), 'Renaming a linked file away must remove its stale entry');
        assert(model.tab === 'memo' && model.dirty && model.editor.text === 'unsaved text during external file removal',
            'External file cleanup must not switch tabs or lose unsaved memo text');
        assert(GLib.file_test(`${root}/moved away.txt`, GLib.FileTest.EXISTS), 'Cleanup must never delete the moved source');
        GLib.file_set_contents(`${root}/replacement.tmp`, 'replacement file fixture');
        Gio.File.new_for_path(`${root}/replacement.tmp`).move(Gio.File.new_for_path(`${root}/replace me.txt`),
            Gio.FileCopyFlags.OVERWRITE, null, null);
        await wait(350);
        await idle();
        assert(model.items.some(item => item.id === 7), 'Atomic file replacement at the same path must not remove the entry');
        assert(await panel.close(), model.failure);
        assert(backend.fileWatches.size === 0, 'Hiding the panel must release local directory monitors');
        Gio.File.new_for_path(`${root}/replace me.txt`).delete(null);
        assert(await panel.toggle(monitor), model.failure);
        await idle();
        assertUnselected();
        assert(!model.items.some(item => item.id === 7), 'Reopening must prune files deleted while the panel was hidden');
        assert(await model.changeTab('file'), model.failure);
        const assertNoDialog = () => assert(!Gtk.Window.list_toplevels().some(window => window.visible && window.modal),
            'Deleting files must not open a confirmation dialog');
        assert(await model.select(3), model.failure);
        await wait(100);
        await idle();
        const beforeDelete = publications;
        historyRow(2).get_last_child().get_last_child().emit('clicked');
        assertNoDialog();
        await idle();
        assertNoDialog();
        assert(!GLib.file_test(`${root}/report one.txt`, GLib.FileTest.EXISTS)
            && !model.items.some(item => item.id === 2), model.failure || 'Row × must remove its linked file and history entry');
        assert(model.editor?.id === 3 && publications === beforeDelete && panel.opened,
            'Deleting another row must preserve selection and must not copy or close the panel');
        const trash = Gio.File.new_for_path(`${GLib.get_user_data_dir()}/Trash/files`);
        const entries = trash.enumerate_children('standard::name', Gio.FileQueryInfoFlags.NONE, null);
        let found = false;
        try {
            let entry;
            while ((entry = entries.next_file(null))) {
                const file = trash.get_child(entry.get_name());
                const [, bytes] = file.load_contents(null);
                if (new TextDecoder().decode(bytes) === 'file reference fixture') found = true;
            }
        } finally { entries.close(null); }
        assert(found, 'The deleted local file must remain recoverable in the isolated Trash');
        assert(await model.select(4), model.failure);
        assert(panel.key(Gdk.KEY_d, Gdk.ModifierType.ALT_MASK), 'Alt+D must immediately delete the selected file');
        assertNoDialog();
        await idle();
        assertNoDialog();
        assert(!GLib.file_test(`${root}/source screenshot.png`, GLib.FileTest.EXISTS)
            && !GLib.file_test(`${root}/images/4.png`, GLib.FileTest.EXISTS)
            && !model.items.some(item => item.id === 4), 'Named images must delete both their local source and cached preview');
        assert(await model.select(3), model.failure);
        assert(await model.remove(), model.failure);
        assert(!GLib.file_test(`${root}/images/3.png`, GLib.FileTest.EXISTS), 'Cached-only images must delete without file confirmation');
        assert(await backend.trashFile(`${root}/already missing.txt`) === false, 'Missing files must not block stale entry removal');
        let refusedFolder = false;
        try { await backend.trashFile(`${root}/images`); } catch (error) { refusedFolder = error.message.includes('Only files'); }
        assert(refusedFolder && GLib.file_test(`${root}/images`, GLib.FileTest.IS_DIR), 'Folders must never be trashed by the panel');
        GLib.file_set_contents(`${root}/link target.txt`, 'keep the symlink target');
        Gio.File.new_for_path(`${root}/linked.txt`).make_symbolic_link(`${root}/link target.txt`, null);
        assert(await backend.trashFile(`${root}/linked.txt`), 'A symbolic link must be movable to Trash');
        assert(GLib.file_test(`${root}/link target.txt`, GLib.FileTest.EXISTS)
            && !GLib.file_test(`${root}/linked.txt`, GLib.FileTest.IS_SYMLINK), 'Trashing a symlink must keep its target');
        assert(await model.changeTab('memo'), model.failure);
        await idle();
        // Keep timer-driven writes after the row-action and file fixtures, and
        // restore the preview before any optional screenshot.
        assert(model.editor?.kind === 'memo' && !model.dirty, 'Autosave smoke requires a clean memo fixture');
        const { id, text: originalText } = model.editor;
        const previousScheduler = model.scheduleAutosave;
        model.scheduleAutosave = scheduleAutosave;
        try {
            const text = '\t native autosave 你好 😀\n\nexact whitespace  \n';
            panel.editor.buffer.set_text(text, -1);
            assert(model.dirty, 'Native preview editing must schedule an idle autosave');
            await wait(450);
            for (let attempt = 0; attempt < 100 && (model.dirty || model.busy) && !model.failure; attempt++) await wait(25);
            await idle();
            assert(!model.dirty && !model.failure, model.failure || 'Native 400ms autosave did not settle');
            const snapshot = await backend.request({ command: 'snapshot' });
            assert(snapshot.items.find(item => item.id === id)?.text === text,
                'The GLib autosave timer must persist the exact preview text through the real helper');
            assert(model.editor.id === id && model.editor.base === text && panel.bufferText === text,
                'Native autosave must preserve the selection and preview text');
        } finally {
            model.cancelAutosave();
            model.scheduleAutosave = previousScheduler;
        }
        await idle();
        model.edit(originalText);
        assert(await model.save(), model.failure || 'Could not restore the memo after native autosave');
        await wait(250);
        if (GLib.getenv('CLIPBOARD_TEST_SCREENSHOT')) {
            const capture = Gio.Subprocess.new(['niri', 'msg', 'action', 'screenshot-screen',
                '--path', GLib.getenv('CLIPBOARD_TEST_SCREENSHOT')], Gio.SubprocessFlags.NONE);
            await new Promise((resolve, reject) => capture.wait_check_async(null, (source, result) => {
                try { source.wait_check_finish(result); resolve(); } catch (error) { reject(error); }
            }));
        }
        print('PASS: compact five-row history layout, pin badges, keyboard controls, nested Wayland layer-shell, GTK/CSS, editor, 400ms GLib autosave, Rust persistence, tabs, file URI policy, PNG preview, cross-client clipboard bytes, immediate Delete/Alt+D without dialogs, recoverable Gio Trash, live external deletion/rename cleanup, atomic replacement, cached-only and named images, missing files, folder refusal, and symlink safety');
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
