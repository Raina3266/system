// One cross-runtime entry point; runtime-specific imports stay inside their modes.
// Node: node --test test.js; node test.js --ui --ags PATH --palette FILE
// GJS: panel-gjs -m test.js native|runtime|ui|application
// ui/application require --ui's authenticated Xvfb and service-free private bus.

async function isolatedUI(argv) {
    const fs = await import('node:fs');
    const { spawn } = await import('node:child_process');
    const { tmpdir, homedir } = await import('node:os');
    const { dirname, isAbsolute, join, resolve, delimiter } = await import('node:path');
    const { fileURLToPath } = await import('node:url');
    const { randomBytes } = await import('node:crypto');
    const { parseArgs } = await import('node:util');
    const { setTimeout: delay } = await import('node:timers/promises');
    const { values } = parseArgs({ args: argv, options: {
        ags: { type: 'string', default: process.env.NETWORK_TEST_AGS || 'ags' },
        gjs: { type: 'string', default: process.env.NETWORK_TEST_GJS },
        xvfb: { type: 'string', default: process.env.NETWORK_TEST_XVFB || 'Xvfb' },
        'dbus-daemon': { type: 'string', default: process.env.NETWORK_TEST_DBUS_DAEMON || 'dbus-daemon' },
        palette: { type: 'string', default: process.env.NETWORK_TEST_PALETTE || process.env.NETWORK_PANEL_PALETTE },
        screen: { type: 'string', default: '1280x800x24' },
        timeout: { type: 'string', default: '30' },
        help: { type: 'boolean', short: 'h' },
    } });
    if (values.help) {
        console.log('Usage: node test.js --ui --ags PATH --palette FILE [--gjs PATH]'
            + '\n  [--xvfb PATH] [--dbus-daemon PATH] [--screen WIDTHxHEIGHTx24] [--timeout SECONDS]'
            + '\nRuns real-import GTK UI and closed-only bootstrap checks on private X11/D-Bus.'
            + '\nThe companion panel-gjs defaults to the executable beside --ags.'
            + '\nTimeout: 30 seconds per phase by default; maximum 300 seconds.');
        return;
    }
    function expand(path) {
        return path === '~' ? homedir() : path.startsWith('~/') ? join(homedir(), path.slice(2)) : path;
    }
    function executable(value) {
        const path = expand(value);
        const candidates = isAbsolute(path) || path.includes('/') ? [resolve(path)]
            : (process.env.PATH || '').split(delimiter).map(directory => resolve(directory, path));
        for (const candidate of candidates) {
            try {
                fs.accessSync(candidate, fs.constants.X_OK);
                if (fs.statSync(candidate).isFile()) return fs.realpathSync(candidate);
            } catch (_) { /* Try the next PATH entry. */ }
        }
        throw new Error(`Executable not found: ${value}`);
    }
    if (!values.palette) throw new Error('--palette or NETWORK_TEST_PALETTE/NETWORK_PANEL_PALETTE is required');
    const palette = resolve(expand(values.palette));
    if (!fs.statSync(palette).isFile()) throw new Error(`Palette not found: ${palette}`);
    if (!/^[1-9]\d*x[1-9]\d*x24$/.test(values.screen)) throw new Error('--screen must be WIDTHxHEIGHTx24');
    const timeout = Number(values.timeout);
    if (!(timeout > 0 && timeout <= 300)) throw new Error('--timeout must be greater than zero and at most 300 seconds');
    const ags = executable(values.ags);
    const gjs = executable(values.gjs || join(dirname(ags), 'panel-gjs'));
    const xvfb = executable(values.xvfb);
    const dbus = executable(values['dbus-daemon']);
    const self = fileURLToPath(import.meta.url);
    const source = dirname(self);
    for (const name of ['backend.js', 'frontend.js', 'logic.js', 'style.css', 'package.json', 'test.js']) {
        if (!fs.statSync(join(source, name)).isFile()) throw new Error(`Missing real source import: ${join(source, name)}`);
    }

    const root = fs.mkdtempSync(join(tmpdir(), 'network-ui-'));
    fs.chmodSync(root, 0o700);
    const env = { ...process.env };
    for (const name of ['DISPLAY', 'WAYLAND_DISPLAY', 'WAYLAND_SOCKET', 'XAUTHORITY', 'LD_PRELOAD',
        'GTK_THEME', 'GTK_PATH', 'GTK_MODULES', 'GDK_DEBUG', 'G_DEBUG', 'DCONF_PROFILE',
        'DBUS_SESSION_BUS_ADDRESS', 'DBUS_SYSTEM_BUS_ADDRESS', 'DBUS_STARTER_ADDRESS',
        'DBUS_STARTER_BUS_TYPE', 'NIRI_SOCKET', 'NIRI_CONFIG', 'XDG_CURRENT_DESKTOP']) delete env[name];
    const processes = [];
    const interrupted = new AbortController();
    const onInterrupt = signal => interrupted.abort(new Error(`Interrupted by ${signal}; private processes terminated`));
    const onSigint = () => onInterrupt('SIGINT');
    const onSigterm = () => onInterrupt('SIGTERM');
    process.on('SIGINT', onSigint);
    process.on('SIGTERM', onSigterm);

    function killGroup(record, signal) {
        if (!record.child.pid) return;
        try { process.kill(-record.child.pid, signal); }
        catch (error) { if (error.code !== 'ESRCH') throw error; }
    }
    async function stop(record) {
        if (record.stopped) return;
        // A wrapper may exit before its descendants. Always kill the group too.
        killGroup(record, 'SIGTERM');
        await Promise.race([record.done, delay(2000, undefined, { ref: false })]);
        killGroup(record, 'SIGKILL');
        const result = await Promise.race([record.done, delay(2000, undefined, { ref: false }).then(() => null)]);
        if (!result) throw new Error(`Could not reap private process group ${record.child.pid}`);
        record.stopped = true;
    }
    async function bounded(promise, milliseconds, message) {
        interrupted.signal.throwIfAborted();
        let timer, abort;
        try {
            return await Promise.race([promise, new Promise((_, reject) => {
                timer = setTimeout(() => reject(new Error(message)), milliseconds);
                abort = () => reject(interrupted.signal.reason);
                interrupted.signal.addEventListener('abort', abort, { once: true });
            })]);
        } finally {
            clearTimeout(timer);
            if (abort) interrupted.signal.removeEventListener('abort', abort);
        }
    }
    function start(command, args, logPath, ready = false) {
        const descriptor = fs.openSync(logPath, 'w', 0o600);
        let child;
        try {
            child = spawn(command, args, { cwd: source, env, detached: true,
                stdio: ['ignore', 'pipe', 'pipe', ...(ready ? ['pipe'] : [])] });
        } catch (error) { fs.closeSync(descriptor); throw error; }
        const record = { child, logPath, bytes: 0, service: ready };
        let limited;
        record.limit = new Promise(resolve => { limited = resolve; });
        // Bound diagnostics as well as runtime; never let a regressed child fill disk.
        const maximum = 4 * 1024 * 1024;
        function capture(chunk) {
            const remaining = maximum - record.bytes;
            if (remaining > 0) fs.writeSync(descriptor, chunk.subarray(0, remaining));
            record.bytes += chunk.length;
            if (record.bytes > maximum && !record.failure) {
                record.failure = new Error('Private process exceeded its 4 MiB diagnostic limit');
                limited(record.failure);
                killGroup(record, 'SIGTERM');
            }
        }
        child.stdout.on('data', capture);
        child.stderr.on('data', capture);
        let spawnError;
        child.once('error', error => { spawnError = error; });
        record.done = new Promise(resolve => child.once('close', (code, signal) => {
            fs.closeSync(descriptor);
            record.result = { code, signal, error: spawnError };
            resolve(record.result);
        }));
        processes.push(record);
        return record;
    }
    async function startService(command, args, name) {
        const record = start(command, args, join(root, `${name}.log`), true);
        let receive, end;
        const ready = new Promise((resolve, reject) => {
            let data = Buffer.alloc(0);
            receive = chunk => {
                data = Buffer.concat([data, chunk]);
                if (data.length > 4096) reject(new Error('Invalid service readiness response'));
                else if (data.includes(10)) resolve(data.toString('utf8').trim());
            };
            end = () => reject(new Error('Service exited before reporting readiness'));
            record.child.stdio[3].on('data', receive);
            record.child.stdio[3].once('end', end);
        });
        try {
            const response = await bounded(Promise.race([ready,
                record.done.then(result => { throw result.error || new Error('Service exited during startup'); }),
                record.limit.then(error => { throw error; }),
            ]), 5000, 'Service startup exceeded 5s');
            if (record.result) throw record.result.error || new Error('Service exited during startup');
            return response;
        } catch (error) {
            throw new Error(`${name}: ${error.message}\n${fs.readFileSync(record.logPath, 'utf8')}`);
        } finally {
            record.child.stdio[3].removeListener('data', receive);
            record.child.stdio[3].removeListener('end', end);
        }
    }
    function authority(path, cookie, display = '') {
        const family = Buffer.alloc(2);
        family.writeUInt16BE(65535);
        const fields = [Buffer.alloc(0), Buffer.from(display), Buffer.from('MIT-MAGIC-COOKIE-1'), cookie];
        fs.writeFileSync(path, Buffer.concat([family, ...fields.flatMap(value => {
            const length = Buffer.alloc(2);
            length.writeUInt16BE(value.length);
            return [length, value];
        })]), { mode: 0o600 });
        fs.chmodSync(path, 0o600);
    }
    async function runTest(phase) {
        const command = [gjs, '-m', self, phase];
        console.log(`RUN ${command.map(value => JSON.stringify(value)).join(' ')} (cwd=${source})`);
        const record = start(gjs, command.slice(1), join(root, `${phase}.log`));
        let failure;
        try {
            const result = await bounded(Promise.race([record.done,
                record.limit.then(error => { throw error; }),
            ]), timeout * 1000, `${phase} test exceeded ${timeout}s`);
            if (result.error) throw result.error;
            if (result.code !== 0) throw new Error(`${phase} test exited ${result.code ?? result.signal}`);
        } catch (error) { failure = error; }
        finally { await stop(record); }
        const output = fs.readFileSync(record.logPath, 'utf8');
        if (failure || record.failure) throw new Error(`${(failure || record.failure).message}:\n${output}`);
        return output;
    }
    try {
        for (const name of ['HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_STATE_HOME', 'XDG_DATA_HOME',
            'XDG_RUNTIME_DIR', 'XDG_CONFIG_DIRS', 'XDG_DATA_DIRS']) {
            const path = join(root, name.toLowerCase());
            fs.mkdirSync(path, { mode: 0o700 });
            env[name] = path;
        }
        Object.assign(env, { GDK_BACKEND: 'x11', GSK_RENDERER: 'cairo', GTK_A11Y: 'none',
            GSETTINGS_BACKEND: 'memory', GIO_USE_VFS: 'local', NO_AT_BRIDGE: '1', LIBGL_ALWAYS_SOFTWARE: '1',
            NETWORK_UI_ISOLATED: '1', NETWORK_PANEL_IMPORT_ONLY: '1',
            NETWORK_PANEL_CONFIG: join(source, 'frontend.js'), NETWORK_PANEL_PALETTE: palette, NETWORK_TEST_AGS: ags });
        const attempts = join(root, 'external-call');
        const guard = join(root, 'external-guard.cjs');
        fs.writeFileSync(guard, `#!${process.execPath}\nconst fs = require('node:fs');\n`
            + `fs.appendFileSync(${JSON.stringify(attempts)}, JSON.stringify(process.argv) + '\\n');\nprocess.exit(125);\n`,
            { mode: 0o700 });
        for (const name of ['DASHBOARD_PANEL', 'MEDIA_PANEL']) env[name] = guard;
        const config = join(root, 'bus.conf');
        const socket = join(env.XDG_RUNTIME_DIR, 'bus').replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;');
        // Omit all service directories: nothing, including desktop portals, can autoactivate.
        fs.writeFileSync(config, '<busconfig><type>session</type>'
            + `<listen>unix:path=${socket}</listen><auth>EXTERNAL</auth>`
            + '<policy context="default"><allow own="*"/><allow send_destination="*"/>'
            + '<allow receive_sender="*"/></policy></busconfig>', { mode: 0o600 });
        const address = await startService(dbus, ['--nofork', '--print-address=3', `--config-file=${config}`], 'dbus');
        if (!address.startsWith(`unix:path=${join(env.XDG_RUNTIME_DIR, 'bus')},`))
            throw new Error('Private D-Bus did not report the expected private Unix address');
        env.DBUS_SESSION_BUS_ADDRESS = env.DBUS_SYSTEM_BUS_ADDRESS = address;
        const auth = join(root, 'Xauthority');
        const cookie = randomBytes(16);
        authority(auth, cookie);
        env.XAUTHORITY = auth;
        const display = await startService(xvfb, ['-displayfd', '3', '-screen', '0', values.screen,
            '-nolisten', 'tcp', '-auth', auth, '-noreset'], 'xvfb');
        if (!/^\d+$/.test(display)) throw new Error(`Xvfb did not report a display number: ${JSON.stringify(display)}`);
        authority(auth, cookie, display);
        env.DISPLAY = `:${display}`;
        const counts = {};
        for (const [phase, title, minimum] of [['ui', 'network GTK UI', 85], ['application', 'network application bootstrap', 36]]) {
            const output = await runTest(phase);
            if (fs.existsSync(attempts)) throw new Error(`${phase} attempted an external panel call:\n${output}`);
            for (const service of processes.filter(record => record.service)) {
                if (service.result || service.failure) throw new Error(`Private service failed:\n${fs.readFileSync(service.logPath, 'utf8')}`);
            }
            const expected = ['init_and_get_layer_shell_global:', 'window.vala:59:', 'GtkWindow is not a layer surface.',
                'MESA-EGL: warning: DRI3 error:', 'MESA-EGL: warning: Ensure your X server supports DRI3'];
            const lines = output.split(/\r?\n/);
            const diagnostics = lines.filter(line => /\b(?:CRITICAL|WARNING)\b|JS ERROR/i.test(line));
            const unexpected = diagnostics.filter(line => !expected.some(value => line.includes(value)));
            const summary = output.match(new RegExp(`PASS ${title}: (\\d+) assertions;`));
            if (unexpected.length || !summary || Number(summary[1]) < minimum || output.includes('SKIP '))
                throw new Error(`${phase} assertions incomplete or unexpected GTK/GJS diagnostics:\n${output}`);
            counts[phase] = Number(summary[1]);
            for (const line of lines) {
                const marker = line.search(/PASS |LAYOUT |LIMITATION /);
                if (marker !== -1) console.log(line.slice(marker));
            }
            console.log(`NOTE ${phase}: ${diagnostics.length} expected X11 layer-shell/Mesa diagnostics; no unexpected diagnostics`);
        }
        console.log(`PASS combined validation: ${counts.ui} UI + ${counts.application} bootstrap assertions; `
            + 'real imports, no skipped checks or external calls');
    } finally {
        try {
            const cleanup = [];
            for (const record of [...processes].reverse()) {
                try { await stop(record); } catch (error) { cleanup.push(error); }
            }
            if (cleanup.length) throw new AggregateError(cleanup, 'Private process cleanup failed');
        } finally {
            fs.rmSync(root, { recursive: true, force: true });
            process.removeListener('SIGINT', onSigint);
            process.removeListener('SIGTERM', onSigterm);
        }
    }
}

// Consolidated suites follow. Each owns its original fixture/helper namespace.

