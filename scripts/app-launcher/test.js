// Node: npm test. Isolated GTK/Astal: npm run test:ui -- --runtime /absolute/path/to/ags-runtime
// Runtime-specific imports keep this single suite usable from both Node and panel-gjs.

async function isolatedUI() {
    const { default: fs } = await import('node:fs');
    const { default: os } = await import('node:os');
    const { default: path } = await import('node:path');
    const { spawn } = await import('node:child_process');
    const { setTimeout: delay } = await import('node:timers/promises');
    const { fileURLToPath } = await import('node:url');
    const { parseArgs } = await import('node:util');
    const { values } = parseArgs({ options: { ui: { type: 'boolean' }, runtime: { type: 'string' } } });
    if (!values.runtime || !path.isAbsolute(values.runtime))
        throw new Error('--runtime must be an absolute path to the native AGS runtime');
    const launcher = path.join(values.runtime, 'bin/panel-gjs');
    fs.accessSync(launcher, fs.constants.X_OK);
    const source = path.dirname(fileURLToPath(import.meta.url));
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'app-launcher-ui-'));
    fs.chmodSync(root, 0o700);
    // Keep executable/library lookup and locale, never desktop, bus, theme, or module overrides.
    // LD_LIBRARY_PATH also permits Nix shells to supply Niri's dlopened X11 libraries.
    const env = Object.fromEntries(Object.entries(process.env)
        .filter(([name]) => /^(PATH|LD_LIBRARY_PATH|LANG|LANGUAGE|LC_[A-Z_]+|TZ)$/.test(name)));
    const processes = [];
    const interrupted = new AbortController();
    const onInterrupt = signal => interrupted.abort(new Error(`Native UI tests interrupted (${signal})`));
    process.on('SIGINT', onInterrupt);
    process.on('SIGTERM', onInterrupt);
    const timeout = setTimeout(() => interrupted.abort(new Error('Native UI runner exceeded 75 seconds')), 75000);

    function start(command, args, { oneShot = false, displayfd = false } = {}) {
        interrupted.signal.throwIfAborted();
        const child = spawn(command, args, {
            env, cwd: source, detached: true,
            stdio: ['ignore', 'pipe', 'pipe', ...(displayfd ? ['pipe'] : [])],
        });
        const record = { child, command, oneShot, log: '', result: null, error: null };
        record.done = new Promise(resolve => {
            child.on('error', error => { record.error = error; resolve(); });
            child.on('close', (code, signal) => { record.result = { code, signal }; resolve(); });
        });
        // Keep diagnostics bounded even if a compositor or driver is noisy.
        const log = bytes => { record.log = (record.log + bytes).slice(-256 * 1024); };
        child.stdout.on('data', log);
        child.stderr.on('data', log);
        processes.push(record);
        return record;
    }

    async function until(check, description, timeoutMs = 15000) {
        const deadline = Date.now() + timeoutMs;
        while (Date.now() < deadline) {
            interrupted.signal.throwIfAborted();
            for (const record of processes) {
                if (record.error) throw record.error;
                if (!record.oneShot && (record.result || record.child.exitCode !== null || record.child.signalCode !== null))
                    throw new Error(`${record.command} exited while waiting for ${description}`);
            }
            const value = check();
            if (value) return value;
            await delay(50);
        }
        throw new Error(`Timed out waiting for ${description} (${timeoutMs} ms)`);
    }

    function kill(record, signal) {
        if (!record.child.pid) return;
        // Signal the group even if its leader exited: descendants may still own sockets/pipes.
        try { process.kill(-record.child.pid, signal); }
        catch (error) { if (error.code !== 'ESRCH') throw error; }
    }

    try {
        for (const name of ['home', 'config', 'data', 'cache', 'state', 'runtime', 'tmp'])
            fs.mkdirSync(path.join(root, name), { mode: 0o700 });
        Object.assign(env, {
            HOME: path.join(root, 'home'), XDG_CONFIG_HOME: path.join(root, 'config'),
            XDG_CONFIG_DIRS: path.join(root, 'config'), XDG_DATA_HOME: path.join(root, 'data'),
            // panel-gjs adds its packaged schemas/resources; omit the live application catalogs.
            XDG_DATA_DIRS: path.join(root, 'data'), XDG_CACHE_HOME: path.join(root, 'cache'),
            XDG_STATE_HOME: path.join(root, 'state'), XDG_RUNTIME_DIR: path.join(root, 'runtime'),
            TMPDIR: path.join(root, 'tmp'), XDG_CURRENT_DESKTOP: 'niri',
            XDG_SESSION_DESKTOP: 'niri', XDG_SESSION_TYPE: 'wayland',
            GSETTINGS_BACKEND: 'memory', GTK_A11Y: 'none', NO_AT_BRIDGE: '1', GIO_USE_VFS: 'local',
            DBUS_SYSTEM_BUS_ADDRESS: `unix:path=${root}/no-system-bus`,
        });
        const applications = path.join(env.XDG_DATA_HOME, 'applications');
        fs.mkdirSync(applications, { mode: 0o700 });
        for (let index = 1; index <= 12; index++) {
            const id = String(index).padStart(2, '0');
            fs.writeFileSync(path.join(applications, `ui-fixture-${id}.desktop`),
                `[Desktop Entry]\nType=Application\nName=UI Fixture ${id}\nExec=true\nIcon=application-x-executable\nTerminal=false\n`);
        }

        const xvfb = start('Xvfb', ['-displayfd', '3', '-screen', '0', '1280x800x24', '-nolisten', 'tcp'], { displayfd: true });
        let display = '';
        xvfb.child.stdio[3].on('data', bytes => { display = (display + bytes).slice(-1024); });
        await until(() => /^\d+\n$/.test(display), 'private Xvfb display');
        env.DISPLAY = `:${display.trim()}`;

        const busConfig = path.join(root, 'bus.conf');
        const xml = text => text.replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;');
        // No includes or servicedir: nothing can activate a desktop session service.
        fs.writeFileSync(busConfig, `<busconfig><type>session</type>
            <listen>${xml(`unix:path=${env.XDG_RUNTIME_DIR}/bus`)}</listen><auth>EXTERNAL</auth>
            <policy context="default"><allow send_destination="*"/>
            <allow receive_sender="*"/><allow own="*"/></policy></busconfig>`);
        const bus = start('dbus-daemon', ['--nofork', '--nopidfile', '--print-address=1', `--config-file=${busConfig}`]);
        env.DBUS_SESSION_BUS_ADDRESS = await until(() => bus.log.match(/^unix:[^\r\n]+/m)?.[0], 'private service-free D-Bus');

        const config = path.join(root, 'niri.kdl');
        fs.writeFileSync(config, 'hotkey-overlay {\n    skip-at-startup\n}\n');
        // Never use --session: no systemd environment import or desktop autostart.
        start('niri', ['--config', config]);
        const sockets = await until(() => {
            const files = fs.readdirSync(env.XDG_RUNTIME_DIR);
            const wayland = files.find(name => /^wayland-\d+$/.test(name));
            const niri = files.find(name => name.startsWith('niri.') && name.endsWith('.sock'));
            return wayland && niri
                && fs.statSync(path.join(env.XDG_RUNTIME_DIR, wayland)).isSocket()
                && fs.statSync(path.join(env.XDG_RUNTIME_DIR, niri)).isSocket() ? { wayland, niri } : null;
        }, 'nested Niri Wayland sockets');
        env.WAYLAND_DISPLAY = sockets.wayland;
        env.NIRI_SOCKET = path.join(env.XDG_RUNTIME_DIR, sockets.niri);
        env.GDK_BACKEND = 'wayland';
        const test = start(launcher, ['-m', path.join(source, 'test.js')], { oneShot: true });
        const result = await until(() => test.result, 'native GTK/Astal tests', 30000);
        if (result.code !== 0)
            throw new Error(`Native tests failed (${result.signal || `exit ${result.code}`})`);
        if (test.log) process.stdout.write(test.log);
    } catch (error) {
        for (const record of processes)
            if (record.log) console.error(`--- ${record.command} ---\n${record.log}`);
        throw error;
    } finally {
        clearTimeout(timeout);
        try {
            for (const record of [...processes].reverse()) kill(record, 'SIGTERM');
            await Promise.race([Promise.allSettled(processes.map(record => record.done)), delay(2000)]);
        } finally {
            try {
                for (const record of [...processes].reverse()) kill(record, 'SIGKILL');
                await Promise.race([Promise.allSettled(processes.map(record => record.done)), delay(1000)]);
            } finally {
                process.off('SIGINT', onInterrupt);
                process.off('SIGTERM', onInterrupt);
                fs.rmSync(root, { recursive: true, force: true });
            }
        }
    }
}

