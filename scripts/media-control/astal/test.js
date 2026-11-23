import test, { describe } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { badge, truncateTitle, formatTime } from './logic.js';
import { loadIPC } from '../../shared/astal/ipc-test-loader.js';

const viewSource = readFileSync(new URL('./frontend.js', import.meta.url), 'utf8');
const accessibleProperty = { LABEL: 'label', DESCRIPTION: 'description' };

function accessibleWidget(properties = {}) {
    return { a11y: {}, update_property(properties, values) {
        assert.equal(properties.length, values.length);
        properties.forEach((property, index) => {
            assert.ok(Object.values(accessibleProperty).includes(property));
            this.a11y[property] = values[index];
        });
    }, ...properties };
}

describe('Media theme color syntax', () => {
    const required = ['yellow', 'cyan', 'pink', 'red', 'dimPink', 'separator', 'foreground', 'secondaryBackground'];
    const palette = Object.fromEntries(required.map((key, index) => [key, `#${(index + 1).toString(16).padStart(6, '0')}`]));

    function themeFixture(contents) {
        const calls = [], reads = [];
        const theme = viewSource.match(/function loadTheme\([\s\S]*?\n\}/)?.[0];
        assert.ok(theme);
        assert.match(viewSource, /import \{ loadPanelTheme \} from '..\/..\/shared\/astal\/theme\.js'/);
        const sharedTheme = readFileSync(new URL('../../shared/astal/theme.js', import.meta.url), 'utf8')
            .replace(/^import .*;$/gm, '').replace(/^export /gm, '');
        const loadTheme = new Function('GLib', 'loadPanelStyle', `${sharedTheme}\n${theme}\nreturn loadTheme;`)(
            { file_get_contents(path) { reads.push(path); return [true, new TextEncoder().encode(contents)]; } },
            options => calls.push(options));
        return { loadTheme, calls, reads };
    }

    test('scoped declarations cover every CSS reference without changing the raw palette', () => {
        const css = readFileSync(new URL('./style.css', import.meta.url), 'utf8');
        const colors = { ...palette, extraColor: '#AbCdEf' };
        const input = { ...colors, 'bad-key': '#123456', 'bad; } .other {': '#123456',
            notColor: 'transparent', shortHex: '#123', alphaHex: '#12345678', metadata: { keep: true } };
        const h = themeFixture(JSON.stringify(input));
        assert.deepEqual(h.loadTheme('fixture/frontend.js', 'fixture/palette.json'), { palette: input });
        assert.deepEqual(h.reads, ['fixture/palette.json']);
        assert.equal(h.calls.length, 1);
        assert.equal(h.calls[0].entry, 'fixture/frontend.js');
        assert.equal(h.calls[0].label, 'Media');
        const { definitions } = h.calls[0];
        assert.equal(definitions, '.media-panel {\n' + Object.entries(colors)
            .map(([key, value]) => `    --mp_${key}: ${value};`).join('\n') + '\n}');
        assert.doesNotMatch(viewSource, /@define-color|@mp_/);
        assert.doesNotMatch(css, /@define-color|@mp_|\balpha\s*\(|#[a-f\d]{3,8}\b/i);
        const declarations = new Set([...definitions.matchAll(/(--[^\s:]+):/g)].map(([, name]) => name));
        const references = new Set([...css.matchAll(/\bvar\(\s*(--[^,\s)]+)/g)].map(([, name]) => name));
        assert.deepEqual([...references].sort(), required.map(key => `--mp_${key}`).sort());
        for (const name of references) assert.ok(declarations.has(name), `Missing declaration for ${name}`);
        const mixes = [...css.matchAll(/color-mix\(in srgb, var\(--mp_([a-zA-Z]+)\) ([\d.]+)%, transparent\)/g)]
            .map(([, key, percent]) => [key, percent]);
        assert.deepEqual(mixes, [
            ['yellow', '15'], ['separator', '40'], ['foreground', '50'], ['pink', '33.3'],
            ['pink', '45'], ['foreground', '25'], ['dimPink', '35'], ['yellow', '45'],
            ['red', '10'], ['red', '55'], ['pink', '50'],
        ]);
    });

    test('invalid required colors and malformed JSON never reach the stylesheet loader', () => {
        for (const key of required) {
            for (const value of [undefined, null, 'transparent', '#123', '#12345678', '#gggggg']) {
                const h = themeFixture(JSON.stringify({ ...palette, [key]: value }));
                assert.throws(() => h.loadTheme('fixture/frontend.js', 'fixture/palette.json'),
                    { message: `Invalid media palette colour: ${key}` });
                assert.equal(h.calls.length, 0, `Invalid ${key} must not load CSS`);
            }
        }
        for (const contents of ['', '{']) {
            const h = themeFixture(contents);
            assert.throws(() => h.loadTheme('fixture/frontend.js', 'fixture/palette.json'), SyntaxError);
            assert.equal(h.calls.length, 0, 'Malformed JSON must not load CSS');
        }
    });
});

test('media frame delegates to shared drawing without clamping small allocations', async () => {
    assert.match(viewSource, /import \{ drawCutCornerFrame \} from '..\/..\/shared\/astal\/drawing\.js'/);
    const { drawCutCornerFrame } = await import('../../shared/astal/drawing.js');
    let draw;
    const frame = new Function('Gtk', 'drawCutCornerFrame',
        `${viewSource.match(/function frame\([\s\S]*?\n\}/)[0]}\nreturn frame;`)(
        { DrawingArea: function(properties) { return { ...properties, set_draw_func(callback) { draw = callback; } }; } },
        drawCutCornerFrame);
    const widget = frame({ yellow: '#AbCdEf' });
    assert.equal(widget.can_target, false);
    assert.equal(widget.hexpand, true);
    assert.equal(widget.vexpand, true);
    for (const [width, height] of [[400, 200], [8, 6], [0, 0]]) {
        const calls = [];
        const cr = Object.fromEntries(['moveTo', 'lineTo', 'closePath', 'setSourceRGBA', 'fillPreserve', 'setLineWidth', 'stroke']
            .map(name => [name, (...args) => calls.push([name, ...args])]));
        draw(widget, cr, width, height);
        assert.deepEqual(calls, [
            ['moveTo', 10.5, 0.5], ['lineTo', width - 0.5, 0.5],
            ['lineTo', width - 0.5, height - 10.5], ['lineTo', width - 10.5, height - 0.5],
            ['lineTo', 0.5, height - 0.5], ['lineTo', 0.5, 10.5], ['closePath'],
            ['setSourceRGBA', 37 / 255, 37 / 255, 47 / 255, 0.9], ['fillPreserve'],
            ['setSourceRGBA', 171 / 255, 205 / 255, 239 / 255, 1], ['setLineWidth', 1], ['stroke'],
        ]);
    }
});

test('media requests retain local parsing and dispatch with shared synchronous-close replies', () => {
    assert.match(viewSource, /import \{ replyAndClose \} from '..\/..\/shared\/astal\/socket\.js'/);
    const socket = readFileSync(new URL('../../shared/astal/socket.js', import.meta.url), 'utf8')
        .replace(/^import .*;$/gm, '').replace(/^export /gm, '');
    const events = [], warnings = [];
    let failWrite = false;
    const fixture = new Function('GObject', 'Astal', 'AstalIO', 'console', `${socket}\n`
        + viewSource.slice(viewSource.indexOf('let backend, panel;'), viewSource.indexOf('\nGtk.init();'))
        + '\nreturn { MediaApplication, setState(b, p) { backend = b; panel = p; } };')(
        { registerClass: value => value }, { Application: class {} }, {
            write_sock(connection, response, callback) {
                events.push(['reply', response]);
                if (failWrite) throw new Error('write failed');
                callback(connection, 'written');
            },
            write_sock_finish(result) { events.push(['finish', result]); },
        }, { warn: message => warnings.push(message) });
    const app = new fixture.MediaApplication();
    const connection = { close(value) { events.push(['close', value]); } };
    fixture.setState({ status: () => 'status' }, { close: () => events.push(['dispatch']) });
    app.vfunc_request('{"command":"status"}', connection);
    assert.deepEqual(events, [['reply', 'status'], ['finish', 'written'], ['close', null]]);
    events.length = 0;
    app.vfunc_request('{"command":"close"}', connection);
    assert.deepEqual(events, [['dispatch'], ['reply', 'ok'], ['finish', 'written'], ['close', null]]);
    events.length = 0;
    fixture.setState(null, null);
    app.vfunc_request('null', connection);
    assert.deepEqual(JSON.parse(events[0][1]), { error: 'A media panel command object is required.' });
    events.length = 0;
    failWrite = true;
    app.vfunc_request('{"command":"status"}', connection);
    assert.deepEqual(JSON.parse(events[0][1]), { error: 'Media panel is starting.' });
    assert.equal(events.filter(([phase]) => phase === 'close').length, 1);
    assert.deepEqual(warnings, ['Media request reply: write failed']);
});

test('source rows use dim yellow interactions with a later dim red current override and unchanged tint', () => {
    const css = readFileSync(new URL('./style.css', import.meta.url), 'utf8');
    const rules = [...css.matchAll(/([^{}]+)\{([^{}]*)\}/g)];
    const matchingRules = (...selectors) => rules.filter(([, group]) => group.split(',').some(value => selectors.includes(value.trim())));
    const rule = selector => {
        const [match] = matchingRules(selector);
        assert.ok(match, `Missing rule for ${selector}`);
        return match[2];
    };
    const row = rule('.media-panel .source-row');
    assert.match(row, /border: 1px solid transparent;/);
    assert.match(row, /background: transparent;/);
    assert.match(row, /background-image: none;/);
    // Reserve the border inside the existing 42px height and 8px/6px side insets.
    assert.match(row, /min-height: 40px;/);
    assert.match(row, /padding: 0 7px 0 5px;/);
    for (const state of ['.current', ':hover', ':focus-within', ':active']) {
        const selected = rule(`.media-panel .source-row${state}`);
        assert.match(selected, /border-color: color-mix\(in srgb, var\(--mp_yellow\) 45%, transparent\);/);
        assert.match(selected, /background: color-mix\(in srgb, var\(--mp_red\) 10%, transparent\);/);
        assert.doesNotMatch(selected, /(?:border(?:-width)?|padding|margin|min-height|min-width):/);
    }
    const current = '.media-panel .source-row.current';
    const currentRules = matchingRules(current);
    assert.equal(currentRules.length, 2);
    assert.equal(currentRules[1][1].trim(), current);
    assert.equal(currentRules[1][2].trim(), 'border-color: color-mix(in srgb, var(--mp_red) 55%, transparent);');
    for (const state of ['', ':hover', ':focus-within', ':active']) {
        // Both selectors have equal specificity, so the last border color wins.
        const colors = matchingRules(current, `.media-panel .source-row${state}`)
            .flatMap(([, , body]) => [...body.matchAll(/border-color:\s*([^;]+);/g)].map(match => match[1]));
        assert.deepEqual(colors, ['color-mix(in srgb, var(--mp_yellow) 45%, transparent)', 'color-mix(in srgb, var(--mp_red) 55%, transparent)'], `Current must win over ${state || 'idle'}`);
    }
    for (const state of ['', ':hover', ':focus-visible', ':active']) {
        const select = rule(`.media-panel button.source-select${state}`);
        assert.match(select, /border: none;/);
        assert.match(select, /background: transparent;/);
        assert.match(select, /background-image: none;/);
        assert.match(select, /box-shadow: none;/);
        assert.match(select, /outline: none;/);
    }
});

test('media never creates visual tooltips or query handlers, including metadata labels', () => {
    assert.doesNotMatch(viewSource, /tooltip|Gtk\.Settings/i);
});

test('icon buttons retain accessible action labels and clicks without tooltips', () => {
    class Widget {
        constructor(properties) { Object.assign(this, accessibleWidget(properties)); }
        connect(name, callback) { this[name] = callback; }
    }
    const Gtk = { Button: Widget, Label: Widget, Align: { CENTER: 0 }, AccessibleProperty: accessibleProperty };
    const button = new Function('Gtk',
        viewSource.slice(viewSource.indexOf('function button('), viewSource.indexOf('function scale('))
        + '\nreturn button;')(Gtk);
    let clicks = 0;
    const widget = button('󰐊', 'Play / pause', () => { clicks++; }, ['play-button']);
    assert.equal(widget.a11y.label, 'Play / pause');
    assert.equal(widget.child.label, '󰐊');
    assert.deepEqual(widget.css_classes, ['icon-button', 'play-button']);
    assert.equal(widget.tooltip_text, undefined);
    assert.equal(widget.tooltip_markup, undefined);
    widget.clicked();
    assert.equal(clicks, 1);
});

test('the entire built media tree stays tooltip-free through source, title and capability rerenders', () => {
    class Widget {
        constructor(properties = {}) {
            this.children = [];
            this.signals = new Map();
            this.sensitive = true;
            Object.assign(this, accessibleWidget(properties));
            if (this.child) this.child.parent = this;
        }
        append(child) { this.children.push(child); child.parent = this; }
        remove(child) { this.children.splice(this.children.indexOf(child), 1); child.parent = null; }
        reorder_child_after(child, previous) {
            this.remove(child);
            this.children.splice(previous ? this.children.indexOf(previous) + 1 : 0, 0, child);
            child.parent = this;
        }
        connect(name, callback) { this.signals.set(name, callback); }
        emit(name) { this.signals.get(name)?.(this); }
        get_child() { return this.child; }
        set_label(text) { this.label = text; }
        set_value(value) { this.value = value; }
        get_value() { return this.value; }
        is_sensitive() { return this.sensitive && (!this.parent || this.parent.is_sensitive()); }
        add_css_class(name) { this.css_classes = [...new Set([...(this.css_classes || []), name])]; }
        remove_css_class(name) { this.css_classes = this.css_classes?.filter(value => value !== name); }
    }
    class Scale extends Widget {}
    const Gtk = { Box: Widget, Label: Widget, Button: Widget, Scale, Adjustment: Widget,
        Separator: Widget, Revealer: Widget, ScrolledWindow: Widget, AccessibleProperty: accessibleProperty,
        Orientation: { HORIZONTAL: 0, VERTICAL: 1 }, Align: { CENTER: 0 },
        PolicyType: { NEVER: 0, AUTOMATIC: 1 },
        RevealerTransitionType: { SLIDE_DOWN: 0 } };
    class Artwork {
        constructor() { this.widget = new Widget(); }
        set() {}
        dispose() {}
    }
    class Credit { set() {} }
    const MediaPanel = new Function('Gtk', 'Pango', 'Artwork', 'Credit', 'formatTime',
        viewSource.slice(viewSource.indexOf('function box('), viewSource.indexOf('function frame('))
        + viewSource.slice(viewSource.indexOf('class MediaPanel {'), viewSource.indexOf('\nlet backend, panel;'))
        + '\nreturn MediaPanel;')(Gtk, { WrapMode: { WORD_CHAR: 0 }, EllipsizeMode: { END: 0 } }, Artwork, Credit, formatTime);
    const actions = [];
    let enabled = true;
    const backend = { current: () => enabled, transport: () => ({ enabled, reason: enabled ? '' : 'Unavailable' }),
        player: bus => panel.groups.find(group => group.primary.bus === bus)?.primary,
        progress: bus => {
            const player = backend.player(bus);
            return player && { samplePosition: player.position, ...player };
        },
        action: (...args) => { actions.push(args); return true; } };
    const panel = Object.assign(Object.create(MediaPanel.prototype), {
        backend, content: new Widget(), groups: [], activePlayer: '', rows: new Map(), opened: true, failure: '', drag() {},
    });
    const checkTree = widget => {
        assert.ok(!widget.tooltip_text && !widget.tooltip_markup && !widget.has_tooltip);
        assert.equal(widget.signals.has('query-tooltip'), false);
        assert.doesNotMatch(widget.a11y.label || '', /volume|mute/i);
        assert.ok((widget.css_classes || []).every(name => !name.startsWith('volume')));
        widget.children.forEach(checkTree);
        if (widget.child) checkTree(widget.child);
    };
    panel.build();
    checkTree(panel.content);
    assert.deepEqual(panel.transport.children, [panel.rewind, panel.previous, panel.play, panel.next, panel.forward]);
    assert.equal(panel.transport.orientation, Gtk.Orientation.HORIZONTAL);
    assert.equal(panel.transport.spacing, 14);
    assert.equal(panel.transport.halign, Gtk.Align.CENTER);
    assert.equal(panel.transport.parent, panel.details);
    assert.equal(panel.rewind.child.label, '−15');
    assert.equal(panel.forward.child.label, '+15');
    assert.equal(panel.rewind.a11y.label, 'Rewind 15 seconds');
    assert.equal(panel.forward.a11y.label, 'Forward 15 seconds');
    for (const widget of [panel.rewind, panel.forward]) assert.ok(widget.css_classes.includes('seek-button'));
    const scales = [];
    const findScales = widget => {
        if (widget instanceof Scale) scales.push(widget);
        widget.children.forEach(findScales);
        if (widget.child) findScales(widget.child);
    };
    findScales(panel.content);
    assert.deepEqual(scales, [panel.seek], 'the track progress slider is the only remaining scale');
    assert.equal(panel.rewind.sensitive, false);
    assert.equal(panel.forward.sensitive, false);
    const group = (bus, title) => ({ displayName: `Source ${bus}`, title,
        primary: { bus, title, instance: `player-${bus}`, trackKey: title, canPrevious: true, canNext: true,
            canSeek: true, positionSupported: true, position: 30, length: 90 } });
    for (const title of ['Long <literal> title & 🎵'.repeat(10), 'Renamed track', '']) {
        panel.setModel([group('one', title), group('two', 'Other track')], 'one');
        checkTree(panel.content);
        assert.equal(panel.title.a11y.label, title || 'Unknown title');
        assert.equal(panel.rows.get('one').widget.a11y.description, `Source one\n${title}`);
    }
    panel.play.emit('clicked');
    panel.previous.emit('clicked');
    panel.next.emit('clicked');
    panel.rows.get('two').play.emit('clicked');
    panel.rewind.emit('clicked');
    panel.forward.emit('clicked');
    assert.deepEqual(actions, [['toggle', 'one'], ['previous', 'one'], ['next', 'one'], ['toggle', 'two'],
        ['jump', 'one', -15], ['jump', 'one', 15]]);
    for (enabled of [false, true]) {
        panel.render();
        checkTree(panel.content);
        assert.equal(panel.play.sensitive, enabled);
        assert.equal(panel.play.a11y.label, 'Play / pause');
        assert.equal(panel.rewind.sensitive, enabled);
        assert.equal(panel.forward.sensitive, enabled);
    }
    panel.setModel([group('two', 'Replacement')], 'two');
    checkTree(panel.content);
    assert.equal(panel.rows.has('one'), false);
    assert.equal(panel.title.a11y.label, 'Replacement');
    panel.setModel([], '');
    checkTree(panel.content);
    assert.equal(panel.title.a11y.label, 'Unknown title');
    assert.equal(panel.rows.size, 0);
    assert.equal(panel.rewind.sensitive, false);
    assert.equal(panel.forward.sensitive, false);
});

test('media runtime supports AVIF artwork and refreshes native progress while open', () => {
    const runtime = readFileSync(new URL('../../shared/default.nix', import.meta.url), 'utf8');
    const recipe = readFileSync(new URL('../default.nix', import.meta.url), 'utf8');
    assert.match(runtime, /pkgs\.libavif/);
    assert.match(recipe, /imageLoaders\s*=\s*true\s*;/);
    assert.match(viewSource, /positionTimer = GLib\.timeout_add/);
    assert.match(viewSource, /GLib\.source_remove\(this\.positionTimer\)/);
});

test('elapsed time, seek bar, and total time share one compact horizontal row', () => {
    assert.match(viewSource, /this\.total\.xalign = 1;\s*this\.details\.append\(box\(false, 6, \[this\.elapsed, this\.seek, this\.total\]\)\);/);
    assert.doesNotMatch(viewSource, /this\.details\.append\(box\(true, 4, \[this\.seek,/);
});

test('the media frontend has no volume or mute controls, helper, or retained state', () => {
    assert.doesNotMatch(viewSource, /function volumeBar\(|this\.volume\w*|this\.backend\.volume\(|Mute \/ unmute/);
});

test('remote page URLs never reach the local artwork extractor after decoding fails', async () => {
    const Artwork = new Function('Gio', 'console',
        viewSource.slice(viewSource.indexOf('class Artwork {'), viewSource.indexOf('class Credit {'))
        + '; return Artwork;')({ Cancellable }, { warn() {} });
    for (const url of ['https://www.youtube.com/watch?v=test', 'file:///music/track.flac']) {
        const calls = [];
        const art = Object.create(Artwork.prototype);
        Object.assign(art, {
            key: '', generation: 0, picture: { set_paintable() {} },
            backend: { async resolveArtwork(value) { calls.push(value); return '/cache/local.png'; } },
            async load(path) { if (path === '/cache/native.avif') throw new Error('Unsupported format'); },
        });
        art.set('/cache/native.avif', url);
        await new Promise(resolve => setImmediate(resolve));
        assert.deepEqual(calls, url.startsWith('file:') ? [url] : []);
    }
});

describe('Media view transport feedback', () => {
    const Gtk = { AccessibleProperty: accessibleProperty };
    const trackKey = player => player ? JSON.stringify([player.bus, player.instance, player.trackKey]) : '';
    const MediaPanel = new Function('Gtk', 'trackKey', 'formatTime',
        viewSource.slice(viewSource.indexOf('class MediaPanel {'), viewSource.indexOf('\nlet backend, panel;'))
        + '\nreturn MediaPanel;')(Gtk, trackKey, formatTime);

    test('stale, unsupported, and rejected actions explain why nothing was dispatched', () => {
        let current = false, enabled = true, accepted = true, refreshes = 0;
        const calls = [], failures = [];
        const panel = Object.assign(Object.create(MediaPanel.prototype), {
            group: { primary: { bus: 'browser' } },
            backend: {
                current: () => current, refresh() { refreshes++; }, failure: '',
                reportFailure(text) { this.failure = text; },
                transport: () => ({ enabled, reason: 'Browser does not expose pause control.' }),
                action(kind, bus) { calls.push({ kind, bus }); return accepted; },
            },
            setFailure(text) { failures.push(text); },
        });
        assert.equal(panel.action('toggle'), false);
        assert.equal(refreshes, 1);
        assert.match(failures.at(-1), /changed.*try the action again/);
        assert.deepEqual(calls, []);
        current = true;
        enabled = false;
        assert.equal(panel.action('toggle'), false);
        assert.match(failures.at(-1), /does not expose pause/);
        assert.deepEqual(calls, []);
        enabled = true;
        accepted = false;
        panel.backend.failure = 'Media worker is not ready.';
        assert.equal(panel.action('toggle'), false);
        assert.equal(failures.at(-1), panel.backend.failure);
        accepted = true;
        assert.equal(panel.action('toggle'), true);
        assert.deepEqual(calls.at(-1), { kind: 'toggle', bus: 'browser' });
        panel.group = null;
        assert.equal(panel.action('toggle'), false);
        assert.match(failures.at(-1), /No media source/);
    });

    const unavailableSeekSamples = [
        ['no live player', null],
        ['seek unsupported', { canSeek: false }],
        ['position unsupported', { positionSupported: false }],
        ['position support missing', { positionSupported: undefined }],
        ...[undefined, null, NaN, Infinity, -Infinity, '30'].map(position => [`position ${String(position)}`, { position }]),
        ...[undefined, null, NaN, Infinity, -1, '30'].map(samplePosition => [`sample ${String(samplePosition)}`, { samplePosition }]),
        ...[undefined, null, 0, -1, NaN, Infinity, -Infinity, '90'].map(length => [`length ${String(length)}`, { length }]),
        ['changed track', { trackKey: '/track/two' }],
        ['replaced player instance', { instance: 'player-2' }],
        ['different bus', { bus: 'other-player' }],
    ];

    function jumpHarness() {
        const selected = { bus: 'browser', instance: 'player-1', trackKey: '/track/one',
            canSeek: true, positionSupported: true, position: 10, length: 90 };
        const h = { selected, live: { ...selected, position: 30 }, current: true, accepted: true,
            actions: [], failures: [], reports: [], resets: 0 };
        h.panel = Object.assign(Object.create(MediaPanel.prototype), {
            group: { primary: selected },
            seekDragging: false, seekPending: false, seekTarget: null,
            seek: { get_value: () => 80 },
            backend: {
                current: () => h.current, refresh() {}, failure: '',
                player(bus) { assert.equal(bus, selected.bus); return h.live; },
                progress(bus) {
                    assert.equal(bus, selected.bus);
                    const player = h.progress === undefined ? h.live : h.progress;
                    return player && { samplePosition: player.position, ...player };
                },
                action(...args) { h.actions.push(args); return h.accepted; },
                reportFailure(text) { this.failure = text; h.reports.push(text); },
            },
            setFailure(text) { h.failures.push(text); },
            position() { h.resets++; },
        });
        return h;
    }

    test('progress uses a known duration and never fills an unknown or unsupported range', () => {
        let player = { position: 86, positionSupported: true, length: 361.9294375 };
        let value = 0, elapsed = '', total = '';
        const panel = Object.assign(Object.create(MediaPanel.prototype), {
            group: { primary: { bus: 'org.mpris.MediaPlayer2.test' } }, seekDragging: false,
            backend: { progress: () => player },
            rewind: {}, forward: {}, modelCurrent: () => true,
            seek: { set_value(next) { value = next; }, get_value: () => value },
            elapsed: { set_label(text) { elapsed = text; } },
            total: { set_label(text) { total = text; } },
        });
        panel.position();
        assert.equal(value, 86);
        assert.equal(elapsed, '1:26');
        assert.equal(total, '6:01');
        for (const length of [null, undefined, 0, -1, NaN, Infinity]) {
            player.length = length;
            panel.position();
            assert.equal(value, 0, `unknown duration ${String(length)} must not fill the bar`);
            assert.equal(elapsed, '1:26', 'elapsed remains available even without duration');
            assert.equal(total, '--:--');
        }
        player.length = 60;
        panel.position();
        assert.equal(value, 60, 'only the displayed thumb is clamped to the range');
        assert.equal(player.position, 86);
        assert.equal(elapsed, '1:26');
        player.positionSupported = false;
        panel.position();
        assert.equal(value, 0);
        assert.equal(elapsed, '--:--');
        panel.seekDragging = true;
        value = 30;
        panel.position();
        assert.equal(value, 30, 'live updates preserve the scrub preview');
        assert.equal(elapsed, '0:30');
        player = null;
        panel.seekDragging = false;
        panel.position();
        assert.equal(value, 0);
        assert.equal(elapsed, '--:--');
        assert.equal(total, '--:--');
    });

    test('15-second jumps delegate signed offsets to Rust, not model or slider positions', () => {
        const h = jumpHarness();
        h.panel.seekBy(-15);
        h.live.position = 50;
        h.panel.seekBy(15);
        assert.deepEqual(h.actions, [['jump', 'browser', -15], ['jump', 'browser', 15]]);
        assert.equal(h.selected.position, 10);
        assert.equal(h.live.position, 50, 'jumping must not fabricate a native position update');
        assert.deepEqual(h.failures, []);
    });

    test('display estimates are never converted into absolute jump targets by the frontend', () => {
        const h = jumpHarness();
        h.progress = { ...h.live, position: 34.5, samplePosition: 30 };
        h.panel.seekBy(15);
        assert.deepEqual(h.actions, [['jump', 'browser', 15]], 'Rust chooses the native sample and absolute target');
        assert.equal(h.progress.position, 34.5);
        assert.equal(h.progress.samplePosition, 30);
    });

    test('jump requests do not select between cached native and current-track progress samples', () => {
        const h = jumpHarness();
        h.live.position = 100;
        h.progress = { ...h.live, position: 20, samplePosition: 20 };
        h.panel.seekBy(15);
        assert.deepEqual(h.actions, [['jump', 'browser', 15]]);
    });

    test('15-second requests leave boundary clamping to Rust, including genuine zero position', () => {
        for (const [position, seconds] of [[2, -15], [35, 15], [0, 15], [40, -15]]) {
            const h = jumpHarness();
            h.live.position = position;
            h.live.length = 40;
            h.panel.seekBy(seconds);
            assert.deepEqual(h.actions, [['jump', 'browser', seconds]], `position ${position}, jump ${seconds}`);
        }
    });

    test('jump buttons disable for unavailable progress or a changed track identity', () => {
        for (const [name, overrides] of unavailableSeekSamples) {
            const h = jumpHarness();
            h.live = overrides && { ...h.live, ...overrides };
            Object.assign(h.panel, {
                rewind: {}, forward: {}, seek: { set_value() {}, get_value: () => 0 },
                elapsed: { set_label() {} }, total: { set_label() {} },
            });
            MediaPanel.prototype.position.call(h.panel);
            assert.equal(h.panel.rewind.sensitive, false, name);
            assert.equal(h.panel.forward.sensitive, false, name);
        }
    });

    test('jumps never dispatch without a selected group or with a stale model', () => {
        for (const seconds of [-15, 15]) {
            const missing = jumpHarness();
            missing.panel.group = null;
            missing.panel.seekBy(seconds);
            assert.deepEqual(missing.actions, [], 'no selected source');
            const stale = jumpHarness();
            stale.current = false;
            stale.panel.seekBy(seconds);
            assert.deepEqual(stale.actions, [], 'stale ownership model');
        }
    });

    test('a jump cancels a pending scrub before queuing and release cannot commit the old thumb value', () => {
        const h = jumpHarness();
        const panel = h.panel;
        panel.seekDragging = true;
        panel.seekPending = true;
        panel.seekTarget = trackKey(h.selected);
        panel.backend.action = (...args) => {
            assert.equal(panel.seekDragging, false, 'cancel drag before dispatch');
            assert.equal(panel.seekPending, false, 'discard preview before dispatch');
            assert.equal(panel.seekTarget, null, 'discard captured scrub identity');
            h.actions.push(args);
            return true;
        };
        panel.seekBy(15);
        assert.equal(h.resets, 1, 'cancel restores the sampled native position');
        panel.finishSeek(true);
        assert.deepEqual(h.actions, [['jump', 'browser', 15]], 'release must not queue the old 80-second preview');
    });

    test('rejected jump queues report backend failures and a nonempty fallback through reportFailure', () => {
        for (const failure of ['Media worker is not ready.', '']) {
            const h = jumpHarness();
            h.accepted = false;
            h.panel.backend.failure = failure;
            h.panel.seekBy(15);
            assert.deepEqual(h.actions, [['jump', 'browser', 15]]);
            assert.equal(h.reports.length, 1);
            assert.equal(h.failures.length, 1);
            assert.ok(h.failures[0].length > 0);
            assert.equal(h.reports[0], h.failures[0]);
            assert.equal(h.panel.backend.failure, h.failures[0], 'recovery refreshes retain the view error');
            if (failure) assert.equal(h.failures[0], failure);
        }
    });

    test('seek scrubbing previews locally and dispatches one guarded seek on release', () => {
        let changeValue;
        const player = { bus: 'browser', instance: 'player-1', trackKey: '/track/one' };
        const actions = [], previews = [];
        let position = 10;
        const panel = Object.assign(Object.create(MediaPanel.prototype), {
            group: { primary: player },
            seekTarget: JSON.stringify([player.bus, player.instance, player.trackKey]),
            seekDragging: true, seekPending: false,
            seek: { connect(_name, callback) { changeValue = callback; }, get_value: () => position },
            elapsed: { set_label(text) { previews.push(text); } },
            backend: { player: bus => bus === player.bus ? player : null, action(...args) { actions.push(args); } },
            modelCurrent: () => true,
        });
        const start = viewSource.indexOf("this.seek.connect('change-value', ");
        const end = viewSource.indexOf('\n        this.drag(this.seek', start);
        const handler = viewSource.slice(start, end);
        new Function('trackKey', 'formatTime', handler).call(panel,
            nativePlayer => nativePlayer ? JSON.stringify([nativePlayer.bus, nativePlayer.instance, nativePlayer.trackKey]) : '',
            seconds => `${Math.floor(seconds)}s`);
        changeValue(null, null, 25);
        position = 30;
        changeValue(null, null, 30);
        assert.deepEqual(actions, [], 'dragging must not dispatch a seek for every motion event');
        assert.deepEqual(previews, ['25s', '30s'], 'elapsed time continues to preview the thumb position');
        panel.finishSeek(true);
        assert.deepEqual(actions, [['seek', 'browser', 30]], 'release dispatches only the final position');
        assert.equal(panel.seekDragging, false);
        assert.equal(panel.seekPending, false);
    });

    test('cancelled or stale seek scrubs never dispatch', () => {
        const player = { bus: 'browser', instance: 'player-1', trackKey: '/track/one' };
        const actions = [];
        let resets = 0;
        const panel = Object.assign(Object.create(MediaPanel.prototype), {
            group: { primary: player }, seekTarget: JSON.stringify([player.bus, player.instance, player.trackKey]),
            seekDragging: true, seekPending: true, seek: { get_value: () => 40 },
            backend: { player: () => player, action(...args) { actions.push(args); } },
            modelCurrent: () => true, position() { resets++; },
        });
        panel.finishSeek(false);
        assert.deepEqual(actions, []);
        assert.equal(resets, 1, 'cancel restores the sampled native position');

        panel.seekDragging = true;
        panel.seekPending = true;
        panel.seekTarget = JSON.stringify([player.bus, player.instance, player.trackKey]);
        player.trackKey = '/track/changed';
        panel.finishSeek(true);
        assert.deepEqual(actions, [], 'a changed track invalidates the captured scrub target');
        assert.equal(panel.seekPending, false);
    });

    test('release handling commits completed seeks and discards touch cancellation', () => {
        assert.match(viewSource, /type === Gdk\.EventType\.BUTTON_RELEASE \|\| type === Gdk\.EventType\.TOUCH_END\)\s*this\.finishSeek\(true\)/);
        assert.match(viewSource, /type === Gdk\.EventType\.TOUCH_CANCEL\)\s*this\.finishSeek\(false\)/);
    });

    test('recovery model refreshes preserve view-originated stale-action errors', () => {
        let current = false, refreshes = 0;
        const backend = { failure: '', current: () => current, refresh() { refreshes++; },
            reportFailure(text) { this.failure = text; }, resolveGroups: () => [], activePlayer: '' };
        const panel = Object.assign(Object.create(MediaPanel.prototype), {
            group: { primary: { bus: 'browser' } }, backend, opened: true,
            errorLabel: { set_label(text) { this.text = text; } }, setModel() {},
        });
        assert.equal(panel.action('next'), false);
        assert.equal(refreshes, 1);
        const failure = panel.failure;
        assert.match(failure, /try the action again/);
        current = true;
        const start = viewSource.indexOf('\nfunction render()');
        const render = new Function('panel', 'backend',
            viewSource.slice(start, viewSource.indexOf('\nfunction monitorFor(', start)) + '\nreturn render;')(panel, backend);
        render();
        assert.equal(panel.failure, failure);
        assert.equal(panel.errorLabel.text, failure);
        assert.equal(panel.errorLabel.visible, true);
    });

    test('main and source-row controls share capability feedback and jumps follow live seek availability', () => {
        const label = () => accessibleWidget({ set_label(text) { this.text = text; } });
        const button = () => { const child = label(); return accessibleWidget({ get_child: () => child }); };
        let enabled = false, current = true;
        const group = { title: 'Track', displayName: 'Spotify', primary: {
            bus: 'browser', title: 'Track', playing: true, instance: 'player-1', trackKey: '/track/one',
            canSeek: true, positionSupported: true, position: 10, length: 90,
        }, players: ['browser'] };
        let live = { ...group.primary, position: 30 };
        const row = { name: label(), title: label(), play: button(), art: { set() {} },
            widget: accessibleWidget({ add_css_class() {}, remove_css_class() {} }) };
        const panel = Object.assign(Object.create(MediaPanel.prototype), {
            groups: [group], activePlayer: 'browser', rows: new Map([['browser', row]]), opened: true,
            backend: {
                current: () => current,
                transport: () => ({ enabled, reason: enabled ? 'Pause' : 'Browser bridge does not expose pause control.' }),
                player: () => live, progress: () => live && { samplePosition: live.position, ...live },
            },
            title: label(), credit: { set() {} }, art: { set() {} }, empty: {}, details: {},
            play: button(), previous: {}, next: {}, rewind: {}, forward: {}, seek: { adjustment: {} },
            sourceName: label(),
            section: {}, sources: {}, chevron: label(), sourceRows: { reorder_child_after() {} },
            setFailure() {}, position() {},
        });
        const assertNoControlTooltips = () => {
            for (const widget of [panel.play, panel.previous, panel.next, panel.rewind, panel.forward, panel.seek,
                panel.section, panel.sources, panel.details, panel.title, row.name, row.title, row.play, row.widget]) {
                assert.equal(widget.tooltip_text, undefined);
                assert.equal(widget.tooltip_markup, undefined);
            }
        };
        panel.render();
        assertNoControlTooltips();
        assert.equal(panel.title.a11y.label, 'Track', 'the full title remains accessible without a tooltip');
        assert.equal(row.widget.a11y.description, 'Spotify\nTrack');
        assert.equal(panel.play.sensitive, false);
        assert.equal(row.play.sensitive, false);
        assert.match(panel.play.a11y.description, /does not expose pause/);
        assert.equal(row.play.a11y.description, panel.play.a11y.description);
        assert.equal(panel.rewind.sensitive, true, 'seek availability is independent of play / pause');
        assert.equal(panel.forward.sensitive, true);
        enabled = true;
        panel.render();
        assertNoControlTooltips();
        assert.equal(panel.play.sensitive, true);
        assert.equal(row.play.sensitive, true);
        assert.equal(panel.play.a11y.description, 'Pause');
        assert.equal(row.play.a11y.description, panel.play.a11y.description);
        for (const [name, overrides] of unavailableSeekSamples) {
            live = overrides && { ...group.primary, ...overrides };
            panel.render();
            assertNoControlTooltips();
            assert.equal(panel.rewind.sensitive, false, name);
            assert.equal(panel.forward.sensitive, false, name);
        }
        live = { ...group.primary, position: 0 };
        for (current of [false, true]) {
            panel.render();
            assert.equal(panel.rewind.sensitive, current, 'fresh model and genuine zero native position');
            assert.equal(panel.forward.sensitive, current, 'fresh model and genuine zero native position');
        }
        group.primary.title = 'Renamed <track> & 🎵';
        panel.render();
        assertNoControlTooltips();
        assert.equal(panel.title.a11y.label, group.primary.title);
        group.primary.title = '';
        panel.render();
        assertNoControlTooltips();
        assert.equal(panel.title.a11y.label, 'Unknown title');

        const visibility = [];
        panel.credit.set = (_, active) => visibility.push(['credit', active]);
        panel.art.set = (_, __, active) => visibility.push(['art', active]);
        row.art.set = (_, __, active) => visibility.push(['row-art', active]);
        panel.opened = false;
        panel.render();
        assert.deepEqual(visibility, [['credit', false], ['art', false], ['row-art', false]],
            'closing performs one cleanup render for outstanding lookups');
        panel.setModel([group], 'browser');
        panel.setModel([group], 'browser');
        assert.equal(visibility.length, 3, 'later hidden model updates do not touch widgets');
        group.primary.title = 'Changed while hidden';
        panel.opened = true;
        panel.render();
        assert.equal(panel.title.a11y.label, 'Changed while hidden');
        assert.equal(visibility.length, 6, 'opening renders the current metadata again');
    });

    test('hidden model callbacks skip native snapshot resolution and GTK updates', () => {
        const panel = { opened: false, setModel() { assert.fail('Hidden GTK model update'); },
            setFailure() { assert.fail('Hidden GTK error update'); } };
        const backend = { resolveGroups() { assert.fail('Hidden metadata snapshot resolution'); } };
        const start = viewSource.indexOf('\nfunction render()');
        const render = new Function('panel', 'backend',
            viewSource.slice(start, viewSource.indexOf('\nfunction monitorFor(', start)) + '\nreturn render;')(panel, backend);
        for (let index = 0; index < 100; index++) render();
    });

    test('hidden failure state is retained without writing GTK labels', () => {
        const panel = Object.assign(Object.create(MediaPanel.prototype), {
            opened: false, errorLabel: { set_label() { assert.fail('Hidden label mutation'); } },
        });
        panel.setFailure('Retry when this panel opens.');
        assert.equal(panel.failure, 'Retry when this panel opens.');
    });

    test('opening resolves the latest model and failure before displaying controls', () => {
        const prototype = new Function('Astal', 'GLib', 'closeNetwork',
            viewSource.slice(viewSource.indexOf('class MediaPanel {'), viewSource.indexOf('\nlet backend, panel;'))
            + '\nreturn MediaPanel.prototype;')({ Keymode: { EXCLUSIVE: 1 } }, {}, () => {});
        const latest = [{ primary: { bus: 'new-source' } }];
        let refreshes = 0;
        const panel = Object.assign(Object.create(prototype), {
            opened: false, positionTimer: 1, card: {}, window: {}, revealer: {},
            backend: { activePlayer: 'new-source', failure: 'Current error',
                refresh() { refreshes++; }, resolveGroups: () => latest },
            setModel(groups, active) { this.groups = groups; this.activePlayer = active; },
            setFailure(text) { this.failure = text; },
        });
        panel.toggle(null);
        assert.equal(panel.groups, latest);
        assert.equal(panel.activePlayer, 'new-source');
        assert.equal(panel.failure, 'Current error');
        assert.equal(panel.opened, true);
        assert.equal(refreshes, 1);
    });

    test('stale and rejected source selection is not silently discarded', () => {
        const start = viewSource.indexOf("select.connect('clicked', ");
        const body = viewSource.slice(start, viewSource.indexOf('\n                row.widget =', start));
        let callback, current = false, accepted = false, refreshes = 0, selections = 0;
        const failures = [];
        const context = {
            modelCurrent: () => current, sources: { reveal_child: true }, chevron: { set_label() {} },
            setSourcesOpen: MediaPanel.prototype.setSourcesOpen,
            backend: { failure: 'Source unavailable.', refresh() { refreshes++; },
                select() { selections++; if (!accepted) this.failure = 'Source unavailable.'; return accepted; } },
            setFailure(text) { failures.push(text); },
            reportFailure(text) { this.backend.failure = text; this.setFailure(text); },
        };
        new Function('select', 'row', body).call(context,
            { connect(_, fn) { callback = fn; } }, { group: { primary: { bus: 'browser' } } });
        callback();
        assert.equal(refreshes, 1);
        assert.equal(selections, 0);
        assert.match(failures.at(-1), /changed/);
        current = true;
        callback();
        assert.equal(failures.at(-1), 'Source unavailable.');
        assert.equal(context.sources.reveal_child, true);
        accepted = true;
        callback();
        assert.equal(context.sources.reveal_child, false);
        assert.equal(context.backend.failure, '', 'an accepted selection clears the previous selection error');
    });
});

// Evaluate the real adapter with in-memory GI doubles; never connect to a bus
// or start a real helper. Rust owns discovery, timing, and action policy.
const source = readFileSync(new URL('./backend.js', import.meta.url), 'utf8')
    .replace(/^import .+;\r?$/gm, '')
    .replace(/^export /gm, '');
const factory = new Function('GLib', 'Gio', 'Mpris', 'Wp', 'badge', 'helper', 'ipc', `'use strict';
    const { communicate, LineDecoder, writeBytes } = ipc;
    ${source}
    if (helper) runHelper = helper;
    return { NativeMedia, MediaBackend, ServiceWorker, runHelper, matchesGuard };
`);

function loadBackend({ GLib = {}, Gio = {}, Mpris = {}, Wp = {}, runHelper = null } = {}) {
    return factory(GLib, Gio, Mpris, Wp, badge, runHelper, loadIPC(GLib, Gio));
}

class Signals {
    constructor() { this.handlers = new Map(); this.nextSignal = 0; }
    connect(name, callback) {
        const id = ++this.nextSignal;
        this.handlers.set(id, { name, callback });
        return id;
    }
    disconnect(id) { this.handlers.delete(id); }
    emit(name, ...args) {
        for (const handler of [...this.handlers.values()])
            if (handler.name === name) handler.callback(this, ...args);
    }
}

class Cancellable extends Signals {
    constructor() { super(); this.cancelled = false; }
    connect(callback) { return super.connect('cancelled', callback); }
    is_cancelled() { return this.cancelled; }
    cancel() {
        if (this.cancelled) return;
        this.cancelled = true;
        this.emit('cancelled');
    }
}

class Player extends Signals {
    constructor(extra = {}) {
        super();
        Object.assign(this, {
            bus_name: 'org.mpris.MediaPlayer2.test', available: true,
            trackid: '/track/one', identity: 'Test Player', entry: 'test-player',
            title: 'Track', artist: 'Artist', album: 'Album', art_url: '', cover_art: '',
            playback_status: 1, rate: 1, length: 200,
            can_play: true, can_pause: true, can_seek: true, can_go_next: true, can_go_previous: true,
            _position: 10, _volume: 0.6, metadata: { 'xesam:url': 'file:///music/track.flac' },
            calls: [], ...extra,
        });
    }
    get position() { return this._position; }
    set position(value) {
        if (this.seekError) throw new Error(this.seekError);
        this.calls.push(['position', value]);
    }
    get volume() { return this._volume; }
    set volume(value) { this.calls.push(['volume', value]); }
    get_meta(key) {
        return key in this.metadata ? { deep_unpack: () => this.metadata[key] } : null;
    }
    play() { this.calls.push(['play']); }
    pause() { this.calls.push(['pause']); }
    next() { this.calls.push(['next']); }
    previous() { this.calls.push(['previous']); }
    publish(name, value) {
        if (name === 'position' || name === 'volume') this[`_${name}`] = value;
        else this[name.replaceAll('-', '_')] = value;
        this.emit('notify', { name });
    }
}

class Stream extends Signals {
    constructor(extra = {}) {
        super();
        Object.assign(this, {
            id: 42, serial: 123, name: 'Track', _volume: 0.7, _mute: false,
            properties: { 'application.name': 'Test Player', 'node.nick': 'Test stream' },
            calls: [], propertyReads: [], ...extra,
        });
    }
    get volume() { return this._volume; }
    set volume(value) { this.calls.push(['volume', value]); }
    get mute() { return this._mute; }
    set mute(value) { this.calls.push(['mute', value]); }
    get_pw_property(key) { this.propertyReads.push(key); return this.properties[key] ?? null; }
    publish(name, value) {
        if (name === 'volume' || name === 'mute') this[`_${name}`] = value;
        else this[name] = value;
        this.emit('notify', { name });
    }
}

function clockHarness() {
    let nextSource = 0;
    const h = { time: 100, idles: new Map(), timers: new Map() };
    h.GLib = {
        PRIORITY_DEFAULT_IDLE: 0, PRIORITY_DEFAULT: 0, SOURCE_REMOVE: false, SOURCE_CONTINUE: true,
        get_monotonic_time: () => h.time * 1e6,
        getenv: () => null,
        idle_add: (_, callback) => { const id = ++nextSource; h.idles.set(id, callback); return id; },
        timeout_add: (_, delay, callback) => { const id = ++nextSource; h.timers.set(id, { delay, callback }); return id; },
        source_remove: id => h.idles.delete(id) || h.timers.delete(id),
    };
    h.flush = () => {
        let rounds = 0;
        while (h.idles.size) {
            assert.ok(++rounds < 50, 'idle callbacks must settle');
            for (const [id, callback] of [...h.idles]) {
                h.idles.delete(id);
                callback();
            }
        }
    };
    h.fireTimer = id => {
        const timer = h.timers.get(id);
        assert.ok(timer, 'expected a scheduled timer');
        h.timers.delete(id);
        if (timer.callback() === h.GLib.SOURCE_CONTINUE) h.timers.set(id, timer);
    };
    return h;
}

function nativeHarness({ player = new Player(), stream = null } = {}) {
    const h = Object.assign(clockHarness(), { player, stream, changes: [] });
    h.manager = Object.assign(new Signals(), { players: player ? [player] : [] });
    h.audio = Object.assign(new Signals(), { streams: stream ? [stream] : [] });
    const api = loadBackend({
        GLib: h.GLib, Gio: { Cancellable },
        Mpris: { get_default: () => h.manager, PlaybackStatus: { PLAYING: 1, PAUSED: 2, STOPPED: 3 } },
        Wp: { get_default: () => ({ audio: h.audio }) },
    });
    h.native = new api.NativeMedia({ onChange: (...args) => h.changes.push(args) });
    h.rawPlayer = () => h.native.capture().players.find(value => value.bus === player.bus_name);
    return Object.assign(h, api);
}

describe('Native Astal adapter', () => {
    test('the factory exposes only the native adapter, cached backend, service worker, and thin helpers', () => {
        assert.deepEqual(Object.keys(loadBackend()).sort(),
            ['NativeMedia', 'MediaBackend', 'ServiceWorker', 'runHelper', 'matchesGuard'].sort());
        assert.doesNotMatch(source, /class\s+(?:Planner|DirectPlayer|LocalPlayerDiscovery)\b/);
        assert.doesNotMatch(source, /\b(?:readPosition|ownershipStamp|projectedPosition|sampleProgress|applicationForPid)\b|\/proc\//);
        assert.doesNotMatch(source, /Gio\.DBus|ListNames|NameOwnerChanged|GetConnectionUnixProcessID/);
    });

    test('capture serializes raw Astal fields and instance/revision fences without JS track or duration policy', () => {
        const player = new Player({ length: -1, rate: 1.25, cover_art: '/cache/native.avif',
            art_url: 'https://example.test/art', metadata: {
                'xesam:url': 'file:///music/track.flac', 'mpris:length': 361929437.5, 'mprisence:browser': 'browser',
            } });
        const h = nativeHarness({ player });
        const record = h.native.players.get(player.bus_name);
        const raw = h.rawPlayer();
        assert.equal(record.object, player);
        assert.ok(Number.isSafeInteger(record.instance));
        assert.ok(Number.isSafeInteger(record.seekRevision));
        assert.ok(Number.isSafeInteger(record.positionRevision));
        assert.deepEqual(raw, {
            bus: player.bus_name, instance: record.instance, trackId: '/track/one',
            identity: 'Test Player', desktopEntry: 'test-player', title: 'Track', artist: 'Artist', album: 'Album',
            url: 'file:///music/track.flac', artUrl: 'https://example.test/art', browserHint: 'browser',
            playing: true, stopped: false, position: 10, positionSupported: true,
            seekRevision: record.seekRevision, positionRevision: record.positionRevision,
            rate: 1.25, length: -1, lengthMicros: 361929437.5,
            canPlay: true, canPause: true, canSeek: true, canNext: true, canPrevious: true,
            cover: '/cache/native.avif', volume: 0.6, available: true,
        });
        assert.equal(Object.hasOwn(raw, 'trackKey'), false);
        assert.equal(Object.hasOwn(raw, 'modelPosition'), false);
        assert.deepEqual(JSON.parse(JSON.stringify(h.native.capture())).players[0], raw);
        h.native.dispose();
    });

    test('invalid track IDs and untyped duration metadata are passed to Rust, never synthesized or coerced', () => {
        for (const trackid of ['', 'not an object path', '/org/mpris/MediaPlayer2/TrackList/NoTrack']) {
            const h = nativeHarness({ player: new Player({ trackid, length: 0,
                metadata: { 'mpris:length': '200000000' } }) });
            const raw = h.rawPlayer();
            assert.equal(raw.trackId, trackid);
            assert.equal(raw.length, 0);
            assert.equal(raw.lengthMicros, '200000000');
            assert.equal(Object.hasOwn(raw, 'trackKey'), false);
            h.native.dispose();
        }
    });

    test('unsupported positions remain raw negative or null values and duration recovery stays in Rust', () => {
        for (const [position, expected] of [[-1, -1], [null, null], [undefined, null], [NaN, null], [Infinity, null]]) {
            const h = nativeHarness({ player: new Player({ _position: position, length: -1,
                metadata: { 'mpris:length': 200000000 } }) });
            const raw = h.rawPlayer();
            assert.equal(raw.position, expected, String(position));
            assert.equal(raw.positionSupported, false);
            assert.equal(raw.canSeek, true, 'unsupported position does not rewrite an advertised capability');
            assert.equal(raw.length, -1);
            assert.equal(raw.lengthMicros, 200000000);
            h.native.dispose();
        }
    });

    test('raw position and advertised capabilities never acquire an elapsed-time projection', () => {
        for (const position of [0, 10]) {
            const h = nativeHarness({ player: new Player({ _position: position, rate: 2 }) });
            const before = h.native.capture();
            h.time += 500;
            assert.deepEqual(h.native.capture(), before);
            assert.equal(h.rawPlayer().position, position);
            assert.equal(h.rawPlayer().canSeek, true);
            assert.equal(h.timers.size, 0, 'native polling and sample expiry belong to Rust');
            h.native.dispose();
        }
    });

    test('stream snapshots use native names and flags, with no ownership or volume descriptor', () => {
        const stream = new Stream({ _mute: true, name: 'Native media name',
            properties: { 'application.name': 'Test Player', 'media.name': 'Wrong raw name', 'node.nick': 'Native nickname' } });
        const h = nativeHarness({ stream });
        const raw = h.native.capture().streams[0];
        assert.equal(h.native.streams.get(stream.id).object, stream);
        assert.equal(raw.instance, h.native.streams.get(stream.id).instance);
        assert.equal(raw.id, 42);
        assert.equal(raw.name, 'Native media name');
        assert.equal(raw.nickname, 'Native nickname');
        assert.equal(raw.properties['media.name'], 'Native media name');
        assert.equal(raw.properties['application.name'], 'Test Player');
        assert.equal(raw.ready, true);
        assert.equal(raw.volume, 0.7);
        assert.equal(raw.muted, true);
        assert.equal(typeof h.native.volume, 'undefined');
        h.native.dispose();
    });

    test('repeated position notifications advance only positionRevision, including an unchanged sample', () => {
        const h = nativeHarness();
        const before = h.rawPlayer();
        for (let index = 1; index <= 2; index++) {
            h.player.publish('position', 10);
            assert.equal(h.rawPlayer().positionRevision, before.positionRevision + index);
            assert.equal(h.rawPlayer().seekRevision, before.seekRevision);
        }
        for (const [property, value] of [['volume', 0.8], ['cover-art', '/cache/new.png'], ['title', 'Renamed']])
            h.player.publish(property, value);
        assert.equal(h.rawPlayer().positionRevision, before.positionRevision + 2);
        assert.equal(h.rawPlayer().seekRevision, before.seekRevision);
        h.native.dispose();
    });

    test('coalesced notifications retain metadata, artwork, volume, and stream changes in the next capture', () => {
        const h = nativeHarness({ stream: new Stream() });
        h.flush();
        h.changes.length = 0;
        h.player.publish('volume', 0.8);
        h.player.publish('cover-art', '/cache/updated.avif');
        h.player.publish('title', 'Changed track');
        h.stream.publish('mute', true);
        assert.equal(h.idles.size, 1);
        h.flush();
        assert.equal(h.changes.length, 1);
        const raw = h.native.capture();
        assert.equal(raw.players[0].title, 'Changed track');
        assert.equal(raw.players[0].cover, '/cache/updated.avif');
        assert.equal(raw.players[0].volume, 0.8);
        assert.equal(raw.streams[0].muted, true);
        h.native.dispose();
    });

    test('object-list notifications replace instance fences and disconnect removed native objects', () => {
        const h = nativeHarness({ stream: new Stream() });
        h.flush();
        const before = h.native.capture();
        const player = new Player();
        const stream = new Stream();
        h.manager.players = [player];
        h.audio.streams = [stream];
        h.manager.emit('notify::players');
        h.audio.emit('notify::streams');
        const after = h.native.capture();
        assert.notEqual(after.players[0].instance, before.players[0].instance);
        assert.notEqual(after.streams[0].instance, before.streams[0].instance);
        assert.equal(h.player.handlers.size, 0);
        assert.equal(h.stream.handlers.size, 0);
        h.manager.players = [];
        h.audio.streams = [];
        h.native.sync();
        assert.deepEqual(h.native.capture(), { players: [], streams: [] });
        assert.equal(player.handlers.size, 0);
        assert.equal(stream.handlers.size, 0);
        h.native.dispose();
        assert.equal(h.manager.handlers.size, 0);
        assert.equal(h.audio.handlers.size, 0);
        assert.equal(h.idles.size, 0);
    });

    test('native playback operations are thin Astal dispatches', () => {
        const h = nativeHarness();
        for (const operation of ['play', 'pause', 'next', 'previous'])
            h.native.apply({ player: h.player.bus_name, operation });
        assert.deepEqual(h.player.calls, [['play'], ['pause'], ['next'], ['previous']]);
        h.native.dispose();
    });

    test('seek advances exactly one dispatch revision without an optimistic position or sample revision', () => {
        const h = nativeHarness();
        const before = h.rawPlayer();
        h.native.apply({ player: h.player.bus_name, operation: 'seek', value: 80 });
        const after = h.rawPlayer();
        assert.deepEqual(h.player.calls, [['position', 80]]);
        assert.equal(after.seekRevision, before.seekRevision + 1);
        assert.equal(after.positionRevision, before.positionRevision);
        assert.equal(after.position, before.position);
        h.player.publish('position', 79.5);
        assert.equal(h.rawPlayer().position, 79.5);
        assert.equal(h.rawPlayer().positionRevision, before.positionRevision + 1);
        assert.equal(h.rawPlayer().seekRevision, before.seekRevision + 1);
        h.native.dispose();
    });

    test('a synchronous native seek failure does not advance either revision or invent a position', () => {
        const h = nativeHarness();
        const before = h.rawPlayer();
        h.player.seekError = 'Native seek failed';
        assert.throws(() => h.native.apply({ player: h.player.bus_name, operation: 'seek', value: 80 }), /Native seek failed/);
        assert.deepEqual(h.rawPlayer(), before);
        assert.deepEqual(h.player.calls, []);
        h.native.dispose();
    });

    test('volume and mute dispatch only the explicit native setters supplied by Rust', () => {
        const h = nativeHarness({ stream: new Stream({ _mute: true }) });
        h.native.apply({ player: h.player.bus_name, operation: 'volume', value: 0.25 });
        assert.deepEqual(h.player.calls, [['volume', 0.25]]);
        assert.deepEqual(h.stream.calls, []);
        h.native.apply({ player: h.player.bus_name, streamId: 42, operation: 'volume', value: 0.4 });
        assert.deepEqual(h.stream.calls, [['volume', 0.4]], 'JS must not infer unmuting');
        h.stream.calls.length = 0;
        h.native.apply({ player: h.player.bus_name, streamId: 42, operation: 'volume', value: 0.5, muted: false });
        assert.deepEqual(h.stream.calls, [['volume', 0.5], ['mute', false]]);
        h.stream.calls.length = 0;
        h.native.apply({ player: h.player.bus_name, streamId: 42, operation: 'mute', muted: true });
        assert.deepEqual(h.stream.calls, [['mute', true]], 'mute is never implemented by zeroing volume');
        h.native.dispose();
    });

    test('disposal cancels queued notifications and disconnects all GI subscriptions', () => {
        const h = nativeHarness({ stream: new Stream() });
        h.native.dispose();
        h.player.publish('title', 'After disposal');
        h.stream.publish('mute', true);
        h.flush();
        assert.equal(h.changes.length, 0);
        for (const object of [h.player, h.stream, h.manager, h.audio]) assert.equal(object.handlers.size, 0);
        assert.equal(h.idles.size, 0);
        assert.equal(h.timers.size, 0);
    });
});

describe('Opaque Rust snapshot guards', () => {
    const matchesGuard = (guard, snapshot) => loadBackend().matchesGuard(guard, snapshot);
    const snapshot = () => ({ players: [{ bus: 'native', instance: 7, position: 10, volume: 0.6,
        futureFence: { nested: { b: 2, a: 1 }, order: ['one', 'two'] } }],
        streams: [{ id: 42, instance: 8, properties: { 'media.name': 'Track', 'application.name': 'Test' }, volume: 0.7 }] });

    test('partial record fields are selected by Rust, not a JS domain whitelist', () => {
        const raw = snapshot();
        const guard = { players: [{ bus: 'native', futureFence: { order: ['one', 'two'], nested: { a: 1, b: 2 } } }],
            streams: [{ id: 42 }] };
        assert.equal(matchesGuard(guard, raw), true);
        raw.players[0].position = 99;
        raw.players[0].volume = 0.1;
        assert.equal(matchesGuard(guard, raw), true, 'fields omitted by Rust are not independently guarded');
        raw.players[0].futureFence.nested.a = 9;
        assert.equal(matchesGuard(guard, raw), false, 'unknown future fields are still meaningful fences');
    });

    test('any explicitly supplied field, including volume and position revisions, participates in equality', () => {
        for (const [field, before, after] of [['volume', 0.6, 0.9], ['positionRevision', 2, 3], ['opaque', 'one', 'two']]) {
            const raw = snapshot();
            raw.players[0][field] = before;
            const guard = { players: [{ [field]: before }], streams: [{}] };
            assert.equal(matchesGuard(guard, raw), true, field);
            raw.players[0][field] = after;
            assert.equal(matchesGuard(guard, raw), false, field);
        }
    });

    test('nested property maps ignore insertion order but not membership or nested array order', () => {
        const raw = snapshot();
        const guard = { players: [{ futureFence: { order: ['one', 'two'], nested: { a: 1, b: 2 } } }],
            streams: [{ properties: { 'application.name': 'Test', 'media.name': 'Track' } }] };
        assert.equal(matchesGuard(guard, raw), true);
        raw.streams[0].properties.extra = 'new';
        assert.equal(matchesGuard(guard, raw), false, 'property maps are complete canonical values');
        delete raw.streams[0].properties.extra;
        raw.players[0].futureFence.order.reverse();
        assert.equal(matchesGuard(guard, raw), false);
    });

    test('player and stream array membership and order are exact even for partial records', () => {
        const raw = snapshot();
        raw.players.push({ bus: 'other', instance: 9 });
        raw.streams.push({ id: 43, instance: 10 });
        const guard = { players: [{ bus: 'native' }, { bus: 'other' }], streams: [{ id: 42 }, { id: 43 }] };
        assert.equal(matchesGuard(guard, raw), true);
        for (const key of ['players', 'streams']) {
            const changed = structuredClone(raw);
            changed[key].reverse();
            assert.equal(matchesGuard(guard, changed), false, `${key} order`);
            changed[key].pop();
            assert.equal(matchesGuard(guard, changed), false, `${key} removal`);
            changed[key].push({}, {});
            assert.equal(matchesGuard(guard, changed), false, `${key} addition`);
        }
    });

    test('missing fields, replacements, and malformed guard containers never match', () => {
        const raw = snapshot();
        assert.equal(matchesGuard({ players: [{ missing: 'fence' }], streams: [{}] }, raw), false);
        assert.equal(matchesGuard({ players: [{ instance: 999 }], streams: [{}] }, raw), false);
        for (const guard of [null, [], {}, { players: null, streams: [] }, { players: [], streams: null }])
            assert.equal(matchesGuard(guard, raw), false);
        assert.equal(matchesGuard({ players: [], streams: [] }, { players: [], streams: [] }), true);
    });
});

function backendFixture() {
    const h = Object.assign(clockHarness(), { sent: [], applied: [], changes: 0, progressChanges: 0, captures: 0 });
    const { MediaBackend } = loadBackend({ GLib: h.GLib, Gio: { Cancellable } });
    h.snapshot = {
        players: [{ bus: 'native', instance: 7, trackId: '/track/one', seekRevision: 0, positionRevision: 2,
            identity: 'Native', desktopEntry: 'native', title: 'Raw title', artist: 'Raw artist', album: '',
            url: 'file:///track', artUrl: '', browserHint: null, playing: true, stopped: false,
            position: 10, positionSupported: true, rate: 1, length: 90, lengthMicros: null,
            canPlay: false, canPause: false, canSeek: false, canNext: false, canPrevious: false,
            cover: '/cache/raw.avif', volume: 0.6, available: true, futureFence: { b: 2, a: 1 } }],
        streams: [{ id: 42, instance: 8, name: 'Track', nickname: 'Stream', ready: true,
            properties: { 'application.name': 'Native', 'media.name': 'Track' }, volume: 0.7, muted: false }],
    };
    h.native = {
        capture() { h.captures++; return structuredClone(h.snapshot); },
        apply(operation) { h.applied.push(structuredClone(operation)); return h.onApply?.(operation); },
        sync() {}, dispose() { h.nativeDisposed = true; h.snapshot = { players: [], streams: [] }; },
    };
    h.worker = {
        ready: true,
        send(message) {
            if (!this.ready) return false;
            h.sent.push(structuredClone(message));
            return h.onSend ? h.onSend(message) : true;
        },
        start() { h.workerStarted = true; },
        stop() { this.ready = false; },
        failed(reason) { this.stop(); h.backend._failed(reason); },
        dispose() { this.stop(); h.workerDisposed = true; },
    };
    h.backend = Object.assign(Object.create(MediaBackend.prototype), {
        helper: '/bin/media-control', disposed: false, ready: false,
        groups: [], players: new Map(), positions: new Map(), transports: {}, guard: null,
        activePlayer: '', failure: '', native: h.native, worker: h.worker,
        onChange() { h.changes++; }, onProgress() { h.progressChanges++; },
    });
    h.handshake = () => { h.worker.ready = true; h.backend._receive({ type: 'ready' }); };
    const player = { ...structuredClone(h.snapshot.players[0]), title: 'Rust title', artist: 'Rust artist',
        trackKey: 'rust-track-key', cover: '/cache/rust.avif' };
    h.state = {
        type: 'state', ready: true,
        groups: [{ primary: 'native', players: ['native'], title: 'Rust group', displayName: 'Rust source',
            streamId: 42, streamReason: 'Rust owns this stream' }],
        activePlayer: 'native', players: [player],
        progress: [{ ...player, position: 14.25, samplePosition: 10, positionSupported: true }],
        transports: { native: { enabled: true, reason: 'Rust transport decision' } },
        volumes: { native: { enabled: true, value: 0.4, key: 'rust-volume-key', reason: 'Rust volume decision',
            muteEnabled: true, muted: true, muteReason: 'Rust mute decision' } },
        failure: '', guard: { players: [{ bus: 'native', instance: 7, seekRevision: 0, futureFence: { a: 1, b: 2 } }],
            streams: [{ id: 42, instance: 8 }] },
    };
    h.publish = (state = h.state) => h.backend._receive(structuredClone(state));
    h.publish();
    h.sent.length = 0;
    h.changes = 0;
    h.progressChanges = 0;
    return h;
}

describe('Cached Rust media backend', () => {
    test('resolves Rust groups and metadata without native regrouping, projection, or metadata overlay', () => {
        const h = backendFixture();
        const captures = h.captures;
        const groups = h.backend.resolveGroups();
        assert.equal(groups.length, 1);
        assert.deepEqual(groups[0], { ...h.state.groups[0], primary: h.state.players[0] });
        assert.deepEqual(h.backend.player('native'), h.state.players[0]);
        assert.deepEqual(h.backend.progress('native'), h.state.progress[0]);
        assert.equal(h.backend.player('missing'), null);
        assert.equal(h.backend.progress('missing'), null);
        assert.equal(h.backend.activePlayer, 'native');
        assert.equal(h.captures, captures, 'cache access must not synchronously query native state');
        assert.deepEqual(h.sent, []);
    });

    test('private-owner player views are cached Rust records, not locally discovered JS objects', () => {
        const h = backendFixture();
        const recovered = { ...h.state.players[0], bus: ':1.220', instance: 901, identity: 'Haruna', title: 'Recovered' };
        const state = { ...h.state, players: [recovered], progress: [{ ...recovered, samplePosition: 10 }],
            activePlayer: recovered.bus, groups: [{ ...h.state.groups[0], primary: recovered.bus, players: [recovered.bus] }],
            transports: { [recovered.bus]: { enabled: true, reason: 'Recovered by Rust' } } };
        h.publish(state);
        assert.deepEqual(h.backend.player(recovered.bus), recovered);
        assert.equal(h.backend.resolveGroups()[0].primary.bus, recovered.bus);
        assert.equal(h.native.players, undefined);
        assert.equal(h.snapshot.players.some(player => player.bus === recovered.bus), false);
        assert.deepEqual(h.backend.transport(h.backend.resolveGroups()[0]), state.transports[recovered.bus]);
    });

    test('state replaces the cache and notifies the view; progress updates only its separate display cache', () => {
        const h = backendFixture();
        const progress = [{ ...h.state.progress[0], position: 22.75, samplePosition: 20 }];
        h.backend._receive({ type: 'progress', progress });
        assert.deepEqual(h.backend.progress('native'), progress[0]);
        assert.equal(h.backend.player('native').position, 10, 'projected progress is not a raw player update');
        assert.equal(h.changes, 0, 'display frames must not trigger a complete GTK model render');
        assert.equal(h.progressChanges, 1);
        const empty = { ...h.state, groups: [], players: [], progress: [], transports: {}, volumes: {}, activePlayer: '' };
        h.publish(empty);
        assert.equal(h.changes, 1);
        assert.deepEqual(h.backend.resolveGroups(), []);
        assert.equal(h.backend.player('native'), null);
        assert.equal(h.backend.progress('native'), null);
        assert.equal(h.backend.activePlayer, '');
    });

    test('current checks only the opaque state guard against the latest native snapshot', () => {
        const h = backendFixture();
        assert.equal(h.backend.current(), true);
        h.snapshot.players[0].position = 99;
        h.snapshot.players[0].volume = 0.1;
        assert.equal(h.backend.current(), true, 'Rust decides which fields matter to freshness');
        h.snapshot.players[0].futureFence.a = 9;
        assert.equal(h.backend.current(), false, 'a future opaque fence cannot be ignored by JS');
        h.snapshot.players[0].futureFence.a = 1;
        h.snapshot.players[0].instance++;
        assert.equal(h.backend.current(), false);
    });

    test('transport is the Rust cache decision, not a reconstruction from native capabilities', () => {
        const h = backendFixture();
        const group = h.backend.resolveGroups()[0];
        assert.deepEqual(h.backend.transport(group), h.state.transports.native);
        h.publish({ ...h.state, transports: { native: { enabled: false, reason: 'Rust disabled this source' } } });
        assert.deepEqual(h.backend.transport(group), { enabled: false, reason: 'Rust disabled this source' });
        assert.equal(h.backend.transport({ primary: { bus: 'missing' } }).enabled, false);
    });

    test('refresh sends raw snapshots and ticks request Rust progress without deriving elapsed time', () => {
        const h = backendFixture();
        assert.equal(h.backend.refresh(), true);
        const captures = h.captures;
        h.time += 12;
        assert.equal(h.backend.tick(), true);
        assert.equal(h.captures, captures, 'a display tick does not recapture native properties');
        assert.deepEqual(h.sent, [{ type: 'snapshot', snapshot: h.snapshot }, { type: 'tick' }]);
        assert.equal(h.backend.progress('native').position, 14.25, 'display time changes only on a Rust reply');
        assert.equal(Object.hasOwn(h.sent[0].snapshot.players[0], 'trackKey'), false);
        assert.equal(Object.hasOwn(h.sent[0].snapshot.players[0], 'modelPosition'), false);
    });

    test('actions, signed jumps, selection, and cycling are requests, never local plans or native dispatches', () => {
        const h = backendFixture();
        h.time = 321;
        assert.equal(h.backend.action('jump', 'native', -15), true);
        assert.equal(h.backend.action('seek', 'native', 85), true);
        assert.equal(h.backend.action('toggle'), true);
        assert.equal(h.backend.select('native'), true);
        assert.equal(h.backend.cycle(), true);
        assert.deepEqual(h.sent, [
            { type: 'action', action: { kind: 'jump', player: 'native', position: -15 }, createdAt: 321, snapshot: h.snapshot },
            { type: 'action', action: { kind: 'seek', player: 'native', position: 85 }, createdAt: 321, snapshot: h.snapshot },
            { type: 'action', action: { kind: 'toggle', player: '', position: 0 }, createdAt: 321, snapshot: h.snapshot },
            { type: 'select', player: 'native', snapshot: h.snapshot }, { type: 'cycle', snapshot: h.snapshot },
        ]);
        assert.deepEqual(h.applied, []);
        assert.equal(h.backend.activePlayer, 'native', 'selection is updated only by a Rust state reply');
    });

    test('Rust volume descriptors are accepted without retaining an unused UI facade or cache', () => {
        const h = backendFixture();
        assert.ok(h.state.volumes.native);
        assert.equal('volume' in h.backend, false);
        assert.equal('volumes' in h.backend, false);
        h.publish({ ...h.state, volumes: {} });
        assert.equal('volumes' in h.backend, false);
        assert.deepEqual(h.sent, []);
        assert.deepEqual(h.applied, []);
    });

    test('view failures are reported to Rust and retained through an error-bearing recovery state', () => {
        const h = backendFixture();
        const failure = 'Media changed; try the action again.';
        h.backend.reportFailure(failure);
        assert.equal(h.backend.failure, failure);
        assert.ok(h.sent.some(message => message.type === 'report' && message.error === failure));
        h.publish({ ...h.state, failure });
        assert.equal(h.backend.failure, failure);
        h.publish({ ...h.state, failure: '' });
        assert.equal(h.backend.failure, '');
    });

    test('status is the serialized presentation badge for the Rust-selected cached primary', () => {
        const h = backendFixture();
        const expected = badge({ primary: h.state.players[0] });
        assert.deepEqual(JSON.parse(h.backend.status()), expected);
        assert.equal(h.applied.length, 0);
    });

    test('transport acknowledgements leave cached media and UI callbacks unchanged', () => {
        const h = backendFixture();
        const before = h.backend.player('native');
        h.backend._receive({ type: 'ack' });
        assert.equal(h.backend.player('native'), before);
        assert.equal(h.backend.current(), true);
        assert.equal(h.changes, 0);
        assert.equal(h.progressChanges, 0);
        assert.deepEqual(h.sent, []);
    });

    test('ready and capture requests send fresh snapshots without inventing action requests', () => {
        const h = backendFixture();
        h.backend.ready = false;
        h.worker.ready = false;
        h.handshake();
        assert.equal(h.worker.ready, true);
        assert.equal(h.backend.ready, false, 'a service handshake is not yet a ready Rust model');
        assert.equal(h.backend.current(), false);
        assert.equal(h.sent.at(-1).type, 'snapshot');
        assert.deepEqual(h.sent.at(-1).snapshot, h.snapshot);
        h.snapshot.players[0].position = 12;
        h.backend._receive({ type: 'capture', id: 17 });
        assert.equal(h.sent.at(-1).type, 'snapshot');
        assert.deepEqual(h.sent.at(-1).snapshot, h.snapshot);
        assert.equal(h.sent.some(message => message.type === 'action'), false);
    });

    test('failure clears every cache and a new ready handshake never replays old actions', () => {
        const h = backendFixture();
        h.backend.action('next', 'native');
        h.worker.failed('Media service exited.');
        assert.equal(h.backend.ready, false);
        assert.equal(h.backend.current(), false);
        assert.deepEqual(h.backend.resolveGroups(), []);
        assert.equal(h.backend.player('native'), null);
        assert.equal(h.backend.progress('native'), null);
        assert.equal(h.backend.activePlayer, '');
        assert.equal(h.backend.transport({ primary: { bus: 'native' } }).enabled, false);

        assert.match(h.backend.failure, /exited/);
        h.sent.length = 0;
        assert.equal(h.backend.action('next', 'native'), false);
        assert.equal(h.backend.select('native'), false);
        assert.equal(h.backend.cycle(), false);
        assert.equal(h.backend.refresh(), false);
        assert.equal(h.backend.tick(), false);
        h.handshake();
        assert.equal(h.backend.ready, false);
        h.publish();
        assert.equal(h.sent.some(message => ['action', 'select', 'cycle', 'volume', 'mute'].includes(message.type)), false);
        assert.equal(h.backend.action('next', 'native'), true);
        assert.equal(h.sent.filter(message => message.type === 'action').length, 1);
    });

    test('a pipe failure after an accepted send clears the cache and the next handshake sends only a fresh snapshot', () => {
        const h = backendFixture();
        assert.equal(h.backend.action('next', 'native'), true);
        h.worker.failed('Service input pipe failed');
        assert.equal(h.backend.ready, false);
        assert.equal(h.backend.current(), false);
        assert.deepEqual(h.backend.resolveGroups(), []);
        assert.match(h.backend.failure, /Service input pipe failed/);
        h.sent.length = 0;
        h.handshake();
        assert.deepEqual(h.sent, [{ type: 'snapshot', snapshot: h.snapshot }]);
    });

    test('disposal closes both adapters and late state replies cannot repopulate the cache', () => {
        const h = backendFixture();
        h.backend.dispose();
        assert.equal(h.workerDisposed, true);
        assert.equal(h.nativeDisposed, true);
        assert.equal(h.backend.current(), false);
        const changes = h.changes;
        h.publish({ ...h.state, failure: 'Late reply' });
        assert.equal(h.changes, changes);
        assert.equal(h.backend.action('next', 'native'), false);
    });
});

describe('Guarded Rust native apply acknowledgements', () => {
    const request = (h, extra = {}) => ({ type: 'apply', id: 7,
        operation: { player: 'native', operation: 'next' }, guard: structuredClone(h.state.guard), expiresAt: h.time + 5, ...extra });

    test('a current, unexpired request applies exactly once and acknowledges a fresh raw snapshot', () => {
        const h = backendFixture();
        const message = request(h);
        h.onApply = () => { h.snapshot.players[0].title = 'Native changed during dispatch'; };
        h.backend._receive(message);
        assert.deepEqual(h.applied, [message.operation]);
        const result = h.sent.at(-1);
        assert.equal(result.type, 'result');
        assert.equal(result.id, 7);
        assert.equal(result.ok, true);
        assert.deepEqual(result.snapshot, h.snapshot);
        assert.equal(h.sent.filter(value => value.type === 'result').length, 1);
    });

    test('Rust-supplied sampling fences reject a changed raw sample without JS regrouping', () => {
        const h = backendFixture();
        const guard = structuredClone(h.state.guard);
        guard.players[0].position = h.snapshot.players[0].position;
        guard.players[0].positionRevision = h.snapshot.players[0].positionRevision;
        h.snapshot.players[0].positionRevision++;
        h.backend._receive(request(h, { guard }));
        assert.deepEqual(h.applied, []);
        assert.equal(h.sent.at(-1).ok, false);
        assert.match(h.sent.at(-1).error, /changed/);
    });

    test('an opaque native fence mismatch rejects apply and returns the current snapshot to Rust', () => {
        const h = backendFixture();
        h.snapshot.players[0].futureFence.a = 9;
        h.backend._receive(request(h));
        assert.deepEqual(h.applied, []);
        const result = h.sent.at(-1);
        assert.equal(result.type, 'result');
        assert.equal(result.ok, false);
        assert.match(result.error, /changed|stale|current/i);
        assert.deepEqual(result.snapshot, h.snapshot);
    });

    test('per-operation guards are used verbatim, including fields omitted from the cached state guard', () => {
        const h = backendFixture();
        const guard = structuredClone(h.state.guard);
        guard.players[0].volume = 0.6;
        h.snapshot.players[0].volume = 0.9;
        assert.equal(h.backend.current(), true, 'the state guard deliberately does not fence volume');
        h.backend._receive(request(h, { guard, operation: { player: 'native', operation: 'volume', value: 0.2 } }));
        assert.deepEqual(h.applied, []);
        assert.equal(h.sent.at(-1).ok, false);
    });

    test('expired or invalid deadlines reject native calls instead of extending or replaying the action', () => {
        for (const expiresAt of [99, NaN, Infinity, undefined]) {
            const h = backendFixture();
            h.backend._receive(request(h, { expiresAt }));
            assert.deepEqual(h.applied, [], String(expiresAt));
            assert.equal(h.sent.at(-1).type, 'result');
            assert.equal(h.sent.at(-1).ok, false);
            assert.ok(h.sent.at(-1).error.length > 0);
        }
    });

    test('native dispatch errors return a useful failure and are never retried locally', () => {
        const h = backendFixture();
        h.onApply = () => { throw new Error('Native call failed'); };
        h.backend._receive(request(h));
        assert.equal(h.applied.length, 1);
        assert.equal(h.sent.at(-1).ok, false);
        assert.match(h.sent.at(-1).error, /Native call failed/);
        assert.deepEqual(h.sent.at(-1).snapshot, h.snapshot);
        assert.equal(h.timers.size, 0);
    });

    test('real Astal seek acknowledgements contain one dispatch revision and no optimistic position', () => {
        const native = nativeHarness();
        const h = backendFixture();
        h.backend.native = native.native;
        h.snapshot = native.native.capture();
        const before = h.snapshot.players[0];
        h.backend._receive(request(h, {
            operation: { player: native.player.bus_name, operation: 'seek', value: 80 },
            guard: { players: [{ bus: native.player.bus_name, instance: before.instance, seekRevision: before.seekRevision }], streams: [] },
        }));
        const result = h.sent.at(-1);
        assert.equal(result.ok, true);
        assert.equal(result.snapshot.players[0].seekRevision, before.seekRevision + 1);
        assert.equal(result.snapshot.players[0].positionRevision, before.positionRevision);
        assert.equal(result.snapshot.players[0].position, before.position);
        assert.deepEqual(native.player.calls, [['position', 80]]);
        native.native.dispose();
    });
});

class Bytes {
    constructor(data) { this.data = Uint8Array.from(data); }
    get_size() { return this.data.length; }
    toArray() { return Uint8Array.from(this.data); }
}

function pipeHarness() {
    const h = Object.assign(clockHarness(), { processes: [], messages: [], failures: [] });
    class ReadPipe {
        constructor() { this.pending = null; this.closed = false; this.reads = []; }
        read_bytes_async(size, _priority, cancel, callback) {
            assert.equal(this.pending, null, 'only one async pipe read may be outstanding');
            this.pending = { size, cancel, callback };
            this.reads.push(this.pending);
        }
        read_bytes_finish(result) {
            if (result.error) throw result.error;
            return new Bytes(result.bytes);
        }
        feed(bytes, error = null) {
            const read = this.pending;
            assert.ok(read, 'expected an outstanding stdout read');
            this.pending = null;
            read.callback(this, { bytes: typeof bytes === 'string' ? new TextEncoder().encode(bytes) : bytes, error });
        }
        is_closed() { return this.closed; }
        has_pending() { return this.pending !== null; }
        close_async(_priority, _cancel, callback) { this.closed = true; callback(this, {}); }
        close_finish() { return true; }
    }
    class WritePipe {
        constructor() { this.pending = null; this.closed = false; this.writes = []; this.chunks = []; }
        enqueue(bytes, cancel, callback) {
            assert.equal(this.pending, null, 'queued messages must not overlap async pipe writes');
            this.pending = { bytes: bytes instanceof Bytes ? bytes.toArray() : Uint8Array.from(bytes), cancel, callback };
            this.writes.push(this.pending);
        }
        write_bytes_async(bytes, _priority, cancel, callback) {
            assert.ok(bytes instanceof Bytes, 'GLib.Bytes must retain the input buffer for each async write');
            this.enqueue(bytes, cancel, callback);
        }
        write_bytes_finish(result) {
            if (result.error) throw result.error;
            return result.written;
        }

        finish(written = this.pending?.bytes.length, error = null) {
            const write = this.pending;
            assert.ok(write, 'expected an outstanding stdin write');
            this.pending = null;
            if (!error && written > 0) this.chunks.push(write.bytes.slice(0, written));
            write.callback(this, { written, error });
        }
        text() {
            const data = new Uint8Array(this.chunks.reduce((total, chunk) => total + chunk.length, 0));
            let offset = 0;
            for (const chunk of this.chunks) { data.set(chunk, offset); offset += chunk.length; }
            return new TextDecoder('utf-8', { fatal: true }).decode(data);
        }
        is_closed() { return this.closed; }
        has_pending() { return this.pending !== null; }
        close_async(_priority, _cancel, callback) { this.closed = true; callback(this, {}); }
        close_finish() { return true; }
    }
    h.GLib.Bytes = Bytes;
    h.Gio = {
        Cancellable,
        SubprocessFlags: { STDIN_PIPE: 1, STDOUT_PIPE: 2 },
        Subprocess: { new(argv, flags) {
            const child = {
                argv, flags, input: new WritePipe(), output: new ReadPipe(), killed: false, waiter: null,
                get_stdin_pipe() { return this.input; }, get_stdout_pipe() { return this.output; },
                force_exit() { this.killed = true; },
                wait_async(cancel, callback) { assert.equal(cancel, null, 'child reaping must not be cancelled'); this.waiter = callback; },
                wait_finish(result) { if (result.error) throw result.error; return true; },
                get_successful() { return this.successful !== false; },
                exit(successful = true, error = null) {
                    this.successful = successful;
                    const callback = this.waiter;
                    assert.ok(callback, 'expected a child wait callback');
                    this.waiter = null;
                    callback(this, { error });
                },
            };
            h.processes.push(child);
            return child;
        } },
    };
    Object.assign(h, loadBackend({ GLib: h.GLib, Gio: h.Gio }));
    h.startWorker = () => {
        h.worker = new h.ServiceWorker('/bin/media-control', {
            onMessage: message => h.messages.push(message), onFailure: reason => h.failures.push(reason),
        });
        h.worker.start();
        return h.processes.at(-1);
    };
    h.drainInput = child => {
        let count = 0;
        while (child.input.pending) {
            assert.ok(++count < 1000, 'bounded asynchronous writes must settle');
            child.input.finish();
        }
        return child.input.text().trim().split('\n').filter(Boolean).map(line => JSON.parse(line));
    };
    h.reply = (child, ...messages) => child.output.feed(messages.map(value => JSON.stringify(value) + '\n').join(''));
    return h;
}

describe('Rust service JSONL worker', () => {
    test('starts the service command and accepts unsolicited ready, state, progress, apply, and capture messages', () => {
        const h = pipeHarness();
        const child = h.startWorker();
        assert.deepEqual(child.argv, ['/bin/media-control', 'service']);
        assert.equal(child.flags, h.Gio.SubprocessFlags.STDIN_PIPE | h.Gio.SubprocessFlags.STDOUT_PIPE);
        assert.equal(h.worker.ready, false);
        assert.equal(h.worker.send({ type: 'tick' }), false, 'requests are rejected before the service handshake');
        assert.equal(child.input.writes.length, 0);
        const messages = [{ type: 'ready' }, { type: 'state', groups: [], players: [], progress: [] },
            { type: 'progress', progress: [] }, { type: 'apply', id: 7 }, { type: 'capture', id: 8 }];
        h.reply(child, ...messages);
        assert.deepEqual(h.messages, messages);
        assert.equal(h.worker.ready, true);
        assert.deepEqual(h.failures, []);
        h.worker.dispose();
    });

    test('a no-op acknowledgement clears the request watchdog without scheduling a restart', () => {
        const h = pipeHarness();
        const child = h.startWorker();
        h.reply(child, { type: 'ready' });
        assert.equal(h.worker.send({ type: 'snapshot', snapshot: { players: [], streams: [] } }), true);
        h.drainInput(child);
        assert.notEqual(h.worker.watchdog, 0);
        h.reply(child, { type: 'ack' });
        assert.equal(h.worker.watchdog, 0);
        assert.equal(h.timers.size, 0);
        assert.equal(child.killed, false);
        assert.deepEqual(h.failures, []);
        h.worker.dispose();
    });

    test('retains partial JSON lines and UTF-8 scalars across asynchronous reads', () => {
        const h = pipeHarness();
        const child = h.startWorker();
        const message = { type: 'state', title: '作者😀' };
        const bytes = new TextEncoder().encode(JSON.stringify(message) + '\n');
        const split = bytes.indexOf(0xe4) + 1;
        child.output.feed(bytes.slice(0, split));
        assert.deepEqual(h.messages, []);
        child.output.feed(bytes.slice(split, -1));
        assert.deepEqual(h.messages, []);
        child.output.feed(bytes.slice(-1));
        assert.deepEqual(h.messages, [message]);
        assert.deepEqual(h.failures, []);
        h.worker.dispose();
    });

    test('queues multiple outbound messages and completes partial pipe writes in exact JSONL order', () => {
        const h = pipeHarness();
        const child = h.startWorker();
        h.reply(child, { type: 'ready' });
        const messages = [
            { type: 'snapshot', snapshot: { players: [], streams: [], metadata: '作者'.repeat(2500) } },
            { type: 'action', action: { kind: 'jump', player: 'native', position: -15 } },
            { type: 'tick' },
        ];
        messages.forEach(value => assert.equal(h.worker.send(value), true));
        assert.equal(child.input.writes.length, 1, 'only the first queued message starts writing immediately');
        child.input.finish(7);
        assert.deepEqual(h.drainInput(child), messages);
        assert.ok(child.input.writes.every(write => write.bytes.length <= 4096), 'each async write stays within PIPE_BUF');
        assert.deepEqual(h.failures, []);
        h.worker.dispose();
    });

    test('rejects an oversized outbound JSON message before writing it', () => {
        const h = pipeHarness();
        const child = h.startWorker();
        h.reply(child, { type: 'ready' });
        assert.throws(() => h.worker.send({ type: 'report', error: '界'.repeat(1024 * 1024 / 2) }), /large|limit|MiB/i);
        assert.equal(child.input.writes.length, 0);
        h.worker.dispose();
    });

    test('malformed JSON and invalid UTF-8 fail closed, and a fresh worker decodes independently', () => {
        for (const bytes of [new TextEncoder().encode('{broken}\n'),
            Uint8Array.from([123, 34, 116, 121, 112, 101, 34, 58, 34, 0xc3, 0x28, 34, 125, 10])]) {
            const h = pipeHarness();
            const child = h.startWorker();
            child.output.feed(bytes);
            assert.equal(h.messages.length, 0);
            assert.equal(h.failures.length, 1);
            assert.equal(child.killed, true);
            assert.equal(h.timers.size, 1, 'one bounded restart is scheduled');
            h.fireTimer([...h.timers.keys()][0]);
            const restarted = h.processes.at(-1);
            assert.notEqual(restarted, child);
            h.reply(restarted, { type: 'ready' }, { type: 'state', title: '作者😀' });
            assert.equal(h.messages.at(-1).title, '作者😀');
            assert.equal(h.failures.length, 1);
            h.worker.dispose();
        }
    });

    test('bounds unterminated inbound frames to one MiB', () => {
        const h = pipeHarness();
        const child = h.startWorker();
        const chunk = new Uint8Array(8192).fill(32);
        for (let index = 0; index < 129 && !h.failures.length; index++) child.output.feed(chunk);
        assert.equal(h.messages.length, 0);
        assert.equal(h.failures.length, 1);
        assert.match(h.failures[0], /large|limit|MiB/i);
        assert.equal(child.killed, true);
        h.worker.dispose();
    });

    test('EOF cancels queued writes; restarted workers never replay old action requests or callbacks', () => {
        const h = pipeHarness();
        const child = h.startWorker();
        h.reply(child, { type: 'ready' });
        h.worker.send({ type: 'action', action: { kind: 'next', player: 'native', position: 0 } });
        h.worker.send({ type: 'action', action: { kind: 'seek', player: 'native', position: 80 } });
        const write = child.input.pending;
        child.output.feed(new Uint8Array());
        assert.equal(h.failures.length, 1);
        assert.equal(write.cancel.is_cancelled(), true);
        assert.equal(child.killed, true);
        h.fireTimer([...h.timers.keys()][0]);
        const restarted = h.processes.at(-1);
        h.reply(restarted, { type: 'ready' });
        child.input.finish(0, new Error('late cancelled write'));
        child.exit();
        assert.equal(restarted.input.writes.length, 0, 'dropped work must not reach the new service');
        assert.equal(h.failures.length, 1, 'late callbacks from the old generation do not kill its replacement');
        h.worker.send({ type: 'tick' });
        assert.deepEqual(h.drainInput(restarted), [{ type: 'tick' }]);
        h.worker.dispose();
    });

    test('write errors and zero-byte writes fail once without blindly retrying the request', () => {
        for (const error of [null, new Error('broken pipe')]) {
            const h = pipeHarness();
            const child = h.startWorker();
            h.reply(child, { type: 'ready' });
            h.worker.send({ type: 'tick' });
            child.input.finish(0, error);
            assert.equal(h.failures.length, 1);
            assert.equal(child.killed, true);
            assert.equal(child.input.writes.length, 1);
            assert.equal(h.timers.size, 1);
            h.worker.dispose();
        }
    });

    test('disposal cancels pending I/O and restart timers and ignores a previously queued read', () => {
        const h = pipeHarness();
        const child = h.startWorker();
        const read = child.output.pending;
        h.worker.dispose();
        assert.equal(read.cancel.is_cancelled(), true);
        assert.equal(child.killed, true);
        assert.equal(h.timers.size, 0);
        child.output.feed('{"type":"ready"}\n');
        child.exit();
        assert.deepEqual(h.messages, []);
        assert.deepEqual(h.failures, []);
        assert.equal(h.timers.size, 0);
        h.worker.start();
        assert.equal(h.processes.length, 1);
    });
});

describe('Native/backend/service wiring', () => {
    test('an empty native guard cannot leave a disposed backend current', () => {
        const h = nativeHarness({ player: null });
        const backend = new h.MediaBackend();
        backend._receive({ type: 'state', ready: true, groups: [], players: [], progress: [],
            transports: {}, volumes: {}, activePlayer: '', failure: '', guard: { players: [], streams: [] } });
        assert.equal(backend.current(), true);
        backend.dispose();
        h.native.dispose();
        assert.equal(backend.disposed, true);
        assert.deepEqual(backend.native.capture(), { players: [], streams: [] });
        assert.equal(backend.current(), false, 'disposed state must invalidate even an otherwise matching empty guard');
    });

    test('the public constructor forwards GI captures, Rust cache updates, ticks, and guarded native results', () => {
        const h = pipeHarness();
        const player = new Player();
        const manager = Object.assign(new Signals(), { players: [player] });
        const audio = Object.assign(new Signals(), { streams: [] });
        let changes = 0, progressChanges = 0;
        const { MediaBackend } = loadBackend({ GLib: h.GLib, Gio: h.Gio,
            Mpris: { get_default: () => manager, PlaybackStatus: { PLAYING: 1, PAUSED: 2, STOPPED: 3 } },
            Wp: { get_default: () => ({ audio }) } });
        const backend = new MediaBackend({ onChange() { changes++; }, onProgress() { progressChanges++; } });
        backend.start();
        h.flush();
        const child = h.processes[0];
        assert.deepEqual(child.argv.slice(1), ['service']);
        assert.equal(child.input.writes.length, 0, 'process creation alone is not the service-ready handshake');
        h.reply(child, { type: 'ready' });
        const startup = h.drainInput(child);
        assert.equal(startup.length, 1);
        assert.equal(startup[0].type, 'snapshot');
        const snapshot = startup[0].snapshot;
        const raw = snapshot.players[0];
        const guard = { players: [{ bus: raw.bus, instance: raw.instance, seekRevision: raw.seekRevision }], streams: [] };
        const view = { ...raw, trackKey: '/track/one' };
        const state = { type: 'state', ready: true,
            groups: [{ primary: raw.bus, players: [raw.bus], title: raw.title, displayName: raw.identity,
                streamId: null, streamReason: 'No stream' }],
            activePlayer: raw.bus, players: [view], progress: [{ ...view, samplePosition: 10 }],
            transports: { [raw.bus]: { enabled: true, reason: 'Pause' } }, volumes: {}, failure: '', guard };
        h.reply(child, state);
        assert.equal(backend.current(), true);
        assert.equal(backend.resolveGroups()[0].primary.trackKey, '/track/one');
        const renders = changes;
        h.reply(child, { type: 'progress', progress: [{ ...view, position: 12.5, samplePosition: 10 }] });
        assert.equal(backend.progress(raw.bus).position, 12.5);
        assert.equal(changes, renders);
        assert.ok(progressChanges > 0);
        player.publish('volume', 0.8);
        h.flush();
        h.drainInput(child);
        const requests = child.input.text().trim().split('\n').map(line => JSON.parse(line));
        assert.equal(requests.at(-1).snapshot.players[0].volume, 0.8);
        backend.tick();
        h.drainInput(child);
        assert.equal(JSON.parse(child.input.text().trim().split('\n').at(-1)).type, 'tick');
        h.reply(child, { type: 'apply', id: 17, operation: { player: raw.bus, operation: 'seek', value: 80 },
            guard, expiresAt: h.time + 5 });
        h.drainInput(child);
        const result = JSON.parse(child.input.text().trim().split('\n').at(-1));
        assert.equal(result.type, 'result');
        assert.equal(result.id, 17);
        assert.equal(result.ok, true);
        assert.equal(result.snapshot.players[0].seekRevision, raw.seekRevision + 1);
        assert.equal(result.snapshot.players[0].position, 10);
        assert.equal(result.snapshot.players[0].positionRevision, raw.positionRevision);
        assert.deepEqual(player.calls, [['position', 80]]);
        assert.equal(backend.current(), false, 'only a new Rust state can acknowledge the changed guard');
        backend.dispose();
        assert.equal(child.killed, true);
        assert.equal(player.handlers.size, 0);
        assert.equal(manager.handlers.size, 0);
        assert.equal(audio.handlers.size, 0);
        assert.equal(h.timers.size, 0);
    });
});

describe('Bounded metadata helper I/O', () => {
    test('waits for stdout EOF and child exit, then strictly decodes the complete UTF-8 reply', async () => {
        const h = pipeHarness();
        const pending = h.runHelper(['/bin/media-control', 'artwork', 'file:///track']);
        const child = h.processes[0];
        const bytes = new TextEncoder().encode('/cache/作者.png\n');
        child.output.feed(bytes.slice(0, 8));
        child.output.feed(bytes.slice(8));
        child.output.feed(new Uint8Array());
        child.exit();
        assert.equal(await pending, '/cache/作者.png\n');
        assert.equal(h.timers.size, 0);
        h.flush();
    });

    test('credit input uses bounded async writes and the helper receives exactly one input string', async () => {
        const h = pipeHarness();
        const input = JSON.stringify({ url: 'https://example.test/', artist: '作者'.repeat(2500), album: '' });
        const pending = h.runHelper(['/bin/media-control', 'credit'], input);
        const child = h.processes[0];
        child.input.finish(3);
        let writes = 0;
        while (child.input.pending) { assert.ok(++writes < 100); child.input.finish(); }
        assert.equal(child.input.text(), input);
        assert.ok(child.input.writes.every(write => write.bytes.length <= 4096));
        assert.equal(child.input.closed, true);
        child.output.feed('reply');
        child.output.feed(new Uint8Array());
        child.exit();
        assert.equal(await pending, 'reply');
        h.flush();
    });

    test('invalid UTF-8 and oversized input fail without poisoning the next helper reply', async () => {
        const h = pipeHarness();
        const bad = h.runHelper(['/bin/media-control', 'credit']);
        const child = h.processes[0];
        child.output.feed(Uint8Array.from([0xc3, 0x28]));
        child.output.feed(new Uint8Array());
        child.exit();
        await assert.rejects(bad, /encoded|encoding|UTF/i);
        await assert.rejects(h.runHelper(['/bin/media-control', 'credit'], '界'.repeat(600000)), /large/i);
        assert.equal(h.processes.length, 1, 'oversized input is rejected before spawning');
        const good = h.runHelper(['/bin/media-control', 'credit']);
        const restarted = h.processes[1];
        restarted.output.feed('作者');
        restarted.output.feed(new Uint8Array());
        restarted.exit();
        assert.equal(await good, '作者');
        h.flush();
    });

    test('view cancellation terminates the helper, cancels I/O, and still reaps its child', async () => {
        const h = pipeHarness();
        const cancel = new Cancellable();
        const pending = h.runHelper(['/bin/media-control', 'credit'], 'input', cancel);
        const child = h.processes[0];
        const read = child.output.pending;
        cancel.cancel();
        await assert.rejects(pending, /cancelled/);
        assert.equal(child.killed, true);
        assert.equal(read.cancel.is_cancelled(), true);
        child.output.feed(new Uint8Array(), new Error('cancelled read'));
        child.input.finish(0, new Error('cancelled write'));
        child.exit(false);
        h.flush();
        assert.equal(cancel.handlers.size, 0);
        assert.equal(h.timers.size, 0);
    });
});

describe('Rust metadata helper bridge', () => {
    function lookupHarness(respond) {
        const calls = [];
        const { MediaBackend } = loadBackend({
            Gio: { Cancellable },
            runHelper: async (argv, input, cancellable) => {
                calls.push({ argv, input, cancellable });
                return respond(argv, input, cancellable);
            },
        });
        const backend = Object.assign(Object.create(MediaBackend.prototype), { helper: '/bin/media-control', disposed: false });
        return { backend, calls };
    }

    const metadata = { url: 'https://example.test/watch?v=one', artist: '作者', album: 'Album' };

    test('artwork resolution constructs the Rust command and validates an absolute single-path reply', async () => {
        const h = lookupHarness(() => '/cache/cover with spaces.png\n');
        const cancellable = new Cancellable();
        assert.equal(await h.backend.resolveArtwork('file:///music/track.flac', cancellable), '/cache/cover with spaces.png');
        assert.deepEqual(h.calls[0], {
            argv: ['/bin/media-control', 'artwork', 'file:///music/track.flac'], input: null, cancellable,
        });
        for (const reply of ['', 'relative.png\n', '/art/one\n/art/two\n', '/art/bad\0.png\n']) {
            const bad = lookupHarness(() => reply);
            await assert.rejects(bad.backend.resolveArtwork('file:///track'), /invalid absolute path/);
        }
        await assert.rejects(h.backend.resolveArtwork(''), /requires a track URL/);
    });

    test('artwork leaves URL/path policy and missing-artwork failures to Rust and propagates helper errors', async () => {
        for (const trackUrl of ['https://example.test/track', 'file:///music/%ZZ', 'relative.flac', 'not a URL',
            'file:///music/no-cover.flac']) {
            const error = new Error('Rust artwork lookup failed');
            const h = lookupHarness(() => { throw error; });
            await assert.rejects(h.backend.resolveArtwork(trackUrl), failure => failure === error);
            assert.deepEqual(h.calls[0].argv, ['/bin/media-control', 'artwork', trackUrl]);
        }
        const h = lookupHarness(() => { throw new Error('must not be called'); });
        for (const input of [null, undefined, 42, '', 'file:///bad\0path'])
            await assert.rejects(h.backend.resolveArtwork(input), /requires a track URL/);
        assert.equal(h.calls.length, 0, 'invalid argument types and NULs fail before starting the helper');
    });

    test('credit resolution serializes only Rust input fields, validates echoed identity, and returns credit text', async () => {
        const h = lookupHarness((_, input) => JSON.stringify({ ...JSON.parse(input), credit: 'Channel 作者' }));
        const cancellable = new Cancellable();
        assert.equal(await h.backend.resolveCredit({ ...metadata, bus: 'ignored' }, cancellable), 'Channel 作者');
        assert.deepEqual(h.calls[0], {
            argv: ['/bin/media-control', 'credit'], input: JSON.stringify(metadata), cancellable,
        });
        const empty = lookupHarness((_, input) => JSON.stringify({ ...JSON.parse(input), credit: '' }));
        assert.equal(await empty.backend.resolveCredit(metadata), '');
        assert.ok(empty.calls[0].cancellable instanceof Cancellable);
    });

    test('credit rejects malformed, mismatched, and non-string replies without applying JS fallback policy', async () => {
        for (const reply of ['invalid', 'null', '[]', '{}',
            JSON.stringify({ ...metadata, credit: 42 }),
            JSON.stringify({ ...metadata, artist: 'Other', credit: 'Wrong source' }),
            JSON.stringify({ ...metadata, url: 'https://other.test/', credit: 'Wrong source' })]) {
            const h = lookupHarness(() => reply);
            await assert.rejects(h.backend.resolveCredit(metadata), /invalid|mismatched/);
        }
        const h = lookupHarness(() => { throw new Error('helper failed'); });
        await assert.rejects(h.backend.resolveCredit(metadata), /helper failed/);
        await assert.rejects(h.backend.resolveCredit({ ...metadata, album: null }), /Credit album must be a string/);
    });

    test('lookups honor view-owned cancellation before dispatch and after an in-flight reply', async () => {
        for (const kind of ['resolveArtwork', 'resolveCredit']) {
            const input = kind === 'resolveArtwork' ? 'file:///track' : metadata;
            const cancelled = new Cancellable();
            cancelled.cancel();
            const h = lookupHarness(() => 'unreachable');
            await assert.rejects(h.backend[kind](input, cancelled), /cancelled/);
            assert.equal(h.calls.length, 0);
            let finish;
            const pending = lookupHarness(() => new Promise(resolve => { finish = resolve; }));
            const cancellable = new Cancellable();
            const promise = pending.backend[kind](input, cancellable);
            cancellable.cancel();
            finish(kind === 'resolveArtwork' ? '/art/cover\n' : JSON.stringify({ ...metadata, credit: 'Stale' }));
            await assert.rejects(promise, /cancelled/);
            h.backend.disposed = true;
            await assert.rejects(h.backend[kind](input), /disposed/);
            assert.equal(h.calls.length, 0);
        }
    });
});
describe('Presentation utilities', () => {
    const icon = '\u{f040a}';
    const truncation = [
        ['empty', '', ''],
        ['exact Latin limit', 'a'.repeat(50), 'a'.repeat(50)],
        ['Latin overflow', 'a'.repeat(51), 'a'.repeat(50) + '…'],
        ['exact CJK limit', '真'.repeat(35), '真'.repeat(35)],
        ['CJK overflow', '真'.repeat(36), '真'.repeat(35) + '…'],
        ['mixed Latin limit', '真'.repeat(35) + 'abcdefghijklmnop', '真'.repeat(35) + 'abcdefghijklmno…'],
        ['mixed exact limit', '真'.repeat(35) + 'abcdefghijklmno', '真'.repeat(35) + 'abcdefghijklmno'],
        ['CJK after intervening Latin', '真'.repeat(35) + 'abc真', '真'.repeat(35) + 'abc…'],
        ['supplementary ideographs', '\u{20000}'.repeat(36), '\u{20000}'.repeat(35) + '…'],
        ['extension G ideographs', '\u{30000}'.repeat(36), '\u{30000}'.repeat(35) + '…'],
        ['emoji count as one scalar', '😀'.repeat(51), '😀'.repeat(50) + '…'],
        ['kana are not ideographs', 'あ'.repeat(51), 'あ'.repeat(50) + '…'],
    ];
    for (const [name, input, output] of truncation) {
        test(`truncateTitle: ${name}`, () => assert.equal(truncateTitle(input), output));
    }

    test('original CJK range boundaries include astral scalars without splitting pairs', () => {
        const ranges = [[0x3400, 0x4dbf], [0x4e00, 0x9fff], [0xf900, 0xfaff], [0x20000, 0x2ebef], [0x30000, 0x3134f]];
        for (const [start, end] of ranges) {
            for (const code of [start, end]) {
                const character = String.fromCodePoint(code);
                assert.equal(truncateTitle(character.repeat(36)), character.repeat(35) + '…');
            }
            for (const code of [start - 1, end + 1]) {
                const character = String.fromCodePoint(code);
                assert.equal(truncateTitle(character.repeat(36)), character.repeat(36));
            }
        }
    });

    test('inactive badge preserves text, class, alt, and full tooltip contract', () => {
        const expected = { text: icon, class: 'inactive', alt: 'inactive', tooltip: 'No media playing' };
        assert.deepEqual(badge(null), expected);
        assert.deepEqual(badge(undefined), expected);
        assert.deepEqual(badge({ primary: null }), expected);
        assert.deepEqual(badge({ primary: 'bus-is-not-a-snapshot' }), expected);
    });

    test('badge uses the resolved selected primary snapshot and Astal snapshot field names', () => {
        const primary = { bus: 'native', title: ' Song ', artist: ' Artist ', identity: 'YouTube', playing: false, length: 200, position: 10 };
        assert.deepEqual(badge({ primary }), {
            text: icon + '  Song', class: 'paused', alt: 'paused', tooltip: 'Song — Artist',
        });
        primary.playing = true;
        assert.equal(badge({ primary }).class, 'playing');
        assert.equal(badge({ primary }).alt, 'playing');
    });

    test('badge title falls back to trimmed identity, then Unknown title', () => {
        const primary = { title: ' ', artist: '', identity: ' Browser ', playing: false };
        assert.equal(badge({ primary }).tooltip, 'Browser');
        primary.identity = ' ';
        assert.equal(badge({ primary }).tooltip, 'Unknown title');
    });

    test('tooltip stays full and unescaped, while badge text alone is truncated', () => {
        const title = '<&"'.repeat(30);
        const result = badge({ primary: { title, artist: ' A&B ', identity: '', playing: true } });
        assert.equal(result.tooltip, title + ' — A&B');
        assert.equal(result.text, icon + '  ' + truncateTitle(title));
        assert.deepEqual(JSON.parse(JSON.stringify(result)), result);
    });

    test('formatTime floors seconds and formats minute/hour boundaries', () => {
        for (const [seconds, expected] of [
            [0, '0:00'], [0.9, '0:00'], [9.99, '0:09'], [59.99, '0:59'], [60, '1:00'],
            [3599.99, '59:59'], [3600, '1:00:00'], [3661.8, '1:01:01'], [360000, '100:00:00'],
        ]) assert.equal(formatTime(seconds), expected);
    });

    test('formatTime preserves unknown/unsupported distinction from genuine zero', () => {
        for (const seconds of [-1, -0.1, NaN, Infinity, -Infinity, null, undefined, '12'])
            assert.equal(formatTime(seconds), '--:--');
    });
});
