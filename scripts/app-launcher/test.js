// Node: node --test test.js (or npm test).
// Native smoke checks: <launcher-ags-runtime>/bin/panel-gjs -m test.js on an isolated Wayland display.
// Runtime-specific imports keep Node free of GI dependencies and GJS free of Node dependencies.
import { parseRequest, panelLayout, applicationsFor, nextIndex, ANIMATION_MS, BAR_GAP, PANEL_LEFT,
    TILE_WIDTH, TILE_HEIGHT, TILE_GAP, GRID_COLUMNS, GRID_ROWS, PANEL_PADDING, SEARCH_HEIGHT, SEARCH_GAP } from './logic.js';

async function unitTests() {
    const { default: test } = await import('node:test');
    const { default: assert } = await import('node:assert/strict');

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

    test('launcher opens and clears search without preselecting the first app', async () => {
        const { readFileSync } = await import('node:fs');
        const { runInNewContext } = await import('node:vm');
        const source = readFileSync(new URL('./frontend.js', import.meta.url), 'utf8');
        const Panel = runInNewContext(source.slice(source.indexOf('export class LauncherPanel'),
            source.indexOf('\nexport const LauncherApplication')).replace('export class', 'class') + '\nLauncherPanel;', {
            Gtk: { EventControllerFocus: class { connect() {} } },
            Astal: { Keymode: { EXCLUSIVE: 1 } },
            tile() {
                return { classes: new Set(), add_controller() {}, grab_focus() {},
                    compute_bounds: () => [false],
                    add_css_class(name) { this.classes.add(name); },
                    remove_css_class(name) { this.classes.delete(name); } };
            },
        });
        let query = '', focused = null;
        const panel = Object.assign(Object.create(Panel.prototype), {
            opened: false, tiles: [], columns: 4, hideTimer: 0,
            backend: { query: () => [{ name: 'Anki', entry: 'anki' }] },
            window: { get_focus: () => focused }, gridStack: {}, empty: {}, revealer: {},
            grid: { get_first_child: () => null, attach() {} },
            scroll: { get_vadjustment: () => ({ set_value() {} }) },
            search: { get_text: () => query, set_text(value) { query = value; }, grab_focus() {} },
            setMonitor() {}, resize() {},
        });
        panel.render();
        assert.equal(panel.selected, -1);
        assert.ok(panel.tiles.every(tile => !tile.classes.has('selected')));
        query = 'Anki';
        panel.render();
        assert.equal(panel.selected, 0, 'typed searches retain their first-result selection');
        query = '';
        panel.render();
        assert.equal(panel.selected, -1, 'clearing search removes the default highlight');
        panel.select(0);
        focused = panel.tiles[0];
        panel.toggle({ get_connector: () => 'test' });
        assert.equal(panel.selected, -1, 'reopening clears stale keyboard selection');
        assert.ok(panel.tiles.every(tile => !tile.classes.has('selected')));
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
        assert(panel.selected === -1 && panel.tiles.every(tile => !tile.has_css_class('selected')),
            'Clearing search must restore the grid without a default selected tile');
        panel.keyPressed(Gdk.KEY_Down);
        panel.keyPressed(Gdk.KEY_f, Gdk.ModifierType.CONTROL_MASK);
        assert(searchFocused(), 'Ctrl+F must return to search');
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
