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
        for (const name of ['NETWORK_CUSTOM_HELPER', 'DASHBOARD_PANEL', 'MEDIA_PANEL']) env[name] = guard;
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
            if (fs.existsSync(attempts)) throw new Error(`${phase} attempted an external helper/panel call:\n${output}`);
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
    const image = 'data:image/png;base64,aW1hZ2U=';
    const otherImage = 'data:image/png;base64,b3RoZXI=';
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
                activate: [], disconnect: [], forget: [], details: [], readSecret: [],
            };
            this.identities = new Map(rows.filter(row => row.connected).map(row => [row.key, {
                interface: row.interface, ssid: row.ssid, profileUuid: row.profiles[0]?.uuid || '',
                bssid: row.wireless ? '02:00:00:00:00:01' : '', activePath: `/active/${row.key}`,
            }]));
            this.detailValues = new Map(rows.map(row => [row.key, { ipv4: ['192.0.2.10/24'], profileId: row.name }]));
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
    }

    function fixture(t, rows = [wifi()], { opened = true } = {}) {
        const backend = new FakeBackend(rows);
        const hooks = { changes: 0, cancellations: [], encodings: [] };
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
            encodeQr: (request, io) => {
                const encoding = { request, io, ...deferred() };
                hooks.encodings.push(encoding);
                return encoding.promise;
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
        assert.equal(controller.credentialKey, '');
        assert.equal(controller.forgetKey, '');
        assert.equal(controller.pendingKey, '');
        assert.deepEqual(controller.details, {});
        assert.equal(controller.secretLoading, false);
        assert.equal(controller.qrLoading, false);
        assert.equal(controller.secretRequested, false);
        assertSecret(controller.secret, '');
        assert.equal(controller.qrImage, '');
    }

    function mutationCounts(backend) {
        return {
            opened: [...backend.calls.opened],
            toggleWifi: backend.calls.toggleWifi, activate: backend.calls.activate.length,
            disconnect: backend.calls.disconnect.length, forget: backend.calls.forget.length,
        };
    }

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

        it('keeps native scanning enabled on info and QR pages until the panel closes', async t => {
            const { controller, backend } = fixture(t, [active()]);
            controller.info('wifi');
            assert.equal(controller.page, 'info');
            assert.equal(backend.opened, true);
            const qr = controller.qr('wifi');
            assert.equal(controller.page, 'qr');
            assert.equal(backend.opened, true);
            assert.deepEqual(backend.calls.opened, [true], 'Page navigation does not interrupt native scanning');
            controller.close();
            assert.equal(backend.opened, false);
            assert.deepEqual(backend.calls.opened, [true, false]);
            backend.calls.readSecret[0].resolve(credentials);
            await qr;
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
            assert.equal(hooks.encodings.length, 0);
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

        it('does not reveal secrets on the list, for disconnected rows, or for Ethernet', async t => {
            const { controller, backend } = fixture(t, [wifi(), wired({ connected: true })]);
            await controller.revealSecret();
            controller.info('wifi');
            assert.equal(controller.page, 'list');
            controller.info('missing');
            await controller.qr('wired');
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

        it('reports secret authorization failure without retaining data or encoding a QR', async t => {
            const { controller, backend, hooks } = fixture(t, [active()]);
            controller.info('wifi');
            const work = controller.revealSecret();
            backend.calls.readSecret[0].reject(new Error('Secret authorization denied.'));
            await work;
            assert.equal(controller.error, 'Secret authorization denied.');
            assert.equal(controller.secretLoading, false);
            assertSecret(controller.secret, '');
            assert.equal(hooks.encodings.length, 0);
        });

        it('describes an open network without manufacturing a password', async t => {
            const { controller, backend } = fixture(t, [active({ securityType: 'open' })]);
            controller.info('wifi');
            const work = controller.revealSecret();
            backend.calls.readSecret[0].resolve({ password: '', authentication: 'nopass', hidden: false });
            await work;
            assert.equal(controller.secret, 'No password (open network)');
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

    describe('request-local QR encoding', () => {
        for (const hidden of [false, true]) {
            it(`encodes a pinned SSID and native authentication/hidden metadata (hidden=${hidden})`, async t => {
                const { controller, backend, hooks } = fixture(t, [active()]);
                const work = controller.qr('wifi');
                assert.equal(controller.page, 'qr');
                assert.equal(controller.qrLoading, true);
                assert.equal(controller.secretRequested, false);
                assert.equal(hooks.encodings.length, 0);
                const request = backend.calls.readSecret[0];
                request.resolve({ ...credentials, hidden });
                await flush();
                assert.equal(hooks.encodings.length, 1);
                const encoding = hooks.encodings[0];
                assert.equal(encoding.request.ssid, 'Cafe');
                assert.equal(encoding.request.authentication, 'WPA');
                assert.equal(encoding.request.hidden, hidden);
                assertSecret(encoding.request.password, password);
                assert.equal(encoding.io, request.io);
                assert.equal(controller.qrImage, '');
                assertSecret(controller.secret, '');
                encoding.resolve(image);
                await work;
                assert.equal(controller.qrImage, image);
                assert.equal(controller.qrLoading, false);
                assertSecret(controller.secret, '');
            });
        }

        it('does not encode if the native secret request fails', async t => {
            const { controller, backend, hooks } = fixture(t, [active()]);
            const work = controller.qr('wifi');
            backend.calls.readSecret[0].reject(new Error('Native secret request failed.'));
            await work;
            assert.equal(hooks.encodings.length, 0);
            assert.equal(controller.qrImage, '');
            assert.equal(controller.qrLoading, false);
            assert.equal(controller.error, 'Native secret request failed.');
        });

        it('does not start the encoder when credentials arrive after closing', async t => {
            const { controller, backend, hooks } = fixture(t, [active()]);
            const work = controller.qr('wifi');
            controller.close();
            backend.calls.readSecret[0].resolve(credentials);
            await work;
            assert.equal(hooks.encodings.length, 0);
            assertCleared(controller);
        });

        for (const invalid of [null, undefined, 42, '', 'not an image', 'data:image/svg+xml;base64,AAAA']) {
            it(`rejects an encoder reply without a PNG data URL (${typeof invalid}, ${String(invalid).length} characters)`, async t => {
                const { controller, backend, hooks } = fixture(t, [active()]);
                const work = controller.qr('wifi');
                backend.calls.readSecret[0].resolve(credentials);
                await flush();
                hooks.encodings[0].resolve(invalid);
                await work;
                assert.equal(controller.qrImage, '');
                assert.match(controller.error, /Could not generate.*QR/);
                assert.equal(controller.qrLoading, false);
                assertSecret(controller.secret, '');
            });
        }

        it('reports asynchronous encoder failure without displaying credentials', async t => {
            const { controller, backend, hooks } = fixture(t, [active()]);
            const work = controller.qr('wifi');
            backend.calls.readSecret[0].resolve(credentials);
            await flush();
            hooks.encodings[0].reject(new Error('The QR encoder timed out.'));
            await work;
            assert.equal(controller.error, 'The QR encoder timed out.');
            assert.equal(controller.qrImage, '');
            assert.equal(controller.qrLoading, false);
            assertSecret(controller.secret, '');
        });

        for (const [field, value] of Object.entries(identityChanges)) {
            it(`discards an asynchronously encoded QR after ${field} changes`, async t => {
                const { controller, backend, hooks } = fixture(t, [active()]);
                const work = controller.qr('wifi');
                backend.calls.readSecret[0].resolve(credentials);
                await flush();
                assert.equal(hooks.encodings.length, 1);
                backend.identities.set('wifi', { ...backend.getIdentity('wifi'), [field]: value });
                hooks.encodings[0].resolve(image);
                await work;
                assertCleared(controller);
                assert.match(controller.error, /connection changed/i);
            });
        }

        for (const destination of ['back', 'close', 'info']) {
            for (const outcome of ['resolve', 'reject']) {
                it(`discards an encoder ${outcome} after navigating to ${destination}`, async t => {
                    const { controller, backend, hooks } = fixture(t, [active()]);
                    const work = controller.qr('wifi');
                    backend.calls.readSecret[0].resolve(credentials);
                    await flush();
                    if (destination === 'info') controller.info('wifi');
                    else controller[destination]();
                    assert.equal(hooks.encodings[0].io.cancelled, true);
                    if (outcome === 'resolve') hooks.encodings[0].resolve(image);
                    else hooks.encodings[0].reject(new Error('Obsolete encoder error.'));
                    await work;
                    assert.equal(controller.qrImage, '');
                    assert.equal(controller.error, '');
                    assertSecret(controller.secret, '');
                    assert.equal(controller.page, destination === 'info' ? 'info' : 'list');
                });
            }
        }

        it('does not let an obsolete QR result clear a newer encoder or replace its image', async t => {
            const { controller, backend, hooks } = fixture(t, [active()]);
            const first = controller.qr('wifi');
            backend.calls.readSecret[0].resolve(credentials);
            await flush();
            const second = controller.qr('wifi');
            backend.calls.readSecret[1].resolve(credentials);
            await flush();
            hooks.encodings[0].resolve(image);
            await first;
            assert.equal(controller.qrLoading, true);
            assert.equal(controller.qrImage, '');
            assert.equal(hooks.encodings[1].io.cancelled, false);
            hooks.encodings[1].resolve(otherImage);
            await second;
            assert.equal(controller.qrImage, otherImage);
            assert.equal(controller.qrLoading, false);
        });
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
                assert.equal(hooks.encodings.length, 0);
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

        it('cancels an asynchronous encoder on dispose and drops its eventual image', async t => {
            const { controller, backend, hooks } = fixture(t, [active()]);
            const work = controller.qr('wifi');
            backend.calls.readSecret[0].resolve(credentials);
            await flush();
            controller.dispose();
            const changes = hooks.changes;
            assert.equal(hooks.encodings[0].io.cancelled, true);
            hooks.encodings[0].resolve(image);
            await work;
            assert.equal(hooks.changes, changes);
            assertCleared(controller);
        });
    });
}

async function nodeLogicTests() {
    const { default: assert } = await import('node:assert/strict');
    const { describe, it } = await import('node:test');
    const {
        escapeMarkup, identityEqual, parseRequest, primary, profileStamp,
        signalBars, signalIcon, sortRows, waybarStatus,
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

    const root = new URL('../../../', import.meta.url);
    const read = path => readFileSync(new URL(path, root), 'utf8');
    const panels = ['network', 'audio', 'media', 'dashboard'];

    test('panel module exports the shared runtime helper once and removes the old helper file', () => {
        const source = read('niri/panels/default.nix');
        assert.equal(existsSync(new URL('niri/panels/ags-runtime.nix', root)), false);
        assert.match(source, /mkAgsRuntime\s*=\s*\{\s*pkgs,\s*ags,?\s*\}:/);
        assert.equal((source.match(/\bmkAgsRuntime\s*=/g) ?? []).length, 1);
        assert.equal((source.match(/makeWrapper\s+\$\{lib\.getExe pkgs\.gjs\}\s+"\$out\/bin\/panel-gjs"/g) ?? []).length, 1);
        assert.match(source, /module\s*=\s*\{[\s\S]*?\}:\s*\{/);
    });

    test('Niri imports the exported panel Home Manager module', () => {
        const source = read('niri/default.nix');
        assert.match(source, /imports = \[\s*\(import \.\/panels\)\.module\b/);
        assert.doesNotMatch(source, /(?:^|\n)\s*\.\/panels\s*(?:\n|$)/);
    });

    for (const panel of [...panels, 'app-launcher']) {
        test(`${panel} uses the shared runtime helper without defining a second wrapper`, () => {
            const source = read(`niri/panels/${panel}.nix`);
            assert.match(source, /ags = \(import \.\/default\.nix\)\.mkAgsRuntime\s*\{\s*inherit pkgs;\s*ags = /);
            assert.doesNotMatch(source, /ags-runtime\.nix|\bmkAgsRuntime\s*=|makeWrapper|\$out\/bin\/panel-gjs/);
        });
    }

    // AGS 2.3 run shares $XDG_RUNTIME_DIR/ags.js across every instance. Keep the
    // original runtime directory for IPC, but compile to an explicit panel file.
    for (const panel of panels) {
        test(`${panel} launcher bundles to its own GTK4 file without changing the IPC namespace`, () => {
            const source = read(`niri/panels/${panel}.nix`);
            assert.match(source, /ags = \(import \.\/default\.nix\)\.mkAgsRuntime\s*\{/);
            assert.doesNotMatch(source, /\$\{(?:lib\.getExe ags|ags)\}(?:\/bin\/ags)?\s+run\b/);
            assert.doesNotMatch(source, /--gtk4\b/, 'the bundle command takes --gtk 4, not run\'s --gtk4');
            assert.doesNotMatch(source, /(?:^|\n)\s*(?:export\s+)?XDG_RUNTIME_DIR=/);
            assert.doesNotMatch(source, /(?:^|[\s/])ags\.js\b/, 'never use AGS\'s shared temporary filename');
            assert.match(source, /runtime_dir="''\$\{XDG_RUNTIME_DIR:\?[^}]+\}\/ags-panels"/);
            assert.match(source, /\$\{pkgs\.coreutils\}\/bin\/install -d -m 700 "\$runtime_dir"/);
            assert.ok(source.includes(`bundle="$runtime_dir/${panel}-panel.js"`), 'bundle filenames must be panel-specific');
            assert.ok(source.includes('${lib.getExe ags} bundle --gtk 4 -- "$'
                + `${panel.toUpperCase()}_PANEL_CONFIG" "$bundle"`), 'compile the configured entry point as a single native argument');
            assert.match(source, /exec \$\{ags\}\/bin\/panel-gjs -m "\$bundle"/);
            assert.ok(source.includes(`request --instance ${panel}-panel -- "$request"`), 'requests still target the native instance');
        });
    }

    test('Waybar network middle-click toggles Wi-Fi without a monitor or a panel visibility command', () => {
        const source = read('niri/waybar/system.nix');
        const module = source.match(/networkModule = monitor: \{([\s\S]*?)\n  \};/);
        assert.ok(module, 'Waybar defines the shared network module');
        assert.ok(module[1].includes('on-click-middle = "${lib.getExe packages.networkPanel} toggleWifi";'));
        assert.ok(module[1].includes('on-click = "${lib.getExe packages.networkPanel} toggle ${lib.escapeShellArg monitor}";'),
            'Left-click still opens or closes the panel on its monitor');
    });

    test('companion GJS wrapper reuses the customized AGS GI/pixbuf environment and GTK4 layer shell', () => {
        const source = read('niri/panels/default.nix');
        assert.match(source, /ags\.overrideAttrs\s*\(old:/, 'extend the supplied per-panel AGS package');
        assert.match(source, /postFixup = \(old\.postFixup or ""\) \+/);
        assert.match(source, /makeWrapper \$\{lib\.getExe pkgs\.gjs\} "\$out\/bin\/panel-gjs"/);
        assert.ok(source.includes('"\'\'${gappsWrapperArgs[@]}"'), 'inherit the generated GI, library and pixbuf wrapper arguments');
        assert.match(source, /--set LD_PRELOAD "\$\{pkgs\.gtk4-layer-shell\}\/lib\/libgtk4-layer-shell\.so"/);
        assert.doesNotMatch(source, /(?:^|\n)\s*(?:export\s+)?XDG_RUNTIME_DIR=/);
    });

    for (const panel of ['media', 'dashboard']) {
        test(`${panel} passes its custom union of pixbuf loaders into the companion runtime`, () => {
            const source = read(`niri/panels/${panel}.nix`);
            for (const loader of ['pkgs.gdk-pixbuf', 'pkgs.librsvg', 'pkgs.webp-pixbuf-loader', 'pkgs.libavif']) {
                assert.ok(source.includes(loader), `${loader} stays in the loader cache`);
            }
            assert.match(source, /GDK_PIXBUF_MODULEDIR="\$module_dir"/);
            assert.match(source, /\$\{lib\.getDev pkgs\.gdk-pixbuf\}\/bin\/gdk-pixbuf-query-loaders/);
            assert.match(source, /ags = \(import \.\/default\.nix\)\.mkAgsRuntime\s*\{\s*inherit pkgs;\s*ags = \(pkgs\.ags\.override/);
            assert.match(source, /postInstall = \(old\.postInstall or ""\) \+ ''\s*export GDK_PIXBUF_MODULE_FILE=\$\{pixbufLoaders\}/);
        });
    }

    const launcherPath = process.env.NETWORK_PANEL_LAUNCHER;

    test('built network launcher', {
        skip: launcherPath ? false : 'Set NETWORK_PANEL_LAUNCHER to the built network-panel executable.',
    }, async t => {
        const source = readFileSync(launcherPath, 'utf8');
        const calls = [...source.matchAll(/(?:^|\s)(\/[^\s"']+\/bin\/ags) (bundle|request|run)\b/g)];
        const runtimes = new Set(calls.map(match => match[1]));
        assert.equal(runtimes.size, 1, 'bundle and request must use the same explicit AGS package');
        assert.deepEqual(calls.map(match => match[2]).sort(), ['bundle', 'request']);
        const [ags] = runtimes;
        const gjs = ags.replace(/\/ags$/, '/panel-gjs');
        assert.ok(source.includes(`exec ${gjs} -m "$bundle"`));

        const fixture = mkdtempSync(join(tmpdir(), 'network-launcher-'));
        t.after(() => rmSync(fixture, { recursive: true, force: true }));
        const keys = ['NETWORK_PANEL_CONFIG', 'NETWORK_PANEL_PALETTE', 'NETWORK_CUSTOM_HELPER',
            'DASHBOARD_PANEL', 'MEDIA_PANEL', 'XDG_RUNTIME_DIR'];
        const fakes = new Map();
        // Neither executable imports GTK, Astal or NetworkManager. The fake compiler
        // writes an identity marker so the fake GJS can verify which file it read.
        for (const [program, path] of [['ags', ags], ['panel-gjs', gjs]]) {
            const fake = join(fixture, program);
            writeFileSync(fake, `#!${process.execPath}\n`
                + 'const fs = require("node:fs");\n'
                + `const program = ${JSON.stringify(program)}, keys = ${JSON.stringify(keys)};\n`
                + 'const argv = process.argv.slice(2);\n'
                + 'const data = {program, argv, env: Object.fromEntries(keys.map(key => [key, process.env[key]]))};\n'
                + 'if (program === "ags" && argv[0] === "bundle") {\n'
                + '  console.log(JSON.stringify(data));\n'
                + '  if (process.env.PANEL_TEST_BUNDLE_STATUS) {\n'
                + '    console.error("fixture compiler failed");\n'
                + '    process.exit(Number(process.env.PANEL_TEST_BUNDLE_STATUS));\n'
                + '  }\n'
                + '  fs.writeFileSync(argv.at(-1), JSON.stringify({config: argv.at(-2)}));\n'
                + '} else {\n'
                + '  if (program === "panel-gjs") data.bundle = JSON.parse(fs.readFileSync(argv.at(-1), "utf8"));\n'
                + '  console.log(JSON.stringify(data));\n'
                + '}\n');
            chmodSync(fake, 0o700);
            fakes.set(path, fake);
        }
        const launcher = join(fixture, 'network-panel');
        writeFileSync(launcher, source.replaceAll(gjs, () => fakes.get(gjs)).replaceAll(ags, () => fakes.get(ags)));
        chmodSync(launcher, 0o700);
        const env = { ...process.env, XDG_CONFIG_HOME: join(fixture, 'config with spaces'),
            XDG_RUNTIME_DIR: join(fixture, 'runtime with spaces') };
        for (const key of [...keys.filter(key => key !== 'XDG_RUNTIME_DIR'), 'PANEL_TEST_BUNDLE_STATUS']) delete env[key];
        mkdirSync(env.XDG_RUNTIME_DIR, { mode: 0o700 });
        const sharedBundle = join(env.XDG_RUNTIME_DIR, 'ags.js');
        writeFileSync(sharedBundle, 'another panel owns this shared-path sentinel');

        function run(args, overrides = {}) {
            const result = spawnSync(launcher, args, { env: { ...env, ...overrides }, encoding: 'utf8',
                timeout: 5000, maxBuffer: 1024 * 1024 });
            assert.ifError(result.error);
            assert.equal(result.signal, null);
            return result;
        }

        function success(args, overrides) {
            const result = run(args, overrides);
            assert.equal(result.status, 0, result.stderr);
            return result.stdout.trim().split('\n').map(line => JSON.parse(line));
        }

        function launch(overrides = {}) {
            const records = success(['run'], overrides);
            assert.equal(records.length, 2, 'compile once, then start GJS once');
            const [compiler, runtime] = records;
            assert.equal(compiler.program, 'ags');
            assert.equal(runtime.program, 'panel-gjs');
            const runtimeDir = overrides.XDG_RUNTIME_DIR ?? env.XDG_RUNTIME_DIR;
            const config = overrides.NETWORK_PANEL_CONFIG ?? `${env.XDG_CONFIG_HOME}/ags/network/frontend.js`;
            const bundle = join(runtimeDir, 'ags-panels', 'network-panel.js');
            assert.deepEqual(compiler.argv, ['bundle', '--gtk', '4', '--', config, bundle]);
            assert.deepEqual(runtime.argv, ['-m', bundle]);
            assert.deepEqual(runtime.bundle, { config });
            assert.equal(statSync(join(runtimeDir, 'ags-panels')).mode & 0o777, 0o700);
            for (const data of records) {
                assert.equal(data.env.XDG_RUNTIME_DIR, runtimeDir, 'the native socket namespace must not change');
                assert.equal(data.env.NETWORK_PANEL_CONFIG, config);
            }
            assert.equal(readFileSync(sharedBundle, 'utf8'), 'another panel owns this shared-path sentinel');
            return { compiler, runtime };
        }

        await t.test('validates commands and help without invoking the compiler or GJS', () => {
            for (const args of [[], ['unknown'], ['run', 'extra'], ['status', 'extra'], ['close', 'extra'], ['toggleWifi', 'DP-1'], ['toggleWifi', ''],
                ['toggle', 'DP-1', 'extra'], ['--help', 'extra'], ['-h', 'extra']]) {
                const result = run(args);
                assert.equal(result.status, 2, JSON.stringify(args));
                assert.match(result.stderr, /Usage:/);
                assert.equal(result.stdout, '');
            }
            for (const command of ['--help', '-h']) {
                const result = run([command]);
                assert.equal(result.status, 0);
                assert.match(result.stdout, /^Usage: network-panel/);
                assert.match(result.stdout, /toggleWifi/);
            }
            assert.equal(existsSync(join(env.XDG_RUNTIME_DIR, 'ags-panels')), false);
        });

        await t.test('requests retain the network instance, JSON payload and original runtime directory', () => {
            const marker = join(fixture, 'monitor-was-evaluated');
            for (const args of [['status'], ['close'], ['toggleWifi'], ['toggle'], ['toggle', ''], ['toggle', 'eDP-1'],
                ['toggle', 'Display with spaces'], ['toggle', `DP-'"; touch ${marker}; $(touch ${marker}); 🌐`]]) {
                const records = success(args);
                assert.equal(records.length, 1, 'requests neither compile nor launch GJS');
                const [data] = records;
                assert.equal(data.program, 'ags');
                assert.deepEqual(data.argv.slice(0, 4), ['request', '--instance', 'network-panel', '--']);
                assert.equal(data.argv.length, 5);
                assert.deepEqual(JSON.parse(data.argv[4]), { command: args[0], ...(args[1] ? { monitor: args[1] } : {}) });
                assert.equal(data.env.XDG_RUNTIME_DIR, env.XDG_RUNTIME_DIR);
            }
            assert.equal(existsSync(marker), false);
            assert.equal(existsSync(join(env.XDG_RUNTIME_DIR, 'ags-panels')), false);
        });

        await t.test('compiles the GTK4 frontend into the network-only bundle with store-backed defaults', () => {
            const { compiler, runtime } = launch();
            for (const data of [compiler, runtime]) {
                assert.match(data.env.NETWORK_PANEL_PALETTE, /^\/nix\/store\//);
                assert.match(data.env.NETWORK_CUSTOM_HELPER, /\/bin\/network-manager$/);

                assert.equal(data.env.DASHBOARD_PANEL, 'dashboard-panel');
                assert.equal(data.env.MEDIA_PANEL, 'media-panel');
            }
        });

        await t.test('preserves overrides and passes shell metacharacters as literal path arguments', () => {
            const marker = join(fixture, 'config-was-evaluated');
            const overrides = { NETWORK_PANEL_CONFIG: join(fixture, `custom '"; $(touch ${marker}); frontend.js`),
                NETWORK_PANEL_PALETTE: join(fixture, 'custom palette.json'), NETWORK_CUSTOM_HELPER: join(fixture, 'custom helper'),
                DASHBOARD_PANEL: join(fixture, 'custom dashboard'),
                MEDIA_PANEL: join(fixture, 'custom media'), XDG_RUNTIME_DIR: join(fixture, 'isolated runtime $HOME with spaces') };
            mkdirSync(overrides.XDG_RUNTIME_DIR, { mode: 0o700 });
            const { compiler, runtime } = launch(overrides);
            for (const data of [compiler, runtime]) assert.deepEqual(data.env, overrides);
            assert.equal(existsSync(marker), false);
            assert.equal(existsSync(join(overrides.XDG_RUNTIME_DIR, 'ags.js')), false);
        });

        await t.test('compiler failure never executes GJS or a stale network bundle', () => {
            const runtimeDir = join(fixture, 'failed compilation runtime');
            const bundleDir = join(runtimeDir, 'ags-panels');
            mkdirSync(bundleDir, { recursive: true, mode: 0o700 });
            writeFileSync(join(bundleDir, 'network-panel.js'), JSON.stringify({ config: 'stale frontend.js' }));
            const result = run(['run'], { XDG_RUNTIME_DIR: runtimeDir, PANEL_TEST_BUNDLE_STATUS: '41' });
            assert.equal(result.status, 41, result.stderr);
            assert.match(result.stderr, /fixture compiler failed/);
            const records = result.stdout.trim().split('\n').map(line => JSON.parse(line));
            assert.equal(records.length, 1, 'no GJS execution follows a failed compilation');
            assert.equal(records[0].program, 'ags');
            assert.equal(records[0].argv[0], 'bundle');
            assert.equal(records[0].env.XDG_RUNTIME_DIR, runtimeDir);
        });

        await t.test('an unavailable runtime directory fails before any compiler or GJS invocation', () => {
            const result = run(['run'], { XDG_RUNTIME_DIR: '' });
            assert.notEqual(result.status, 0);
            assert.match(result.stderr, /XDG_RUNTIME_DIR/);
            assert.equal(result.stdout, '');
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
        assert(error && expression.test(error.message), `Expected ${expression}, got ${error?.message}`);
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
        connection.get_version_id = () => 1;
        connection.secretFinishes = 0;
        connection.get_secrets_async = (setting, io, callback) => {
            assert(setting === NM.SETTING_WIRELESS_SECURITY_SETTING_NAME, 'Native secret setting name');
            later(() => callback(connection, { cancelled: io.is_cancelled() }), 5);
        };
        connection.get_secrets_finish = result => {
            connection.secretFinishes++;
            if (result.cancelled) throw new Error('sensitive cancelled password');
            return new GLib.Variant('a{sa{sv}}', { '802-11-wireless-security': {
                psk: new GLib.Variant('s', 'offline-secret'), 'wep-key0': new GLib.Variant('s', 'abcde'),
            } });
        };
        connection.update2 = (settings, flags, args, io, callback) => {
            assert(flags === NM.SettingsUpdate2Flags.TO_DISK, 'Native profile update flags');
            assert(args.lookup_value('version-id', null).get_uint64() === 1, 'Versioned profile update');
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

    async function automaticScanning() {
        const { client, wifi, backend } = fixture();
        const second = new Device('wlan-other');
        client.devices.push(second);
        const saved = profile('scan', 'psk');
        client.profiles = [saved]; wifi.aps = [new AP('scan', 'psk', 70, 1)];
        wifi.active = new Active(saved, 'scan'); wifi.active.state = NM.ActiveConnectionState.ACTIVATED;
        wifi.activeAP = wifi.aps[0]; wifi.state = NM.DeviceState.ACTIVATED;
        wifi.holdScans = second.holdScans = true;
        const controller = new NetworkController({ backend, createCancellable: () => new Gio.Cancellable(), encodeQr: async () => '' });
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
            await controller.qr(key);
            await tick();
            assert(controller.page === 'qr' && wifi.scans === 3 && second.scans === 3, 'Periodic native scans continue on the QR page');
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
        await grouping(); await automaticScanning(); await nativeRadioToggle();
        await nativeSettings(); await nativeSettingsEdges(); await profilesAndSecrets();
        await activationIdentityAndCancellation(); await activationLifecycle();
        print('Native network tests passed: security labels, libnm settings, grouping, devices, scans, profiles, secrets, exact ACTIVATED state, cancellation and teardown. No live NM mutations.');
    }
    let failed = false;
    try { await main(); }
    catch (error) { failed = true; logError(error); }
    System.exit(failed ? 1 : 0);
}

async function gjsRuntimeTests() {
    // Offline QR transport tests. Import the real backend exports without constructing a native client.
    // Run directly: panel-gjs -m test.js runtime
    // NETWORK_TEST_QR_HELPER optionally exercises a real QR-only helper as well.
    const { default: Gio } = await import('gi://Gio');
    const { default: GLib } = await import('gi://GLib');
    const { default: System } = await import('system');
    const { encodeQr, controllerRuntime, dismissOtherPanels } = await import('./backend.js');

    const PRIORITY = GLib.PRIORITY_DEFAULT;
    const MAX_REPLY = 2 * 1024 * 1024;
    const PRIVATE_MARKER = 'offline-runtime-private-sentinel';
    // Generated 1x1 RGBA PNGs, including correct chunk CRCs (no image service needed).
    const RED = 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg==';
    const BLUE = 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGNgYPj/HwADAgH/5ncLrgAAAABJRU5ErkJggg==';
    const GENERIC_ERRORS = new Set([
        'The QR request is too large.',
        'Could not generate the Wi-Fi QR code.',
        'Could not read the Wi-Fi QR image.',
        'Could not send the Wi-Fi QR request.',
        'Could not start the Wi-Fi QR encoder.',
        'The Wi-Fi QR encoder failed.',
        'The QR request was cancelled.',
    ]);

    class CheckFailure extends Error {}
    function check(condition, message) {
        // All messages are fixed test descriptions, never actual values or exceptions.
        if (!condition) throw new CheckFailure(message);
    }
    const pause = (milliseconds = 10) => new Promise(resolve => GLib.timeout_add(PRIORITY, milliseconds,
        () => { resolve(); return GLib.SOURCE_REMOVE; }));
    const exists = path => GLib.file_test(path, GLib.FileTest.EXISTS);
    function text(path) {
        const [ok, bytes] = GLib.file_get_contents(path);
        check(ok, 'Could not read a test fixture file.');
        return new TextDecoder().decode(bytes);
    }
    function report(path) { return exists(path) ? JSON.parse(text(path)) : null; }
    async function until(predicate, message, milliseconds = 2000) {
        const end = GLib.get_monotonic_time() + milliseconds * 1000;
        do {
            if (predicate()) return;
            await pause();
        } while (GLib.get_monotonic_time() < end);
        throw new CheckFailure(message);
    }
    async function bounded(promise, message, milliseconds = 2500) {
        let timer = 0;
        try {
            return await Promise.race([promise, new Promise((_, reject) => {
                timer = GLib.timeout_add(PRIORITY, milliseconds, () => {
                    timer = 0;
                    reject(new CheckFailure(message));
                    return GLib.SOURCE_REMOVE;
                });
            })]);
        } finally { if (timer) GLib.source_remove(timer); }
    }
    function removeTree(path) {
        const file = Gio.File.new_for_path(path);
        if (file.query_file_type(Gio.FileQueryInfoFlags.NOFOLLOW_SYMLINKS, null) === Gio.FileType.DIRECTORY) {
            const entries = file.enumerate_children('standard::name', Gio.FileQueryInfoFlags.NOFOLLOW_SYMLINKS, null);
            try {
                let entry;
                while ((entry = entries.next_file(null))) removeTree(GLib.build_filenamev([path, entry.get_name()]));
            } finally { entries.close(null); }
        }
        file.delete(null);
    }
    function executable(path, source) {
        GLib.file_set_contents(path, '#!/usr/bin/env node\n' + source);
        Gio.File.new_for_path(path).set_attribute_uint32('unix::mode', 0o700, Gio.FileQueryInfoFlags.NONE, null);
    }
    function childPids(pid) {
        const path = `/proc/${pid}/task/${pid}/children`;
        if (!exists(path)) return [];
        return text(path).trim().split(/\s+/).filter(Boolean);
    }

    async function worker(root) {
        const injected = GLib.build_filenamev([root, 'shell-must-not-run']);
        const request = {
            ssid: 'Offline ; " café \\ fixture', authentication: 'WPA',
            password: `${PRIVATE_MARKER}; $(touch ${injected}) \`touch ${injected}\` ' " \\ , :`, hidden: true,
        };
        const large = { ...request, password: request.password + 'é\\;:'.repeat(2200) };
        const operations = [];
        const fixtures = [];
        const environment = new Map(['NETWORK_CUSTOM_HELPER', 'DASHBOARD_PANEL', 'MEDIA_PANEL']
            .map(name => [name, GLib.getenv(name)]));
        const ownPid = String(Gio.Credentials.new().get_unix_pid());
        const originalChildren = new Set(childPids(ownPid));
        let serial = 0, passed = 0, failed = 0, skipped = 0;

        function fixture(body, { input = request, args = ['qr'] } = {}) {
            const id = ++serial;
            // Whitespace and shell punctuation are deliberately part of argv[0].
            const path = GLib.build_filenamev([root, `fake helper ${id} ; literal.cjs`]);
            const output = GLib.build_filenamev([root, `report-${id}.json`]);
            const gate = GLib.build_filenamev([root, `release-${id}`]);
            const wire = JSON.stringify(input);
            const digest = GLib.compute_checksum_for_string(GLib.ChecksumType.SHA256, wire, -1);
            // Standalone Node fixtures keep every pipe/exit test real without Python.
            executable(path, `const fs = require('node:fs');
    const crypto = require('node:crypto');
    const REPORT = ${JSON.stringify(output)}, GATE = ${JSON.stringify(gate)}, STATE = {};
    function record(fields) {
        Object.assign(STATE, fields);
        fs.writeFileSync(REPORT + '.tmp', JSON.stringify(STATE));
        fs.renameSync(REPORT + '.tmp', REPORT);
    }
    function receive(step = null) {
        let raw, reads = 0;
        if (step === null) {
            raw = fs.readFileSync(0);
            reads = 1;
        } else {
            const pieces = [], buffer = Buffer.alloc(step);
            for (;;) {
                const count = fs.readSync(0, buffer, 0, step, null);
                if (!count) break;
                pieces.push(Buffer.from(buffer.subarray(0, count)));
                reads++;
            }
            raw = Buffer.concat(pieces);
        }
        let value;
        try { value = JSON.parse(raw.toString('utf8')); }
        catch (_) { process.exit(91); }
        record({ stdin_ok: value !== null && typeof value === 'object' && !Array.isArray(value)
            && crypto.createHash('sha256').update(raw).digest('hex') === ${JSON.stringify(digest)},
            input_bytes: raw.length, reads });
        return value;
    }
    function emit(image = ${JSON.stringify(RED)}) { fs.writeFileSync(1, JSON.stringify({ image })); }
    function gate() {
        const end = performance.now() + 4000;
        const sleeper = new Int32Array(new SharedArrayBuffer(4));
        while (!fs.existsSync(GATE)) {
            if (performance.now() > end) process.exit(92);
            Atomics.wait(sleeper, 0, 0, 5);
        }
    }
    record({ pid: process.pid, argv_ok: JSON.stringify(process.argv.slice(2)) === ${JSON.stringify(JSON.stringify(args))},
        executable_ok: process.argv[1] === ${JSON.stringify(path)},
        cmdline_private: !fs.readFileSync('/proc/self/cmdline').includes(${JSON.stringify(PRIVATE_MARKER)}) });
    if (!STATE.argv_ok || !STATE.executable_ok) process.exit(93);
    ${body}
    `);
            const value = { path, output, gate, inputBytes: new TextEncoder().encode(wire).length };
            fixtures.push(value);
            return value;
        }
        function start(helper, input = request, io = new Gio.Cancellable()) {
            GLib.setenv('NETWORK_CUSTOM_HELPER', helper.path, true);
            const operation = { io, settled: false };
            // Attach both handlers immediately, even for pre-cancelled requests.
            operation.result = encodeQr(input, io).then(
                value => { operation.settled = true; return { ok: true, value }; },
                error => { operation.settled = true; return { ok: false, error }; });
            operations.push(operation);
            return operation;
        }
        async function success(operation, image = RED) {
            const result = await bounded(operation.result, 'The QR request did not complete promptly.');
            check(result.ok && result.value === image, 'The QR result must match the expected PNG exactly.');
        }
        async function failure(operation, expected = null) {
            const result = await bounded(operation.result, 'The failed or cancelled request did not settle promptly.');
            check(!result.ok, 'An invalid, failed or cancelled QR request must reject.');
            check(result.error instanceof Error && GENERIC_ERRORS.has(result.error.message),
                'The rejection must use a fixed generic error, never helper output.');
            check(!String(result.error).includes(PRIVATE_MARKER), 'A rejected request exposed a private fixture marker.');
            if (expected) check(result.error.message === expected, 'The failure used an unexpected generic error.');
        }
        async function ready(helper, predicate = value => Boolean(value)) {
            await until(() => { const value = report(helper.output); return value && predicate(value); },
                'The helper did not reach the expected pipe state.');
        }
        async function reaped(helper) {
            const value = report(helper.output);
            check(value && Number.isInteger(value.pid), 'The helper must report its process identity.');
            await until(() => !exists(`/proc/${value.pid}`), 'The helper exited but was not reaped.');
        }
        function privateTransport(helper) {
            const value = report(helper.output);
            check(value?.argv_ok && value.executable_ok && value.cmdline_private,
                'The executable must receive only literal argv and no credentials.');
            check(value?.stdin_ok && value.input_bytes === helper.inputBytes,
                'Credentials must arrive as the exact complete JSON stdin request.');
            check(!exists(injected), 'Credential or executable shell syntax must never execute.');
        }
        async function test(name, action) {
            try { await action(); passed++; print(`ok ${passed + failed} - ${name}`); }
            catch (error) {
                failed++;
                print(`not ok ${passed + failed} - ${name}`);
                print(error instanceof CheckFailure ? `  ${error.message}` : '  Unexpected exception (details suppressed for privacy).');
            }
        }

        try {
            await test('literal helper argv, private JSON stdin and valid PNG', async () => {
                const helper = fixture('receive()\nemit()');
                await success(start(helper));
                privateTransport(helper);
                await reaped(helper);
            });
            await test('UTF-8 stdin larger than 4096 bytes survives chunked writes', async () => {
                const helper = fixture('receive(257)\nemit()', { input: large });
                check(helper.inputBytes > 8192 && helper.inputBytes < 16 * 1024, 'The chunked input fixture must span multiple writes.');
                await success(start(helper, large));
                privateTransport(helper);
                check(report(helper.output).reads > 1, 'The helper must actually drain stdin in multiple reads.');
                await reaped(helper);
            });
            await test('full-duplex pipes allow stdout EOF before stdin is drained', async () => {
                const helper = fixture('emit()\nfs.closeSync(1)\nreceive(113)', { input: large });
                await success(start(helper, large));
                privateTransport(helper);
                await reaped(helper);
            });
            await test('success waits for process exit as well as stdout EOF', async () => {
                const helper = fixture('receive()\nemit()\nfs.closeSync(1)\nrecord({ stdout_closed: true })\ngate()');
                const operation = start(helper);
                await ready(helper, value => value.stdout_closed);
                await pause(30);
                check(!operation.settled, 'EOF alone must not resolve a still-running encoder.');
                GLib.file_set_contents(helper.gate, 'release');
                await success(operation);
                await reaped(helper);
            });
            await test('spawn failure hides an untrusted helper path', async () => {
                await failure(start({ path: GLib.build_filenamev([root, PRIVATE_MARKER]) }),
                    'Could not start the Wi-Fi QR encoder.');
            });
            await test('requests over 16 KiB reject before spawning', async () => {
                const helper = fixture('receive()\nemit()');
                await failure(start(helper, { ...request, password: PRIVATE_MARKER.repeat(1024) }), 'The QR request is too large.');
                await pause(30);
                check(!exists(helper.output), 'An oversized request must not launch the helper.');
            });

            const badReplies = [
                ['malformed JSON and credential-bearing stderr', 'const value = receive();\nfs.writeSync(2, value.password)\nfs.writeSync(1, value.password)'],
                ['error JSON with a reflected credential', 'const value = receive();\nfs.writeSync(1, JSON.stringify({ error: value.password }))'],
                ['nonzero exit even with an otherwise valid image', 'const value = receive();\nfs.writeSync(2, value.password)\nemit()\nprocess.exit(23)'],
                ['nonzero exit with reflected JSON and stderr', 'const value = receive();\nfs.writeSync(2, value.password)\nfs.writeSync(1, JSON.stringify({ error: value.password }))\nprocess.exit(2)'],
                ['empty reply', 'receive()'],
                ['invalid UTF-8 reply', 'receive()\nfs.writeSync(1, Buffer.from([255, 254]))'],
                ['JSON null', "receive()\nfs.writeSync(1, 'null')"],
                ['JSON array', "receive()\nfs.writeSync(1, '[]')"],
                ['missing image', "receive()\nfs.writeSync(1, '{}')"],
                ['non-string image', 'receive()\nfs.writeSync(1, JSON.stringify({ image: 42 }))'],
                ['untrusted text instead of an image URL', 'const value = receive();\nemit(value.password)'],
                ['wrong image MIME type', "receive()\nemit('data:image/svg+xml;base64,PHN2Zz4=')"],
                ['empty base64 image', "receive()\nemit('data:image/png;base64,')"],
                ['invalid base64 alphabet', "receive()\nemit('data:image/png;base64,AAAA!AAA')"],
                ['invalid base64 padding', "receive()\nemit('data:image/png;base64,AA=A')"],
                ['impossible base64 length', "receive()\nemit('data:image/png;base64,A')"],
                ['base64 data that is not a PNG', "receive()\nemit('data:image/png;base64,bm90IGEgcG5n')"],
            ];
            for (const [name, body] of badReplies) {
                await test(`rejects ${name} with a generic error`, async () => {
                    const helper = fixture(body);
                    await failure(start(helper));
                    await reaped(helper);
                });
            }
            await test('a valid JSON reply exactly at the 2 MiB boundary succeeds', async () => {
                const helper = fixture(`receive()
    const reply = Buffer.from(JSON.stringify({ image: ${JSON.stringify(RED)} }));
    fs.writeFileSync(1, Buffer.concat([reply, Buffer.alloc(${MAX_REPLY} - reply.length, 32)]));`);
                await success(start(helper));
                await reaped(helper);
            });
            await test('a reply over 2 MiB is rejected before EOF and the child is killed/reaped', async () => {
                const helper = fixture(`receive()
    const reply = Buffer.from(JSON.stringify({ image: ${JSON.stringify(RED)} }));
    fs.writeFileSync(1, Buffer.concat([reply, Buffer.alloc(${MAX_REPLY} + 1 - reply.length, 32)]));
    gate()`);
                await failure(start(helper), 'Could not read the Wi-Fi QR image.');
                await reaped(helper);
            });
            await test('early helper exit without reading stdin is handled and reaped', async () => {
                const helper = fixture('process.exit(19)', { input: large });
                await failure(start(helper, large));
                await reaped(helper);
            });
            await test('a real broken stdin pipe rejects without exposing helper diagnostics', async () => {
                const helper = fixture('fs.closeSync(0)\nrecord({ stdin_closed: true })\ngate()', { input: large });
                const operation = start(helper, large);
                // Do not dispatch the first write completion until the child closes fd 0.
                // This makes a subsequent 4096-byte write hit EPIPE even on 64 KiB pipes.
                const end = GLib.get_monotonic_time() + 2000 * 1000;
                while (!report(helper.output)?.stdin_closed && GLib.get_monotonic_time() < end) GLib.usleep(1000);
                check(report(helper.output)?.stdin_closed, 'The helper must close stdin before the next write callback.');
                await failure(operation, 'Could not send the Wi-Fi QR request.');
                await reaped(helper);
            });
            await test('pre-cancelled Gio.Cancellable rejects without launching a child', async () => {
                const helper = fixture('receive()\nemit()');
                const io = new Gio.Cancellable();
                io.cancel();
                await failure(start(helper, request, io), 'Could not start the Wi-Fi QR encoder.');
                await pause(30);
                check(!exists(helper.output), 'Pre-cancellation must not launch an encoder.');
            });
            await test('cancelling a pending read returns without deadlock and reaps the child', async () => {
                const helper = fixture('receive()\nrecord({ waiting: true })\ngate()');
                const operation = start(helper);
                await ready(helper, value => value.waiting);
                operation.io.cancel();
                check(operation.io.is_cancelled(), 'Gio cancellation must return to its caller.');
                await failure(operation);
                await reaped(helper);
                operation.io.cancel();
                await pause();
            });
            await test('immediate cancellation settles pending pipe callbacks and reaps the child', async () => {
                const helper = fixture('receive()\ngate()', { input: large });
                const operation = start(helper, large);
                const spawned = childPids(ownPid).filter(pid => !originalChildren.has(pid));
                operation.io.cancel();
                await failure(operation);
                await until(() => spawned.every(pid => !exists(`/proc/${pid}`)), 'Immediate cancellation left an unreaped child.');
            });
            await test('cancelling after success cannot disturb a later independent request', async () => {
                const helper = fixture('receive()\nemit()');
                const operation = start(helper);
                await success(operation);
                operation.io.cancel();
                await reaped(helper);
                const next = fixture(`receive()\nemit(${JSON.stringify(BLUE)})`);
                await success(start(next), BLUE);
                await reaped(next);
            });
            await test('concurrent encodes preserve helper, stdin, output and cancellation identity', async () => {
                const other = { ...request, ssid: 'Independent offline request', hidden: false };
                const first = fixture('receive()\nrecord({ waiting: true })\ngate()\nemit()');
                const cancelled = fixture('receive()\nrecord({ waiting: true })\ngate()');
                const second = fixture(`receive(97)\nemit(${JSON.stringify(BLUE)})`, { input: other });
                const a = start(first), c = start(cancelled), b = start(second, other);
                await ready(first, value => value.waiting);
                await ready(cancelled, value => value.waiting);
                await success(b, BLUE);
                check(!a.settled && !c.settled, 'One successful request must not settle its neighbours.');
                c.io.cancel();
                await failure(c);
                check(!a.settled, 'Cancelling one request must not cancel another.');
                GLib.file_set_contents(first.gate, 'release');
                await success(a);
                for (const helper of [first, second, cancelled]) {
                    privateTransport(helper);
                    await reaped(helper);
                }
            });
            await test('controllerRuntime exposes the encoder and real independent Gio cancellables', async () => {
                const runtime = controllerRuntime();
                check(runtime.encodeQr === encodeQr, 'The controller must use the imported runtime encoder.');
                const a = runtime.createCancellable(), b = runtime.createCancellable();
                check(a instanceof Gio.Cancellable && b instanceof Gio.Cancellable && a !== b,
                    'Controller requests must receive distinct native cancellables.');
                a.cancel();
                check(a.is_cancelled() && !b.is_cancelled(), 'Native cancellation must remain request-local.');
            });
            await test('dismissOtherPanels closes exactly the fake dashboard and media launchers', async () => {
                const dashboard = fixture('', { args: ['close'] });
                const media = fixture('', { args: ['close'] });
                GLib.setenv('DASHBOARD_PANEL', dashboard.path, true);
                GLib.setenv('MEDIA_PANEL', media.path, true);
                dismissOtherPanels();
                for (const helper of [dashboard, media]) {
                    await ready(helper);
                    check(report(helper.output).argv_ok && report(helper.output).executable_ok,
                        'Each panel must receive only the literal close argument.');
                    await reaped(helper);
                }
            });
            await test('a failed fake dashboard launch does not prevent closing fake media', async () => {
                const media = fixture('', { args: ['close'] });
                GLib.setenv('DASHBOARD_PANEL', GLib.build_filenamev([root, 'missing-dashboard']), true);
                GLib.setenv('MEDIA_PANEL', media.path, true);
                dismissOtherPanels();
                await ready(media);
                check(report(media.output).argv_ok, 'Media must still receive close after a dashboard spawn failure.');
                await reaped(media);
            });
            const actual = GLib.getenv('NETWORK_TEST_QR_HELPER');
            if (actual) {
                await test('optional provided QR helper returns an offline PNG', async () => {
                    const result = await bounded(start({ path: actual }).result, 'The provided helper did not complete promptly.');
                    check(result.ok && typeof result.value === 'string' && result.value.startsWith('data:image/png;base64,'),
                        'The provided helper must return a PNG data URL.');
                    const image = GLib.base64_decode(result.value.slice('data:image/png;base64,'.length));
                    check(image.length > 8 && [137, 80, 78, 71, 13, 10, 26, 10].every((byte, i) => image[i] === byte),
                        'The provided helper must return bytes with the PNG signature.');
                });
            } else {
                skipped++;
                print('skip - optional real QR helper (NETWORK_TEST_QR_HELPER is unset)');
            }
            skipped++;
            print('skip - fixed 12-second timeout (not injectable; short failure/cancellation watchdogs are exercised)');
        } finally {
            await test('all test requests settle and every spawned child is reaped', async () => {
                for (const operation of operations) if (!operation.settled) operation.io.cancel();
                await bounded(Promise.all(operations.map(operation => operation.result)), 'Cleanup left an unsettled QR request.');
                // Release any intentionally gated fixture after a failed assertion.
                for (const helper of fixtures) GLib.file_set_contents(helper.gate, 'release');
                await until(() => childPids(ownPid).every(pid => originalChildren.has(pid)), 'Cleanup left a child process or zombie.');
                for (const helper of fixtures) {
                    const value = report(helper.output);
                    if (value) check(!exists(`/proc/${value.pid}`), 'A fixture process was not reaped.');
                }
                check(!exists(injected), 'A shell-injection fixture unexpectedly executed.');
            });
            for (const [name, value] of environment) {
                if (value === null) GLib.unsetenv(name);
                else GLib.setenv(name, value, true);
            }
        }
        print(`RESULT ${passed} passed, ${failed} failed, ${skipped} skipped`);
        return failed ? 1 : 0;
    }

    async function supervise() {
        // A separate GJS process is essential: a Cancellable.disconnect deadlock
        // would also block an in-process GLib timeout. Capture logs before printing
        // anything so even a regressed runtime cannot leak fixture credentials.
        const root = GLib.dir_make_tmp('network-runtime-tests-XXXXXX');
        let child, timer = 0, timedOut = false, code = 1;
        try {
            const launcher = Gio.SubprocessLauncher.new(Gio.SubprocessFlags.STDOUT_PIPE | Gio.SubprocessFlags.STDERR_PIPE);
            launcher.setenv('NETWORK_RUNTIME_TEST_WORKER', root, true);
            const source = GLib.filename_from_uri(import.meta.url)[0];
            child = launcher.spawnv([GLib.file_read_link('/proc/self/exe'), '-m', source, 'runtime']);
            timer = GLib.timeout_add(PRIORITY, 20000, () => {
                timer = 0;
                timedOut = true;
                child.force_exit();
                return GLib.SOURCE_REMOVE;
            });
            const [stdout, stderr] = await new Promise((resolve, reject) => child.communicate_utf8_async(null, null, (process, result) => {
                try { const [, out, err] = process.communicate_utf8_finish(result); resolve([out, err]); }
                catch (_) { reject(new CheckFailure('Could not collect the isolated runtime test output.')); }
            }));
            if (timer) { GLib.source_remove(timer); timer = 0; }
            if (timedOut) {
                print('FAIL runtime worker exceeded its 20-second deadlock watchdog.');
                // Fixtures have their own four-second lifetime bound if their parent dies.
                await pause(4200);
            } else if ((stdout + stderr).includes(PRIVATE_MARKER)) {
                print('FAIL captured process logs exposed a private fixture marker; output suppressed.');
            } else {
                if (stdout.trim()) print(stdout.trim());
                if (stderr.trim()) print(stderr.trim());
                print('PASS captured stdout/stderr contain no credential marker (including suppressed helper diagnostics)');
                code = child.get_successful() ? 0 : 1;
            }
        } catch (_) { print('FAIL runtime supervisor (exception details suppressed for privacy).'); }
        finally {
            if (timer) GLib.source_remove(timer);
            try {
                removeTree(root);
                check(!exists(root), 'Temporary fixture cleanup failed.');
                print('PASS temporary executable scripts and report files removed');
            } catch (_) { print('FAIL temporary fixture cleanup'); code = 1; }
        }
        return code;
    }

    const root = GLib.getenv('NETWORK_RUNTIME_TEST_WORKER');
    let exitCode = 1;
    try { exitCode = root ? await worker(root) : await supervise(); }
    catch (_) { print('FAIL runtime tests (exception details suppressed for privacy).'); }
    System.exit(exitCode);
}

async function gjsUITests() {
    // Run through node test.js --ui: real frontend/backend/logic imports,
    // fake controller actions, authenticated private Xvfb and isolated D-Bus.
    const { default: GLib } = await import('gi://GLib');
    const { default: Gio } = await import('gi://Gio');
    const { default: Gtk } = await import('gi://Gtk?version=4.0');
    const { default: Gdk } = await import('gi://Gdk?version=4.0');
    const { default: GdkPixbuf } = await import('gi://GdkPixbuf');
    const { default: System } = await import('system');

    if (GLib.getenv('NETWORK_UI_ISOLATED') !== '1'
        || GLib.getenv('NETWORK_PANEL_IMPORT_ONLY') !== '1'
        || GLib.getenv('GDK_BACKEND') !== 'x11'
        || GLib.getenv('DBUS_SYSTEM_BUS_ADDRESS') !== GLib.getenv('DBUS_SESSION_BUS_ADDRESS'))
        throw new Error('Use node test.js --ui, with a private Xvfb and both bus addresses isolated.');

    const { NetworkPanel, NetworkApplication, loadTheme, monitorFor } = await import(
        GLib.filename_to_uri(GLib.getenv('NETWORK_PANEL_CONFIG'), null));
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
                selectedRow: null, forgetKey: '', pendingKey: '', details: {}, secret: '', qrImage: '',
                secretLoading: false, qrLoading: false, secretRequested: false, calls: [] });
        }
        emit() { this.onChange?.(); }
        record(...args) { this.calls.push(args); }
        toggle(monitor = '') { this.record('toggle', monitor); this.opened = !this.opened || monitor !== this.monitor; this.monitor = monitor; this.emit(); }
        close() { this.record('close'); this.opened = false; this.emit(); }
        back() { this.record('back'); this.page = 'list'; this.selectedRow = null; this.emit(); }
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
        requestForget(key) { this.record('forget', key); this.forgetKey = key; this.emit(); }
        confirmForget() { this.record('confirmForget'); this.forgetKey = ''; this.emit(); }
        cancelForget() { this.record('cancelForget'); this.forgetKey = ''; this.emit(); }
        info(key) { this.record('info', key); this.page = 'info'; this.selectedRow = this.rows.find(row => row.key === key); this.emit(); }
        qr(key) { this.record('qr', key); this.page = 'qr'; this.selectedRow = this.rows.find(row => row.key === key); this.emit(); }
        revealSecret() { this.record('reveal'); this.secretRequested = true; this.secret = '<b>literal secret</b>'; this.emit(); }
        hideSecret() { this.record('hide'); this.secretRequested = false; this.secret = ''; this.emit(); }
        status() { return '{"text":"offline","class":"wifi"}'; }
    }

    let panel, host;
    function visibleNetworkOrder() {
        const keys = new Map([...panel.rows.values()].map(row => [row.widget, row.key]));
        const order = [];
        for (let child = panel.networkRows.get_first_child(); child; child = child.get_next_sibling()) {
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
        panel = new NetworkPanel({ app, controller: c, palette, dismiss: () => { dismissals++; } });
        c.onChange = () => panel.render();
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
            equal([host.get_width(), host.get_height()], [width, height], `Host remains ${width}x${height}`);
            const cardW = Math.max(0, Math.min(420, width - 16)), cardH = Math.max(0, Math.min(520, height - 54));
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
        print('PASS initial layout, monitor resolution and opening-only handoff');
        assert(panel.pages.get_height() > 200, 'Network list occupies the remaining card height');
        const tree = [...widgets(panel.root)];
        assert(panel.content.get_first_child() === panel.wifiNotice, 'Content begins with network notices, with no empty header row');
        assert(!tree.some(widget => widget instanceof Gtk.Label && widget.label === 'NETWORK'), 'No NETWORK title is present in the native widget tree');
        assert(!tree.some(widget => widget instanceof Gtk.Button
            && (/^(?:Wi-Fi (?:on|off)|↻|×|Advanced(?:…|\.{3})?)$/.test(widget.get_child()?.label || widget.label || '')
                || widget.has_css_class('wifi-toggle') || ['Refresh networks', 'Close network panel'].includes(widget.tooltip_text))),
        'No global Wi-Fi, manual scan, close or Advanced button is present in the native widget tree');
        const wifi = panel.rows.get('wifi');
        assert(wifi.name.label.includes('<b>Literal SSID</b>') && !wifi.name.use_markup, 'SSID is always literal text');
        assert(wifi.profile.visible && !panel.rows.get('wired').profile.visible, 'Profile chooser appears only for multiple profiles');
        wifi.profile.selected = 2;
        equal(panel.profileChoices.get('wifi'), 'uuid-b', 'Duplicate profile names select by UUID');
        equal(wifi.connect.get_child().label, 'Disconnect', 'Connected network displays Disconnect');
        wifi.connect.emit('clicked');
        equal(c.calls.at(-1), ['disconnect', 'wifi'], 'Connected row dispatches disconnect by stable key');
        // Replace the live row without rendering: the callback must not trust the
        // old delegate data or its still-visible Disconnect label.
        c.rows = c.rows.map(row => row.key === 'wifi' ? { ...row, connected: false } : row);
        wifi.connect.emit('clicked');
        equal(c.calls.at(-1), ['connect', 'wifi', 'uuid-b'], 'Disconnected live row dispatches connect with the selected UUID');
        c.emit();
        equal(wifi.connect.get_child().label, 'Connect', 'Disconnected network displays Connect');
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
            for (const row of [wifi, unsaved, joining, wired]) {
                assert(row.widget.get_first_child() === row.heading && row.heading.orientation === Gtk.Orientation.HORIZONTAL
                    && row.name.get_parent() === row.heading && row.connect.get_parent() === row.heading
                    && row.heading.get_first_child() === row.name && row.heading.get_last_child() === row.connect,
                'The name and Connect/Disconnect button share the top horizontal row');
                assert(![...widgets(row.actions)].includes(row.connect), 'Connect is not repeated in the secondary action row');
            }
            const [inline, connectBounds] = unsaved.connect.compute_bounds(unsaved.heading);
            assert(inline && Math.abs(connectBounds.get_x() + connectBounds.get_width() - unsaved.heading.get_width()) <= 1,
                'The inline Connect button is right-aligned in the normal viewport');
            assert(unsaved.name.wrap && unsaved.name.label === 'Wi-Fi · Never connected', 'Inline network names retain wrapping and the Wi-Fi prefix');
            for (const row of [unsaved, joining]) {
                assert(!row.forget.visible && !row.forget.sensitive, 'Never-connected Wi-Fi has no visible or enabled Forget button');
                assert(!row.info.visible && !row.qr.visible && !row.profile.visible && !row.actions.visible,
                    'Unsaved Wi-Fi has no connected-only actions, profile chooser or empty secondary row');
            }
            assert(unsaved.connect.sensitive && !joining.connect.sensitive, 'An unsaved changing network stays in the available group with Connect disabled');
            unsaved.connect.emit('clicked');
            equal(c.calls.at(-1), ['connect', 'new-wifi', ''], 'Unsaved inline Connect dispatches the current key without a saved UUID');
            assert(!connected.forget.visible && !connected.forget.sensitive && connected.info.visible && connected.qr.visible && connected.actions.visible,
                'A connected-only Wi-Fi network is remembered for grouping, but has no saved profile to forget');
            assert(wired.name.label === 'Wired · Saved Ethernet' && wired.forget.visible && wired.forget.sensitive
                && wired.actions.visible && !wired.info.visible && !wired.qr.visible, 'Disconnected saved Ethernet retains Forget but no connected-only controls');
            wired.connect.emit('clicked');
            equal(c.calls.at(-1), ['connect', 'saved-wired', ''], 'Saved Ethernet still dispatches native default-profile activation');
            wired.forget.emit('clicked');
            equal(c.calls.at(-1), ['forget', 'saved-wired'], 'Saved Ethernet Forget still dispatches its own key');
            panel.forgetCancel.emit('clicked');
            const liveWired = panel.rows.get('wired');
            assert(liveWired.forget.visible && liveWired.info.visible && !liveWired.qr.visible, 'Connected saved Ethernet keeps Forget and Info, never Wi-Fi QR');
            liveWired.connect.emit('clicked');
            equal(c.calls.at(-1), ['disconnect', 'wired'], 'Connected Ethernet still dispatches Disconnect');

            c.rows = c.rows.map(row => row.key === 'wifi' ? { ...row, known: false, connected: false } : row); c.emit();
            assertNetworkOrder(['saved-wired', 'connected-only', 'wired', 'divider', 'new-wifi', 'wifi', 'joining-wifi'],
                'Losing known and connected state moves the existing row across the divider');
            assert(panel.rows.get('wifi') === wifi && wifi.profile.get_model() === model && panel.profileChoices.get('wifi') === 'uuid-b',
                'Cross-group moves preserve the widget, unchanged profile model and selected UUID');
            assert(!wifi.forget.visible && !wifi.forget.sensitive && !wifi.actions.visible, 'A newly unsaved row hides Forget and its empty secondary action row');
            wifi.connect.emit('clicked');
            equal(c.calls.at(-1), ['connect', 'wifi', 'uuid-b'], 'The moved inline Connect retains UUID-based dispatch');
            c.rows = c.rows.map(row => row.key === 'wifi' ? { ...row, known: true } : row); c.emit();
            assertNetworkOrder(['wifi', 'saved-wired', 'connected-only', 'wired', 'divider', 'new-wifi', 'joining-wifi'],
                'A newly saved disconnected row returns to the remembered group');
            assert(wifi.forget.visible && wifi.forget.sensitive && wifi.actions.visible && !wifi.info.visible && !wifi.qr.visible,
                'Saving restores Forget without exposing connected-only actions');
            assert(panel.rows.get('wifi') === wifi && wifi.profile.get_model() === model && panel.profileChoices.get('wifi') === 'uuid-b',
                'Saving does not recreate the row or lose its selected UUID');

            c.rows = c.rows.map(row => row.key === 'new-wifi' ? { ...row, connected: true } : row); c.emit();
            assertNetworkOrder(['new-wifi', 'wifi', 'saved-wired', 'connected-only', 'wired', 'divider', 'joining-wifi'],
                'Connecting an unsaved network moves it above the divider without marking it saved');
            assert(panel.rows.get('new-wifi') === unsaved && unsaved.profile.get_model() === availableModel
                && !unsaved.forget.visible && !unsaved.forget.sensitive && unsaved.info.visible && unsaved.qr.visible && unsaved.actions.visible,
                'The connected unsaved row keeps its widgets and gains only connected actions');
            unsaved.connect.emit('clicked');
            equal(c.calls.at(-1), ['disconnect', 'new-wifi'], 'The moved row now dispatches Disconnect');
            c.rows = c.rows.map(row => row.key === 'new-wifi' ? { ...row, connected: false } : row);
            unsaved.connect.emit('clicked');
            equal(c.calls.at(-1), ['connect', 'new-wifi', ''], 'A stale Disconnect label still consults live data after a group change');
            c.emit();
            assertNetworkOrder(['wifi', 'saved-wired', 'connected-only', 'wired', 'divider', 'new-wifi', 'joining-wifi'],
                'Disconnecting an unsaved network restores available-group order');
            assert(!unsaved.actions.visible, 'Disconnecting the unsaved row removes the now-empty secondary actions');

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
        assert(panel.pages.visible_child_name === 'info' && !panel.password.text, 'Info page change wipes credentials');
        c.details = { ipv4: ['192.0.2.5/24'], dns: ['192.0.2.1', '2001:db8::1'], mode: { native: '<b>literal</b>' } }; c.emit();
        equal(panel.detailLabels.get('ipv4').label, 'IPv4: 192.0.2.5/24', 'Detail fields format native values');
        assert(panel.detailLabels.get('dns').label.includes('\n2001:db8::1') && !panel.summary.use_markup, 'Details use plain wrapped text');
        assert(panel.reveal.sensitive, 'Synchronous native details allow on-demand secret retrieval');
        c.secretLoading = true; c.emit();
        assert(panel.secretLoading.visible && !panel.reveal.sensitive, 'Secret authorization gates repeated retrieval');
        c.secretLoading = false; c.emit();
        panel.reveal.emit('clicked');
        assert(c.secretRequested && panel.secret.visible && panel.secret.label === '<b>literal secret</b>' && !panel.secret.use_markup, 'Reveal follows controller secretRequested, without markup');
        panel.reveal.emit('clicked');
        assert(!c.secretRequested && !panel.secret.label, 'Hide removes displayed secret');
        panel.reveal.emit('clicked'); panel.backButton.emit('clicked');
        assert(c.page === 'list' && !c.secretRequested && !panel.secret.label, 'Back hides the secret and returns to list');
        c.info('wired');
        assert(!panel.detailLabels.get('bssid').visible && !panel.reveal.sensitive, 'Wired info omits Wi-Fi fields and secret reveal');
        panel.backButton.emit('clicked');

        print('PASS Wi-Fi notices, forget confirmation and info/reveal/hide pages');
        wifi.qr.emit('clicked');
        c.qrLoading = true; c.emit();
        equal(panel.qrStatus.label, 'Generating QR code…', 'QR loading state');
        const image = GdkPixbuf.Pixbuf.new(GdkPixbuf.Colorspace.RGB, false, 8, 21, 21);
        image.fill(0xffffffff);
        const [ok, png] = image.save_to_bufferv('png', [], []);
        assert(ok, 'Offline QR fixture encoded');
        c.qrLoading = false; c.qrImage = `data:image/png;base64,${GLib.base64_encode(png)}`; c.emit();
        await wait();
        assert(panel.qrPixbuf && panel.qrStatus.label === 'Scan to join this network', 'QR PNG is decoded and rendered');
        c.qrImage = 'data:image/png;base64,not-a-png'; c.emit();
        equal(panel.qrStatus.label, 'Could not display QR code', 'Invalid QR data has a safe UI error');
        panel.backButton.emit('clicked');
        assert(!panel.qrPixbuf && !panel.qrKey, 'Leaving QR destroys its displayed image');

        print('PASS QR PNG rendering and page cleanup');
        c.error = '<b>Literal error</b> ' + 'An unusually long error '.repeat(60); c.emit();
        panel.root.grab_focus();
        await wait();
        equal([panel.card.get_width(), panel.card.get_height()], [420, 520], 'Root focus, long SSIDs and errors never enlarge the frame');
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
        assert(panel.empty.visible && panel.rows.size === 0 && panel.profileChoices.size === 0, 'Removed rows and their profile choices are cleaned up');
        assertNetworkOrder([], 'An empty network list has no visible rows or divider');
        assert(services.every(name => !owner(name)), 'No real service was started or mutated');
        print(`PASS network GTK UI: ${checks} assertions; fake controller only, no native network mutations`);
        print('LIMITATION Xvfb validates GTK layout/events, not compositor layer-shell placement or keyboard grabs.');
    }

    let exitCode = 0;
    try { await smoke(); }
    catch (error) { console.error(error); exitCode = 1; }
    finally {
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
            && controller.qrImage === '' && panel.password.get_text() === '' && panel.secret.label === '' && !panel.qrPixbuf,
        `${phase}: secrets, credentials and QR image remain empty`);
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
        assert(controller.secret === '' && controller.credentialKey === '' && controller.qrImage === '', 'Disposed controller retains no credential or QR state');
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