async function unitTests() {
    const { default: test } = await import('node:test');
    const { default: assert } = await import('node:assert/strict');
    const { readFileSync } = await import('node:fs');
    const { runInNewContext } = await import('node:vm');
    const drawing = await import('../shared/astal/drawing.js');
    const read = file => readFileSync(new URL(file, import.meta.url), 'utf8');
    // Evaluate whole modules, including private helpers, without GI imports or running main().
    const load = (file, names, context = {}) => runInNewContext(`(async () => {
        ${read(file).replace(/^import\s[^]*?;\n/gm, '').replace(/^export\s+/gm, '')}
        return { ${names.join(', ')} };
    })()`, { GObject: { registerClass: (_properties, type) => type },
        Astal: { Application: class {} }, GLib: { getenv: () => '1' }, ...context }, { filename: file });
    const { parseRequest, panelLayout, drawPanelFrame, drawTileFrame } = await load('./frontend.js',
        ['parseRequest', 'panelLayout', 'drawPanelFrame', 'drawTileFrame'], drawing);
    const { applicationsFor } = await load('./backend.js', ['applicationsFor']);

    test('request validation and fixed layout, including constrained monitors', () => {
        for (const command of ['toggle', 'close', 'status']) {
            assert.equal(parseRequest(JSON.stringify({ command })).command, command);
            assert.equal(parseRequest(JSON.stringify({ command, monitor: 'DP-8' })).monitor, 'DP-8');
        }
        for (const value of ['null', '[]', '{}', '42', '"status"', 'not JSON', '{"command":"launch"}',
            '{"command":"toggle","monitor":42}', '{"command":"toggle","monitor":" "}'])
            assert.throws(() => parseRequest(value));
        for (const [width, height] of [[1920, 1200], [2560, 1440], [2048, 2304], [800, 600]])
            assert.deepEqual({ ...panelLayout(width, height) }, { columns: 4, width: 318, gridHeight: 190 });
        for (const [width, height, columns, gridHeight] of [[320, 600, 3, 190], [200, 240, 2, 124], [30, 80, 1, 1]]) {
            const layout = panelLayout(width, height);
            assert.equal(layout.columns, columns);
            assert.equal(layout.gridHeight, gridHeight);
            assert.ok(layout.width > 0 && layout.width <= width - 24);
        }
    });

    test('drawing preserves literal panel geometry and tile shape/colors', () => {
        const calls = [];
        const cr = Object.fromEntries(['moveTo', 'lineTo', 'closePath', 'setSourceRGBA', 'fillPreserve',
            'setLineWidth', 'stroke', 'strokePreserve', 'rectangle', 'fill']
            .map(name => [name, (...args) => calls.push([name, ...args])]));
        for (const [width, height] of [[318, 258], [200, 100]]) {
            calls.length = 0;
            drawPanelFrame(cr, width, height);
            assert.deepEqual(calls, [['moveTo', 10.5, 0.5], ['lineTo', width - 0.5, 0.5],
                ['lineTo', width - 0.5, height - 10.5], ['lineTo', width - 10.5, height - 0.5],
                ['lineTo', 0.5, height - 0.5], ['lineTo', 0.5, 10.5], ['closePath'],
                ['setSourceRGBA', 37 / 255, 37 / 255, 47 / 255, 0.9], ['fillPreserve'],
                ['setSourceRGBA', 252 / 255, 238 / 255, 10 / 255, 1], ['setLineWidth', 1], ['stroke']]);
        }
        calls.length = 0;
        drawTileFrame(cr, 56, 56);
        assert.deepEqual(calls.slice(0, 7), [['moveTo', 4, 4], ['lineTo', 47, 4], ['lineTo', 52, 9],
            ['lineTo', 52, 52], ['lineTo', 9, 52], ['lineTo', 4, 47], ['closePath']]);
        assert.deepEqual(calls.filter(([name]) => name === 'setSourceRGBA').map(call => call.slice(1)),
            [[0.015, 0.045, 0.05, 0.94], [0.20, 0.87, 0.92, 0.12], [0.24, 0.85, 0.91, 0.85],
                [0.20, 0.85, 0.90, 0.055], [0.42, 0.97, 1, 0.95], [0.27, 0.88, 0.93, 0.55]]);
        assert.equal(calls.filter(([name]) => name === 'rectangle').length, 8);
    });

    test('CSS keeps literal mixes and selected-state precedence without changing tile geometry', () => {
        const css = read('./style.css');
        assert.doesNotMatch(css, /@define-color|@mp_|\balpha\s*\(|\bvar\s*\(/);
        assert.deepEqual([...css.matchAll(/color-mix\(in srgb, (#[a-f\d]{6}) ([\d.]+)%, transparent\)/gi)]
            .map(([, color, percent]) => [color, percent]), [['#FCEE0A', '35'], ['#0F0C17', '55'],
            ['#ff6b75', '10'], ['#FCEE0A', '45'], ['#ff6b75', '55'], ['#26c6da', '28']]);
        const rules = [...css.matchAll(/([^{}]+)\{([^{}]*)\}/g)]
            .map(([, group, body]) => ({ selectors: group.split(',').map(value => value.trim()), body }));
        const selector = state => `.app-launcher flowboxchild.app-tile${state}`;
        const matching = (...states) => rules.filter(rule => states.some(state => rule.selectors.includes(selector(state))));
        const base = matching('')[0].body;
        for (const declaration of ['border: 1px solid transparent;', 'background: transparent;', 'background-image: none;',
            'box-shadow: none;', 'outline: none;', 'transition: border-color 140ms ease;']) assert.ok(base.includes(declaration));
        for (const state of [':selected', ':hover', ':focus-visible', ':active']) {
            const interaction = matching(state)[0].body;
            for (const declaration of ['border-color: color-mix(in srgb, #FCEE0A 45%, transparent);',
                'background: color-mix(in srgb, #ff6b75 10%, transparent);', 'background-image: none;', 'box-shadow: none;'])
                assert.ok(interaction.includes(declaration), state);
            assert.doesNotMatch(interaction, /(?:border(?:-width)?|padding|margin|min-height|min-width):/);
            assert.deepEqual(matching(':selected', state).flatMap(({ body }) =>
                [...body.matchAll(/border-color:\s*([^;]+);/g)].map(match => match[1])),
            ['color-mix(in srgb, #FCEE0A 45%, transparent)', 'color-mix(in srgb, #ff6b75 55%, transparent)']);
        }
        assert.equal(matching(':selected')[1].selectors.join(), selector(':selected'));
    });

    test('application ordering is alphabetical with entry ties; search preserves all native ranks', () => {
        const list = [{ name: 'Zed', entry: 'zed' }, { name: 'Anki', entry: 'anki-alt' }, { name: 'Anki', entry: 'anki' }];
        const ranked = Array.from({ length: 40 }, (_, index) => ({ entry: `${index}` }));
        const apps = { list, fuzzy_query(query) { assert.equal(query, 'chr'); return ranked; } };
        assert.equal(Array.from(applicationsFor(apps), app => app.entry).join(), 'anki,anki-alt,zed');
        assert.equal(list[0].name, 'Zed');
        assert.equal(Array.from(applicationsFor(apps, '  '), app => app.entry).join(), 'anki,anki-alt,zed');
        assert.equal(applicationsFor(apps, ' chr '), ranked);
    });

    test('catalog invalidation is immediate on every event but refresh notifications coalesce', async () => {
        let entries = [{ name: 'Zed', entry: 'zed' }], reads = 0, searches = 0, changed = 0, signal;
        const pending = new Map();
        const { LauncherBackend } = await load('./backend.js', ['LauncherBackend'], {
            Apps: { Apps: class {
                get list() { reads++; return entries; }
                fuzzy_query(search) { searches++; assert.equal(search, 'z'); return entries; }
            } },
            Gio: { AppInfoMonitor: { get: () => ({
                connect(name, callback) { assert.equal(name, 'changed'); signal = callback; return 1; },
                disconnect(id) { assert.equal(id, 1); signal = null; },
            }) } },
            GLib: { PRIORITY_DEFAULT_IDLE: 0, SOURCE_REMOVE: false,
                idle_add(_priority, callback) { pending.set(1, callback); return 1; },
                source_remove(id) { pending.delete(id); } },
        });
        const backend = new LauncherBackend(() => changed++);
        const first = backend.applications;
        assert.equal(backend.applications, first);
        assert.equal(backend.query('  '), first);
        assert.equal(reads, 1);
        backend.query(' z ');
        backend.query('z');
        assert.equal(searches, 2, 'Native ranking must not be cached');
        signal();
        assert.notEqual(backend.applications, first);
        entries = [{ name: 'Zoom', entry: 'zoom' }];
        signal();
        assert.equal(backend.applications[0].entry, 'zoom');
        assert.equal(reads, 3, 'A second event must invalidate even before the idle refresh');
        assert.equal(pending.size, 1);
        const refresh = pending.get(1);
        pending.clear();
        refresh();
        assert.equal(changed, 1);
        signal();
        backend.dispose();
        assert.equal(pending.size, 0);
        assert.equal(signal, null);
    });

    test('monitor helper is bounded, falls back safely, and never runs for explicit monitors', async () => {
        for (const [reply, warning] of [[{ name: 'DP-2' }, false], [{ name: 'removed' }, false], [{ name: null }, false],
            [{ name: 3 }, true], [null, true], ['malformed JSON', true], [new Error('helper failed'), true]]) {
            const calls = [], warnings = [];
            const available = ['DP-1', 'DP-2'].map(name => ({ get_connector: () => name }));
            const { focusedMonitor, monitors } = await load('./backend.js', ['focusedMonitor', 'monitors'], {
                GLib: { getenv: () => '/packaged/panel-launcher' },
                Gio: { SubprocessFlags: { STDERR_SILENCE: 4 } },
                Gdk: { Display: { get_default: () => ({ get_monitors: () => available }) } },
                async communicate(argv, options) {
                    calls.push(Array.from(argv));
                    assert.equal(options.maxBytes, 65536);
                    assert.equal(options.timeoutMs, 10000);
                    assert.equal(options.flags, 4);
                    assert.equal(options.errors.failed, 'Focused-monitor helper failed.');
                    if (reply instanceof Error) throw reply;
                    return typeof reply === 'string' ? reply : JSON.stringify(reply);
                },
                console: { warn: message => warnings.push(message) },
            });
            assert.equal(await focusedMonitor('DP-2'), available[1]);
            await assert.rejects(focusedMonitor('missing'), /No connected monitor named missing/);
            assert.equal(calls.length, 0);
            assert.equal(Array.from(monitors()).length, 2);
            assert.equal(await focusedMonitor(), available[reply?.name === 'DP-2' ? 1 : 0]);
            assert.deepEqual(calls, [['/packaged/panel-launcher', 'runtime', 'focused-output']]);
            assert.equal(warnings.length, Number(warning));
            if (warning) assert.match(warnings[0], /^Launcher monitor lookup:/);
            available.length = 0;
            await assert.rejects(focusedMonitor(), /No connected monitor is available/);
        }
    });

    test('launcher requests serialize lookups, recover from errors, and configure local reply diagnostics', async () => {
        const events = [], replies = [], warnings = [];
        let finishLookup;
        const lookup = new Promise(resolve => { finishLookup = resolve; });
        const { LauncherApplication } = await load('./frontend.js', ['LauncherApplication'], {
            focusedMonitor: async name => {
                events.push(`lookup:${name}`);
                if (name === 'missing') throw new Error('missing monitor');
                return lookup;
            },
            replyAndClose(connection, response, options) {
                replies.push({ connection, response, options });
                return new Promise(() => {}); // Reply/close completion must not stall the request queue.
            },
            console: { warn: message => warnings.push(message), error: message => warnings.push(message) },
        });
        const app = new LauncherApplication();
        await assert.rejects(app.handleRequest('{"command":"status"}'), /launcher is starting/);
        app.panel = { opened: false, backend: { applications: [] },
            toggle(monitor) { events.push(`toggle:${monitor}`); }, close() { events.push('close'); } };
        const requests = ['{"command":"toggle","monitor":"DP-2"}', 'bad JSON',
            '{"command":"toggle","monitor":"missing"}', '{"command":"close"}'];
        for (const text of requests) assert.equal(app.vfunc_request(text, text), undefined);
        await Promise.resolve();
        assert.deepEqual(events, ['lookup:DP-2']);
        assert.equal(replies.length, 0);
        finishLookup('DP-2');
        await app.requests;
        assert.deepEqual(events, ['lookup:DP-2', 'toggle:DP-2', 'lookup:missing', 'close']);
        assert.deepEqual(replies.map(reply => reply.connection), requests);
        assert.equal(replies[0].response, 'ok');
        assert.equal(typeof JSON.parse(replies[1].response).error, 'string');
        assert.equal(JSON.parse(replies[2].response).error, 'missing monitor');
        assert.equal(replies[3].response, 'ok', 'A rejected request must not poison the queue');
        for (const { options } of replies) {
            assert.equal(options.label, 'Launcher');
            assert.equal(options.asyncClose, true);
        }
        replies[0].options.onError('reply', new Error('write failed'));
        replies[0].options.onError('close', new Error('close failed'));
        assert.deepEqual(warnings, ['Launcher reply: write failed', 'Launcher connection: close failed']);
    });
}

async function smokeTests() {
    const { default: GLib } = await import('gi://GLib');
    const { default: Gio } = await import('gi://Gio');
    const { default: Gtk } = await import('gi://Gtk?version=4.0');
    const { default: Gdk } = await import('gi://Gdk?version=4.0');
    GLib.setenv('APP_LAUNCHER_IMPORT_ONLY', '1', true);
    const { LauncherApplication, LauncherPanel } = await import('./frontend.js');
    const { monitors } = await import('./backend.js');
    function assert(condition, message) { if (!condition) throw new Error(message); }
    function equal(actual, expected, message) { assert(actual === expected, `${message}: expected ${expected}, got ${actual}`); }
    function same(actual, expected, message) {
        assert(actual.length === expected.length && actual.every((item, index) => item === expected[index]), message);
    }
    function children(widget) {
        const result = [];
        for (let child = widget.get_first_child(); child; child = child.get_next_sibling()) result.push(child);
        return result;
    }
    function tooltipFree(widget) {
        assert(!widget.has_tooltip && !widget.tooltip_text && !widget.tooltip_markup, 'Entire widget tree must be tooltip-free');
        children(widget).forEach(tooltipFree);
    }
    const wait = ms => new Promise(resolve => GLib.timeout_add(GLib.PRIORITY_DEFAULT, ms, () => {
        resolve();
        return GLib.SOURCE_REMOVE;
    }));
    async function until(check, description) {
        const deadline = Date.now() + 4000;
        while (Date.now() < deadline) {
            if (check()) return;
            await wait(20);
        }
        throw new Error(`Timed out waiting for ${description}`);
    }

    Gtk.init();
    GLib.unsetenv('LD_PRELOAD');
    const provider = new Gtk.CssProvider();
    let cssErrors = 0;
    provider.connect('parsing-error', (_provider, _section, error) => { cssErrors++; console.error(error.message); });
    provider.load_from_file(Gio.File.new_for_uri(import.meta.url).get_parent().get_child('style.css'));
    Gtk.StyleContext.add_provider_for_display(Gdk.Display.get_default(), provider, Gtk.STYLE_PROVIDER_PRIORITY_USER + 1);
    const app = new LauncherApplication({ instance_name: 'app-launcher-smoke' });
    app.register(null);
    const panel = new LauncherPanel(app);
    if (GLib.getenv('GDK_BACKEND') === 'x11') panel.window.set_default_size(1280, 800);
    app.panel = panel;
    const searchFocused = () => panel.window.get_focus() === panel.search || Boolean(panel.window.get_focus()?.is_ancestor(panel.search));
    const move = (step, count, control = false) => panel.grid.emit('move-cursor', Gtk.MovementStep[step], count, false, control);
    const noSelection = () => {
        equal(panel.selected, -1, 'No default selected tile');
        equal(panel.grid.get_selected_children().length, 0, 'Native selection must be empty');
    };
    const displayed = () => children(panel.grid).filter(child => child.get_child_visible()).map(child => child.application);
    try {
        equal(cssErrors, 0, 'CSS parsing errors');
        const monitorModel = Gdk.Display.get_default().get_monitors();
        equal(monitors().length, monitorModel.get_n_items(), 'Native monitor enumeration');
        const monitor = monitorModel.get_item(0);
        panel.toggle(monitor);
        await until(() => panel.revealer.child_revealed, 'opening animation');
        assert(panel.opened && panel.window.visible, 'Opening must map and reveal the panel');
        const catalog = panel.backend.applications;
        // The supplied runtime may add packaged apps alongside our isolated desktop entries.
        equal(catalog.filter(app => /^UI Fixture \d{2}$/.test(app.name)).length, 12, 'Isolated fixture catalog');
        same(panel.results, catalog, 'Unfiltered grid must include the complete native catalog');
        const originals = new Map(panel.results.map((application, index) => [application, panel.tiles[index]]));
        const attached = () => {
            equal(children(panel.grid).length, originals.size, 'Search must not detach catalog tiles');
            assert(children(panel.grid).every(child => child === originals.get(child.application)), 'Reuse attached native tiles');
            same(panel.tiles, panel.results.map(application => originals.get(application)), 'Result tiles must reuse native objects');
            same(displayed(), panel.results, 'Actual FlowBox filtering/order must match results');
            tooltipFree(panel.root);
        };
        attached();
        const [valid, bounds] = panel.card.compute_bounds(panel.root);
        assert(valid && bounds.get_x() === 12, 'Panel must sit 12px from the left edge');
        assert(Math.abs(bounds.get_y() + bounds.get_height() - (panel.root.get_height() - 48)) < 2, 'Keep 48px bottom clearance');
        assert(panel.search instanceof Gtk.SearchEntry && !('message' in panel) && !('count' in panel), 'Search/grid only, no footer');
        assert(panel.grid instanceof Gtk.FlowBox && panel.grid.activate_on_single_click, 'Use native single-click FlowBox');
        for (const [actual, expected, name] of [[panel.columns, 4, 'columns'], [panel.scroll.get_height(), 190, 'two-row viewport'],
            [panel.card.get_width(), 318, 'card width'], [panel.card.get_height(), 258, 'card height'],
            [panel.revealer.transition_duration, 260, 'animation duration']]) equal(actual, expected, name);
        assert(searchFocused(), 'Search must receive initial focus');
        noSelection();
        assert(!panel.keyPressed(Gdk.KEY_Left), 'Search retains normal cursor navigation');
        panel.keyPressed(Gdk.KEY_Down);
        equal(panel.window.get_focus(), panel.tiles[0], 'Down from search focuses first app');
        equal(panel.tiles[0].get_allocated_width(), 66, 'Tile width');
        equal(panel.tiles[0].get_allocated_height(), 90, 'Tile height');
        const [, secondRow] = panel.tiles[4].compute_bounds(panel.grid);
        const [, thirdRow] = panel.tiles[8].compute_bounds(panel.grid);
        assert(secondRow.get_y() + secondRow.get_height() <= 190 && thirdRow.get_y() >= 190, 'Exactly two rows fit in viewport');
        for (const [step, count, selected] of [['DISPLAY_LINES', 1, 4], ['VISUAL_POSITIONS', 1, 5], ['DISPLAY_LINES', -1, 1]]) {
            move(step, count);
            equal(panel.selected, selected, `Native ${step} movement`);
        }
        move('BUFFER_ENDS', 1);
        await until(() => panel.scroll.get_vadjustment().value > 0, 'native End scrolling');
        equal(panel.selected, catalog.length - 1, 'End selects last app');
        move('BUFFER_ENDS', -1);
        await until(() => panel.scroll.get_vadjustment().value === 0, 'native Home scrolling');
        equal(panel.selected, 0, 'Home selects first app');
        panel.keyPressed(Gdk.KEY_Up);
        assert(searchFocused(), 'Up from first row returns to search');

        const application = catalog[0];
        panel.search.set_text(application.name);
        const expected = panel.backend.query(application.name);
        await until(() => panel.results.map(app => app.entry).join() === expected.map(app => app.entry).join(), 'native ranked search');
        assert(panel.results.includes(application) && panel.selected === 0, 'Search selects first ranked result and includes exact name');
        assert(searchFocused(), 'Filtering must retain search focus');
        attached();
        const launch = panel.backend.launch;
        let activated;
        try {
            panel.backend.launch = application => { activated = application; return false; };
            panel.tiles[0].emit('activate');
            equal(activated, panel.results[0], 'Activation dispatches current ranked result without starting an app');
            assert(panel.opened && panel.tiles[0].has_css_class('launch-error'), 'Failed activation stays open with error styling');
            panel.render();
            assert(!panel.tiles[0].has_css_class('launch-error'), 'Render clears transient activation errors');
        } finally { panel.backend.launch = launch; }
        panel.search.set_text('this-app-does-not-exist-394853');
        await until(() => panel.results.length === 0, 'empty search');
        equal(panel.gridStack.visible_child_name, 'empty', 'No-match state');
        attached();
        panel.keyPressed(Gdk.KEY_Return);
        assert(panel.opened, 'Enter with no results is harmless');
        panel.search.set_text('');
        await until(() => panel.results.length === catalog.length, 'cleared search');
        attached();
        noSelection();
        panel.select(5, true);
        panel.render();
        equal(panel.selected, 0, 'Grid-focused refresh selects first result');
        equal(panel.window.get_focus(), panel.tiles[0], 'Grid-focused refresh resets native cursor');
        panel.keyPressed(Gdk.KEY_f, Gdk.ModifierType.CONTROL_MASK);
        assert(searchFocused(), 'Ctrl+F returns to search');
        panel.backend.apps.reload();
        panel.backend.appMonitor.emit('changed');
        await until(() => panel.catalog !== catalog, 'native catalog replacement');
        assert(panel.backend.applications !== catalog, 'Catalog change invalidates alphabetical cache');
        assert(panel.tiles.every(tile => ![...originals.values()].includes(tile)), 'Reload replaces stale native tile metadata');
        equal(panel.tileCache.size, panel.backend.applications.length, 'Reload releases obsolete cached tiles');
        assert(searchFocused(), 'Catalog refresh retains search focus');
        tooltipFree(panel.root);

        const query = panel.backend.query;
        try {
            for (const ranks of [[9, 3, 7], [11, 1, 6, 3, 8]]) {
                const ranked = ranks.map(index => panel.backend.applications[index]);
                panel.backend.query = () => ranked;
                panel.render();
                same(displayed(), ranked, 'Finite gint comparator must sort mixed matches/nonmatches by native rank');
            }
            panel.backend.query = () => panel.backend.applications.slice(0, 10);
            panel.render();
            await until(() => {
                const [valid, bounds] = panel.tiles[9].compute_bounds(panel.grid);
                return valid && bounds.get_y() >= 180;
            }, 'partial last-row layout');
            for (const [selected, direction, focused, key, destination] of [[0, 1, 4, Gdk.KEY_Up, 0], [7, -1, 3, Gdk.KEY_Down, 7]]) {
                panel.select(selected, true);
                move('DISPLAY_LINES', direction, true);
                equal(panel.selected, selected, 'Ctrl movement retains selection');
                equal(panel.window.get_focus(), panel.tiles[focused], 'Ctrl movement changes native focus cursor');
                assert(!panel.keyPressed(key), 'Movement boundary must use focus, not selection');
                move('DISPLAY_LINES', -direction);
                equal(panel.window.get_focus(), panel.tiles[destination], 'Normal movement follows focused row');
            }
            if (!panel.keyPressed(Gdk.KEY_Down)) move('DISPLAY_LINES', 1);
            equal(panel.selected, 9, 'Down clamps to partial final row');
            move('VISUAL_POSITIONS', 1);
            equal(panel.selected, 9, 'Right cannot move past final result');
        } finally {
            panel.backend.query = query;
            panel.search.grab_focus();
            panel.render();
        }
        const status = JSON.parse(await app.handleRequest('{"command":"status"}'));
        assert(status.opened && status.applications === panel.backend.applications.length
            && status.monitor === monitor.get_connector(), 'Status reports native state');
        await app.handleRequest('{"command":"close"}');
        assert(!panel.opened && panel.window.visible, 'Close keeps surface mapped during animation');
        await until(() => !panel.window.visible, 'closing animation');
        assert(!panel.revealer.child_revealed, 'Close finishes revealing transition');
        panel.toggle(monitor);
        await wait(50);
        panel.close();
        await wait(50);
        panel.toggle(monitor);
        await until(() => panel.revealer.child_revealed, 'rapid reopen');
        assert(panel.opened && panel.window.visible, 'Reopening reverses close without a stale hide');
        noSelection();
        const settings = Gtk.Settings.get_default();
        const animations = settings.gtk_enable_animations;
        try {
            settings.gtk_enable_animations = false;
            const [valid, bounds] = panel.card.compute_bounds(panel.root);
            assert(valid, 'Outside-click test requires allocated card bounds');
            const controllers = panel.root.observe_controllers();
            const click = Array.from({ length: controllers.get_n_items() }, (_, index) => controllers.get_item(index))
                .find(controller => controller instanceof Gtk.GestureClick);
            click.emit('released', 1, bounds.get_x(), bounds.get_y());
            assert(panel.opened, 'Clicking the card boundary must not dismiss it');
            click.emit('released', 1, bounds.get_x() - 1, bounds.get_y());
            assert(!panel.opened && !panel.window.visible, 'Outside click closes immediately with animations disabled');
            panel.toggle(monitor);
            assert(panel.window.visible && panel.opened, 'Disabled animations still reopen');
            panel.close(false);
            assert(!panel.window.visible, 'Explicit nonanimated close hides immediately');
            panel.toggle(monitor);
        } finally { settings.gtk_enable_animations = animations; }
        try {
            panel.backend.launch = application => { activated = application; return true; };
            panel.tiles[0].emit('activate');
            equal(activated, panel.results[0], 'Successful activation dispatches the native application without launching it');
            assert(!panel.opened, 'Successful activation dismisses panel');
        } finally { panel.backend.launch = launch; }
        print(`PASS: native GTK/Astal checks; ${status.applications} apps; card 318×258`);
    } finally {
        panel.dispose();
        Gtk.StyleContext.remove_provider_for_display(Gdk.Display.get_default(), provider);
    }
}

if (typeof process !== 'undefined' && process.versions?.node) {
    if (process.argv.includes('--ui')) {
        try { await isolatedUI(); }
        catch (error) { console.error(error.stack || error); process.exitCode = 1; }
    } else await unitTests();
} else await smokeTests();
