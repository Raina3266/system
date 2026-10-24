// Node: node --test test.js (or npm test).
// Native smoke checks: <launcher-ags-runtime>/bin/panel-gjs -m test.js on an isolated Wayland display.
// Runtime-specific imports keep Node free of GI dependencies and GJS free of Node dependencies.
import { parseRequest, panelLayout, applicationsFor, nextIndex, ANIMATION_MS, BAR_GAP, PANEL_LEFT,
    TILE_WIDTH, TILE_HEIGHT, TILE_GAP, GRID_COLUMNS, GRID_ROWS, PANEL_PADDING, SEARCH_HEIGHT, SEARCH_GAP } from './logic.js';

async function unitTests() {
    const { default: test } = await import('node:test');
    const { default: assert } = await import('node:assert/strict');
    const { readFileSync } = await import('node:fs');
    const { runInNewContext } = await import('node:vm');

    function loadClass(file, name, context) {
        const source = readFileSync(new URL(file, import.meta.url), 'utf8');
        const start = source.indexOf(`export class ${name}`);
        const end = source.indexOf('\nexport ', start + 1);
        const definition = source.slice(start, end).replace('export class', 'class');
        return runInNewContext(`${definition}\n${name};`, context);
    }

    function panelFixture() {
        let query = '', focused = null;
        let results = [{ name: 'Anki', entry: 'anki' }];
        const created = [], children = [], launches = [];
        const Panel = loadClass('./frontend.js', 'LauncherPanel', {
            Gtk: { EventControllerFocus: class { connect(_signal, callback) { this.enter = callback; } } },
            Astal: { Keymode: { EXCLUSIVE: 1 } },
            console: { error() {} },
            tile(application, activate) {
                const button = {
                    application, activate, classes: new Set(), controllers: [],
                    add_controller(controller) { this.controllers.push(controller); },
                    grab_focus() { focused = this; this.controllers.forEach(controller => controller.enter()); },
                    compute_bounds: () => [false],
                    add_css_class(name) { this.classes.add(name); },
                    remove_css_class(name) { this.classes.delete(name); },
                };
                created.push(button);
                return button;
            },
        });
        const panel = Object.assign(Object.create(Panel.prototype), {
            opened: false, monitor: null, selected: -1, tiles: [], tileCache: new WeakMap(), columns: GRID_COLUMNS, hideTimer: 0,
            backend: { query: () => results, launch(application) { launches.push(application); return false; } },
            window: { get_focus: () => focused }, gridStack: {}, empty: {}, revealer: {},
            grid: {
                get_first_child: () => children[0] || null,
                remove(button) { children.splice(children.indexOf(button), 1); },
                attach(button, column, row) {
                    assert.ok(!children.includes(button), 'Tiles must be detached before reattaching');
                    Object.assign(button, { column, row });
                    children.push(button);
                },
            },
            scroll: { get_vadjustment: () => ({ set_value() {} }) },
            search: { get_text: () => query, set_text(value) { query = value; }, grab_focus() { focused = panel.search; } },
            setMonitor() {}, resize() {},
        });
        return { panel, created, children, launches,
            setQuery(value) { query = value; }, setResults(value) { results = value; } };
    }

    test('IPC accepts only supported commands and valid monitor names', () => {
        assert.deepEqual(parseRequest('{"command":"toggle","monitor":"DP-8"}'), { command: 'toggle', monitor: 'DP-8' });
        for (const command of ['status', 'close', 'toggle']) assert.equal(parseRequest(JSON.stringify({ command })).command, command);
        for (const value of ['null', '[]', '{}', '{"command":"launch"}', '{"command":"toggle","monitor":42}',
            '{"command":"toggle","monitor":" "}', 'not JSON']) assert.throws(() => parseRequest(value));
    });

    test('layout fits configured monitors and leaves room above the bottom Waybar', () => {
        for (const [width, height] of [[1920, 1200], [2560, 1440], [2048, 2304], [800, 600]]) {
            const layout = panelLayout(width, height);
            assert.ok(layout.width + PANEL_LEFT * 2 <= width);
            assert.ok(layout.gridHeight + BAR_GAP + PANEL_PADDING * 2 + SEARCH_HEIGHT + SEARCH_GAP <= height);
            assert.equal(layout.columns, GRID_COLUMNS);
            assert.equal(layout.gridHeight, GRID_ROWS * TILE_HEIGHT + (GRID_ROWS - 1) * TILE_GAP);
        }
        assert.equal(panelLayout(1920, 1200).columns, 4);
        assert.equal(panelLayout(800, 600).columns, 4);
        assert.ok(panelLayout(200, 240).gridHeight > 0);
    });

    test('all native-visible apps are alphabetized without mutating native state', () => {
        const list = [{ name: 'Zed', entry: 'zed' }, { name: 'Anki', entry: 'anki' }, { name: 'Anki', entry: 'anki-alt' }];
        const apps = { list };
        assert.deepEqual(applicationsFor(apps).map(app => app.entry), ['anki', 'anki-alt', 'zed']);
        assert.equal(apps.list[0].name, 'Zed');
        assert.deepEqual(applicationsFor(apps, '   '), applicationsFor(apps));
    });

    test('search delegates fuzzy matching and ranking to Astal without truncating results', () => {
        const results = [{ name: 'Google Chrome' }, { name: 'Chromium' }];
        const apps = { fuzzy_query(query) { assert.equal(query, 'chr'); return results; } };
        assert.equal(applicationsFor(apps, ' chr '), results);
    });

    test('compact panel shows four half-width tiles per row and exactly two rows', () => {
        const layout = panelLayout(1920, 1200);
        assert.equal(TILE_WIDTH, 66);
        assert.equal(layout.columns, 4);
        assert.equal(GRID_ROWS, 2);
        assert.equal(layout.width, 318);
        assert.equal(layout.gridHeight, 190);
        assert.ok(panelLayout(320, 600).width <= 320 - PANEL_LEFT * 2);
        assert.ok(panelLayout(320, 600).columns < GRID_COLUMNS);
    });

    test('launcher opens and clears search without preselecting the first app', () => {
        const { panel, setQuery } = panelFixture();
        panel.render();
        assert.equal(panel.selected, -1);
        assert.ok(panel.tiles.every(tile => !tile.classes.has('selected')));
        setQuery('Anki');
        panel.render();
        assert.equal(panel.selected, 0, 'typed searches retain their first-result selection');
        setQuery('');
        panel.render();
        assert.equal(panel.selected, -1, 'clearing search removes the default highlight');
        panel.select(0, true);
        panel.toggle({ get_connector: () => 'test' });
        assert.equal(panel.selected, -1, 'reopening clears stale keyboard selection');
        assert.ok(panel.tiles.every(tile => !tile.classes.has('selected')));
    });

    test('tiles are reused across filtering and reflow without stale click or focus indices', () => {
        const { panel, created, children, launches, setResults, setQuery } = panelFixture();
        const apps = ['Anki', 'Chrome', 'Zed'].map(name => ({ name, entry: name }));
        setResults(apps);
        panel.render();
        const original = [...panel.tiles];
        setQuery('zed');
        setResults([apps[2], apps[1]]);
        panel.render();
        assert.equal(created.length, 3);
        assert.equal(panel.tiles[0], original[2]);
        panel.tiles[0].activate();
        assert.equal(launches[0], apps[2]);
        assert.ok(panel.tiles[0].classes.has('launch-error'));
        panel.tiles[1].grab_focus();
        assert.equal(panel.selected, 1);
        panel.render();
        assert.equal(panel.selected, 0, 'Grid-focused refresh must refocus the first result');
        assert.ok(panel.tiles.every(tile => !tile.classes.has('launch-error')));
        assert.ok(panel.tiles.every(tile => tile.controllers.length === 1));
        setResults(apps);
        panel.columns = 2;
        panel.render();
        assert.equal(panel.tiles[2], original[2]);
        assert.deepEqual([panel.tiles[2].column, panel.tiles[2].row], [0, 1]);
        assert.equal(created.length, 3);
        setResults([]);
        panel.render();
        assert.equal(children.length, 0);
        assert.equal(panel.selected, -1);
        const replacement = { name: 'New Anki', entry: apps[0].entry };
        setResults([replacement]);
        panel.render();
        assert.notEqual(panel.tiles[0], original[0], 'Native object replacements must refresh tile metadata');
        panel.tiles[0].activate();
        assert.equal(launches[1], replacement);
    });

    test('alphabetical cache invalidates on every native change, including coalesced events', () => {
        let entries = [{ name: 'Zed', entry: 'zed' }], reads = 0, searches = 0, changed = 0;
        const pending = new Map();
        let signal;
        const monitor = { connect(_name, callback) { signal = callback; return 1; }, disconnect() { signal = null; } };
        const Backend = loadClass('./backend.js', 'LauncherBackend', {
            Apps: { Apps: class {
                get list() { reads++; return entries; }
                fuzzy_query(search) { searches++; assert.equal(search, 'z'); return entries; }
            } },
            Gio: { AppInfoMonitor: { get: () => monitor } },
            GLib: {
                PRIORITY_DEFAULT_IDLE: 0, SOURCE_REMOVE: false,
                idle_add(_priority, callback) { pending.set(1, callback); return 1; },
                source_remove(id) { pending.delete(id); },
            },
            applicationsFor,
        });
        const backend = new Backend(() => changed++);
        const first = backend.applications;
        assert.equal(backend.applications, first);
        assert.equal(backend.query('  '), first);
        assert.equal(reads, 1);
        backend.query(' z ');
        backend.query('z');
        assert.equal(searches, 2, 'Native ranking must not be cached');
        signal();
        const second = backend.applications;
        assert.notEqual(second, first);
        entries = [{ name: 'Zoom', entry: 'zoom' }];
        signal();
        assert.equal(backend.applications[0].entry, 'zoom');
        assert.equal(reads, 3, 'Each catalog change must invalidate even before the idle refresh');
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

    test('keyboard navigation clamps to real entries, including a partial last row', () => {
        assert.equal(nextIndex(0, 'Left', GRID_COLUMNS, 10), 0);
        assert.equal(nextIndex(0, 'Down', GRID_COLUMNS, 10), 4);
        assert.equal(nextIndex(7, 'Down', GRID_COLUMNS, 10), 9);
        assert.equal(nextIndex(9, 'Right', GRID_COLUMNS, 10), 9);
        assert.equal(nextIndex(9, 'Up', GRID_COLUMNS, 10), 5);
        assert.equal(nextIndex(7, 'Home', GRID_COLUMNS, 10), 0);
        assert.equal(nextIndex(7, 'End', GRID_COLUMNS, 10), 9);
        assert.equal(nextIndex(0, 'Down', GRID_COLUMNS, 0), -1);
    });
}

async function smokeTests() {
    const { default: GLib } = await import('gi://GLib');
    const { default: Gio } = await import('gi://Gio');
    const { default: Gtk } = await import('gi://Gtk?version=4.0');
    const { default: Gdk } = await import('gi://Gdk?version=4.0');
    GLib.setenv('APP_LAUNCHER_IMPORT_ONLY', '1', true);
    const { LauncherApplication, LauncherPanel } = await import('./frontend.js');

    function assert(condition, message) { if (!condition) throw new Error(message); }
    function wait(ms) {
        return new Promise(resolve => GLib.timeout_add(GLib.PRIORITY_DEFAULT, ms, () => {
            resolve();
            return GLib.SOURCE_REMOVE;
        }));
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
    // Xvfb has no layer-shell compositor to supply the all-edge anchored size.
    if (GLib.getenv('GDK_BACKEND') === 'x11') panel.window.set_default_size(1280, 800);
    app.panel = panel;
    try {
        assert(cssErrors === 0, 'CSS must parse without errors');
        const monitor = Gdk.Display.get_default().get_monitors().get_item(0);
        panel.toggle(monitor);
        await wait(ANIMATION_MS + 200);
        assert(panel.opened && panel.window.visible && panel.revealer.child_revealed,
            `Panel must finish sliding open: open=${panel.opened}, visible=${panel.window.visible}, revealed=${panel.revealer.child_revealed}`);
        assert(panel.results.length === panel.backend.applications.length, 'Unfiltered grid must include every visible app');
        assert(panel.tiles.length === panel.results.length, 'Every app must have a tile');
        assert(panel.tiles.every(tile => !tile.get_has_tooltip()), 'Application tiles must not show tooltips');
        const catalog = panel.backend.applications;
        const originalTiles = new Map(panel.results.map((application, index) => [application, panel.tiles[index]]));
        assert(panel.backend.applications === catalog && panel.backend.query('  ') === catalog,
            'Unfiltered reads must reuse the alphabetical catalog');
        assert(panel.card.get_width() > 0 && panel.card.get_height() > 0, 'Panel must be allocated');
        const [valid, bounds] = panel.card.compute_bounds(panel.root);
        assert(valid && bounds.get_x() === PANEL_LEFT, 'Panel must sit at the bottom-left');
        assert(Math.abs(bounds.get_y() + bounds.get_height() - (panel.root.get_height() - BAR_GAP)) < 2,
            'Panel must leave the bottom Waybar unobscured');
        assert(panel.search instanceof Gtk.SearchEntry && !('message' in panel) && !('count' in panel),
            'Panel must contain search and the application grid without a footer');
        assert(panel.columns === GRID_COLUMNS, 'Grid must have four columns');
        assert(panel.scroll.get_height() === GRID_ROWS * TILE_HEIGHT + (GRID_ROWS - 1) * TILE_GAP,
            'Viewport must show exactly two rows');
        assert(panel.card.get_width() === 318 && panel.card.get_height() === 214 + SEARCH_HEIGHT + SEARCH_GAP,
            'Panel must add only the search bar above the compact grid');
        const searchFocused = () => {
            const focused = panel.window.get_focus();
            return focused === panel.search || Boolean(focused?.is_ancestor(panel.search));
        };
        assert(searchFocused(), 'Search must receive initial keyboard focus');
        assert(panel.selected === -1 && panel.tiles.every(tile => !tile.has_css_class('selected')),
            'Opening the launcher must not preselect Anki or any other application');
        assert(!panel.keyPressed(Gdk.KEY_Left), 'Left must retain normal search cursor movement');
        panel.keyPressed(Gdk.KEY_Down);
        assert(panel.window.get_focus() === panel.tiles[0], 'Down from search must focus the first app');
        assert(panel.tiles[0].get_allocated_width() === TILE_WIDTH && panel.tiles[0].get_allocated_height() === TILE_HEIGHT,
            'Application tiles must use the compact dimensions');
        if (panel.tiles.length > GRID_COLUMNS * GRID_ROWS) {
            const [, lastVisibleRow] = panel.tiles[GRID_COLUMNS * (GRID_ROWS - 1)].compute_bounds(panel.grid);
            const [, nextRow] = panel.tiles[GRID_COLUMNS * GRID_ROWS].compute_bounds(panel.grid);
            assert(lastVisibleRow.get_y() + lastVisibleRow.get_height() <= panel.scroll.get_height(),
                'Second row must be fully visible');
            assert(nextRow.get_y() >= panel.scroll.get_height(), 'Third row must stay outside the viewport');
        }
        panel.keyPressed(Gdk.KEY_Down);
        assert(panel.selected === GRID_COLUMNS, 'Down must move to the second row');
        panel.keyPressed(Gdk.KEY_Right);
        assert(panel.selected === GRID_COLUMNS + 1, 'Right must select the next app');
        panel.keyPressed(Gdk.KEY_Up);
        assert(panel.selected === 1, 'Up must return to the first row');
        panel.keyPressed(Gdk.KEY_End);
        assert(panel.selected === panel.results.length - 1 && panel.scroll.get_vadjustment().value > 0,
            'Keyboard navigation must scroll to applications beyond the first eight');
        panel.keyPressed(Gdk.KEY_Home);
        assert(panel.selected === 0 && panel.scroll.get_vadjustment().value === 0, 'Home must return to the top');
        panel.keyPressed(Gdk.KEY_Up);
        assert(searchFocused(), 'Up from the first grid row must return to search');
        const application = panel.backend.applications[0];
        panel.search.set_text(application.name);
        await wait(150);
        assert(panel.results.some(result => result.entry === application.entry), 'Search must include an exact application name');
        assert(panel.tiles.every((tile, index) => tile === originalTiles.get(panel.results[index])),
            'Filtering must reuse the existing native application tiles');
        assert(searchFocused(), 'Filtering must not steal focus from the search field');
        assert(panel.results.map(result => result.entry).join() === panel.backend.query(application.name).map(result => result.entry).join(),
            'Displayed search results must preserve native Astal ranking');
        panel.search.set_text('this-app-does-not-exist-394853');
        await wait(150);
        assert(panel.results.length === 0 && panel.gridStack.visible_child_name === 'empty', 'Unmatched search must show the empty state');
        panel.keyPressed(Gdk.KEY_Return);
        assert(panel.opened, 'Enter with no matching applications must be harmless');
        panel.search.set_text('');
        await wait(150);
        assert(panel.results.length === panel.backend.applications.length, 'Clearing search must restore every application');
        assert(panel.tiles.every((tile, index) => tile === originalTiles.get(panel.results[index])),
            'Clearing search must reuse the original tiles');
        panel.tiles[0].add_css_class('launch-error');
        panel.render();
        assert(!panel.tiles[0].has_css_class('launch-error'), 'Rerendering must clear transient launch errors');
        assert(panel.selected === -1 && panel.tiles.every(tile => !tile.has_css_class('selected')),
            'Clearing search must restore the grid without a default selected tile');
        panel.keyPressed(Gdk.KEY_Down);
        panel.keyPressed(Gdk.KEY_f, Gdk.ModifierType.CONTROL_MASK);
        assert(searchFocused(), 'Ctrl+F must return to search');
        // A real desktop change reloads Astal's catalog before the frontend's idle refresh.
        panel.backend.apps.reload();
        panel.backend.appMonitor.emit('changed');
        await wait(150);
        assert(panel.backend.applications !== catalog, 'Native catalog changes must invalidate the alphabetical cache');
        assert(panel.tiles.every(tile => ![...originalTiles.values()].includes(tile)),
            'Native reloads must replace tiles rather than retain stale application metadata');
        assert(searchFocused(), 'Native catalog refresh must retain search focus');
        const status = JSON.parse(await app.handleRequest('{"command":"status"}'));
        assert(status.opened && status.applications === panel.backend.applications.length, 'Status must expose native application count');
        await app.handleRequest('{"command":"close"}');
        assert(!panel.opened && panel.window.visible, 'Close must retain the mapped surface during animation');
        await wait(ANIMATION_MS + 80);
        assert(!panel.window.visible && !panel.revealer.child_revealed, 'Panel must hide after sliding closed');
        panel.toggle(monitor);
        await wait(50);
        panel.close();
        await wait(50);
        panel.toggle(monitor);
        await wait(ANIMATION_MS + 80);
        assert(panel.opened && panel.window.visible && panel.revealer.child_revealed, 'Reopening must cancel an old hide timer');
        assert(panel.selected === -1 && panel.tiles.every(tile => !tile.has_css_class('selected')),
            'Reopening must clear the previous keyboard selection');
        panel.results = [{ name: 'Test app', entry: 'test.desktop', launch: () => true }];
        panel.launch(0);
        assert(!panel.opened, 'Successful native launch must dismiss the panel');
        print(`PASS: native GTK/Astal smoke checks; ${status.applications} apps; card ${panel.card.get_width()}×${panel.card.get_height()}`);
    } finally {
        panel.dispose();
        Gtk.StyleContext.remove_provider_for_display(Gdk.Display.get_default(), provider);
    }
}

if (typeof process !== 'undefined' && process.versions?.node) await unitTests();
else await smokeTests();
