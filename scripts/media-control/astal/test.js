import test, { describe } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { badge, truncateTitle, formatTime } from './logic.js';

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
        assert.match(selected, /border-color: alpha\(@mp_yellow, 0\.45\);/);
        assert.match(selected, /background: alpha\(@mp_red, 0\.1\);/);
        assert.doesNotMatch(selected, /(?:border(?:-width)?|padding|margin|min-height|min-width):/);
    }
    const current = '.media-panel .source-row.current';
    const currentRules = matchingRules(current);
    assert.equal(currentRules.length, 2);
    assert.equal(currentRules[1][1].trim(), current);
    assert.equal(currentRules[1][2].trim(), 'border-color: alpha(@mp_red, 0.55);');
    for (const state of ['', ':hover', ':focus-within', ':active']) {
        // Both selectors have equal specificity, so the last border color wins.
        const colors = matchingRules(current, `.media-panel .source-row${state}`)
            .flatMap(([, , body]) => [...body.matchAll(/border-color:\s*([^;]+);/g)].map(match => match[1]));
        assert.deepEqual(colors, ['alpha(@mp_yellow, 0.45)', 'alpha(@mp_red, 0.55)'], `Current must win over ${state || 'idle'}`);
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
        ['seek', 'one', 15], ['seek', 'one', 45]]);
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
    const runtime = readFileSync(new URL('../../../niri/panels.nix', import.meta.url), 'utf8');
    assert.match(runtime, /pkgs\.libavif/);
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

    test('15-second jumps use each fresh native position, not model or slider values', () => {
        const h = jumpHarness();
        h.panel.seekBy(-15);
        h.live.position = 50;
        h.panel.seekBy(15);
        assert.deepEqual(h.actions, [['seek', 'browser', 15], ['seek', 'browser', 65]]);
        assert.equal(h.selected.position, 10);
        assert.equal(h.live.position, 50, 'jumping must not fabricate a native position update');
        assert.deepEqual(h.failures, []);
    });

    test('display estimates are never used as native seek targets and unavailable progress blocks jumps', () => {
        const h = jumpHarness();
        h.progress = { ...h.live, position: 34.5, samplePosition: 30 };
        h.panel.seekBy(15);
        assert.deepEqual(h.actions, [['seek', 'browser', 45]], 'use the real 30-second sample, not estimated time');
        for (const progress of [null, { ...h.live, positionSupported: false }]) {
            h.progress = progress;
            h.panel.seekBy(15);
            assert.equal(h.actions.length, 1, 'stale or unavailable display timing must not queue jumps');
        }
    });

    test('jumps use a current-track position read rather than an older cached native position', () => {
        const h = jumpHarness();
        h.live.position = 100;
        h.progress = { ...h.live, position: 20, samplePosition: 20 };
        h.panel.seekBy(15);
        assert.deepEqual(h.actions, [['seek', 'browser', 35]]);
    });

    test('15-second jumps clamp to zero and the live track length, including genuine zero position', () => {
        for (const [position, seconds, target] of [[2, -15, 0], [35, 15, 40], [0, 15, 15], [40, -15, 25]]) {
            const h = jumpHarness();
            h.live.position = position;
            h.live.length = 40;
            h.panel.seekBy(seconds);
            assert.deepEqual(h.actions, [['seek', 'browser', target]], `position ${position}, jump ${seconds}`);
        }
    });

    test('jumps revalidate native support, finite samples, and exact track identity before dispatch', () => {
        for (const [name, overrides] of unavailableSeekSamples) {
            for (const seconds of [-15, 15]) {
                const h = jumpHarness();
                h.live = overrides && { ...h.live, ...overrides };
                h.panel.seekBy(seconds);
                assert.deepEqual(h.actions, [], `${name}, jump ${seconds}`);
            }
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
        assert.deepEqual(h.actions, [['seek', 'browser', 45]], 'release must not queue the old 80-second preview');
    });

    test('rejected jump queues report backend failures and a nonempty fallback through reportFailure', () => {
        for (const failure of ['Media worker is not ready.', '']) {
            const h = jumpHarness();
            h.accepted = false;
            h.panel.backend.failure = failure;
            h.panel.seekBy(15);
            assert.deepEqual(h.actions, [['seek', 'browser', 45]]);
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

// Evaluate the real backend with in-memory GI doubles; never connect to a bus
// or start a helper. Keep production imports native and dependency-free.
const source = readFileSync(new URL('./backend.js', import.meta.url), 'utf8')
    .replace(/^import .+;\r?$/gm, '')
    .replace(/^export /gm, '');
const factory = new Function('GLib', 'Gio', 'Mpris', 'Wp', 'badge', 'helper', 'queryPosition', `'use strict';
    ${source}
    if (helper) runHelper = helper;
    if (queryPosition) readPosition = queryPosition;
    return { Planner, NativeMedia, MediaBackend, ModelWorker, runHelper, ownershipStamp, readPosition,
        DirectPlayer: typeof DirectPlayer === 'undefined' ? undefined : DirectPlayer,
        LocalPlayerDiscovery: typeof LocalPlayerDiscovery === 'undefined' ? undefined : LocalPlayerDiscovery };
`);

const variantDouble = {
    is_object_path: value => typeof value === 'string' && /^\/(?:[A-Za-z0-9_]+(?:\/[A-Za-z0-9_]+)*)?$/.test(value),
};

function loadBackend({ GLib = {}, Gio = {}, Mpris = {}, Wp = {}, runHelper = null, queryPosition = null } = {}) {
    return factory({ Variant: variantDouble, ...GLib }, Gio, Mpris, Wp, badge, runHelper, queryPosition);
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

// Tuple replies unpack to arrays; a{sv} values stay variants, as they do in GJS.
class DBusVariant {
    static is_object_path = variantDouble.is_object_path;
    constructor(signature, values) { Object.assign(this, { signature, values }); }
    deep_unpack() { return this.values; }
}

const dbusVariant = (signature, values) => new DBusVariant(signature, values);
const mprisPath = '/org/mpris/MediaPlayer2';
const mprisRoot = 'org.mpris.MediaPlayer2';
const mprisPlayer = `${mprisRoot}.Player`;
const propertiesInterface = 'org.freedesktop.DBus.Properties';
const daemonInterface = 'org.freedesktop.DBus';
const playbackStatus = { PLAYING: 1, PAUSED: 2, STOPPED: 3 };

function dbusHarness() {
    const h = { calls: [], pending: new Set(), subscriptions: new Map(), sources: new Map(), time: 0 };
    let nextSignal = 0, nextSource = 0;
    h.respond = call => { throw new Error(`Unexpected D-Bus call: ${call.bus} ${call.method}`); };
    h.connection = {
        call(bus, path, iface, method, parameters, replyType, flags, timeout, cancellable, callback) {
            const call = { bus, path, iface, method, parameters, replyType, flags, timeout, cancellable,
                params: parameters?.deep_unpack() ?? [] };
            h.calls.push(call);
            h.pending.add(call);
            let finished = false, cancelSignal = 0;
            const finish = result => {
                if (finished) return;
                finished = true;
                h.pending.delete(call);
                if (cancelSignal) cancellable.disconnect(cancelSignal);
                queueMicrotask(() => callback(h.connection, result));
            };
            call.resolve = value => finish({ value });
            call.reject = error => finish({ error });
            if (cancellable?.is_cancelled()) call.reject(new Error('Operation was cancelled'));
            else if (cancellable) cancelSignal = cancellable.connect(() => call.reject(new Error('Operation was cancelled')));
            queueMicrotask(() => {
                if (finished) return;
                try {
                    const reply = h.respond(call);
                    // Undefined deliberately holds a request for race/cancellation tests.
                    if (reply !== undefined) call.resolve(reply);
                } catch (error) { call.reject(error); }
            });
        },
        call_finish(result) {
            if (result.error) throw result.error;
            return result.value;
        },
        signal_subscribe(sender, iface, member, path, arg0, flags, callback) {
            const id = ++nextSignal;
            h.subscriptions.set(id, { sender, iface, member, path, arg0, flags, callback });
            return id;
        },
        signal_unsubscribe(id) { h.subscriptions.delete(id); },
    };
    h.emit = (sender, iface, member, path, parameters) => {
        for (const signal of [...h.subscriptions.values()]) {
            if (signal.sender && signal.sender !== sender || signal.iface && signal.iface !== iface
                || signal.member && signal.member !== member || signal.path && signal.path !== path
                || signal.arg0 && signal.arg0 !== parameters.deep_unpack()[0]) continue;
            signal.callback(h.connection, sender, path, iface, member, parameters);
        }
    };
    const addSource = (kind, interval, callback) => {
        const id = ++nextSource;
        h.sources.set(id, { kind, interval, callback, due: h.time + interval });
        return id;
    };
    h.GLib = {
        Variant: DBusVariant, VariantType: class { constructor(signature) { this.signature = signature; } },
        PRIORITY_DEFAULT: 0, PRIORITY_DEFAULT_IDLE: 0, SOURCE_REMOVE: false, SOURCE_CONTINUE: true,
        get_monotonic_time: () => h.time * 1000, getenv: () => null,
        idle_add: (_, callback) => addSource('idle', 0, callback),
        timeout_add: (_, interval, callback) => addSource('timeout', interval, callback),
        source_remove: id => h.sources.delete(id),
    };
    h.Gio = { Cancellable, DBusCallFlags: { NONE: 0 }, DBusSignalFlags: { NONE: 0 }, DBus: { session: h.connection },
        DBusError: { get_remote_error: error => error.remoteName ?? null } };
    h.manager = Object.assign(new Signals(), { players: [] });
    h.audio = Object.assign(new Signals(), { streams: [] });
    h.api = loadBackend({ GLib: h.GLib, Gio: h.Gio,
        Mpris: { PlaybackStatus: playbackStatus, get_default: () => h.manager },
        Wp: { get_default: () => ({ audio: h.audio }) } });
    h.flush = async () => {
        for (let turn = 0; turn < 20; turn++) {
            await new Promise(resolve => setImmediate(resolve));
            const idle = [...h.sources].filter(([, entry]) => entry.kind === 'idle');
            if (!idle.length) return;
            for (const [id, entry] of idle)
                if (h.sources.has(id) && !entry.callback()) h.sources.delete(id);
        }
        assert.fail('Recovery work did not settle after 20 idle turns');
    };
    h.advance = async (milliseconds = 1000) => {
        h.time += milliseconds;
        for (const [id, entry] of [...h.sources]) {
            if (entry.kind !== 'timeout' || entry.due > h.time || !h.sources.has(id)) continue;
            entry.due = h.time + entry.interval;
            if (!entry.callback()) h.sources.delete(id);
        }
        await h.flush();
    };
    h.timers = () => [...h.sources.values()].filter(entry => entry.kind === 'timeout');
    h.positionCalls = () => h.calls.filter(call => call.iface === propertiesInterface
        && call.method === 'Get' && call.params[1] === 'Position');
    return h;
}

function mprisFixture({ identity = 'Haruna', entry = 'org.kde.haruna', status = 'Playing', position = 12_500_000 } = {}) {
    const metadata = {
        'mpris:trackid': dbusVariant('o', '/track/lost_name'),
        'mpris:length': dbusVariant('x', 240_000_000),
        'xesam:title': dbusVariant('s', 'Recovered track'),
        'xesam:artist': dbusVariant('as', ['Recovered artist']),
        'xesam:album': dbusVariant('s', 'Recovered album'),
        'xesam:url': dbusVariant('s', 'file:///music/recovered.flac'),
        'mpris:artUrl': dbusVariant('s', 'file:///music/cover.jpg'),
    };
    return {
        root: { Identity: dbusVariant('s', identity), DesktopEntry: dbusVariant('s', entry) },
        player: {
            PlaybackStatus: dbusVariant('s', status), Rate: dbusVariant('d', 1.25),
            Metadata: dbusVariant('a{sv}', metadata), Position: dbusVariant('x', position),
            CanPlay: dbusVariant('b', true), CanPause: dbusVariant('b', true), CanSeek: dbusVariant('b', true),
            CanGoNext: dbusVariant('b', false), CanGoPrevious: dbusVariant('b', true),
        },
        positionReply: dbusVariant('(v)', [dbusVariant('x', position)]),
    };
}

function mprisReply(fixture, call) {
    assert.equal(call.path, mprisPath);
    if (call.iface === propertiesInterface && call.method === 'GetAll') {
        assert.ok([mprisRoot, mprisPlayer].includes(call.params[0]));
        return dbusVariant('(a{sv})', [call.params[0] === mprisRoot ? fixture.root : fixture.player]);
    }
    if (call.iface === propertiesInterface && call.method === 'Get') {
        assert.deepEqual(call.params, [mprisPlayer, 'Position']);
        if (fixture.positionReply instanceof Error) throw fixture.positionReply;
        return fixture.positionReply;
    }
    assert.equal(call.iface, mprisPlayer);
    assert.ok(['Play', 'Pause', 'Next', 'Previous', 'SetPosition'].includes(call.method));
    return dbusVariant('()', []);
}

function directPlayerHarness(options) {
    const h = dbusHarness();
    assert.equal(typeof h.api.DirectPlayer, 'function', 'backend.js must implement DirectPlayer for lost-name recovery');
    h.bus = ':1.220';
    h.fixture = mprisFixture(options);
    h.respond = call => {
        assert.equal(call.bus, h.bus, 'MPRIS queries and controls must target the actual private owner');
        return mprisReply(h.fixture, call);
    };
    h.player = new h.api.DirectPlayer(h.bus, { connection: h.connection });
    h.changed = (values, invalidated = []) => h.emit(h.bus, propertiesInterface, 'PropertiesChanged', mprisPath,
        dbusVariant('(sa{sv}as)', [mprisPlayer, values, invalidated]));
    h.seeked = position => h.emit(h.bus, mprisPlayer, 'Seeked', mprisPath, dbusVariant('(x)', [position]));
    return h;
}

function assertUnknownPosition(player) {
    assert.ok(!Number.isFinite(player.position) || player.position < 0,
        `Missing/invalid Position must not become a fabricated sample: ${player.position}`);
}

describe('DirectPlayer lost-name recovery', () => {
    test('decodes Haruna MPRIS exports from a private owner without requiring its lost well-known name', async t => {
        const h = directPlayerHarness();
        t.after(() => h.player.dispose());
        assert.equal(await h.player.start(), h.player);
        await h.flush();
        const player = h.player;
        assert.equal(player.bus_name, ':1.220');
        assert.equal(player.available, true);
        assert.equal(player.identity, 'Haruna');
        assert.equal(player.entry, 'org.kde.haruna');
        assert.equal(player.playback_status, playbackStatus.PLAYING);
        assert.equal(player.rate, 1.25);
        assert.equal(player.length, 240);
        assert.equal(player.position, 12.5);
        assert.equal(player.trackid, '/track/lost_name');
        assert.equal(player.title, 'Recovered track');
        assert.equal(player.artist, 'Recovered artist');
        assert.equal(player.album, 'Recovered album');
        assert.equal(player.art_url, 'file:///music/cover.jpg');
        assert.equal(typeof player.cover_art, 'string', 'an empty cover is valid; local extraction remains Rust policy');
        assert.equal(player.get_meta('xesam:url').deep_unpack(), 'file:///music/recovered.flac');
        assert.deepEqual([player.can_play, player.can_pause, player.can_seek, player.can_go_next, player.can_go_previous],
            [true, true, true, false, true]);
        assert.deepEqual(h.calls.filter(call => call.method === 'GetAll').map(call => call.params[0]).sort(),
            [mprisRoot, mprisPlayer].sort());
        for (const call of h.calls) {
            assert.equal(call.timeout, 1500);
            assert.equal(call.flags, 0);
            assert.ok(call.cancellable instanceof Cancellable);
        }
    });

    test('unknown applications use their own identity and accept double-valued microseconds', async t => {
        const h = directPlayerHarness({ identity: 'Uncatalogued Audio Lab', entry: 'audio-lab', position: 1_500_000.5 });
        t.after(() => h.player.dispose());
        h.fixture.player.Metadata.deep_unpack()['mpris:length'] = dbusVariant('d', 240_000_000.5);
        h.fixture.player.Position = dbusVariant('d', 1_500_000.5);
        h.fixture.positionReply = dbusVariant('(v)', [dbusVariant('d', 1_500_000.5)]);
        await h.player.start();
        await h.flush();
        assert.equal(h.player.identity, 'Uncatalogued Audio Lab');
        assert.equal(h.player.entry, 'audio-lab');
        assert.equal(h.player.length, 240.0000005);
        assert.equal(h.player.position, 1.5000005);
    });

    test('PropertiesChanged and Seeked decode the final variant argument and notify live subscribers', async t => {
        const h = directPlayerHarness();
        t.after(() => h.player.dispose());
        await h.player.start();
        await h.flush();
        const notifications = [];
        const signal = h.player.connect('notify', (_, property) => notifications.push(property.name));
        h.fixture.positionReply = dbusVariant('(v)', [dbusVariant('x', 21_250_000)]);
        h.changed({ PlaybackStatus: dbusVariant('s', 'Paused'), Rate: dbusVariant('d', 0.75),
            Metadata: dbusVariant('a{sv}', {
                'mpris:trackid': dbusVariant('o', '/track/two'), 'xesam:title': dbusVariant('s', 'Second track'),
            }) });
        h.seeked(21_250_000);
        await h.flush();
        assert.equal(h.player.playback_status, playbackStatus.PAUSED);
        assert.equal(h.player.rate, 0.75);
        assert.equal(h.player.trackid, '/track/two');
        assert.equal(h.player.title, 'Second track');
        assert.equal(h.player.artist, '', 'replacement metadata must not retain the previous artist');
        assert.equal(h.player.album, '');
        assert.equal(h.player.art_url, '');
        assert.equal(h.player.get_meta('xesam:url'), null);
        assert.equal(h.player.position, 21.25);
        assert.ok(notifications.includes('position'));
        assert.ok(notifications.includes('playback-status'));
        const count = notifications.length;
        h.player.disconnect(signal);
        h.fixture.positionReply = dbusVariant('(v)', [dbusVariant('x', 22_000_000)]);
        h.seeked(22_000_000);
        await h.flush();
        assert.equal(h.player.position, 22);
        assert.equal(notifications.length, count);
        h.changed({ PlaybackStatus: dbusVariant('s', 'Stopped') });
        await h.flush();
        assert.equal(h.player.playback_status, playbackStatus.STOPPED);
    });

    test('controls dispatch to the unique bus and SetPosition never changes the sampled position optimistically', async t => {
        const h = directPlayerHarness({ status: 'Paused' });
        t.after(() => h.player.dispose());
        h.fixture.player.CanGoNext = dbusVariant('b', true);
        await h.player.start();
        await h.flush();
        const before = h.player.position;
        const start = h.calls.length;
        h.player.play();
        h.player.pause();
        h.player.next();
        h.player.previous();
        h.player.position = 42.25;
        assert.equal(h.player.position, before);
        await h.flush();
        const controls = h.calls.slice(start).filter(call => call.iface === mprisPlayer);
        assert.deepEqual(controls.map(call => call.method), ['Play', 'Pause', 'Next', 'Previous', 'SetPosition']);
        assert.ok(controls.every(call => call.bus === ':1.220' && call.path === mprisPath && call.timeout === 1500));
        assert.equal(controls.at(-1).parameters.signature, '(ox)');
        assert.deepEqual(controls.at(-1).params, ['/track/lost_name', 42_250_000]);
        assert.equal(h.player.position, before, 'successful dispatch is not a position sample');
        assert.equal(h.player.playback_status, playbackStatus.PAUSED);
        h.seeked(41_000_000);
        assert.equal(h.player.position, 41, 'only a real reply or signal can update progress');
    });

    test('polls Position asynchronously once per second only while playing, without overlapping queries', async t => {
        const h = directPlayerHarness({ status: 'Paused' });
        t.after(() => h.player.dispose());
        await h.player.start();
        await h.flush();
        assert.equal(h.timers().length, 0);
        const before = h.positionCalls().length;
        await h.advance(3000);
        assert.equal(h.positionCalls().length, before);
        h.changed({ PlaybackStatus: dbusVariant('s', 'Playing') });
        await h.flush();
        assert.equal(h.timers().length, 1);
        assert.equal(h.timers()[0].interval, 1000);
        const respond = h.respond;
        h.respond = call => call.method === 'Get' ? undefined : respond(call);
        await h.advance();
        const pending = [...h.pending].find(call => call.method === 'Get');
        assert.ok(pending, 'a playing timer must issue an async Position read');
        assert.deepEqual(pending.params, [mprisPlayer, 'Position']);
        assert.equal(pending.timeout, 1500);
        const count = h.positionCalls().length;
        await h.advance();
        assert.equal(h.positionCalls().length, count, 'a slow read must not produce unbounded overlapping requests');
        assert.equal(h.player.position, 12.5, 'elapsed time alone must not fabricate a native sample');
        pending.resolve(dbusVariant('(v)', [dbusVariant('d', 16_500_000.5)]));
        await h.flush();
        assert.equal(h.player.position, 16.5000005);
        h.changed({ PlaybackStatus: dbusVariant('s', 'Paused') });
        await h.flush();
        assert.equal(h.timers().length, 0);
        const paused = h.positionCalls().length;
        await h.advance(5000);
        assert.equal(h.positionCalls().length, paused);
    });

    test('missing metadata and Position remain unknown rather than inheriting fabricated track or progress data', async t => {
        for (const metadata of [undefined, dbusVariant('a{sv}', {})]) {
            const h = directPlayerHarness();
            t.after(() => h.player.dispose());
            if (metadata) h.fixture.player.Metadata = metadata;
            else delete h.fixture.player.Metadata;
            delete h.fixture.player.Position;
            h.fixture.positionReply = new Error('org.freedesktop.DBus.Error.UnknownProperty: Position');
            await h.player.start();
            await h.flush();
            assert.equal(h.player.available, true);
            for (const key of ['title', 'artist', 'album', 'art_url']) assert.equal(h.player[key], '');
            assert.equal(h.player.get_meta('xesam:url'), null);
            assert.ok(!Number.isFinite(h.player.length) || h.player.length <= 0);
            assert.notEqual(h.player.trackid, '/track/lost_name');
            assertUnknownPosition(h.player);
            await h.advance(60_000);
            assertUnknownPosition(h.player);
        }
    });

    test('negative, non-finite, or non-numeric Position replies never become numeric samples', async t => {
        const values = [['x', -1], ['d', NaN], ['d', Infinity], ['s', '1000000'], ['b', true]];
        for (const [signature, value] of values) {
            const h = directPlayerHarness();
            t.after(() => h.player.dispose());
            delete h.fixture.player.Position;
            h.fixture.positionReply = dbusVariant('(v)', [dbusVariant(signature, value)]);
            await h.player.start();
            await h.flush();
            await h.advance();
            assertUnknownPosition(h.player);
            h.player.dispose();
        }
    });

    test('dispose cancels queries and timers, unsubscribes signals, and ignores an already queued successful reply', async t => {
        const h = directPlayerHarness();
        t.after(() => h.player.dispose());
        await h.player.start();
        await h.flush();
        const respond = h.respond;
        h.respond = call => call.method === 'Get' ? undefined : respond(call);
        await h.advance();
        const pending = [...h.pending].find(call => call.method === 'Get');
        assert.ok(pending);
        assert.ok(h.subscriptions.size > 0);
        const notifications = [];
        h.player.connect('notify', (_, property) => notifications.push(property.name));
        pending.resolve(dbusVariant('(v)', [dbusVariant('x', 99_000_000)]));
        h.player.dispose();
        const count = notifications.length;
        await h.flush();
        assert.equal(pending.cancellable.is_cancelled(), true);
        assert.equal(h.player.available, false);
        assert.notEqual(h.player.position, 99);
        assert.equal(h.sources.size, 0);
        assert.equal(h.subscriptions.size, 0);
        const calls = h.calls.length;
        h.seeked(100_000_000);
        h.changed({ PlaybackStatus: dbusVariant('s', 'Playing') });
        await h.advance(5000);
        assert.equal(h.calls.length, calls);
        assert.equal(notifications.length, count);
        assert.doesNotThrow(() => h.player.dispose());
    });

    test('review race: coalesced reads discard the previous track and recover a fresh sample while paused', { timeout: 3000 }, async t => {
        const h = directPlayerHarness({ status: 'Paused' });
        t.after(() => h.player.dispose());
        await h.player.start();
        await h.flush();
        const respond = h.respond;
        h.respond = call => call.method === 'Get' ? undefined : respond(call);
        const before = h.positionCalls().length;
        const refreshing = h.player.refreshPosition();
        let joinedFinished = false;
        const joined = h.player.refreshPosition().then(() => { joinedFinished = true; });
        await h.flush();
        assert.equal(h.positionCalls().length, before + 1, 'concurrent callers must share one physical read');
        const old = [...h.pending].find(call => call.method === 'Get');
        assert.ok(old);
        const samples = [];
        h.player.connect('notify', (_, property) => {
            if (property.name === 'position') samples.push(h.player.position);
        });
        h.changed({ Metadata: dbusVariant('a{sv}', {
            'mpris:trackid': dbusVariant('o', '/track/current'), 'xesam:title': dbusVariant('s', 'Current track'),
        }) });
        old.resolve(dbusVariant('(v)', [dbusVariant('x', 99_000_000)]));
        await h.flush();
        assert.equal(h.player.trackid, '/track/current');
        assert.equal(h.player.title, 'Current track');
        assert.notEqual(h.player.position, 99);
        assert.ok(!samples.includes(99), 'an old-track value must not even be published transiently');
        const fresh = [...h.pending].find(call => call.method === 'Get' && call !== old);
        assert.ok(fresh, 'discarding an old generation must request one current sample even without a playing timer');
        assert.equal(h.positionCalls().length, before + 2);
        assert.equal(joinedFinished, false, 'coalesced callers must wait for current-generation recovery');
        fresh.resolve(dbusVariant('(v)', [dbusVariant('d', 5_250_000.5)]));
        await Promise.all([refreshing, joined]);
        await h.flush();
        assert.equal(h.player.position, 5.2500005);
        assert.equal(h.player.playback_status, playbackStatus.PAUSED);
        assert.equal(h.timers().length, 0);
        await h.advance(60_000);
        assert.equal(h.player.position, 5.2500005);
        assert.equal(h.positionCalls().length, before + 2, 'the recovery read is not a paused polling loop');
    });

    test('review race: a Position read started before SetPosition cannot overwrite the seek and gets one fresh paused read', { timeout: 3000 }, async t => {
        const h = directPlayerHarness({ status: 'Paused' });
        t.after(() => h.player.dispose());
        await h.player.start();
        await h.flush();
        const respond = h.respond;
        h.respond = call => call.method === 'Get' ? undefined : respond(call);
        const before = h.positionCalls().length;
        const refreshing = h.player.refreshPosition();
        await h.flush();
        const old = [...h.pending].find(call => call.method === 'Get');
        assert.ok(old);
        const position = h.player.position;
        const samples = [];
        h.player.connect('notify', (_, property) => {
            if (property.name === 'position') samples.push(h.player.position);
        });
        h.player.position = 80;
        assert.equal(h.player.position, position, 'SetPosition dispatch must not optimistically change the sample');
        await h.flush();
        assert.ok(h.calls.some(call => call.method === 'SetPosition' && call.params[1] === 80_000_000));
        old.resolve(dbusVariant('(v)', [dbusVariant('x', 99_000_000)]));
        await h.flush();
        assert.notEqual(h.player.position, 99);
        assert.ok(!samples.includes(99));
        const fresh = [...h.pending].find(call => call.method === 'Get' && call !== old);
        assert.ok(fresh, 'a pre-seek generation must be replaced even when the player stays paused');
        assert.equal(h.positionCalls().length, before + 2);
        fresh.resolve(dbusVariant('(v)', [dbusVariant('x', 79_500_000)]));
        await refreshing;
        await h.flush();
        assert.equal(h.player.position, 79.5, 'use the remote sample, not the requested 80-second target');
        assert.equal(h.player.playback_status, playbackStatus.PAUSED);
        assert.equal(h.timers().length, 0);
    });

    test('review race: initial GetAll baselines cannot overwrite newer root/player changes or Seeked', { timeout: 3000 }, async t => {
        const h = directPlayerHarness();
        t.after(() => h.player.dispose());
        const respond = h.respond;
        h.respond = call => call.method === 'GetAll' ? undefined : respond(call);
        const starting = h.player.start();
        await h.flush();
        const root = [...h.pending].find(call => call.params[0] === mprisRoot);
        const player = [...h.pending].find(call => call.params[0] === mprisPlayer);
        assert.ok(root && player);
        h.emit(h.bus, propertiesInterface, 'PropertiesChanged', mprisPath, dbusVariant('(sa{sv}as)', [mprisRoot,
            { Identity: dbusVariant('s', 'Newer identity'), DesktopEntry: dbusVariant('s', 'newer-entry') }, []]));
        h.changed({ PlaybackStatus: dbusVariant('s', 'Paused'), Rate: dbusVariant('d', 0.5),
            Metadata: dbusVariant('a{sv}', {
                'mpris:trackid': dbusVariant('o', '/track/newer'), 'xesam:title': dbusVariant('s', 'Newer track'),
            }), Position: dbusVariant('x', 31_000_000) });
        h.seeked(44_500_000);
        root.resolve(dbusVariant('(a{sv})', [{ ...h.fixture.root }]));
        player.resolve(dbusVariant('(a{sv})', [{ ...h.fixture.player }]));
        await starting;
        await h.flush();
        assert.equal(h.player.available, true);
        assert.equal(h.player.identity, 'Newer identity');
        assert.equal(h.player.entry, 'newer-entry');
        assert.equal(h.player.trackid, '/track/newer');
        assert.equal(h.player.title, 'Newer track');
        assert.equal(h.player.artist, '', 'the older baseline must not resurrect old metadata');
        assert.equal(h.player.playback_status, playbackStatus.PAUSED);
        assert.equal(h.player.rate, 0.5);
        assert.equal(h.player.position, 44.5, 'replay changes in arrival order, including Seeked after Position');
        assert.equal(h.timers().length, 0);
    });

    test('startup metadata without a new position cannot inherit the previous track sample', async t => {
        const h = directPlayerHarness({ status: 'Paused' });
        t.after(() => h.player.dispose());
        const respond = h.respond;
        h.respond = call => call.method === 'GetAll' ? undefined : respond(call);
        const starting = h.player.start();
        await h.flush();
        const root = [...h.pending].find(call => call.params[0] === mprisRoot);
        const player = [...h.pending].find(call => call.params[0] === mprisPlayer);
        player.resolve(dbusVariant('(a{sv})', [{ ...h.fixture.player }]));
        await h.flush();
        h.changed({ Metadata: dbusVariant('a{sv}', {
            'mpris:trackid': dbusVariant('o', '/track/current'), 'xesam:title': dbusVariant('s', 'Current track'),
        }) });
        h.fixture.positionReply = new Error('Position is unavailable');
        root.resolve(dbusVariant('(a{sv})', [{ ...h.fixture.root }]));
        await starting;
        await h.flush();
        assert.equal(h.player.available, true);
        assert.equal(h.player.trackid, '/track/current');
        assertUnknownPosition(h.player);
        assert.ok(h.positionCalls().length > 0, 'startup must attempt to read the new track position');
        assert.equal(h.timers().length, 0);
    });

    test('review race: a reload baseline cannot roll back PropertiesChanged or Seeked received while pending', async t => {
        const h = directPlayerHarness({ status: 'Paused' });
        t.after(() => h.player.dispose());
        await h.player.start();
        await h.flush();
        const respond = h.respond;
        h.respond = call => call.method === 'GetAll' && call.params[0] === mprisPlayer ? undefined : respond(call);
        h.changed({}, ['Metadata', 'Position']);
        await h.flush();
        const reload = [...h.pending].find(call => call.method === 'GetAll' && call.params[0] === mprisPlayer);
        assert.ok(reload);
        h.changed({ Rate: dbusVariant('d', 2), CanPlay: dbusVariant('b', false),
            Metadata: dbusVariant('a{sv}', {
                'mpris:trackid': dbusVariant('o', '/track/reloaded'), 'xesam:title': dbusVariant('s', 'Newer than reload'),
            }), Position: dbusVariant('x', 15_000_000) });
        h.seeked(27_500_000);
        reload.resolve(dbusVariant('(a{sv})', [{ ...h.fixture.player }]));
        await h.flush();
        assert.equal(h.player.trackid, '/track/reloaded');
        assert.equal(h.player.title, 'Newer than reload');
        assert.equal(h.player.rate, 2);
        assert.equal(h.player.can_play, false);
        assert.equal(h.player.position, 27.5);
        assert.equal(h.player.playback_status, playbackStatus.PAUSED);
    });

    test('review race: invalidations during a pending reload coalesce into one follow-up reload without losing newer signals', async t => {
        const h = directPlayerHarness({ status: 'Paused' });
        t.after(() => h.player.dispose());
        await h.player.start();
        await h.flush();
        const reloads = () => h.calls.filter(call => call.method === 'GetAll' && call.params[0] === mprisPlayer);
        const before = reloads().length;
        const respond = h.respond;
        h.respond = call => call.method === 'GetAll' && call.params[0] === mprisPlayer ? undefined : respond(call);
        h.changed({}, ['Metadata']);
        await h.flush();
        const first = [...h.pending].find(call => call.method === 'GetAll' && call.params[0] === mprisPlayer);
        assert.ok(first);
        h.changed({}, ['Position']);
        h.changed({}, ['Rate', 'Metadata']);
        await h.flush();
        assert.equal(reloads().length, before + 1, 'invalidations must not start overlapping reloads');
        first.resolve(dbusVariant('(a{sv})', [{ ...h.fixture.player }]));
        await h.flush();
        const followup = [...h.pending].find(call => call.method === 'GetAll' && call.params[0] === mprisPlayer && call !== first);
        assert.ok(followup, 'a pending reload must not swallow later invalidations');
        assert.equal(reloads().length, before + 2);
        h.changed({ Rate: dbusVariant('d', 0.75), Metadata: dbusVariant('a{sv}', {
            'mpris:trackid': dbusVariant('o', '/track/latest'), 'xesam:title': dbusVariant('s', 'Latest signal'),
        }) });
        h.seeked(77_000_000);
        followup.resolve(dbusVariant('(a{sv})', [{ ...h.fixture.player, Position: dbusVariant('x', 50_000_000) }]));
        await h.flush();
        assert.equal(h.player.trackid, '/track/latest');
        assert.equal(h.player.title, 'Latest signal');
        assert.equal(h.player.rate, 0.75);
        assert.equal(h.player.position, 77);
        assert.equal(reloads().length, before + 2);
        assert.equal(h.timers().length, 0);
    });

    test('review race: NativeMedia refreshes DirectPlayer through its coalesced reader, never a second raw D-Bus query', { timeout: 3000 }, async t => {
        const h = directPlayerHarness({ status: 'Paused' });
        t.after(() => h.player.dispose());
        await h.player.start();
        await h.flush();
        const respond = h.respond;
        h.respond = call => call.method === 'Get' ? undefined : respond(call);
        const before = h.positionCalls().length;
        const refreshing = h.player.refreshPosition();
        await h.flush();
        const old = [...h.pending].find(call => call.method === 'Get');
        assert.ok(old);
        let adapterReads = 0;
        const refreshPosition = h.player.refreshPosition.bind(h.player);
        h.player.refreshPosition = (...args) => { adapterReads++; return refreshPosition(...args); };
        const rawReads = [];
        const native = new h.api.NativeMedia({ queryPosition: (bus, cancel) => {
            rawReads.push({ bus, cancel });
            return Promise.resolve(99);
        } });
        t.after(() => native.dispose());
        native.discovery = { players: [h.player], refresh: () => Promise.resolve(), dispose() {} };
        native.sync();
        await h.flush();
        assert.ok(adapterReads > 0, 'NativeMedia must delegate confirmed reads to the DirectPlayer adapter');
        assert.deepEqual(rawReads, [], 'raw readPosition bypasses the adapter generation guard');
        assert.equal(h.positionCalls().length, before + 1);
        h.changed({ Metadata: dbusVariant('a{sv}', {
            'mpris:trackid': dbusVariant('o', '/track/native-current'), 'xesam:title': dbusVariant('s', 'Native current track'),
        }) });
        old.resolve(dbusVariant('(v)', [dbusVariant('x', 99_000_000)]));
        await h.flush();
        assert.notEqual(native.player(h.bus).position, 99);
        const fresh = [...h.pending].find(call => call.method === 'Get' && call !== old);
        assert.ok(fresh);
        fresh.resolve(dbusVariant('(v)', [dbusVariant('x', 6_250_000)]));
        await refreshing;
        await h.flush();
        assert.equal(native.player(h.bus).position, 6.25);
        assert.equal(native.progress(h.bus).samplePosition, 6.25);
        assert.equal(native.progress(h.bus).positionSupported, true);
        assert.deepEqual(rawReads, []);
        assert.equal(h.positionCalls().length, before + 2);
        await h.advance(60_000);
        assert.equal(native.progress(h.bus).position, 6.25);
        assert.equal(h.positionCalls().length, before + 2);
    });

    test('dispose during start cancels outstanding GetAll calls and cannot resurrect an available player', { timeout: 3000 }, async t => {
        const h = directPlayerHarness();
        t.after(() => h.player.dispose());
        h.respond = () => undefined;
        const starting = h.player.start().then(() => null, error => error);
        await h.flush();
        const pending = [...h.pending];
        assert.ok(pending.length > 0);
        h.player.dispose();
        assert.ok(pending.every(call => call.cancellable.is_cancelled()));
        await starting;
        await h.flush();
        assert.equal(h.player.available, false);
        assert.equal(h.sources.size, 0);
        assert.equal(h.subscriptions.size, 0);
    });
});

function discoveryHarness({ owners = {}, streams = [], players = [], standardOwners = {}, realPlayers = false,
    gated = false, failing = [], startErrors = {} } = {}) {
    const h = dbusHarness();
    assert.equal(typeof h.api.LocalPlayerDiscovery, 'function', 'backend.js must implement LocalPlayerDiscovery for lost-name recovery');
    h.owners = new Map(Object.entries(owners));
    h.names = [daemonInterface, ...h.owners.keys(), ...Object.keys(standardOwners)];
    h.standardOwners = standardOwners;
    h.streams = streams;
    h.standardPlayers = players;
    h.applicationReads = [];
    h.created = [];
    h.starts = [];
    h.gates = new Map();
    h.startErrors = new Map(Object.entries(startErrors).map(([bus, errors]) => [bus, [...errors]]));
    h.changes = 0;
    h.fixture = mprisFixture();
    h.respond = call => {
        if (call.iface !== daemonInterface) return mprisReply(h.fixture, call);
        assert.equal(call.bus, daemonInterface);
        assert.equal(call.path, '/org/freedesktop/DBus');
        if (call.method === 'ListNames') return dbusVariant('(as)', [h.names]);
        if (call.method === 'GetNameOwner') {
            const owner = h.standardOwners[call.params[0]];
            if (!owner) throw new Error('org.freedesktop.DBus.Error.NameHasNoOwner');
            return dbusVariant('(s)', [owner]);
        }
        assert.equal(call.method, 'GetConnectionUnixProcessID');
        const owner = h.owners.get(call.params[0]);
        if (!owner) throw new Error('org.freedesktop.DBus.Error.NameHasNoOwner');
        return dbusVariant('(u)', [owner.pid]);
    };
    h.discovery = new h.api.LocalPlayerDiscovery({
        connection: h.connection, getStreams: () => h.streams, getPlayers: () => h.standardPlayers,
        onChange: () => { h.changes++; h.onDiscoveryChange?.(); },
        applicationForPid: pid => {
            h.applicationReads.push(pid);
            return [...h.owners.values()].find(owner => owner.pid === pid)?.application ?? '';
        },
        createPlayer: bus => {
            let player;
            if (realPlayers) {
                assert.equal(typeof h.api.DirectPlayer, 'function', 'the real discovery path needs DirectPlayer');
                player = new h.api.DirectPlayer(bus, { connection: h.connection });
            } else {
                player = new Player({ bus_name: bus });
                player.start = async () => {
                    h.starts.push(bus);
                    if (gated) await new Promise(resolve => h.gates.set(bus, resolve));
                    const errors = h.startErrors.get(bus);
                    if (errors?.length) throw errors.shift();
                    if (failing.includes(bus)) throw new Error('No MPRIS exports on this connection');
                    return player;
                };
                player.disposeCalls = 0;
                player.dispose = () => { player.disposeCalls++; player.available = false; };
            }
            h.created.push(player);
            return player;
        },
    });
    h.ownerChanged = (name, oldOwner, newOwner) => h.emit(daemonInterface, daemonInterface, 'NameOwnerChanged',
        '/org/freedesktop/DBus', dbusVariant('(sss)', [name, oldOwner, newOwner]));
    h.credentials = () => h.calls.filter(call => call.method === 'GetConnectionUnixProcessID');
    h.buses = () => h.discovery.players.map(player => player.bus_name).sort();
    h.refresh = async () => { await h.discovery.refresh(); await h.flush(); };
    return h;
}

function dbusRemoteError(name) {
    const remoteName = `${daemonInterface}.Error.${name}`;
    return Object.assign(new Error(`GDBus.Error:${remoteName}: Player exports are not ready`), { remoteName });
}

describe('LocalPlayerDiscovery lost-name recovery', () => {
    test('recovers the exact Haruna case: no org.mpris Haruna name, but a matching private owner exports MPRIS', async t => {
        const h = discoveryHarness({
            owners: { ':1.220': { pid: 2200, application: 'haruna' }, ':1.221': { pid: 2210, application: 'unrelated-daemon' } },
            streams: [new Stream({ name: 'Recovered track', properties: { 'application.name': 'Haruna', 'application.process.binary': 'haruna' } })],
            realPlayers: true,
        });
        t.after(() => h.discovery.dispose());
        assert.ok(!h.names.some(name => name.startsWith('org.mpris.')));
        await h.discovery.start();
        await h.flush();
        assert.deepEqual(h.buses(), [':1.220']);
        assert.equal(h.discovery.players[0].identity, 'Haruna');
        assert.equal(h.discovery.players[0].title, 'Recovered track');
        assert.equal(h.discovery.players[0].position, 12.5);
        const probes = h.calls.filter(call => call.method === 'GetAll');
        assert.ok(probes.length > 0);
        assert.ok(probes.every(call => call.bus === ':1.220'), 'unrelated processes must never receive MPRIS probes');
        const before = h.calls.length;
        h.discovery.players[0].pause();
        await h.flush();
        assert.ok(h.calls.slice(before).some(call => call.bus === ':1.220' && call.method === 'Pause'));
    });

    test('unknown application matching is generic across application.name, application.id, and process binary', async t => {
        for (const key of ['application.name', 'application.id', 'application.process.binary']) {
            const h = discoveryHarness({
                owners: { ':1.310': { pid: 3100, application: 'uncatalogued-player' }, ':1.311': { pid: 3110, application: 'other-process' } },
                streams: [new Stream({ name: 'A track, not the application', properties: { [key]: 'uncatalogued-player' } })],
                realPlayers: true,
            });
            t.after(() => h.discovery.dispose());
            h.fixture = mprisFixture({ identity: 'Uncatalogued Player', entry: 'uncatalogued-player' });
            await h.discovery.start();
            await h.flush();
            assert.deepEqual(h.buses(), [':1.310'], key);
            assert.equal(h.discovery.players[0].identity, 'Uncatalogued Player');
            assert.ok(h.calls.filter(call => call.method === 'GetAll').every(call => call.bus === ':1.310'));
            h.discovery.dispose();
        }
    });

    test('standard Astal players retain the fast path and their unique owners are not probed or duplicated', async t => {
        const standardBus = 'org.mpris.MediaPlayer2.uncatalogued';
        const h = discoveryHarness({
            owners: { ':1.401': { pid: 4010, application: 'uncatalogued-player' }, ':1.402': { pid: 4020, application: 'uncatalogued-player' } },
            streams: [new Stream({ properties: { 'application.process.binary': 'uncatalogued-player' } })],
            players: [new Player({ bus_name: standardBus })], standardOwners: { [standardBus]: ':1.401' },
        });
        t.after(() => h.discovery.dispose());
        await h.discovery.start();
        await h.flush();
        assert.deepEqual(h.buses(), [':1.402']);
        assert.deepEqual(h.starts, [':1.402']);
        assert.ok(h.calls.some(call => call.method === 'GetNameOwner' && call.params[0] === standardBus));
        await h.refresh();
        assert.deepEqual(h.starts, [':1.402']);
        assert.equal(h.standardPlayers[0].available, true, 'discovery must not dispose the standard Astal player');
    });

    test('volume notifications and unchanged refreshes reuse cached credentials and never repeat probes or discovery timers', async t => {
        const stream = new Stream({ properties: { 'application.process.binary': 'audio-lab' } });
        const h = discoveryHarness({
            owners: { ':1.501': { pid: 5010, application: 'audio-lab' }, ':1.502': { pid: 5020, application: 'unrelated' } },
            streams: [stream],
        });
        t.after(() => h.discovery.dispose());
        await h.discovery.start();
        await h.flush();
        const credentials = h.credentials().length;
        const applications = [...h.applicationReads];
        assert.ok(credentials > 0);
        assert.deepEqual(h.starts, [':1.501']);
        for (const volume of [0.2, 0.4, 0.6, 0.8]) {
            stream.publish('volume', volume);
            await h.refresh();
        }
        assert.equal(h.credentials().length, credentials);
        assert.deepEqual(h.applicationReads, applications);
        assert.deepEqual(h.starts, [':1.501']);
        assert.equal(h.calls.filter(call => call.method === 'ListNames').length, 1);
        assert.equal(h.timers().length, 0, 'discovery is event-driven, not a recurring bus scan');
        const calls = h.calls.length;
        await h.advance(60_000);
        assert.equal(h.calls.length, calls);
    });

    test('review race: streams admit new owners but pause/removal retain the verified object until its owner disappears', async t => {
        const h = discoveryHarness({ owners: { ':1.601': { pid: 6010, application: 'audio-lab' } } });
        t.after(() => h.discovery.dispose());
        await h.discovery.start();
        await h.flush();
        assert.deepEqual(h.starts, []);
        assert.equal(h.credentials().length, 0, 'no playback applications means no process lookup or MPRIS scan');
        h.streams = [new Stream({ properties: { 'application.name': 'audio-lab' } })];
        await h.refresh();
        assert.deepEqual(h.buses(), [':1.601']);
        const credentials = h.credentials().length;
        const applications = [...h.applicationReads];
        const recovered = h.discovery.players[0];
        recovered.publish('playback-status', playbackStatus.PAUSED);
        h.streams = [];
        await h.refresh();
        assert.deepEqual(h.discovery.players, [recovered]);
        assert.equal(recovered.available, true);
        assert.equal(recovered.disposeCalls, 0, 'a missing playback stream is not evidence of a vanished MPRIS owner');
        h.streams = [new Stream({ id: 125, properties: { 'application.name': 'audio-lab' } })];
        await h.refresh();
        assert.equal(h.discovery.players[0], recovered, 'resuming must preserve the adapter instance and action guards');
        assert.deepEqual(h.starts, [':1.601']);
        assert.equal(h.credentials().length, credentials);
        assert.deepEqual(h.applicationReads, applications);
        h.streams = [];
        await h.refresh();
        h.owners.delete(':1.601');
        h.ownerChanged(':1.601', ':1.601', '');
        await h.flush();
        assert.deepEqual(h.buses(), []);
        assert.equal(recovered.disposeCalls, 1);
    });

    test('NameOwnerChanged discovers a new matching private connection and removes a disconnected owner', async t => {
        const h = discoveryHarness({ streams: [new Stream({ properties: { 'application.name': 'audio-lab' } })] });
        t.after(() => h.discovery.dispose());
        await h.discovery.start();
        await h.flush();
        h.owners.set(':1.701', { pid: 7010, application: 'audio-lab' });
        h.ownerChanged(':1.701', '', ':1.701');
        await h.flush();
        assert.deepEqual(h.buses(), [':1.701']);
        const recovered = h.created[0];
        const changes = h.changes;
        h.owners.delete(':1.701');
        h.ownerChanged(':1.701', ':1.701', '');
        await h.flush();
        assert.deepEqual(h.buses(), []);
        assert.equal(recovered.disposeCalls, 1);
        assert.ok(h.changes > changes);
        assert.equal(h.calls.filter(call => call.method === 'ListNames').length, 1,
            'the daemon signal already identifies the changed owner');
        assert.equal(h.timers().length, 0);
    });

    test('matching processes without MPRIS exports are not repeatedly reprobed on volume-only refreshes', async t => {
        const h = discoveryHarness({
            owners: { ':1.801': { pid: 8010, application: 'audio-lab' } }, failing: [':1.801'],
            streams: [new Stream({ properties: { 'application.process.binary': 'audio-lab' } })],
        });
        t.after(() => h.discovery.dispose());
        await h.discovery.start();
        await h.flush();
        assert.deepEqual(h.buses(), []);
        assert.deepEqual(h.starts, [':1.801']);
        for (let index = 0; index < 3; index++) await h.refresh();
        assert.equal(h.timers().length, 0, 'an unclassified failure must not schedule a startup retry');
        await h.advance(60_000);
        assert.deepEqual(h.starts, [':1.801']);
        assert.equal(h.created[0].disposeCalls, 1, 'failed probes must release their player');
    });

    for (const name of ['UnknownObject', 'UnknownInterface', 'UnknownMethod']) {
        test(`review race: startup ${name} gets one 250ms retry and can then recover`, { timeout: 3000 }, async t => {
            const bus = ':1.1201';
            const h = discoveryHarness({ owners: { [bus]: { pid: 12010, application: 'audio-lab' } },
                streams: [new Stream({ properties: { 'application.name': 'audio-lab' } })],
                startErrors: { [bus]: [dbusRemoteError(name)] } });
            t.after(() => h.discovery.dispose());
            const starting = Promise.resolve(h.discovery.start()).then(() => null, error => error);
            await h.flush();
            assert.deepEqual(h.starts, [bus]);
            assert.deepEqual(h.buses(), []);
            assert.equal(h.created[0].disposeCalls, 1);
            assert.equal(h.timers().length, 1);
            assert.equal(h.timers()[0].interval, 250);
            await h.advance(249);
            assert.deepEqual(h.starts, [bus], 'do not retry early or spin through microtasks');
            await h.advance(1);
            await starting;
            await h.flush();
            assert.deepEqual(h.starts, [bus, bus]);
            assert.deepEqual(h.buses(), [bus]);
            assert.equal(h.created.length, 2, 'a disposed failed adapter must not be restarted');
            assert.equal(h.discovery.players[0], h.created[1]);
            assert.equal(h.created[1].disposeCalls, 0);
            assert.equal(h.credentials().length, 1, 'the retry must reuse the owner credential lookup');
            assert.equal(h.timers().length, 0);
            await h.refresh();
            await h.advance(60_000);
            assert.deepEqual(h.starts, [bus, bus]);
            assert.equal(h.calls.filter(call => call.method === 'ListNames').length, 1);
        });

        test(`review race: a second startup ${name} is negatively cached with no third attempt`, { timeout: 3000 }, async t => {
            const bus = ':1.1202';
            const stream = new Stream({ properties: { 'application.process.binary': 'audio-lab' } });
            const h = discoveryHarness({ owners: { [bus]: { pid: 12020, application: 'audio-lab' } }, streams: [stream],
                startErrors: { [bus]: [dbusRemoteError(name), dbusRemoteError(name)] } });
            t.after(() => h.discovery.dispose());
            const starting = Promise.resolve(h.discovery.start()).then(() => null, error => error);
            await h.flush();
            assert.deepEqual(h.starts, [bus]);
            assert.equal(h.timers().length, 1);
            assert.equal(h.timers()[0].interval, 250);
            await h.advance(250);
            await starting;
            await h.flush();
            assert.deepEqual(h.starts, [bus, bus]);
            assert.deepEqual(h.buses(), []);
            assert.equal(h.timers().length, 0, 'the retry budget is one, not a discovery loop');
            for (const volume of [0.3, 0.5, 0.7]) {
                stream.publish('volume', volume);
                await h.refresh();
            }
            await h.advance(60_000);
            assert.deepEqual(h.starts, [bus, bus]);
            assert.ok(h.created.every(player => player.disposeCalls === 1));
            assert.equal(h.credentials().length, 1);
            assert.equal(h.calls.filter(call => call.method === 'ListNames').length, 1);
        });
    }

    for (const name of ['Timeout', 'NoReply', 'UnknownProperty', 'AccessDenied']) {
        test(`review race: startup ${name} is not retryable and stays negatively cached`, { timeout: 3000 }, async t => {
            const bus = ':1.1203';
            const h = discoveryHarness({ owners: { [bus]: { pid: 12030, application: 'audio-lab' } },
                streams: [new Stream({ properties: { 'application.name': 'audio-lab' } })],
                startErrors: { [bus]: [dbusRemoteError(name)] } });
            t.after(() => h.discovery.dispose());
            await h.discovery.start();
            await h.flush();
            assert.deepEqual(h.buses(), []);
            assert.deepEqual(h.starts, [bus]);
            assert.equal(h.timers().length, 0);
            await h.advance(250);
            await h.refresh();
            await h.advance(60_000);
            assert.deepEqual(h.starts, [bus]);
            assert.equal(h.created[0].disposeCalls, 1);
            assert.equal(h.credentials().length, 1);
            assert.equal(h.calls.filter(call => call.method === 'ListNames').length, 1);
        });
    }

    test('review race: disposal cancels a pending startup retry instead of creating another adapter', { timeout: 3000 }, async t => {
        const bus = ':1.1204';
        const h = discoveryHarness({ owners: { [bus]: { pid: 12040, application: 'audio-lab' } },
            streams: [new Stream({ properties: { 'application.name': 'audio-lab' } })],
            startErrors: { [bus]: [dbusRemoteError('UnknownObject')] } });
        t.after(() => h.discovery.dispose());
        const starting = Promise.resolve(h.discovery.start()).then(() => null, error => error);
        await h.flush();
        assert.equal(h.timers().length, 1);
        h.discovery.dispose();
        const changes = h.changes;
        assert.equal(h.sources.size, 0);
        assert.equal(h.subscriptions.size, 0);
        await h.advance(60_000);
        await starting;
        assert.deepEqual(h.starts, [bus]);
        assert.deepEqual(h.buses(), []);
        assert.equal(h.changes, changes);
    });

    test('review race: a standard Astal arrival replaces the verified adapter and cleans up native records and subscriptions', async t => {
        const bus = ':1.1301';
        const standardBus = 'org.mpris.MediaPlayer2.audio_lab';
        const h = discoveryHarness({ owners: { [bus]: { pid: 13010, application: 'audio-lab' } }, realPlayers: true,
            streams: [new Stream({ properties: { 'application.name': 'audio-lab' } })] });
        t.after(() => h.discovery.dispose());
        h.fixture = mprisFixture({ identity: 'Audio Lab', entry: 'audio-lab', status: 'Paused' });
        h.audio.streams = h.streams;
        const native = new h.api.NativeMedia({ queryPosition: () => Promise.resolve(12.5) });
        t.after(() => native.dispose());
        native.discovery = h.discovery;
        h.onDiscoveryChange = () => native.sync();
        native.sync();
        await h.discovery.start();
        await h.flush();
        const recovered = h.discovery.players[0];
        assert.ok(recovered);
        assert.deepEqual(native.capture().players.map(player => player.bus), [bus]);
        assert.ok(recovered.handlers.size > 0);
        h.standardOwners[standardBus] = bus;
        h.ownerChanged(standardBus, '', bus);
        await h.flush();
        assert.equal(h.discovery.players[0], recovered, 'a well-known alias alone is not an Astal replacement');
        const standard = new Player({ bus_name: standardBus, playback_status: playbackStatus.PAUSED });
        h.standardPlayers = h.manager.players = [standard];
        h.manager.emit('notify::players');
        await h.flush();
        assert.deepEqual(h.discovery.players, []);
        assert.deepEqual(native.capture().players.map(player => player.bus), [standardBus]);
        assert.equal(native.player(bus), null);
        assert.equal(native.players.get(standardBus).object, standard);
        assert.equal(recovered.available, false);
        assert.equal(recovered.handlers.size, 0);
        assert.ok([...h.subscriptions.values()].every(signal => signal.iface === daemonInterface),
            'all direct MPRIS subscriptions must be removed at takeover');
        assert.equal(h.created.length, 1);
        assert.equal(h.timers().length, 0);
        assert.equal(standard.available, true);
        assert.deepEqual(standard.calls, [], 'deduplication must not dispatch playback controls');
    });

    test('review race: standard Astal takeover during a probe prevents the late adapter from being published', { timeout: 3000 }, async t => {
        const bus = ':1.1302';
        const standardBus = 'org.mpris.MediaPlayer2.audio_lab';
        const h = discoveryHarness({ owners: { [bus]: { pid: 13020, application: 'audio-lab' } }, gated: true,
            streams: [new Stream({ properties: { 'application.name': 'audio-lab' } })] });
        t.after(() => { h.discovery.dispose(); for (const resolve of h.gates.values()) resolve(); });
        const starting = Promise.resolve(h.discovery.start()).then(() => null, error => error);
        await h.flush();
        assert.equal(h.gates.size, 1);
        const standard = new Player({ bus_name: standardBus });
        h.standardOwners[standardBus] = bus;
        h.standardPlayers = [standard];
        h.ownerChanged(standardBus, '', bus);
        for (const resolve of h.gates.values()) resolve();
        h.gates.clear();
        await starting;
        await h.flush();
        assert.deepEqual(h.buses(), []);
        assert.equal(h.created.length, 1);
        assert.equal(h.created[0].disposeCalls, 1);
        assert.equal(standard.available, true);
        assert.deepEqual(standard.calls, []);
    });

    test('candidate MPRIS probes are bounded to four concurrent starts', { timeout: 3000 }, async t => {
        const owners = Object.fromEntries(Array.from({ length: 9 }, (_, index) =>
            [`:1.${900 + index}`, { pid: 9000 + index, application: 'audio-lab' }]));
        const h = discoveryHarness({ owners, gated: true,
            streams: [new Stream({ properties: { 'application.process.binary': 'audio-lab' } })] });
        t.after(() => { h.discovery.dispose(); for (const resolve of h.gates.values()) resolve(); });
        const starting = h.discovery.start();
        await h.flush();
        assert.equal(h.gates.size, 4);
        const [bus, release] = h.gates.entries().next().value;
        h.gates.delete(bus);
        release();
        await h.flush();
        assert.equal(h.starts.length, 5);
        assert.equal(h.gates.size, 4, 'finishing one probe permits exactly one replacement');
        for (let batch = 0; batch < 3; batch++) {
            assert.ok(h.gates.size <= 4);
            const gates = [...h.gates.values()];
            h.gates.clear();
            gates.forEach(resolve => resolve());
            await h.flush();
        }
        await starting;
        assert.equal(h.starts.length, 9);
        assert.deepEqual(h.buses(), Object.keys(owners).sort());
    });

    test('dispose cancels a credential query and ignores its already queued success', { timeout: 3000 }, async t => {
        const h = discoveryHarness({ owners: { ':1.1001': { pid: 10010, application: 'audio-lab' } },
            streams: [new Stream({ properties: { 'application.name': 'audio-lab' } })] });
        t.after(() => h.discovery.dispose());
        const respond = h.respond;
        h.respond = call => call.method === 'GetConnectionUnixProcessID' ? undefined : respond(call);
        const starting = Promise.resolve(h.discovery.start()).then(() => null, error => error);
        await h.flush();
        const pending = [...h.pending].find(call => call.method === 'GetConnectionUnixProcessID');
        assert.ok(pending);
        pending.resolve(dbusVariant('(u)', [10010]));
        h.discovery.dispose();
        const changes = h.changes;
        await starting;
        await h.flush();
        assert.equal(pending.cancellable.is_cancelled(), true);
        assert.deepEqual(h.created, []);
        assert.deepEqual(h.buses(), []);
        assert.equal(h.subscriptions.size, 0);
        assert.equal(h.sources.size, 0);
        assert.equal(h.changes, changes);
    });

    test('a probe completing after disposal cannot publish or retain a recovered player', { timeout: 3000 }, async t => {
        const h = discoveryHarness({ owners: { ':1.1101': { pid: 11010, application: 'audio-lab' } }, gated: true,
            streams: [new Stream({ properties: { 'application.name': 'audio-lab' } })] });
        t.after(() => { h.discovery.dispose(); for (const resolve of h.gates.values()) resolve(); });
        const starting = Promise.resolve(h.discovery.start()).then(() => null, error => error);
        await h.flush();
        assert.equal(h.gates.size, 1);
        h.discovery.dispose();
        const changes = h.changes;
        for (const resolve of h.gates.values()) resolve();
        h.gates.clear();
        await starting;
        await h.flush();
        assert.deepEqual(h.buses(), []);
        assert.ok(h.created[0].disposeCalls >= 1);
        assert.equal(h.changes, changes);
        assert.equal(h.subscriptions.size, 0);
        assert.equal(h.sources.size, 0);
    });
});

function nativeHarness({ player = new Player(), stream = null, isObjectPath = variantDouble.is_object_path } = {}) {
    const manager = Object.assign(new Signals(), { players: [player] });
    const audio = Object.assign(new Signals(), { streams: stream ? [stream] : [] });
    const sources = new Map();
    let nextSource = 0;
    const h = { time: 0, changes: 0, modelChanges: [], player, stream, manager, audio, sources, positionQueries: [] };
    const GLib = {
        PRIORITY_DEFAULT_IDLE: 0, PRIORITY_DEFAULT: 0, SOURCE_REMOVE: false,
        get_monotonic_time: () => h.time * 1e6,
        getenv: () => null,
        idle_add: (_, callback) => { const id = ++nextSource; sources.set(id, callback); return id; },
        source_remove: id => sources.delete(id),
        Variant: { is_object_path: isObjectPath },
    };
    const Gio = { Cancellable };
    const api = loadBackend({
        GLib, Gio,
        queryPosition: (bus, cancel) => new Promise((resolve, reject) => h.positionQueries.push({ bus, cancel, resolve, reject })),
        Mpris: { get_default: () => manager, PlaybackStatus: { PLAYING: 1, PAUSED: 2, STOPPED: 3 } },
        Wp: { get_default: () => ({ audio }) },
    });
    const native = new api.NativeMedia({ onChange: modelChanged => {
        h.changes++;
        h.modelChanges.push(modelChanged);
    } });
    return Object.assign(h, api, {
        native,
        stamp: () => api.ownershipStamp(native.capture()),
        group: () => ({ primary: native.player(player.bus_name), streamId: stream?.id ?? null }),
        flush: () => {
            for (const [id, callback] of [...sources]) {
                sources.delete(id);
                callback();
            }
        },
    });
}

describe('Native service integration', () => {
    const seek = (h, value = 80) => h.native.apply({ player: h.player.bus_name, operation: 'seek', value });
    const volume = h => h.native.volume(h.group(), h.stamp());

    test('volume and cover notifications update the view without invalidating grouping', () => {
        const h = nativeHarness();
        h.flush();
        assert.deepEqual(h.modelChanges, [true]);
        for (const [property, value] of [['volume', 0.8], ['cover-art', '/cache/cover.png']]) {
            h.player.publish(property, value);
            h.flush();
        }
        assert.deepEqual(h.modelChanges, [true, false, false]);
        h.player.publish('position', 11);
        h.flush();
        assert.equal(h.modelChanges.at(-1), true, 'Rust grouping can depend on relative playback positions');
        assert.equal(h.native.player(h.player.bus_name).position, 11);
        assert.equal(h.native.player(h.player.bus_name).cover, '/cache/cover.png');
        h.player.publish('title', 'New title');
        h.flush();
        assert.equal(h.modelChanges.at(-1), true);
    });

    test('coalescing a view notification never drops a model-changing notification', () => {
        for (const first of ['volume', 'title']) {
            const h = nativeHarness();
            h.flush();
            h.player.publish(first, first === 'volume' ? 0.8 : 'Changed');
            h.player.publish(first === 'volume' ? 'title' : 'volume', first === 'volume' ? 'Changed' : 0.8);
            assert.equal(h.sources.size, 1);
            h.flush();
            assert.deepEqual(h.modelChanges, [true, true]);
        }
    });

    test('stream readiness and object-list notifications still invalidate the model', () => {
        const h = nativeHarness({ stream: new Stream({ _volume: -1 }) });
        h.flush();
        h.stream.publish('volume', 0.5);
        h.flush();
        h.manager.emit('notify::players');
        h.audio.emit('notify::streams');
        h.flush();
        assert.deepEqual(h.modelChanges, [true, true, true]);
        assert.equal(h.native.capture().streams[0].ready, true);
    });

    test('the public backend refreshes grouping only for model-changing native notifications', () => {
        const h = nativeHarness();
        let refreshes = 0, changes = 0;
        const backend = new h.MediaBackend({ onChange: () => changes++ });
        backend.planner.refresh = () => { refreshes++; return true; };
        h.flush();
        refreshes = changes = 0;
        h.player.publish('position', 11);
        h.flush();
        h.player.publish('volume', 0.7);
        h.flush();
        assert.equal(refreshes, 1, 'position changes still reconcile position-dependent grouping');
        assert.equal(changes, 2, 'visible views still receive native progress/volume changes');
        h.player.publish('title', 'New track');
        h.flush();
        assert.equal(refreshes, 2);
        backend.dispose();
    });

    test('repeated status requests read current metadata without refreshing the Rust model', () => {
        const h = nativeHarness();
        const group = h.group();
        let captures = 0;
        const capture = h.native.capture.bind(h.native);
        h.native.capture = () => { captures++; return capture(); };
        const backend = Object.assign(Object.create(h.MediaBackend.prototype), {
            activePlayer: h.player.bus_name, resolveGroups: () => [group], native: h.native, modelStamp: h.stamp(),
            planner: { ready: true, refresh() { assert.fail('Unchanged status must not schedule model work'); } },
        });
        captures = 0;
        for (let index = 0; index < 100; index++)
            assert.deepEqual(JSON.parse(backend.status()), badge(group));
        assert.equal(captures, 1, 'three monitor badges share one bounded ownership check');
        h.time = 5;
        backend.status();
        assert.equal(captures, 2);
        group.primary.title = 'Current title';
        assert.equal(JSON.parse(backend.status()).tooltip, 'Current title — Artist');
    });

    test('status reconciliation recovers silent PipeWire changes without a polling timer', () => {
        const h = nativeHarness({ player: new Player({ _volume: -1 }), stream: new Stream() });
        let refreshes = 0;
        const backend = Object.assign(Object.create(h.MediaBackend.prototype), {
            activePlayer: h.player.bus_name, resolveGroups: () => [h.group()], native: h.native, modelStamp: h.stamp(),
            planner: { ready: true, refresh() { refreshes++; return true; } },
        });
        const descriptor = h.native.volume(h.group(), h.stamp());
        backend.status();
        assert.equal(refreshes, 0);
        h.stream.properties['application.id'] = 'a-replaced-owner';
        assert.equal(descriptor.set(0.1), false, 'old ownership is rejected immediately, before reconciliation');
        h.time = 1;
        backend.status();
        assert.equal(refreshes, 0);
        h.time = 5;
        backend.status();
        assert.equal(refreshes, 1, 'silent property changes eventually trigger a fresh model');
        backend.modelStamp = h.stamp();
        h.time = 10;
        backend.status();
        assert.equal(refreshes, 1, 'reconciled state does not repeatedly refresh');
    });

    test('position refresh uses a bounded cancellable D-Bus read and accepts only numeric microseconds', async () => {
        let value;
        const cancel = new Cancellable();
        const api = loadBackend({
            GLib: {
                Variant: class { constructor(signature, values) { Object.assign(this, { signature, values }); } },
                VariantType: class { constructor(signature) { this.signature = signature; } },
            },
            Gio: { DBusCallFlags: { NONE: 0 }, DBus: { session: {
                call(bus, path, iface, method, params, replyType, flags, timeout, cancellable, callback) {
                    assert.equal(bus, 'org.mpris.MediaPlayer2.test');
                    assert.equal(path, '/org/mpris/MediaPlayer2');
                    assert.equal(iface, 'org.freedesktop.DBus.Properties');
                    assert.equal(method, 'Get');
                    assert.equal(params.signature, '(ss)');
                    assert.deepEqual(params.values, ['org.mpris.MediaPlayer2.Player', 'Position']);
                    assert.equal(replyType.signature, '(v)');
                    assert.equal(flags, 0);
                    assert.equal(timeout, 1500);
                    assert.equal(cancellable, cancel);
                    queueMicrotask(() => callback({ call_finish() {
                        if (value instanceof Error) throw value;
                        return { deep_unpack: () => [{ deep_unpack: () => value }] };
                    } }, {}));
                },
            } } },
        });
        for (value of [0, 1000000, 1500000.5])
            assert.equal(await api.readPosition('org.mpris.MediaPlayer2.test', cancel), value / 1e6);
        for (value of [-1, NaN, Infinity, null, undefined, '1000000', true, {}, []])
            await assert.rejects(api.readPosition('org.mpris.MediaPlayer2.test', cancel), /Unsupported/);
        value = new Error('The player disappeared');
        await assert.rejects(api.readPosition('org.mpris.MediaPlayer2.test', cancel), /disappeared/);
    });

    test('worker UTF-8 decoding stays strict and independent across failed and successful replies', () => {
        const { ModelWorker } = loadBackend({ GLib: {
            PRIORITY_DEFAULT: 0, SOURCE_REMOVE: false,
            timeout_add: () => 1, source_remove() {},
        } });
        const responses = [], failures = [];
        const reply = bytes => {
            let callback;
            const worker = new ModelWorker('unused', {
                onReady() {}, onResponse: value => responses.push(value), onFailure: error => failures.push(error),
            });
            worker.pending = 1;
            worker.buffer = new Uint8Array();
            worker.output = {
                read_bytes_async(_size, _priority, _cancel, next) { callback = next; },
                read_bytes_finish: bytes => ({ toArray: () => bytes }),
            };
            worker.read(worker.generation);
            callback(worker.output, bytes);
            worker.dispose();
        };
        reply(new Uint8Array([0xff, 10]));
        assert.equal(responses.length, 0);
        assert.match(failures[0], /protocol failed/);
        const response = { requestId: 1, title: '作者 🎵' };
        const bytes = new TextEncoder().encode(JSON.stringify(response) + '\n');
        reply(bytes);
        reply(bytes);
        assert.deepEqual(responses, [response, response]);
        assert.equal(failures.length, 1, 'a failed decode must not poison later workers');
    });

    test('capture reuses its native player snapshot while deriving model progress', () => {
        const h = nativeHarness();
        const player = h.native.player.bind(h.native);
        let reads = 0;
        h.native.player = bus => { reads++; return player(bus); };
        const snapshot = h.native.capture().players[0];
        assert.equal(reads, 1, 'one native snapshot per captured player');
        assert.equal(snapshot.position, 10);
        assert.equal(snapshot.modelPosition, 10);
        assert.equal(snapshot.title, h.player.title);
        h.native.progress(h.player.bus_name);
        assert.equal(reads, 2, 'standalone progress still obtains its own fresh native snapshot');
    });

    test('display progress interpolates from real samples without changing native snapshots or ownership', () => {
        const h = nativeHarness();
        const stamp = h.stamp();
        h.time = 1.5;
        assert.equal(h.native.progress(h.player.bus_name).position, 11.5);
        assert.equal(h.native.progress(h.player.bus_name).samplePosition, 10);
        assert.equal(h.native.player(h.player.bus_name).position, 10);
        assert.equal(h.native.capture().players[0].position, 10);
        assert.equal(h.stamp(), stamp);
        h.player.publish('position', 20);
        h.time = 2;
        assert.equal(h.native.progress(h.player.bus_name).position, 20.5);
        h.player.publish('position', 5);
        h.time = 3;
        assert.equal(h.native.progress(h.player.bus_name).position, 6, 'backward seeks reanchor immediately');
        h.player.length = 5.5;
        assert.equal(h.native.progress(h.player.bus_name).position, 5.5);
        assert.equal(h.native.player(h.player.bus_name).position, 5);
    });

    test('display progress freezes on pause, resumes without counting paused time, and follows rate changes', () => {
        const h = nativeHarness();
        const progress = () => h.native.progress(h.player.bus_name);
        h.time = 1;
        h.player.publish('rate', 2);
        h.time = 2;
        assert.equal(progress().position, 13);
        h.player.publish('playback-status', 2);
        h.time = 40;
        assert.equal(progress().position, 13);
        h.player.publish('playback-status', 1);
        h.time = 41;
        assert.equal(progress().position, 15);
        h.player.publish('playback-status', 3);
        assert.equal(progress().positionSupported, false);
    });

    test('playing placeholder zeros never start a synthetic clock, regardless of application or seek support', () => {
        for (const bus_name of ['org.mpris.MediaPlayer2.test', 'org.mpris.MediaPlayer2.future.tab']) {
            for (const can_seek of [false, true]) {
                const h = nativeHarness({ player: new Player({ bus_name, can_seek, _position: 0 }) });
                const progress = () => h.native.progress(bus_name);
                h.time = 2;
                h.player.publish('position', 0);
                assert.equal(progress().positionSupported, false);
                assert.equal(progress().position, 0);
                assert.equal(progress().canSeek, can_seek, 'do not rewrite advertised capabilities');
                h.time = 10;
                assert.equal(progress().positionSupported, false);
                h.player.publish('position', 25);
                assert.equal(progress().position, 25);
                assert.equal(progress().positionSupported, true);
                h.time = 11;
                h.player.publish('position', 0);
                assert.equal(progress().positionSupported, false, 'loss of timing must not restart at zero');
            }
        }
    });

    test('expired nonzero samples become unknown only for matching, and quality changes refresh grouping once', () => {
        const h = nativeHarness({ player: new Player({ _position: 0.208 }) });
        const initial = h.native.capture().players[0];
        assert.equal(initial.position, 0.208);
        assert.equal(initial.modelPosition, 0.208);
        const stamp = h.stamp();
        h.flush();
        const changes = h.modelChanges.length;
        h.time = 6;
        const stale = h.native.capture().players[0];
        assert.equal(stale.position, 0.208, 'never overwrite native state with a model sentinel');
        assert.equal(stale.positionSupported, true, 'native syntactic support is unchanged');
        assert.equal(stale.modelPosition, 0, 'a frozen sample must not veto a matching live publisher');
        assert.equal(h.native.player(h.player.bus_name).position, 0.208);
        assert.equal(h.stamp(), stamp, 'sample quality is not an ownership change');
        h.flush();
        assert.equal(h.modelChanges.length, changes + 1);
        assert.equal(h.modelChanges.at(-1), true, 'expiry requests a new grouping decision');
        h.native.capture();
        h.flush();
        assert.equal(h.modelChanges.length, changes + 1, 'do not repeatedly refresh an already stale source');
        h.player.publish('position', 12);
        assert.equal(h.native.capture().players[0].modelPosition, 12, 'fresh real disagreement remains available to the matcher');
    });

    test('confirmed paused reads refresh grouping even when position quality stays supported', async () => {
        const h = nativeHarness({ player: new Player({ playback_status: 2, _position: 100 }) });
        assert.equal(h.native.capture().players[0].modelPosition, 100);
        h.flush();
        const changes = h.modelChanges.length;
        h.positionQueries[0].resolve(20);
        await new Promise(resolve => setImmediate(resolve));
        h.flush();
        assert.equal(h.modelChanges.length, changes + 1);
        assert.equal(h.modelChanges.at(-1), true, 'a corrected sample affects matching even without a quality transition');
        const player = h.native.capture().players[0];
        assert.equal(player.position, 100);
        assert.equal(player.modelPosition, 20);
        assert.equal(h.native.progress(h.player.bus_name).positionSupported, true);
    });

    test('model positions use confirmed reads rather than old native or interpolated display positions', async () => {
        const h = nativeHarness({ player: new Player({ playback_status: 2, _position: 100 }) });
        h.player.publish('trackid', '/track/two');
        h.positionQueries.at(-1).resolve(20);
        await new Promise(resolve => setImmediate(resolve));
        h.player.publish('playback-status', 1);
        h.time = 2;
        const snapshot = h.native.capture().players[0];
        assert.equal(snapshot.position, 100);
        assert.equal(snapshot.modelPosition, 20);
        assert.equal(h.native.progress(h.player.bus_name).position, 22);
    });

    test('display estimates expire without fresh samples and unrelated notifications cannot renew them', () => {
        const h = nativeHarness();
        const progress = () => h.native.progress(h.player.bus_name);
        h.time = 5;
        assert.equal(progress().positionSupported, true);
        h.time = 5.01;
        h.player.publish('cover-art', '/cover.png');
        h.player.publish('volume', 0.5);
        h.player.emit('notify', { name: 'metadata' });
        assert.equal(progress().positionSupported, false);
        h.player.publish('playback-status', 2);
        assert.equal(progress().positionSupported, false, 'pausing must not revive an expired sample');
        h.player.publish('position', 15);
        assert.equal(progress().positionSupported, true);
        h.time = 100;
        assert.equal(progress().position, 15, 'a confirmed paused position does not expire');
    });

    test('seek dispatch invalidates display estimates until a real position update arrives', () => {
        const h = nativeHarness();
        h.time = 1;
        assert.equal(h.native.progress(h.player.bus_name).position, 11);
        seek(h, 80);
        assert.equal(h.native.progress(h.player.bus_name).positionSupported, false);
        assert.equal(h.native.player(h.player.bus_name).position, 10, 'dispatch is not an acknowledgement');
        h.player.publish('position', 79.5);
        assert.equal(h.native.progress(h.player.bus_name).position, 79.5);
    });

    test('track changes do not inherit cached progress and paused tracks recover through an explicit read', async () => {
        const h = nativeHarness({ player: new Player({ playback_status: 2 }) });
        h.player.publish('position', 30);
        h.player.title = 'Another track';
        h.player.metadata['xesam:url'] = 'file:///music/another.flac';
        h.player.emit('notify', { name: 'metadata' });
        assert.equal(h.native.progress(h.player.bus_name).positionSupported, false);
        assert.equal(h.positionQueries[0].cancel.is_cancelled(), true);
        h.positionQueries.at(-1).resolve(30);
        await new Promise(resolve => setImmediate(resolve));
        assert.equal(h.native.progress(h.player.bus_name).position, 30);
        assert.equal(h.native.progress(h.player.bus_name).positionSupported, true);
        h.time = 100;
        assert.equal(h.native.progress(h.player.bus_name).position, 30);
        h.player.publish('playback-status', 1);
        h.time = 101;
        assert.equal(h.native.progress(h.player.bus_name).position, 31);
    });

    test('pause and rate transitions renew pending recovery reads instead of stranding paused progress', async () => {
        const h = nativeHarness();
        h.player.publish('trackid', '/track/two');
        const playingRead = h.positionQueries.at(-1);
        h.player.publish('playback-status', 2);
        const pausedRead = h.positionQueries.at(-1);
        assert.notEqual(pausedRead, playingRead);
        assert.equal(playingRead.cancel.is_cancelled(), true);
        h.player.publish('rate', 2);
        const rateRead = h.positionQueries.at(-1);
        assert.notEqual(rateRead, pausedRead);
        assert.equal(pausedRead.cancel.is_cancelled(), true);
        playingRead.resolve(99);
        pausedRead.resolve(99);
        rateRead.resolve(20);
        await new Promise(resolve => setImmediate(resolve));
        h.time = 60;
        const progress = h.native.progress(h.player.bus_name);
        assert.equal(progress.positionSupported, true);
        assert.equal(progress.position, 20);
        assert.equal(progress.samplePosition, 20);
        assert.equal(h.native.player(h.player.bus_name).position, 10, 'raw native state remains untouched');
    });

    test('late position reads cannot overwrite newer samples, tracks, or replacement players', async () => {
        const h = nativeHarness();
        const initial = h.positionQueries[0];
        h.player.publish('position', 20);
        initial.resolve(99);
        await new Promise(resolve => setImmediate(resolve));
        assert.equal(h.native.progress(h.player.bus_name).position, 20);
        h.player.publish('trackid', '/track/two');
        const changed = h.positionQueries.at(-1);
        h.manager.players = [new Player({ _position: 40 })];
        h.manager.emit('notify::players');
        changed.resolve(99);
        await new Promise(resolve => setImmediate(resolve));
        assert.equal(changed.cancel.is_cancelled(), true);
        assert.equal(h.native.progress(h.player.bus_name).position, 40);
        const replacement = h.positionQueries.at(-1);
        h.native.dispose();
        replacement.resolve(99);
        await new Promise(resolve => setImmediate(resolve));
        assert.equal(replacement.cancel.is_cancelled(), true);
        assert.equal(h.native.progress(h.player.bus_name), null);
    });

    test('explicit position reads can supply display timing without fabricating native position support', async () => {
        const h = nativeHarness({ player: new Player({ _position: -1 }) });
        h.positionQueries[0].resolve(25.5);
        await new Promise(resolve => setImmediate(resolve));
        h.time = 1;
        assert.equal(h.native.progress(h.player.bus_name).position, 26.5);
        assert.equal(h.native.progress(h.player.bus_name).samplePosition, 25.5);
        assert.equal(h.native.player(h.player.bus_name).positionSupported, false);
        h.player.publish('playback-status', 2);
        h.time = 100;
        assert.equal(h.native.progress(h.player.bus_name).position, 26.5);
    });

    test('zero and invalid native positions stay unavailable while playing, but paused zero is valid', () => {
        for (const position of [-1, NaN, Infinity, null, undefined, 0]) {
            const h = nativeHarness({ player: new Player({ _position: position }) });
            assert.equal(h.native.progress(h.player.bus_name).positionSupported, false);
        }
        const h = nativeHarness({ player: new Player({ _position: 0, playback_status: 2 }) });
        assert.equal(h.native.progress(h.player.bus_name).positionSupported, true);
        assert.equal(h.native.progress(h.player.bus_name).position, 0);
        h.player.publish('playback-status', 1);
        assert.equal(h.native.progress(h.player.bus_name).positionSupported, false);
    });

    test('position comes only from Astal: no elapsed-time, rate, or length projection', () => {
        const h = nativeHarness();
        h.time = 100;
        h.player.rate = 2;
        assert.equal(h.native.player(h.player.bus_name).position, 10);
        h.player.publish('position', 250);
        assert.equal(h.native.player(h.player.bus_name).position, 250, 'do not clamp a native sample to length');
        h.time = 200;
        assert.equal(h.native.capture().players[0].position, 250);
    });

    test('numeric microseconds supply duration when the typed native length is unavailable', () => {
        const h = nativeHarness({ player: new Player({
            length: -1, metadata: { 'mpris:length': 361929437.5 },
        }) });
        const sample = () => h.native.player(h.player.bus_name);
        assert.equal(sample().length, 361.9294375);
        assert.equal(sample().canSeek, true);
        assert.equal(sample().position, 10);
        h.player.length = 200;
        assert.equal(sample().length, 200, 'valid native seconds take precedence over raw metadata');
        for (const length of [-1, 0, NaN, Infinity, null, undefined]) {
            h.player.length = length;
            assert.equal(sample().length, 361.9294375);
        }
        h.player.metadata['mpris:length'] = 200000000;
        assert.equal(sample().length, 200, 'integer microseconds work too');
        for (const raw of [undefined, null, 0, -1, NaN, Infinity, -Infinity, '200000000', true, [], {}]) {
            h.player.metadata['mpris:length'] = raw;
            assert.equal(sample().length, null, `invalid raw duration ${String(raw)}`);
        }
        delete h.player.metadata['mpris:length'];
        assert.equal(sample().length, null);
    });

    test('duration recovery is independent of player identity and preserves advertised capabilities', () => {
        for (const [bus_name, identity, entry] of [
            ['org.mpris.MediaPlayer2.test', 'Test Player', 'test-player'],
            ['org.mpris.MediaPlayer2.future_player.instance42', 'An Unknown Future Player', 'future-player'],
            ['org.mpris.MediaPlayer2.browser.tab17', '', ''],
        ]) {
            for (const can_seek of [false, true]) {
                const h = nativeHarness({ player: new Player({
                    bus_name, identity, entry, can_seek, length: -1,
                    metadata: { 'mpris:length': 361929437.5 },
                }) });
                const recovered = h.native.player(bus_name);
                assert.equal(recovered.length, 361.9294375, bus_name);
                assert.equal(recovered.canSeek, can_seek, 'duration recovery must not invent seek support');
                h.player.metadata = {};
                assert.equal(h.native.player(bus_name).length, null, 'missing data stays unknown for every player');
                h.player.length = 120;
                assert.equal(h.native.player(bus_name).length, 120, 'standard native duration remains unchanged');
            }
        }
    });

    test('unsupported native positions normalize to zero without disabling native can_seek', () => {
        const h = nativeHarness();
        for (const position of [-1, NaN, Infinity, null, undefined]) {
            h.player.publish('position', position);
            const player = h.native.capture().players[0];
            assert.equal(player.positionSupported, false);
            assert.equal(player.position, 0);
            assert.equal(player.canSeek, true);
        }
        h.player.publish('position', 0);
        assert.equal(h.native.capture().players[0].positionSupported, true);
        h.player.publish('can-seek', false);
        assert.equal(h.native.capture().players[0].canSeek, false);
    });

    test('metadata changes preserve native position/support and prefer a valid native track ID', () => {
        const h = nativeHarness({ player: new Player({ playback_status: 2 }) });
        const before = h.native.capture().players[0];
        h.player.title = 'Corrected title';
        h.player.artist = 'Corrected artist';
        h.player.metadata['xesam:url'] = 'file:///music/corrected.flac';
        h.player.emit('notify', { name: 'metadata' });
        const after = h.native.capture().players[0];
        assert.equal(after.trackKey, '/track/one');
        assert.equal(after.trackKey, before.trackKey);
        assert.equal(after.positionSupported, true);
        assert.equal(after.canSeek, true);
        assert.equal(after.position, 10);
        assert.equal(after.seekRevision, 0);
        h.player.trackid = '/track/two';
        h.player.emit('notify', { name: 'metadata' });
        assert.equal(h.native.capture().players[0].trackKey, '/track/two');
        assert.equal(h.native.capture().players[0].positionSupported, true);
    });

    test('invalid/missing/NoTrack IDs use metadata only as a freshness key', () => {
        for (const trackid of ['', 'not/a/path', '/invalid-dash', '/trailing/', '/org/mpris/MediaPlayer2/TrackList/NoTrack']) {
            const h = nativeHarness({ player: new Player({ trackid }) });
            const before = h.native.capture().players[0];
            h.player.title = 'Different metadata';
            h.player.emit('notify', { name: 'metadata' });
            const after = h.native.capture().players[0];
            assert.notEqual(after.trackKey, before.trackKey);
            assert.equal(after.positionSupported, true);
            assert.equal(after.canSeek, true, 'the advertised native capability is not rewritten');
            assert.throws(() => seek(h), /unavailable/);
            assert.deepEqual(h.player.calls, []);
        }
    });

    test('track identity and seek preflight defer syntax validation to GLib.Variant.is_object_path', () => {
        const checked = [];
        const h = nativeHarness({
            isObjectPath: value => { checked.push(value); return false; },
        });
        const player = h.native.capture().players[0];
        assert.notEqual(player.trackKey, h.player.trackid, 'use metadata when native syntax validation rejects the ID');
        assert.throws(() => seek(h), /unavailable/);
        assert.deepEqual(checked, ['/track/one', '/track/one', '/track/one', '/track/one']);
        assert.deepEqual(h.player.calls, []);
    });

    test('paused seeks only dispatch and advance a dispatch revision; native state stays unchanged', () => {
        for (const position of [-1, 10]) {
            const h = nativeHarness({ player: new Player({ playback_status: 2, _position: position }) });
            const before = h.native.capture().players[0];
            seek(h);
            assert.deepEqual(h.player.calls, [['position', 80]]);
            h.time = 100;
            let after = h.native.capture().players[0];
            assert.equal(after.seekRevision, 1);
            assert.equal(after.position, before.position);
            assert.equal(after.positionSupported, before.positionSupported);
            h.player.publish('position', 20);
            after = h.native.capture().players[0];
            assert.equal(after.position, 20, 'never suppress a native sample as a pre-seek trajectory');
            assert.equal(after.seekRevision, 1, 'native samples are not synthetic dispatches or acknowledgements');
        }
    });

    test('seek preflight and synchronous dispatch errors do not advance revisions', () => {
        const h = nativeHarness();
        for (const value of [-1, NaN, Infinity]) assert.throws(() => seek(h, value), /finite and nonnegative/);
        h.player.can_seek = false;
        assert.throws(() => seek(h), /unavailable/);
        h.player.can_seek = true;
        h.player.seekError = 'native dispatch failed';
        assert.throws(() => seek(h), /native dispatch failed/);
        assert.equal(h.native.capture().players[0].seekRevision, 0);
        h.player.available = false;
        assert.throws(() => seek(h), /disappeared/);
    });

    test('native notifications are coalesced and replacement/disposal disconnect old objects', () => {
        const h = nativeHarness({ stream: new Stream() });
        h.flush();
        assert.equal(h.changes, 1);
        h.player.publish('position', 11);
        h.player.publish('position', 12);
        h.flush();
        assert.equal(h.changes, 2);
        const oldPlayer = h.player;
        const oldInstance = h.native.capture().players[0].instance;
        h.manager.players = [new Player()];
        h.manager.emit('notify::players');
        assert.equal(oldPlayer.handlers.size, 0);
        assert.notEqual(h.native.capture().players[0].instance, oldInstance);
        h.native.dispose();
        assert.equal(h.manager.handlers.size, 0);
        assert.equal(h.audio.handlers.size, 0);
        assert.equal(h.manager.players[0].handlers.size, 0);
        assert.equal(h.stream.handlers.size, 0);
        assert.equal(h.sources.size, 0);
    });

    test('stream snapshots use the exact native media.name property, not the raw getter', () => {
        const stream = new Stream();
        stream.properties['media.name'] = 'stale raw name';
        const h = nativeHarness({ stream });
        assert.equal(h.native.capture().streams[0].properties['media.name'], stream.name);
        assert.equal(stream.propertyReads.includes('media.name'), false);
        assert.equal(stream.propertyReads.includes('application.name'), true);
    });

    test('advertised artwork follows native cover_art without deriving or checking cache paths', () => {
        for (const art_url of ['file:///art/current.png', 'https://example.test/art.png', '', 'not a URI']) {
            const h = nativeHarness({ player: new Player({ art_url, cover_art: '/native/arbitrary-cover-path' }) });
            assert.equal(h.native.capture().players[0].cover, h.player.cover_art);
            h.player.publish('cover-art', '/native/updated-cover');
            assert.equal(h.native.capture().players[0].cover, '/native/updated-cover');
            h.player.publish('cover-art', '');
            assert.equal(h.native.capture().players[0].cover, '');
        }
    });

    test('web bridge placeholder volume uses only a uniquely owned native stream', () => {
        const player = new Player({ bus_name: 'org.mpris.MediaPlayer2.mprisence_web.generic.ptest', _volume: 1 });
        const h = nativeHarness({ player, stream: new Stream() });
        const control = volume(h);
        assert.equal(control.enabled, true);
        assert.equal(control.value, 0.7);
        assert.equal(control.set(0.3), true);
        assert.deepEqual(h.player.calls, []);
        assert.deepEqual(h.stream.calls, [['volume', 0.3], ['mute', false]]);
        const shared = h.native.volume({ ...h.group(), streamId: null }, h.stamp());
        assert.equal(shared.enabled, false);
        assert.equal(shared.set(0.2), false);
        assert.match(shared.reason, /does not support tab volume/);
    });

    test('WirePlumber mute uses its native flag and retains volume even while muted', () => {
        const h = nativeHarness({ player: new Player({ _volume: -1 }), stream: new Stream() });
        let descriptor = volume(h);
        assert.equal(descriptor.value, 0.7);
        assert.equal(descriptor.muteEnabled, true);
        assert.equal(descriptor.muted, false);
        assert.equal(descriptor.toggleMute(), true);
        assert.deepEqual(h.stream.calls, [['mute', true]]);
        assert.equal(h.stream.volume, 0.7);
        assert.equal(h.stream.mute, false, 'a dispatched setter is not remote completion');
        h.stream.publish('mute', true);
        descriptor = volume(h);
        assert.equal(descriptor.value, 0.7, 'muted volume remains available to the slider');
        assert.equal(descriptor.muted, true);
        assert.equal(descriptor.setMuted(false), true);
        assert.deepEqual(h.stream.calls.at(-1), ['mute', false]);
        assert.equal(h.stream.calls.some(([name]) => name === 'volume'), false);
        assert.equal(descriptor.setMuted('false'), false);
    });

    test('MPRIS slider volume and owned-stream native mute are independent', () => {
        const h = nativeHarness({ stream: new Stream() });
        let descriptor = volume(h);
        assert.equal(descriptor.enabled, true);
        assert.equal(descriptor.value, 0.6);
        assert.match(descriptor.key, /^player:/);
        assert.equal(descriptor.muteEnabled, true);
        assert.equal(descriptor.muted, false);
        assert.equal(descriptor.toggleMute(), true);
        assert.deepEqual(h.stream.calls, [['mute', true]]);
        assert.deepEqual(h.player.calls, []);
        assert.equal(h.stream.volume, 0.7);
        assert.equal(h.stream.mute, false, 'dispatch does not fabricate native mute completion');
        h.stream.publish('mute', true);
        descriptor = volume(h);
        assert.equal(descriptor.value, 0.6);
        assert.equal(descriptor.muted, true);
        assert.equal(descriptor.setMuted(false), true);
        assert.deepEqual(h.stream.calls.at(-1), ['mute', false]);
        h.stream.publish('mute', false);
        h.player.publish('volume', 0);
        descriptor = volume(h);
        assert.equal(descriptor.value, 0);
        assert.equal(descriptor.muted, false, 'zero MPRIS volume is not a synthetic mute flag');
        assert.equal(descriptor.set(0.4), true);
        assert.deepEqual(h.player.calls, [['volume', 0.4]]);
        assert.deepEqual(h.stream.calls, [['mute', true], ['mute', false]]);
    });

    test('MPRIS volume remains enabled without an owned stream, but mute never zeroes or restores volume', () => {
        for (const level of [0, 0.6]) {
            const h = nativeHarness({ player: new Player({ _volume: level }) });
            const group = { ...h.group(), streamReason: 'Shared or ambiguous audio stream.' };
            const descriptor = h.native.volume(group, h.stamp());
            assert.equal(descriptor.enabled, true);
            assert.equal(descriptor.value, level);
            assert.equal(descriptor.muteEnabled, false);
            assert.equal(descriptor.muted, false);
            assert.match(descriptor.muteReason, /Native mute requires/);
            assert.match(descriptor.muteReason, /Shared or ambiguous/);
            assert.equal(descriptor.setMuted(true), false);
            assert.equal(descriptor.setMuted(false), false);
            assert.equal(descriptor.toggleMute(), false);
            assert.deepEqual(h.player.calls, []);
            assert.equal(descriptor.set(0.3), true);
            assert.deepEqual(h.player.calls, [['volume', 0.3]]);
        }
    });

    test('an available stream is not a mute target unless the Rust model assigns it', () => {
        const h = nativeHarness({ stream: new Stream({ _mute: true }) });
        const descriptor = h.native.volume({ ...h.group(), streamId: null }, h.stamp());
        assert.equal(descriptor.enabled, true);
        assert.equal(descriptor.muteEnabled, false);
        assert.equal(descriptor.muted, false);
        assert.equal(descriptor.toggleMute(), false);
        assert.deepEqual(h.stream.calls, []);
    });

    test('replacement of a player invalidates both MPRIS slider and owned-stream mute setters', () => {
        const h = nativeHarness({ stream: new Stream() });
        const descriptor = volume(h);
        const replacement = new Player();
        h.manager.players = [replacement];
        h.manager.emit('notify::players');
        assert.equal(descriptor.set(0.2), false);
        assert.equal(descriptor.setMuted(true), false);
        assert.equal(descriptor.toggleMute(), false);
        assert.deepEqual(h.player.calls, []);
        assert.deepEqual(replacement.calls, []);
        assert.deepEqual(h.stream.calls, []);
    });

    test('volume setters retain finite/range guards and a positive stream volume unmutes natively', () => {
        const h = nativeHarness({ player: new Player({ _volume: -1 }), stream: new Stream({ _mute: true }) });
        const descriptor = volume(h);
        assert.equal(descriptor.set(NaN), false);
        assert.equal(descriptor.set(Infinity), false);
        assert.equal(descriptor.set(2), true);
        assert.deepEqual(h.stream.calls, [['volume', 1], ['mute', false]]);
        assert.equal(h.stream.volume, 0.7);
        const m = nativeHarness();
        assert.equal(volume(m).set(-1), true);
        assert.deepEqual(m.player.calls, [['volume', 0]]);
    });

    test('all volume/mute operations reject stale ownership and replaced streams', () => {
        for (const useStream of [false, true]) {
            const h = nativeHarness({ player: new Player({ _volume: useStream ? -1 : 0.6 }), stream: new Stream() });
            const descriptor = volume(h);
            h.player.title = 'Next source';
            assert.equal(descriptor.set(0.2), false);
            assert.equal(descriptor.setMuted(true), false);
            assert.equal(descriptor.toggleMute(), false);
            assert.deepEqual(h.player.calls, []);
            assert.deepEqual(h.stream.calls, []);
        }
        for (const level of [-1, 0.6]) {
            const h = nativeHarness({ player: new Player({ _volume: level }), stream: new Stream() });
            const descriptor = volume(h);
            h.audio.streams = [new Stream()];
            h.audio.emit('notify::streams');
            assert.equal(descriptor.set(0.2), false);
            assert.equal(descriptor.setMuted(true), false);
            assert.equal(descriptor.toggleMute(), false);
            assert.deepEqual(h.stream.calls, []);
            assert.deepEqual(h.player.calls, []);
        }
    });

    test('new MPRIS volume disables stale stream slider dispatch, not its owned-stream mute', () => {
        const h = nativeHarness({ player: new Player({ _volume: -1 }), stream: new Stream() });
        const group = h.group();
        const descriptor = h.native.volume(group, h.stamp());
        group.streamId = 999;
        assert.equal(descriptor.setMuted(true), true, 'the descriptor captures its original native target');
        h.player.publish('volume', 0.4);
        assert.equal(descriptor.setMuted(false), true);
        assert.equal(descriptor.set(0.1), false);
        assert.deepEqual(h.stream.calls, [['mute', true], ['mute', false]]);
        assert.deepEqual(h.player.calls, []);
    });

    test('disabled descriptors expose the same safe mute API and disposal invalidates live descriptors', () => {
        const h = nativeHarness({ player: new Player({ _volume: -1 }) });
        const disabled = volume(h);
        assert.equal(disabled.enabled, false);
        assert.equal(disabled.muteEnabled, false);
        assert.match(disabled.muteReason, /Native mute requires/);
        assert.equal(disabled.muted, false);
        assert.equal(disabled.set(1), false);
        assert.equal(disabled.setMuted(true), false);
        assert.equal(disabled.toggleMute(), false);
        const m = nativeHarness({ stream: new Stream() });
        const live = volume(m);
        assert.equal(live.muteEnabled, true);
        m.native.dispose();
        assert.equal(live.set(0.2), false);
        assert.equal(live.setMuted(true), false);
        assert.equal(live.toggleMute(), false);
    });

    test('public backend descriptors require a current Rust ownership decision, including at dispatch', () => {
        const h = nativeHarness({ player: new Player({ _volume: -1 }), stream: new Stream() });
        const model = { primary: h.player.bus_name, streamId: h.stream.id };
        const backend = Object.assign(Object.create(h.MediaBackend.prototype), {
            native: h.native, modelStamp: h.stamp(), disposed: false,
            planner: { ready: true }, groups: [model],
        });
        const descriptor = backend.volume(h.group());
        assert.equal(descriptor.enabled, true);
        assert.equal(descriptor.muteEnabled, true);
        backend.planner.ready = false;
        assert.equal(descriptor.toggleMute(), false);
        assert.equal(backend.volume(h.group()).enabled, false);
        assert.equal(backend.volume(h.group()).muteEnabled, false);
        backend.planner.ready = true;
        backend.groups = [];
        assert.equal(descriptor.setMuted(true), false);
        backend.groups = [{ ...model, streamId: null }];
        assert.equal(descriptor.set(0.2), false);
        assert.deepEqual(h.stream.calls, []);
    });

    test('transport availability uses native capabilities from every current group member', () => {
        const h = nativeHarness({ player: new Player({
            bus_name: 'org.mpris.MediaPlayer2.mprisence_web.generic.pmetadata', can_play: false, can_pause: false,
        }) });
        const model = { primary: h.player.bus_name, players: [h.player.bus_name], streamId: null };
        const backend = Object.assign(Object.create(h.MediaBackend.prototype), {
            native: h.native, modelStamp: h.stamp(), disposed: false,
            planner: { ready: true }, groups: [model],
        });
        assert.equal(h.native.player(h.player.bus_name).canPlay, false);
        assert.equal(h.native.player(h.player.bus_name).canPause, false);
        assert.equal(backend.transport(h.group()).enabled, false);
        assert.match(backend.transport(h.group()).reason, /browser bridge does not expose pause/);
        assert.deepEqual(h.player.calls, []);

        const real = new Player({ bus_name: 'org.mpris.MediaPlayer2.mprisence_web.generic.preal' });
        h.manager.players.push(real);
        h.manager.emit('notify::players');
        model.players.push(real.bus_name);
        backend.modelStamp = h.stamp();
        assert.equal(backend.transport(h.group()).enabled, true, 'a capable alias can control the group');
        assert.equal(backend.transport(h.group()).reason, 'Pause');
        real.publish('can-pause', false);
        assert.equal(backend.transport(h.group()).enabled, false);
        assert.match(backend.transport(h.group()).reason, /changed/);
        backend.modelStamp = h.stamp();
        assert.match(backend.transport(h.group()).reason, /does not expose pause/);

        h.player.publish('playback-status', 2);
        real.publish('playback-status', 2);
        backend.modelStamp = h.stamp();
        assert.equal(backend.transport(h.group()).reason, 'Play');
        h.native.apply({ player: h.player.bus_name, operation: 'pause' });
        assert.deepEqual(h.player.calls, [['pause']], 'native pause-all dispatch still bypasses inaccurate CanPause flags');
        backend.planner.ready = false;
        assert.equal(backend.transport(h.group()).enabled, false);
        backend.planner.ready = true;
        h.player.available = false;
        assert.equal(backend.transport(h.group()).enabled, false);
    });

    test('an unavailable non-primary member cannot leave group transport enabled', () => {
        const h = nativeHarness();
        const alias = new Player({ bus_name: 'org.mpris.MediaPlayer2.alias', available: false });
        h.manager.players.push(alias);
        h.manager.emit('notify::players');
        const backend = Object.assign(Object.create(h.MediaBackend.prototype), {
            native: h.native, modelStamp: h.stamp(), disposed: false,
            planner: { ready: true }, groups: [{ primary: h.player.bus_name, players: [alias.bus_name, h.player.bus_name] }],
        });
        assert.equal(backend.current(), true, 'availability can change before native discovery removes an object');
        assert.equal(backend.transport(h.group()).enabled, false);
        assert.match(backend.transport(h.group()).reason, /member is unavailable/);
        assert.deepEqual(h.player.calls, []);
        alias.available = true;
        assert.equal(backend.transport(h.group()).enabled, true);
        backend.disposed = true;
        assert.equal(backend.transport(h.group()).enabled, false);
    });

    test('transport rejects replaced representatives and missing Rust group decisions', () => {
        const h = nativeHarness();
        const group = h.group();
        const backend = Object.assign(Object.create(h.MediaBackend.prototype), {
            native: h.native, modelStamp: h.stamp(), disposed: false,
            planner: { ready: true }, groups: [{ primary: h.player.bus_name, players: [h.player.bus_name] }],
        });
        assert.equal(backend.transport(group).enabled, true);
        h.manager.players = [new Player({ bus_name: h.player.bus_name })];
        h.manager.emit('notify::players');
        backend.modelStamp = h.stamp();
        assert.equal(backend.transport(group).enabled, false);
        assert.equal(backend.transport(h.group()).enabled, true);
        backend.groups = [];
        assert.equal(backend.transport(h.group()).enabled, false);
        assert.equal(backend.transport(null).enabled, false);
    });

    test('the real native bridge and Planner coalesce seeks without fabricated playback state', async () => {
        const h = nativeHarness({ player: new Player({ playback_status: 2 }) });
        const sent = [];
        const planner = new h.Planner({
            capture: () => h.native.capture(), now: () => h.time,
            send: request => { sent.push(request); }, apply: operation => h.native.apply(operation),
        });
        const response = (actions = []) => ({
            requestId: sent.at(-1).requestId, activePlayer: h.player.bus_name, actions,
            groups: [{
                primary: h.player.bus_name, players: [h.player.bus_name], title: 'Track',
                displayName: 'Test Player', streamId: null, streamReason: 'No stream',
            }],
        });
        planner.refresh();
        await Promise.resolve();
        assert.equal(planner.receive(response()), true);
        planner.action('seek', h.player.bus_name, 80);
        await Promise.resolve();
        planner.action('seek', h.player.bus_name, 90);
        assert.equal(planner.receive(response([{ player: h.player.bus_name, operation: 'seek', value: 80 }])), true);
        await Promise.resolve();
        assert.equal(sent.at(-1).action.position, 90);
        assert.equal(sent.at(-1).players[0].position, 0, 'position is unknown until a seek is confirmed');
        assert.equal('trackKey' in sent.at(-1).players[0], false, 'native freshness keys do not change the Rust protocol');
        assert.equal(planner.receive(response([{ player: h.player.bus_name, operation: 'seek', value: 90 }])), true);
        assert.deepEqual(h.player.calls, [['position', 80], ['position', 90]]);
        assert.equal(h.native.capture().players[0].position, 10);
        assert.equal(h.native.capture().players[0].seekRevision, 2);
        h.player.publish('position', 85);
        assert.equal(h.native.capture().players[0].position, 85);
    });

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
describe('Planner', () => {
    const { Planner, MediaBackend } = loadBackend();

    function player(bus = 'a', extra = {}) {
        return {
            bus, identity: `App ${bus}`, desktopEntry: `app-${bus}`, title: `Track ${bus}`,
            artist: 'Artist', album: 'Album', url: '', artUrl: '', browserHint: null,
            playing: true, stopped: false, position: 10, length: 200,
            canPlay: true, canPause: true, canSeek: true, canNext: true, canPrevious: true,
            trackId: `/track/${bus}`, instance: `${bus}-instance-1`, seekRevision: 0,
            positionSupported: true, rate: 1, volume: 0.5, ...extra,
        };
    }

    function stream(extra = {}) {
        return {
            id: 42, name: 'App a', nickname: '', ready: true,
            properties: { 'application.name': 'App a', 'media.name': 'Track a' },
            instance: 'stream-instance-1', volume: 0.5, ...extra,
        };
    }

    function harness(players = [player()], streams = []) {
        const h = {
            state: { players, streams }, time: 100, sent: [], applied: [], models: [], errors: [],
            onApply: null, onSend: null, onModel: null,
        };
        h.planner = new Planner({
            capture: () => h.state,
            now: () => h.time,
            send: request => {
                h.sent.push(request);
                return h.onSend?.(request);
            },
            apply: operation => {
                h.applied.push({ ...operation });
                if (h.onApply) return h.onApply(operation);
                if (operation.operation === 'seek') {
                    const target = h.state.players.find(p => p.bus === operation.player);
                    target.seekRevision++;
                }
            },
            onModel: (groups, active) => {
                h.models.push({ groups, active });
                h.onModel?.(groups, active);
            },
            onError: error => h.errors.push(error),
        });
        return h;
    }

    function group(h, primary = 'a', players = [primary], extra = {}) {
        const native = h.state.players.find(p => p.bus === primary);
        return {
            title: native?.title ?? '', primary, players,
            displayName: native?.identity ?? '', streamId: null, streamReason: 'No matching audio stream.',
            ...extra,
        };
    }

    function response(h, options = {}, request = h.sent.at(-1)) {
        const groups = options.groups ?? h.state.players.map(p => group(h, p.bus));
        const active = options.active ?? (groups.find(g => g.players.includes(request.selectedPlayer)) ?? groups[0])?.primary ?? '';
        return {
            requestId: request.requestId, groups, activePlayer: active, actions: [], ...options,
        };
    }

    const tick = async () => { await Promise.resolve(); await Promise.resolve(); };

    async function prime(h, groups) {
        assert.equal(h.planner.refresh(), true);
        await tick();
        assert.equal(h.planner.receive(response(h, groups ? { groups } : {})), true);
        await tick();
        return h;
    }

    function seek(player, value) { return { player, operation: 'seek', value }; }
    function op(player, operation) { return { player, operation }; }

    test('read-only startup races refresh silently without publishing stale models', async () => {
        const h = harness();
        h.planner.refresh();
        await tick();
        h.state.players[0].title = 'Metadata arrived';
        assert.equal(h.planner.receive(response(h)), false);
        assert.deepEqual(h.errors, []);
        assert.deepEqual(h.models, []);
        assert.deepEqual(h.applied, []);
        await tick();
        assert.equal(h.sent.length, 2);
        assert.equal(h.sent[1].players[0].title, 'Metadata arrived');
        assert.equal(h.planner.receive(response(h)), true);
        assert.deepEqual(h.errors, []);
        assert.equal(h.models.length, 1);
    });

    test('view errors survive recovery refreshes and clear after a successful explicit action', async () => {
        const h = await prime(harness());
        const backend = Object.assign(Object.create(MediaBackend.prototype), { planner: h.planner });
        backend.reportFailure('Source changed. Try the action again.');
        assert.match(h.planner.error, /Try the action again/);
        h.planner.refresh();
        await tick();
        assert.equal(h.planner.receive(response(h)), true);
        assert.match(h.planner.error, /Try the action again/);
        assert.deepEqual(h.applied, []);
        h.planner.action('next', 'a');
        await tick();
        assert.equal(h.planner.receive(response(h, { actions: [op('a', 'next')] })), true);
        assert.equal(h.planner.error, '');
        assert.equal(h.errors.at(-1), '');
        assert.deepEqual(h.applied, [op('a', 'next')]);
    });

    test('matching receives normalized positions without leaking extra fields or changing raw capture', async () => {
        const h = harness([player('a', { position: 0.208, modelPosition: 0 })]);
        h.planner.refresh();
        await tick();
        assert.equal(h.sent[0].players[0].position, 0);
        assert.equal('modelPosition' in h.sent[0].players[0], false);
        assert.equal(h.state.players[0].position, 0.208);
        h.state.players[0].modelPosition = 12;
        assert.equal(h.planner.receive(response(h)), true, 'matching samples, like native positions, are not ownership guards');
        h.planner.refresh();
        await tick();
        assert.equal(h.sent.at(-1).players[0].position, 12);
        assert.equal(h.state.players[0].position, 0.208);
    });

    test('invalid matching positions are rejected instead of serialized as invented samples', async () => {
        for (const modelPosition of [-1, NaN, Infinity, null, '12']) {
            const h = harness([player('a', { modelPosition })]);
            h.planner.refresh();
            await tick();
            assert.equal(h.sent.length, 0);
            assert.match(h.planner.error, /Model positions must be finite and nonnegative/);
        }
    });

    test('refresh coalesces and sends detached JSON OBJECT protocol fields only', async () => {
        const h = harness([player('a', { position: -1, positionSupported: false, length: Infinity })], [stream()]);
        for (let i = 0; i < 20; i++) h.planner.refresh();
        assert.equal(h.sent.length, 0, 'dispatch is deferred');
        await tick();
        assert.equal(h.sent.length, 1);
        const request = h.sent[0];
        assert.equal(typeof request, 'object');
        assert.deepEqual(Object.keys(request).sort(), ['players', 'requestId', 'selectedPlayer', 'streams']);
        assert.equal(request.players[0].position, 0);
        assert.equal(request.players[0].length, null);
        assert.equal(request.players[0].canPlay, true);
        assert.equal(request.players[0].canPause, true);
        for (const key of ['instance', 'trackId', 'seekRevision', 'positionSupported', 'rate', 'volume'])
            assert.equal(key in request.players[0], false);
        assert.equal('instance' in request.streams[0], false);
        assert.equal('volume' in request.streams[0], false);
        assert.equal('routing' in request, false);
        assert.deepEqual(JSON.parse(JSON.stringify(request)), request);
        h.state.streams[0].properties['application.name'] = 'Changed owner';
        assert.equal(request.streams[0].properties['application.name'], 'App a');
        assert.ok(Object.isFrozen(request));
    });

    test('group responses omit artist and artwork while retaining native player metadata', async () => {
        const native = player('a', { artist: 'Native artist', artUrl: 'https://i.ytimg.com/vi/id/hq.jpg?format=avif' });
        const h = await prime(harness([native]));
        assert.equal(h.sent[0].players[0].artist, native.artist);
        assert.equal(h.sent[0].players[0].artUrl, native.artUrl);
        assert.deepEqual(Object.keys(h.planner.groups[0]).sort(),
            ['displayName', 'players', 'primary', 'streamId', 'streamReason', 'title']);
        const backend = Object.assign(Object.create(MediaBackend.prototype), {
            groups: h.planner.groups,
            native: { player: bus => h.state.players.find(player => player.bus === bus) ?? null },
        });
        const [resolved] = backend.resolveGroups();
        assert.equal(resolved.primary, native);
        assert.equal(resolved.primary.artist, 'Native artist');
        assert.equal(resolved.primary.artUrl, native.artUrl);
        assert.equal('artist' in resolved, false);
        assert.equal('artUrl' in resolved, false);
    });

    test('fresh response publishes Rust groups without JS regrouping and remembers selection', async () => {
        const h = harness([player(), player('alias', { playing: false }), player('other')]);
        const groups = [group(h, 'a', ['a', 'alias']), group(h, 'other')];
        await prime(h, groups);
        assert.deepEqual(h.planner.groups, groups);
        assert.equal(h.planner.activePlayer, 'a');
        assert.equal(h.planner.selectedPlayer, 'a');
        assert.ok(Object.isFrozen(h.planner.groups[0].players));
        assert.equal(h.planner.select('alias'), true);
        await tick();
        assert.equal(h.sent.at(-1).selectedPlayer, 'alias');
        assert.equal(h.planner.receive(response(h, { groups, active: 'a' })), true);
        assert.equal(h.planner.selectedPlayer, 'alias', 'retain an explicit member selection');
    });

    test('only one request is in flight; a pending refresh is not lost', async () => {
        const h = await prime(harness());
        h.planner.refresh();
        await tick();
        const pending = h.sent.at(-1);
        h.planner.refresh();
        h.planner.refresh();
        h.planner.action('next');
        await tick();
        assert.equal(h.sent.length, 2);
        h.planner.receive(response(h, {}, pending));
        await tick();
        assert.equal(h.sent.length, 3);
        assert.equal(h.sent.at(-1).action.kind, 'next');
        h.planner.receive(response(h, { actions: [op('a', 'next')] }));
        await tick();
        assert.equal(h.sent.length, 3, 'action response also satisfies the pending refresh');
        assert.deepEqual(h.applied, [op('a', 'next')]);
    });

    test('position, volume, and property insertion order are excluded from staleness', async () => {
        const h = await prime(harness([player()], [stream()]));
        h.planner.action('next');
        h.state.players[0].position = 11;
        h.state.players[0].volume = 0.9;
        h.state.streams[0].volume = 0.1;
        h.state.streams[0].properties = { 'media.name': 'Track a', 'application.name': 'App a' };
        await tick();
        assert.equal(h.sent.at(-1).players[0].position, 11, 'send recaptures rather than reusing the queued position');
        h.state.players[0].position = 12;
        h.time++;
        assert.equal(h.planner.receive(response(h, { actions: [op('a', 'next')] })), true);
        assert.deepEqual(h.applied, [op('a', 'next')]);
        assert.deepEqual(h.errors, []);
    });

    const playerChanges = {
        bus: p => { p.bus = 'replacement-bus'; },
        identity: p => { p.identity = 'Replacement App'; },
        desktopEntry: p => { p.desktopEntry = 'other.desktop'; },
        title: p => { p.title = 'Next track'; },
        artist: p => { p.artist = 'Other artist'; },
        album: p => { p.album = 'Other album'; },
        url: p => { p.url = 'https://example.test/other'; },
        artUrl: p => { p.artUrl = 'file:///other-art'; },
        browserHint: p => { p.browserHint = 'firefox'; },
        playing: p => { p.playing = false; },
        stopped: p => { p.stopped = true; },
        length: p => { p.length = 201; },
        canPlay: p => { p.canPlay = false; },
        canPause: p => { p.canPause = false; },
        canSeek: p => { p.canSeek = false; },
        canNext: p => { p.canNext = false; },
        canPrevious: p => { p.canPrevious = false; },
        trackId: p => { p.trackId = '/track/next'; },
        instance: p => { p.instance = 'a-instance-2'; },
        seekRevision: p => { p.seekRevision++; },
        positionSupported: p => { p.positionSupported = false; },
        rate: p => { p.rate = 2; },
    };

    for (const [field, change] of Object.entries(playerChanges)) {
        test(`player ${field} invalidates both queued and in-flight actions`, async () => {
            for (const inFlight of [false, true]) {
                const h = await prime(harness());
                h.planner.action('next');
                if (inFlight) await tick();
                change(h.state.players[0]);
                if (inFlight) assert.equal(h.planner.receive(response(h, { actions: [op('a', 'next')] })), false);
                await tick();
                assert.equal(h.applied.length, 0);
                assert.match(h.planner.error, /stale|Stale/);
                assert.match(h.planner.error, /not retried/);
                assert.equal(h.sent.at(-1).action, undefined, 'refreshes may recover, actions never retry');
            }
        });
    }

    const streamChanges = {
        id: s => { s.id = 43; },
        name: s => { s.name = 'Other'; },
        nickname: s => { s.nickname = 'Other'; },
        ready: s => { s.ready = false; },
        properties: s => { s.properties['application.process.id'] = '5678'; },
        instance: s => { s.instance = 'stream-instance-2'; },
    };
    for (const [field, change] of Object.entries(streamChanges)) {
        test(`stream ${field} invalidates both queued and in-flight plans`, async () => {
            for (const inFlight of [false, true]) {
                const h = await prime(harness([player()], [stream()]));
                h.planner.action('toggle');
                if (inFlight) await tick();
                change(h.state.streams[0]);
                if (inFlight) assert.equal(h.planner.receive(response(h, { actions: [op('a', 'pause')] })), false);
                await tick();
                assert.equal(h.applied.length, 0);
                assert.match(h.planner.error, /ownership changed/);
            }
        });
    }

    test('player/stream addition, removal, and order changes invalidate pending work', async () => {
        const changes = [
            h => h.state.players.push(player('c')),
            h => { h.state.players = []; },
            h => h.state.players.reverse(),
            h => h.state.streams.push(stream({ id: 44, instance: 3 })),
            h => { h.state.streams = []; },
            h => h.state.streams.reverse(),
        ];
        for (const change of changes) {
            const h = await prime(harness([player(), player('b')], [stream(), stream({ id: 43, instance: 2 })]));
            h.planner.action('next');
            await tick();
            change(h);
            assert.equal(h.planner.receive(response(h, { actions: [op('a', 'next')] })), false);
            assert.equal(h.applied.length, 0);
        }
    });

    test('a replaced native object with a reused bus or stream ID is not mistaken for the old instance', async () => {
        for (const replace of [
            h => { h.state.players[0] = { ...h.state.players[0], instance: 'new-player-object' }; },
            h => { h.state.streams[0] = { ...h.state.streams[0], instance: 'new-stream-object' }; },
        ]) {
            const h = await prime(harness([player()], [stream()]));
            h.planner.action('next');
            await tick();
            replace(h);
            assert.equal(h.planner.receive(response(h, { actions: [op('a', 'next')] })), false);
            assert.equal(h.applied.length, 0);
        }
    });

    test('selection revision detects an A -> B -> A change before send or response', async () => {
        for (const inFlight of [false, true]) {
            const h = await prime(harness([player(), player('b')]));
            h.planner.action('next');
            if (inFlight) await tick();
            h.planner.select('b');
            h.planner.select('a');
            if (inFlight) assert.equal(h.planner.receive(response(h, { actions: [op('a', 'next')] })), false);
            await tick();
            assert.equal(h.applied.length, 0);
            assert.match(h.planner.error, /selection changed/);
        }
    });

    test('cycle follows the selected group, including a selection whose model reply is still pending', async () => {
        const h = await prime(harness([player(), player('b'), player('c')]));
        assert.equal(h.planner.select('b'), true);
        assert.equal(h.planner.cycle(), true);
        assert.equal(h.planner.selectedPlayer, 'c');
        assert.equal(h.planner.cycle(), true);
        assert.equal(h.planner.selectedPlayer, 'a');
        assert.equal(h.planner.select('gone'), false);
        assert.match(h.planner.error, /no longer available/);
        assert.equal(harness([]).planner.cycle(), false);
    });

    test('seek gesture coalesces to the final queued seek and rebases only our own revision change', async () => {
        const h = await prime(harness());
        h.planner.action('seek', 'a', 20);
        await tick();
        const first = h.sent.at(-1);
        for (const value of [30, 40, 50]) h.planner.action('seek', 'a', value);
        assert.equal(h.planner.receive(response(h, { actions: [seek('a', 20)] }, first)), true);
        assert.equal(h.state.players[0].seekRevision, 1);
        await tick();
        assert.equal(h.sent.length, 3);
        assert.equal(h.sent.at(-1).action.position, 50);
        assert.equal(h.sent.at(-1).players[0].position, 10, 'dispatch does not change the sampled native position');
        assert.equal(h.planner.receive(response(h, { actions: [seek('a', 50)] })), true);
        assert.deepEqual(h.applied, [seek('a', 20), seek('a', 50)]);
        assert.equal(h.state.players[0].seekRevision, 2);
        assert.equal(h.planner.error, '');
    });

    test('a paused native seek is dispatch-only, even when native position is unsupported', async () => {
        for (const supported of [false, true]) {
            const h = await prime(harness([player('a', {
                playing: false, position: supported ? 10 : -1, positionSupported: supported,
            })]));
            h.planner.action('seek', 'a', 80);
            await tick();
            assert.equal(h.planner.receive(response(h, { actions: [seek('a', 80)] })), true);
            assert.equal(h.state.players[0].position, supported ? 10 : -1);
            assert.equal(h.state.players[0].positionSupported, supported);
            assert.equal(h.state.players[0].seekRevision, 1);
            h.time += 100;
            h.planner.refresh();
            await tick();
            assert.equal(h.sent.at(-1).players[0].position, supported ? 10 : 0);
            assert.equal(h.sent.at(-1).action, undefined, 'no acknowledgement, retry, or fabricated timeout');
        }
    });

    test('seeks also coalesce before the first deferred send', async () => {
        const h = await prime(harness());
        for (let position = 0; position < 100; position++) h.planner.action('seek', '', position);
        await tick();
        assert.equal(h.sent.length, 2);
        assert.equal(h.sent.at(-1).action.position, 99);
    });

    test('grouped seek validates and rebases exact per-player own revision increments', async () => {
        const h = harness([player(), player('alias', { playing: false })]);
        const groups = [group(h, 'a', ['a', 'alias'])];
        await prime(h, groups);
        h.planner.action('seek', 'alias', 20);
        await tick();
        h.planner.action('seek', 'alias', 40);
        assert.equal(h.planner.receive(response(h, { groups, actions: [seek('a', 20), seek('alias', 20)] })), true);
        await tick();
        assert.equal(h.sent.at(-1).action.position, 40);
        assert.deepEqual(h.state.players.map(p => p.seekRevision), [1, 1]);
    });

    test('an external change before, during, or after own seek apply never rebases a queued seek', async () => {
        for (const when of ['before', 'during', 'after']) {
            const h = await prime(harness());
            h.planner.action('seek', 'a', 20);
            await tick();
            const request = h.sent.at(-1);
            h.planner.action('seek', 'a', 50);
            if (when === 'before') h.state.players[0].trackId = '/track/other';
            if (when === 'during') h.onApply = () => {
                h.state.players[0].seekRevision++;
                h.state.players[0].trackId = '/track/other';
            };
            h.planner.receive(response(h, { actions: [seek('a', 20)] }, request));
            if (when === 'after') h.state.players[0].trackId = '/track/other';
            await tick();
            assert.equal(h.sent.at(-1).action, undefined);
            assert.equal(h.applied.length, when === 'before' ? 0 : 1);
            assert.match(h.planner.error, /not retried/);
        }
    });

    test('seek rebase cannot cross a different target or an intervening non-seek action', async () => {
        for (const barrier of ['other-bus', 'next']) {
            const h = await prime(harness([player(), player('b')]));
            h.planner.action('seek', 'a', 20);
            await tick();
            if (barrier === 'other-bus') h.planner.action('seek', 'b', 30);
            else {
                h.planner.action('next', 'a');
                h.planner.action('seek', 'a', 30);
            }
            h.planner.receive(response(h, { actions: [seek('a', 20)] }));
            await tick();
            assert.deepEqual(h.applied, [seek('a', 20)]);
            assert.equal(h.sent.at(-1).action, undefined);
            assert.match(h.planner.error, /stale queued/);
        }
    });

    test('pending queue is bounded to eight actions, with no blind retries', async () => {
        const h = await prime(harness());
        h.planner.refresh();
        await tick();
        for (let i = 0; i < 8; i++) assert.equal(h.planner.action('next'), true);
        assert.equal(h.planner.action('next'), false);
        assert.match(h.planner.error, /queue is full/);
        h.planner.receive(response(h));
        await tick();
        assert.equal(h.sent.at(-1).action.kind, 'next');
    });

    test('five-second action age is checked before send and before applying a response', async () => {
        for (const inFlight of [false, true]) {
            const h = await prime(harness());
            h.planner.action('next');
            if (inFlight) await tick();
            h.time += 5.01;
            if (inFlight) assert.equal(h.planner.receive(response(h, { actions: [op('a', 'next')] })), false);
            await tick();
            assert.equal(h.applied.length, 0);
            assert.match(h.planner.error, /expired/);
        }
    });

    test('stale refresh cannot publish old groups, and errors survive a successful recovery refresh', async () => {
        const h = await prime(harness());
        const previous = h.planner.groups;
        h.planner.refresh();
        await tick();
        h.state.players[0].trackId = '/new-track';
        assert.equal(h.planner.receive(response(h)), false);
        assert.equal(h.planner.groups, previous);
        const error = h.planner.error;
        await tick();
        assert.equal(h.planner.receive(response(h)), true);
        assert.equal(h.planner.error, error);
    });

    test('invalid action kinds/routing, invalid seconds, and unavailable bus do not dispatch native calls', async () => {
        const h = await prime(harness());
        for (const args of [['move', 'a'], ['seek', 'a', -1], ['seek', 'a', NaN], ['next', {}, 0]])
            assert.equal(h.planner.action(...args), false);
        assert.equal(h.sent.length, 1);
        h.planner.action('next', 'gone');
        await tick();
        assert.equal(h.planner.receive(response(h, { error: 'The requested media source is no longer available.' })), true);
        assert.equal(h.applied.length, 0);
        assert.match(h.planner.error, /no longer available/);
    });

    test('model policy errors can publish valid groups but cannot apply operations', async () => {
        const h = await prime(harness());
        h.planner.action('seek', 'a', 20);
        await tick();
        assert.equal(h.planner.receive(response(h, { error: 'Source cannot seek.' })), true);
        assert.match(h.planner.error, /cannot seek/);
        assert.equal(h.applied.length, 0);
        h.planner.action('next');
        await tick();
        assert.equal(h.planner.receive(response(h, { actions: [op('a', 'next')] })), true);
        assert.equal(h.planner.error, '');
        assert.equal(h.errors.at(-1), '');
    });

    test('the entire native plan is validated before the first operation is applied', async () => {
        const h = harness([player(), player('alias')]);
        const groups = [group(h, 'a', ['a', 'alias'])];
        await prime(h, groups);
        h.planner.action('toggle', 'a');
        await tick();
        assert.equal(h.planner.receive(response(h, { groups, actions: [op('a', 'pause'), op('alias', 'move')] })), false);
        assert.equal(h.applied.length, 0);
        assert.match(h.planner.error, /routing is not supported/);
    });

    const invalidPlans = [
        ['wrong operation', {}, 'next', [op('a', 'pause')]],
        ['wrong target', {}, 'next', [op('b', 'next')]],
        ['missing next capability', { canNext: false }, 'next', [op('a', 'next')]],
        ['missing previous capability', { canPrevious: false }, 'previous', [op('a', 'previous')]],
        ['missing seek capability', { canSeek: false }, 'seek', [seek('a', 20)]],
        ['wrong seek value', {}, 'seek', [seek('a', 21)]],
        ['negative seek value', {}, 'seek', [seek('a', -1)]],
        ['nonfinite seek value', {}, 'seek', [seek('a', NaN)]],
        ['unrequested native value', {}, 'next', [{ ...op('a', 'next'), value: 20 }]],
        ['routing field', {}, 'next', [{ ...op('a', 'next'), target: 'speaker' }]],
        ['duplicate native target', {}, 'next', [op('a', 'next'), op('a', 'next')]],
    ];
    for (const [name, extra, kind, actions] of invalidPlans) {
        test(`reject invalid plan: ${name}`, async () => {
            const h = await prime(harness([player('a', extra), player('b')]));
            h.planner.action(kind, 'a', kind === 'seek' ? 20 : 0);
            await tick();
            assert.equal(h.planner.receive(response(h, { actions })), false);
            assert.equal(h.applied.length, 0);
            assert.match(h.planner.error, /rejected/);
        });
    }

    test('refresh actions, error actions, malformed groups, invalid active bus, and unavailable streams are rejected', async () => {
        const modifications = [
            h => ({ actions: [op('a', 'pause')] }),
            h => ({ actions: [op('a', 'pause')], error: 'Failure' }),
            h => ({ groups: [group(h, 'gone')] }),
            h => ({ groups: [group(h, 'a', ['a', 'a'])] }),
            h => ({ groups: [group(h), group(h)] }),
            h => ({ groups: [group(h, 'a', ['b'])] }),
            h => ({ activePlayer: 'gone' }),
            h => ({ groups: [group(h, 'a', ['a'], { streamId: 999 })] }),
            h => ({ groups: [group(h, 'a', ['a'], { streamId: 0 })] }),
            h => ({ groups: [group(h, 'a', ['a'], { streamId: 42 })] }),
            h => ({ groups: null }),
            h => ({ actions: null }),
            h => ({ error: 42 }),
        ];
        for (const modify of modifications) {
            const h = await prime(harness([player(), player('b')], [stream({ ready: false })]));
            h.planner.refresh();
            await tick();
            assert.equal(h.planner.receive(response(h, modify(h))), false);
            assert.equal(h.applied.length, 0);
        }
    });

    test('valid ready stream ownership and ungrouped playing pause-all targets are accepted', async () => {
        const h = harness([player(), player('filtered')], [stream()]);
        const groups = [group(h, 'a', ['a'], { streamId: 42 })];
        await prime(h, groups);
        h.planner.action('pauseAll');
        await tick();
        assert.equal(h.planner.receive(response(h, { groups, actions: [op('a', 'pause'), op('filtered', 'pause')] })), true);
        assert.deepEqual(h.applied, [op('a', 'pause'), op('filtered', 'pause')]);
    });

    test('toggle follows the Rust group, next/previous only its primary, and idle togglePauseAll resumes selected group', async () => {
        const h = harness([player('a', { playing: false }), player('alias', { playing: false }), player('b', { playing: false })]);
        const groups = [group(h, 'a', ['a', 'alias']), group(h, 'b')];
        await prime(h, groups);
        for (const [kind, actions] of [
            ['toggle', [op('a', 'play'), op('alias', 'play')]],
            ['next', [op('a', 'next')]],
            ['previous', [op('a', 'previous')]],
            ['togglePauseAll', [op('a', 'play'), op('alias', 'play')]],
        ]) {
            h.planner.action(kind, 'alias');
            await tick();
            assert.equal(h.planner.receive(response(h, { groups, actions })), true);
        }
        assert.equal(h.applied.length, 6);
    });

    test('togglePauseAll must not mix play and pause; pauseAll must not play', async () => {
        for (const [kind, actions] of [
            ['togglePauseAll', [op('a', 'pause'), op('b', 'play')]],
            ['pauseAll', [op('a', 'play')]],
        ]) {
            const h = await prime(harness([player(), player('b', { playing: false })]));
            h.planner.action(kind);
            await tick();
            assert.equal(h.planner.receive(response(h, { actions })), false);
            assert.equal(h.applied.length, 0);
        }
    });

    test('snapshot is guarded between native operations; external changes stop the remaining plan', async () => {
        const h = harness([player(), player('alias')]);
        const groups = [group(h, 'a', ['a', 'alias'])];
        await prime(h, groups);
        h.onApply = () => { h.state.players[1].instance = 'new-alias-instance'; };
        h.planner.action('toggle');
        await tick();
        assert.equal(h.planner.receive(response(h, { groups, actions: [op('a', 'pause'), op('alias', 'pause')] })), false);
        assert.deepEqual(h.applied, [op('a', 'pause')]);
        assert.match(h.planner.error, /remaining actions were not retried/);
    });

    test('apply errors, false, and explicit error results are useful and never retry the operation', async () => {
        for (const apply of [
            () => { throw new Error('native object disappeared'); },
            () => false,
            () => ({ error: 'unsupported native operation' }),
        ]) {
            const h = await prime(harness());
            h.onApply = apply;
            h.planner.action('next');
            await tick();
            assert.equal(h.planner.receive(response(h, { actions: [op('a', 'next')] })), false);
            await tick();
            assert.deepEqual(h.applied, [op('a', 'next')]);
            assert.equal(h.sent.at(-1).action, undefined);
            assert.match(h.planner.error, /native object disappeared|returned false|unsupported native/);
            assert.equal(h.planner.ready, true);
        }
    });

    test('failed native seek discards its queued final seek rather than assuming it can rebase', async () => {
        const h = await prime(harness());
        h.onApply = () => { throw new Error('native seek failed'); };
        h.planner.action('seek', 'a', 20);
        await tick();
        h.planner.action('seek', 'a', 50);
        assert.equal(h.planner.receive(response(h, { actions: [seek('a', 20)] })), false);
        await tick();
        assert.deepEqual(h.applied, [seek('a', 20)]);
        assert.equal(h.sent.at(-1).action, undefined);
        assert.match(h.planner.error, /pending actions were discarded, not retried/);
    });

    test('Promise send/apply and an explicit false send fail safely', async () => {
        for (const send of [() => false, () => Promise.reject(new Error('async send failure'))]) {
            const h = await prime(harness());
            h.onSend = send;
            h.planner.action('next');
            await tick();
            assert.equal(h.planner.ready, false);
            assert.equal(h.applied.length, 0);
            assert.match(h.planner.error, /send failed/);
        }
        const h = await prime(harness());
        h.onApply = () => Promise.reject(new Error('async apply failure'));
        h.planner.action('next');
        await tick();
        assert.equal(h.planner.receive(response(h, { actions: [op('a', 'next')] })), false);
        await tick();
        assert.match(h.planner.error, /apply\(\) must be synchronous/);
        assert.equal(h.sent.at(-1).action, undefined);
    });

    test('seek apply must synchronously advance exactly its own seek dispatch revision', async () => {
        const h = await prime(harness());
        h.onApply = () => {};
        h.planner.action('seek', 'a', 20);
        await tick();
        assert.equal(h.planner.receive(response(h, { actions: [seek('a', 20)] })), false);
        assert.match(h.planner.error, /advance only its own seek dispatch revision once/);
    });

    test('synchronous send replies and callback refresh/action calls drain without recursive send', async () => {
        const h = harness();
        let depth = 0;
        let maxDepth = 0;
        let didAction = false;
        h.onSend = request => {
            depth++;
            maxDepth = Math.max(depth, maxDepth);
            const actions = request.action ? [op('a', 'next')] : [];
            h.planner.receive(response(h, { actions }, request));
            depth--;
        };
        h.onModel = () => {
            if (!didAction) {
                didAction = true;
                h.planner.refresh();
                h.planner.action('next');
            }
        };
        h.planner.refresh();
        await tick();
        await tick();
        assert.equal(h.sent.length, 2);
        assert.equal(maxDepth, 1);
        assert.deepEqual(h.applied, [op('a', 'next')]);
    });

    test('reset clears model/errors/selection/work and late replies cannot match restarted IDs', async () => {
        const h = await prime(harness());
        h.planner.action('next');
        await tick();
        const old = h.sent.at(-1);
        h.planner.action('seek', 'a', 50);
        h.planner.action('bad-kind');
        h.planner.reset();
        assert.equal(h.planner.ready, false);
        assert.deepEqual(h.planner.groups, []);
        assert.equal(h.planner.activePlayer, '');
        assert.equal(h.planner.selectedPlayer, '');
        assert.equal(h.planner.error, '');
        assert.equal(h.errors.at(-1), '');
        assert.equal(h.planner.receive(response(h, { actions: [op('a', 'next')] }, old)), false);
        h.planner.setReady(true);
        await tick();
        assert.ok(h.sent.at(-1).requestId > old.requestId);
        assert.equal(h.sent.at(-1).action, undefined);
        assert.equal(h.planner.receive(response(h, {}, old)), false);
        assert.equal(h.planner.receive(response(h)), true);
        assert.equal(h.applied.length, 0);
    });

    test('reset during native apply cancels the rest of the plan and never republishes the old model', async () => {
        const h = harness([player(), player('alias')]);
        const groups = [group(h, 'a', ['a', 'alias'])];
        await prime(h, groups);
        h.onApply = () => h.planner.reset();
        h.planner.action('toggle');
        await tick();
        assert.equal(h.planner.receive(response(h, { groups, actions: [op('a', 'pause'), op('alias', 'pause')] })), false);
        assert.equal(h.applied.length, 1);
        assert.deepEqual(h.planner.groups, []);
        assert.equal(h.models.at(-1).active, '');
    });

    test('send failure disables worker, clears model, and never replays actions after restart', async () => {
        const h = await prime(harness());
        h.onSend = () => { throw new Error('worker stdin closed'); };
        h.planner.action('next');
        h.planner.action('seek', 'a', 40);
        await tick();
        assert.equal(h.planner.ready, false);
        assert.deepEqual(h.planner.groups, []);
        assert.match(h.planner.error, /stdin closed/);
        assert.match(h.planner.error, /not retried/);
        h.onSend = null;
        h.planner.setReady(true);
        await tick();
        assert.equal(h.sent.at(-1).action, undefined);
        assert.equal(h.applied.length, 0);
    });

    test('setReady(false) prevents capture/send, and actions are rejected instead of retained', async () => {
        const h = harness();
        h.planner.setReady(false);
        assert.equal(h.planner.refresh(), false);
        assert.equal(h.planner.action('next'), false);
        assert.match(h.planner.error, /not ready/);
        await tick();
        assert.equal(h.sent.length, 0);
    });

    test('orphan, duplicate, and mismatched request responses cannot apply or clear the in-flight request', async () => {
        const h = await prime(harness());
        const duplicate = response(h);
        h.planner.action('next');
        await tick();
        assert.equal(h.planner.receive(duplicate), false);
        assert.equal(h.planner.receive(null), false);
        assert.equal(h.planner.receive({ requestId: 0, error: 'Invalid request' }), false);
        assert.equal(h.planner.receive(response(h, { actions: [op('a', 'next')] })), true);
        assert.equal(h.applied.length, 1);
    });

    test('capture contract errors are reported before send; no unsafe serialization defaults', async () => {
        for (const extra of [
            { instance: undefined }, { trackId: undefined }, { seekRevision: -1 },
            { positionSupported: undefined }, { rate: Infinity }, { position: NaN },
        ]) {
            const h = harness([player('a', extra)]);
            assert.equal(h.planner.action('next'), false);
            assert.match(h.planner.error, /Cannot capture/);
            h.planner.refresh();
            await tick();
            assert.equal(h.sent.length, 0);
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
