// Node: node --test scripts/panels/test.js
// Native: GDK_BACKEND=x11 xvfb-run -a <panel-gjs> -m scripts/panels/test.js native
// Only Nix evaluation and isolated shell fixtures run in Node; no panel/service is started.
if (typeof process !== 'undefined' && process.versions?.node) {
    const { default: test, before, after, describe, it } = await import('node:test');
    const { default: assert } = await import('node:assert/strict');
    const { default: fs } = await import('node:fs');
    const { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } = fs;
    const { spawnSync } = await import('node:child_process');
    const { tmpdir } = await import('node:os');
    const { default: path } = await import('node:path');
    const { dirname, join, relative } = path;
    const { fileURLToPath } = await import('node:url');
    const { stripTypeScriptTypes } = await import('node:module');
    const projectRoot = path.resolve(fileURLToPath(new URL('../../', import.meta.url)));

    describe('style', () => {
        const source = readFileSync(new URL('./style.js', import.meta.url), 'utf8');

        function fixture({ override = '/live/theme/style.css', css = 'label { color: red; }', monitorError = false } = {}) {
            const files = new Map([[override || '/entry/style.css', css]]);
            const reads = [], providers = [], installed = [], logs = [], timers = new Map(), watchers = [];
            let serial = 0;
            class File {
                constructor(name) { this.path = name; }
                equal(other) { return this.path === other.path; }
                get_parent() { return new File(path.dirname(this.path)); }
                monitor_directory(flags, cancellable) {
                    assert.equal(flags, 8);
                    assert.equal(cancellable, null);
                    if (monitorError) throw new Error('monitor unavailable');
                    const watcher = { directory: this.path, connect(signal, callback) {
                        assert.equal(signal, 'changed'); this.callback = callback;
                    } };
                    watchers.push(watcher);
                    return watcher;
                }
            }
            const GLib = {
                getenv(name) { assert.equal(name, 'PANEL_CSS_FILE'); return override; },
                path_get_dirname: path.dirname, build_filenamev: parts => path.join(...parts),
                file_get_contents(name) {
                    reads.push(name);
                    if (!files.has(name)) throw new Error('CSS missing');
                    const text = files.get(name);
                    return text === false ? [false, null] : [true, new TextEncoder().encode(text)];
                },
                PRIORITY_DEFAULT: 0, SOURCE_REMOVE: false,
                timeout_add(priority, delay, callback) {
                    assert.equal(priority, 0); assert.equal(delay, 120);
                    timers.set(++serial, callback); return serial;
                },
                source_remove(id) { assert.ok(timers.delete(id)); },
            };
            class CssProvider {
                loads = [];
                constructor() { providers.push(this); }
                connect(signal, callback) { assert.equal(signal, 'parsing-error'); this.parseError = callback; }
                load_from_string(text) {
                    this.loads.push(text); this.css = text;
                    if (text.includes('invalid')) this.parseError(this, null, new Error('bad CSS'));
                    if (text.includes('throws')) throw new Error('load failed');
                }
            }
            const display = {};
            const Gtk = { CssProvider, STYLE_PROVIDER_PRIORITY_USER: 800,
                StyleContext: { add_provider_for_display(...args) { installed.push(args); } } };
            const Gdk = { Display: { get_default: () => display } };
            const Gio = { File: { new_for_path: name => new File(name) }, FileMonitorFlags: { WATCH_MOVES: 8 } };
            const loadPanelStyle = new Function('Gio', 'GLib', 'Gtk', 'Gdk', 'console',
                source.replace(/^import .*;\n/gm, '').replace('export function', 'function') + '\nreturn loadPanelStyle;')(
                Gio, GLib, Gtk, Gdk, { error: text => logs.push(text) });
            return { loadPanelStyle, files, reads, providers, installed, logs, timers, watchers,
                event(changed = override || '/entry/style.css', other = null) {
                    watchers[0].callback(watchers[0], changed && new File(changed), other && new File(other));
                },
                flush() {
                    const callbacks = [...timers.values()]; timers.clear();
                    for (const callback of callbacks) assert.equal(callback(), false);
                } };
        }

        test('override is used exclusively; fallback CSS is next to the entry; provider priority stays above desktop', () => {
            for (const override of ['/live/theme with spaces/style.css', null, '']) {
                const f = fixture({ override });
                const provider = f.loadPanelStyle({ entry: '/entry/frontend.js', label: 'Test' });
                assert.deepEqual(f.reads, [override || '/entry/style.css']);
                assert.deepEqual(f.installed.map(args => args.slice(1)), [[provider, 801]]);
                assert.equal(f.watchers[0].directory, path.dirname(f.reads[0]));
            }
            assert.ok(fixture().loadPanelStyle({ entry: null }));
            assert.throws(() => fixture({ override: null }).loadPanelStyle({ entry: null }), /entry file or PANEL_CSS_FILE/);
        });

        test('save bursts debounce; either side of atomic moves reloads the same provider repeatedly', () => {
            const f = fixture();
            const definitions = '@define-color test_accent #ffff00;';
            const provider = f.loadPanelStyle({ entry: '/entry/frontend.js', definitions });
            f.event('/live/theme/frontend.js'); f.event('/live/theme/palette.json'); f.event('/live/theme/.style.css.tmp');
            assert.equal(f.timers.size, 0, 'JS, palette and temporary files are not watched');
            f.files.set('/live/theme/style.css', 'label { color: blue; }');
            f.event(); f.event(); f.event('/live/theme/.style.css.tmp', '/live/theme/style.css');
            assert.equal(f.timers.size, 1);
            assert.equal(provider.loads.length, 1);
            f.flush();
            assert.equal(provider.css, `${definitions}\nlabel { color: blue; }`);
            f.files.set('/live/theme/style.css', 'label { color: green; }');
            f.event('/live/theme/style.css', '/live/theme/backup'); f.flush();
            assert.equal(provider.css, `${definitions}\nlabel { color: green; }`);
            f.event(); f.flush();
            assert.equal(provider.loads.length, 3, 'unchanged CSS does not reparse');
            assert.equal(f.providers.length, 1); assert.equal(f.installed.length, 1);
        });

        test('read and parse failures are logged, keep last valid CSS, and recover on later saves', () => {
            const f = fixture();
            const provider = f.loadPanelStyle({ entry: '/entry/frontend.js', label: 'Test' });
            const previous = provider.css;
            for (const failure of [null, false, 'invalid CSS', 'throws while loading']) {
                if (failure === null) f.files.delete('/live/theme/style.css');
                else f.files.set('/live/theme/style.css', failure);
                f.event(); f.flush();
                assert.equal(provider.css, previous);
            }
            assert.equal(f.logs.length, 4);
            assert.ok(f.logs.every(text => text.includes('Test stylesheet (/live/theme/style.css)')));
            f.files.set('/live/theme/style.css', 'label { color: green; }');
            f.event(); f.flush();
            assert.equal(provider.css, '\nlabel { color: green; }');
            assert.equal(f.providers.length, 1);
        });

        test('initial CSS failures still throw; monitor failures log without discarding loaded CSS', () => {
            for (const css of [false, 'invalid CSS']) {
                const f = fixture({ css });
                assert.throws(() => f.loadPanelStyle({ entry: '/entry/frontend.js' }));
                assert.equal(f.logs.length, 1); assert.equal(f.installed.length, 0);
            }
            const f = fixture({ monitorError: true });
            assert.ok(f.loadPanelStyle({ entry: '/entry/frontend.js' }));
            assert.equal(f.installed.length, 1);
            assert.match(f.logs[0], /Could not monitor CSS directory: monitor unavailable/);
        });

        test('all six frontends import the packaged relative helper; palette returns and definitions stay compatible', () => {
            const panels = [
                ['audio-control/astal/frontend.js', '../../panels/style.js', 'ap', 'Audio'],
                ['media-control/astal/frontend.js', '../../panels/style.js', 'mp', 'Media'],
                ['network-manager/astal/frontend.js', '../../panels/style.js', 'np', 'Network'],
                ['clipboard/astal/frontend.js', '../../panels/style.js', 'cp', 'Clipboard'],
                ['dashboard/notification.ts', '../panels/style.js', 'dp', 'Dashboard'],
                ['app-launcher/frontend.js', '../panels/style.js', null, 'Launcher'],
            ];
            const palette = Object.fromEntries(['background', 'secondaryBackground', 'chromeBackground', 'elevated',
                'foreground', 'separator', 'cyan', 'pink', 'dimPink', 'yellow', 'red'].map(key => [key, '#123456']));
            for (const [file, relative, prefix, label] of panels) {
                const frontend = new URL(`../${file}`, import.meta.url);
                let text = readFileSync(frontend, 'utf8');
                assert.ok(text.includes(`import { loadPanelStyle } from '${relative}';`), file);
                assert.equal(new URL(relative, frontend).href, new URL('./style.js', import.meta.url).href);
                assert.doesNotMatch(text, /new Gtk\.CssProvider|load_from_(?:path|string|file)/);
                if (!prefix) { assert.match(text, /provider = loadPanelStyle\(\{ entry: config, label: 'Launcher' \}\)/); continue; }
                const start = text.indexOf('function read(');
                let theme = text.slice(start, text.indexOf('\n}', text.indexOf('function loadTheme(')) + 2)
                    .replace(/export /g, '');
                if (file.endsWith('.ts')) theme = stripTypeScriptTypes(theme);
                let options;
                const provider = {};
                const loadTheme = new Function('GLib', 'loadPanelStyle', `${theme}\nreturn loadTheme;`)(
                    { file_get_contents(name) {
                        assert.equal(name, '/packaged/palette.json');
                        return [true, new TextEncoder().encode(JSON.stringify(palette))];
                    } }, value => { options = value; return provider; });
                const result = loadTheme('/packaged/frontend.js', '/packaged/palette.json');
                assert.deepEqual(options, { entry: '/packaged/frontend.js', label,
                    definitions: Object.keys(palette).map(key => `@define-color ${prefix}_${key} #123456;`).join('\n') });
                assert.deepEqual(result, label === 'Media' ? { palette } : label === 'Dashboard' ? { palette, provider } : palette);
            }
        });
    });

    describe('launcher', () => {
        const panels = [
            {
                key: 'appLauncher', name: 'app-launcher', prefix: 'APP_LAUNCHER',
                sourceDir: 'app-launcher', entryPoint: 'frontend.js',
                commands: ['run', 'toggle', 'close', 'status'],
                environment: { NIRI: 'pkgs.lib.getExe pkgs.niri' }, palette: false,
            },
            {
                key: 'audioPanel', name: 'audio-panel', prefix: 'AUDIO_PANEL',
                sourceDir: 'audio-control/astal', entryPoint: 'frontend.js',
                commands: ['run', 'status', 'toggle', 'close', 'bluetoothPower'],
                helper: ['audioControl', 'audio-control'],
                environment: { AUDIO_CONTROL_HELPER: 'pkgs.lib.getExe helpers.audioControl' },
            },
            {
                key: 'clipboardPanel', name: 'clipboard-panel', prefix: 'CLIPBOARD_PANEL',
                sourceDir: 'clipboard/astal', entryPoint: 'frontend.js', commands: ['run', 'status', 'toggle', 'close'],
                helper: ['clipboardCustom', 'clipboard-custom'], permissiveHelp: true, checkResponse: true,
                environment: { CLIPBOARD_CUSTOM_HELPER: 'pkgs.lib.getExe helpers.clipboardCustom' },
            },
            {
                key: 'dashboardPanel', name: 'dashboard-panel', prefix: 'DASHBOARD_PANEL',
                sourceDir: 'dashboard', entryPoint: 'power.ts',
                commands: ['run', 'status', 'toggle', 'close', 'toggleDnd', 'clearNotifications'],
                environment: { NETWORK_PANEL: '"network-panel"', COREUTILS_DF: '"${pkgs.coreutils}/bin/df"' },
            },
            {
                key: 'mediaPanel', name: 'media-panel', prefix: 'MEDIA_PANEL',
                sourceDir: 'media-control/astal', entryPoint: 'frontend.js',
                commands: ['run', 'status', 'toggle', 'togglePauseAll', 'cycleMedia', 'pauseAll', 'close'],
                helper: ['mediaControl', 'media-control'], permissiveHelp: true,
                environment: { MEDIA_CONTROL: 'pkgs.lib.getExe helpers.mediaControl', NETWORK_PANEL: '"network-panel"' },
            },
            {
                key: 'networkPanel', name: 'network-panel', prefix: 'NETWORK_PANEL',
                sourceDir: 'network-manager/astal', entryPoint: 'frontend.js',
                commands: ['run', 'status', 'toggle', 'close', 'toggleWifi'],
                helper: ['networkManager', 'network-manager'],
                environment: {
                    NETWORK_CUSTOM_HELPER: 'pkgs.lib.getExe helpers.networkManager',
                    DASHBOARD_PANEL: '"dashboard-panel"', MEDIA_PANEL: '"media-panel"',
                },
            },
        ];

        // JSON strings are also Nix strings after protecting Nix's interpolation syntax.
        const nixString = value => JSON.stringify(value).replaceAll('${', '\\${');
        const shellQuote = value => `'${value.replaceAll("'", "'\\''")}'`;
        let root, evaluated, bash, jq, install;

        function executable(name) {
            const result = spawnSync('bash', ['-c', 'command -v -- "$1"', 'launcher-test', name], {
                encoding: 'utf8', timeout: 10000,
            });
            assert.ifError(result.error);
            assert.equal(result.status, 0, `${name} is required: ${result.stderr}`);
            const resolved = result.stdout.trim();
            assert.ok(path.isAbsolute(resolved), `Expected an absolute executable for ${name}: ${resolved}`);
            return resolved;
        }

        function evaluatePanels() {
            assert.ok(fs.existsSync(path.join(projectRoot, 'niri/panels.nix')),
                'the consolidated panel module must exist (no git add is needed)');
            const helpers = panels.filter(panel => panel.helper).map(({ helper: [argument, name] }) =>
                `${argument} = pkgs.writeShellScriptBin ${nixString(name)} "exit 97";`).join('\n');
            const definitions = panels.map(panel => {
                const defaults = Object.entries(panel.environment).map(([name, value]) => `${name} = ${value};`).join('\n');
                return `${nixString(panel.key)} = let
            launcher = packages.${panel.key};
        in {
            inherit (launcher) text;
            entry = launcher.passthru.entry;
            source = toString launcher.passthru.source;
            ags = toString launcher.passthru.ags;
            agsExe = pkgs.lib.getExe launcher.passthru.ags;
            palette = if launcher.passthru ? palette then toString launcher.passthru.palette else null;
            defaults = { ${defaults} };
        };`;
            }).join('\n');
            const expression = `let
        lock = builtins.fromJSON (builtins.readFile (builtins.toPath ${nixString(path.join(projectRoot, 'flake.lock'))}));
        nixpkgs = builtins.fetchTree lock.nodes.\${lock.nodes.root.inputs.nixpkgs}.locked;
        pkgs = import nixpkgs.outPath {};
        # Capture source/passthru rather than realizing a shell application or Rust helpers.
        shellPkgs = pkgs // { writeShellApplication = attrs: attrs; };
        helpers = { ${helpers} };
        packages = (import (builtins.toPath ${nixString(path.join(projectRoot, 'niri/panels.nix'))})).packages
            (helpers // { pkgs = shellPkgs; repoRoot = ${nixString(projectRoot)}; });
    in {
        panels = { ${definitions} };
        jq = pkgs.lib.getExe pkgs.jq;
        install = "\${pkgs.coreutils}/bin/install";
    }`;
            const result = spawnSync('nix', ['eval', '--impure', '--json', '--expr', expression], {
                cwd: projectRoot, encoding: 'utf8', timeout: 120000, maxBuffer: 16 * 1024 * 1024,
            });
            assert.ifError(result.error);
            assert.equal(result.status, 0, `nix eval failed:\n${result.stderr}`);
            return JSON.parse(result.stdout);
        }

        before(() => {
            evaluated = evaluatePanels();
            bash = executable('bash');
            jq = fs.existsSync(evaluated.jq) ? evaluated.jq : executable('jq');
            install = fs.existsSync(evaluated.install) ? evaluated.install : executable('install');
            root = fs.mkdtempSync(path.join(tmpdir(), 'panel-launchers-'));
            const fake = `#!${process.execPath}
const fs = require('node:fs');
const path = require('node:path');
const args = process.argv.slice(2);
const kind = path.basename(process.argv[1]) === 'panel-gjs' ? 'gjs' : args[0];
fs.appendFileSync(process.env.TEST_LOG, JSON.stringify({ kind, args, env: process.env, pid: process.pid }) + '\\n');
const prefix = kind === 'gjs' ? 'GJS' : kind === 'bundle' ? 'BUNDLE' : 'REQUEST';
process.stdout.write(process.env['TEST_' + prefix + '_STDOUT'] || '');
process.stderr.write(process.env['TEST_' + prefix + '_STDERR'] || '');
const status = Number(process.env['TEST_' + prefix + '_EXIT'] || 0);
if (kind === 'bundle' && status === 0) fs.writeFileSync(args.at(-1), '// fake bundle\\n');
if (kind === 'gjs' && !fs.existsSync(args.at(-1))) process.exit(98);
process.exit(status);
`;
            for (const name of ['ags', 'panel-gjs']) fs.writeFileSync(path.join(root, name), fake, { mode: 0o700 });
            for (const panel of panels) {
                const metadata = evaluated.panels[panel.key];
                assert.match(metadata.entry, /^\/nix\/store\//);
                assert.equal(metadata.entry, path.join(metadata.source, panel.sourceDir, panel.entryPoint));
                assert.ok(fs.existsSync(metadata.entry), `${panel.name}: packaged entry exists`);
                assert.ok(fs.existsSync(path.join(metadata.source, 'panels/style.js')), 'shared style helper is packaged');
                assert.match(metadata.ags, /^\/nix\/store\//, `${panel.name}: retain the native runtime derivation`);
                assert.equal(metadata.agsExe, `${metadata.ags}/bin/ags`);
                assert.equal(metadata.palette !== null, panel.palette !== false, `${panel.name}: palette passthru`);
                let text = metadata.text;
                for (const [original, replacement] of [
                    [metadata.agsExe, path.join(root, 'ags')],
                    [`${metadata.ags}/bin/panel-gjs`, path.join(root, 'panel-gjs')],
                    [evaluated.jq, jq], [evaluated.install, install],
                ]) text = text.replaceAll(original, shellQuote(replacement));
                assert.ok(text.includes(shellQuote(path.join(root, 'ags'))), `${panel.name}: AGS intercepted`);
                assert.ok(text.includes(shellQuote(path.join(root, 'panel-gjs'))), `${panel.name}: GJS intercepted`);
                assert.doesNotMatch(text, /\/nix\/store\/[^\s"']*\/bin\/(?:ags|panel-gjs)\b/,
                    `${panel.name}: never execute a real AGS or GJS`);
                panel.script = path.join(root, `${panel.name}.sh`);
                // writeShellApplication's default strict flags, without a build or shellcheck.
                fs.writeFileSync(panel.script, `#!${bash}\nset -o errexit\nset -o nounset\nset -o pipefail\n${text}`, { mode: 0o700 });
            }
        });

        after(() => {
            if (root) fs.rmSync(root, { recursive: true, force: true });
        });

        function invoke(panel, args, overrides = {}, prepare = () => {}) {
            const directory = fs.mkdtempSync(path.join(root, 'case-'));
            for (const name of ['home', 'config', 'runtime']) fs.mkdirSync(path.join(directory, name), { mode: 0o700 });
            // Deliberately do not inherit display/bus addresses, user panel overrides, or LD_PRELOAD.
            const env = {
                PATH: process.env.PATH, HOME: path.join(directory, 'home'),
                XDG_CONFIG_HOME: path.join(directory, 'config'), XDG_RUNTIME_DIR: path.join(directory, 'runtime'),
                DBUS_SESSION_BUS_ADDRESS: `unix:path=${directory}/no-session-bus`,
                DBUS_SYSTEM_BUS_ADDRESS: `unix:path=${directory}/no-system-bus`,
                TEST_LOG: path.join(directory, 'calls.jsonl'), TEST_REQUEST_STDOUT: '{"ok":true}\n',
                ...overrides,
            };
            for (const key of Object.keys(env)) if (env[key] === undefined) delete env[key];
            prepare({ directory, env });
            const result = spawnSync(bash, [panel.script, ...args], { env, encoding: 'utf8', timeout: 10000 });
            assert.ifError(result.error);
            assert.equal(result.signal, null, `${panel.name} ${JSON.stringify(args)}: ${result.stderr}`);
            const calls = fs.existsSync(env.TEST_LOG)
                ? fs.readFileSync(env.TEST_LOG, 'utf8').trim().split('\n').map(line => JSON.parse(line)) : [];
            return { ...result, calls, directory, env };
        }

        function success(result) {
            assert.equal(result.status, 0, `stdout: ${result.stdout}\nstderr: ${result.stderr}`);
            assert.equal(result.stderr, '');
        }

        function usage(panel, output) {
            assert.match(output, /Usage:/i);
            assert.ok(output.includes(panel.name), `Usage identifies ${panel.name}: ${output}`);
            // Check advertised verbs and ordering, not whitespace, quoting, or shell source formatting.
            assert.deepEqual(output.match(/\b(?:run|status|toggle|close|bluetoothPower|toggleDnd|clearNotifications|togglePauseAll|cycleMedia|pauseAll|toggleWifi)\b/g),
                panel.commands);
            assert.match(output, /toggle\s+\[monitor\]/);
        }

        function request(panel, result, expected) {
            assert.equal(result.calls.length, 1);
            const call = result.calls[0];
            assert.equal(call.kind, 'request');
            assert.deepEqual(call.args.slice(0, 4), ['request', '--instance', panel.name, '--']);
            assert.equal(call.args.length, 5);
            assert.deepEqual(JSON.parse(call.args[4]), expected);
            return call;
        }

        function defaults(panel, call) {
            const metadata = evaluated.panels[panel.key];
            assert.equal(call.env[`${panel.prefix}_CONFIG`], metadata.entry);
            assert.equal(call.env.PANEL_CSS_FILE, path.join(projectRoot, 'scripts', panel.sourceDir, 'style.css'));
            if (panel.palette !== false) assert.equal(call.env[`${panel.prefix}_PALETTE`], metadata.palette);
            for (const [name, value] of Object.entries(metadata.defaults)) assert.equal(call.env[name], value, name);
        }

        for (const panel of panels) describe(panel.name, () => {
            it('advertises commands in the original order and preserves help arity policy', () => {
                for (const flag of ['-h', '--help']) {
                    const result = invoke(panel, [flag]);
                    success(result);
                    usage(panel, result.stdout);
                    assert.deepEqual(result.calls, []);
                    const extra = invoke(panel, [flag, 'extra', 'arguments']);
                    assert.equal(extra.status, panel.permissiveHelp ? 0 : 2);
                    usage(panel, panel.permissiveHelp ? extra.stdout : extra.stderr);
                    assert.equal(panel.permissiveHelp ? extra.stderr : extra.stdout, '');
                    assert.deepEqual(extra.calls, []);
                }
            });

            it('rejects no command, unknown commands, and invalid arity without invoking AGS', () => {
                const invalid = [[], ['not-a-command'], ['--'], ['toggle', 'one', 'two'],
                    ...panel.commands.filter(command => command !== 'toggle').map(command => [command, 'extra']),
                    ...['bluetoothPower', 'toggleDnd', 'clearNotifications', 'togglePauseAll', 'cycleMedia', 'pauseAll', 'toggleWifi']
                        .filter(command => !panel.commands.includes(command)).map(command => [command])];
                for (const args of invalid) {
                    const result = invoke(panel, args);
                    assert.equal(result.status, 2, JSON.stringify(args));
                    assert.equal(result.stdout, '');
                    usage(panel, result.stderr);
                    assert.deepEqual(result.calls, [], JSON.stringify(args));
                }
            });

            it('sends every native request to the correct instance with no spurious monitor', () => {
                for (const command of panel.commands.filter(command => command !== 'run')) {
                    const result = invoke(panel, [command]);
                    success(result);
                    assert.equal(result.stdout, '{"ok":true}\n');
                    const call = request(panel, result, { command });
                    defaults(panel, call);
                }
            });

            it('JSON-encodes optional monitor names safely, including spaces, quotes and shell syntax', () => {
                for (const monitor of ['', 'DP-1', 'HDMI A "quoted" \'single\' \\ $HOME ; $(false)\nnext line']) {
                    const result = invoke(panel, ['toggle', monitor]);
                    success(result);
                    request(panel, result, { command: 'toggle', ...(monitor ? { monitor } : {}) });
                }
            });

            it('only clipboard interprets a top-level JSON error; other replies remain raw', () => {
                const replies = [
                    ['{"error":"not running"}\n', true], ['{"error":null}\n', true],
                    ['{"ok":true}\n', false], ['{"nested":{"error":"not top-level"}}\n', false],
                    ['[{"error":"not an object"}]\n', false], ['null\n', false],
                    ['plain reply without newline', false], ['  raw reply\n\n', false], ['', false],
                ];
                for (const [reply, error] of replies) {
                    const result = invoke(panel, ['status'], { TEST_REQUEST_STDOUT: reply });
                    assert.equal(result.status, panel.checkResponse && error ? 1 : 0, JSON.stringify(reply));
                    assert.equal(result.stderr, '');
                    assert.equal(result.stdout, panel.checkResponse ? `${reply.replace(/\n+$/, '')}\n` : reply);
                    request(panel, result, { command: 'status' });
                }
            });

            it('passes through AGS request failures and diagnostics without turning them into success', () => {
                const result = invoke(panel, ['toggle', 'monitor'], {
                    TEST_REQUEST_EXIT: '37', TEST_REQUEST_STDOUT: 'transport output\n',
                    TEST_REQUEST_STDERR: 'fake AGS transport failure\n',
                });
                assert.equal(result.status, 37);
                assert.equal(result.stderr, 'fake AGS transport failure\n');
                assert.equal(result.stdout, panel.checkResponse ? '' : 'transport output\n');
                request(panel, result, { command: 'toggle', monitor: 'monitor' });
            });

            it('bundles GTK4 into a private runtime directory and execs companion GJS on the same bundle', () => {
                const result = invoke(panel, ['run'], {}, ({ env }) => {
                    // install -m must also repair a pre-existing directory with permissive permissions.
                    fs.mkdirSync(path.join(env.XDG_RUNTIME_DIR, 'ags-panels'), { mode: 0o755 });
                });
                success(result);
                assert.deepEqual(result.calls.map(call => call.kind), ['bundle', 'gjs']);
                const [bundle, gjs] = result.calls;
                const directory = path.join(result.env.XDG_RUNTIME_DIR, 'ags-panels');
                const output = path.join(directory, `${panel.name}.js`);
                assert.deepEqual(bundle.args, ['bundle', '--gtk', '4', '--',
                    evaluated.panels[panel.key].entry, output]);
                assert.deepEqual(gjs.args, ['-m', output]);
                assert.equal(gjs.pid, result.pid, 'GJS replaces the launcher process via exec');
                assert.equal(fs.statSync(directory).mode & 0o777, 0o700);
                assert.equal(fs.readFileSync(output, 'utf8'), '// fake bundle\n');
                defaults(panel, bundle);
                defaults(panel, gjs);
            });

            it('keeps the immutable entry independent of XDG_CONFIG_HOME, and empty overrides use defaults', () => {
                for (const configHome of [undefined, '']) {
                    const overrides = { XDG_CONFIG_HOME: configHome, [`${panel.prefix}_CONFIG`]: '', PANEL_CSS_FILE: '' };
                    if (panel.palette !== false) overrides[`${panel.prefix}_PALETTE`] = '';
                    for (const name of Object.keys(panel.environment)) overrides[name] = '';
                    const result = invoke(panel, ['status'], overrides);
                    success(result);
                    defaults(panel, request(panel, result, { command: 'status' }));
                }
            });

            it('forces the store entry while preserving CSS, palette, helper and companion-command overrides', () => {
                const overrides = { [`${panel.prefix}_CONFIG`]: '/custom config/quoted "entry" \'file\'.js',
                    PANEL_CSS_FILE: '/custom styles/quoted "style".css' };
                if (panel.palette !== false) overrides[`${panel.prefix}_PALETTE`] = '/custom palette/with spaces.json';
                for (const name of Object.keys(panel.environment)) overrides[name] = `/custom helpers/${name} "quoted"`;
                const result = invoke(panel, ['run'], overrides);
                success(result);
                assert.deepEqual(result.calls.map(call => call.kind), ['bundle', 'gjs']);
                assert.equal(result.calls[0].args[4], evaluated.panels[panel.key].entry);
                assert.notEqual(result.calls[0].args[4], overrides[`${panel.prefix}_CONFIG`]);
                for (const call of result.calls) {
                    for (const [name, value] of Object.entries(overrides)) {
                        assert.equal(call.env[name], name === `${panel.prefix}_CONFIG`
                            ? evaluated.panels[panel.key].entry : value, name);
                    }
                }
            });

            it('requires a nonempty XDG_RUNTIME_DIR for run but not for requests', () => {
                for (const runtime of [undefined, '']) {
                    const failed = invoke(panel, ['run'], { XDG_RUNTIME_DIR: runtime });
                    assert.notEqual(failed.status, 0);
                    assert.match(failed.stderr, /XDG_RUNTIME_DIR/);
                    assert.equal(failed.stdout, '');
                    assert.deepEqual(failed.calls, []);
                    const result = invoke(panel, ['status'], { XDG_RUNTIME_DIR: runtime });
                    success(result);
                    request(panel, result, { command: 'status' });
                }
            });

            it('stops on bundle failure and preserves GJS exit status and output', () => {
                const failed = invoke(panel, ['run'], {
                    TEST_BUNDLE_EXIT: '41', TEST_BUNDLE_STDERR: 'bundle failed\n',
                });
                assert.equal(failed.status, 41);
                assert.equal(failed.stderr, 'bundle failed\n');
                assert.deepEqual(failed.calls.map(call => call.kind), ['bundle']);
                const gjs = invoke(panel, ['run'], {
                    TEST_GJS_EXIT: '43', TEST_GJS_STDOUT: 'GJS output\n', TEST_GJS_STDERR: 'GJS failed\n',
                });
                assert.equal(gjs.status, 43);
                assert.equal(gjs.stdout, 'GJS output\n');
                assert.equal(gjs.stderr, 'GJS failed\n');
                assert.deepEqual(gjs.calls.map(call => call.kind), ['bundle', 'gjs']);
            });
        });
    });

    describe('packaging', () => {
        const repoRoot = '/checkout with spaces/System';
        const panels = [
            { key: 'audioPanel', name: 'audio-panel', prefix: 'AUDIO_PANEL', sourceDir: 'audio-control/astal', entry: 'frontend.js', service: 'audio-control-ui',
                deps: ['wireplumber', 'bluetooth'], helper: 'AUDIO_CONTROL_HELPER', binary: 'audio-control' },
            { key: 'mediaPanel', name: 'media-panel', prefix: 'MEDIA_PANEL', sourceDir: 'media-control/astal', entry: 'frontend.js', service: 'ags-media',
                deps: ['mpris', 'wireplumber'], helper: 'MEDIA_CONTROL', binary: 'media-control', images: true },
            { key: 'networkPanel', name: 'network-panel', prefix: 'NETWORK_PANEL', sourceDir: 'network-manager/astal', entry: 'frontend.js', service: 'ags-network',
                deps: ['network'], helper: 'NETWORK_CUSTOM_HELPER', binary: 'network-manager' },
            { key: 'dashboardPanel', name: 'dashboard-panel', prefix: 'DASHBOARD_PANEL', sourceDir: 'dashboard', entry: 'power.ts', service: 'ags-dashboard',
                deps: ['battery', 'powerprofiles', 'network', 'bluetooth', 'notifd'], images: true },
            { key: 'appLauncher', name: 'app-launcher', prefix: 'APP_LAUNCHER', sourceDir: 'app-launcher', entry: 'frontend.js', service: 'ags-app-launcher',
                deps: ['apps'], palette: false },
            { key: 'clipboardPanel', name: 'clipboard-panel', prefix: 'CLIPBOARD_PANEL', sourceDir: 'clipboard/astal', entry: 'frontend.js', service: 'ags-clipboard',
                deps: [], helper: 'CLIPBOARD_CUSTOM_HELPER', binary: 'clipboard-custom', images: true },
        ];
        let metadata, fixture;

        function checked(command, args, options = {}) {
            const result = spawnSync(command, args, { encoding: 'utf8', timeout: 120000, maxBuffer: 8 * 1024 * 1024, ...options });
            assert.ifError(result.error);
            assert.equal(result.status, 0, `${command} failed:\n${result.stderr}\n${result.stdout}`);
            return result.stdout;
        }

        before(() => {
            const expression = `let
      lock = builtins.fromJSON (builtins.readFile (builtins.toPath ${JSON.stringify(join(projectRoot, 'flake.lock'))}));
      nixpkgs = builtins.fetchTree lock.nodes.\${lock.nodes.root.inputs.nixpkgs}.locked;
      base = import nixpkgs.outPath {};
      pkgs = base // { writeShellApplication = args: args // (args.passthru or {}) // {
        type = "derivation";
        outPath = "/nix/store/00000000000000000000000000000000-" + args.name;
        meta.mainProgram = args.name;
      }; };
      module = import (builtins.toPath ${JSON.stringify(join(projectRoot, 'niri/panels.nix'))});
      helper = name: { type = "derivation"; outPath = "/nix/store/00000000000000000000000000000000-" + name; meta.mainProgram = name; };
      packages = module.packages {
        inherit pkgs;
        repoRoot = ${JSON.stringify(repoRoot)};
        audioControl = helper "audio-control";
        mediaControl = helper "media-control";
        networkManager = helper "network-manager";
        clipboardCustom = helper "clipboard-custom";
      };
      homeModule = module.module {
        inherit pkgs;
        inherit (pkgs) lib;
        repoRoot = ${JSON.stringify(repoRoot)};
        repoPackages = packages // {
          audioControl = helper "audio-control";
          mediaControl = helper "media-control";
          networkManager = helper "network-manager";
          clipboardCustom = helper "clipboard-custom";
        };
        config = {
          lib.file.mkOutOfStoreSymlink = path: path;
          xdg.configHome = "/test-config";
          xdg.configFile = homeModule.xdg.configFile;
        };
      };
    in {
      exports = builtins.attrNames module;
      links = builtins.mapAttrs (_: value: toString value.source) homeModule.xdg.configFile;
      services = homeModule.systemd.user.services;
      panels = builtins.mapAttrs (_: panel: {
        inherit (panel) text;
        entry = panel.entry;
        source = toString panel.source;
        ags = toString panel.ags;
        inputs = map toString panel.ags.buildInputs;
        postFixup = panel.ags.postFixup or "";
        postInstall = panel.ags.postInstall or "";
        postInstallReferences = builtins.attrNames (builtins.getContext (panel.ags.postInstall or ""));
        palette = if (panel.palette or null) != null then toString panel.palette else null;
      }) packages;
      astal = builtins.mapAttrs (_: value: toString value) {
        inherit (pkgs.astal) wireplumber bluetooth mpris network battery powerprofiles notifd apps;
      };
      gjs = pkgs.lib.getExe pkgs.gjs;
      layerShell = toString pkgs.gtk4-layer-shell;
      imageLoaders = map toString [ pkgs.gdk-pixbuf pkgs.librsvg pkgs.webp-pixbuf-loader pkgs.libavif ];
      queryLoaders = (toString (pkgs.lib.getDev pkgs.gdk-pixbuf)) + "/bin/gdk-pixbuf-query-loaders";
      jq = pkgs.lib.getExe pkgs.jq;
      coreutils = toString pkgs.coreutils;
      wlPaste = pkgs.lib.getExe' pkgs.wl-clipboard "wl-paste";
      utilityModules = builtins.attrNames (import (builtins.toPath ${JSON.stringify(join(projectRoot, 'niri/waybar/utilities.nix'))}) { packages = {}; }).modules;
    }`;
            metadata = JSON.parse(checked('nix', ['eval', '--impure', '--json', '--expr', expression], { cwd: projectRoot }));
            fixture = mkdtempSync(join(tmpdir(), 'panel-packaging-'));
            const fake = join(fixture, 'capture');
            writeFileSync(fake, `#!${process.execPath}\n`
                + 'const fs = require("node:fs");\n'
                + 'const argv = process.argv.slice(2);\n'
                + 'fs.appendFileSync(process.env.PANEL_TEST_LOG, JSON.stringify({argv, env: process.env}) + "\\n");\n'
                + 'if (argv[0] === "bundle") {\n'
                + '  if (process.env.PANEL_TEST_FAIL) process.exit(41);\n'
                + '  fs.writeFileSync(argv.at(-1), argv.at(-2));\n'
                + '}\n');
            chmodSync(fake, 0o700);
            for (const panel of panels) {
                const value = metadata.panels[panel.key];
                let text = value.text.replaceAll(`${value.ags}/bin/ags`, fake)
                    .replaceAll(`${value.ags}/bin/panel-gjs`, fake)
                    .replaceAll(metadata.jq, 'jq').replaceAll(`${metadata.coreutils}/bin/`, '');
                assert.doesNotMatch(text, /\/nix\/store\/[^\s"']*\/bin\/(?:ags|panel-gjs)\b/,
                    'all native runtime invocations must be intercepted; no desktop services may be contacted');
                panel.script = join(fixture, `${panel.name}.sh`);
                writeFileSync(panel.script, `set -euo pipefail\n${text}`);
            }
        });
        after(() => { if (fixture) rmSync(fixture, { recursive: true, force: true }); });

        function invoke(panel, overrides = {}, args = ['run']) {
            const directory = mkdtempSync(join(fixture, 'case-'));
            const runtime = join(directory, 'runtime');
            mkdirSync(runtime, { mode: 0o700 });
            const log = join(directory, 'calls.jsonl');
            if (overrides.PANEL_TEST_FAIL) {
                const bundles = join(runtime, 'ags-panels');
                mkdirSync(bundles, { mode: 0o700 });
                writeFileSync(join(bundles, `${panel.name}.js`), 'stale bundle must not execute');
            }
            const result = spawnSync('bash', [panel.script, ...args], {
                env: { PATH: process.env.PATH, HOME: directory, XDG_CONFIG_HOME: join(directory, 'config'),
                    XDG_RUNTIME_DIR: runtime, PANEL_TEST_LOG: log, ...overrides },
                encoding: 'utf8', timeout: 5000,
            });
            assert.ifError(result.error);
            const calls = existsSync(log) ? readFileSync(log, 'utf8').trim().split('\n').map(line => JSON.parse(line)) : [];
            return { ...result, calls, runtime };
        }

        function files(directory) {
            return readdirSync(directory, { withFileTypes: true }).flatMap(entry =>
                entry.isDirectory() ? files(join(directory, entry.name)).map(name => join(entry.name, name)) : [entry.name]);
        }

        function launch(panel, overrides) {
            const result = invoke(panel, overrides);
            assert.equal(result.status, 0, result.stderr);
            assert.equal(result.calls.length, 2, 'bundle once, then execute GJS once');
            const [compiler, runtime] = result.calls;
            const config = compiler.env[`${panel.prefix}_CONFIG`];
            assert.match(config, /^\/nix\/store\//, 'application entry is immutable');
            assert.ok(config.endsWith(`/${panel.sourceDir}/${panel.entry}`), config);
            const bundle = join(result.runtime, 'ags-panels', `${panel.name}.js`);
            assert.deepEqual(compiler.argv, ['bundle', '--gtk', '4', '--', config, bundle]);
            assert.deepEqual(runtime.argv, ['-m', bundle]);
            assert.equal(readFileSync(bundle, 'utf8'), config, 'GJS executes the bundle from this compilation');
            assert.equal(statSync(dirname(bundle)).mode & 0o777, 0o700, 'bundle directory stays private');
            for (const call of result.calls) {
                assert.equal(call.env.XDG_RUNTIME_DIR, result.runtime, 'bundling cannot move the native IPC namespace');
                assert.equal(call.env[`${panel.prefix}_CONFIG`], config);
            }
            assert.equal(existsSync(join(result.runtime, 'ags.js')), false, 'never use AGS\'s shared startup output');
            return { ...result, config };
        }

        test('one consolidated module exports the package/runtime/service contract', () => {
            for (const name of ['mkAgsRuntime', 'mkPanel', 'packages', 'module']) assert.ok(metadata.exports.includes(name), name);
            assert.ok(statSync(join(projectRoot, 'niri/panels.nix')).isFile());
            assert.equal(existsSync(join(projectRoot, 'niri/panels')), false, 'no superseded panel module directory');
            assert.deepEqual(Object.keys(metadata.panels).sort(), panels.map(panel => panel.key).sort());
            assert.equal(metadata.services.waybar, undefined, 'Waybar service integration belongs to the Waybar module');
        });

        test('clipboard capture uses one private, session-scoped collector independent of Waybar', () => {
            assert.deepEqual(Object.keys(metadata.services).filter(name => name.includes('collector')), ['clipboard-collector']);
            const collector = metadata.services['clipboard-collector'];
            assert.equal(collector.Service.ExecStart,
                `${metadata.wlPaste} --no-newline --watch /nix/store/00000000000000000000000000000000-clipboard-custom/bin/clipboard-custom capture`);
            assert.equal(collector.Service.UMask, '0077');
            assert.equal(collector.Service.Restart, 'always');
            assert.deepEqual(collector.Unit.ConditionEnvironment, ['WAYLAND_DISPLAY', 'XDG_CURRENT_DESKTOP=niri']);
            assert.deepEqual(collector.Unit.PartOf, ['graphical-session.target']);
            assert.deepEqual(collector.Install.WantedBy, ['graphical-session.target']);
            assert.deepEqual(metadata.services['ags-clipboard'].Unit.Wants, ['clipboard-collector.service']);
            assert.ok(metadata.services['ags-clipboard'].Unit.After.includes('clipboard-collector.service'));
            assert.equal(metadata.utilityModules.some(name => name.includes('clipboard')), false,
                'Waybar must not start a second collector or define a duplicate clipboard module');
            const layout = JSON.parse(readFileSync(join(projectRoot, 'themes/waybar/top.jsonc'), 'utf8'));
            assert.deepEqual(layout['modules-right'].filter(name => name.includes('clipboard')), ['custom/clipboard-panel']);
            const keybindings = readFileSync(join(projectRoot, 'niri/config.kdl'), 'utf8');
            assert.match(keybindings, /Mod\+Alt\+V[^\n]*\{ spawn "clipboard-panel" "toggle"; \}/);
        });

        test('Home Manager installs no children under legacy live-linked panel directories; services retain store entries and palette triggers', () => {
            const liveLinks = Object.entries(metadata.links).filter(([, source]) => source.startsWith(repoRoot));
            assert.deepEqual(liveLinks, [], 'CSS is read directly from the checkout, not installed through config directory links');
            assert.deepEqual(Object.keys(metadata.links).sort(),
                ['audio', 'clipboard', 'dashboard', 'media', 'network'].map(name => `ags/${name}-palette.json`).sort(),
                'only sibling palette files may be installed: child links can traverse old directory links into the repository');
            for (const panel of panels) {
                const service = metadata.services[panel.service];
                assert.equal(service.Service.ExecStart,
                    `/nix/store/00000000000000000000000000000000-${panel.name}/bin/${panel.name} run`);
                const environment = service.Service.Environment || [];
                assert.ok(environment.includes('GDK_BACKEND=wayland'));
                assert.ok(!environment.some(value => value.startsWith(`${panel.prefix}_CONFIG=`)),
                    `${panel.name}: no XDG entry override in service wiring`);
                if (panel.palette !== false) {
                    assert.ok(service.Unit['X-Restart-Triggers'].includes(metadata.panels[panel.key].palette),
                        `${panel.name}: palette generation changes still restart the resident service`);
                }
            }
        });

        for (const panel of panels) {
            test(`${panel.name}: immutable source, CSS-only override, isolated runtime bundle and store defaults`, () => {
                const result = launch(panel);
                const value = metadata.panels[panel.key];
                for (const call of result.calls) {
                    assert.equal(call.env.PANEL_CSS_FILE, `${repoRoot}/scripts/${panel.sourceDir}/style.css`);
                    if (panel.palette !== false) assert.equal(call.env[`${panel.prefix}_PALETTE`], value.palette);
                    if (panel.helper) assert.equal(call.env[panel.helper],
                        `/nix/store/00000000000000000000000000000000-${panel.binary}/bin/${panel.binary}`);
                }
            });

            test(`${panel.name}: native dependencies, wrapper environment and artwork loaders are retained`, () => {
                const value = metadata.panels[panel.key];
                for (const dep of panel.deps) assert.ok(value.inputs.includes(metadata.astal[dep]),
                    `${panel.name}: use the unmodified native Astal ${dep} package`);
                assert.ok(value.postFixup.includes('"${gappsWrapperArgs[@]}"'), 'inherit the complete AGS wrapper environment');
                assert.ok(value.postFixup.includes(metadata.gjs), 'wrap native GJS');
                assert.ok(value.postFixup.includes(`${metadata.layerShell}/lib/libgtk4-layer-shell.so`), 'retain layer-shell preload');
                if (panel.images) {
                    assert.match(value.postInstall, /GDK_PIXBUF_MODULE_FILE=/,
                        'retain the custom native artwork loader cache in both runtime wrappers');
                    const references = value.postInstallReferences.filter(path => path.endsWith('.drv'));
                    assert.ok(references.length, 'artwork cache must retain its derivation context');
                    const derivations = JSON.parse(checked('nix', ['derivation', 'show', ...references]));
                    const recipes = Object.values(derivations.derivations ?? derivations)
                        .map(derivation => derivation.env.buildCommand || '').join('\n');
                    for (const loader of metadata.imageLoaders) assert.ok(recipes.includes(loader),
                        `${panel.name}: preserve native artwork loader ${loader}`);
                    assert.ok(recipes.includes(metadata.queryLoaders), 'generate the combined native loader cache');
                    assert.match(recipes, /GDK_PIXBUF_MODULEDIR=/, 'query the combined directory, not the default cache');
                }
            });

            test(`${panel.name}: source filter preserves relative imports but excludes CSS, Rust and unrelated panels`, () => {
                const { entry: config, source: sourceRoot } = metadata.panels[panel.key];
                assert.match(sourceRoot, /^\/nix\/store\//);
                assert.equal(config, join(sourceRoot, panel.sourceDir, panel.entry));
                const contents = files(sourceRoot);
                assert.ok(contents.includes('panels/style.js'), 'relative shared style import must resolve from the store');
                for (const file of files(join(projectRoot, 'scripts', panel.sourceDir)).filter(file => /\.(?:js|ts|json)$/.test(file))) {
                    assert.ok(contents.includes(join(panel.sourceDir, file)), `${panel.name}: missing ${file}`);
                }
                for (const file of contents) {
                    assert.match(file, /\.(?:js|ts|json)$/, `${panel.name}: unexpected runtime source ${file}`);
                    assert.ok(file === 'panels/style.js' || file.startsWith(`${panel.sourceDir}/`),
                        `${panel.name}: unrelated source ${relative(dirname(config), join(sourceRoot, file))}`);
                }
            });

            test(`${panel.name}: live-source injection cannot replace the entry; CSS/palette/helper overrides remain literal`, () => {
                const marker = join(fixture, `${panel.name}-injection`);
                const overrides = { [`${panel.prefix}_CONFIG`]: `/checkout/frontend.js; touch ${marker}`,
                    PANEL_CSS_FILE: join(fixture, 'custom style with spaces.css') };
                if (panel.palette !== false) overrides[`${panel.prefix}_PALETTE`] = join(fixture, 'custom palette.json');
                if (panel.helper) overrides[panel.helper] = join(fixture, 'custom helper');
                const result = launch(panel, overrides);
                assert.notEqual(result.config, overrides[`${panel.prefix}_CONFIG`]);
                for (const call of result.calls) for (const [name, value] of Object.entries(overrides)) {
                    if (!name.endsWith('_CONFIG')) assert.equal(call.env[name], value);
                }
                assert.equal(existsSync(marker), false);
            });

            test(`${panel.name}: requests retain the native instance and escape monitor names without bundling`, () => {
                const marker = join(fixture, `${panel.name}-monitor-injection`);
                for (const args of [['status'], ['close'], ['toggle'], ['toggle', `DP-'\"; touch ${marker}; 🌐`]]) {
                    const result = invoke(panel, {}, args);
                    assert.equal(result.status, 0, result.stderr);
                    assert.equal(result.calls.length, 1, 'requests must neither bundle nor start GJS');
                    const [call] = result.calls;
                    assert.deepEqual(call.argv.slice(0, 4), ['request', '--instance', panel.name, '--']);
                    assert.equal(call.argv.length, 5);
                    assert.deepEqual(JSON.parse(call.argv[4]), { command: args[0], ...(args[1] ? { monitor: args[1] } : {}) });
                    assert.equal(call.env.XDG_RUNTIME_DIR, result.runtime);
                    assert.equal(existsSync(join(result.runtime, 'ags-panels')), false);
                }
                assert.equal(existsSync(marker), false);
            });

            test(`${panel.name}: compiler failure cannot execute a stale bundle`, () => {
                const result = invoke(panel, { PANEL_TEST_FAIL: '1' });
                assert.equal(result.status, 41, result.stderr);
                assert.equal(result.calls.length, 1, 'GJS must not run after bundling fails');
                assert.equal(result.calls[0].argv[0], 'bundle');
                assert.equal(readFileSync(join(result.runtime, 'ags-panels', `${panel.name}.js`), 'utf8'),
                    'stale bundle must not execute');
            });
        }
    });
} else {
    const { default: Gio } = await import('gi://Gio');
    const { default: GLib } = await import('gi://GLib');
    const { default: Gtk } = await import('gi://Gtk?version=4.0');
    const { default: Gdk } = await import('gi://Gdk?version=4.0');
    const { default: System } = await import('system');
    const { loadPanelStyle } = await import('./style.js');
    if (GLib.getenv('GDK_BACKEND') !== 'x11') throw new Error('Run native tests on a private Xvfb display.');
    Gtk.init();
    GLib.unsetenv('LD_PRELOAD');
    const root = GLib.dir_make_tmp('astal-style-XXXXXX');
    const css = `${root}/style.css`, temp = `${root}/.style.css.tmp`;
    const file = Gio.File.new_for_path(css);
    const wait = ms => new Promise(resolve => GLib.timeout_add(GLib.PRIORITY_DEFAULT, ms,
        () => { resolve(); return GLib.SOURCE_REMOVE; }));
    const assert = (condition, message) => { if (!condition) throw new Error(message); };
    const write = text => GLib.file_set_contents(css, text);
    const originalOverride = GLib.getenv('PANEL_CSS_FILE');

    let provider;
    async function run() {
        try {
            GLib.setenv('PANEL_CSS_FILE', css, true);
            write('label { color: @test_accent; }');

            provider = loadPanelStyle({ entry: `${root}/frontend.js`, definitions: '@define-color test_accent #ff0000;', label: 'Native test' });
            assert(provider.to_string().includes('test_accent'), 'Initial CSS includes fixed palette definitions');
            const replace = text => {
                GLib.file_set_contents(temp, text);
                Gio.File.new_for_path(temp).move(file, Gio.FileCopyFlags.OVERWRITE, null, null);
            };
            const until = async (check, message) => {
                for (let i = 0; i < 60; i++) { if (check()) return; await wait(50); }
                throw new Error(message);
            };
            for (const color of ['#0000ff', '#00ff00']) {
                System.gc();
                replace(`label { color: ${color}; }`);
                await until(() => provider.to_string().includes(color === '#0000ff' ? 'rgb(0,0,255)' : 'rgb(0,255,0)'),
                    `Atomic replacement did not apply ${color}`);
            }
            const valid = provider.to_string();
            replace('label { color: definitely-not-a-color; }');
            await wait(350);
            assert(provider.to_string() === valid, 'Parse failure restores last valid CSS');
            file.delete(null);
            await wait(350);
            assert(provider.to_string() === valid, 'Read failure keeps last valid CSS');
            replace('label { color: #ffffff; }');
            await until(() => provider.to_string().includes('rgb(255,255,255)'), 'Watcher did not recover after deletion');
            assert(provider.to_string().includes('test_accent'), 'Reloads keep the original palette');
            print('Native CSS reload passed: repeated atomic saves, GC lifetime, parse rollback, read failure, recovery, fixed palette.');
        } finally {

            if (provider) Gtk.StyleContext.remove_provider_for_display(Gdk.Display.get_default(), provider);
            if (originalOverride === null) GLib.unsetenv('PANEL_CSS_FILE');
            else GLib.setenv('PANEL_CSS_FILE', originalOverride, true);
            if (file.query_exists(null)) file.delete(null);
            const temporary = Gio.File.new_for_path(temp);
            if (temporary.query_exists(null)) temporary.delete(null);
            Gio.File.new_for_path(root).delete(null);
        }
    }
    await run();
}