async function nodeControllerTests() {
    const { default: assert } = await import('node:assert/strict');
    const { describe, it } = await import('node:test');
    const { NetworkController, parseRequest } = await import('./logic.js');
    const { readFileSync } = await import('node:fs');
    const { default: vm } = await import('node:vm');
    const frontend = readFileSync(new URL('./frontend.js', import.meta.url), 'utf8');
    it('hides the password row for open and enhanced-open Wi-Fi details', () => {
        const expression = frontend.match(/const hasPassword = ([\s\S]*?);/)?.[1];
        assert.ok(expression, 'The real details password visibility condition is available');
        for (const securityType of ['open', 'owe', 'wep', 'psk', 'sae', 'enterprise']) {
            for (const connected of [false, true]) {
                const row = { wireless: true, known: !connected, connected, securityType };
                assert.equal(vm.runInNewContext(expression, { row }), !['open', 'owe'].includes(securityType),
                    `${securityType}, connected=${connected}`);
            }
        }
        assert.match(frontend, /this\.passwordCaption\.visible = this\.passwordLine\.visible = this\.reveal\.visible = hasPassword/);
    });

    it('omits Wi-Fi saved status and hides empty metadata without losing connection progress', () => {
        const statements = frontend.match(/row\.metadata\.set_label\([\s\S]*?row\.metadata\.visible = [^;]+;/)?.[0];
        assert.ok(statements);
        const render = (dataRow, undetected = false) => {
            const metadata = { set_label(text) { this.label = text; } };
            vm.runInNewContext(statements, { row: { metadata }, dataRow, undetected });
            return { label: metadata.label, visible: metadata.visible };
        };
        for (const known of [false, true]) {
            const wifi = { wireless: true, known };
            assert.deepEqual(render(wifi), { label: '', visible: false });
            assert.deepEqual(render({ ...wifi, connected: true }), { label: 'Connected', visible: true });
            assert.deepEqual(render({ ...wifi, changing: true }), { label: 'Working…', visible: true });
            assert.deepEqual(render({ ...wifi, connected: true, changing: true }),
                { label: 'Connected · Working…', visible: true });
            assert.deepEqual(render({ wireless: false, known, interface: 'eth0' }),
                { label: `${known ? 'Saved' : 'Not saved'} · eth0`, visible: true });
        }
        assert.deepEqual(render({ wireless: true, known: true }, true), { label: '', visible: false });
    });

    it('does not open details for Saved-tab rows, but keeps Nearby details', () => {
        const method = frontend.match(/    toggleInfo\(key\) \{([\s\S]*?)\n    \}/)?.[1];
        assert.ok(method);
        const toggle = vm.runInNewContext(`(function(key) {${method}\n})`, {
            isUndetectedSaved: row => row.strength === null,
        });
        const calls = [];
        const view = { listTab: 'saved', maskSecret() {}, clearPassword() {}, controller: {
            page: 'list', rows: [{ key: 'saved', strength: null }, { key: 'nearby', strength: 50 }],
            info(key) { calls.push(key); },
        } };
        toggle.call(view, 'saved');
        toggle.call(view, 'nearby');
        view.listTab = 'nearby';
        toggle.call(view, 'saved');
        assert.deepEqual(calls, []);
        toggle.call(view, 'nearby');
        assert.deepEqual(calls, ['nearby']);
        assert.match(frontend, /row\.info\.visible = Boolean\(!undetected &&/);
    });

    it('draws the panel frame without a translucent inner rail', () => {
        const frame = frontend.match(/function frame\(palette\) \{([\s\S]*?)\n\}/)?.[1];
        assert.ok(frame);
        assert.doesNotMatch(frame, /railX|railTop|setLineWidth\(3\)/);
        assert.match(frame, /cr\.setLineWidth\(1\)/);
    });

    it('matches the audio panel background, yellow outline and cut-corner drawing', () => {
        const audio = readFileSync(new URL('../audio-control/astal/frontend.js', import.meta.url), 'utf8');
        const palette = { yellow: '#ffcc33', cyan: '#33ddff', background: '#110011' };
        const capture = (source, width, height) => {
            const definition = source.match(/function frame\(palette\) \{[\s\S]*?\n\}/)?.[0];
            assert.ok(definition);
            const frame = vm.runInNewContext(`${definition}\nframe;`, {
                Gtk: { DrawingArea: class {
                    set_draw_func(callback) { this.draw = callback; }
                } },
            });
            const calls = [];
            const cr = Object.fromEntries(['moveTo', 'lineTo', 'closePath', 'setSourceRGBA',
                'fillPreserve', 'setLineWidth', 'stroke'].map(name =>
                [name, (...args) => calls.push([name, ...args])]));
            const drawing = frame(palette);
            drawing.draw(drawing, cr, width, height);
            return calls;
        };
        for (const [width, height] of [[380, 416], [240, 180], [24, 32]]) {
            const actual = capture(frontend, width, height);
            assert.deepEqual(actual, capture(audio, width, height));
            assert.ok(actual.some(([name, ...args]) => name === 'setSourceRGBA'
                && args.every((value, index) => value === [37 / 255, 37 / 255, 47 / 255, 0.9][index])));
        }
    });

    it('network rows and profile choices use dim yellow hover and dim red selected outlines', () => {
        const css = readFileSync(new URL('./style.css', import.meta.url), 'utf8');
        const rows = css.slice(css.indexOf('.network-panel .network-row {'), css.indexOf('.network-panel .network-name {'));
        assert.match(rows, /border: 1px solid transparent;/);
        assert.doesNotMatch(rows, /\.network-row\.connected/,
            'connected state alone must not look selected');
        for (const state of [':hover', ':focus-within', '.expanded'])
            assert.ok(rows.includes(`.network-row${state}`));
        assert.match(rows, /border-color: alpha\(@np_yellow, 0\.45\);/);
        assert.match(rows, /\.network-row\.expanded \{\s*border-color: alpha\(@np_red, 0\.55\);/);
        assert.ok(rows.lastIndexOf('.network-row.expanded {') > rows.indexOf('.network-row:hover,'));
        assert.match(rows, /background: alpha\(@np_red, 0\.1\);/);
        const profiles = css.slice(css.indexOf('.network-panel dropdown.profile-choice listview row {'), css.indexOf('.network-panel scrolledwindow,'));
        assert.match(profiles, /border: 1px solid transparent;/);
        assert.match(profiles, /border-color: alpha\(@np_yellow, 0\.45\);/);
        assert.match(profiles, /row:selected \{\s*border-color: alpha\(@np_red, 0\.55\);/);
        assert.ok(profiles.indexOf('row:selected {') > profiles.indexOf('row:hover,'));
        assert.match(profiles, /background: alpha\(@np_red, 0\.1\);/);
        const details = css.match(/\.network-panel \.network-details \{([^}]+)\}/)?.[1];
        assert.match(details, /background: transparent;/);
    });

    it('keeps tabs, network lists and forms inset from the outer frame', () => {
        const css = readFileSync(new URL('./style.css', import.meta.url), 'utf8');
        assert.match(css, /\.network-content \{ padding: 12px; \}/);
    });

    it('uses a taller network card while keeping it bounded by the monitor', () => {
        const method = frontend.match(/    position\(\) \{([\s\S]*?)\n    \}/)?.[1];
        assert.ok(method);
        const position = vm.runInNewContext(`(function() {${method}\n})`);
        for (const [width, height, expected] of [[1280, 800, 480], [360, 240, 186], [16, 54, 0]]) {
            const panel = { monitor: { get_geometry: () => ({ width, height }) }, card: {}, preferredHeight: () => 480 };
            position.call(panel);
            assert.equal(panel.card.height_request, expected);
            assert.ok(panel.card.margin_top + expected <= height);
        }
    });

    it('fits six measured unsaved rows plus remembered rows, padding and the divider', () => {
        const method = frontend.match(/    preferredHeight\(width\) \{([\s\S]*?)\n    \}/)?.[1];
        assert.ok(method);
        const preferredHeight = vm.runInNewContext(`(function(width) {${method}\n})`, {
            Gtk: { Orientation: { VERTICAL: 1 } }, isUndetectedSaved: () => false,
        });
        const available = Array.from({ length: 8 }, (_, index) => ({ key: `available-${index}`, height: index < 6 ? 40 : 999 }));
        const remembered = { key: 'connected', known: true, height: 70, detailsHeight: 120 };
        const rows = [remembered, ...available];
        const panel = { controller: { rows }, rows: new Map(rows.map(data => [data.key, {
            widget: { get_style_context: () => ({
                get_padding: () => ({ left: 4, right: 4 }), get_border: () => ({ left: 1, right: 1 }),
            }), measure: () => [0, data.height + (data.detailsHeight || 0)] },
            details: { measure: () => [0, data.detailsHeight || 0] },
        }])), content: { get_style_context: () => ({ get_padding: () => ({ top: 12, bottom: 12, left: 12, right: 12 }) }) },
        networkLists: { margin_top: 6, margin_bottom: 6, margin_start: 6, margin_end: 6 },
        networkRows: { spacing: 8 }, networkSeparator: { measure: () => [0, 1] }, listTabs: { measure: () => [0, 32] } };
        assert.equal(preferredHeight.call(panel, 380), 435);
        available[5].height = 60;
        assert.equal(preferredHeight.call(panel, 380), 455, 'wrapped row heights increase the fit budget');
        panel.controller.rows = [remembered, available[0]];
        assert.equal(preferredHeight.call(panel, 380), 435, 'reserve six slots when fewer networks are found');
        panel.controller.rows = [remembered];
        assert.equal(preferredHeight.call(panel, 380), 480, 'retain a usable fallback without unsaved rows');
    });

    it('places the network expand button on a second line without reserving empty unsaved rows', () => {
        assert.match(frontend, /row\.heading = box\(false, 4, \[row\.marker, row\.name, row\.status, row\.connect, row\.forget, row\.remove\]\)/);
        assert.match(frontend, /row\.secondary = box\(false, 4, \[row\.metadata, row\.info\]\)/);
        assert.match(frontend, /row\.secondary\.visible = row\.metadata\.visible \|\| row\.info\.visible/);
    });

    it('leaves a gap between expanded details text and the left accent line', () => {
        const css = readFileSync(new URL('./style.css', import.meta.url), 'utf8');
        const details = css.match(/\.network-panel \.network-details \{([^}]+)\}/)?.[1];
        assert.ok(details);
        assert.match(details, /padding: 6px 6px 6px 12px;/);
        assert.match(details, /box-shadow: inset 2px 0 0/);
    });

    const requestMethod = frontend.match(/        handleRequest\(request\) \{([\s\S]*?)\n        \}/);
    assert.ok(requestMethod, 'The real frontend request handler is available for isolated IPC tests');
    const handleRequest = vm.runInNewContext(`({ handleRequest(request) { ${requestMethod[1]} } }).handleRequest`, { parseRequest });

    // These doubles model the adapter boundary, not libnm. In particular, activation
    // stays pending until the fake native adapter reports ACTIVATED or rejects it.
    function deferred() {
        let resolve, reject;
        const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
        return { promise, resolve, reject };
    }

    function wifi(overrides = {}) {
        return {
            key: 'wifi', name: 'Cafe', ssid: 'Cafe', interface: 'wlan0', kind: 'wifi',
            wireless: true, connected: false, changing: false, known: false,
            strength: 50, security: 'WPA2 Personal', securityType: 'psk', profiles: [],
            ...overrides,
        };
    }

    function saved(overrides = {}) {
        return wifi({ known: true, profiles: [{ id: 'Cafe', uuid: 'profile-a' }], ...overrides });
    }

    function active(overrides = {}) { return saved({ connected: true, ...overrides }); }

    function wired(overrides = {}) {
        return saved({
            key: 'wired', name: 'Wired', ssid: '', interface: 'eth0', kind: 'ethernet',
            wireless: false, security: '', securityType: 'open',
            profiles: [{ id: 'Wired', uuid: 'wired-profile' }], ...overrides,
        });
    }

    const password = 'offline-only-fixture-value';
    const credentials = { password, authentication: 'WPA', hidden: false };
    const identityChanges = {
        interface: 'wlan1', ssid: 'Another network', profileUuid: 'profile-b',
        bssid: '02:00:00:00:00:02', activePath: '/active/replacement',
    };

    // A failing assertion must not print a credential-bearing object or value.
    function assertSecret(actual, expected) {
        assert.ok(actual === expected, 'the request-local credential matches without printing it');
    }

    function authenticationFailure() {
        return Object.assign(new Error('Native authentication failed.'), { authentication: true });
    }

    async function flush() { await Promise.resolve(); await Promise.resolve(); }

    class FakeBackend {
        constructor(rows) {
            this.rows = rows;
            this.available = true;
            this.wifiAvailable = true;
            this.wifiEnabled = true;
            this.wifiHardwareEnabled = true;
            this.opened = false;
            this.calls = {
                start: 0, dispose: 0, opened: [], toggleWifi: 0,
                activate: [], disconnect: [], forget: [], details: [], readSecret: [], readSavedSecret: [],
            };
            this.identities = new Map(rows.filter(row => row.connected).map(row => [row.key, {
                interface: row.interface, ssid: row.ssid, profileUuid: row.profiles[0]?.uuid || '',
                bssid: row.wireless ? '02:00:00:00:00:01' : '', activePath: `/active/${row.key}`,
            }]));
            this.detailValues = new Map(rows.map(row => [row.key, {
                identity: this.getIdentity(row.key), ssid: row.ssid, interface: row.interface,
                profileId: row.profiles[0]?.id || row.name, profileUuid: row.profiles[0]?.uuid || '',
                ...(row.connected ? { ipv4: ['192.0.2.10/24'] } : {}),
            }]));
        }

        start() { this.calls.start++; }
        dispose() { this.calls.dispose++; this.opened = false; }
        notify() { this.onChange?.(); }
        setOpened(opened) { this.calls.opened.push(opened); this.opened = opened; }
        toggleWifi() {
            this.calls.toggleWifi++;
            if (this.toggleError) throw this.toggleError;
            if (!this.wifiHardwareEnabled) throw new Error('Wi-Fi is blocked by the hardware switch.');
            this.wifiEnabled = !this.wifiEnabled;
            this.notify();
        }
        activate(key, options) {
            const request = { key, options, ...deferred() };
            this.calls.activate.push(request);
            return request.promise;
        }
        disconnect(key) { this.calls.disconnect.push(key); return this.disconnectResult; }
        forget(key) { this.calls.forget.push(key); return this.forgetResult; }
        getIdentity(key) { return this.identities.get(key) || null; }
        details(key) {
            this.calls.details.push(key);
            if (this.detailsError) throw this.detailsError;
            this.detailsHook?.(key);
            return this.detailValues.get(key) || {};
        }
        readSecret(key, identity, io) {
            // Cancellation deliberately does not settle promises. Tests can deliver
            // late native callbacks and verify that the controller distrusts them.
            const request = { key, identity, io, ...deferred() };
            this.calls.readSecret.push(request);
            return request.promise;
        }
        readSavedSecret(key, profileUuid, io) {
            const request = { key, profileUuid, io, ...deferred() };
            this.calls.readSavedSecret.push(request);
            return request.promise;
        }
    }

    function fixture(t, rows = [wifi()], { opened = true } = {}) {
        const backend = new FakeBackend(rows);
        const hooks = { changes: 0, cancellations: [] };
        const controller = new NetworkController({
            backend,
            onChange: () => hooks.changes++,
            createCancellable: () => {
                const io = {
                    cancelled: false, cancelCalls: 0,
                    cancel() { this.cancelled = true; this.cancelCalls++; },
                    is_cancelled() { return this.cancelled; },
                };
                hooks.cancellations.push(io);
                return io;
            },
        });
        t.after(() => controller.dispose());
        controller.start();
        if (opened) controller.toggle('DP-1');
        return { controller, backend, hooks };
    }

    function assertCleared(controller) {
        assert.equal(controller.page, 'list');
        assert.equal(controller.selectedKey, '');
        assert.equal(controller.selectedProfileUuid, '');
        assert.equal(controller.credentialKey, '');
        assert.equal(controller.forgetKey, '');
        assert.equal(controller.pendingKey, '');
        assert.deepEqual(controller.details, {});
        assert.equal(controller.secretLoading, false);
        assert.equal(controller.secretRequested, false);
        assertSecret(controller.secret, '');
    }

    function mutationCounts(backend) {
        return {
            opened: [...backend.calls.opened],
            toggleWifi: backend.calls.toggleWifi, activate: backend.calls.activate.length,
            disconnect: backend.calls.disconnect.length, forget: backend.calls.forget.length,
        };
    }

    it('dismisses only the error without resetting the current network interaction', t => {
        const { controller, backend, hooks } = fixture(t, [saved()]);
        controller.info('wifi');
        controller.error = 'Network operation failed';
        const selection = controller.selectedKey, page = controller.page;
        const before = mutationCounts(backend), changes = hooks.changes;
        controller.dismissError();
        assert.equal(controller.error, '');
        assert.equal(controller.selectedKey, selection);
        assert.equal(controller.page, page);
        assert.equal(hooks.changes, changes + 1);
        assert.deepEqual(mutationCounts(backend), before);
        controller.error = 'A later failure';
        backend.notify();
        assert.equal(controller.error, 'A later failure');
    });

    describe('lifecycle and scan visibility policy', () => {
        it('starts the backend, exposes native radio state, and sorts a copy of its rows', t => {
            const rows = [wifi({ key: 'z', name: 'Zulu' }), wired(), wifi({ key: 'a', name: 'Alpha' })];
            const { controller, backend } = fixture(t, rows, { opened: false });
            assert.equal(backend.calls.start, 1);
            assert.deepEqual(controller.rows.map(row => row.key), ['wired', 'a', 'z']);
            assert.deepEqual(backend.rows.map(row => row.key), ['z', 'wired', 'a']);
            assert.equal(controller.row('missing'), null);
            assert.equal(controller.available, true);
            assert.equal(controller.wifiAvailable, true);
            backend.available = backend.wifiAvailable = false;
            backend.wifiEnabled = backend.wifiHardwareEnabled = false;
            backend.notify();
            assert.equal(controller.available, false);
            assert.equal(controller.wifiAvailable, false);
            assert.equal(controller.wifiEnabled, false);
            assert.equal(controller.wifiHardwareEnabled, false);
        });

        it('forwards opened state for native scanning and toggles closed on the same monitor', t => {
            const { controller, backend } = fixture(t, undefined, { opened: false });
            assert.deepEqual(backend.calls.opened, []);
            controller.toggle('DP-1');
            assert.equal(controller.opened, true);
            assert.equal(controller.monitor, 'DP-1');
            assert.equal(backend.opened, true);
            controller.toggle('DP-1');
            assert.equal(controller.opened, false);
            assert.equal(backend.opened, false);
            assert.deepEqual(backend.calls.opened, [true, false]);
            assert.equal(typeof controller.refresh, 'undefined', 'scan scheduling has no public manual-refresh path');
        });

        it('moves an open panel to a new monitor without a false close transition', t => {
            const { controller, backend } = fixture(t);
            controller.connectNetwork('wifi');
            controller.toggle('HDMI-A-1');
            assert.equal(controller.opened, true);
            assert.equal(controller.monitor, 'HDMI-A-1');
            assertCleared(controller);
            assert.deepEqual(backend.calls.opened, [true, true]);
        });

        it('keeps native scanning enabled on inline Info until the panel closes', async t => {
            const { controller, backend } = fixture(t, [active()]);
            controller.info('wifi');
            assert.equal(controller.page, 'info');
            assert.equal(backend.opened, true);
            assert.deepEqual(backend.calls.opened, [true], 'Page navigation does not interrupt native scanning');
            controller.close();
            assert.equal(backend.opened, false);
            assert.deepEqual(backend.calls.opened, [true, false]);
        });
    });

    describe('connection and password policy', () => {
        it('selects a saved profile by UUID even when names collide and the list is reordered', async t => {
            const profiles = [{ id: 'Duplicate', uuid: 'uuid-b' }, { id: 'Duplicate', uuid: 'uuid-a' }];
            const { controller, backend, hooks } = fixture(t, [saved({ profiles })]);
            profiles.reverse();
            backend.notify();
            const work = controller.connectNetwork('wifi', 'uuid-b');
            const request = backend.calls.activate[0];
            assert.equal(request.key, 'wifi');
            assert.equal(request.options.profileUuid, 'uuid-b');
            assert.equal(request.options.cancellable, hooks.cancellations[0]);
            assert.equal(Object.hasOwn(request.options, 'password'), false);
            assert.equal(controller.credentialKey, '');
            request.resolve();
            await work;
        });

        it('leaves default saved-profile selection to the native backend without reading secrets', async t => {
            const { controller, backend } = fixture(t, [saved()]);
            const work = controller.connectNetwork('wifi');
            assert.equal(backend.calls.activate[0].options.profileUuid, '');
            assert.equal(backend.calls.readSecret.length, 0);
            backend.calls.activate[0].resolve();
            await work;
        });

        it('rejects a stale profile UUID rather than connecting a similarly named profile', async t => {
            const { controller, backend } = fixture(t, [saved()]);
            await controller.connectNetwork('wifi', 'deleted-uuid');
            assert.equal(backend.calls.activate.length, 0);
            assert.match(controller.error, /saved profile.*no longer available/i);
            assert.equal(controller.credentialKey, '');
        });

        for (const securityType of ['wep', 'psk', 'sae']) {
            it(`prompts for a new ${securityType.toUpperCase()} network and forwards a submitted password only to activation`, async t => {
                const { controller, backend } = fixture(t, [wifi({ securityType })]);
                await controller.connectNetwork('wifi');
                assert.equal(controller.credentialRow, controller.row('wifi'));
                assert.equal(controller.pendingKey, '');
                assert.equal(backend.calls.activate.length, 0);
                assert.equal(backend.calls.readSecret.length, 0);
                await controller.submitPassword('');
                assert.equal(backend.calls.activate.length, 0);
                const work = controller.submitPassword(password);
                const request = backend.calls.activate[0];
                assertSecret(request.options.password, password);
                assert.equal(Object.hasOwn(request.options, 'profileUuid'), false);
                assert.equal(controller.credentialKey, '');
                assertSecret(controller.secret, '');
                request.resolve();
                await work;
                assert.equal(controller.pendingKey, '');
                assert.equal(controller.error, '');
            });
        }

        for (const securityType of ['open', 'owe']) {
            it(`connects a new ${securityType.toUpperCase()} network natively without a password prompt`, async t => {
                const { controller, backend } = fixture(t, [wifi({ securityType })]);
                const work = controller.connectNetwork('wifi');
                assert.equal(backend.calls.activate.length, 1);
                assert.equal(Object.hasOwn(backend.calls.activate[0].options, 'password'), false);
                assert.equal(controller.credentialKey, '');
                assert.equal(backend.calls.readSecret.length, 0);
                backend.calls.activate[0].resolve();
                await work;
            });
        }

        for (const securityType of ['enterprise', 'unknown']) {
            it(`refuses inline setup for a new ${securityType} network instead of inventing credentials`, async t => {
                const { controller, backend } = fixture(t, [wifi({ securityType })]);
                await controller.connectNetwork('wifi');
                assert.notEqual(controller.error, '');
                assert.equal(controller.credentialKey, '');
                assert.equal(backend.calls.activate.length, 0);
            });
        }

        it('activates an existing enterprise profile natively without a password prompt', async t => {
            const { controller, backend } = fixture(t, [saved({ securityType: 'enterprise' })]);
            const work = controller.connectNetwork('wifi', 'profile-a');
            assert.equal(backend.calls.activate[0].options.profileUuid, 'profile-a');
            backend.calls.activate[0].reject(authenticationFailure());
            await work;
            assert.equal(controller.credentialKey, '');
            assert.equal(controller.error, 'Native authentication failed.');
        });

        it('offers a password retry for a single saved profile after native authentication rejection', async t => {
            const { controller, backend } = fixture(t, [saved()]);
            const first = controller.connectNetwork('wifi', 'profile-a');
            backend.calls.activate[0].reject(authenticationFailure());
            await first;
            assert.equal(controller.credentialKey, 'wifi');
            assert.match(controller.error, /password.*required|not accepted/i);
            const retry = controller.submitPassword(password);
            assert.equal(backend.calls.activate.length, 2);
            assertSecret(backend.calls.activate[1].options.password, password);
            assert.equal(backend.calls.readSecret.length, 0);
            backend.calls.activate[1].resolve();
            await retry;
            assert.equal(controller.error, '');
            assert.equal(controller.credentialKey, '');
        });

        it('refuses a generic password retry when multiple saved profiles could be modified', async t => {
            const profiles = [{ id: 'Duplicate', uuid: 'uuid-a' }, { id: 'Duplicate', uuid: 'uuid-b' }];
            const { controller, backend } = fixture(t, [saved({ profiles })]);
            const first = controller.connectNetwork('wifi', 'uuid-b');
            backend.calls.activate[0].reject(authenticationFailure());
            await first;
            assert.equal(controller.credentialKey, '');
            assert.notEqual(controller.error, '');
            await controller.submitPassword(password);
            assert.equal(backend.calls.activate.length, 1);
        });

        it('invalidates a password prompt when saved UUIDs change on a native notification', async t => {
            const { controller, backend } = fixture(t);
            await controller.connectNetwork('wifi');
            backend.rows[0].profiles = [{ id: 'New profile', uuid: 'profile-new' }];
            backend.rows[0].known = true;
            backend.notify();
            assert.equal(controller.credentialKey, '');
            assert.match(controller.error, /Saved profiles changed/);
            await controller.submitPassword(password);
            assert.equal(backend.calls.activate.length, 0);
        });

        it('checks the profile stamp again at submission even before a queued native notification', async t => {
            const { controller, backend } = fixture(t, [saved()]);
            const first = controller.connectNetwork('wifi');
            backend.calls.activate[0].reject(authenticationFailure());
            await first;
            backend.rows[0].profiles = [{ id: 'Same display name', uuid: 'replacement' }];
            await controller.submitPassword(password);
            assert.equal(backend.calls.activate.length, 1);
            assert.equal(controller.credentialKey, '');
            assert.notEqual(controller.error, '');
        });

        it('keeps a retry valid across harmless saved-profile renames', async t => {
            const { controller, backend } = fixture(t, [saved()]);
            const first = controller.connectNetwork('wifi');
            backend.calls.activate[0].reject(authenticationFailure());
            await first;
            backend.rows[0].profiles[0].id = 'Renamed';
            backend.notify();
            assert.equal(controller.credentialKey, 'wifi');
            const retry = controller.submitPassword(password);
            backend.calls.activate[1].resolve();
            await retry;
        });

        it('cancels a prompt without submitting or retaining a password', async t => {
            const { controller, backend } = fixture(t);
            await controller.connectNetwork('wifi');
            controller.cancelCredentials();
            await controller.submitPassword(password);
            assert.equal(controller.credentialRow, null);
            assert.equal(controller.error, '');
            assertSecret(controller.secret, '');
            assert.equal(backend.calls.activate.length, 0);
        });

        it('ignores missing, changing, and disposed connection targets', async t => {
            const { controller, backend } = fixture(t, [saved({ changing: true })]);
            await controller.connectNetwork('missing');
            await controller.connectNetwork('wifi');
            controller.dispose();
            await controller.connectNetwork('wifi');
            assert.equal(backend.calls.activate.length, 0);
        });
    });

    describe('native activation lifetime and stale callbacks', () => {
        it('keeps activation pending through state notifications until the backend settles', async t => {
            const { controller, backend } = fixture(t, [saved(), wired()]);
            const work = controller.connectNetwork('wifi');
            await flush();
            assert.equal(controller.pendingKey, 'wifi');
            backend.rows[0].changing = true;
            backend.notify();
            await controller.connectNetwork('wired');
            await controller.submitPassword(password);
            assert.equal(controller.pendingKey, 'wifi');
            assert.equal(backend.calls.activate.length, 1);
            backend.rows[0].changing = false;
            backend.calls.activate[0].resolve();
            await work;
            assert.equal(controller.pendingKey, '');
            assert.equal(controller.error, '');
        });

        for (const message of ['The connection failed.', 'The device disconnected.', 'The native activation timed out.']) {
            it(`clears pending and exposes a backend rejection: ${message}`, async t => {
                const { controller, backend } = fixture(t, [saved()]);
                const work = controller.connectNetwork('wifi');
                backend.calls.activate[0].reject(new Error(message));
                await work;
                assert.equal(controller.pendingKey, '');
                assert.equal(controller.credentialKey, '');
                assert.equal(controller.error, message);
            });
        }

        it('cancels activation when its native row disappears and ignores a late rejection', async t => {
            const { controller, backend } = fixture(t, [saved()]);
            const work = controller.connectNetwork('wifi');
            const request = backend.calls.activate[0];
            backend.rows = [];
            backend.notify();
            assert.equal(request.options.cancellable.cancelled, true);
            assert.equal(controller.pendingKey, '');
            assert.match(controller.error, /no longer available/);
            const currentError = controller.error;
            request.reject(authenticationFailure());
            await work;
            assert.equal(controller.error, currentError);
            assert.equal(controller.credentialKey, '');
        });

        for (const outcome of ['resolve', 'reject']) {
            it(`ignores activation ${outcome} after close`, async t => {
                const { controller, backend } = fixture(t, [saved()]);
                const work = controller.connectNetwork('wifi');
                const request = backend.calls.activate[0];
                controller.close();
                assert.equal(request.options.cancellable.cancelled, true);
                if (outcome === 'resolve') request.resolve();
                else request.reject(authenticationFailure());
                await work;
                assert.equal(controller.opened, false);
                assert.equal(controller.error, '');
                assertCleared(controller);
            });

            it(`does not let an older activation ${outcome} clear a newer pending request`, async t => {
                const { controller, backend } = fixture(t, [saved()]);
                const oldWork = controller.connectNetwork('wifi');
                const old = backend.calls.activate[0];
                controller.close();
                controller.toggle('DP-2');
                const newWork = controller.connectNetwork('wifi');
                const current = backend.calls.activate[1];
                if (outcome === 'resolve') old.resolve();
                else old.reject(authenticationFailure());
                await oldWork;
                assert.equal(controller.pendingKey, 'wifi');
                assert.equal(current.options.cancellable.cancelled, false);
                assert.equal(controller.credentialKey, '');
                assert.equal(controller.error, '');
                current.reject(new Error('Current activation failed.'));
                await newWork;
                assert.equal(controller.pendingKey, '');
                assert.equal(controller.error, 'Current activation failed.');
            });
        }

        it('does not let a late successful activation erase a newer failure', async t => {
            const { controller, backend } = fixture(t, [saved()]);
            const first = controller.connectNetwork('wifi');
            controller.back();
            const second = controller.connectNetwork('wifi');
            backend.calls.activate[1].reject(new Error('New request failed.'));
            await second;
            backend.calls.activate[0].resolve();
            await first;
            assert.equal(controller.error, 'New request failed.');
        });
    });

    describe('forget and disconnect delegation', () => {
        it('requires confirmation, then forwards one forget-all request without choosing a UUID', async t => {
            const profiles = [{ id: 'First', uuid: 'uuid-a' }, { id: 'Second', uuid: 'uuid-b' }];
            const { controller, backend } = fixture(t, [saved({ profiles })]);
            controller.requestForget('wifi');
            assert.equal(controller.forgetKey, 'wifi');
            assert.deepEqual(backend.calls.forget, []);
            await controller.confirmForget();
            assert.equal(controller.forgetKey, '');
            assert.deepEqual(backend.calls.forget, ['wifi']);
            assert.deepEqual(profiles.map(profile => profile.uuid), ['uuid-a', 'uuid-b']);
            await controller.confirmForget();
            assert.deepEqual(backend.calls.forget, ['wifi']);
        });

        it('does nothing for cancelled, unknown, unsaved, or disappeared forget targets', async t => {
            const { controller, backend } = fixture(t, [saved(), wifi({ key: 'new' })]);
            controller.requestForget('missing');
            controller.requestForget('new');
            assert.equal(controller.forgetKey, '');
            controller.requestForget('wifi');
            controller.cancelForget();
            await controller.confirmForget();
            controller.requestForget('wifi');
            backend.rows = [];
            backend.notify();
            assert.equal(controller.forgetKey, '');
            await controller.confirmForget();
            assert.deepEqual(backend.calls.forget, []);
        });

        it('rechecks that the confirmed row is still saved', async t => {
            const { controller, backend } = fixture(t, [saved()]);
            controller.requestForget('wifi');
            backend.rows[0].known = false;
            await controller.confirmForget();
            assert.deepEqual(backend.calls.forget, []);
        });

        it('reports native forget failures and ignores them after a newer page request', async t => {
            const { controller, backend } = fixture(t, [saved()]);
            const first = deferred();
            backend.forgetResult = first.promise;
            controller.requestForget('wifi');
            const work = controller.confirmForget();
            first.reject(new Error('Native profile deletion failed.'));
            await work;
            assert.equal(controller.error, 'Native profile deletion failed.');
            const second = deferred();
            backend.forgetResult = second.promise;
            controller.requestForget('wifi');
            const stale = controller.confirmForget();
            controller.back();
            second.reject(new Error('Old profile deletion failed.'));
            await stale;
            assert.equal(controller.error, '');
        });

        it('delegates disconnect by row key and ignores missing/changing rows', async t => {
            const { controller, backend } = fixture(t, [active(), wired({ changing: true })]);
            await controller.disconnectNetwork('missing');
            await controller.disconnectNetwork('wired');
            await controller.disconnectNetwork('wifi');
            assert.deepEqual(backend.calls.disconnect, ['wifi']);
            assert.equal(backend.calls.readSecret.length, 0);
        });

        it('reports native disconnect failures', async t => {
            const { controller, backend } = fixture(t, [active()]);
            const disconnect = deferred();
            backend.disconnectResult = disconnect.promise;
            const work = controller.disconnectNetwork('wifi');
            disconnect.reject(new Error('Native disconnect failed.'));
            await work;
            assert.equal(controller.error, 'Native disconnect failed.');
        });
    });

    describe('radio changes and hardware blocking', () => {
        for (const field of ['wifiEnabled', 'wifiHardwareEnabled']) {
            it(`resets a password prompt when ${field} changes`, async t => {
                const { controller, backend } = fixture(t);
                await controller.connectNetwork('wifi');
                backend[field] = false;
                backend.notify();
                assertCleared(controller);
                await controller.submitPassword(password);
                await controller.connectNetwork('wifi');
                assert.equal(backend.calls.activate.length, 0);
            });

            it(`cancels pending activation when ${field} changes`, async t => {
                const { controller, backend } = fixture(t, [saved()]);
                const work = controller.connectNetwork('wifi');
                const request = backend.calls.activate[0];
                backend[field] = false;
                backend.notify();
                assert.equal(request.options.cancellable.cancelled, true);
                assertCleared(controller);
                request.reject(authenticationFailure());
                await work;
                assert.equal(controller.error, '');
                assertCleared(controller);
            });

            it(`cancels sensitive reads and clears details when ${field} changes`, async t => {
                const { controller, backend } = fixture(t, [active()]);
                controller.info('wifi');
                const work = controller.revealSecret();
                const request = backend.calls.readSecret[0];
                backend[field] = false;
                backend.notify();
                assert.equal(request.io.cancelled, true);
                assertCleared(controller);
                request.resolve(credentials);
                await work;
                assertCleared(controller);
            });
        }

        it('delegates radio toggling and leaves hardware-block enforcement to the native adapter', t => {
            const { controller, backend } = fixture(t);
            controller.toggleWifi();
            assert.equal(backend.calls.toggleWifi, 1);
            assert.equal(controller.wifiEnabled, false);
            assertCleared(controller);
            backend.wifiHardwareEnabled = false;
            backend.notify();
            controller.toggleWifi();
            assert.equal(backend.calls.toggleWifi, 2);
            assert.equal(controller.wifiEnabled, false);
            assert.match(controller.error, /hardware switch/);
        });

        it('does not block Ethernet activation when the wireless radios are unavailable', async t => {
            const { controller, backend } = fixture(t, [wired()]);
            backend.wifiEnabled = backend.wifiHardwareEnabled = false;
            backend.notify();
            const work = controller.connectNetwork('wired', 'wired-profile');
            assert.equal(backend.calls.activate[0].key, 'wired');
            backend.calls.activate[0].resolve();
            await work;
        });
    });

    describe('toggleWifi IPC policy', () => {
        for (const opened of [false, true]) {
            for (const failure of [false, true]) {
                it(`${failure ? 'reports a native radio failure' : 'delegates the native radio toggle'} while ${opened ? 'open' : 'closed'} without changing visibility`, t => {
                    const { controller, backend } = fixture(t, undefined, { opened });
                    const visibility = { opened: controller.opened, monitor: controller.monitor, backendOpened: backend.opened };
                    const openedCalls = [...backend.calls.opened];
                    const panel = {
                        toggle() { assert.fail('Radio IPC must not open or move the panel'); },
                        close() { assert.fail('Radio IPC must not close the panel'); },
                    };
                    if (failure) backend.toggleError = new Error('Native radio permission denied.');
                    const request = () => handleRequest.call({ controller, panel }, '{"command":"toggleWifi"}');
                    if (failure) assert.throws(request, error => error.message === 'Native radio permission denied.');
                    else assert.equal(request(), 'ok');
                    assert.equal(backend.calls.toggleWifi, 1);
                    assert.equal(controller.wifiEnabled, failure);
                    assert.deepEqual({ opened: controller.opened, monitor: controller.monitor, backendOpened: backend.opened }, visibility);
                    assert.deepEqual(backend.calls.opened, openedCalls, 'Radio IPC does not change the scan visibility policy');
                    assert.equal(backend.calls.activate.length, 0);
                    assert.equal(backend.calls.disconnect.length, 0);
                    assert.equal(backend.calls.forget.length, 0);
                    if (failure) {
                        backend.toggleError = null;
                        assert.equal(request(), 'ok', 'A successful retry clears the prior IPC error');
                        assert.equal(controller.error, '');
                        assert.equal(backend.calls.toggleWifi, 2);
                        assert.deepEqual({ opened: controller.opened, monitor: controller.monitor, backendOpened: backend.opened }, visibility);
                        assert.deepEqual(backend.calls.opened, openedCalls);
                    }
                });
            }
        }

        it('rejects monitor and mutation arguments before reaching the native radio adapter', t => {
            const { controller, backend } = fixture(t, undefined, { opened: false });
            for (const extra of [{ monitor: '' }, { monitor: 'DP-1' }, { enabled: true }, { password: 'not-a-radio-argument' }]) {
                assert.throws(() => handleRequest.call({ controller, panel: {} }, JSON.stringify({ command: 'toggleWifi', ...extra })),
                    /Invalid network panel request/);
            }
            assert.equal(backend.calls.toggleWifi, 0);
            assert.deepEqual(backend.calls.opened, []);
        });
    });

    describe('saved disconnected information', () => {
        for (const [label, selected] of [
            ['undetected Wi-Fi', saved({ strength: null })],
            ['detected Wi-Fi', saved({ strength: 64 })],
            ['genuine zero Wi-Fi', saved({ strength: 0 })],
            ['Ethernet', wired()],
        ]) {
            it(`opens ${label} profile metadata without credentials or a live identity`, async t => {
                const { controller, backend, hooks } = fixture(t, [selected]);
                controller.info(selected.key);
                assert.equal(controller.page, 'info');
                assert.equal(controller.selectedRow, selected);
                assert.equal(controller.details.identity, null);
                assert.equal(controller.details.ssid, selected.ssid);
                assert.equal(controller.details.interface, selected.interface);
                assert.equal(controller.details.profileId, selected.profiles[0].id);
                assert.equal(controller.details.profileUuid, selected.profiles[0].uuid);
                assert.deepEqual(backend.calls.details, [selected.key]);
                assert.equal(controller.selectedProfileUuid, selected.profiles[0].uuid);
                assert.equal(controller.secretRequested, false);
                assert.equal(controller.secretLoading, false);
                assertSecret(controller.secret, '');
                assert.equal(controller.credentialKey, '');
                assert.equal(backend.calls.readSecret.length, 0);
                assert.equal(backend.calls.readSavedSecret.length, 0);
                assert.equal(hooks.cancellations.length, 0);
                assert.equal(backend.calls.activate.length, 0);
                assert.equal(controller.opened, true);
                assert.equal(backend.opened, true, 'Offline Info keeps native scanning enabled');
                controller.back();
                assertCleared(controller);
                assert.deepEqual(backend.calls.opened, [true], 'Back does not close scanning');
            });
        }

        it('keeps offline Info across signal appearance/disappearance and cosmetic profile updates', t => {
            const profiles = [{ id: 'Same name', uuid: 'profile-a' }, { id: 'Same name', uuid: 'profile-b' }];
            const { controller, backend } = fixture(t, [saved({ strength: null, profiles })]);
            controller.info('wifi');
            assert.equal(controller.page, 'info');
            const details = controller.details;
            for (const strength of [64, null, 0, undefined, 35]) {
                backend.rows = backend.rows.map(row => ({ ...row, strength,
                    profiles: profiles.slice().reverse().map(profile => ({ ...profile, id: 'Renamed' })) }));
                backend.notify();
                assert.equal(controller.page, 'info', `Signal ${String(strength)} does not invalidate offline Info`);
                assert.equal(controller.selectedRow, backend.rows[0]);
                assert.equal(controller.details, details);
                assert.equal(controller.secretRequested, false);
            }
            assert.deepEqual(backend.calls.details, ['wifi']);
            assert.equal(backend.calls.readSecret.length, 0);
            assert.equal(backend.calls.readSavedSecret.length, 0);
        });

        for (const change of ['missing row', 'missing profile', 'replaced UUID', 'connected', 'changing']) {
            it(`clears offline Info on ${change} without requesting a secret`, async t => {
                const { controller, backend } = fixture(t, [saved({ strength: null })]);
                controller.info('wifi');
                assert.equal(controller.page, 'info');
                if (change === 'missing row') backend.rows = [];
                else backend.rows = backend.rows.map(row => ({ ...row,
                    ...(change === 'missing profile' ? { known: false, profiles: [] } : {}),
                    ...(change === 'replaced UUID' ? { profiles: [{ id: 'Cafe', uuid: 'replacement' }] } : {}),
                    ...(change === 'connected' ? { connected: true } : {}),
                    ...(change === 'changing' ? { changing: true } : {}),
                }));
                backend.notify();
                assertCleared(controller);
                await controller.revealSecret();
                assert.equal(backend.calls.readSecret.length, 0);
                assert.equal(backend.calls.readSavedSecret.length, 0);
                assert.equal(backend.opened, true);
            });
        }

        for (const change of ['profile UUID', 'connection state']) {
            it(`rejects an offline ${change} race during the details snapshot`, t => {
                const { controller, backend } = fixture(t, [saved({ strength: null })]);
                backend.detailsHook = () => {
                    backend.rows = backend.rows.map(row => ({ ...row, ...(change === 'profile UUID'
                        ? { profiles: [{ id: 'Cafe', uuid: 'replacement' }] } : { connected: true }) }));
                };
                controller.info('wifi');
                assertCleared(controller);
                assert.equal(backend.calls.readSecret.length, 0);
                assert.equal(backend.calls.readSavedSecret.length, 0);
            });
        }
    });

    describe('explicit saved-profile password reads', () => {
        const profiles = [{ id: 'Duplicate', uuid: 'profile-a' }, { id: 'Duplicate', uuid: 'profile-b' }];
        const changes = {
            'row removal': backend => { backend.rows = []; },
            'profile removal': backend => { backend.rows[0] = saved({ profiles: profiles.slice(1) }); },
            'UUID replacement': backend => { backend.rows[0] = saved({ profiles: [{ id: 'Duplicate', uuid: 'replacement' }] }); },
            'native profile replacement': backend => { backend.rows[0] = { ...backend.rows[0], savedIdentity: 'replacement-object-stamp' }; },
            'interface change': backend => { backend.rows[0] = { ...backend.rows[0], interface: 'wlan1' }; },
            'connection': backend => { backend.rows[0] = { ...backend.rows[0], connected: true }; },
            'activation': backend => { backend.rows[0] = { ...backend.rows[0], changing: true }; },
            'active identity': backend => { backend.identities.set('wifi', { ...identityChanges }); },
            'software radio change': backend => { backend.wifiEnabled = false; },
            'hardware radio change': backend => { backend.wifiHardwareEnabled = false; },
        };

        for (const securityType of ['wep', 'psk', 'sae', 'open']) {
            it(`reads a saved ${securityType} password only on explicit reveal, by exact UUID and without activation`, async t => {
                const { controller, backend, hooks } = fixture(t, [saved({ profiles, securityType })]);
                controller.info('wifi');
                controller.selectSavedProfile('profile-b');
                backend.rows[0] = { ...backend.rows[0], profiles: profiles.slice().reverse() };
                backend.notify();
                controller.status();
                assert.equal(controller.page, 'info');
                assert.equal(controller.selectedProfileUuid, 'profile-b');
                assert.equal(controller.details.profileUuid, 'profile-b');
                assert.equal(backend.calls.readSavedSecret.length, 0, 'Info, selection, notification, status never read secrets');
                assert.equal(hooks.cancellations.length, 0);
                const work = controller.revealSecret();
                await controller.revealSecret();
                assert.equal(backend.calls.readSavedSecret.length, 1, 'Duplicate reveal is ignored while loading');
                const request = backend.calls.readSavedSecret[0];
                assert.equal(request.key, 'wifi');
                assert.equal(request.profileUuid, 'profile-b');
                assert.equal(request.io, hooks.cancellations[0]);
                assert.equal(controller.secretRequested, true);
                assert.equal(controller.secretLoading, true);
                assertSecret(controller.secret, '');
                controller.selectSavedProfile('profile-b');
                assert.equal(request.io.cancelled, false, 'An unchanged UUID does not disturb an explicit read');
                assert.equal(controller.secretLoading, true);
                request.resolve(securityType === 'open' ? { password: '', authentication: 'nopass', hidden: false } : credentials);
                await work;
                assertSecret(controller.secret, securityType === 'open' ? 'No password (open network)' : password);
                assert.equal(controller.secretLoading, false);
                assert.equal(backend.calls.readSecret.length, 0, 'Offline reads never borrow the active identity API');
                assert.equal(backend.calls.activate.length, 0);
                assert.deepEqual(backend.calls.disconnect, []);
                assert.deepEqual(backend.calls.forget, []);
                assert.ok(!JSON.stringify([controller.rows, controller.details, controller.status()]).includes(password),
                    'Rows, safe details and public status never contain credentials');
                controller.hideSecret();
                assertSecret(controller.secret, '');
                assert.equal(controller.secretRequested, false);
            });
        }

        for (const outcome of ['resolve', 'reject']) {
            it(`switching UUID cancels a pending saved read and isolates its late ${outcome} from the new read`, async t => {
                const { controller, backend } = fixture(t, [saved({ profiles })]);
                controller.info('wifi');
                const first = controller.revealSecret(), old = backend.calls.readSavedSecret[0];
                controller.selectSavedProfile('profile-b');
                assert.equal(old.io.cancelled, true);
                assert.equal(controller.secretRequested, false);
                assert.equal(controller.secretLoading, false);
                assertSecret(controller.secret, '');
                assert.equal(controller.details.profileId, 'Duplicate');
                assert.equal(controller.details.profileUuid, 'profile-b');
                assert.equal(controller.details.identity, null);
                assert.equal(backend.calls.readSavedSecret.length, 1, 'Selection itself never authorizes a read');
                const second = controller.revealSecret(), current = backend.calls.readSavedSecret[1];
                if (outcome === 'resolve') old.resolve(credentials);
                else old.reject(new Error('Obsolete saved-profile authorization error.'));
                await first;
                assert.equal(controller.secretLoading, true, 'Old completion cannot stop the new loading indicator');
                assert.equal(controller.secretRequested, true, 'An obsolete failure cannot reset the newer Reveal request');
                assert.equal(current.io.cancelled, false);
                assert.equal(current.profileUuid, 'profile-b');
                assertSecret(controller.secret, '');
                assert.equal(controller.error, '');
                current.resolve(credentials); await second;
                assertSecret(controller.secret, password);
                controller.selectSavedProfile('profile-a');
                assert.equal(controller.selectedProfileUuid, 'profile-a');
                assert.equal(controller.secretRequested, false);
                assertSecret(controller.secret, '');
                assert.equal(backend.calls.readSavedSecret.length, 2, 'Switching a displayed password clears it without another read');
                assert.equal(backend.calls.activate.length, 0);
            });
        }

        it('ignores invalid, unchanged, connected, changing and closed profile choices', async t => {
            const { controller, backend } = fixture(t, [saved({ profiles }), active({ key: 'active' })]);
            controller.selectSavedProfile('profile-b');
            controller.info('wifi');
            const details = controller.details;
            for (const uuid of ['', null, 7, 'missing', 'profile-a']) controller.selectSavedProfile(uuid);
            assert.equal(controller.selectedProfileUuid, 'profile-a');
            assert.equal(controller.details, details);
            backend.rows[0] = { ...backend.rows[0], changing: true };
            controller.selectSavedProfile('profile-b');
            assertCleared(controller);
            backend.notify();
            controller.info('active');
            controller.selectSavedProfile('profile-b');
            assert.equal(controller.selectedProfileUuid, '');
            assert.equal(controller.details.profileUuid, 'profile-a');
            controller.close(); controller.selectSavedProfile('profile-b');
            assertCleared(controller);
            assert.equal(backend.calls.readSavedSecret.length, 0);
            assert.equal(backend.calls.readSecret.length, 0);
            assert.equal(backend.calls.activate.length, 0);
        });

        it('preserves a pending saved read across detection, profile order and display-name changes', async t => {
            const { controller, backend } = fixture(t, [saved({ profiles, strength: null })]);
            controller.info('wifi'); controller.selectSavedProfile('profile-b');
            const work = controller.revealSecret(), request = backend.calls.readSavedSecret[0];
            for (const strength of [0, 64, null]) {
                backend.rows[0] = { ...backend.rows[0], strength,
                    profiles: profiles.slice().reverse().map(profile => ({ ...profile, id: 'Renamed' })) };
                backend.notify();
                assert.equal(controller.selectedProfileUuid, 'profile-b');
                assert.equal(controller.secretLoading, true);
                assert.equal(request.io.cancelled, false);
            }
            request.resolve(credentials); await work;
            assertSecret(controller.secret, password);
            assert.equal(backend.calls.readSavedSecret.length, 1);
        });

        for (const [change, apply] of Object.entries(changes)) {
            for (const notified of [false, true]) {
                for (const outcome of ['resolve', 'reject']) {
                    it(`discards saved read ${outcome} after ${change}${notified ? ' with notification' : ' before notification'}`, async t => {
                        const { controller, backend } = fixture(t, [saved({ profiles })]);
                        controller.info('wifi');
                        const work = controller.revealSecret(), request = backend.calls.readSavedSecret[0];
                        apply(backend);
                        if (notified) {
                            backend.notify();
                            assert.equal(request.io.cancelled, true);
                            assertCleared(controller);
                        }
                        if (outcome === 'resolve') request.resolve(credentials);
                        else request.reject(new Error('Obsolete saved-profile authorization error.'));
                        await work;
                        assertCleared(controller);
                        assert.ok(!controller.error.includes('Obsolete'), 'A stale error cannot replace current context feedback');
                        assert.equal(backend.calls.readSavedSecret.length, 1, 'Context changes never restart authorization');
                    });
                }
            }
            it(`clears a displayed saved password on notified ${change}`, async t => {
                const { controller, backend } = fixture(t, [saved({ profiles })]);
                controller.info('wifi');
                const work = controller.revealSecret();
                backend.calls.readSavedSecret[0].resolve(credentials); await work;
                assertSecret(controller.secret, password);
                apply(backend); backend.notify();
                assertCleared(controller);
                assert.equal(backend.calls.readSavedSecret.length, 1);
            });
        }

        for (const destination of ['hide', 'tab/back', 'close', 'other Info', 'dispose']) {
            for (const outcome of ['resolve', 'reject']) {
                it(`${destination} cancels a saved read and ignores its late ${outcome}`, async t => {
                    const { controller, backend } = fixture(t, [saved({ profiles }), saved({ key: 'other' })]);
                    controller.info('wifi');
                    const work = controller.revealSecret(), request = backend.calls.readSavedSecret[0];
                    if (destination === 'hide') controller.hideSecret();
                    else if (destination === 'tab/back') controller.back();
                    else if (destination === 'other Info') controller.info('other');
                    else controller[destination]();
                    assert.equal(request.io.cancelled, true);
                    assert.equal(controller.secretRequested, false);
                    assert.equal(controller.secretLoading, false);
                    if (outcome === 'resolve') request.resolve(credentials);
                    else request.reject(new Error('Obsolete saved-profile authorization error.'));
                    await work;
                    assertSecret(controller.secret, '');
                    assert.equal(controller.error, '');
                    assert.equal(controller.selectedKey, destination === 'hide' ? 'wifi' : destination === 'other Info' ? 'other' : '');
                    assert.equal(backend.calls.readSavedSecret.length, 1);
                });
            }
        }

        it('reports saved authorization failure without retaining credentials, then allows an explicit retry', async t => {
            const { controller, backend, hooks } = fixture(t, [saved()]);
            controller.info('wifi');
            const work = controller.revealSecret();
            backend.calls.readSavedSecret[0].reject(new Error('Saved password authorization denied.'));
            await work;
            assert.equal(controller.error, 'Saved password authorization denied.');
            assert.equal(controller.secretLoading, false);
            assert.equal(controller.secretRequested, false, 'Accepted failure restores Reveal rather than leaving Hide selected');
            assertSecret(controller.secret, '');
            assert.equal(controller.page, 'info');
            assert.equal(controller.selectedProfileUuid, 'profile-a');
            assert.equal(controller.details.profileUuid, 'profile-a');
            assert.equal(backend.calls.readSavedSecret.length, 1, 'Failure never automatically retries authorization');
            const retry = controller.revealSecret();
            assert.equal(backend.calls.readSavedSecret.length, 2, 'The next Reveal click retries directly, without Hide or reopening Info');
            assert.equal(backend.calls.readSavedSecret[1].profileUuid, 'profile-a');
            assert.equal(controller.secretRequested, true);
            assert.equal(controller.secretLoading, true);
            assert.equal(controller.error, '');
            backend.calls.readSavedSecret[1].resolve(credentials); await retry;
            assertSecret(controller.secret, password);
            assert.equal(controller.secretRequested, true);
            assert.equal(controller.error, '');
        });
    });

    describe('details and on-demand sensitive reads', () => {
        it('opens details without authorizing a secret read and pins all native identity fields on reveal', async t => {
            const { controller, backend, hooks } = fixture(t, [active()]);
            controller.info('wifi');
            assert.equal(controller.page, 'info');
            assert.equal(controller.selectedRow, controller.row('wifi'));
            assert.equal(controller.details.profileId, 'Cafe');
            assert.equal(Object.hasOwn(controller, 'detailsLoading'), false);
            assert.deepEqual(backend.calls.details, ['wifi']);
            assert.equal(backend.calls.readSecret.length, 0);
            assert.equal(backend.calls.readSavedSecret.length, 0);
            assert.equal(controller.selectedProfileUuid, '');
            assert.equal(controller.secretRequested, false);
            assertSecret(controller.secret, '');
            const work = controller.revealSecret();
            const request = backend.calls.readSecret[0];
            assert.equal(request.key, 'wifi');
            assert.deepEqual(request.identity, {
                interface: 'wlan0', ssid: 'Cafe', profileUuid: 'profile-a',
                bssid: '02:00:00:00:00:01', activePath: '/active/wifi',
            });
            assert.notEqual(request.identity, backend.getIdentity('wifi'));
            assert.equal(request.io, hooks.cancellations[0]);
            assert.equal(controller.secretRequested, true);
            assert.equal(controller.secretLoading, true);
            assertSecret(controller.secret, '');
            request.resolve(credentials);
            await work;
            assertSecret(controller.secret, password);
            assert.equal(controller.secretLoading, false);
            assert.equal(backend.calls.readSavedSecret.length, 0, 'Connected reveal retains the active-identity API');
            controller.hideSecret();
            assertSecret(controller.secret, '');
            assert.equal(controller.secretRequested, false);
        });

        it('ignores duplicate reveal clicks while authorization is pending', async t => {
            const { controller, backend } = fixture(t, [active()]);
            controller.info('wifi');
            const work = controller.revealSecret();
            await controller.revealSecret();
            assert.equal(backend.calls.readSecret.length, 1);
            backend.calls.readSecret[0].resolve(credentials);
            await work;
        });

        it('does not reveal secrets on the list, for unsaved disconnected rows, or for Ethernet', async t => {
            const { controller, backend } = fixture(t, [wifi(), wired({ connected: true })]);
            await controller.revealSecret();
            controller.info('wifi');
            assert.equal(controller.page, 'list');
            controller.info('missing');
            assert.equal(controller.page, 'list');
            controller.info('wired');
            assert.equal(controller.page, 'info');
            await controller.revealSecret();
            assert.equal(backend.calls.readSecret.length, 0);
        });

        it('reports a details failure without requesting credentials', t => {
            const { controller, backend } = fixture(t, [active()]);
            backend.detailsError = new Error('Native details unavailable.');
            controller.info('wifi');
            assert.equal(controller.page, 'list');
            assert.equal(controller.selectedKey, '');
            assert.equal(controller.error, 'Native details unavailable.');
            assert.equal(backend.calls.readSecret.length, 0);
        });

        it('rejects a native identity transition during the synchronous details snapshot', t => {
            const { controller, backend } = fixture(t, [active()]);
            backend.detailsHook = key => {
                backend.identities.set(key, { ...backend.getIdentity(key), activePath: '/active/replaced' });
            };
            controller.info('wifi');
            assert.equal(controller.page, 'list');
            assert.equal(controller.selectedKey, '');
            assert.match(controller.error, /connection changed/i);
            assert.equal(backend.calls.readSecret.length, 0);
        });

        it('reports secret authorization failure without retaining data', async t => {
            const { controller, backend, hooks } = fixture(t, [active()]);
            controller.info('wifi');
            const work = controller.revealSecret();
            backend.calls.readSecret[0].reject(new Error('Secret authorization denied.'));
            await work;
            assert.equal(controller.error, 'Secret authorization denied.');
            assert.equal(controller.secretLoading, false);
            assert.equal(controller.secretRequested, false, 'Connected failure also restores Reveal');
            assertSecret(controller.secret, '');
            assert.equal(controller.page, 'info');
            const retry = controller.revealSecret();
            assert.equal(backend.calls.readSecret.length, 2);
            assert.equal(controller.secretRequested, true);
            assert.equal(controller.error, '');
            backend.calls.readSecret[1].resolve(credentials); await retry;
            assertSecret(controller.secret, password);
        });

        it('describes an open network without manufacturing a password', async t => {
            const { controller, backend } = fixture(t, [active({ securityType: 'open' })]);
            controller.info('wifi');
            const work = controller.revealSecret();
            backend.calls.readSecret[0].resolve({ password: '', authentication: 'nopass', hidden: false });
            await work;
            assertSecret(controller.secret, 'No password (open network)');
        });

        for (const outcome of ['resolve', 'reject']) {
            it(`hiding a pending secret cancels authorization and discards its late ${outcome}`, async t => {
                const { controller, backend } = fixture(t, [active()]);
                controller.info('wifi');
                const work = controller.revealSecret();
                const request = backend.calls.readSecret[0];
                controller.hideSecret();
                assert.equal(request.io.cancelled, true);
                assert.equal(controller.secretLoading, false);
                assert.equal(controller.secretRequested, false);
                if (outcome === 'resolve') request.resolve(credentials);
                else request.reject(new Error('Obsolete secret error.'));
                await work;
                assertSecret(controller.secret, '');
                assert.equal(controller.error, '');
                assert.equal(controller.page, 'info');
            });
        }

        it('a late hidden read cannot clear a newer reveal or its loading state', async t => {
            const { controller, backend } = fixture(t, [active()]);
            controller.info('wifi');
            const first = controller.revealSecret();
            controller.hideSecret();
            const second = controller.revealSecret();
            backend.calls.readSecret[0].resolve(credentials);
            await first;
            assert.equal(controller.secretLoading, true);
            assert.equal(backend.calls.readSecret[1].io.cancelled, false);
            assertSecret(controller.secret, '');
            backend.calls.readSecret[1].resolve(credentials);
            await second;
            assertSecret(controller.secret, password);
        });

        for (const [field, value] of Object.entries(identityChanges)) {
            it(`discards a secret after ${field} changes, even before the native change notification`, async t => {
                const { controller, backend } = fixture(t, [active()]);
                controller.info('wifi');
                const work = controller.revealSecret();
                const request = backend.calls.readSecret[0];
                const original = request.identity[field];
                // Mutate the backend's identity object to also verify snapshot copying.
                backend.getIdentity('wifi')[field] = value;
                assert.equal(request.identity[field], original);
                request.resolve(credentials);
                await work;
                assertCleared(controller);
                assert.match(controller.error, /connection changed/i);
            });
        }

        it('clears an already displayed secret immediately on a roam notification', async t => {
            const { controller, backend } = fixture(t, [active()]);
            controller.info('wifi');
            const work = controller.revealSecret();
            backend.calls.readSecret[0].resolve(credentials);
            await work;
            assertSecret(controller.secret, password);
            backend.identities.set('wifi', { ...backend.getIdentity('wifi'), bssid: identityChanges.bssid });
            backend.notify();
            assertCleared(controller);
            assert.match(controller.error, /connection changed/i);
        });

        it('cancels a pending read on a notified roam and ignores its stale error', async t => {
            const { controller, backend } = fixture(t, [active()]);
            controller.info('wifi');
            const work = controller.revealSecret();
            backend.identities.delete('wifi');
            backend.notify();
            assert.equal(backend.calls.readSecret[0].io.cancelled, true);
            const error = controller.error;
            backend.calls.readSecret[0].reject(new Error('Obsolete authorization error.'));
            await work;
            assert.equal(controller.error, error);
            assertCleared(controller);
        });

        for (const destination of ['back', 'close', 'other details']) {
            it(`discards a pending secret after navigating to ${destination}`, async t => {
                const { controller, backend } = fixture(t, [active(), active({ key: 'other', name: 'Other', ssid: 'Other' })]);
                controller.info('wifi');
                const work = controller.revealSecret();
                if (destination === 'other details') controller.info('other');
                else controller[destination]();
                assert.equal(backend.calls.readSecret[0].io.cancelled, true);
                backend.calls.readSecret[0].resolve(credentials);
                await work;
                assertSecret(controller.secret, '');
                assert.equal(controller.error, '');
                assert.equal(controller.selectedKey, destination === 'other details' ? 'other' : '');
            });
        }
    });

    describe('read-only status and disposal', () => {
        for (const opened of [false, true]) {
            it(`returns Waybar JSON without network mutations or UI hooks (opened=${opened})`, t => {
                const { controller, backend, hooks } = fixture(t, [active(), wired({ connected: true })], { opened });
                const before = mutationCounts(backend);
                const changes = hooks.changes;
                const status = JSON.parse(controller.status());
                assert.equal(status.class, 'ethernet');
                assert.match(status.tooltip, /Ethernet: eth0/);
                assert.deepEqual(backend.calls.details, ['wired']);
                assert.deepEqual(mutationCounts(backend), before);
                assert.equal(backend.calls.readSecret.length, 0);
                assert.equal(hooks.cancellations.length, 0);
                assert.equal(hooks.changes, changes, 'polling must not cause panel render/dismiss hooks');
                assert.equal(controller.opened, opened);
            });
        }

        it('does not navigate, cancel, or expose a pending sensitive request during status polling', async t => {
            const { controller, backend, hooks } = fixture(t, [active()]);
            controller.info('wifi');
            const work = controller.revealSecret();
            const before = mutationCounts(backend);
            const changes = hooks.changes;
            const status = controller.status();
            assert.equal(JSON.parse(status).class, 'wifi');
            assert.ok(!status.includes(password), 'status never contains credentials');
            assert.equal(controller.page, 'info');
            assert.equal(controller.selectedKey, 'wifi');
            assert.equal(controller.secretLoading, true);
            assert.equal(backend.calls.readSecret.length, 1);
            assert.equal(backend.calls.readSecret[0].io.cancelled, false);
            assert.deepEqual(mutationCounts(backend), before);
            assert.equal(hooks.changes, changes);
            backend.calls.readSecret[0].resolve(credentials);
            await work;
            assertSecret(controller.secret, password);
            assert.ok(!controller.status().includes(password), 'revealed credentials stay out of status');
        });

        it('tolerates native details racing a status request', t => {
            const { controller, backend } = fixture(t, [active()]);
            backend.detailsError = new Error('Connection transitioned.');
            const status = JSON.parse(controller.status());
            assert.equal(status.class, 'wifi');
            assert.match(status.tooltip, /Reading IPv4/);
            assert.equal(controller.error, '');
        });

        it('returns offline/unavailable status without querying details', t => {
            const { controller, backend } = fixture(t, [], { opened: false });
            backend.available = false;
            const status = JSON.parse(controller.status());
            assert.equal(status.class, 'offline');
            assert.match(status.tooltip, /unavailable/);
            assert.deepEqual(backend.calls.details, []);
        });

        it('disposes idempotently, cancels authorization, and ignores late native notifications and results', async t => {
            const { controller, backend, hooks } = fixture(t, [active()]);
            controller.info('wifi');
            const work = controller.revealSecret();
            const request = backend.calls.readSecret[0];
            const changes = hooks.changes;
            controller.dispose();
            controller.dispose();
            assert.equal(backend.calls.dispose, 1);
            assert.equal(request.io.cancelled, true);
            assert.equal(request.io.cancelCalls, 1);
            assert.equal(backend.opened, false);
            assertCleared(controller);
            backend.rows = [];
            backend.notify();
            request.resolve(credentials);
            await work;
            assert.equal(hooks.changes, changes);
            assertCleared(controller);
        });

        it('cancels a pending activation on dispose and ignores its late authentication rejection', async t => {
            const { controller, backend, hooks } = fixture(t, [saved()]);
            const work = controller.connectNetwork('wifi');
            const request = backend.calls.activate[0];
            controller.dispose();
            const changes = hooks.changes;
            assert.equal(request.options.cancellable.cancelled, true);
            request.reject(authenticationFailure());
            await work;
            assert.equal(backend.calls.dispose, 1);
            assert.equal(hooks.changes, changes);
            assert.equal(controller.error, '');
            assertCleared(controller);
        });


    });
}

async function nodeLogicTests() {
    const { default: assert } = await import('node:assert/strict');
    const { describe, it } = await import('node:test');
    const {
        escapeMarkup, identityEqual, isUndetectedSaved, parseRequest, primary, profileStamp,
        signalBars, signalIcon, signalLabel, sortRows, waybarStatus,
    } = await import('./logic.js');

    function row(key, overrides = {}) {
        return {
            key, name: key, interface: 'wlan0', wireless: true,
            connected: false, changing: false, known: false,
            strength: 50, security: 'WPA2 Personal', ...overrides,
        };
    }

    const identity = {
        interface: 'wlan0', ssid: 'Cafe', profileUuid: 'profile-a',
        bssid: '02:00:00:00:00:01', activePath: '/active/1',
    };

    const requestError = { name: 'Error', message: 'Invalid network panel request.' };

    describe('signal formatting', () => {
        it('labels finite signal strengths numerically, including a genuine zero', () => {
            for (const strength of [0, 0.5, 51, 100, -1, 101])
                assert.equal(signalLabel(strength), `${strength}%`);
        });

        it('labels unknown and non-numeric strengths as not currently detected without coercion', () => {
            for (const strength of [null, undefined, NaN, Infinity, -Infinity, '0', '51', '', false])
                assert.equal(signalLabel(strength), 'Not currently detected', `signal label for ${String(strength)}`);
        });

        it('clamps and quantizes into four ceil-rounded signal buckets', () => {
            for (const [strength, expected] of [
                [-100, 0], [0, 0], [0.1, 1], [1, 1], [24, 1], [25, 1],
                [25.1, 2], [26, 2], [50, 2], [50.1, 3], [51, 3], [75, 3],
                [75.1, 4], [76, 4], [100, 4], [101, 4], [Infinity, 4],
                [-Infinity, 0], ['51', 3], ['', 0], [null, 0], [undefined, 0],
                [NaN, 0], ['not a signal', 0],
            ]) assert.equal(signalBars(strength), expected, `signal bucket for ${String(strength)}`);
        });

        it('uses a distinct icon for each bucket, with the same boundary behavior', () => {
            const icons = [0, 1, 26, 51, 76].map(signalIcon);
            assert.deepEqual(icons, ['󰤯', '󰤟', '󰤢', '󰤥', '󰤨']);
            assert.equal(signalIcon(25), icons[1]);
            assert.equal(signalIcon(1000), icons[4]);
            assert.equal(signalIcon(undefined), icons[0]);
            assert.equal(signalIcon(null), icons[0]);
        });
    });

    describe('saved-undetected tab classification', () => {
        it('accepts only saved idle wireless rows with non-finite, non-coerced strength', () => {
            for (const strength of [null, undefined, NaN, Infinity, -Infinity, '0', '64', '', false]) {
                const selected = row('saved', { known: true, strength });
                assert.equal(isUndetectedSaved(selected), true, `Undetected strength ${String(strength)}`);
                assert.equal(selected.strength, strength, 'Classification does not mutate the row');
            }
        });

        it('keeps real zero, detected, connected, changing, unsaved and wired rows in Nearby', () => {
            for (const strength of [0, 0.5, 64, 100, -1, 101])
                assert.equal(isUndetectedSaved(row('saved', { known: true, strength })), false);
            for (const overrides of [
                { connected: true }, { changing: true }, { connected: true, changing: true },
                { known: false }, { wireless: false }, { wireless: false, connected: true },
            ]) {
                for (const strength of [null, undefined, NaN])
                    assert.equal(isUndetectedSaved(row('nearby', { known: true, strength, ...overrides })), false);
            }
        });
    });

    describe('row ordering and preferred connection', () => {
        it('sorts Ethernet before Wi-Fi, including a connected Wi-Fi row', () => {
            const rows = [
                row('wifi-connected', { connected: true }),
                row('wired-new', { wireless: false, interface: 'eth1' }),
                row('wired-saved', { wireless: false, known: true, interface: 'eth0' }),
                row('wired-changing', { wireless: false, changing: true }),
            ];
            assert.deepEqual(sortRows(rows).map(item => item.key), [
                'wired-changing', 'wired-saved', 'wired-new', 'wifi-connected',
            ]);
        });

        it('ranks connected/connecting, saved, and unsaved rows before considering strength', () => {
            const rows = [
                row('unsaved', { strength: 100 }),
                row('saved', { known: true, strength: 75 }),
                row('changing', { changing: true, strength: 1 }),
                row('connected', { connected: true, strength: 26 }),
            ];
            assert.deepEqual(sortRows(rows).map(item => item.key), ['connected', 'changing', 'saved', 'unsaved']);
        });

        it('sorts by signal bucket rather than noisy raw strength, then case-insensitive name', () => {
            const rows = [
                row('z', { name: 'Zulu', strength: 50 }),
                row('a', { name: 'alpha', strength: 26 }),
                row('b', { name: 'Bravo', strength: 49 }),
                row('strong', { name: 'zz', strength: 51 }),
            ];
            assert.deepEqual(sortRows(rows).map(item => item.key), ['strong', 'a', 'b', 'z']);
        });

        it('keeps undetected saved rows in the zero signal bucket without changing row priorities', () => {
            const rows = [
                row('zero', { name: 'Zulu', known: true, strength: 0 }),
                row('unknown', { name: 'Alpha', known: true, strength: null }),
                row('detected', { known: true, strength: 1 }),
                row('unsaved', { strength: 100 }),
                row('connected', { connected: true, strength: null }),
                row('wired', { wireless: false, strength: 0 }),
            ];
            assert.deepEqual(sortRows(rows).map(item => item.key), [
                'wired', 'connected', 'detected', 'unknown', 'zero', 'unsaved',
            ]);
            assert.equal(rows[1].strength, null);
            assert.equal(rows[0].strength, 0);
        });

        it('puts WPA/WPA2 profiles first in Saved, retaining alphabetical order within each group', () => {
            const rows = [
                row('open', { name: 'Alpha', known: true, strength: null, securityType: 'open' }),
                row('wpa2', { name: 'Zulu', known: true, strength: null, securityType: 'psk', security: 'WPA2' }),
                row('wpa3', { name: 'Bravo', known: true, strength: null, securityType: 'sae' }),
                row('mixed', { name: 'Delta', known: true, strength: null, securityType: 'psk', security: 'WPA/WPA2' }),
                row('wpa', { name: 'Charlie', known: true, strength: null, securityType: 'psk', security: 'WPA' }),
            ];
            assert.deepEqual(sortRows(rows).map(item => item.key), ['wpa', 'mixed', 'wpa2', 'open', 'wpa3']);
        });

        it('does not prioritize WPA/WPA2 over other security types in Nearby', () => {
            const rows = [
                row('psk', { name: 'Zulu', known: true, strength: 50, securityType: 'psk' }),
                row('open', { name: 'Alpha', known: true, strength: 50, securityType: 'open' }),
            ];
            assert.deepEqual(sortRows(rows).map(item => item.key), ['open', 'psk']);
        });

        it('breaks equal-name ties by interface and stable row key', () => {
            const rows = [
                row('c', { name: 'CAFE', interface: 'wlan1' }),
                row('b', { name: 'cafe' }),
                row('a', { name: 'Cafe' }),
            ];
            assert.deepEqual(sortRows(rows).map(item => item.key), ['a', 'b', 'c']);
        });

        it('does not sort Ethernet by a meaningless wireless signal value', () => {
            const rows = [
                row('z', { wireless: false, strength: 100 }),
                row('a', { wireless: false, strength: 0 }),
            ];
            assert.deepEqual(sortRows(rows).map(item => item.key), ['a', 'z']);
        });

        it('returns a new array without mutating backend rows or replacing their objects', () => {
            const rows = Object.freeze([Object.freeze(row('z')), Object.freeze(row('a'))]);
            const sorted = sortRows(rows);
            assert.notEqual(sorted, rows);
            assert.deepEqual(rows.map(item => item.key), ['z', 'a']);
            assert.equal(sorted[0], rows[1]);
            assert.deepEqual(sortRows([]), []);
        });

        it('prefers connected Ethernet even when the input is not sorted', () => {
            const wifi = row('wifi', { connected: true });
            const wired = row('wired', { wireless: false, connected: true });
            assert.equal(primary([wifi, row('other'), wired]), wired);
            assert.equal(primary([row('inactive'), wifi]), wifi);
        });

        it('never mistakes connecting or saved rows for an active connection', () => {
            assert.equal(primary([row('pending', { changing: true }), row('saved', { known: true })]), null);
            assert.equal(primary([]), null);
        });
    });

    describe('credential identity and profile stamps', () => {
        it('compares the complete native identity, not object references or cosmetic metadata', () => {
            assert.equal(identityEqual(identity, { ...identity }), true);
            assert.equal(identityEqual(identity, { ...identity, profileId: 'Renamed profile', strength: 99 }), true);
            assert.equal(identityEqual(null, identity), false);
            assert.equal(identityEqual(identity, null), false);
            assert.equal(identityEqual(undefined, undefined), false);
        });

        for (const field of Object.keys(identity)) {
            it(`rejects an identity whose ${field} changed or disappeared`, () => {
                assert.equal(identityEqual(identity, { ...identity, [field]: `${identity[field]}-changed` }), false);
                const missing = { ...identity };
                delete missing[field];
                assert.equal(identityEqual(identity, missing), false);
            });
        }

        it('stamps saved profile UUIDs independent of their order or display names', () => {
            const first = { profiles: [{ uuid: 'uuid-b', id: 'Duplicate' }, { uuid: 'uuid-a', id: 'Duplicate' }] };
            const second = { profiles: [{ uuid: 'uuid-a', id: 'Renamed' }, { uuid: 'uuid-b', id: 'Other' }] };
            assert.equal(profileStamp(first), profileStamp(second));
            assert.deepEqual(first.profiles.map(profile => profile.uuid), ['uuid-b', 'uuid-a']);
            assert.notEqual(profileStamp(first), profileStamp({ profiles: [{ uuid: 'uuid-a' }] }));
            assert.notEqual(profileStamp(first), profileStamp({ profiles: [{ uuid: 'uuid-a' }, { uuid: 'uuid-c' }] }));
            assert.equal(profileStamp(null), profileStamp({ profiles: [] }));
        });
    });

    describe('markup and Waybar JSON fields', () => {
        it('escapes every Pango-sensitive character while preserving Unicode and newlines', () => {
            assert.equal(escapeMarkup(`Cafe & <guest> "quoted" 'name'\n网络`),
                'Cafe &amp; &lt;guest&gt; &quot;quoted&quot; &apos;name&apos;\n网络');
            assert.equal(escapeMarkup('&amp;'), '&amp;amp;');
            assert.equal(escapeMarkup(0), '0');
        });

        it('formats Ethernet as primary with all IPv4 addresses', () => {
            const status = waybarStatus([
                row('wifi', { connected: true }),
                row('wired', { wireless: false, connected: true, interface: 'eth<&>' }),
            ], { ipv4: ['192.0.2.10/24', '198.51.100.2/24'] }, true);
            assert.deepEqual(status, {
                text: "<span size='large'>󰈀</span>", class: 'ethernet',
                tooltip: 'Ethernet: eth&lt;&amp;&gt;\nIPv4: 192.0.2.10/24, 198.51.100.2/24',
            });
        });

        it('formats Wi-Fi strength and security without allowing tooltip markup injection', () => {
            const status = waybarStatus([row('wifi', {
                connected: true, name: `<Cafe & "guest" 'wifi'>`,
                strength: 51, security: '<WPA2>',
            })], { ipv4: '<unknown>' }, true);
            assert.deepEqual(status, {
                text: "<span size='large'>󰤥</span>", class: 'wifi',
                tooltip: 'Wi-Fi: &lt;Cafe &amp; &quot;guest&quot; &apos;wifi&apos;&gt;\nSignal: 51%\nSecurity: &lt;WPA2&gt;\nIPv4: &lt;unknown&gt;',
            });
            assert.deepEqual(JSON.parse(JSON.stringify(status)), status);
        });

        for (const strength of [null, undefined]) {
            it(`formats unknown Wi-Fi strength (${String(strength)}) without a bogus percentage`, () => {
                const status = waybarStatus([row('wifi', { connected: true, strength })], {}, true);
                assert.deepEqual(status, {
                    text: "<span size='large'>󰤯</span>", class: 'wifi',
                    tooltip: 'Wi-Fi: wifi\nSignal: Not currently detected\nSecurity: WPA2 Personal\nIPv4: No IPv4',
                });
                assert.doesNotMatch(status.tooltip, /(?:null|undefined)%/);
            });
        }

        it('keeps a detected zero Wi-Fi strength as 0% in the Waybar tooltip', () => {
            assert.deepEqual(waybarStatus([row('wifi', { connected: true, strength: 0 })], {}, true), {
                text: "<span size='large'>󰤯</span>", class: 'wifi',
                tooltip: 'Wi-Fi: wifi\nSignal: 0%\nSecurity: WPA2 Personal\nIPv4: No IPv4',
            });
        });

        for (const [label, details, address] of [
            ['pending details', null, 'Reading IPv4…'],
            ['an empty address list', { ipv4: [] }, 'No IPv4'],
            ['missing IPv4', {}, 'No IPv4'],
            ['an empty address', { ipv4: '' }, 'No IPv4'],
            ['a single address', { ipv4: '192.0.2.1/24' }, '192.0.2.1/24'],
        ]) {
            it(`distinguishes ${label}`, () => {
                const status = waybarStatus([row('wifi', { connected: true })], details, true);
                assert.ok(status.tooltip.endsWith(`IPv4: ${address}`));
            });
        }

        it('shows connecting only when there is no connected row', () => {
            const pending = row('pending', { changing: true });
            assert.deepEqual(waybarStatus([pending], null, true), {
                text: "<span size='large'>󰔟</span>", tooltip: 'Connecting…', class: 'connecting',
            });
            assert.equal(waybarStatus([pending, row('active', { connected: true })], {}, true).class, 'wifi');
        });

        it('distinguishes an idle service from unavailable NetworkManager/devices', () => {
            assert.deepEqual(waybarStatus([row('saved', { known: true })], null, true), {
                text: "<span size='large'>󰤭</span>", tooltip: 'Network disconnected', class: 'offline',
            });
            assert.deepEqual(waybarStatus([], null, false), {
                text: "<span size='large'>󰤭</span>",
                tooltip: 'NetworkManager unavailable or no network devices', class: 'offline',
            });
        });
    });

    describe('strict panel request parsing', () => {
        it('accepts only the four panel commands, with an optional monitor for toggle', () => {
            for (const request of [
                { command: 'status' }, { command: 'close' }, { command: 'toggle' }, { command: 'toggleWifi' },
                { command: 'toggle', monitor: '' }, { command: 'toggle', monitor: 'DP-1' },
                { command: 'toggle', monitor: `屏幕 'quoted'; echo $(not-a-shell)` },
            ]) assert.deepEqual(parseRequest(JSON.stringify(request)), request);
        });

        it('allows whitespace and exact request/monitor length limits', () => {
            const request = '{"command":"status"}';
            assert.deepEqual(parseRequest(request.padEnd(8192, ' ')), { command: 'status' });
            assert.deepEqual(parseRequest(' \n' + request + '\t'), { command: 'status' });
            const monitor = 'm'.repeat(512);
            assert.deepEqual(parseRequest(JSON.stringify({ command: 'toggle', monitor })), { command: 'toggle', monitor });
        });

        it('rejects malformed JSON, non-string transport, and non-object requests', () => {
            for (const input of [
                '', 'not JSON', '{', '{"command":"status",}', '{"command":"status"}{}',
                'null', 'true', '42', '"status"', '[]', '[{"command":"status"}]',
                null, undefined, 42, true, {}, { command: 'status' }, Buffer.from('{"command":"status"}'),
            ]) assert.throws(() => parseRequest(input), requestError);
        });

        it('rejects missing, wrong-type, unknown, case-changed, and native mutation commands', () => {
            for (const command of [undefined, null, 1, true, [], {}, '', 'Status', 'togglewifi', 'ToggleWifi', 'run', 'connect', 'disconnect', 'forget', 'scan', 'wifi']) {
                assert.throws(() => parseRequest(JSON.stringify({ command })), requestError);
            }
        });

        it('rejects every unknown field, including native mutation arguments and prototype-shaped fields', () => {
            for (const command of ['status', 'toggle', 'close', 'toggleWifi']) {
                for (const extra of ['extra', 'password', 'profileUuid', 'interface', '__proto__', 'constructor']) {
                    assert.throws(() => parseRequest(JSON.stringify({ command, [extra]: true })), requestError);
                }
            }
            for (const command of ['status', 'close', 'toggleWifi']) {
                for (const monitor of ['', 'DP-1', null, 7]) {
                    assert.throws(() => parseRequest(JSON.stringify({ command, monitor })), requestError);
                }
            }
        });

        it('rejects non-string and oversized monitors', () => {
            for (const monitor of [null, 1, false, [], {}, 'm'.repeat(513)]) {
                assert.throws(() => parseRequest(JSON.stringify({ command: 'toggle', monitor })), requestError);
            }
        });

        it('rejects every ASCII control character in monitor names, including DEL', () => {
            for (const code of [...Array.from({ length: 32 }, (_, index) => index), 127]) {
                const monitor = `DP${String.fromCharCode(code)}1`;
                assert.throws(() => parseRequest(JSON.stringify({ command: 'toggle', monitor })), requestError);
            }
        });

        it('rejects oversized transport with a fixed error rather than reflecting untrusted input', () => {
            assert.throws(() => parseRequest('{"command":"status"}'.padEnd(8193, ' ')), requestError);
        });
    });
}

async function nodeLauncherTests() {
    const { default: test } = await import('node:test');
    const { default: assert } = await import('node:assert/strict');
    const { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } = await import('node:fs');
    const { spawnSync } = await import('node:child_process');
    const { tmpdir } = await import('node:os');
    const { join } = await import('node:path');

    const root = new URL('../../', import.meta.url);
    const read = path => readFileSync(new URL(path, root), 'utf8');

    test('Niri imports the exported panel Home Manager module', () => {
        const source = read('niri/default.nix');
        assert.match(source, /imports = \[\s*\(import \.\/panels\.nix\)\.module\b/);
        assert.doesNotMatch(source, /(?:^|\n)\s*\.\/panels\s*(?:\n|$)/);
    });


    test('Waybar network middle-click toggles Wi-Fi without a monitor or a panel visibility command', () => {
        const source = read('niri/waybar/system.nix');
        const module = source.match(/networkModule = monitor: \{([\s\S]*?)\n  \};/);
        assert.ok(module, 'Waybar defines the shared network module');
        assert.ok(module[1].includes('on-click-middle = "${lib.getExe packages.networkPanel} toggleWifi";'));
        assert.ok(module[1].includes('on-click = "${lib.getExe packages.networkPanel} toggle ${lib.escapeShellArg monitor}";'),
            'Left-click still opens or closes the panel on its monitor');
    });


    const launcherPath = process.env.NETWORK_PANEL_LAUNCHER;
    const panelLauncherPath = process.env.PANEL_LAUNCHER;

    test('built network launcher', {
        skip: launcherPath && panelLauncherPath ? false
            : 'Set NETWORK_PANEL_LAUNCHER to the built network-panel wrapper and PANEL_LAUNCHER to the shared Rust executable; both are required.',
    }, async t => {
        const { readdirSync, realpathSync } = await import('node:fs');
        const { dirname } = await import('node:path');
        const fixture = mkdtempSync(join(tmpdir(), 'network-launcher-'));
        t.after(() => rmSync(fixture, { recursive: true, force: true }));

        function invoke(executable, args, env = process.env) {
            const result = spawnSync(executable, args, { env, encoding: 'utf8',
                timeout: 5000, maxBuffer: 1024 * 1024 });
            assert.ifError(result.error);
            assert.equal(result.signal, null);
            return result;
        }

        function checkUsage(run) {
            for (const args of [[], ['unknown'], ['run', 'extra'], ['status', 'extra'], ['close', 'extra'], ['toggleWifi', 'DP-1'], ['toggleWifi', ''],
                ['toggle', 'DP-1', 'extra'], ['--help', 'extra'], ['-h', 'extra']]) {
                const result = run(args);
                assert.equal(result.status, 2, JSON.stringify(args));
                assert.match(result.stderr, /Usage:/);
                assert.equal(result.stdout, '');
            }
            for (const command of ['--help', '-h']) {
                const result = run([command]);
                assert.equal(result.status, 0, result.stderr);
                assert.match(result.stdout, /^Usage: network-panel/);
                assert.match(result.stdout, /toggleWifi/);
            }
        }

        await t.test('installed wrapper validates command arity and help', () => {
            checkUsage(args => invoke(launcherPath, args));
        });

        const manifestPath = join(dirname(realpathSync(launcherPath)), '..', 'share', 'network-panel', 'launcher.json');
        const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));
        assert.equal(manifest.name, 'network-panel');
        assert.match(manifest.bundle, /^\/nix\/store\/[a-z0-9]{32}-[^/]+(?:\/|$)/, 'the prebuilt bundle must be immutable');
        assert.ok(statSync(manifest.bundle).isFile());
        assert.match(manifest.forced_environment.NETWORK_PANEL_CONFIG,
            /^\/nix\/store\/[^/]+\/(?:scripts\/)?network-manager\/frontend\.js$/,
            'the entry must come from the packaged source, never the checkout or XDG config');
        for (const key of ['NETWORK_PANEL_PALETTE', 'PANEL_CSS_FILE', 'DASHBOARD_PANEL', 'MEDIA_PANEL']) {
            assert.equal(typeof manifest.environment[key], 'string', `${key} must have a manifest default`);
            assert.notEqual(manifest.environment[key], '');
        }
        const keys = [...new Set([...Object.keys(manifest.environment), ...Object.keys(manifest.forced_environment), 'XDG_RUNTIME_DIR'])];
        const fixtureManifest = { ...manifest, ags: join(fixture, 'ags'), gjs: join(fixture, 'panel-gjs') };
        // Read the real prebuilt JavaScript without importing GTK, Astal or NetworkManager.
        for (const [program, fake] of [['ags', fixtureManifest.ags], ['panel-gjs', fixtureManifest.gjs]]) {
            writeFileSync(fake, `#!${process.execPath}\n`
                + 'const fs = require("node:fs");\n'
                + `const program = ${JSON.stringify(program)}, keys = ${JSON.stringify(keys)};\n`
                + 'const argv = process.argv.slice(2);\n'
                + 'const data = {program, argv, env: Object.fromEntries(keys.map(key => [key, process.env[key]]))};\n'
                + 'if (program === "panel-gjs") {\n'
                + '  data.bundle = argv.at(-1);\n'
                + '  fs.readFileSync(data.bundle);\n'
                + '}\n'
                + 'console.log(JSON.stringify(data));\n'
                + 'if (program === "ags" && argv[0] !== "request") {\n'
                + '  console.error("AGS may only request: runtime bundling and AGS run are forbidden");\n'
                + '  process.exit(41);\n'
                + '}\n');
            chmodSync(fake, 0o700);
        }
        const configPath = join(fixture, 'launcher fixture.json');
        writeFileSync(configPath, JSON.stringify(fixtureManifest));
        const env = { ...process.env, XDG_CONFIG_HOME: join(fixture, 'config with spaces'),
            XDG_RUNTIME_DIR: join(fixture, 'runtime with spaces') };
        for (const key of keys.filter(key => key !== 'XDG_RUNTIME_DIR')) delete env[key];
        mkdirSync(env.XDG_RUNTIME_DIR, { mode: 0o700 });
        const sharedBundle = join(env.XDG_RUNTIME_DIR, 'ags.js');
        writeFileSync(sharedBundle, 'another panel owns this shared-path sentinel');

        function runtimeState(path) {
            const stat = statSync(path);
            return { mode: stat.mode, mtimeMs: stat.mtimeMs, ctimeMs: stat.ctimeMs,
                content: stat.isDirectory()
                    ? readdirSync(path).sort().map(name => [name, runtimeState(join(path, name))])
                    : readFileSync(path, 'utf8') };
        }

        function run(args, overrides = {}) {
            const childEnv = { ...env, ...overrides };
            const runtimeDir = childEnv.XDG_RUNTIME_DIR;
            const before = runtimeDir ? runtimeState(runtimeDir) : null;
            const result = invoke(panelLauncherPath, ['--config', configPath, ...args], childEnv);
            if (runtimeDir) assert.deepEqual(runtimeState(runtimeDir), before, 'the launcher must not write runtime files');
            return result;
        }

        function success(args, overrides) {
            const result = run(args, overrides);
            assert.equal(result.status, 0, result.stderr);
            return result.stdout.trim().split('\n').map(line => JSON.parse(line));
        }

        function request(args, overrides = {}) {
            const records = success(args, overrides);
            assert.equal(records.length, 1, 'requests neither compile nor launch GJS');
            const [data] = records;
            assert.equal(data.program, 'ags');
            assert.deepEqual(data.argv.slice(0, 4), ['request', '--instance', 'network-panel', '--']);
            assert.equal(data.argv.length, 5);
            assert.equal(data.env.XDG_RUNTIME_DIR, overrides.XDG_RUNTIME_DIR ?? env.XDG_RUNTIME_DIR,
                'IPC uses the original runtime directory');
            assert.equal(data.env.NETWORK_PANEL_CONFIG, manifest.forced_environment.NETWORK_PANEL_CONFIG);
            return data;
        }

        function launch(overrides = {}) {
            const records = success(['run'], overrides);
            assert.equal(records.length, 1, 'start GJS exactly once without invoking AGS');
            const [runtime] = records;
            assert.equal(runtime.program, 'panel-gjs');
            assert.deepEqual(runtime.argv, ['-m', manifest.bundle]);
            assert.equal(runtime.bundle, manifest.bundle, 'GJS reads only the installed immutable bundle');
            assert.equal(runtime.env.XDG_RUNTIME_DIR, overrides.XDG_RUNTIME_DIR ?? env.XDG_RUNTIME_DIR,
                'the native socket namespace must not change');
            assert.equal(runtime.env.NETWORK_PANEL_CONFIG, manifest.forced_environment.NETWORK_PANEL_CONFIG);
            assert.equal(readFileSync(sharedBundle, 'utf8'), 'another panel owns this shared-path sentinel');
            return runtime;
        }

        await t.test('validates commands and help without invoking AGS or GJS', () => {
            checkUsage(run);
        });

        await t.test('requests retain the network instance, JSON payload and original runtime directory', () => {
            const marker = join(fixture, 'monitor-was-evaluated');
            for (const args of [['status'], ['close'], ['toggleWifi'], ['toggle'], ['toggle', ''], ['toggle', 'eDP-1'],
                ['toggle', 'Display with spaces'], ['toggle', 'Display "quoted" \\ with\nnewlines\tand tabs'],
                ['toggle', `DP-'"; touch ${marker}; $(touch ${marker}); 🌐`]]) {
                const data = request(args);
                assert.deepEqual(JSON.parse(data.argv[4]), { command: args[0], ...(args[1] ? { monitor: args[1] } : {}) });
            }
            assert.equal(existsSync(marker), false);
            assert.equal(existsSync(join(env.XDG_RUNTIME_DIR, 'ags-panels')), false);
        });

        await t.test('runs the prebuilt bundle with store-backed defaults and live CSS', () => {
            const runtime = launch();
            assert.deepEqual(runtime.env, { ...manifest.environment, ...manifest.forced_environment,
                XDG_RUNTIME_DIR: env.XDG_RUNTIME_DIR });
            assert.match(runtime.env.NETWORK_PANEL_PALETTE, /^\/nix\/store\//);
            assert.match(runtime.env.PANEL_CSS_FILE, /\/scripts\/network-manager\/style\.css$/);
            assert.doesNotMatch(runtime.env.PANEL_CSS_FILE, /^\/nix\/store\//);
            assert.equal(runtime.env.DASHBOARD_PANEL, 'dashboard-panel');
            assert.equal(runtime.env.MEDIA_PANEL, 'media-panel');
        });

        await t.test('missing and empty environment variables use manifest defaults for runs and requests', () => {
            const empty = Object.fromEntries(Object.keys(manifest.environment).map(key => [key, '']));
            for (const overrides of [{}, { ...empty, NETWORK_PANEL_CONFIG: '' }]) {
                for (const data of [launch(overrides), request(['status'], overrides)]) {
                    assert.deepEqual(data.env, { ...manifest.environment, ...manifest.forced_environment,
                        XDG_RUNTIME_DIR: env.XDG_RUNTIME_DIR });
                }
            }
        });

        await t.test('ignores live-source overrides but preserves literal palette, peer and CSS overrides', () => {
            const marker = join(fixture, 'config-was-evaluated');
            const overrides = { NETWORK_PANEL_CONFIG: join(fixture, `custom '"; $(touch ${marker}); frontend.js`),
                NETWORK_PANEL_PALETTE: join(fixture, 'custom palette.json'),
                PANEL_CSS_FILE: join(fixture, 'custom style with spaces.css'),
                DASHBOARD_PANEL: join(fixture, 'custom dashboard'),
                MEDIA_PANEL: join(fixture, 'custom media'), XDG_RUNTIME_DIR: join(fixture, 'isolated runtime $HOME with spaces') };
            mkdirSync(overrides.XDG_RUNTIME_DIR, { mode: 0o700 });
            for (const data of [launch(overrides), request(['status'], overrides)]) {
                assert.notEqual(data.env.NETWORK_PANEL_CONFIG, overrides.NETWORK_PANEL_CONFIG);
                assert.deepEqual(data.env, { ...manifest.environment, ...overrides, ...manifest.forced_environment });
            }
            assert.equal(existsSync(marker), false);
            assert.equal(existsSync(join(overrides.XDG_RUNTIME_DIR, 'ags.js')), false);
        });

        await t.test('ignores stale runtime bundles without compiling or writing runtime files', () => {
            const runtimeDir = join(fixture, 'stale bundle runtime');
            const bundleDir = join(runtimeDir, 'ags-panels');
            mkdirSync(bundleDir, { recursive: true, mode: 0o700 });
            writeFileSync(join(bundleDir, 'network-panel.js'), 'throw new Error("stale runtime bundle was executed");\n');
            writeFileSync(join(runtimeDir, 'ags.js'), 'another panel owns this stale shared bundle');
            launch({ XDG_RUNTIME_DIR: runtimeDir });
        });

        await t.test('a missing or empty runtime directory fails before any AGS or GJS invocation', () => {
            for (const XDG_RUNTIME_DIR of [undefined, '']) {
                const result = run(['run'], { XDG_RUNTIME_DIR });
                assert.notEqual(result.status, 0);
                assert.match(result.stderr, /XDG_RUNTIME_DIR/);
                assert.equal(result.stdout, '');
            }
        });
    });
}

async function nodeStartupTests() {
    const { default: test } = await import('node:test');
    const { default: assert } = await import('node:assert/strict');
    const { readFileSync } = await import('node:fs');
    const { default: vm } = await import('node:vm');

    const source = readFileSync(new URL('./frontend.js', import.meta.url), 'utf8');
    const entry = source.slice(source.lastIndexOf("if (GLib.getenv('NETWORK_PANEL_IMPORT_ONLY')"));

    async function bootstrap({ socketError, registration = true, registrationError, remote = false, importOnly = false } = {}) {
        const calls = [];
        const exit = new Error('test process exit');
        const context = {
            Gtk: { init() { calls.push('init'); } },
            GLib: { getenv() { return importOnly ? '1' : null; }, unsetenv() {} },
            console: { error() {} },
            printerr() { calls.push('diagnostic'); },
            System: { exit(code) { calls.push(['exit', code]); throw exit; } },
            NetworkApplication: class {
                constructor() { this.handlers = {}; }
                connect(name, callback) { this.handlers[name] = callback; }
                acquire_socket() { calls.push('socket'); if (socketError) throw new Error('occupied'); }
                register() { calls.push('register'); if (registrationError) throw new Error('registration failed'); return registration; }
                get_is_remote() { calls.push('remote'); return remote; }
                quit() { calls.push('quit'); }
                hold() { calls.push('hold'); }
                startPanel() { calls.push('start'); }
                disposePanel() { calls.push('dispose'); }
                async runAsync() { calls.push('run'); this.handlers.activate(); return 0; }
            },
        };
        try { await vm.runInNewContext(`(async () => { ${entry} })()`, context); }
        catch (error) { if (error !== exit) throw error; }
        return calls;
    }

    test('network bootstrap registers locally before holding or activating the application', async () => {
        assert.deepEqual(await bootstrap(), ['init', 'socket', 'register', 'remote', 'hold', 'run', 'start', ['exit', 0]]);
    });

    for (const [name, options] of [
        ['socket acquisition throws', { socketError: true }],
        ['registration throws', { registrationError: true }],
        ['registration returns false', { registration: false }],
        ['registration resolves to a remote instance', { remote: true }],
    ]) {
        test(`network bootstrap exits rather than serving an uninitialized badge when ${name}`, async () => {
            const calls = await bootstrap(options);
            assert.deepEqual(calls.slice(-3), ['quit', 'diagnostic', ['exit', 1]]);
            assert.ok(!calls.includes('hold') && !calls.includes('run') && !calls.includes('start'));
        });
    }

    test('import-only network widgets never acquire sockets or start services', async () => {
        assert.deepEqual(await bootstrap({ importOnly: true }), []);
    });
}

async function gjsNativeTests() {
    // GJS-only, offline tests: real libnm settings and Gio cancellation, simulated
    // devices/remote replies. No NM.Client is created and no live mutation is called.
    const { default: GLib } = await import('gi://GLib');
    const { default: Gio } = await import('gi://Gio');
    const { default: GObject } = await import('gi://GObject');
    const { default: NM } = await import('gi://NM?version=1.0');
    const { default: System } = await import('system');
    const { NetworkBackend } = await import('./backend.js');
    const { NetworkController } = await import('./logic.js');

    const S = NM['80211ApSecurityFlags'];
    const bytes = value => new GLib.Bytes(new TextEncoder().encode(value));
    const assert = (condition, message) => { if (!condition) throw new Error(message); };
    const equal = (actual, expected, message) => assert(JSON.stringify(actual) === JSON.stringify(expected), message);
    const later = (callback, milliseconds = 0) => GLib.timeout_add(GLib.PRIORITY_DEFAULT, milliseconds, () => { callback(); return GLib.SOURCE_REMOVE; });
    const delay = (milliseconds = 1) => new Promise(resolve => later(resolve, milliseconds));
    async function rejects(promise, expression, authentication = false) {
        let error;
        try { await promise; } catch (caught) { error = caught; }
        assert(error && expression.test(error.message), `Expected a safe native rejection matching ${expression}`);
        if (authentication) assert(error.authentication === true, 'Authentication error flag');
        return error;
    }

    class Signals {
        constructor() { this.handlers = new Map(); this.serial = 0; }
        connect(signal, callback) { const id = ++this.serial; this.handlers.set(id, { signal, callback }); return id; }
        disconnect(id) { assert(this.handlers.delete(id), 'Signal disconnected exactly once'); }
        emit(signal, ...args) { for (const handler of [...this.handlers.values()]) if (handler.signal === signal) handler.callback(this, ...args); }
    }

    class AP extends Signals {
        constructor(ssid, security, strength, suffix) {
            super(); Object.assign(this, { ssid: bytes(ssid), security, strength, suffix });
        }
        get_path() { return `/offline/ap/${this.suffix}`; }
        get_ssid() { return this.ssid; }
        get_strength() { return this.strength; }
        get_bssid() { return `02:00:00:00:00:${String(this.suffix).padStart(2, '0')}`; }
        get_frequency() { return 6115; }
        get_mode() { return NM['80211Mode'].INFRA; }
        get_flags() { return this.security === 'open' ? 0 : NM['80211ApFlags'].PRIVACY; }
        get_wpa_flags() { return this.wpa ?? 0; }
        get_rsn_flags() {
            return this.rsn ?? { open: 0, wep: 0, psk: S.KEY_MGMT_PSK, sae: S.KEY_MGMT_SAE,
                transition: S.KEY_MGMT_PSK | S.KEY_MGMT_SAE, owe: S.KEY_MGMT_OWE,
                enterprise: S.KEY_MGMT_802_1X, unknown: S.PAIR_CCMP }[this.security];
        }
        connection_valid(profile) {
            const wireless = profile.get_setting_wireless();
            const security = profile.get_setting_wireless_security();
            // libnm's GI annotation disallows null here even though open networks
            // have no security setting. The real NM.AccessPoint handles that case.
            const securityMatches = security
                ? wireless?.ap_security_compatible(security, this.get_flags(), this.get_wpa_flags(), this.get_rsn_flags(), this.get_mode())
                : this.get_flags() === 0 && this.get_wpa_flags() === 0 && this.get_rsn_flags() === 0;
            return Boolean(wireless && NM.utils_same_ssid(wireless.get_ssid().get_data(), this.ssid.get_data(), false)
                && (!wireless.get_bssid() || wireless.get_bssid() === this.get_bssid()) && securityMatches);
        }
    }

    class Device extends Signals {
        constructor(iface, wireless = true) {
            super(); Object.assign(this, { iface, wireless, aps: [], active: null, activeAP: null,
                state: NM.DeviceState.DISCONNECTED, reason: NM.DeviceStateReason.NONE, scans: 0, scanFinishes: 0, disconnects: 0,
                                holdScans: false, pendingScans: [] });
        }
        get_path() { return `/offline/device/${this.iface}`; }
        get_iface() { return this.iface; }
        get_device_type() { return this.wireless ? NM.DeviceType.WIFI : NM.DeviceType.ETHERNET; }
        get_access_points() { return this.aps; }
        get_active_access_point() { return this.activeAP; }
        get_active_connection() { return this.active; }
        get_state() { return this.state; }
        get_state_reason() { return this.reason; }
        get_ip4_config() { return null; }
        get_ip6_config() { return null; }
        get_hw_address() { return '02:00:00:00:00:ff'; }
        get_bitrate() { return 866700; }
        get_speed() { return 1000; }
        connection_valid(profile) {
            return profile.get_connection_type() === (this.wireless ? NM.SETTING_WIRELESS_SETTING_NAME : NM.SETTING_WIRED_SETTING_NAME)
                && (!profile.get_interface_name() || profile.get_interface_name() === this.iface);
        }
        request_scan_async(io, callback) {
            this.scans++;
            const scan = { io, finish: () => callback(this, { cancelled: io.is_cancelled() }) };
            if (this.holdScans) this.pendingScans.push(scan);
            else later(scan.finish, 5);
        }
        finishScans() { for (const scan of this.pendingScans.splice(0)) scan.finish(); }
        request_scan_finish(result) { this.scanFinishes++; if (result.cancelled) throw new Error('cancelled'); return true; }
        disconnect_async(io, callback) {
            this.disconnects++;
            this.state = NM.DeviceState.DISCONNECTED; this.active = null; this.activeAP = null;
            this.emit('notify'); later(() => callback(this, {}));
        }
        disconnect_finish() { return true; }
    }

    class Active extends Signals {
        constructor(profile, suffix) {
            super(); this.profile = profile; this.suffix = suffix;
            this.state = NM.ActiveConnectionState.ACTIVATING; this.reason = NM.ActiveConnectionStateReason.NONE;
        }
        get_path() { return `/offline/active/${this.suffix}`; }
        get_uuid() { return this.profile.get_uuid(); }
        get_id() { return this.profile.get_id(); }
        get_connection() { return this.profile; }
        get_connection_type() { return this.profile.get_connection_type(); }
        get_state() { return this.state; }
        get_state_reason() { return this.reason; }
    }

    class Client extends Signals {
        constructor() {
            super(); this.devices = []; this.profiles = []; this.running = true; this.enabled = true; this.hardware = true;
            this.calls = []; this.radioWrites = []; this.finishes = 0; this.accepted = null; this.automatic = true; this.replyDelay = 0;
        }
        get_devices() { return this.devices; }
        get_connections() { return this.profiles; }
        get_nm_running() { return this.running; }
        wireless_get_enabled() { return this.enabled; }
        wireless_hardware_get_enabled() { return this.hardware; }
        wireless_set_enabled(enabled) { this.radioWrites.push(enabled); this.enabled = enabled; this.emit('notify'); }
        activate_connection_async(profile, device, specific, io, callback) {
            this.calls.push({ profile, device, specific, saved: true }); this.accept(profile, device, specific, io, callback);
        }
        add_and_activate_connection_async(profile, device, specific, io, callback) {
            this.calls.push({ profile, device, specific, saved: false }); this.profiles.push(profile);
            this.accept(profile, device, specific, io, callback);
        }
        accept(profile, device, specific, io, callback) {
            const active = new Active(profile, this.calls.length);
            if (!this.deferBinding) {
                device.active = active; device.activeAP = device.aps.find(ap => ap.get_path() === specific) ?? null;
                device.state = NM.DeviceState.PREPARE;
            }
            this.accepted = { device, active, specific };
            device.emit('notify');
            later(() => {
                callback(this, { active, cancelled: io.is_cancelled() });
                if (this.automatic && !io.is_cancelled()) later(() => this.complete(active, device), 2);
            }, this.replyDelay);
        }
        activate_connection_finish(result) { this.finishes++; if (result.cancelled) throw new Error('cancelled with sensitive test value'); return result.active; }
        add_and_activate_connection_finish(result) { return this.activate_connection_finish(result); }
        complete(active = this.accepted.active, device = this.accepted.device) {
            active.state = NM.ActiveConnectionState.ACTIVATED; device.state = NM.DeviceState.ACTIVATED;
            active.emit('notify'); device.emit('notify');
        }
    }

    let profileSerial = 0;
    function profile(ssid, type, iface = null) {
        const connection = NM.SimpleConnection.new();
        const serial = ++profileSerial;
        connection.add_setting(new NM.SettingConnection({ id: `${ssid} ${serial}`, uuid: GLib.uuid_string_random(),
            type: NM.SETTING_WIRELESS_SETTING_NAME, interface_name: iface }));
        connection.add_setting(new NM.SettingWireless({ ssid: bytes(ssid), mode: 'infrastructure', hidden: true }));
        if (type !== 'open') {
            const key = { wep: 'none', psk: 'wpa-psk', sae: 'sae', owe: 'owe', enterprise: 'wpa-eap', unknown: 'unsupported' }[type];
            connection.add_setting(new NM.SettingWirelessSecurity({ key_mgmt: key }));
        }
        connection.set_path(`/offline/settings/${serial}`);
        connection.settingsVersion = 1;
        connection.get_version_id = () => connection.settingsVersion;
        connection.holdSecrets = false;
        connection.pendingSecrets = [];
        connection.secretCalls = 0;
        connection.secretFinishes = 0;
        connection.get_secrets_async = (setting, io, callback) => {
            connection.secretCalls++;
            assert(setting === NM.SETTING_WIRELESS_SECURITY_SETTING_NAME, 'Native secret setting name');
            const finish = () => callback(connection, { cancelled: io.is_cancelled() });
            if (connection.holdSecrets) connection.pendingSecrets.push({ io, finish });
            else later(finish, 5);
        };
        connection.get_secrets_finish = result => {
            connection.secretFinishes++;
            if (result.cancelled) throw new Error('sensitive cancelled password');
            if (connection.secretError) throw connection.secretError;
            // NM refreshes the profile cache/revision on successful GetSecrets,
            // even when the selected UUID and credential context are unchanged.
            connection.settingsVersion++;
            if (connection.secretReply) return connection.secretReply;
            return new GLib.Variant('a{sa{sv}}', { '802-11-wireless-security': {
                psk: new GLib.Variant('s', 'offline-secret'), 'wep-key0': new GLib.Variant('s', 'abcde'),
                'wep-key2': new GLib.Variant('s', 'fghij'),
            } });
        };
        connection.update2 = (settings, flags, args, io, callback) => {
            assert(flags === NM.SettingsUpdate2Flags.TO_DISK, 'Native profile update flags');
            assert(args.lookup_value('version-id', null).get_uint64() === connection.settingsVersion, 'Versioned profile update');
            connection.updated = NM.SimpleConnection.new_from_dbus(settings);
            later(() => callback(connection, {}));
        };
        connection.update2_finish = () => true;
        return connection;
    }

    function fixture() {
        const client = new Client();
        const wifi = new Device('wlan-test');
        client.devices.push(wifi);
        const backend = new NetworkBackend({ network: { client } });
        backend.start();
        return { client, wifi, backend };
    }
    function row(backend, ssid, security = null, iface = 'wlan-test') {
        backend._sync(false);
        const result = backend.rows.find(item => item.ssid === ssid && (!security || item.securityType === security) && item.interface === iface);
        assert(result, `Missing row ${ssid}/${security}/${iface}`);
        return result;
    }

    function securityLabels() {
        const { client, wifi, backend } = fixture();
        try {
            const cases = [
                ['open', 0, 0, 'open', 'Open'],
                ['wep', 0, 0, 'wep', 'WEP'],
                ['psk', S.KEY_MGMT_PSK, 0, 'psk', 'WPA'],
                ['psk', 0, S.KEY_MGMT_PSK, 'psk', 'WPA2'],
                ['psk', S.KEY_MGMT_PSK, S.KEY_MGMT_PSK, 'psk', 'WPA2'],
                ['sae', 0, S.KEY_MGMT_PSK | S.KEY_MGMT_SAE, 'sae', 'WPA3'],
                ['enterprise', S.KEY_MGMT_802_1X, 0, 'enterprise', 'WPA Enterprise'],
                ['enterprise', 0, S.KEY_MGMT_802_1X, 'enterprise', 'WPA2 Enterprise'],
                ['enterprise', S.KEY_MGMT_802_1X, S.KEY_MGMT_802_1X, 'enterprise', 'WPA2 Enterprise'],
                ['enterprise', 0, S.KEY_MGMT_EAP_SUITE_B_192, 'enterprise', 'WPA3 Enterprise'],
                ['owe', 0, S.KEY_MGMT_OWE, 'owe', 'Enhanced Open (OWE)'],
                ['owe', 0, S.KEY_MGMT_OWE_TM, 'owe', 'Enhanced Open (OWE)'],
                ['unknown', 0, S.PAIR_CCMP, 'unknown', 'Unknown'],
            ];
            for (const [type, wpa, rsn, grouping, label] of cases) {
                const ap = new AP('labels', type, 65, 1); ap.wpa = wpa; ap.rsn = rsn;
                wifi.aps = [ap];
                const selected = row(backend, 'labels');
                assert(selected.securityType === grouping && selected.security === label, `AP display ${label} preserves coarse ${grouping}`);
            }
            const wpa = new AP('roam', 'psk', 30, 1); wpa.wpa = S.KEY_MGMT_PSK; wpa.rsn = 0;
            const wpa2 = new AP('roam', 'psk', 90, 2);
            wifi.aps = [wpa, wpa2];
            const saved = profile('roam', 'psk'); client.profiles = [saved];
            const key = row(backend, 'roam').key;
            assert(row(backend, 'roam').security === 'WPA2', 'Strongest AP determines disconnected display');
            wifi.active = new Active(saved, 1); wifi.active.state = NM.ActiveConnectionState.ACTIVATED;
            wifi.activeAP = wpa; wifi.state = NM.DeviceState.ACTIVATED;
            const connected = row(backend, 'roam');
            assert(connected.key === key && connected.security === 'WPA', 'Active AP changes display without regrouping');
            wifi.active = null; wifi.activeAP = null; wifi.state = NM.DeviceState.DISCONNECTED;
            const legacy = profile('legacy', 'enterprise');
            const security = legacy.get_setting_wireless_security(); security.key_mgmt = 'ieee8021x';
            const legacyAP = new AP('legacy', 'wep', 60, 3); wifi.aps = [legacyAP]; client.profiles = [legacy];
            assert(row(backend, 'legacy').security === 'Dynamic WEP' && row(backend, 'legacy').securityType === 'wep', 'Native saved profile distinguishes Dynamic WEP without regrouping');
            security.auth_alg = 'leap';
            assert(row(backend, 'legacy').security === 'LEAP', 'Native auth-alg distinguishes LEAP');
            client.profiles = [];
            assert(row(backend, 'legacy').security === 'WEP', 'Privacy flags alone do not claim Dynamic WEP or LEAP');
            wifi.aps = [];
            const cached = [
                ['psk', 'wpa-psk', ['wpa'], 'WPA'], ['psk', 'wpa-psk', ['rsn'], 'WPA2'],
                ['psk', 'wpa-psk', [], 'WPA/WPA2'], ['psk', 'wpa-psk', ['wpa', 'rsn'], 'WPA/WPA2'],
                ['enterprise', 'wpa-eap', ['wpa'], 'WPA Enterprise'],
                ['enterprise', 'wpa-eap', ['rsn'], 'WPA2 Enterprise'],
                ['enterprise', 'wpa-eap', [], 'WPA/WPA2 Enterprise'],
                ['enterprise', 'wpa-eap-suite-b-192', ['rsn'], 'WPA3 Enterprise'],
                ['enterprise', 'ieee8021x', [], 'Dynamic WEP'],
            ];
            for (const [type, keyMgmt, protocols, label] of cached) {
                const remote = profile('cached', type); const setting = remote.get_setting_wireless_security();
                setting.key_mgmt = keyMgmt; for (const protocol of protocols) setting.add_proto(protocol);
                client.profiles = [remote];
                assert(row(backend, 'cached').security === label, `Cached profile display ${label}`);
            }
            const pmf = profile('pmf', 'enterprise');
            pmf.get_setting_wireless_security().set_property('pmf', NM.SettingWirelessSecurityPmf.REQUIRED);
            client.profiles = [pmf]; wifi.aps = [new AP('pmf', 'enterprise', 60, 4)];
            assert(row(backend, 'pmf').security === 'WPA2 Enterprise', 'PMF alone is not WPA3 Enterprise detection');
        } finally { backend.dispose(); }
    }

    async function activationCacheOrdering() {
        const { client, wifi, backend } = fixture();
        client.automatic = false; client.deferBinding = true;
        wifi.aps = [new AP('new', 'open', 75, 1)];
        wifi.active = new Active(profile('old', 'open'), 'old');
        wifi.state = NM.DeviceState.FAILED;
        try {
            const key = row(backend, 'new').key;
            let resolved = false; let error = null;
            const activation = backend.activate(key).then(() => { resolved = true; }, caught => { error = caught; });
            await delay(10);
            assert(!resolved && !error, 'An accepted target must not fail on the previous device FAILED state');
            const { active } = client.accepted;
            active.state = NM.ActiveConnectionState.ACTIVATED; active.emit('notify');
            await delay();
            assert(!resolved && !error, 'ActiveConnection cache may update before device association');
            wifi.state = NM.DeviceState.ACTIVATED; wifi.emit('notify'); await delay();
            assert(!resolved && !error, 'Both ACTIVATED flags still require the exact target device association');
            wifi.active = active; wifi.activeAP = wifi.aps[0]; wifi.emit('notify');
            await activation;
            assert(resolved && !error, 'Correct target association finally resolves activation');
        } finally { backend.dispose(); }
    }

    async function grouping() {
        const { client, wifi, backend } = fixture();
        const second = new Device('wlan-other'); const wired = new Device('en-test', false);
        client.devices.push(second, wired);
        wifi.aps = [new AP('same', 'psk', 20, 1), new AP('same', 'psk', 95, 2), new AP('same', 'open', 70, 3)];
        second.aps = [new AP('same', 'psk', 80, 4)];
        const saved = profile('same', 'psk', 'wlan-test');
        const cached = profile('out of range', 'wep', 'wlan-test');
        client.profiles = [saved, cached];
        const active = new Active(saved, 1); active.state = NM.ActiveConnectionState.ACTIVATED;
        wifi.active = active; wifi.activeAP = wifi.aps[0]; wifi.state = NM.DeviceState.ACTIVATED;
        try {
            let selected = row(backend, 'same', 'psk');
            assert(selected.connected && selected.strength === 20 && selected.profiles.length === 1, 'Active AP is preferred to stronger AP');
            const key = selected.key;
            assert(!row(backend, 'same', 'psk', 'wlan-other').known, 'Profiles restricted per device');
            assert(row(backend, 'out of range').known, 'Cached profiles remain visible');
            assert(backend.rows.some(item => item.kind === 'WIRED'), 'Ethernet row present');
            assert(backend.available && backend.wifiAvailable, 'Native device availability');
            const stamp = backend.getIdentity(key);
            equal(Object.keys(stamp).sort(), ['activePath', 'bssid', 'interface', 'profileUuid', 'ssid'].sort(), 'Plain identity contract');
            const details = backend.details(key);
            assert(details.frequency === '6115 MHz' && details.band === '6 GHz' && details.channel === '33' && details.rate === '866.7 Mbit/s', 'Native detail units');
            assert(JSON.parse(JSON.stringify(details)).identity.profileUuid === saved.get_uuid(), 'Details contain only plain data');
            wifi.active = null; wifi.activeAP = null; wifi.state = NM.DeviceState.DISCONNECTED;
            selected = row(backend, 'same', 'psk');
            assert(selected.key === key && selected.strength === 95 && !backend.getIdentity(key), 'Stable grouping and best AP selection');
            await delay(12); assert(wifi.scans === 0, 'No closed-panel scans');
            assert(typeof backend.refresh === 'undefined', 'Native scanning has no public manual-refresh path');
            backend.setOpened(true); await delay(12); assert(wifi.scans === 1 && second.scans === 1, 'All Wi-Fi devices scan while open');
            backend.setOpened(false); assert(!backend._scanTimer, 'Closing removes periodic scan');
            client.hardware = false;
            let error; try { backend.toggleWifi(); } catch (caught) { error = caught; }
            assert(/hardware/.test(error?.message), 'Hardware block error');
            client.devices = [wired]; client.emit('device-removed', wifi); await delay();
            assert(!backend.wifiAvailable && backend.rows.length === 1 && wifi.handlers.size === 0, 'Removed device signals released');
            client.running = false; client.emit('notify'); await delay();
            assert(!backend.available && backend.rows.length === 0, 'NM lifecycle reflected');
        } finally { backend.dispose(); }
        assert(client.handlers.size === 0 && wired.handlers.size === 0, 'Signal teardown');
    }

    async function savedSignalStrength() {
        const { client, wifi, backend } = fixture();
        const wired = new Device('en-test', false);
        const saved = profile('saved signal', 'psk'), alternate = profile('saved signal', 'psk');
        client.devices.push(wired); client.profiles = [saved, alternate];
        wifi.aps = [new AP('unrelated', 'open', 90, 1)];
        try {
            const initial = row(backend, 'saved signal', 'psk');
            const { key, profiles } = initial;
            equal(profiles.map(item => item.uuid).sort(), [saved.get_uuid(), alternate.get_uuid()].sort(),
                'Both saved UUIDs remain available without a matching AP');
            function check(strength, transition) {
                const matches = backend.rows.filter(item => item.ssid === 'saved signal');
                assert(matches.length === 1 && matches[0].key === key, `${transition}: saved row key is stable without duplicates`);
                assert(matches[0].strength === strength, `${transition}: expected strength ${String(strength)}`);
                assert(matches[0].known && !matches[0].connected, `${transition}: detection does not change saved or connection state`);
                equal(matches[0].profiles, profiles, `${transition}: saved profiles are unchanged`);
                assert(backend.rows.find(item => item.interface === 'en-test')?.strength === 0,
                    `${transition}: wired strength remains numeric zero`);
            }
            check(null, 'No matching AP');
            const ap = new AP('saved signal', 'psk', 64, 2);
            wifi.aps.push(ap); wifi.emit('access-point-added', ap); await delay(12);
            check(64, 'Matching AP appears');
            ap.strength = 0; ap.emit('notify'); await delay(12);
            check(0, 'Detected AP reports a genuine zero');
            wifi.aps = wifi.aps.filter(item => item !== ap);
            wifi.emit('access-point-removed', ap); await delay(12);
            check(null, 'Matching AP disappears');
            assert(client.calls.length === 0 && client.radioWrites.length === 0 && wifi.scans === 0,
                'Detection updates do not activate profiles, toggle the radio or scan a closed panel');
        } finally { backend.dispose(); }
    }

    async function offlineDetails() {
        const { client, wifi, backend } = fixture();
        const first = profile('offline saved', 'psk'), second = profile('offline saved', 'psk');
        const live = profile('different active network', 'psk');
        client.profiles = [first, second, live];
        wifi.activeAP = new AP('different active network', 'psk', 88, 8);
        wifi.aps = [wifi.activeAP];
        wifi.active = new Active(live, 'different'); wifi.active.state = NM.ActiveConnectionState.ACTIVATED;
        wifi.state = NM.DeviceState.ACTIVATED;
        const ip4 = Object.assign(new Signals(), {
            get_addresses: () => [{ get_address: () => '192.0.2.77', get_prefix: () => 24 }],
            get_gateway: () => '192.0.2.1', get_nameservers: () => ['192.0.2.53'],
        });
        wifi.get_ip4_config = () => ip4;
        const controller = new NetworkController({ backend, createCancellable: () => new Gio.Cancellable() });
        try {
            controller.start(); controller.toggle('offline-monitor');
            const selected = row(backend, 'offline saved'), key = selected.key;
            assert(selected.known && !selected.connected && selected.strength === null, 'Saved row is offline beside a different active connection');
            assert(backend.getIdentity(key) === null, 'Offline details never manufacture an active credential identity');
            function checkDetails() {
                const details = backend.details(key);
                assert(details.identity === null && details.ssid === 'offline saved' && details.interface === 'wlan-test',
                    'Offline metadata identifies the saved network and interface, not the device active network');
                const matching = [first, second].find(item => item.get_uuid() === details.profileUuid);
                assert(matching && details.profileId === matching.get_id(), 'Offline profile ID and UUID describe the same matching saved profile');
                for (const field of ['ipv4', 'ipv6', 'prefix', 'gateway4', 'gateway6', 'dns', 'mac', 'bssid',
                    'frequency', 'band', 'channel', 'mode', 'rate', 'activePath']) {
                    const value = details[field];
                    assert(value == null || value === '' || (Array.isArray(value) && value.length === 0),
                        `Offline ${field} does not leak live device/connection data`);
                }
                equal(JSON.parse(JSON.stringify(details)), details, 'Offline metadata is plain serializable data');
            }
            checkDetails();
            controller.info(key);
            assert(controller.page === 'info' && controller.details.identity === null, 'Real controller accepts native offline details');
            await rejects(backend.readSecret(key, controller.details.identity), /active connection changed/);
            assert([first, second, live].every(item => item.secretCalls === 0), 'Opening offline Info never authorizes secrets');
            controller.selectSavedProfile(second.get_uuid());
            assert(second.secretCalls === 0 && controller.selectedProfileUuid === second.get_uuid(), 'Selecting a saved UUID only updates metadata');
            await controller.revealSecret();
            assert(controller.page === 'info' && controller.secretRequested && !controller.secretLoading,
                'Explicit offline reveal stays on Info');
            assert(controller.secret === 'offline-secret' && second.secretCalls === 1 && second.secretFinishes === 1
                && first.secretCalls === 0 && live.secretCalls === 0, 'Only the explicitly selected offline profile supplies credentials');
            controller.hideSecret();
            assert(!controller.secret && !controller.secretRequested, 'Hiding drops the saved credential');
            const detected = new AP('offline saved', 'psk', 0, 9);
            wifi.aps.push(detected); wifi.emit('access-point-added', detected); await delay(12);
            assert(controller.page === 'info' && controller.selectedRow.strength === 0, 'A detected real zero does not invalidate offline Info');
            checkDetails();
            wifi.aps = wifi.aps.filter(ap => ap !== detected); wifi.emit('access-point-removed', detected); await delay(12);
            assert(controller.page === 'info' && controller.selectedRow.strength === null, 'Losing signal preserves offline Info');
            checkDetails();
            const activeRow = row(backend, 'different active network');
            const identity = backend.getIdentity(activeRow.key), details = backend.details(activeRow.key);
            equal(details.identity, identity, 'Active details retain the existing complete identity contract');
            equal(details.ipv4, ['192.0.2.77/24'], 'Active details retain live IPv4');
            equal(details.dns, ['192.0.2.53'], 'Active details retain live DNS');
            assert(details.profileUuid === live.get_uuid() && details.bssid === wifi.activeAP.get_bssid()
                && details.rate === '866.7 Mbit/s', 'Active profile, BSSID and link rate remain unchanged');
            assert(first.secretCalls === 0 && live.secretCalls === 0 && second.secretCalls === 1 && second.secretFinishes === 1
                && client.calls.length === 0 && client.radioWrites.length === 0 && wifi.disconnects === 0,
            'Offline Info and detection changes never automatically reread credentials or mutate network state');
        } finally { controller.dispose(); }
    }

    async function automaticScanning() {
        const { client, wifi, backend } = fixture();
        const second = new Device('wlan-other');
        client.devices.push(second);
        const saved = profile('scan', 'psk');
        client.profiles = [saved]; wifi.aps = [new AP('scan', 'psk', 70, 1)];
        wifi.active = new Active(saved, 'scan'); wifi.active.state = NM.ActiveConnectionState.ACTIVATED;
        wifi.activeAP = wifi.aps[0]; wifi.state = NM.DeviceState.ACTIVATED;
        wifi.holdScans = second.holdScans = true;
        const controller = new NetworkController({ backend, createCancellable: () => new Gio.Cancellable() });
        async function finishScans() { wifi.finishScans(); second.finishScans(); await delay(); }
        async function tick() {
            const source = GLib.MainContext.default().find_source_by_id(backend._scanTimer);
            assert(source, 'An open panel owns a native periodic scan source');
            // Advance only this source, not the system clock or the native adapter.
            source.set_ready_time(0);
            await delay(2);
        }
        try {
            controller.start();
            const key = row(backend, 'scan').key;
            assert(wifi.scans === 0 && second.scans === 0 && !backend._scanTimer, 'Startup stays scan-free while hidden');
            controller.toggle('offline-test');
            assert(wifi.scans === 1 && second.scans === 1, 'Opening immediately scans every native Wi-Fi device');
            const source = GLib.MainContext.default().find_source_by_id(backend._scanTimer);
            const interval = source.get_ready_time() - GLib.get_monotonic_time();
            assert(interval > 13e6 && interval < 17e6, 'Native periodic scan deadline is fifteen seconds, allowing GLib coalescing');
            controller.info(key);
            await tick();
            assert(controller.page === 'info' && wifi.scans === 1 && second.scans === 1,
                'Periodic ticks on info do not overlap in-flight scans');
            await finishScans();
            await tick();
            assert(wifi.scans === 2 && second.scans === 2, 'Periodic native scans continue on the info page');
            await finishScans();
            await tick();
            assert(controller.page === 'info' && wifi.scans === 3 && second.scans === 3, 'Periodic native scans continue across repeated Info ticks');
            controller.close();
            assert(!backend._scanTimer && !backend._scanMembers.size, 'Closing removes the periodic source and scan members');
            assert([...wifi.pendingScans, ...second.pendingScans].every(scan => scan.io.is_cancelled()), 'Closing cancels every in-flight native scan');
            await finishScans();
            assert(!backend._scans.size, 'Cancelled scan callbacks are drained');
            await delay(12);
            assert(wifi.scans === 3 && second.scans === 3, 'No scans resume while hidden');
            controller.toggle('offline-test');
            assert(wifi.scans === 4 && second.scans === 4, 'Reopening starts an immediate scan again');
            await finishScans();
            client.enabled = false; client.emit('notify'); await delay(12);
            assert(!backend._scanTimer && !backend._scanMembers.size && wifi.scans === 4 && second.scans === 4, 'Radio-off removes the timer and stops scans while open');
            client.enabled = true; client.emit('notify'); await delay(12);
            assert(backend._scanTimer && wifi.scans === 5 && second.scans === 5, 'Radio-on resumes immediate and periodic scans while open');
            client.hardware = false; client.emit('notify'); await delay(12);
            assert(!backend._scanTimer && !backend._scanMembers.size && wifi.scans === 5 && second.scans === 5, 'Hardware-off stops scans while open');
            assert([...wifi.pendingScans, ...second.pendingScans].every(scan => scan.io.is_cancelled()), 'Hardware-off cancels in-flight scans');
            await finishScans();
            client.hardware = true; client.emit('notify'); await delay(12);
            assert(backend._scanTimer && wifi.scans === 6 && second.scans === 6, 'Hardware recovery resumes automatic scans');
            await finishScans();
        } finally {
            controller.dispose();
            await finishScans();
        }
        assert(!backend._scanTimer && !backend._scans.size, 'Disposal releases every automatic scan');
    }

    async function nativeRadioToggle() {
        const { client, wifi, backend } = fixture();
        try {
            backend.toggleWifi(); backend.toggleWifi();
            equal(client.radioWrites, [false, true], 'Global toggles delegate exact radio state changes to the stubbed native client');
            assert(!backend._opened && !backend._scanTimer && wifi.scans === 0, 'A native radio toggle never opens or scans a hidden panel');
            client.hardware = false;
            let error; try { backend.toggleWifi(); } catch (caught) { error = caught; }
            assert(/hardware/.test(error?.message) && client.radioWrites.length === 2, 'Hardware blocking fails before a native radio write');
        } finally { backend.dispose(); }
    }

    async function nativeSettings() {
        for (const type of ['open', 'wep', 'psk', 'sae', 'owe']) {
            const { client, wifi, backend } = fixture();
            wifi.aps = [new AP('offline', type, 65, 1)];
            try {
                const selected = row(backend, 'offline', type);
                await backend.activate(selected.key, { password: type === 'wep' ? 'abcde' : ['psk', 'sae'].includes(type) ? 'offline-test' : null });
                const created = client.calls[0].profile;
                assert(created.verify() && created.verify_secrets(), `Real libnm ${type} settings verify`);
                assert(wifi.aps[0].connection_valid(created), `Native ${type} settings match the selected AP`);
                assert(client.calls[0].device === wifi && client.calls[0].specific === wifi.aps[0].get_path(), 'Explicit native device and AP');
                assert(!created.get_setting_wireless().get_bssid(), 'New profiles do not pin a BSSID');
                assert(backend.getIdentity(selected.key), 'Activation resolves on actual activated state');
            } finally { backend.dispose(); }
        }
        for (const type of ['enterprise', 'unknown']) {
            const { client, wifi, backend } = fixture(); wifi.aps = [new AP('advanced', type, 65, 1)];
            try {
                await rejects(backend.activate(row(backend, 'advanced').key), /./);
                assert(client.calls.length === 0, 'Unsupported inline setup does not invoke activation');
            } finally { backend.dispose(); }
        }
    }

    async function profilesAndSecrets() {
        const { client, wifi, backend } = fixture(); wifi.aps = [new AP('saved', 'psk', 80, 1)];
        const first = profile('saved', 'psk'); const second = profile('saved', 'psk');
        client.profiles = [first, second];
        try {
            let selected = row(backend, 'saved');
            await rejects(backend.activate(selected.key, { profileUuid: first.get_uuid(), password: 'offline-test' }), /Multiple saved/);
            await rejects(backend.activate(selected.key, { profileUuid: GLib.uuid_string_random() }), /no longer available/);
            assert(client.calls.length === 0, 'Ambiguous updates and stale UUIDs are rejected before native activation');
            await backend.activate(selected.key, { profileUuid: second.get_uuid() });
            assert(client.calls[0].profile === second, 'Exact explicitly selected saved profile');
            const stamp = backend.getIdentity(selected.key);
            equal(await backend.readSecret(selected.key, stamp), { password: 'offline-secret', authentication: 'WPA', hidden: true }, 'Native secret variant unpacking');
            const reading = backend.readSecret(selected.key, stamp); await delay();
            wifi.activeAP = new AP('saved', 'psk', 50, 7); wifi.aps.push(wifi.activeAP);
            await rejects(reading, /active connection changed/);
            assert(second.secretFinishes === 2, 'Finish called even when identity changed');
            wifi.activeAP = wifi.aps[0];
            const io = new Gio.Cancellable();
            const cancelled = backend.readSecret(selected.key, stamp, io); await delay(); io.cancel();
            await rejects(cancelled, /cancelled/); await delay(10);
            assert(second.secretFinishes === 3, 'Cancelled secret request is still finished');
            client.profiles = [first]; wifi.active = null; wifi.activeAP = null; wifi.state = NM.DeviceState.DISCONNECTED;
            selected = row(backend, 'saved');
            await backend.activate(selected.key, { password: 'updated-offline-password' });
            assert(first.updated.get_setting_wireless_security().get_psk() === 'updated-offline-password', 'Saved profile update uses native settings');
            assert(!first.get_setting_wireless_security().get_psk(), 'No mutation of live profile settings before remote update');
            let deleted = 0;
            for (const saved of [first, second]) {
                saved.delete_async = (io, callback) => {
                    client.profiles = client.profiles.filter(item => item !== saved); deleted++;
                    later(() => callback(saved, {}));
                };
                saved.delete_finish = () => true;
            }
            client.profiles = [first, second];
            await backend.forget(selected.key);
            assert(deleted === 2 && wifi.disconnects === 1, 'Forget disconnects and deletes every matching profile');
        } finally { backend.dispose(); }
    }

    async function savedPasswords() {
        for (const type of ['wep', 'psk', 'sae', 'open']) {
            const { client, wifi, backend } = fixture();
            const first = profile('saved password', type), second = profile('saved password', type);
            first.get_setting_connection().id = second.get_setting_connection().id = 'Duplicate saved ID';
            if (type === 'wep') second.get_setting_wireless_security().wep_tx_keyidx = 2;
            client.profiles = [first, second];
            try {
                const selected = row(backend, 'saved password');
                backend.details(selected.key); backend.rows; backend.getIdentity(selected.key);
                assert(first.secretCalls === 0 && second.secretCalls === 0, 'Saved snapshots never automatically authorize secrets');
                client.profiles.reverse();
                const uuid = second.get_uuid(), version = second.get_version_id();
                const result = await backend.readSavedSecret(selected.key, uuid);
                assert(second.get_uuid() === uuid && row(backend, 'saved password').profiles.some(profile => profile.uuid === uuid),
                    'Successful retrieval keeps the exact selected saved UUID');
                assert(second.get_version_id() === version + (type === 'open' ? 0 : 1),
                    'A successful native retrieval may bump the cached profile revision without invalidating the result');
                assert(result.password === (type === 'open' ? '' : type === 'wep' ? 'fghij' : 'offline-secret'),
                    'Native saved credential uses the exact UUID and selected WEP transmit key without diagnostic disclosure');
                equal([result.authentication, result.hidden], [type === 'open' ? 'nopass' : type === 'wep' ? 'WEP' : 'WPA', true],
                    'Saved WEP, PSK, SAE and open authentication metadata');
                assert(first.secretCalls === 0 && second.secretCalls === (type === 'open' ? 0 : 1),
                    'Duplicate display IDs and reordering cannot redirect native authorization; open needs no GetSecrets');
                for (const uuid of ['', null, 7, GLib.uuid_string_random()])
                    await rejects(backend.readSavedSecret(selected.key, uuid), /selected saved profile is no longer available/);
                assert(client.calls.length === 0 && client.radioWrites.length === 0 && wifi.disconnects === 0,
                    'Saved reads never activate, disconnect or toggle a radio');
                const snapshots = JSON.stringify([backend.rows, backend.details(selected.key)]);
                assert(!['offline-secret', 'abcde', 'fghij'].some(value => snapshots.includes(value)),
                    'Native rows and details stay credential-free after saved reads');
            } finally { backend.dispose(); }
        }
        for (const type of ['owe', 'enterprise', 'unknown']) {
            const { client, backend } = fixture();
            const remote = profile('unsupported saved password', type); client.profiles = [remote];
            try {
                await rejects(backend.readSavedSecret(row(backend, 'unsupported saved password').key, remote.get_uuid()), /not supported/);
                assert(remote.secretCalls === 0 && client.calls.length === 0, 'Unsupported saved security never asks libnm for a password');
            } finally { backend.dispose(); }
        }
    }

    async function savedPasswordContext() {
        {
            const { client, backend } = fixture();
            const remote = profile('saved revision', 'psk'), alternate = profile('saved revision', 'psk');
            client.profiles = [alternate, remote]; remote.holdSecrets = true;
            remote.get_setting_connection().id = alternate.get_setting_connection().id = 'Duplicate saved ID';
            try {
                const key = row(backend, 'saved revision').key, uuid = remote.get_uuid(), version = remote.get_version_id();
                const reading = backend.readSavedSecret(key, uuid); await delay();
                assert(remote.pendingSecrets.length === 1 && remote.get_version_id() === version,
                    'The exact saved-profile request waits for its fake native response before the cache revision changes');
                remote.pendingSecrets.shift().finish();
                const result = await reading;
                assert(remote.get_version_id() === version + 1 && remote.get_uuid() === uuid,
                    'Successful fake GetSecrets bumps VersionId but preserves the requested saved UUID');
                assert(result.password === 'offline-secret' && result.authentication === 'WPA' && result.hidden,
                    'Revision-only cache refresh accepts the request-local saved credential without diagnostic disclosure');
                assert(remote.secretCalls === 1 && remote.secretFinishes === 1 && alternate.secretCalls === 0,
                    'A revision bump never redirects authorization to a duplicate display ID');
                assert(client.calls.length === 0, 'Accepting the revision refresh never activates a connection');
            } finally { backend.dispose(); }
        }
        const changes = {
            'removed saved profile': ({ client }) => { client.profiles = []; },
            'same-UUID replacement': ({ client, remote }) => {
                const replacement = profile('saved context', 'psk');
                replacement.get_setting_connection().uuid = remote.get_uuid();
                client.profiles = [replacement];
            },
            'ambiguous duplicate UUID': ({ client, remote }) => {
                const duplicate = profile('saved context', 'psk');
                duplicate.get_setting_connection().uuid = remote.get_uuid(); client.profiles.push(duplicate);
            },

            'UUID change': ({ remote }) => { remote.get_setting_connection().uuid = GLib.uuid_string_random(); },
            'SSID change': ({ remote }) => { remote.get_setting_wireless().ssid = bytes('different saved SSID'); },
            'security change': ({ remote }) => { remote.get_setting_wireless_security().key_mgmt = 'sae'; },
            'hidden setting change': ({ remote }) => { remote.get_setting_wireless().hidden = false; },
            'profile device binding': ({ remote }) => { remote.get_setting_connection().interface_name = 'other-wlan'; },
            'device replacement': ({ client }) => { client.devices = [new Device('wlan-test')]; },
            'device removal': ({ client }) => { client.devices = []; },
            'device interface change': ({ wifi }) => { wifi.iface = 'renamed-wlan'; },
            'device type change': ({ wifi }) => { wifi.wireless = false; },
            'connection': ({ wifi, remote }) => {
                wifi.active = new Active(remote, 'saved-connected'); wifi.active.state = NM.ActiveConnectionState.ACTIVATED;
                wifi.state = NM.DeviceState.ACTIVATED;
            },
            'activation': ({ wifi, remote }) => { wifi.active = new Active(remote, 'saved-activating'); wifi.state = NM.DeviceState.PREPARE; },
            'software radio': ({ client }) => { client.enabled = false; },
            'hardware radio': ({ client }) => { client.hardware = false; },
        };
        for (const [change, apply] of Object.entries(changes)) {
            const context = fixture(), remote = profile('saved context', 'psk');
            Object.assign(context, { remote }); context.client.profiles = [remote]; remote.holdSecrets = true;
            const { backend } = context;
            try {
                const key = row(backend, 'saved context').key;
                const reading = backend.readSavedSecret(key, remote.get_uuid());
                await delay();
                assert(remote.pendingSecrets.length === 1, `Saved request waits for native authorization before ${change}`);
                apply(context);
                remote.pendingSecrets.shift().finish();
                await rejects(reading, /changed|no longer available/);
                assert(remote.secretFinishes === 1, `Native saved callback is finished after ${change}`);
                assert(context.client.calls.length === 0, `Rejecting ${change} never activates a profile`);
            } finally { backend.dispose(); }
        }
        {
            const { client, backend } = fixture();
            const remote = profile('WEP context', 'wep'); client.profiles = [remote]; remote.holdSecrets = true;
            try {
                const key = row(backend, 'WEP context').key;
                const reading = backend.readSavedSecret(key, remote.get_uuid()); await delay();
                remote.get_setting_wireless_security().wep_tx_keyidx = 2;
                remote.pendingSecrets.shift().finish();
                await rejects(reading, /changed/);
            } finally { backend.dispose(); }
        }
        for (const change of ['same-UUID replacement', 'SSID change', 'security change', 'hidden setting change']) {
            const context = fixture(), remote = profile('saved context', 'psk');
            Object.assign(context, { remote }); context.client.profiles = [remote];
            try {
                const key = row(context.backend, 'saved context').key;
                const reading = context.backend.readSavedSecret(key, remote.get_uuid());
                changes[change](context);
                await rejects(reading, /changed|no longer available/);
                assert(remote.secretCalls === 0, 'A real profile/context change before the first async turn cannot retarget authorization');
            } finally { context.backend.dispose(); }
        }
        for (const action of ['cancel', 'dispose']) {
            const { client, backend } = fixture();
            const remote = profile('cancel saved', 'psk'); client.profiles = [remote]; remote.holdSecrets = true;
            const io = new Gio.Cancellable();
            try {
                const reading = backend.readSavedSecret(row(backend, 'cancel saved').key, remote.get_uuid(), io); await delay();
                const reply = remote.pendingSecrets.shift();
                if (action === 'cancel') io.cancel(); else backend.dispose();
                await rejects(reading, /cancelled|closed/);
                assert(reply.io.is_cancelled(), 'Saved native authorization receives cancellation');
                reply.finish(); await delay();
                assert(remote.secretFinishes === 1, 'Late saved callbacks are still finished after cancellation/close');
            } finally { backend.dispose(); }
        }
        {
            const { client, backend } = fixture();
            const remote = profile('saved failure', 'psk'); client.profiles = [remote];
            try {
                const key = row(backend, 'saved failure').key;
                remote.secretReply = new GLib.Variant('a{sa{sv}}', { '802-11-wireless-security': {} });
                await rejects(backend.readSavedSecret(key, remote.get_uuid()), /saved password is not available/);
                remote.secretReply = null; remote.secretError = new Error('offline-secret');
                const error = await rejects(backend.readSavedSecret(key, remote.get_uuid()), /network operation failed/);
                assert(!error.message.includes('offline-secret'), 'Untrusted native secret errors are sanitized without printing them');
            } finally { backend.dispose(); }
        }
    }

    async function savedControllerContext() {
        for (const change of ['retrieval revision', 'same-UUID replacement', 'device replacement']) {
            const { client, backend } = fixture();
            const remote = profile('controller saved', 'psk'); client.profiles = [remote]; remote.holdSecrets = true;
            const controller = new NetworkController({ backend, createCancellable: () => new Gio.Cancellable() });
            try {
                controller.start(); controller.toggle('offline-monitor');
                const key = row(backend, 'controller saved').key;
                controller.info(key);
                const reading = controller.revealSecret(); await delay();
                const reply = remote.pendingSecrets.shift();
                const version = remote.get_version_id(), uuid = remote.get_uuid();
                if (change === 'same-UUID replacement') {
                    const replacement = profile('controller saved', 'psk');
                    replacement.get_setting_connection().uuid = remote.get_uuid(); client.profiles = [replacement];
                } else if (change === 'device replacement') client.devices = [new Device('wlan-test')];
                backend._sync();
                reply.finish(); await reading;
                if (change === 'retrieval revision') {
                    assert(remote.get_version_id() === version + 1 && controller.selectedProfileUuid === uuid
                        && controller.details.profileUuid === uuid, 'Controller accepts the retrieval revision for the same saved UUID');
                    assert(controller.page === 'info' && controller.secretRequested && controller.secret === 'offline-secret'
                        && !controller.secretLoading && !controller.error, 'Cache hydration reveals the requested fake password without leaving Info');
                } else assert(!controller.secret && !controller.secretLoading && remote.secretFinishes === 1,
                    `The real controller drops a native saved response after ${change}`);
                assert(remote.secretCalls === 1 && client.calls.length === 0, 'Native context changes do not reread secrets or activate');
            } finally { controller.dispose(); }
        }
        {
            const { client, backend } = fixture();
            const remote = profile('retry saved', 'psk'); client.profiles = [remote];
            const controller = new NetworkController({ backend, createCancellable: () => new Gio.Cancellable() });
            try {
                controller.start(); controller.toggle('offline-monitor');
                controller.info(row(backend, 'retry saved').key);
                const uuid = controller.selectedProfileUuid;
                remote.secretReply = new GLib.Variant('a{sa{sv}}', { '802-11-wireless-security': {} });
                await controller.revealSecret();
                assert(controller.page === 'info' && controller.selectedProfileUuid === uuid && controller.error
                    && !controller.secret && !controller.secretRequested && !controller.secretLoading,
                    'An accepted native saved-password failure keeps Info and restores the explicit Reveal action');
                assert(remote.secretCalls === 1, 'Native failure never automatically retries a secret request');
                remote.secretReply = null;
                await controller.revealSecret();
                assert(controller.page === 'info' && controller.selectedProfileUuid === uuid && !controller.error
                    && controller.secretRequested && controller.secret === 'offline-secret' && !controller.secretLoading,
                    'The next explicit Reveal directly retries the same saved UUID and displays its fake credential');
                assert(remote.secretCalls === 2 && client.calls.length === 0, 'Retry makes one native secret request, never an activation');
            } finally { controller.dispose(); }
        }
        {
            const { client, backend } = fixture();
            const remote = profile('displayed saved', 'psk'); client.profiles = [remote];
            const controller = new NetworkController({ backend, createCancellable: () => new Gio.Cancellable() });
            try {
                controller.start(); controller.toggle('offline-monitor'); controller.info(row(backend, 'displayed saved').key);
                await controller.revealSecret();
                assert(controller.secret === 'offline-secret', 'Native saved password is displayed only after explicit reveal');
                const replacement = profile('displayed saved', 'psk');
                replacement.get_setting_connection().uuid = remote.get_uuid(); client.profiles = [replacement]; backend._sync();
                assert(controller.page === 'list' && !controller.secret && !controller.selectedProfileUuid,
                    'Replacing the native profile object clears a displayed password even when UUID is reused');
                assert(replacement.secretCalls === 0, 'Replacement profile never inherits authorization');
            } finally { controller.dispose(); }
        }
    }

    async function nativeSettingsEdges() {
        for (const [type, password, keyType] of [
            ['wep', '0123456789', NM.WepKeyType.KEY],
            ['wep', 'offline-wep-passphrase', NM.WepKeyType.PASSPHRASE],
            ['sae', 'x', null], ['sae', 'x'.repeat(100), null],
        ]) {
            const { client, wifi, backend } = fixture(); wifi.aps = [new AP('edge', type, 70, 1)];
            try {
                await backend.activate(row(backend, 'edge').key, { password });
                const settings = client.calls[0].profile.get_setting_wireless_security();
                if (type === 'wep') assert(settings.get_wep_key_type() === keyType && settings.get_wep_key(0) === password, 'Native WEP key/passphrase settings');
                else assert(settings.get_psk() === password, 'SAE keeps native password rules instead of WPA-PSK length rules');
            } finally { backend.dispose(); }
        }
        const { client, wifi, backend } = fixture(); wifi.aps = [new AP('saved-wep', 'wep', 70, 1)];
        const remote = profile('saved-wep', 'wep'); remote.get_setting_wireless_security().wep_tx_keyidx = 2;
        client.profiles = [remote];
        try {
            await backend.activate(row(backend, 'saved-wep').key, { password: 'abcde' });
            const settings = remote.updated.get_setting_wireless_security();
            assert(settings.get_wep_tx_keyidx() === 2 && settings.get_wep_key(2) === 'abcde', 'Updating WEP preserves the selected native transmit key index');
        } finally { backend.dispose(); }
    }

    async function activationIdentityAndCancellation() {
        {
            const { client, wifi, backend } = fixture(); wifi.aps = [new AP('exact', 'psk', 70, 1)];
            const wanted = profile('exact', 'psk'); const other = profile('exact', 'psk'); client.profiles = [wanted, other];
            client.activate_connection_finish = result => { client.finishes++; result.active.profile = other; return result.active; };
            try {
                await rejects(backend.activate(row(backend, 'exact').key, { profileUuid: wanted.get_uuid() }), /profile changed/);
            } finally { backend.dispose(); }
        }
        for (const dispose of [false, true]) {
            const { client, wifi, backend } = fixture(); wifi.aps = [new AP('cancel-update', 'psk', 70, 1)];
            const remote = profile('cancel-update', 'psk'); client.profiles = [remote];
            let finishes = 0;
            remote.update2 = (settings, flags, args, io, callback) => later(() => callback(remote, {}), 12);
            remote.update2_finish = () => { finishes++; return new GLib.Variant('a{sv}', {}); };
            const io = new Gio.Cancellable();
            try {
                const updating = backend.activate(row(backend, 'cancel-update').key, { password: 'offline-test', cancellable: io });
                await delay();
                if (dispose) backend.dispose(); else io.cancel();
                await rejects(updating, /cancelled/); await delay(20);
                assert(finishes === 1 && client.calls.length === 0, 'Cancelled/disposed profile update finishes without a late activation');
            } finally { backend.dispose(); }
        }
        {
            const { client, wifi, backend } = fixture(); wifi.aps = [new AP('cancel-wait', 'open', 70, 1)]; client.automatic = false;
            try {
                const io = new Gio.Cancellable(); io.cancel();
                await rejects(backend.activate(row(backend, 'cancel-wait').key, { cancellable: io }), /cancelled/);
                assert(client.calls.length === 0, 'Already-cancelled request does not mutate native state');
                const waitingIo = new Gio.Cancellable();
                const waiting = backend.activate(row(backend, 'cancel-wait').key, { cancellable: waitingIo });
                await delay(10); waitingIo.cancel();
                await rejects(waiting, /cancelled/); await delay();
                const { active } = client.accepted;
                backend.dispose();
                assert(active.handlers.size === 0 && wifi.handlers.size === 0 && client.handlers.size === 0, 'Cancellation during activation releases temporary and lifecycle signals');
            } finally { backend.dispose(); }
        }
    }

    async function activationLifecycle() {
        const { client, wifi, backend } = fixture(); wifi.aps = [new AP('wait', 'open', 80, 1)]; client.automatic = false;
        try {
            const key = row(backend, 'wait').key;
            let resolved = false;
            const activation = backend.activate(key).then(() => { resolved = true; });
            await delay(10); assert(!resolved, 'Accepted request is not activation success');
            client.complete(); await activation; assert(resolved, 'Actual activated transition resolves');
            wifi.active = null; wifi.activeAP = null; wifi.state = NM.DeviceState.DISCONNECTED;
            const failed = backend.activate(key); await delay(10);
            client.accepted.active.state = NM.ActiveConnectionState.DEACTIVATED;
            client.accepted.active.reason = NM.ActiveConnectionStateReason.NO_SECRETS;
            client.accepted.active.emit('notify');
            await rejects(failed, /connection failed/, true);
            wifi.active = null; wifi.activeAP = null; wifi.state = NM.DeviceState.DISCONNECTED;
            const gone = backend.activate(key); await delay(10);
            client.devices = []; client.emit('device-removed', wifi);
            await rejects(gone, /no longer available/);
            client.devices = [wifi]; wifi.active = null; wifi.activeAP = null; wifi.state = NM.DeviceState.DISCONNECTED;
            client.replyDelay = 15;
            const io = new Gio.Cancellable();
            const cancelled = backend.activate(key, { cancellable: io }); await delay(); io.cancel();
            await rejects(cancelled, /cancelled/); await delay(25);
            assert(client.finishes === 4, 'All activation callbacks finish, including cancellation');
            const timeout = backend._run(null, 5, async operation => {
                await delay(12); backend._check(operation); throw new Error('must not reach');
            });
            await rejects(timeout, /timed out/);
            const secret = 'never-display-this-credential';
            const sanitized = await rejects(backend._run(null, 0, () => { throw new Error(secret); }), /network operation failed/);
            assert(!sanitized.message.includes(secret), 'Raw NM errors never escape');
            const nativeError = new GLib.Error(NM.secret_agent_error_quark(), NM.SecretAgentError.NOSECRETS, secret);
            const auth = await rejects(backend._run(null, 0, () => { throw nativeError; }), /network operation failed/, true);
            assert(!auth.message.includes(secret), 'Native NoSecrets error preserves authentication flag without its message');
        } finally { backend.dispose(); }
        await delay(15);
        assert(wifi.handlers.size === 0 && client.handlers.size === 0 && backend._operations.size === 0, 'Pending operation teardown');
    }

    function nativeSignalTeardown() {
        const { backend } = fixture();
        const device = new NM.DeviceWifi();
        backend._watch(new Map([[device, ['notify']]]));
        const [first] = backend._watches.get(device);
        assert(GObject.signal_handler_is_connected(device, first), 'Real NM.Device notify connected');
        backend._watch(new Map());
        assert(!GObject.signal_handler_is_connected(device, first), 'Removed NM.Device signals disconnected without a network operation');
        backend._watch(new Map([[device, ['notify']]]));
        const [second] = backend._watches.get(device);
        backend.dispose();
        assert(!GObject.signal_handler_is_connected(device, second), 'Disposed NM.Device signals disconnected without a network operation');
    }

    async function main() {
        nativeSignalTeardown();
        securityLabels();
        await activationCacheOrdering();
        await grouping(); await savedSignalStrength(); await offlineDetails();
        await automaticScanning(); await nativeRadioToggle();
        await nativeSettings(); await nativeSettingsEdges(); await profilesAndSecrets();
        await savedPasswords(); await savedPasswordContext(); await savedControllerContext();
        await activationIdentityAndCancellation(); await activationLifecycle();
        print('Native network tests passed: security labels, libnm settings, grouping, signal detection, safe offline Info, exact saved UUID passwords, retrieval revision refresh, devices, scans, profiles, secrets, exact ACTIVATED state, cancellation and teardown. No live NM mutations.');
    }
    let failed = false;
    try { await main(); }
    catch (error) { failed = true; logError(error); }
    System.exit(failed ? 1 : 0);
}

async function gjsRuntimeTests() {
    const Gio = (await import('gi://Gio')).default;
    const GLib = (await import('gi://GLib')).default;
    const System = (await import('system')).default;
    const { controllerRuntime, dismissOtherPanels } = await import('./backend.js');
    const root = GLib.dir_make_tmp('network-runtime-tests-XXXXXX');
    const environment = new Map(['DASHBOARD_PANEL', 'MEDIA_PANEL'].map(name => [name, GLib.getenv(name)]));
    let serial = 0;
    const paths = [];
    function check(condition, message) { if (!condition) throw new Error(message); }
    const pause = () => new Promise(resolve => GLib.timeout_add(GLib.PRIORITY_DEFAULT, 10, () => {
        resolve(); return GLib.SOURCE_REMOVE;
    }));
    function fixture() {
        const path = GLib.build_filenamev([root, `fake panel ${++serial} ; literal.cjs`]);
        const output = `${path}.json`;
        GLib.file_set_contents(path, '#!/usr/bin/env node\n' + `const fs = require('node:fs');
fs.writeFileSync(${JSON.stringify(output)}, JSON.stringify(process.argv.slice(2)));\n`);
        Gio.File.new_for_path(path).set_attribute_uint32('unix::mode', 0o700, Gio.FileQueryInfoFlags.NONE, null);
        paths.push(path, output);
        return { path, output };
    }
    async function closed(panel) {
        const deadline = GLib.get_monotonic_time() + 3e6;
        while (!GLib.file_test(panel.output, GLib.FileTest.EXISTS) && GLib.get_monotonic_time() < deadline) await pause();
        const [ok, bytes] = GLib.file_get_contents(panel.output);
        check(ok && JSON.stringify(JSON.parse(new TextDecoder().decode(bytes))) === '["close"]',
            'Each panel must receive only the literal close argument.');
    }
    async function main() {
        try {
            const runtime = controllerRuntime();
            const a = runtime.createCancellable(), b = runtime.createCancellable();
            check(a instanceof Gio.Cancellable && b instanceof Gio.Cancellable && a !== b,
                'Controller requests must receive distinct native cancellables.');
            a.cancel();
            check(a.is_cancelled() && !b.is_cancelled(), 'Native cancellation must remain request-local.');
            print('PASS controllerRuntime exposes real independent Gio cancellables');
            const dashboard = fixture(), media = fixture();
            GLib.setenv('DASHBOARD_PANEL', dashboard.path, true);
            GLib.setenv('MEDIA_PANEL', media.path, true);
            dismissOtherPanels();
            await closed(dashboard); await closed(media);
            print('PASS dismissOtherPanels closes exactly the fake dashboard and media launchers');
            const nextMedia = fixture();
            GLib.setenv('DASHBOARD_PANEL', GLib.build_filenamev([root, 'missing-dashboard']), true);
            GLib.setenv('MEDIA_PANEL', nextMedia.path, true);
            dismissOtherPanels();
            await closed(nextMedia);
            print('PASS a failed dashboard launch does not prevent closing media');
        } finally {
            for (const [name, value] of environment) {
                if (value === null) GLib.unsetenv(name); else GLib.setenv(name, value, true);
            }
            for (const path of paths) if (GLib.file_test(path, GLib.FileTest.EXISTS)) Gio.File.new_for_path(path).delete(null);
            Gio.File.new_for_path(root).delete(null);
        }
    }
    let exitCode = 0;
    try { await main(); }
    catch (error) { printerr(error.stack || String(error)); exitCode = 1; }
    System.exit(exitCode);
}

async function gjsUITests() {
    // Run through node test.js --ui: real frontend/backend/logic imports,
    // fake actions/credentials, authenticated private Xvfb and isolated D-Bus.
    const { default: GLib } = await import('gi://GLib');
    const { default: Gio } = await import('gi://Gio');
    const { default: Gtk } = await import('gi://Gtk?version=4.0');
    const { default: Gdk } = await import('gi://Gdk?version=4.0');

    const { default: Pango } = await import('gi://Pango');
    const { default: System } = await import('system');

    if (GLib.getenv('NETWORK_UI_ISOLATED') !== '1'
        || GLib.getenv('NETWORK_PANEL_IMPORT_ONLY') !== '1'
        || GLib.getenv('GDK_BACKEND') !== 'x11'
        || GLib.getenv('DBUS_SYSTEM_BUS_ADDRESS') !== GLib.getenv('DBUS_SESSION_BUS_ADDRESS'))
        throw new Error('Use node test.js --ui, with a private Xvfb and both bus addresses isolated.');

    const { NetworkPanel, NetworkApplication, loadTheme, monitorFor } = await import(
        GLib.filename_to_uri(GLib.getenv('NETWORK_PANEL_CONFIG'), null));
    const { NetworkController, isUndetectedSaved, signalLabel } = await import('./logic.js');
    // GTK has no introspectable accessible-property getter. Observe updates while
    // forwarding them to the real GTK implementation, including later rerenders.
    const accessibleProperties = new WeakMap();
    const updateAccessible = Gtk.Button.prototype.update_property;
    const updateDropdownAccessible = Gtk.DropDown.prototype.update_property;
    function trackAccessible(properties, values) {
        const state = accessibleProperties.get(this) || new Map();
        properties.forEach((property, index) => state.set(property, values[index]));
        accessibleProperties.set(this, state);
        return updateAccessible.call(this, properties, values);
    }
    let checks = 0;
    function assert(condition, message) {
        if (!condition) throw new Error(message);
        checks++;
    }
    function equal(actual, expected, message) { assert(JSON.stringify(actual) === JSON.stringify(expected), message); }
    function wait(milliseconds = 80) {
        return new Promise(resolve => GLib.timeout_add(GLib.PRIORITY_DEFAULT, milliseconds,
            () => { resolve(); return GLib.SOURCE_REMOVE; }));
    }
    async function waitFor(predicate, message, milliseconds = 1500) {
        const deadline = GLib.get_monotonic_time() + milliseconds * 1000;
        while (!predicate() && GLib.get_monotonic_time() < deadline) await wait(20);
        assert(predicate(), message);
    }
    function owner(name) {
        return Gio.DBus.session.call_sync('org.freedesktop.DBus', '/org/freedesktop/DBus', 'org.freedesktop.DBus',
            'NameHasOwner', new GLib.Variant('(s)', [name]), new GLib.VariantType('(b)'),
            Gio.DBusCallFlags.NONE, 1000, null).deep_unpack()[0];
    }
    function eventController(widget, type) {
        const model = widget.observe_controllers();
        for (let index = 0; index < model.get_n_items(); index++) {
            const controller = model.get_item(index);
            if (controller instanceof type) return controller;
        }
        throw new Error(`Missing event controller ${type.name}`);
    }
    function* widgets(widget) {
        yield widget;
        for (let child = widget.get_first_child(); child; child = child.get_next_sibling()) yield* widgets(child);
    }
    function assertTooltipFree(view, phase = 'rerender') {
        // Include hidden pages, GTK-owned descendants/popovers and detached shared
        // details: none may retain text, markup or custom query-tooltip activation.
        const tree = new Set([view.window, view.root, view.infoContent].flatMap(root => [...widgets(root)]));
        const violations = [...tree].filter(widget => widget.has_tooltip || widget.tooltip_text || widget.tooltip_markup);
        assert(violations.length === 0,
            `${phase}: entire widget tree is tooltip-free (${violations.map(widget =>
                `${widget.constructor.name}/${widget.icon_name || ''}: ${JSON.stringify([widget.has_tooltip, widget.tooltip_text,
                    widget.tooltip_markup])} in ${widget.get_parent()?.constructor.name}`).join(', ')})`);
        equal(accessibleProperties.get(view.savedTab)?.get(Gtk.AccessibleProperty.LABEL),
            `Saved networks (${view.controller.rows.filter(isUndetectedSaved).length})`,
            `${phase}: Saved keeps its live count in an explicit accessible label without hover text`);
        equal(accessibleProperties.get(view.savedTab)?.get(Gtk.AccessibleProperty.DESCRIPTION),
            'Saved Wi-Fi networks that are not currently detected', `${phase}: Saved keeps its accessible description`);
        equal(accessibleProperties.get(view.infoProfile)?.get(Gtk.AccessibleProperty.LABEL), 'Saved profile',
            `${phase}: the Info profile chooser has an explicit accessible label`);
        equal(accessibleProperties.get(view.infoProfile)?.get(Gtk.AccessibleProperty.DESCRIPTION),
            'Saved profile whose password will be revealed', `${phase}: the Info chooser keeps its accessible description`);
        for (const row of view.rows.values()) {
            equal(accessibleProperties.get(row.remove)?.get(Gtk.AccessibleProperty.LABEL), `Forget saved network ${row.data.name}`,
                `${phase}: Remove keeps its descriptive accessible action`);
            equal(accessibleProperties.get(row.info)?.get(Gtk.AccessibleProperty.LABEL),
                `${row.details.reveal_child ? 'Collapse' : 'Expand'} details for ${row.data.name}`,
                `${phase}: the Info accessible action follows expansion and the current SSID`);
            equal(accessibleProperties.get(row.profile)?.get(Gtk.AccessibleProperty.LABEL), `Saved profile for ${row.data.name}`,
                `${phase}: the connection profile chooser is labelled independently of tooltips`);
        }
    }
    async function assertDropdownTooltipFree(view, dropdown, phase) {
        await wait();
        const popover = [...widgets(dropdown)].find(widget => widget instanceof Gtk.Popover);
        assert(popover, `${phase}: the real dropdown owns a popup`);
        await waitFor(() => dropdown.get_mapped(), `${phase}: the dropdown maps after the details slide opens`);
        popover.popup();
        try {
            await wait();
            assert(popover.get_mapped() && [...widgets(popover)].some(widget => widget instanceof Gtk.Label && widget.label),
                `${phase}: the opened dropdown contains real GTK item descendants`);
            assertTooltipFree(view, `${phase}, dropdown open`);
        } finally { popover.popdown(); }
        await wait();
        assertTooltipFree(view, `${phase}, dropdown closed`);
    }
    function captureDrawing(draw, drawing) {
        const strokes = [], fills = [];
        let point = null, origin = null, path = [], color = [], width = 1;
        const lineTo = (x, y) => { if (point) path.push([point, [x, y]]); point = [x, y]; };
        const paint = target => { target.push({ path: [...path], color: [...color], width }); path = []; };
        const preserve = target => { const saved = [...path]; paint(target); path = saved; };
        const cairo = new Proxy({
            moveTo(x, y) { point = origin = [x, y]; }, lineTo,
            closePath() { if (origin) lineTo(...origin); },
            rectangle(x, y, w, h) {
                point = origin = [x, y]; lineTo(x + w, y); lineTo(x + w, y + h);
                lineTo(x, y + h); lineTo(x, y);
            },
            setSourceRGBA(...rgba) { color = rgba; },
            setSourceRGB(...rgb) { color = [...rgb, 1]; },
            setLineWidth(value) { width = value; },
            stroke() { paint(strokes); }, strokePreserve() { preserve(strokes); },
            fill() { paint(fills); }, fillPreserve() { preserve(fills); },
            newPath() { point = origin = null; path = []; },
        }, { get: (target, name) => target[name] || (() => {}) });
        draw(drawing, cairo, drawing.get_width(), drawing.get_height());
        return { strokes, fills };
    }
    function matchesColor(rgba, text, alpha = null) {
        const color = new Gdk.RGBA();
        assert(color.parse(text), `Test palette contains a valid color: ${text}`);
        return rgba.length === 4 && ['red', 'green', 'blue'].every((channel, index) => Math.abs(rgba[index] - color[channel]) < 0.01)
            && (alpha === null || Math.abs(rgba[3] - alpha) < 0.01);
    }
    function assertEye(view, requested = false) {
        const text = requested ? 'Hide password' : 'Reveal password';
        assert(view.reveal instanceof Gtk.Button && view.reveal.get_child() instanceof Gtk.Image,
            'The password control is a real icon-only GTK button');
        equal(view.reveal.get_child().icon_name, requested ? 'view-conceal-symbolic' : 'view-reveal-symbolic',
            'The eye icon follows requested/shown state, including authorization');
        assertTooltipFree(view, `${text} eye`);
        equal(accessibleProperties.get(view.reveal)?.get(Gtk.AccessibleProperty.LABEL), text,
            'The icon-only eye has an explicit accessible action label');
        for (const control of [view.reveal, ...[...view.rows.values()].map(row => row.info)]) {
            const metadata = [control.tooltip_text, control.tooltip_markup,
                ...Array.from(accessibleProperties.get(control)?.values() || [])];
            assert(!metadata.some(value => String(value || '').includes('literal secret')
                || String(value || '').includes('fake-password-')), 'Controls never expose fake credentials through tooltips or accessibility');
        }
    }
    function assertMasked(view, controller, message = 'Hidden credentials use a constant non-selectable mask') {
        assert(view.secret.label === '*******' && !view.secret.selectable && !controller.secret, message);
    }
    function assertConnectionAction(row, data) {
        assert(row.connect instanceof Gtk.Button && row.connect.get_child() instanceof Gtk.Image,
            `${data.key}: connection action is an icon-only GTK button`);
        equal(row.connect.get_child().icon_name, data.connected ? 'network-offline-symbolic'
            : data.wireless ? 'network-wireless-symbolic' : 'network-wired-symbolic',
        `${data.key}: connection icon follows the current action and network type`);
        equal(accessibleProperties.get(row.connect)?.get(Gtk.AccessibleProperty.LABEL),
            `${data.connected ? 'Disconnect' : 'Connect'} ${data.name}`,
            `${data.key}: connection action has an explicit accessible label with the literal network name`);
        assert(!row.connect.has_tooltip && !row.connect.tooltip_text && !row.connect.tooltip_markup,
            `${data.key}: connection action has no tooltip`);
    }
    function assertRowPresentation(view, data) {
        const row = view.rows.get(data.key), saved = isUndetectedSaved(data);
        const title = saved ? `Wi-Fi - ${data.security || 'Open'} - ${data.name}`
            : `${data.kind === 'WIRED' ? 'Wired' : 'Wi-Fi'} · ${data.name}`;
        equal(row.name.label, title, `${data.key}: only undetected Saved titles put security before the literal SSID`);
        assert(!row.name.use_markup && !row.name.use_underline && row.name.get_text() === title,
            `${data.key}: titles preserve literal markup, ampersands and underscores`);
        equal([row.name.wrap, row.name.ellipsize], [!saved, saved ? Pango.EllipsizeMode.END : Pango.EllipsizeMode.NONE],
            `${data.key}: Saved uses one ellipsized line; Nearby restores the original wrapped title`);
        const colors = row.name.get_attributes().to_string();
        if (data.wireless) {
            assert(colors.includes('0 5 foreground #ffffffffffff'), 'Wi-Fi prefix is white without changing literal SSID text');
            if (saved) {
                const end = 8 + new TextEncoder().encode(data.security || 'Open').length;
                assert(colors.includes(`8 ${end} foreground #ffffffffffff`), 'Saved security text is white');
            }
            const color = row.marker.get_style_context().get_color();
            assert(['red', 'green', 'blue', 'alpha'].every(channel => Math.abs(color[channel] - 1) < 0.01), 'Wi-Fi icon is white');
        } else assert(!colors, 'Wired title styling is unchanged');
        assert(row.status instanceof Gtk.Label && row.status.has_css_class('network-status')
            && row.status.get_parent() === row.heading && row.name.get_next_sibling() === row.status
            && row.status.get_next_sibling() === row.connect,
        `${data.key}: a dedicated status label sits immediately after the title, before actions`);
        equal(row.status.visible, Boolean(data.wireless && !saved), `${data.key}: only Nearby Wi-Fi shows heading status`);
        if (data.wireless && !saved) {
            equal(row.status.label, [data.security || 'Open', Number.isFinite(data.strength) ? signalLabel(data.strength) : '']
                .filter(Boolean).join(' · '), `${data.key}: heading shows security and only a finite signal percentage`);
            assert(!row.status.use_markup && row.status.get_text() === row.status.label,
                `${data.key}: heading security is literal text`);
            const color = row.status.get_style_context().get_color();
            assert(['red', 'green', 'blue', 'alpha'].every(channel => Math.abs(color[channel] - 1) < 0.01),
                `${data.key}: heading security and signal are opaque white`);
        }
        assertConnectionAction(row, data);
        equal(row.connect.visible, !saved, `${data.key}: Saved has no connection action`);
        equal(row.metadata.visible, !saved && Boolean(row.metadata.label),
            `${data.key}: Saved rows and empty status lines stay hidden`);
        assert(row.info.get_parent() === row.secondary && row.metadata.get_parent() === row.secondary
            && row.secondary.get_parent() === row.widget && row.heading.get_next_sibling() === row.secondary,
            `${data.key}: details chevron shares the second line with connection status`);
        equal(row.secondary.visible, row.info.visible || row.metadata.visible,
            `${data.key}: empty second lines are hidden`);
        assert(!row.name.has_tooltip && !row.name.tooltip_text && !row.name.tooltip_markup,
            `${data.key}: even a long or ellipsized title has no hover substitute`);
        if (!saved) {
            equal(row.metadata.label, [data.connected ? 'Connected' : data.wireless ? '' : data.known ? 'Saved' : 'Not saved',
                data.wireless ? '' : data.interface, data.wireless ? '' : data.security,
                data.changing ? 'Working…' : ''].filter(Boolean).join(' · '),
            `${data.key}: Nearby Wi-Fi metadata keeps only connection/working state; wired metadata is unchanged`);
        }
        assert(row.widget.has_css_class('network-row'), `${data.key}: full-row styling belongs on the network widget`);
        equal(row.widget.has_css_class('saved-network-row'), saved, `${data.key}: Saved styling follows tab migration`);
        equal(row.widget.has_css_class('connected'), Boolean(data.connected), `${data.key}: full-row connected styling follows live state`);
        equal(row.widget.has_css_class('expanded'), row.details.reveal_child, `${data.key}: full-row expansion styling matches the revealer`);
        if (row.marker) {
            assert(row.marker instanceof Gtk.Image
                && row.marker.icon_name === (data.wireless ? 'network-wireless-symbolic' : 'network-wired-symbolic')
                && row.marker.get_parent() === row.heading && row.marker.get_next_sibling() === row.name
                && !row.marker.can_target && !row.marker.focusable,
            'The decorative network-type icon sits immediately before the name');
        }
    }
    function assertInline(view, controller, key) {
        equal([controller.page, view.pages.visible_child_name], ['info', 'list'], 'Info remains controller state, not a separate UI page');
        assert(!view.pages.get_child_by_name('info'),
            'Inline Info has no standalone stack child or Back/navigation');
        const row = view.rows.get(key);
        assert(row.details instanceof Gtk.Revealer && row.details.get_parent() === row.widget,
            'Details are attached directly to their network row');
        assert(row.details.get_child() === view.infoContent && view.infoContent.get_parent() === row.details,
            'The single shared Info content is reparented into the selected row');
        equal(view.infoContent.get_style_context().get_border().left, 0,
            'Expanded details have no extra thin left divider beside the inset accent');
        equal([...view.rows.values()].filter(item => item.details.reveal_child).map(item => item.key), [key],
            'Only the selected row is expanded');
        equal([...view.rows.values()].filter(item => item.widget.has_css_class('expanded')).map(item => item.key), [key],
            'Only the selected full row receives the red expanded outline, not just its Info chevron');
        assert(view.summary.label.split('\n').every(line => line.startsWith('• ')), 'Network summary entries are bullets');
        for (const [key, widget] of view.detailLabels) {
            assert(widget.label.startsWith('• '), `${key} detail is a bullet`);
            const color = widget.get_style_context().get_color();
            assert(['red', 'green', 'blue', 'alpha'].every(channel => Math.abs(color[channel] - 1) < 0.01),
                `${key} detail text is white`);
        }
        assert(view.profileGroup.get_first_child().label === 'Profiles'
            && view.addressGroup.get_first_child().label === 'IP addresses'
            && view.linkHeading.label === (row.data.wireless ? 'Wireless link' : 'Wired link'),
            'Details are grouped under profile, address and link headings');
        equal([view.addressGroup.visible, view.linkGroup.visible], [Boolean(row.data.connected), Boolean(row.data.connected)],
            'Disconnected profiles do not show empty live-information groups');
        for (const data of controller.rows) assertRowPresentation(view, data);
    }
    function network(key, properties = {}) {
        return { key, name: '<b>Literal SSID</b> ' + 'Long network name '.repeat(8), ssid: 'test', interface: 'wlan0',
            kind: 'WI-FI', wireless: true, connected: true, changing: false, known: true,
            strength: 87, security: 'WPA2', securityType: 'psk',
            profiles: [{ id: '<i>Duplicate profile</i>', uuid: 'uuid-a' }, { id: '<i>Duplicate profile</i>', uuid: 'uuid-b' }],
            ...properties };
    }
    class FakeController {
        constructor() {
            Object.assign(this, { rows: [network('wifi'), network('wired', { kind: 'WIRED', wireless: false, profiles: [] })],
                available: true, wifiAvailable: true, wifiEnabled: true, wifiHardwareEnabled: true,
                opened: false, monitor: '', page: 'list', error: '', credentialKey: '', credentialRow: null,
                selectedRow: null, selectedProfileUuid: '', forgetKey: '', pendingKey: '', details: {}, secret: '',
                secretLoading: false, secretRequested: false, calls: [] });
        }
        emit() { this.onChange?.(); }
        record(...args) { this.calls.push(args); }
        toggle(monitor = '') { this.record('toggle', monitor); this.opened = !this.opened || monitor !== this.monitor; this.monitor = monitor; this.emit(); }
        reset() {
            Object.assign(this, { page: 'list', selectedRow: null, selectedKey: '', selectedProfileUuid: '', details: {},
                credentialKey: '', credentialRow: null, forgetKey: '', pendingKey: '', error: '',
                secret: '', secretRequested: false, secretLoading: false });
        }
        close() { this.record('close'); this.opened = false; this.reset(); this.emit(); }
        back() { this.record('back'); this.reset(); this.emit(); }
        toggleWifi() {
            this.record('wifi');
            this.error = this.toggleError?.message || '';
            if (!this.error) this.wifiEnabled = !this.wifiEnabled;
            this.emit();
        }
        connectNetwork(...args) { this.record('connect', ...args); }
        disconnectNetwork(key) { this.record('disconnect', key); }
        submitPassword(value) { this.record('password', value); this.cancelCredentials(); }
        cancelCredentials() { this.record('cancelCredentials'); this.credentialKey = ''; this.credentialRow = null; this.emit(); }
        requestForget(key) { this.record('forget', key); this.reset(); this.forgetKey = key; this.emit(); }
        confirmForget() { this.record('confirmForget'); this.forgetKey = ''; this.emit(); }
        cancelForget() { this.record('cancelForget'); this.forgetKey = ''; this.emit(); }
        info(key) {
            const row = this.rows.find(row => row.key === key);
            if (!row || (!row.known && !row.connected)) return;
            this.record('info', key); this.reset(); this.page = 'info'; this.selectedRow = row; this.selectedKey = key;
            this.selectedProfileUuid = row.connected ? '' : row.profiles[0]?.uuid || '';
            this.details = { identity: row.connected ? { profileUuid: row.profiles[0]?.uuid || '' } : null,
                ssid: row.ssid, interface: row.interface,
                profileId: row.profiles[0]?.id || '', profileUuid: row.profiles[0]?.uuid || '' };
            this.emit();
        }
        selectSavedProfile(uuid) {
            const row = this.rows.find(row => row.key === this.selectedKey);
            const profile = row?.profiles.find(profile => profile.uuid === uuid);
            if (this.page !== 'info' || !this.opened || !row?.wireless || row.connected || row.changing
                || !profile || uuid === this.selectedProfileUuid) return;
            this.record('selectSavedProfile', uuid);
            this.selectedProfileUuid = uuid;
            this.secret = ''; this.secretRequested = false; this.secretLoading = false;
            this.details = { ...this.details, profileId: profile.id, profileUuid: profile.uuid };
            this.emit();
        }
        revealSecret() {
            const row = this.selectedRow;
            if (this.page !== 'info' || !row?.wireless || !(row.connected || row.known)
                || row.changing || this.secretLoading) return;
            this.record('reveal'); this.error = ''; this.secretRequested = true; this.secretLoading = Boolean(this.holdReveal);
            this.secret = this.secretLoading ? '' : '<b>literal secret</b>'; this.emit();
        }
        failReveal(message) {
            this.error = message; this.secretRequested = false; this.secretLoading = false; this.secret = ''; this.emit();
        }
        hideSecret() { this.record('hide'); this.secretRequested = false; this.secretLoading = false; this.secret = ''; this.emit(); }
        status() { return '{"text":"offline","class":"wifi"}'; }
    }

    let panel, host;
    function visibleNetworkOrder(container = panel.networkRows) {
        const keys = new Map([...panel.rows.values()].map(row => [row.widget, row.key]));
        const order = [];
        for (let child = container.get_first_child(); child; child = child.get_next_sibling()) {
            if (child.visible) order.push(child === panel.networkSeparator ? 'divider' : keys.get(child) || 'unexpected widget');
        }
        return order;
    }
    function assertNetworkOrder(expected, message) {
        equal(visibleNetworkOrder(), expected, message);
        const dividers = [...widgets(panel.networkRows)].filter(widget => widget instanceof Gtk.Separator
            && widget.has_css_class('network-divider'));
        assert(dividers.length === 1 && dividers[0] === panel.networkSeparator, 'The list owns one group divider, not per-row separators');
        equal(panel.networkSeparator.visible, expected.includes('divider'), 'The divider is visible only between two nonempty groups');
    }
    function assertListPartition(nearby, saved, message) {
        assertNetworkOrder(nearby, message);
        equal(visibleNetworkOrder(panel.savedRows), saved, 'Saved contains only undetected saved rows, in native order');
        const nearbyKeys = nearby.filter(key => key !== 'divider');
        equal(panel.rows.size, nearbyKeys.length + saved.length, 'Shared row-map count equals both tab counts, without duplicates');
        for (const [key, row] of panel.rows) {
            assert(row.widget.get_parent() === (saved.includes(key) ? panel.savedRows : panel.networkRows),
                `${key} is parented to exactly its own list`);
            assertRowPresentation(panel, row.data);
        }
        equal(panel.empty.visible, nearbyKeys.length === 0, 'Nearby empty state uses only Nearby count');
        equal(panel.savedEmpty.visible, saved.length === 0, 'Saved empty state uses only Saved count');
        equal(panel.nearbyTab.get_child().label, 'NEARBY', 'Nearby tab uses the uppercase sibling-panel label');
        equal(panel.savedTab.get_child().label, `SAVED (${saved.length})`, 'Saved tab shows its uppercase live count');
        for (const [name, tab] of [['nearby', panel.nearbyTab], ['saved', panel.savedTab]])
            equal(tab.has_css_class('active'), panel.listTab === name, 'Only the selected tab has the active style');
    }
    async function inlineControllerTests(app, palette) {
        // This is the real controller driving the real GTK tree. Only the backend
        // is fake; cancelled promises intentionally deliver late secret replies.
        const freshRows = () => [network('active'), network('second'),
            network('saved', { connected: false, strength: 50 }),
            network('detected', { connected: false, strength: 50 })];
        const backend = {
            rows: freshRows(), available: true, wifiAvailable: true, wifiEnabled: true, wifiHardwareEnabled: true,
            requests: [], mutations: [],
            start() {}, dispose() {}, setOpened() {},
            notify() { this.onChange?.(); },
            getIdentity(key) {
                const row = this.rows.find(row => row.key === key);
                return row?.connected ? { interface: row.interface, ssid: row.ssid, profileUuid: row.profiles[0].uuid,
                    bssid: '02:00:00:00:00:01', activePath: `/fake/active/${key}` } : null;
            },
            details(key) {
                const row = this.rows.find(row => row.key === key);
                return { identity: this.getIdentity(key), ssid: row.ssid, interface: row.interface,
                    profileId: row.profiles[0].id, profileUuid: row.profiles[0].uuid };
            },
            readSecret(key, identity, io) { return this.defer({ key, identity, io }); },
            readSavedSecret(key, profileUuid, io) { return this.defer({ key, profileUuid, io }); },
            defer(context) {
                const request = { ...context };
                request.promise = new Promise((resolve, reject) => { request.resolve = resolve; request.reject = reject; });
                this.requests.push(request);
                return request.promise;
            },
            activate(key) { this.mutations.push(['connect', key]); return Promise.resolve(); },
            disconnect(key) { this.mutations.push(['disconnect', key]); return Promise.resolve(); },
            forget(key) { this.mutations.push(['forget', key]); return Promise.resolve(); },
        };
        let view, window;
        const controller = new NetworkController({ backend, onChange: () => {
            if (view) { view.render(); assertTooltipFree(view, 'Real-controller rerender'); }
        },
            createCancellable: () => new Gio.Cancellable() });
        const complete = async (request, password = 'fake-password-current') => {
            request.resolve({ password, authentication: 'WPA', hidden: false });
            await wait(10);
        };
        const open = key => {
            const count = backend.requests.length;
            view.rows.get(key).info.emit('clicked');
            assertInline(view, controller, key);
            equal(backend.requests.length, count, 'Opening inline Info never reads credentials');
            assertMasked(view, controller);
        };
        const reveal = () => {
            const count = backend.requests.length;
            view.reveal.emit('clicked');
            equal(backend.requests.length, count + 1, 'One explicit reveal starts exactly one fake backend request');
            assert(view.reveal.sensitive && controller.secretLoading && view.secretLoading.visible,
                'Pending authorization leaves the eye usable');
            assertMasked(view, controller);
            assertEye(view, true);
            return backend.requests.at(-1);
        };
        const assertClosed = row => {
            assert(controller.page === 'list' && !controller.selectedKey && !controller.secretRequested
                && !controller.secretLoading && !row.details.reveal_child, 'Closing details clears controller selection and authorization synchronously');
            assert(!row.widget.has_css_class('expanded')
                && ![...view.rows.values()].some(item => item.widget.has_css_class('expanded')),
            'Collapse/reset immediately removes the full-row highlight, including detached rows');
            assertMasked(view, controller, 'The shared plaintext label is masked before a collapse animation can finish');
            assert(![...widgets(view.root)].some(widget => widget instanceof Gtk.Label
                && /fake-password-/.test(widget.label)), 'No row keeps a plaintext credential label after closing');
        };
        const reset = () => {
            controller.back();
            backend.rows = freshRows(); backend.wifiEnabled = backend.wifiHardwareEnabled = true; backend.notify();
            view.selectListTab('nearby');
            if (!controller.opened) view.toggle();
        };
        try {
            controller.start();
            view = new NetworkPanel({ app, controller, palette, dismiss: () => {} });
            assertTooltipFree(view, 'Real-controller initial tree');
            controller.error = 'Network operation failed'; controller.emit();
            assert(view.errorRow.visible && view.dismissError.visible && view.error.visible, 'Errors show a dismiss button beside the message');
            view.dismissError.emit('clicked');
            assert(!controller.error && !view.errorRow.visible, 'Clicking the dismiss button clears the error and its row');
            controller.error = 'Later failure'; controller.emit();
            assert(view.errorRow.visible && view.error.label === 'Later failure', 'Later errors remain visible after dismissal');
            view.dismissError.emit('clicked');
            view.toggle();
            view.window.set_child(null);
            window = new Gtk.Window({ child: view.root, decorated: false, default_width: 1280, default_height: 800,
                css_classes: ['network-panel'] });
            window.present();
            await wait();
            const shared = [view.infoContent, view.summary, view.savedProfiles, view.infoProfile,
                view.detailLabels, view.reveal, view.secret, view.secretLoading];
            open('active');
            const row = view.rows.get('active'), second = view.rows.get('second');
            await complete(reveal());
            assert(view.secret.label === 'fake-password-current' && view.secret.selectable, 'Only a successful explicit reply becomes selectable plaintext');
            assertEye(view, true);
            await waitFor(() => row.details.child_revealed, 'The mapped real-GTK slide reaches its expanded state');
            const collapseStates = [];
            row.details.connect('notify::reveal-child', () => {
                if (!row.details.reveal_child) collapseStates.push([view.secret.label, view.secret.selectable, controller.secret]);
            });
            view.password.set_text('fake-input-before-other-info');
            second.info.emit('clicked');
            assertInline(view, controller, 'second');
            assertMasked(view, controller);
            assert(!view.password.text && row.details.get_child() === null, 'Selecting another Info clears input and detaches the shared details from the old row');
            equal(collapseStates.at(-1), ['*******', false, ''], 'The closing revealer observes masking before its collapse starts');
            const nextShared = [view.infoContent, view.summary, view.savedProfiles, view.infoProfile,
                view.detailLabels, view.reveal, view.secret, view.secretLoading];
            assert(shared.every((field, index) => field === nextShared[index]), 'Selection changes reuse every shared testing field, not per-row secret caches');
            await complete(reveal(), 'fake-password-second');
            view.password.set_text('fake-input-before-collapse');
            second.info.emit('clicked');
            assertClosed(second);
            assert(!view.password.text, 'Toggling the selected Info closed clears password input');
            await wait(second.details.transition_duration + 30);
            assert(!second.details.child_revealed, 'The real-GTK slide reaches its collapsed state without plaintext');
            open('active');
            let old = reveal();
            view.reveal.emit('clicked');
            assert(old.io.is_cancelled() && !controller.secretLoading && !controller.secretRequested,
                'Clicking the eye during authorization cancels the real Gio.Cancellable');
            assertMasked(view, controller);
            assertEye(view);
            const current = reveal();
            await complete(old, 'fake-password-cancelled');
            assert(controller.secretLoading && controller.secretRequested && view.secretLoading.visible,
                'A cancelled late reply cannot clear a newer read or loading state');
            assertMasked(view, controller);
            assertEye(view, true);
            await complete(current);
            view.reveal.emit('clicked');
            const failed = reveal();
            failed.reject(new Error('Fake authorization denied.')); await wait(10);
            assert(controller.page === 'info' && !controller.secretRequested && !view.secretLoading.visible
                && view.error.label === 'Fake authorization denied.', 'Authorization failure preserves inline selection and a safe retryable error');
            assertMasked(view, controller);
            assertEye(view);
            await complete(reveal());
            assert(view.secret.label === 'fake-password-current' && !view.error.visible, 'A fresh explicit retry succeeds after failure');

            reset(); open('saved');
            equal(controller.selectedProfileUuid, 'uuid-a', 'Saved Info starts with the exact snapshot profile UUID');
            old = reveal();
            view.infoProfile.selected = 1;
            assert(old.io.is_cancelled() && controller.selectedProfileUuid === 'uuid-b', 'Profile selection cancels the old UUID read');
            assertMasked(view, controller);
            const profileRead = reveal();
            equal(profileRead.profileUuid, 'uuid-b', 'Duplicate display names still request the selected UUID exactly');
            await complete(old, 'fake-password-old-profile');
            assert(controller.secretLoading && !controller.secret, 'An obsolete profile reply cannot clear the new profile loading state');
            assertMasked(view, controller);
            await complete(profileRead, 'fake-password-profile-b');
            view.infoProfile.selected = 0;
            assertMasked(view, controller, 'Switching profiles also masks an already displayed password synchronously');
            assertEye(view);

            for (const pending of [false, true]) {
                for (const action of ['collapse', 'other Info', 'tab', 'close', 'migration to Saved',
                    'deletion', 'forget confirmation', 'connection', 'software radio', 'hardware radio', 'connect action']) {
                    reset();
                    const key = ['forget confirmation', 'connection'].includes(action) ? 'saved'
                        : ['migration to Saved', 'connect action'].includes(action) ? 'detected' : 'active';
                    open(key);
                    const selected = view.rows.get(key);
                    const request = reveal();
                    if (!pending) await complete(request);
                    const closing = [];
                    const signal = selected.details.connect('notify::reveal-child', () => {
                        if (!selected.details.reveal_child) closing.push([view.secret.label, view.secret.selectable, controller.secret]);
                    });
                    view.password.set_text('fake-input-before-transition');
                    if (action === 'collapse') selected.info.emit('clicked');
                    else if (action === 'other Info') second.info.emit('clicked');
                    else if (action === 'tab') view.savedTab.emit('clicked');
                    else if (action === 'close') view.close();
                    else if (action === 'forget confirmation') selected.remove.emit('clicked');
                    else if (action === 'connect action') selected.connect.emit('clicked');
                    else {
                        if (action === 'migration to Saved') backend.rows = backend.rows.map(item => item.key === key ? { ...item, strength: null } : item);

                        if (action === 'deletion') backend.rows = backend.rows.filter(item => item.key !== key);
                        if (action === 'connection') backend.rows = backend.rows.map(item => item.key === key ? { ...item, connected: true } : item);
                        if (action === 'software radio') backend.wifiEnabled = false;
                        if (action === 'hardware radio') backend.wifiHardwareEnabled = false;
                        backend.notify();
                    }
                    selected.details.disconnect(signal);
                    assert(!selected.details.reveal_child && !view.password.text && !controller.secret && !controller.secretRequested,
                        `${action} immediately collapses details and clears both passwords (${pending ? 'pending' : 'shown'})`);
                    assert(!selected.widget.has_css_class('expanded'), `${action} clears the old full-row expanded class before animation finishes`);
                    for (const data of backend.rows) assertRowPresentation(view, data);
                    assertMasked(view, controller);
                    equal(closing.at(-1), ['*******', false, ''], `${action} masks before the revealer begins closing`);
                    if (action === 'other Info') assertInline(view, controller, 'second');
                    else assertClosed(selected);
                    if (action.startsWith('migration')) {
                        assert(selected.widget.get_parent() === (action === 'migration to Saved' ? view.savedRows : view.networkRows),
                            'Migration keeps the original row in the destination list, but closes its details');
                    }
                    if (pending) {
                        assert(request.io.is_cancelled(), `${action} cancels the pending credential authorization`);
                        await complete(request, 'fake-password-obsolete');
                        assertMasked(view, controller, `${action} ignores the late fake credential reply`);
                    }
                }
            }
            assert(backend.mutations.every(([action, key]) => action === 'connect' && key === 'detected'),
                'Only the explicitly tested fake Connect actions reach backend mutations; no real networks are touched');
            print('PASS real-controller GTK inline selection, slide timing, mask-before-collapse, cancellation, UUID/stale replies and transition cleanup');
        } finally {
            controller.dispose();
            window?.set_child(null); window?.destroy();
            view?.dispose();
        }
    }
    async function smoke() {
        const services = ['org.freedesktop.NetworkManager', 'org.bluez', 'org.freedesktop.Notifications'];
        assert(services.every(name => !owner(name)), 'No desktop services exist on the isolated bus');
        Gtk.init();
        GLib.unsetenv('LD_PRELOAD');
        const palette = loadTheme(GLib.getenv('NETWORK_PANEL_CONFIG'), GLib.getenv('NETWORK_PANEL_PALETTE'));
        const app = new NetworkApplication({ instance_name: 'network-ui-offline' });
        app.register(null);
        const c = new FakeController();
        let dismissals = 0;
        // Preserve real GTK drawing while retaining both frame callbacks for
        // path/color assertions independent of screenshots or compositor output.
        const drawFunctions = new Map();
        const setDrawFunc = Gtk.DrawingArea.prototype.set_draw_func;
        Gtk.DrawingArea.prototype.set_draw_func = function (callback) {
            drawFunctions.set(this, callback);
            return setDrawFunc.call(this, callback);
        };
        try { panel = new NetworkPanel({ app, controller: c, palette, dismiss: () => { dismissals++; } }); }
        finally { Gtk.DrawingArea.prototype.set_draw_func = setDrawFunc; }
        c.onChange = () => { panel.render(); assertTooltipFree(panel, 'Fake-controller rerender'); };
        assertTooltipFree(panel, 'Initial tree');
        app.controller = c;
        app.panel = panel;
        assert(c.calls.length === 0, 'Constructing widgets does not call the controller or native backend');
        const monitor = monitorFor();
        assert(monitor && monitorFor('removed-connector') === monitor, 'Missing monitor falls back to a connected monitor');
        panel.toggle();
        assert(c.opened && c.monitor === (monitor.get_connector() || ''), 'Omitted monitor is resolved before controller toggle');
        equal(dismissals, 1, 'Opening dismisses other panels once');
        assert(panel.root.focusable && panel.window.get_focus() === panel.root, 'Opening initially focuses the root without a header control');
        panel.toggle();
        assert(!c.opened && dismissals === 1, 'Repeated omitted-monitor toggle closes without dismissing other panels again');
        panel.toggle('removed-connector');
        assert(c.opened && dismissals === 2, 'Stale connector opens on the effective monitor');

        // X11 cannot test layer-shell. Host the unchanged real GTK tree in a plain
        // window; the Astal window and its request/key controllers remain real.
        panel.window.set_child(null);
        async function layout(width, height) {
            if (host) { host.set_child(null); host.destroy(); }
            const original = panel.monitor;
            panel.monitor = { get_geometry: () => ({ width, height }) };
            panel.position();
            panel.monitor = original;
            host = new Gtk.Window({ child: panel.root, decorated: false,
                default_width: width, default_height: height, css_classes: ['network-panel'] });
            host.present();
            print(`LAYOUT checking ${width}x${height}`);
            await wait(150);
            // Reparenting replaces the window's CSS scope; measure again once those styles are resolved.
            panel.monitor = { get_geometry: () => ({ width, height }) };
            panel.position();
            panel.monitor = original;
            await wait(150);
            equal([host.get_width(), host.get_height()], [width, height], `Host remains ${width}x${height}`);
            const cardW = Math.max(0, Math.min(380, width - 16)), cardH = Math.max(0, Math.min(panel.preferredHeight(cardW), height - 54));
            equal([panel.card.width_request, panel.card.height_request], [cardW, cardH], 'Exact bounded card size requests');
            equal([panel.card.margin_top, panel.card.margin_end],
                [Math.min(46, Math.max(0, height - cardH - 8)), Math.min(72, Math.max(8, width - cardW - 8))], 'Exact original margins');
            if (cardW && cardH) {
                equal([panel.card.get_width(), panel.card.get_height()], [cardW, cardH], 'Content cannot resize the frame');
                const [ok, rect] = panel.card.compute_bounds(panel.root);
                assert(ok && rect.get_x() >= 0 && rect.get_y() >= 0
                    && rect.get_x() + rect.get_width() <= width && rect.get_y() + rect.get_height() <= height,
                'Card allocation fits inside the monitor');
            } else assert(!panel.card.visible, 'Degenerate monitor bounds hide the card');
        }
        await layout(1280, 800);
        const outer = panel.card.get_child(), outerDraw = drawFunctions.get(outer);
        assert(outer instanceof Gtk.DrawingArea && !outer.can_target && typeof outerDraw === 'function',
            'The outer card retains its non-interactive real Cairo frame');
        const { strokes: outerStrokes, fills: outerFills } = captureDrawing(outerDraw, outer);
        assert(outerStrokes.some(stroke => matchesColor(stroke.color, palette.yellow, 1) && stroke.path.length >= 4),
            'The outer cut-corner frame matches the audio panel’s yellow outline');
        assert(!outerStrokes.some(stroke => matchesColor(stroke.color, palette.cyan)),
            'The outer frame does not retain a cyan outline');
        assert(outerFills.some(fill => matchesColor(fill.color, '#25252f', 0.9)),
            'The outer card matches the audio panel’s dark background at 90% opacity without fading text or outlines');
        assert(![...outerStrokes, ...outerFills].some(paint => matchesColor(paint.color, palette.cyan)
            && paint.color[3] > 0 && paint.color[3] < 1
            && paint.path.some(([[x1, y1], [x2, y2]]) => Math.max(x1, x2) <= 16
                && Math.min(x1, x2) > 0 && Math.abs(y2 - y1) >= outer.get_height() / 4)),
        'The dark frame has no translucent cyan inner rail');
        print('PASS audio-matched yellow outer frame, 90%-opaque dark background, no inner rail, original layout and opening-only handoff');
        assert(panel.pages.get_height() > 200, 'Network list occupies the remaining card height');
        const tree = [...widgets(panel.root)];
        assert(panel.content.get_first_child() === panel.wifiNotice, 'Content begins with network notices, with no empty header row');
        assert(!tree.some(widget => widget instanceof Gtk.Label && widget.label === 'NETWORK'), 'No NETWORK title is present in the native widget tree');
        assert(!panel.pages.get_child_by_name('info'), 'The page stack has no standalone Info child');
        for (const row of panel.rows.values()) {
            assert(row.info instanceof Gtk.Button && row.info.get_child() instanceof Gtk.Image
                && row.info.get_parent() === row.secondary,
            'The small Info chevron belongs on the second line beside connection status');
            assert(/(?:down|end|right|up).*symbolic/.test(row.info.get_child().icon_name || ''), 'Info uses a symbolic chevron image');
            assert(row.details instanceof Gtk.Revealer && row.details.get_parent() === row.widget
                && row.details.transition_type === Gtk.RevealerTransitionType.SLIDE_DOWN
                && row.details.transition_duration >= 150 && row.details.transition_duration <= 220 && !row.details.reveal_child,
            'Each row starts collapsed with a roughly 180 ms slide-down revealer');
            assert(accessibleProperties.get(row.info)?.get(Gtk.AccessibleProperty.LABEL), 'The icon-only Info chevron has an accessible label');
            assert(row.forget.get_parent() === row.heading && row.forget.get_prev_sibling() === row.connect
                && row.forget.get_child().label === '×', 'Forget is a cross immediately to the right of Connect');
            assert(accessibleProperties.get(row.forget)?.get(Gtk.AccessibleProperty.LABEL)?.startsWith('Forget saved network '),
                'The cross has an accessible description of the forget action');
        }
        assert(typeof panel.toggleInfo === 'function', 'The panel exposes the shared inline Info toggle');
        const rowRemovals = new Set([...panel.rows.values()].flatMap(row => [row.remove, row.forget]));
        assert(!tree.some(widget => widget instanceof Gtk.Button && !rowRemovals.has(widget) && widget !== panel.dismissError
            && (/^(?:Wi-Fi (?:on|off)|↻|×|Advanced(?:…|\.{3})?)$/.test(widget.get_child()?.label || widget.label || '')
                || widget.has_css_class('wifi-toggle') || ['Refresh networks', 'Close network panel'].includes(
                    accessibleProperties.get(widget)?.get(Gtk.AccessibleProperty.LABEL)))),
        'No global Wi-Fi, manual scan, close or Advanced button is present in the native widget tree');
        assert(panel.nearbyTab instanceof Gtk.Button && panel.savedTab instanceof Gtk.Button
            && panel.networkLists instanceof Gtk.Stack, 'Nearby/Saved use real buttons and a list stack');
        assert(panel.listFrame instanceof Gtk.Overlay && panel.listFrame.get_child() instanceof Gtk.Box,
            'The shared list area has an unpainted base, without an inner outline');
        assert(!panel.listBorder, 'There is no list-border drawing widget');
        assert(panel.networkLists.get_parent() === panel.listFrame && !panel.listFrame.get_measure_overlay(panel.networkLists),
            'Both network lists share a non-measuring content overlay');
        equal([panel.networkLists.margin_top, panel.networkLists.margin_bottom,
            panel.networkLists.margin_start, panel.networkLists.margin_end], [6, 6, 6, 6],
        'List content has a compact six-pixel inset on every side');
        const tabs = panel.nearbyTab.get_parent();
        assert(tabs instanceof Gtk.Box && panel.savedTab.get_parent() === tabs && tabs.spacing === 4 && tabs.homogeneous,
            'The two equal-width underlined tabs have a small horizontal gap');
        assert(tabs.get_parent() === panel.listFrame.get_parent() && tabs.get_parent().spacing === 0,
            'The tab strip sits directly above the borderless list');
        async function assertJoinedTabs(selected) {
            await wait(150);
            const page = tabs.get_parent();
            const [nearbyOK, nearbyBounds] = panel.nearbyTab.compute_bounds(page);
            const [savedOK, savedBounds] = panel.savedTab.compute_bounds(page);
            const [frameOK, frameBounds] = panel.listFrame.compute_bounds(page);
            assert(nearbyOK && savedOK && frameOK, 'Joined tabs and list frame have real GTK allocations');
            equal([panel.nearbyTab.height_request, panel.savedTab.height_request], [32, 32], 'Both sibling-panel tabs request 32 pixels');
            equal([panel.nearbyTab.get_allocated_height(), panel.savedTab.get_allocated_height()], [32, 32],
                'Both sibling-panel tabs receive exactly 32 pixels of layout height');

            assert([nearbyBounds, savedBounds].every(bounds => Math.abs(bounds.get_height() - 32) <= 0.5),
                'Tab bounds preserve the 32-pixel strip within fractional-border rounding');
            assert(Math.abs(savedBounds.get_x() - nearbyBounds.get_x() - nearbyBounds.get_width() - 4) < 1,
                'The underline tabs have a four-pixel gap');
            assert(Math.abs(nearbyBounds.get_y() + nearbyBounds.get_height() - frameBounds.get_y()) < 1
                && Math.abs(savedBounds.get_y() + savedBounds.get_height() - frameBounds.get_y()) < 1,
            'Tabs sit immediately above the borderless list area');
            for (const [name, tab] of [['nearby', panel.nearbyTab], ['saved', panel.savedTab]]) {
                const expected = new Gdk.RGBA(); expected.parse(palette[name === selected ? 'red' : 'cyan']);
                const actual = tab.get_style_context().get_color();
                assert(['red', 'green', 'blue', 'alpha'].every(channel => Math.abs(actual[channel] - expected[channel]) < 0.01),
                    'Selected tab text is red and inactive tab text is cyan in the real theme');
            }

            const [listsOK, listsBounds] = panel.networkLists.compute_bounds(panel.listFrame);
            assert(listsOK && listsBounds.get_x() === 6 && listsBounds.get_y() === 6
                && listsBounds.get_width() === panel.listFrame.get_width() - 12
                && listsBounds.get_height() === panel.listFrame.get_height() - 12,
            'The real list allocation keeps its inset without an outline');
            for (const tab of [panel.nearbyTab, panel.savedTab]) {
                const border = tab.get_style_context().get_border();
                equal([border.top, border.right, border.bottom, border.left], [0, 0, 2, 0],
                    'Tabs draw only a two-pixel underline, never a surrounding outline');
            }
        }
        await assertJoinedTabs('nearby');
        panel.savedTab.emit('clicked');
        await assertJoinedTabs('saved');
        panel.nearbyTab.emit('clicked');
        await assertJoinedTabs('nearby');
        equal([panel.listTab, panel.networkLists.visible_child_name], ['nearby', 'nearby'], 'The initial list tab is Nearby');
        assertListPartition(['wifi', 'wired'], [], 'Initially connected networks are Nearby');
        let wifi = panel.rows.get('wifi');
        assert(wifi.name.label.includes('<b>Literal SSID</b>') && !wifi.name.use_markup, 'SSID is always literal text');
        assert(wifi.profile.visible && !panel.rows.get('wired').profile.visible, 'Profile chooser appears only for multiple profiles');
        await assertDropdownTooltipFree(panel, wifi.profile, 'Connection profile chooser');
        wifi.profile.selected = 2;
        equal(panel.profileChoices.get('wifi'), 'uuid-b', 'Duplicate profile names select by UUID');
        assertConnectionAction(wifi, c.rows.find(row => row.key === 'wifi'));
        wifi.connect.emit('clicked');
        equal(c.calls.at(-1), ['disconnect', 'wifi'], 'Connected row dispatches disconnect by stable key');
        // Replace the live row without rendering: the callback must not trust the
        // old delegate data or its still-visible Disconnect icon.
        c.rows = c.rows.map(row => row.key === 'wifi' ? { ...row, connected: false } : row);
        wifi.connect.emit('clicked');
        equal(c.calls.at(-1), ['connect', 'wifi', 'uuid-b'], 'Disconnected live row dispatches connect with the selected UUID');
        c.emit();
        assertConnectionAction(wifi, c.rows.find(row => row.key === 'wifi'));
        const currentRows = c.rows, callsBeforeRemoval = c.calls.length;
        c.rows = c.rows.filter(row => row.key !== 'wifi');
        wifi.connect.emit('clicked');
        equal(c.calls.length, callsBeforeRemoval, 'Removed live key cannot dispatch a stale row action');
        c.rows = currentRows.map(row => row.key === 'wifi' ? { ...row, connected: true } : row); c.emit();
        c.credentialRow = c.rows[0]; c.credentialKey = 'credential:1'; c.emit();
        panel.password.set_text('offline-test-password');
        panel.password.grab_focus();
        const focus = host.get_focus(), profileModel = wifi.profile.get_model();
        c.rows = [{ ...c.rows[1] }, { ...c.rows[0], strength: 43 }]; c.emit();
        await wait();
        assert(panel.rows.get('wifi') === wifi, 'Stable row survives native-style updates and reorder');
        assertNetworkOrder(['wired', 'wifi'], 'Visible row order follows native updates even with a hidden separator');
        assert(wifi.profile.get_model() === profileModel, 'Unchanged profile model is not recreated');
        assert(panel.password.text === 'offline-test-password' && host.get_focus() === focus, 'Ordinary updates preserve password and keyboard focus');
        assert(!panel.password.visibility, 'Password entry is masked');
        c.credentialKey = 'credential:2'; c.emit();
        equal(panel.password.text, '', 'Credential identity change clears the input');
        panel.password.set_text('submit-on-enter');
        panel.password.emit('activate');
        assert(c.calls.some(call => call[0] === 'password' && call[1] === 'submit-on-enter') && panel.password.text === '', 'Enter submits only through the controller and wipes the input');
        c.credentialRow = c.rows.find(row => row.key === 'wifi'); c.credentialKey = 'credential:3'; c.emit();
        panel.password.set_text('cancel-me');
        panel.credentials.get_last_child().get_last_child().emit('clicked');
        assert(!c.credentialKey && !panel.password.text, 'Cancel clears credentials outside row delegates');

        const dataRow = c.rows.find(row => row.key === 'wifi'), profiles = dataRow.profiles;
        dataRow.profiles = profiles.slice(0, 1); c.emit();
        assert(!wifi.profile.visible && !panel.profileChoices.has('wifi'), 'Removed profile UUID falls back to automatic without stale selection');
        dataRow.profiles = profiles; c.emit();
        print('PASS stable rows, UUID selection, focus and credential lifecycle');

        {
            const originalRows = c.rows;
            const available = network('new-wifi', { name: 'Never connected', known: false, connected: false, profiles: [] });
            const changing = network('joining-wifi', { name: 'Joining for the first time', known: false, connected: false, changing: true, profiles: [] });
            const connectedOnly = network('connected-only', { name: 'Connected without a saved profile', known: false, profiles: [] });
            const savedWired = network('saved-wired', { name: 'Saved Ethernet', kind: 'WIRED', wireless: false, connected: false,
                profiles: [{ id: 'Wired profile', uuid: 'wired-saved' }] });
            wifi.profile.selected = 2;
            const model = wifi.profile.get_model();
            c.rows = [available, originalRows.find(row => row.key === 'wifi'), changing, savedWired, connectedOnly,
                originalRows.find(row => row.key === 'wired')];
            c.emit();
            await wait();
            assertNetworkOrder(['wifi', 'saved-wired', 'connected-only', 'wired', 'divider', 'new-wifi', 'joining-wifi'],
                'Saved or connected networks precede unsaved networks, preserving order within each group');
            const unsaved = panel.rows.get('new-wifi'), joining = panel.rows.get('joining-wifi');
            const connected = panel.rows.get('connected-only'), wired = panel.rows.get('saved-wired');
            const availableModel = unsaved.profile.get_model();
            for (const data of c.rows) assertRowPresentation(panel, data);
            for (const row of [wifi, unsaved, joining, wired]) {
                assert(row.widget.get_first_child() === row.heading && row.heading.orientation === Gtk.Orientation.HORIZONTAL
                    && row.name.get_parent() === row.heading && row.connect.get_parent() === row.heading
                    && row.heading.get_first_child() === (row.marker || row.name),
                'The optional decorative marker, name and Connect/Disconnect button share the top horizontal row');
            }
            const [inline, connectBounds] = unsaved.connect.compute_bounds(unsaved.heading);
            assert(inline && Math.abs(connectBounds.get_x() + connectBounds.get_width() - unsaved.heading.get_width()) <= 1,
                'The inline Connect button is right-aligned in the normal viewport');
            assert(unsaved.name.wrap && unsaved.name.label === 'Wi-Fi · Never connected', 'Inline network names retain wrapping and the Wi-Fi prefix');
            for (const row of [unsaved, joining]) {
                assert(!row.forget.visible && !row.forget.sensitive, 'Never-connected Wi-Fi has no visible or enabled Forget button');
                assert(!row.info.visible && !row.profile.visible,
                    'Unsaved Wi-Fi has no connected-only actions, profile chooser or empty secondary row');
            }
            assert(unsaved.connect.sensitive && !joining.connect.sensitive, 'An unsaved changing network stays in the available group with Connect disabled');
            unsaved.connect.emit('clicked');
            equal(c.calls.at(-1), ['connect', 'new-wifi', ''], 'Unsaved inline Connect dispatches the current key without a saved UUID');
            assert(!connected.forget.visible && !connected.forget.sensitive && connected.info.visible,
                'A connected-only Wi-Fi network is remembered for grouping, but has no saved profile to forget');
            assert(wired.name.label === 'Wired · Saved Ethernet' && wired.forget.visible && wired.forget.sensitive
                && wired.info.visible, 'Disconnected saved Ethernet retains heading Remove and Info, without secondary actions');
            wired.connect.emit('clicked');
            equal(c.calls.at(-1), ['connect', 'saved-wired', ''], 'Saved Ethernet still dispatches native default-profile activation');
            wired.forget.emit('clicked');
            equal(c.calls.at(-1), ['forget', 'saved-wired'], 'Saved Ethernet Forget still dispatches its own key');
            panel.forgetCancel.emit('clicked');
            const liveWired = panel.rows.get('wired');
            assert(liveWired.forget.visible && liveWired.info.visible, 'Connected saved Ethernet keeps Forget and Info');
            liveWired.connect.emit('clicked');
            equal(c.calls.at(-1), ['disconnect', 'wired'], 'Connected Ethernet still dispatches Disconnect');

            c.rows = c.rows.map(row => row.key === 'wifi' ? { ...row, known: false, connected: false } : row); c.emit();
            assertNetworkOrder(['saved-wired', 'connected-only', 'wired', 'divider', 'new-wifi', 'wifi', 'joining-wifi'],
                'Losing known and connected state moves the existing row across the divider');
            assert(panel.rows.get('wifi') === wifi && wifi.profile.get_model() === model && panel.profileChoices.get('wifi') === 'uuid-b',
                'Cross-group moves preserve the widget, unchanged profile model and selected UUID');
            assert(!wifi.forget.visible && !wifi.forget.sensitive, 'A newly unsaved row hides Forget and its empty secondary action row');
            wifi.connect.emit('clicked');
            equal(c.calls.at(-1), ['connect', 'wifi', 'uuid-b'], 'The moved inline Connect retains UUID-based dispatch');
            c.rows = c.rows.map(row => row.key === 'wifi' ? { ...row, known: true } : row); c.emit();
            assertNetworkOrder(['wifi', 'saved-wired', 'connected-only', 'wired', 'divider', 'new-wifi', 'joining-wifi'],
                'A newly saved disconnected row returns to the remembered group');
            assert(wifi.forget.visible && wifi.forget.sensitive && wifi.info.visible,
                'Saving restores Forget and offline Info without secondary actions');
            assert(panel.rows.get('wifi') === wifi && wifi.profile.get_model() === model && panel.profileChoices.get('wifi') === 'uuid-b',
                'Saving does not recreate the row or lose its selected UUID');

            c.rows = c.rows.map(row => row.key === 'new-wifi' ? { ...row, connected: true } : row); c.emit();
            assertNetworkOrder(['new-wifi', 'wifi', 'saved-wired', 'connected-only', 'wired', 'divider', 'joining-wifi'],
                'Connecting an unsaved network moves it above the divider without marking it saved');
            assert(panel.rows.get('new-wifi') === unsaved && unsaved.profile.get_model() === availableModel
                && !unsaved.forget.visible && !unsaved.forget.sensitive && unsaved.info.visible,
                'The connected unsaved row keeps its widgets and gains only connected actions');
            unsaved.connect.emit('clicked');
            equal(c.calls.at(-1), ['disconnect', 'new-wifi'], 'The moved row now dispatches Disconnect');
            c.rows = c.rows.map(row => row.key === 'new-wifi' ? { ...row, connected: false } : row);
            unsaved.connect.emit('clicked');
            equal(c.calls.at(-1), ['connect', 'new-wifi', ''], 'A stale Disconnect icon still consults live data after a group change');
            c.emit();
            assertNetworkOrder(['wifi', 'saved-wired', 'connected-only', 'wired', 'divider', 'new-wifi', 'joining-wifi'],
                'Disconnecting an unsaved network restores available-group order');
            assert(!unsaved.info.visible, 'Disconnecting the unsaved row hides connected-only Info');

            c.rows = c.rows.map(row => ({ ...row, known: true, connected: false })); c.emit();
            assertNetworkOrder(c.rows.map(row => row.key), 'The divider is hidden when only remembered networks remain');
            c.rows = c.rows.map(row => ({ ...row, known: false, connected: false })); c.emit();
            assertNetworkOrder(c.rows.map(row => row.key), 'The divider is hidden when only unsaved networks remain');
            c.rows = originalRows; c.emit();
            assertNetworkOrder(['wired', 'wifi'], 'Restoring the original saved group leaves no trailing visible divider');
            assert(panel.rows.get('wifi') === wifi && wifi.profile.get_model() === model && panel.profileChoices.get('wifi') === 'uuid-b',
                'Repeated group transitions preserve the original Wi-Fi widget and UUID selection');
            assert(!panel.rows.has('new-wifi') && !panel.rows.has('saved-wired'), 'Temporary networks are removed without disturbing retained rows');
            print('PASS inline network actions, unsaved Forget policy, single group divider and live group transitions');
        }

        {
            const originalRows = c.rows, before = c.calls.length;
            const coffee = '_Black Sheep Coffee Free WiFi';
            const cases = [
                [network('saved-open-title', { name: coffee, security: 'Open' }), 'Wi-Fi - Open - _Black Sheep Coffee Free WiFi'],
                [network('saved-wpa2-title', { name: coffee }), 'Wi-Fi - WPA2 - _Black Sheep Coffee Free WiFi'],
                ...['', null, undefined].map((security, index) => [network(`saved-fallback-${index}`, { name: coffee, security }),
                    'Wi-Fi - Open - _Black Sheep Coffee Free WiFi']),
                [network('saved-literal-title', { name: '_<b>Coffee & "Tea"</b> — <span>Wi-Fi</span>', security: '<i>WPA2</i>' }),
                    'Wi-Fi - <i>WPA2</i> - _<b>Coffee & "Tea"</b> — <span>Wi-Fi</span>'],
                [network('saved-long-title', { name: `${coffee} ${'<b>Literal & very long SSID</b> '.repeat(24)}` }), null],
            ].map(([data, title]) => [{ ...data, connected: false, strength: null }, title]);
            c.rows = [...originalRows, ...cases.map(([data]) => data)]; c.emit();
            panel.savedTab.emit('clicked');
            await wait(150);
            for (const [data, title] of cases) {
                const row = panel.rows.get(data.key);
                assertRowPresentation(panel, data);
                if (title !== null) equal(row.name.label, title, 'Saved titles use the exact security-first hyphen format, without escaping or rearranging the SSID');
                assert(!row.metadata.visible && !row.profile.visible && !row.details.reveal_child,
                    'Collapsed Saved rows show no second status, profile or secondary-action line');
                equal(row.name.get_layout().get_line_count(), 1, 'Saved labels occupy a single real Pango layout line');
            }
            const data = cases.at(-1)[0], row = panel.rows.get(data.key);
            const title = `Wi-Fi - WPA2 - ${data.name}`;
            assert(row.name.get_layout().is_ellipsized(), 'A long Saved title actually ellipsizes at the normal card width');
            equal(row.name.label, title, 'The ellipsized Saved label retains the full untruncated literal title');
            row.profile.selected = 2;
            const model = row.profile.get_model(), dropdown = row.profile;
            for (const strength of [64, null, 0, null]) {
                c.rows = c.rows.map(item => item.key === data.key ? { ...item, strength } : item); c.emit();
                const live = c.rows.find(item => item.key === data.key);
                assertRowPresentation(panel, live);
                assert(panel.rows.get(data.key) === row && row.profile === dropdown && row.profile.get_model() === model
                    && panel.profileChoices.get(data.key) === 'uuid-b' && row.profile.selected === 2,
                'Repeated Saved/Nearby migration preserves the widget, dropdown, model and exact chosen UUID');
                assert(row.widget.get_parent() === (strength === null ? panel.savedRows : panel.networkRows),
                    'Detection reparents the same widget, including real zero in Nearby');
                if (strength !== null) {
                    equal(row.name.label, `Wi-Fi · ${data.name}`, 'Nearby restores the original title without a Saved security prefix');
                    assertTooltipFree(panel, 'Saved-to-Nearby title migration');
                }
            }
            equal(c.calls.slice(before), [['back']], 'Formatting and scan migration only switch tabs; no metadata, password or network action is requested');
            row.info.emit('clicked');
            assert(!row.info.visible && !row.details.reveal_child, 'Long Saved titles have no expandable details');
            equal(row.name.label, title, 'Saved retains the full long title text without hover');
            c.rows = c.rows.map(item => item.key === data.key ? { ...item, strength: 64 } : item); c.emit();
            panel.nearbyTab.emit('clicked'); row.info.emit('clicked');
            assertInline(panel, c, data.key);
            assert(panel.summary.label.startsWith(`• SSID / name: ${data.name}\n`) && !panel.summary.use_markup,
                'Expanded details expose the complete literal SSID without a tooltip');
            assertTooltipFree(panel, 'Expanded long Saved title');
            row.info.emit('clicked');
            panel.nearbyTab.emit('clicked');
            c.rows = originalRows; c.emit();
            print('PASS exact Saved titles, Open fallback, literal SSIDs, one-line ellipsis/full expanded text and reversible UUID-preserving migration');
        }

        {
            const originalRows = c.rows, before = c.calls.length;
            const cases = [
                ...[0, 68, null, undefined, NaN, Infinity, -Infinity, '68', false].map((strength, index) =>
                    network(`heading-signal-${index}`, { connected: true, strength })),
                ...['', null, undefined, '<b>WPA2 & WPA3</b>'].map((security, index) =>
                    network(`heading-security-${index}`, { connected: false, known: false, profiles: [], security, strength: 68 })),
            ];
            c.rows = [...originalRows, ...cases]; c.emit();
            await wait();
            for (const data of cases) assertRowPresentation(panel, data);
            equal(c.calls.length, before, 'Heading formatting never requests a network action or secret');
            c.rows = originalRows; c.emit();
            print('PASS white heading security/signal, literal Open fallback and finite-only percentages');
        }

        {
            const originalRows = c.rows, model = wifi.profile.get_model();
            const choice = panel.profileChoices.get('wifi'), callsBefore = c.calls.length;
            const radios = [c.wifiAvailable, c.wifiEnabled, c.wifiHardwareEnabled];
            const signals = [[null, 'Not currently detected'], [64, '64%'], [null, 'Not currently detected'], [0, '0%']];
            for (const [strength, expected] of signals) {
                c.rows = c.rows.map(row => row.key === 'wifi' ? { ...row, connected: false, strength } : row); c.emit();
                assertRowPresentation(panel, c.rows.find(row => row.key === 'wifi'));
                if (strength !== null)
                    equal(wifi.status.label, `WPA2 · ${expected}`, `Nearby saved heading displays ${expected}`);
                assert(panel.rows.get('wifi') === wifi && wifi.profile.get_model() === model && panel.profileChoices.get('wifi') === choice,
                    'Detection transitions preserve the row widget, profile model and selected UUID');
                const undetected = strength === null;
                assert(wifi.info.visible === !undetected, 'Only detected saved rows retain heading Info in Nearby, without secondary actions');
                equal([wifi.connect.visible, wifi.profile.visible, wifi.forget.visible, wifi.remove.visible],
                    undetected ? [false, false, false, true] : [true, true, true, false],
                    'Undetected rows use heading Remove and hide Connect/profile; detected rows use Nearby actions');
                assertListPartition(undetected ? ['wired'] : ['wired', 'wifi'], undetected ? ['wifi'] : [],
                    'Signal appearance/disappearance migrates the existing row between tabs; genuine zero stays Nearby');
            }
            equal(c.calls.length, callsBefore, 'Signal-only updates do not dispatch controller actions');
            equal([c.wifiAvailable, c.wifiEnabled, c.wifiHardwareEnabled], radios, 'Detection does not change radio state');

            c.rows = c.rows.map(row => row.key === 'wifi' ? { ...row, connected: true, strength: null } : row); c.emit();
            wifi.info.emit('clicked');
            assertInline(panel, c, 'wifi');
            for (const [strength, expected] of signals) {
                c.rows = c.rows.map(row => row.key === 'wifi' ? { ...row, strength } : row);
                c.selectedRow = c.rows.find(row => row.key === 'wifi'); c.emit();
                equal(panel.summary.label.split('\n').at(-1), `• Signal: ${expected}`, `Info signal displays ${expected}`);
                equal(wifi.metadata.label, 'Connected', 'Connected Wi-Fi metadata does not duplicate heading security/signal');
                equal(wifi.status.label, strength === null ? 'WPA2' : `WPA2 · ${expected}`,
                    'Connected heading omits unknown signal while Info retains its explanatory text');
                assertRowPresentation(panel, c.rows.find(row => row.key === 'wifi'));
            }
            wifi.info.emit('clicked');
            assert(c.page === 'list' && !wifi.details.reveal_child, 'Clicking the same heading closes inline Info');
            c.rows = originalRows; c.emit();
            print('PASS undetected saved signal labels, detection transitions, genuine zero and inline Info signal text');
        }

        {
            const originalRows = c.rows;
            const offline = network('offline-a', { connected: false, strength: null });
            const offlineOther = network('offline-b', { connected: false, strength: undefined });
            const zero = network('zero-signal', { connected: false, strength: 0 });
            const changing = network('changing-saved', { connected: false, changing: true, strength: null });
            const available = network('available', { connected: false, known: false, profiles: [], strength: null });
            const callsBefore = c.calls.length;
            c.rows = [available, offline, { ...originalRows.find(row => row.key === 'wifi'), strength: null }, offlineOther,
                zero, { ...originalRows.find(row => row.key === 'wired'), strength: null }, changing]; c.emit();
            assertListPartition(['wifi', 'zero-signal', 'wired', 'changing-saved', 'divider', 'available'], ['offline-a', 'offline-b'],
                'Only idle saved Wi-Fi without finite strength leaves Nearby; connected, changing, wired and real zero remain');
            equal(c.calls.length, callsBefore, 'Partitioning rows does not request credentials or dispatch network operations');
            const row = panel.rows.get('offline-a'), other = panel.rows.get('offline-b');
            const nearbyZero = panel.rows.get('zero-signal'), joining = panel.rows.get('changing-saved');
            for (const entry of [row, other]) {
                assert(entry.remove instanceof Gtk.Button && entry.remove.get_parent() === entry.heading
                    && (entry.remove.get_child()?.label || entry.remove.label) === '×', 'Saved removal is a small heading × button');
                assert(entry.remove.visible && entry.remove.sensitive && !entry.connect.visible && !entry.profile.visible
                    && !entry.forget.visible && !entry.info.visible,
                'Offline saved rows show heading Remove only, not Info, Connect, profile choice, Forget or an empty action row');
            }
            assert(!wifi.remove.visible && wifi.connect.visible && wifi.info.visible,
                'A connected Wi-Fi row without signal retains connected actions in Nearby');
            assert(!nearbyZero.remove.visible && nearbyZero.connect.visible && nearbyZero.forget.visible && nearbyZero.info.visible,
                'A real zero remains a detected saved Nearby row');
            assert(!joining.remove.visible && joining.connect.visible && !joining.connect.sensitive && joining.info.visible,
                'A changing no-signal saved row stays Nearby with Connect disabled');
            row.profile.selected = 2;
            const model = row.profile.get_model();
            equal(panel.profileChoices.get('offline-a'), 'uuid-b', 'Hidden dropdown retains UUID-based profile choice');
            const visibility = [c.opened, c.monitor, dismissals], beforeTab = c.calls.length;
            c.credentialKey = 'tab-password'; c.credentialRow = available; c.emit();
            panel.password.set_text('clear-on-tab');
            panel.savedTab.emit('clicked');
            equal(c.calls.slice(beforeTab), [['back']], 'Switching to Saved delegates controller.back without closing or scanning actions');
            equal([panel.listTab, panel.networkLists.visible_child_name, panel.pages.visible_child_name], ['saved', 'saved', 'list'],
                'Saved button selects the Saved list stack');
            assert(!panel.password.text && !c.credentialKey && !c.credentialRow && !panel.credentials.visible,
                'Tab switching clears password and credential prompt');
            equal([c.opened, c.monitor, dismissals], visibility, 'Tab switching preserves open state, monitor and panel handoff');

            const beforeRemove = c.calls.length;
            row.remove.emit('clicked');
            equal(c.calls.slice(beforeRemove), [['forget', 'offline-a']], 'Heading Remove only requests the existing forget confirmation');
            assert(panel.forget.visible && c.forgetKey === 'offline-a' && panel.listTab === 'saved', 'Remove opens confirmation on Saved');
            panel.forgetCancel.emit('clicked');
            assert(!panel.forget.visible && !c.forgetKey && panel.rows.get('offline-a') === row, 'Cancel keeps the saved row and its profiles');
            assert(!c.calls.slice(beforeRemove).some(call => call[0] === 'confirmForget'), 'Cancel never dispatches confirmed forgetting');
            row.remove.emit('clicked'); panel.forgetConfirm.emit('clicked');
            equal(c.calls.slice(-2), [['forget', 'offline-a'], ['confirmForget']], 'Confirmation dispatches forgetting exactly once');
            assert(!c.forgetKey && !panel.forget.visible, 'Confirmed removal clears confirmation UI');

            const beforeSavedInfo = c.calls.length;
            row.info.emit('clicked');
            panel.toggleInfo('offline-a');
            equal(c.calls.length, beforeSavedInfo, 'Saved rows cannot expand through a hidden button or programmatic toggle');
            assert(!row.details.reveal_child && c.page === 'list', 'Saved rows remain collapsed');
            c.rows = c.rows.map(item => item.key === 'offline-a' ? { ...item, strength: 64 } : item); c.emit();
            panel.nearbyTab.emit('clicked');
            const beforeInfo = c.calls.length;
            row.info.emit('clicked');
            equal(c.calls.slice(beforeInfo), [['info', 'offline-a']], 'Detected saved Info requests metadata only, never reveal');
            assertInline(panel, c, 'offline-a');
            assert(panel.listTab === 'nearby', 'Detected saved Info expands only within Nearby');
            assert(panel.summary.label.includes(offline.name) && panel.summary.label.includes(offline.interface)
                && !panel.summary.use_markup && panel.savedProfiles.label.includes('uuid-a') && panel.savedProfiles.label.includes('uuid-b'),
            'Offline Info presents literal saved SSID/interface and all profile UUIDs');
            assert(panel.detailLabels.get('profileId').visible && panel.detailLabels.get('profileUuid').visible,
                'Offline Info displays the selected profile metadata');
            for (const field of ['ipv4', 'gateway4', 'ipv6', 'gateway6', 'dns', 'mac', 'bssid', 'frequency', 'band', 'channel', 'mode', 'rate'])
                assert(!panel.detailLabels.get(field).visible, `Offline Info hides live ${field}`);
            assert(panel.passwordLine instanceof Gtk.Box && panel.secret.get_parent() === panel.passwordLine
                && panel.reveal.get_parent() === panel.passwordLine && panel.passwordLine.visible
                && panel.reveal.visible && panel.reveal.sensitive && panel.secret.visible && !panel.secretLoading.visible,
            'Offline saved Wi-Fi displays one masked password line with an explicit eye control');
            assertMasked(panel, c);
            assertEye(panel);
            assert(panel.infoProfile instanceof Gtk.DropDown && panel.infoProfile.visible && panel.infoProfile.sensitive,
                'Multiple disconnected saved Wi-Fi profiles expose the Info profile dropdown');
            equal(c.selectedProfileUuid, 'uuid-a', 'Offline Info begins with the safe snapshot profile UUID');
            equal(panel.infoProfile.selected, 0, 'Info dropdown maps its first actual profile, without an automatic activation choice');
            await assertDropdownTooltipFree(panel, panel.infoProfile, 'Saved-password profile chooser');
            for (const strength of [null, 0, null, 64]) {
                c.rows = c.rows.map(item => item.key === 'offline-a' ? { ...item, strength } : item);
                c.selectedRow = c.rows.find(item => item.key === 'offline-a'); c.emit();
                assert(c.page === 'list' && !row.details.reveal_child && !c.selectedKey && !c.secretRequested,
                    'Signal migration closes the expanded row instead of hiding open details on another tab');
                assertMasked(panel, c, 'Migration masks the shared label synchronously, before the slide finishes');
                assert(panel.rows.get('offline-a') === row && row.profile.get_model() === model
                    && panel.profileChoices.get('offline-a') === 'uuid-b', 'Signal changes during Info retain the row, model and chosen UUID');
                assertListPartition(strength === null
                    ? ['wifi', 'zero-signal', 'wired', 'changing-saved', 'divider', 'available']
                    : ['offline-a', 'wifi', 'zero-signal', 'wired', 'changing-saved', 'divider', 'available'],
                strength === null ? ['offline-a', 'offline-b'] : ['offline-b'], 'The closed row reparents between the correct lists');
                panel.selectListTab(strength === null ? 'saved' : 'nearby');
                row.info.emit('clicked');
                if (strength === null) {
                    assert(!row.info.visible && !row.details.reveal_child && c.page === 'list', 'Undetected saved rows cannot reopen Info');
                } else {
                    assert(row.info.visible, 'Detected saved rows retain the Nearby Info control');
                    assertInline(panel, c, 'offline-a');
                }
            }
            assert(!c.calls.slice(beforeInfo).some(call => ['reveal', 'connect', 'disconnect'].includes(call[0])),
                'Signal changes and explicit metadata reopens never request credentials or network operations');
            row.info.emit('clicked');
            equal(c.calls.at(-1), ['back'], 'Clicking the selected heading delegates only controller.back');
            equal([panel.listTab, panel.networkLists.visible_child_name, panel.pages.visible_child_name], ['nearby', 'nearby', 'list'],
                'Collapsing detected saved Info stays on Nearby without navigation');
            assert(panel.rows.get('offline-a') === row && row.profile.get_model() === model
                && panel.profileChoices.get('offline-a') === 'uuid-b', 'Expansion/collapse preserves row, model and profile UUID');
            assertMasked(panel, c);
            row.info.emit('clicked');
            const infoModel = panel.infoProfile.get_model();
            equal(infoModel.get_n_items(), 2, 'Info dropdown contains actual saved profiles only');
            panel.infoProfile.selected = 1;
            equal(c.calls.at(-1), ['selectSavedProfile', 'uuid-b'], 'Duplicate IDs select the actual second profile UUID');
            assert(c.selectedProfileUuid === 'uuid-b' && panel.detailLabels.get('profileUuid').label.includes('uuid-b')
                && !c.secretRequested && panel.secret.label === '*******', 'Choosing a saved profile updates safe details without revealing');
            const beforeCosmeticUpdate = c.calls.length;
            c.rows = c.rows.map(item => item.key === 'offline-a' ? { ...item, profiles: item.profiles.slice().reverse()
                .map(profile => ({ ...profile, id: '<i>Renamed duplicate</i>' })) } : item);
            c.selectedRow = c.rows.find(item => item.key === 'offline-a'); c.emit();
            equal(panel.infoProfile.selected, 0, 'Info profile reorder follows the selected UUID, not its old index or duplicate ID');
            equal(c.calls.length, beforeCosmeticUpdate, 'Rebuilding the Info profile model never dispatches a user selection or secret read');
            panel.reveal.emit('clicked');
            assert(c.secretRequested && panel.secret.visible && panel.secret.selectable && !panel.secret.use_markup
                && panel.secret.label === '<b>literal secret</b>', 'Offline reveal displays only explicit literal, selectable credentials');
            equal(c.calls.at(-1), ['reveal'], 'Offline reveal is a user action, not a connection attempt');
            panel.reveal.emit('clicked');
            assert(!c.secretRequested && panel.secret.visible, 'Offline Hide keeps the password line visible');
            assertMasked(panel, c);
            assertEye(panel);
            equal(c.calls.at(-1), ['hide'], 'Offline Hide delegates only credential clearing');
            const retryUuid = c.selectedProfileUuid, beforeFailedReveal = c.calls.length;
            c.holdReveal = true; panel.reveal.emit('clicked');
            assert(c.secretLoading && panel.reveal.sensitive, 'The eye remains usable during pending saved authorization');
            assertMasked(panel, c);
            assertEye(panel, true);
            c.failReveal('Saved password authorization denied.');
            assert(panel.pages.visible_child_name === 'list' && row.details.reveal_child && c.selectedProfileUuid === retryUuid
                && panel.error.visible && panel.error.label === 'Saved password authorization denied.',
            'Failed saved authorization keeps Info and its profile selection while displaying the safe error');
            assert(!c.secretRequested && !panel.secretLoading.visible && panel.secret.visible
                && panel.reveal.visible && panel.reveal.sensitive,
            'Failure clears loading and restores an enabled reveal eye over the constant mask');
            assertMasked(panel, c);
            assertEye(panel);
            c.emit();
            equal(c.calls.slice(beforeFailedReveal), [['reveal']], 'Failure and rerender never automatically retry or dispatch Hide');
            c.holdReveal = false; panel.reveal.emit('clicked');
            equal(c.calls.slice(beforeFailedReveal), [['reveal'], ['reveal']], 'The next button click retries directly rather than hiding an empty value');
            assert(c.selectedProfileUuid === retryUuid && !panel.error.visible && c.secretRequested
                && panel.secret.visible && panel.secret.label === '<b>literal secret</b>',
            'Successful explicit retry clears the error and displays only the fake credential');
            assertEye(panel, true);
            panel.reveal.emit('clicked');
            c.holdReveal = true;
            panel.reveal.emit('clicked');
            assert(panel.secretLoading.visible && panel.reveal.sensitive, 'Offline authorization shows loading but allows eye cancellation');
            assertMasked(panel, c);
            assertEye(panel, true);
            panel.infoProfile.selected = 1;
            assert(c.selectedProfileUuid === 'uuid-a' && !c.secretLoading && !c.secretRequested && !c.secret
                && !panel.secretLoading.visible && panel.secret.label === '*******' && !panel.secret.selectable,
            'A user profile switch clears a pending saved authorization and immediately masks the shared label');
            assertEye(panel);
            c.holdReveal = false;
            panel.reveal.emit('clicked'); panel.infoProfile.selected = 0;
            assert(c.selectedProfileUuid === 'uuid-b' && !c.secretRequested, 'A profile switch clears an already displayed saved password');
            assertMasked(panel, c);
            // Detect the row before testing a changing state: a Saved-to-Nearby
            // migration now deliberately closes Info, even without credentials.
            row.info.emit('clicked');
            c.rows = c.rows.map(item => item.key === 'offline-a' ? { ...item, strength: 50 } : item); c.emit();
            panel.nearbyTab.emit('clicked'); row.info.emit('clicked');
            const beforeChanging = c.calls.length;
            c.rows = c.rows.map(item => item.key === 'offline-a' ? { ...item, changing: true } : item);
            c.selectedRow = c.rows.find(item => item.key === 'offline-a'); c.emit();
            assert(panel.reveal.visible && !panel.reveal.sensitive && panel.infoProfile.visible && !panel.infoProfile.sensitive,
                'Changing saved Wi-Fi cannot reveal credentials or change profile selection');
            c.rows = c.rows.map(item => item.key === 'offline-a' ? { ...item, changing: false,
                profiles: item.profiles.filter(profile => profile.uuid === 'uuid-b') } : item);
            c.selectedRow = c.rows.find(item => item.key === 'offline-a'); c.emit();
            assert(!panel.infoProfile.visible && panel.reveal.visible && panel.reveal.sensitive,
                'A single saved profile needs no dropdown but still supports explicit offline reveal');
            equal(c.calls.length, beforeChanging, 'Changing/single-profile renders never automatically request a password');
            row.info.emit('clicked');
            c.rows = c.rows.map(item => item.key === 'offline-a' ? offline : item); c.emit();
            assert(!c.calls.slice(beforeInfo).some(call => ['connect', 'disconnect', 'password'].includes(call[0])),
                'Info profile selection, reveal and hide never activate or share the disconnected network');
            assert(!panel.infoProfile.visible && !row.details.reveal_child, 'Leaving Info hides the dropdown and collapses its row');
            assertMasked(panel, c);

            panel.savedTab.emit('clicked'); row.remove.emit('clicked'); panel.nearbyTab.emit('clicked');
            assert(!c.forgetKey && !panel.forget.visible && panel.listTab === 'nearby', 'Tab switching cancels a pending removal confirmation');
            c.rows = c.rows.map(item => item.key === 'offline-a' ? { ...item, strength: 50 } : item); c.emit();
            panel.nearbyTab.emit('clicked'); row.info.emit('clicked'); c.secretLoading = true; c.secretRequested = true; c.emit();
            panel.password.set_text('clear-pending-reveal');
            panel.selectListTab('saved');
            assert(c.page === 'list' && !c.secretLoading && !c.secretRequested && !c.secret
                && !row.details.reveal_child && !panel.password.text,
            'Programmatic tab switching clears pending reveal, selected details and password');
            assertMasked(panel, c);
            equal([c.opened, c.monitor, dismissals], visibility, 'Pending-read cleanup does not close or move the panel');
            panel.nearbyTab.emit('clicked'); row.info.emit('clicked'); panel.reveal.emit('clicked');
            assert(panel.secret.label === '<b>literal secret</b>', 'A saved password is displayed before explicit panel close');
            panel.close();
            assert(!c.secret && !c.selectedProfileUuid && !row.details.reveal_child && !panel.infoProfile.visible,
                'Closing clears offline credentials, profile selection and the Info dropdown');
            assertMasked(panel, c);
            equal([panel.listTab, panel.networkLists.visible_child_name], ['nearby', 'nearby'], 'Closing resets the selected tab to Nearby');
            panel.toggle();
            equal([panel.listTab, panel.networkLists.visible_child_name], ['nearby', 'nearby'], 'Reopening starts in Nearby');

            c.rows = [offline, offlineOther]; c.emit();
            assertListPartition([], ['offline-a', 'offline-b'], 'Saved-only data leaves Nearby independently empty');
            panel.savedTab.emit('clicked');
            c.rows = [offline]; c.emit();
            assertListPartition([], ['offline-a'], 'Removing a saved row updates only its tab count');
            assert(other.widget.get_parent() === null && !panel.profileChoices.has('offline-b'), 'Removed Saved widgets and UUID choices are cleaned up');
            c.rows = [available]; c.emit();
            assertListPartition(['available'], [], 'Nearby-only data leaves Saved independently empty');
            assert(panel.listTab === 'saved' && panel.savedEmpty.visible, 'Saved empty state stays on the selected Saved tab');
            c.rows = []; c.emit();
            assertListPartition([], [], 'Both tabs have independent empty labels when no networks remain');

            c.rows = Array.from({ length: 32 }, (_, index) => network(`offline-layout-${index}`, { connected: false, strength: null })); c.emit();
            assertListPartition([], c.rows.map(row => row.key), 'Many long saved rows stay exclusively in Saved');
            await layout(1280, 800);
            await layout(360, 240);
            await layout(160, 110);
            const expanded = panel.rows.get('offline-layout-0');
            expanded.info.emit('clicked');
            assert(c.page === 'list' && !expanded.details.reveal_child, 'Saved layout rows cannot expand');
            for (const [width, height] of [[1280, 800], [360, 240], [160, 110]]) {
                await layout(width, height);
                assert(!expanded.info.visible && !expanded.metadata.visible && !expanded.name.wrap
                    && expanded.name.ellipsize === Pango.EllipsizeMode.END && !expanded.details.reveal_child,
                'Saved rows remain single-line and non-expandable at narrow bounds');
                equal(expanded.name.get_layout().get_line_count(), 1, 'The Saved title remains one Pango line');
                equal(expanded.name.label, `Wi-Fi - WPA2 - ${expanded.data.name}`,
                    'Narrow bounds preserve the complete literal title text');
                assertTooltipFree(panel, `${width}x${height} Saved rows`);
                assertMasked(panel, c);
            }
            assert(!expanded.widget.has_css_class('expanded'), 'Saved rows never receive the expanded highlight');
            panel.nearbyTab.emit('clicked');
            c.rows = originalRows; c.emit();
            wifi = panel.rows.get('wifi');
            await layout(1280, 800);
            print('PASS Saved partition/count, heading Remove confirmation, no Saved expansion, Nearby saved Info, mask/eye, tab cleanup, empty states and bounded layout');
        }

        {
            const originalRows = c.rows;
            const unsaved = Array.from({ length: 8 }, (_, index) => network(`six-unsaved-${index}`, {
                name: `Nearby network ${index + 1}`, known: false, connected: false, profiles: [],
            }));
            c.rows = [network('six-connected', { name: 'Connected network', profiles: [] }), ...unsaved];
            c.emit();
            await layout(1280, 800);
            const viewport = panel.list.get_child();
            viewport.vadjustment.value = 0;
            await wait(150);
            for (const data of unsaved.slice(0, 6)) {
                const row = panel.rows.get(data.key);
                assertRowPresentation(panel, data);
                const [ok, bounds] = row.widget.compute_bounds(viewport);
                assert(ok && bounds.get_y() >= -1
                    && bounds.get_y() + bounds.get_height() <= viewport.get_height() + 1,
                    `${data.key}: all six unsaved rows fit completely below the connected network`);
            }
            const [seventhOK, seventh] = panel.rows.get(unsaved[6].key).widget.compute_bounds(viewport);
            assert(seventhOK && seventh.get_y() >= viewport.get_height() - 1,
                'The seventh unsaved row scrolls instead of enlarging the six-row viewport');
            const compactHeight = panel.card.get_height();
            unsaved[0].name = 'A long nearby network name that needs more than one line '.repeat(2);
            c.emit();
            await layout(1280, 800);
            const [wrappedOK, last] = panel.rows.get(unsaved[5].key).widget.compute_bounds(viewport);
            assert(wrappedOK && last.get_y() + last.get_height() <= viewport.get_height() + 1
                && panel.card.get_height() > compactHeight,
                'Wrapped names grow the measured height without clipping the sixth unsaved row');
            assert(c.page === 'list' && !c.selectedKey, 'Rendering connected networks does not select a row');
            c.rows = originalRows; c.emit();
            await layout(1280, 800);
            wifi = panel.rows.get('wifi');
        }

        c.wifiEnabled = false; c.emit();
        assert(panel.wifiNotice.label === 'Wi-Fi is off' && !wifi.connect.sensitive, 'Disabled Wi-Fi state is reflected without header controls');
        c.wifiHardwareEnabled = false; c.emit();
        assert(panel.wifiNotice.label === 'Wi-Fi is hardware blocked' && !wifi.connect.sensitive, 'Hardware block is reflected without a global toggle');
        c.wifiAvailable = false; c.emit();
        equal(panel.wifiNotice.label, 'No Wi-Fi adapter', 'Missing Wi-Fi adapter is reflected');
        c.wifiAvailable = c.wifiHardwareEnabled = c.wifiEnabled = true; c.emit();
        wifi.forget.emit('clicked');
        assert(panel.forget.visible && c.forgetKey === 'wifi', 'Forget opens explicit forget-all confirmation');
        panel.forgetCancel.emit('clicked');
        assert(!c.forgetKey, 'Forget confirmation can be canceled');
        wifi.forget.emit('clicked'); panel.forgetConfirm.emit('clicked');
        assert(c.calls.some(call => call[0] === 'confirmForget'), 'Forget confirmation delegates the operation');

        c.credentialRow = c.rows.find(row => row.key === 'wifi'); c.credentialKey = 'credential:4'; c.emit();
        panel.password.set_text('clear-on-page-change');
        wifi.info.emit('clicked');
        assertInline(panel, c, 'wifi');
        assert(!panel.password.text, 'Expanding Info wipes credential input');
        assertMasked(panel, c);
        assertEye(panel);
        c.details = { ipv4: ['192.0.2.5/24'], dns: ['192.0.2.1', '2001:db8::1'], mode: { native: '<b>literal</b>' } }; c.emit();
        equal(panel.detailLabels.get('ipv4').label, '• IPv4: 192.0.2.5/24', 'Detail fields format native values');
        assert(panel.detailLabels.get('dns').label.includes('\n2001:db8::1') && !panel.summary.use_markup, 'Details use plain wrapped text');
        assert(panel.reveal.visible && panel.reveal.sensitive && !panel.infoProfile.visible,
            'Connected Info offers active-profile reveal without an offline profile dropdown');
        c.holdReveal = true; panel.reveal.emit('clicked');
        assert(c.secretLoading && panel.secretLoading.visible && panel.reveal.sensitive,
            'Authorization keeps the eye usable to cancel without exposing old text');
        assertMasked(panel, c);
        assertEye(panel, true);
        panel.reveal.emit('clicked');
        assert(!c.secretLoading && !c.secretRequested, 'The loading eye cancels and hides instead of requesting duplicate credentials');
        equal(c.calls.at(-1), ['hide'], 'Cancelling authorization delegates hideSecret');
        assertMasked(panel, c);
        assertEye(panel);
        c.holdReveal = false;
        panel.reveal.emit('clicked');
        assert(c.secretRequested && panel.secret.visible && panel.secret.label === '<b>literal secret</b>'
            && !panel.secret.use_markup && panel.secret.selectable, 'Reveal is literal, selectable text controlled by explicit secretRequested');
        panel.reveal.emit('clicked');
        assert(!c.secretRequested, 'Hide clears controller reveal state');
        assertMasked(panel, c);
        panel.reveal.emit('clicked'); wifi.info.emit('clicked');
        assert(c.page === 'list' && !c.secretRequested && !wifi.details.reveal_child,
            'Clicking the selected Info heading collapses and clears without Back navigation');
        assertMasked(panel, c, 'Collapse replaces plaintext immediately, before the slide finishes');
        panel.rows.get('wired').info.emit('clicked');
        assertInline(panel, c, 'wired');
        assert(!panel.detailLabels.get('bssid').visible && !panel.reveal.visible && !panel.reveal.sensitive
            && !panel.infoProfile.visible, 'Wired info omits Wi-Fi fields, profile dropdown and secret reveal');
        panel.rows.get('wired').info.emit('clicked');

        print('PASS Wi-Fi notices, forget confirmation and inline Info/reveal/hide');
        c.error = '<b>Literal error</b> ' + 'An unusually long error '.repeat(60); c.emit();
        panel.root.grab_focus();
        await wait();
        equal([panel.card.get_width(), panel.card.get_height()], [380, 480], 'Root focus, long SSIDs and errors never enlarge the frame');
        assert(!panel.error.use_markup && panel.error.lines === 3, 'Errors are literal and bounded to three lines');
        c.error = ''; c.credentialKey = ''; c.credentialRow = null; c.emit();
        await layout(360, 240);
        await layout(160, 110);
        await layout(16, 54);
        eventController(panel.root, Gtk.GestureClick).emit('released', 1, 0, 0);
        assert(!c.opened, 'Outside click can dismiss even when monitor bounds leave no visible card');
        panel.toggle();
        await layout(1280, 800);

        const outside = eventController(panel.root, Gtk.GestureClick);
        const [, bounds] = panel.card.compute_bounds(panel.root);
        outside.emit('released', 1, bounds.get_x() + 20, bounds.get_y() + 20);
        assert(c.opened, 'Inside click does not dismiss');
        c.credentialRow = c.rows.find(row => row.key === 'wifi'); c.credentialKey = 'credential:5'; c.emit();
        panel.password.set_text('clear-on-outside');
        outside.emit('released', 1, 0, 0);
        assert(!c.opened && !panel.window.visible && !panel.password.text && !c.credentialKey, 'Outside click closes and wipes credentials');
        panel.toggle();
        eventController(panel.window, Gtk.EventControllerKey).emit('key-pressed', Gdk.KEY_Escape, 9, 0);
        assert(!c.opened, 'Escape closes the panel');
        panel.toggle();
        const realMonitors = panel.monitors;
        panel.monitors = { get_n_items: () => 0 };
        panel.updateMonitors();
        panel.monitors = realMonitors;
        assert(!c.opened && !panel.monitor, 'Removing the selected monitor closes safely');

        const before = c.calls.length, dismissedBefore = dismissals;
        equal(app.handleRequest('{"command":"status"}'), c.status(), 'Status returns controller Waybar JSON unchanged');
        assert(c.calls.length === before && dismissals === dismissedBefore, 'Status has no controller mutation or panel handoff');
        app.handleRequest('{"command":"close"}');
        assert(!c.calls.slice(before).some(call => ['connect', 'disconnect', 'wifi', 'password', 'forget', 'confirmForget'].includes(call[0])), 'Close cannot mutate network state');
        for (const request of ['{"command":"run"}', '{"command":"toggle","monitor":7}', '{"command":"close","extra":true}', '<untrusted>']) {
            let rejected = false;
            try { app.handleRequest(request); } catch (_) { rejected = true; }
            assert(rejected, 'Invalid IPC request is rejected');
        }
        for (const opened of [false, true]) {
            if (opened) panel.toggle();
            const visibility = [c.opened, c.monitor, panel.window.visible, dismissals];
            const callsBeforeToggle = c.calls.length;
            equal(app.handleRequest('{"command":"toggleWifi"}'), 'ok', 'Radio IPC delegates successfully to the fake controller');
            equal(c.calls.slice(callsBeforeToggle), [['wifi']], 'Radio IPC dispatches only the global toggle action');
            equal([c.opened, c.monitor, panel.window.visible, dismissals], visibility, 'Radio IPC does not open, close, move or hand off the panel');
            c.toggleError = new Error('Mock radio permission denied.');
            let error;
            try { app.handleRequest('{"command":"toggleWifi"}'); } catch (caught) { error = caught; }
            equal(error?.message, 'Mock radio permission denied.', 'Native radio failure propagates to the IPC error path');
            equal([c.opened, c.monitor, panel.window.visible, dismissals], visibility, 'Failed radio IPC also preserves visibility');
            c.toggleError = null;
            if (opened) panel.close();
        }
        for (const monitor of ['', 'DP-1']) {
            const callsBeforeInvalid = c.calls.length;
            let rejected = false;
            try { app.handleRequest(JSON.stringify({ command: 'toggleWifi', monitor })); } catch (_) { rejected = true; }
            assert(rejected && c.calls.length === callsBeforeInvalid, 'Radio IPC rejects monitor arguments without dispatch');
        }
        c.error = ''; c.emit();
        print('PASS real-import frontend IPC status, close, radio toggle/error and strict request parsing');
        c.rows = []; c.emit();
        assert(panel.empty.visible && panel.savedEmpty.visible && panel.rows.size === 0 && panel.profileChoices.size === 0,
            'Removed rows and profile choices are cleaned up across both empty tabs');
        assertListPartition([], [], 'Empty network tabs have no visible rows or divider');
        await inlineControllerTests(app, palette);
        assert(services.every(name => !owner(name)), 'No real service was started or mutated');
        print(`PASS network GTK UI: ${checks} assertions; fake credentials/backends only, no native network mutations`);
        print('LIMITATION Xvfb validates GTK layout/events, not compositor layer-shell placement or keyboard grabs.');
    }

    let exitCode = 0;
    Gtk.Button.prototype.update_property = Gtk.DropDown.prototype.update_property = trackAccessible;
    try { await smoke(); }
    catch (error) { console.error(error); exitCode = 1; }
    finally {
        Gtk.Button.prototype.update_property = updateAccessible;
        Gtk.DropDown.prototype.update_property = updateDropdownAccessible;
        host?.destroy();
        panel?.dispose();
    }
    System.exit(exitCode);
}

async function gjsApplicationTests() {
    // Closed-only bootstrap/socket check. Run exclusively through node test.js --ui.
    const { default: GLib } = await import('gi://GLib');
    const { default: Gio } = await import('gi://Gio');
    const { default: Gtk } = await import('gi://Gtk?version=4.0');
    const { default: Astal } = await import('gi://Astal?version=4.0');
    const { default: NM } = await import('gi://NM?version=1.0');
    const { default: System } = await import('system');

    const address = GLib.getenv('DBUS_SESSION_BUS_ADDRESS');
    const runtime = GLib.getenv('XDG_RUNTIME_DIR');
    if (GLib.getenv('NETWORK_UI_ISOLATED') !== '1'
        || GLib.getenv('NETWORK_PANEL_IMPORT_ONLY') !== '1'
        || GLib.getenv('GDK_BACKEND') !== 'x11'
        || !runtime || !address?.startsWith(`unix:path=${runtime}/bus,`)
        || GLib.getenv('DBUS_SYSTEM_BUS_ADDRESS') !== address
        || !GLib.getenv('NETWORK_TEST_AGS'))
        throw new Error('Use node test.js --ui with its private service-free bus and authenticated Xvfb.');

    const entry = GLib.getenv('NETWORK_PANEL_CONFIG');
    const source = GLib.path_get_dirname(entry);
    const { NetworkApplication } = await import(GLib.filename_to_uri(entry, null));
    const { NetworkBackend } = await import(GLib.filename_to_uri(GLib.build_filenamev([source, 'backend.js']), null));
    const { NetworkController } = await import(GLib.filename_to_uri(GLib.build_filenamev([source, 'logic.js']), null));
    const instance = 'network-panel-bootstrap';
    let app, checks = 0;
    function assert(condition, message) {
        if (!condition) throw new Error(message);
        checks++;
    }
    function equal(actual, expected, message) { assert(JSON.stringify(actual) === JSON.stringify(expected), message); }
    function pause() {
        return new Promise(resolve => GLib.timeout_add(GLib.PRIORITY_DEFAULT, 50,
            () => { resolve(); return GLib.SOURCE_REMOVE; }));
    }
    function serviceFree() {
        const [names] = Gio.DBus.session.call_sync('org.freedesktop.DBus', '/org/freedesktop/DBus',
            'org.freedesktop.DBus', 'ListNames', null, new GLib.VariantType('(as)'),
            Gio.DBusCallFlags.NONE, 1000, null).deep_unpack();
        return names.every(name => name.startsWith(':') || name === 'org.freedesktop.DBus' || name === app?.application_id);
    }

    async function request(message) {
        const child = Gio.Subprocess.new([GLib.getenv('NETWORK_TEST_AGS'), 'request', '--instance', instance, '--', message],
            Gio.SubprocessFlags.STDOUT_PIPE | Gio.SubprocessFlags.STDERR_PIPE);
        let timedOut = false;
        let timer = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 5000, () => {
            timer = 0;
            timedOut = true;
            child.force_exit();
            return GLib.SOURCE_REMOVE;
        });
        try {
            // Awaiting communication pumps the same native default context that
            // serves the real Astal socket; a synchronous client would deadlock it.
            const [ok, stdout, stderr] = await new Promise((resolve, reject) => child.communicate_utf8_async(null, null,
                (process, result) => {
                    try { resolve(process.communicate_utf8_finish(result)); }
                    catch (error) { reject(error); }
                }));
            assert(!timedOut && ok && child.get_successful(), 'Real AGS request completes successfully within five seconds');
            equal(stderr.trim(), '', 'Real AGS request has no client diagnostics');
            return stdout.trim();
        } finally { if (timer) GLib.source_remove(timer); }
    }

    function closedState(controller, backend, panel, phase) {
        assert(!controller.opened && !backend._opened && !panel.window.visible && !panel.window.get_mapped()
            && panel.window.keymode === Astal.Keymode.NONE, `${phase}: panel stays hidden without a keyboard grab`);
        assert(!controller.available && !controller.wifiAvailable && !backend.available && !backend.wifiAvailable
            && !controller.rows.length && !backend.rows.length && !backend.client.get_devices().length,
        `${phase}: real native backend has no service or network devices`);
        assert(controller.secret === '' && !controller.secretRequested && !controller.credentialKey && !controller.credentialRow
            && panel.password.get_text() === '' && panel.secret.label === '*******'
            && !panel.secret.selectable,
        `${phase}: credentials remain empty, with the password label masked and non-selectable`);
        assert(backend._scanTimer === 0 && backend._scans.size === 0 && backend._scanMembers.size === 0
            && backend._operations.size === 0 && controller.read === null && controller.mutation === null,
        `${phase}: no scans, scan timer or native mutations are pending`);
        assert(serviceFree(), `${phase}: private bus has no other well-known service owners`);
    }

    async function bootstrap() {
        assert(Gio.DBus.session.get_guid() === Gio.DBus.system.get_guid(), 'Native session and system connections use the same private bus');
        assert(serviceFree(), 'Private bus is service-free before application construction');
        Gtk.init();
        GLib.unsetenv('LD_PRELOAD');
        app = new NetworkApplication({ instance_name: instance });
        // Match production: registering first makes Astal see its own name as occupied.
        app.acquire_socket();
        assert(app.register(null), 'Separate bootstrap application registers on the private bus');
        assert(!app.get_is_remote(), 'Bootstrap application owns its registration');
        app.startPanel();
        const controller = app.controller, panel = app.panel, backend = controller.backend;
        assert(controller instanceof NetworkController && backend instanceof NetworkBackend, 'startPanel constructs the real controller and backend');
        assert(backend.client instanceof NM.Client && backend.network !== null, 'Real Astal network singleton exposes a native libnm client');
        assert(app.started && backend._started, 'Application bootstrap starts the real backend');
        app.startPanel();
        assert(app.controller === controller && app.panel === panel && controller.backend === backend,
            'Repeated startPanel does not recreate application state');
        await pause();
        closedState(controller, backend, panel, 'After startup');
        const status = controller.status();
        const badge = JSON.parse(status);
        assert(badge.class === 'offline' && badge.tooltip === 'NetworkManager unavailable or no network devices',
            'Actual controller status reports offline with no network devices');
        equal(await request('{"command":"status"}'), status, 'Real socket status matches actual controller Waybar JSON');
        equal(await request('{"command":"close"}'), 'ok', 'Real socket close succeeds while already closed');
        const nativeToggleWifi = backend.toggleWifi;
        let radioCalls = 0, radioError = null;
        // Exercise real IPC/controller routing, but never write to the real NM.Client.
        backend.toggleWifi = () => { radioCalls++; if (radioError) throw radioError; };
        try {
            equal(await request('{"command":"toggleWifi"}'), 'ok', 'Real socket radio command reaches the stubbed native boundary');
            equal(radioCalls, 1, 'Socket radio command delegates exactly once');
            closedState(controller, backend, panel, 'After stubbed radio toggle');
            radioError = new Error('Isolated radio permission denied.');
            equal(JSON.parse(await request('{"command":"toggleWifi"}')), { error: radioError.message }, 'Real socket returns the controller radio error');
            equal(radioCalls, 2, 'Failed radio request is not retried implicitly');
            closedState(controller, backend, panel, 'After stubbed radio failure');
            equal(JSON.parse(await request('{"command":"toggleWifi","monitor":"DP-1"}')),
                { error: 'Invalid network panel request.' }, 'Socket radio command rejects monitor arguments');
            equal(radioCalls, 2, 'Invalid radio arguments never reach the native boundary');
            radioError = null;
            equal(await request('{"command":"toggleWifi"}'), 'ok', 'A later radio request clears the previous controller error');
            equal(controller.error, '', 'Successful socket radio request clears stale errors');
        } finally { backend.toggleWifi = nativeToggleWifi; }
        equal(JSON.parse(await request('{"command":"run","untrusted":"bootstrap-untrusted-marker"}')),
            { error: 'Invalid network panel request.' }, 'Real socket rejects invalid requests without reflecting untrusted input');
        equal(await request('{"command":"status"}'), status, 'Socket close and error leave offline status unchanged');
        await pause();
        closedState(controller, backend, panel, 'After socket requests');
        print('PASS real application startPanel, native offline backend and closed-only socket status/close/stubbed-radio/error roundtrips');

        app.disposePanel();
        assert(app.panel === null && app.controller === null && app.get_windows().length === 0, 'Application disposal removes panel references and windows');
        assert(panel.disposed && controller.disposed && backend._disposed, 'Application disposal reaches real panel, controller and backend');
        await pause();
        assert(backend._idle === 0 && backend._scanTimer === 0 && backend._scans.size === 0
            && backend._scanMembers.size === 0 && backend._operations.size === 0 && backend._watches.size === 0
            && backend._entries.size === 0, 'Disposed backend retains no idle source, scan, operation or signal watch');
        assert(controller.secret === '' && controller.credentialKey === '', 'Disposed controller retains no credential state');
        assert(serviceFree(), 'Private bus remains service-free after native backend disposal');
        print(`PASS network application bootstrap: ${checks} assertions; real backend, closed only, no native network mutations`);
    }

    let exitCode = 0;
    try { await bootstrap(); }
    catch (error) { console.error(error); exitCode = 1; }
    finally {
        try { app?.disposePanel(); app?.quit(); }
        catch (error) { console.error(error); exitCode = 1; }
    }
    System.exit(exitCode);
}

if (typeof process !== 'undefined' && process.versions?.node) {
    const args = process.argv.slice(2);
    if (args[0] === '--ui') {
        try { await isolatedUI(args.slice(1)); }
        catch (error) {
            console.error('FAIL ' + error.message);
            process.exitCode = error.message.startsWith('Interrupted by SIGINT') ? 130
                : error.message.startsWith('Interrupted by SIGTERM') ? 143 : 1;
        }
    } else {
        if (args.length) throw new Error('Use node --test test.js or node test.js --ui --help');
        await nodeControllerTests();
        await nodeLogicTests();
        await nodeLauncherTests();
        await nodeStartupTests();
    }
} else {
    const args = typeof ARGV === 'undefined' ? [] : ARGV;
    if (args.length !== 1) throw new Error('Use panel-gjs -m test.js native|runtime|ui|application');
    switch (args[0]) {
    case 'native': await gjsNativeTests(); break;
    case 'runtime': await gjsRuntimeTests(); break;
    case 'ui': await gjsUITests(); break;
    case 'application': await gjsApplicationTests(); break;
    default: throw new Error('Unknown GJS test mode: ' + args[0]);
    }
}
