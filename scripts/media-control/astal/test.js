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

test('only the main title retains a GTK tooltip, including source-select and section ancestors', () => {
    assert.deepEqual(viewSource.split('\n').filter(line => /tooltip/i.test(line)).map(line => line.trim()),
        ["this.title.tooltip_text = player?.title || '';"]);
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

test('media runtime supports AVIF artwork and refreshes native progress while open', () => {
    const runtime = readFileSync(new URL('../../../niri/panels/media.nix', import.meta.url), 'utf8');
    assert.match(runtime, /pkgs\.libavif/);
    assert.match(viewSource, /positionTimer = GLib\.timeout_add/);
    assert.match(viewSource, /GLib\.source_remove\(this\.positionTimer\)/);
});

test('elapsed time, seek bar, and total time share one compact horizontal row', () => {
    assert.match(viewSource, /this\.total\.xalign = 1;\s*this\.details\.append\(box\(false, 6, \[this\.elapsed, this\.seek, this\.total\]\)\);/);
    assert.doesNotMatch(viewSource, /this\.details\.append\(box\(true, 4, \[this\.seek,/);
});

test('volume drag refreshes ownership but never switches its captured native target', () => {
    const start = viewSource.indexOf("this.volume.connect('change-value', ");
    const body = viewSource.slice(start, viewSource.indexOf('\n        this.drag(this.volume', start));
    let callback;
    let writes = 0;
    let failures = 0;
    const context = {
        volume: { connect(name, fn) { callback = fn; } },
        volumeTarget: 'stream:1', group: {},
        volumeControl: { enabled: true, set() { throw new Error('Stale descriptor used'); } },
        backend: { volume() { return { key: 'stream:1', enabled: true, set() { writes++; return true; } }; } },
        setFailure() { failures++; },
    };
    new Function(body).call(context);
    assert.equal(callback(null, null, 0.4), false);
    assert.equal(writes, 1);
    context.volumeTarget = 'stream:2';
    assert.equal(callback(null, null, 0.5), true);
    assert.equal(writes, 1);
    assert.equal(failures, 1);
});

describe('Media volume bar', () => {
    function harness() {
        class Widget {
            constructor(properties = {}) { this.sensitive = true; this.children = []; this.signals = new Map(); Object.assign(this, accessibleWidget(properties)); }
            append(child) { this.children.push(child); child.parent = this; }
            connect(name, callback) { this.signals.set(name, callback); }
            emit(name, ...args) { return this.signals.get(name)?.(this, ...args); }
            is_sensitive() { return this.sensitive && (!this.parent || this.parent.is_sensitive()); }
            add_css_class(name) { (this.css_classes ||= []).push(name); }
        }
        class Scale extends Widget {
            constructor(properties) { super(properties); this.value = 0; this.formats = 0; this.moves = []; }
            set draw_value(value) { this.drawValue = value; this.round_digits = value ? 1 : -1; }
            get draw_value() { return this.drawValue; }
            set_format_value_func(callback) { this.format = callback; this.formats++; }
            set_value(value) { this.value = value; }
            emit(name, direction) {
                this.moves.push([name, direction]);
                assert.equal(name, 'move-slider');
                const delta = direction === Gtk.ScrollType.STEP_FORWARD ? this.adjustment.step_increment : -this.adjustment.step_increment;
                const value = Math.max(this.adjustment.lower, Math.min(this.adjustment.upper, this.value + delta));
                if (!this.signals.get('change-value')(this, direction, value)) this.set_value(value);
            }
        }
        const Gtk = { Scale, Box: Widget, Button: Widget, Label: Widget, Adjustment: Widget,
            Orientation: { HORIZONTAL: 0, VERTICAL: 1 }, Align: { CENTER: 0 }, PositionType: { TOP: 0 },
            ScrollType: { STEP_BACKWARD: 0, STEP_FORWARD: 1 }, AccessibleProperty: accessibleProperty };
        const { scale, volumeBar } = new Function('Gtk',
            viewSource.slice(viewSource.indexOf('function scale('), viewSource.indexOf('function trackKey('))
            + '\nreturn { scale, volumeBar };')(Gtk);
        const writes = [], failures = [];
        let reads = 0;
        const control = { key: 'stream:2', enabled: true, set(value) { writes.push(value); return true; } };
        const panel = {
            details: new Widget(), volumeControl: { key: 'stream:1', set() { throw new Error('Cached volume setter used'); } },
            backend: { volume(group) { assert.equal(group, panel.group); reads++; return control; } }, group: {},
            drag(widget, begin) { this.beginVolumeDrag = begin; }, setFailure(text) { failures.push(text); },
        };
        const start = viewSource.indexOf('        this.volumeGlyph =');
        const end = viewSource.indexOf("\n        this.details.append(new Gtk.Separator", start);
        new Function('Gtk', 'label', 'scale', 'volumeBar', 'box', 'transport', viewSource.slice(start, end))
            .call(panel, Gtk, (text, classes) => new Widget({ label: text, css_classes: classes }), scale, volumeBar,
                (_, spacing, children) => { const row = new Widget({ spacing }); children.forEach(child => row.append(child)); return row; }, new Widget());
        const [backward, native, forward] = panel.volumeBar.widget.children;
        return { panel, native, backward, forward, control, writes, failures, reads: () => reads, Gtk };
    }

    test('wraps the existing native scale with hollow step buttons and keeps mute glyph-only', () => {
        const { panel, native, backward, forward } = harness();
        assert.equal(native, panel.volume);
        assert.equal(native.draw_value, true);
        assert.equal(native.round_digits, -1, 'drawing the native value must not round 1% steps to tenths');
        assert.deepEqual([native.adjustment.lower, native.adjustment.upper,
            native.adjustment.step_increment, native.adjustment.page_increment], [0, 1, 0.01, 0.05]);
        assert.deepEqual(native.css_classes, ['volume-track']);
        assert.equal(native.value_pos, 0);
        assert.equal(backward.label, '◁');
        assert.equal(forward.label, '▷');
        assert.equal(backward.a11y.label, 'Decrease volume');
        assert.equal(forward.a11y.label, 'Increase volume');
        assert.equal(native.a11y.label, 'Volume');
        assert.equal(panel.volumeButton.a11y.label, 'Mute / unmute');
        for (const widget of [backward, forward, native, panel.volumeButton]) {
            for (let ancestor = widget; ancestor; ancestor = ancestor.parent) {
                assert.equal(ancestor.tooltip_text, undefined);
                assert.equal(ancestor.tooltip_markup, undefined);
            }
        }
        assert.deepEqual(backward.css_classes, ['volume-step']);
        assert.equal(panel.volumeButton.child, panel.volumeGlyph);
        assert.equal('volumeValue' in panel, false);
        assert.equal(panel.details.children[0].children[2], panel.volumeBar.widget);
        for (const [value, text] of [[0, '0%'], [0.375, '38%'], [1, '100%']])
            assert.equal(native.format(native, value), text);
    });

    test('availability invalidates cached native text at the same value and disables the whole bar', () => {
        const { panel, native, forward } = harness();
        native.set_value(0.5);
        const formats = native.formats;
        native.sensitive = false;
        panel.volumeBar.setAvailable(false);
        assert.equal(panel.volumeBar.widget.sensitive, false);
        assert.equal(forward.is_sensitive(), false);
        assert.equal(native.format(native, native.value), '—');
        assert.equal(native.formats, formats + 1);
        panel.volumeBar.setAvailable(false);
        assert.equal(native.formats, formats + 1);
        native.sensitive = true;
        panel.volumeBar.setAvailable(true);
        assert.equal(panel.volumeBar.widget.sensitive, true);
        assert.equal(native.format(native, native.value), '50%');
        assert.equal(native.formats, formats + 2);
    });

    test('arrows clear a captured drag and use move-slider plus a freshly validated descriptor', () => {
        const { panel, native, backward, forward, writes, reads, Gtk } = harness();
        native.set_value(0.5);
        panel.beginVolumeDrag();
        assert.equal(panel.volumeDragging, true);
        assert.equal(panel.volumeTarget, 'stream:1');
        forward.emit('clicked');
        assert.equal(panel.volumeDragging, false);
        assert.equal(panel.volumeTarget, null);
        assert.deepEqual(writes, [0.51]);
        assert.equal(reads(), 1);
        backward.emit('clicked');
        assert.deepEqual(writes, [0.51, 0.5]);
        assert.equal(reads(), 2);
        assert.deepEqual(native.moves, [['move-slider', Gtk.ScrollType.STEP_FORWARD], ['move-slider', Gtk.ScrollType.STEP_BACKWARD]]);
    });

    test('disabled arrows do nothing and fresh unavailable or rejected descriptors stop the native change', () => {
        const { panel, native, backward, forward, control, failures, writes, reads } = harness();
        native.set_value(0.5);
        panel.beginVolumeDrag();
        native.sensitive = false;
        forward.emit('clicked');
        assert.equal(panel.volumeDragging, true);
        assert.equal(panel.volumeTarget, 'stream:1');
        assert.equal(reads(), 0);
        native.sensitive = true;
        panel.volumeBar.widget.sensitive = false;
        backward.emit('clicked');
        assert.equal(reads(), 0, 'the native effective-sensitivity guard includes ancestors');
        panel.volumeBar.widget.sensitive = true;
        control.enabled = false;
        forward.emit('clicked');
        assert.equal(reads(), 1);
        assert.deepEqual(writes, []);
        assert.equal(native.value, 0.5);
        control.enabled = true;
        control.set = () => false;
        backward.emit('clicked');
        assert.equal(reads(), 2);
        assert.equal(native.value, 0.5);
        assert.equal(failures.length, 2);
        assert.ok(failures.every(text => /media source changed/.test(text)));
    });

    test('mute retains its native toggle and unavailable feedback', () => {
        const { panel } = harness();
        let toggles = 0;
        panel.volumeControl.toggleMute = () => { toggles++; return true; };
        panel.volumeButton.emit('clicked');
        assert.equal(toggles, 1);
        panel.volumeControl.toggleMute = () => false;
        let failure;
        panel.setFailure = text => { failure = text; };
        panel.volumeButton.emit('clicked');
        assert.match(failure, /Native mute is unavailable or the media source changed/);
    });
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
    const MediaPanel = new Function('Gtk', 'trackKey',
        viewSource.slice(viewSource.indexOf('class MediaPanel {'), viewSource.indexOf('\nlet backend, panel;'))
        + '\nreturn MediaPanel;')(Gtk, trackKey);

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
            group: { primary: { bus: 'browser' } }, backend,
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

    test('main and source-row controls share capability feedback without hiding volume reasons', () => {
        const label = () => accessibleWidget({ set_label(text) { this.text = text; } });
        const button = () => { const child = label(); return accessibleWidget({ get_child: () => child }); };
        let enabled = false;
        const volume = { enabled: false, value: 0, reason: 'No uniquely owned audio stream.',
            muteEnabled: false, muteReason: 'No native stream mute is available.' };
        const volumeValues = [];
        const group = { title: 'Track', displayName: 'Spotify', primary: { bus: 'browser', title: 'Track', playing: true }, players: ['browser'] };
        const row = { name: label(), title: label(), play: button(), art: { set() {} },
            widget: accessibleWidget({ add_css_class() {}, remove_css_class() {} }) };
        const panel = Object.assign(Object.create(MediaPanel.prototype), {
            groups: [group], activePlayer: 'browser', rows: new Map([['browser', row]]),
            backend: {
                current: () => true,
                transport: () => ({ enabled, reason: enabled ? 'Pause' : 'Browser bridge does not expose pause control.' }),
                volume: () => volume,
            },
            title: label(), credit: { set() {} }, art: { set() {} }, empty: {}, details: {},
            play: button(), previous: {}, next: {}, seek: { adjustment: {} },
            volume: accessibleWidget({ set_value(value) { volumeValues.push(value); } }),
            volumeButton: accessibleWidget(), volumeGlyph: label(), sourceName: label(),
            volumeBar: { widget: accessibleWidget(), setAvailable(value) { this.available = value; } },
            section: {}, sources: {}, chevron: label(), sourceRows: { reorder_child_after() {} },
            setFailure() {}, position() {},
        });
        const assertNoControlTooltips = () => {
            for (const widget of [panel.play, panel.previous, panel.next, panel.volume, panel.volumeBar.widget,
                panel.volumeButton, panel.section, panel.sources, panel.details, row.play, row.widget]) {
                assert.equal(widget.tooltip_text, undefined);
                assert.equal(widget.tooltip_markup, undefined);
            }
        };
        panel.render();
        assertNoControlTooltips();
        assert.equal(panel.title.tooltip_text, 'Track', 'the main title keeps its tooltip');
        assert.equal(row.widget.a11y.description, 'Spotify\nTrack');
        assert.equal(panel.play.sensitive, false);
        assert.equal(row.play.sensitive, false);
        assert.match(panel.play.a11y.description, /does not expose pause/);
        assert.equal(row.play.a11y.description, panel.play.a11y.description);
        assert.equal(panel.volume.sensitive, false);
        assert.equal(panel.volumeBar.available, false);
        assert.equal(panel.volume.a11y.description, 'No uniquely owned audio stream.');
        assert.equal(panel.volumeBar.widget.a11y.description, panel.volume.a11y.description);
        assert.equal(panel.volumeButton.sensitive, false);
        assert.equal(panel.volumeButton.a11y.description, 'No native stream mute is available.');
        enabled = true;
        volume.enabled = true;
        volume.value = 0.74;
        volume.reason = 'Native MPRIS volume';
        panel.volumeDragging = true;
        panel.render();
        assertNoControlTooltips();
        assert.equal(panel.play.sensitive, true);
        assert.equal(row.play.sensitive, true);
        assert.equal(panel.play.a11y.description, 'Pause');
        assert.equal(row.play.a11y.description, panel.play.a11y.description);
        assert.equal(panel.volume.sensitive, true);
        assert.equal(panel.volumeBar.available, true);
        assert.equal(panel.volume.a11y.description, 'Native MPRIS volume');
        assert.equal(panel.volumeBar.widget.a11y.description, panel.volume.a11y.description);
        assert.equal(panel.volumeButton.sensitive, false, 'mute availability remains independent');
        assert.equal(panel.volumeButton.a11y.description, volume.muteReason);
        assert.deepEqual(volumeValues, [0], 'render must not replace the value during a drag');
        panel.volumeDragging = false;
        panel.render();
        assertNoControlTooltips();
        assert.deepEqual(volumeValues, [0, 0.74]);
    });

    test('stale and rejected source selection is not silently discarded', () => {
        const start = viewSource.indexOf("select.connect('clicked', ");
        const body = viewSource.slice(start, viewSource.indexOf('\n                row.widget =', start));
        let callback, current = false, accepted = false, refreshes = 0, selections = 0;
        const failures = [];
        const context = {
            modelCurrent: () => current, sources: { reveal_child: true }, chevron: { set_label() {} },
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
const factory = new Function('GLib', 'Gio', 'Mpris', 'Wp', 'badge', 'helper', `'use strict';
    ${source}
    if (helper) runHelper = helper;
    return { Planner, NativeMedia, MediaBackend, ModelWorker, runHelper, ownershipStamp };
`);

const variantDouble = {
    is_object_path: value => typeof value === 'string' && /^\/(?:[A-Za-z0-9_]+(?:\/[A-Za-z0-9_]+)*)?$/.test(value),
};

function loadBackend({ GLib = {}, Gio = {}, Mpris = {}, Wp = {}, runHelper = null } = {}) {
    return factory({ Variant: variantDouble, ...GLib }, Gio, Mpris, Wp, badge, runHelper);
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

function nativeHarness({ player = new Player(), stream = null, isObjectPath = variantDouble.is_object_path } = {}) {
    const manager = Object.assign(new Signals(), { players: [player] });
    const audio = Object.assign(new Signals(), { streams: stream ? [stream] : [] });
    const sources = new Map();
    let nextSource = 0;
    const h = { time: 0, changes: 0, player, stream, manager, audio, sources };
    const GLib = {
        PRIORITY_DEFAULT_IDLE: 0, PRIORITY_DEFAULT: 0, SOURCE_REMOVE: false,
        get_monotonic_time: () => h.time * 1e6,
        idle_add: (_, callback) => { const id = ++nextSource; sources.set(id, callback); return id; },
        source_remove: id => sources.delete(id),
        Variant: { is_object_path: isObjectPath },
    };
    const Gio = { Cancellable };
    const api = loadBackend({
        GLib, Gio,
        Mpris: { get_default: () => manager, PlaybackStatus: { PLAYING: 1, PAUSED: 2, STOPPED: 3 } },
        Wp: { get_default: () => ({ audio }) },
    });
    const native = new api.NativeMedia({ onChange: () => h.changes++ });
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
        assert.deepEqual(checked, ['/track/one', '/track/one']);
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
        assert.equal(sent.at(-1).players[0].position, 10);
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
