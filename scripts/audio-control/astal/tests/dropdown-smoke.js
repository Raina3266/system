// Offline inline-selector/volume GTK4 integration smoke. Run with the audio launcher's AGS --gtk4
// wrapper, AUDIO_PANEL_CONFIG/PALETTE, and AUDIO_DROPDOWN_ISOLATED=1 in private
// Xvfb + service-free D-Bus (SYSTEM and SESSION addresses must be identical).
// Optional AUDIO_DROPDOWN_TEST_PYTHON / AUDIO_DROPDOWN_TEST_X11 select Python
// and libX11 for real XTEST mouse/keyboard input. No AudioBackend is imported.
import GLib from 'gi://GLib';
import Gio from 'gi://Gio';
import Gtk from 'gi://Gtk?version=4.0';
import Gdk from 'gi://Gdk?version=4.0';
import GdkX11 from 'gi://GdkX11?version=4.0';
import Pango from 'gi://Pango';
import Astal from 'gi://Astal?version=4.0';
import System from 'system';
import { loadPanelStyle } from '../../../live-style.js';

if (GLib.getenv('AUDIO_DROPDOWN_ISOLATED') !== '1'
    || GLib.getenv('GDK_BACKEND') !== 'x11'
    || !GLib.getenv('DBUS_SESSION_BUS_ADDRESS')
    || GLib.getenv('DBUS_SYSTEM_BUS_ADDRESS') !== GLib.getenv('DBUS_SESSION_BUS_ADDRESS'))
    throw new Error('Use private Xvfb and service-free D-Bus, not the desktop session.');

let checks = 0;
function assert(condition, message) {
    checks++;
    if (!condition) throw new Error(message);
}
function equal(actual, expected, message) {
    assert(JSON.stringify(actual) === JSON.stringify(expected),
        `${message}: ${JSON.stringify(actual)} != ${JSON.stringify(expected)}`);
}
// Fractional CSS borders produce float bounds beside integer GTK allocations.
// Accept subpixel rounding only, never a full pixel of layout drift.
function geometryEqual(actual, expected, message) {
    assert(Math.abs(actual - expected) < 1, `${message}: ${actual} != ${expected} (tolerance <1px)`);
}
function boundsEqual(actual, expected, message) {
    for (const key of ['x', 'y', 'width', 'height', 'bottom'])
        geometryEqual(actual[key], expected[key], `${message}: ${key}`);
}
function wait(milliseconds = 100) {
    return new Promise(resolve => GLib.timeout_add(GLib.PRIORITY_DEFAULT, milliseconds,
        () => { resolve(); return GLib.SOURCE_REMOVE; }));
}
function read(path) {
    if (!path) throw new Error('AUDIO_PANEL_CONFIG and AUDIO_PANEL_PALETTE are required.');
    const [ok, bytes] = GLib.file_get_contents(path);
    if (!ok) throw new Error(`Cannot read ${path}`);
    return new TextDecoder().decode(bytes);
}
function walk(widget) {
    const result = [widget];
    for (let child = widget.get_first_child(); child; child = child.get_next_sibling())
        result.push(...walk(child));
    return result;
}
function noTooltip(widget, message) {
    assert(!widget.has_tooltip && !widget.tooltip_text && !widget.tooltip_markup, message);
}
function noInheritedTooltip(widget, message) {
    for (let current = widget; current; current = current.get_parent()) noTooltip(current, message);
}
function bounds(widget, relative = panel.card) {
    const [ok, rect] = widget.compute_bounds(relative);
    assert(ok && rect.get_width() > 0 && rect.get_height() > 0, 'Widget has real GTK bounds');
    return { x: rect.get_x(), y: rect.get_y(), width: rect.get_width(), height: rect.get_height(),
        bottom: rect.get_y() + rect.get_height() };
}
function expanded(view, value, message) {
    equal([view.devices.active, view.deviceChoices.reveal_child, view.deviceArrow.label],
        [value, value, value ? '⌃' : '⌄'], message);
}
function focused(widget, message) {
    const focus = host.get_focus();
    assert(focus === widget || (focus && focus.is_ancestor(widget)), message);
}
function listLabels(view) {
    return walk(view.deviceList).filter(widget => widget instanceof Gtk.Label
        && widget.get_mapped() && view.choiceItems.some(item => item.label === widget.label));
}
async function scrollIntoView(widget, viewport) {
    const rect = bounds(widget, viewport), adjustment = viewport.get_vadjustment();
    if (rect.y < 0) adjustment.value += rect.y;
    else if (rect.bottom > viewport.get_height()) adjustment.value += rect.bottom - viewport.get_height();
    await wait();
}
function noDevicePopups() {
    assert(!walk(panel.root).some(widget => widget instanceof Gtk.DropDown || widget instanceof Gtk.Popover),
        'Audio device selectors contain no native dropdown or popup');
}
function owner(name) {
    return Gio.DBus.session.call_sync('org.freedesktop.DBus', '/org/freedesktop/DBus',
        'org.freedesktop.DBus', 'NameHasOwner', new GLib.Variant('(s)', [name]),
        new GLib.VariantType('(b)'), Gio.DBusCallFlags.NONE, 1000, null).deep_unpack()[0];
}

// Only evaluate the production helpers and AudioPanel, as the Node renderer
// tests do. The application/bootstrap below monitorFor must never execute.
const entry = GLib.getenv('AUDIO_PANEL_CONFIG');
const source = read(entry);
const boundary = source.indexOf('\nfunction monitorFor(');
assert(boundary > source.indexOf('export class AudioPanel'), 'Production extraction boundary exists');
const extracted = source.slice(0, boundary)
    .replace(/^import .*;[ \t]*$/gm, '')
    .replace('export class AudioPanel', 'class AudioPanel');
assert(!/^\s*(?:import|export)\b/m.test(extracted), 'All imports/exports stripped before evaluation');
const { AudioPanel, loadTheme } = new Function('Gtk', 'Gdk', 'Pango', 'Astal', 'GLib', 'loadPanelStyle',
    `${extracted}\nreturn { AudioPanel, loadTheme };`)(Gtk, Gdk, Pango, Astal, GLib, loadPanelStyle);

// XTEST uses the existing libX11 request allocator, so no xdotool, python-xlib,
// libXtst, shell, or extra on-disk input harness is needed. All input is sent
// exclusively to the authenticated private DISPLAY checked above.
const inputCode = `
import ctypes as C, ctypes.util, os, struct, sys
assert os.environ['AUDIO_DROPDOWN_ISOLATED'] == '1'
x = C.CDLL(os.environ.get('AUDIO_DROPDOWN_TEST_X11') or ctypes.util.find_library('X11'))
def api(name, result, *args):
    f = getattr(x, name); f.restype = result; f.argtypes = args; return f
P, I, U, L = C.c_void_p, C.c_int, C.c_uint, C.c_ulong
open_display = api('XOpenDisplay', P, C.c_char_p)
d = open_display(None)
assert d, 'Cannot open private Xvfb'
try:
    root = api('XDefaultRootWindow', L, P)(d)
    opcode, event, error = I(), I(), I()
    assert api('XQueryExtension', I, P, C.c_char_p, C.POINTER(I), C.POINTER(I), C.POINTER(I))(
        d, b'XTEST', C.byref(opcode), C.byref(event), C.byref(error)), 'XTEST unavailable'
    request = api('_XGetRequest', P, P, C.c_ubyte, C.c_size_t)
    def send(kind, detail=0, px=0, py=0):
        data = struct.pack('=BBHBBHII8xhh8x', opcode.value, 2, 9, kind, detail, 0, 0, root, px, py)
        C.memmove(request(d, opcode.value, len(data)), data, len(data))
    mode, xid = sys.argv[1], int(sys.argv[2])
    if mode == 'key':
        api('XSetInputFocus', I, P, L, I, L)(d, xid, 2, 0)
        symbol = api('XStringToKeysym', L, C.c_char_p)(sys.argv[3].encode())
        code = api('XKeysymToKeycode', C.c_ubyte, P, L)(d, symbol)
        assert code, 'Missing X11 keycode'
        send(2, code); send(3, code)
    elif mode == 'pixels':
        import hashlib
        left, top, width, height = map(int, sys.argv[3:7])
        image = api('XGetImage', P, P, L, I, I, U, U, L, I)(d, xid, left, top, width, height, L(-1).value, 2)
        assert image, 'Cannot read private widget pixels'
        try:
            pixel = api('XGetPixel', L, P, I, I)
            data = b''.join(pixel(image, col, row).to_bytes(8, 'little') for row in range(height) for col in range(width))
            print(hashlib.sha256(data).hexdigest())
        finally:
            api('XDestroyImage', I, P)(image)
    else:
        px, py, child = I(), I(), L()
        assert api('XTranslateCoordinates', I, P, L, L, I, I, C.POINTER(I), C.POINTER(I), C.POINTER(L))(
            d, xid, root, int(sys.argv[3]), int(sys.argv[4]), C.byref(px), C.byref(py), C.byref(child))
        send(6, px=px.value, py=py.value)
        if mode == 'click': send(4, 1); send(5, 1)
        elif mode == 'press': send(4, 1)
        elif mode == 'release': send(5, 1)
        elif mode == 'scroll': send(4, 4); send(5, 4)
    api('XSync', I, P, I)(d, 0)
finally:
    api('XCloseDisplay', I, P)(d)
`;
function input(mode, widget, key = '') {
    const native = widget.get_native();
    const surface = native.get_surface();
    assert(surface instanceof GdkX11.X11Surface, 'Input targets a private real X11 surface');
    const args = [mode, String(surface.get_xid())];
    if (mode === 'key') args.push(key);
    else {
        const [ok, rect] = widget.compute_bounds(native);
        assert(ok && rect.get_width() > 0 && rect.get_height() > 0, 'Input widget is allocated');
        const [sx, sy] = native.get_surface_transform();
        if (mode === 'pixels') args.push(String(Math.round(rect.get_x() + sx)),
            String(Math.round(rect.get_y() + sy)), String(Math.floor(rect.get_width())), String(Math.floor(rect.get_height())));
        else args.push(String(Math.round(rect.get_x() + rect.get_width() / 2 + sx)),
            String(Math.round(rect.get_y() + rect.get_height() / 2 + sy)));
    }
    const process = Gio.Subprocess.new([GLib.getenv('AUDIO_DROPDOWN_TEST_PYTHON')
        || GLib.find_program_in_path('python3'), '-c', inputCode, ...args],
    Gio.SubprocessFlags.STDOUT_PIPE | Gio.SubprocessFlags.STDERR_PIPE);
    const [, stdout, stderr] = process.communicate_utf8(null, null);
    assert(process.get_successful(), `Private XTEST input: ${stdout}${stderr}`);
    return stdout.trim();
}

function choices(kind) {
    const duplicate = `<b>${kind} USB & 🎧</b> ` + 'very long literal device name '.repeat(24);
    return [
        { key: `native:${kind}:a`, label: duplicate, description: `<i>${kind} route A & literal</i>`, default: false },
        { key: `policy:${kind}:b`, label: duplicate, description: `<i>${kind} route B & literal</i>`, default: true },
        { key: `native:${kind}:c`, label: `${kind} Dock`, description: '', default: false },
    ];
}
class FakeBackend {
    constructor() {
        this.calls = [];
        this.activity = [];
        this.starts = 0;
        this.state = { bluetooth: { available: false, powered: false, devices: [] } };
        for (const kind of ['output', 'input']) this.state[kind] = {
            available: true, nodeKey: `current:${kind}`, name: `<b>${kind} native default</b>`,
            volume: 0.25, muted: false, busy: false, choices: choices(kind),
        };
    }
    start() { this.starts++; throw new Error('A fake-only UI smoke must never start a backend'); }
    snapshot() { return this.state; }
    select(...args) {
        this.calls.push(['select', ...args]);
        if (this.selectionError) throw new Error(this.selectionError);
    }
    setVolume(kind, value, key) {
        this.calls.push(['volume', kind, value, key]);
        this.state[kind] = { ...this.state[kind], volume: value };
    }
    toggleMute(kind, key) {
        this.calls.push(['mute', kind, key]);
        this.state[kind] = { ...this.state[kind], muted: !this.state[kind].muted };
    }
    setActive(...args) { this.activity.push(args); }
}

let panel, host, app;
const backend = new FakeBackend();
function allocation(message) {
    equal(panel.card.get_width(), 400, `${message}: constrained width`);
    const [, naturalHeight] = panel.content.measure(Gtk.Orientation.VERTICAL, 400);
    equal(panel.card.get_height(), naturalHeight, `${message}: content-sized height`);
    assert(panel.card.get_height() <= 520, `${message}: height stays within the monitor budget`);
    equal([host.get_width(), host.get_height()], [1280, 800], 'Long names do not enlarge the host');
}
async function testCompactLayout() {
    const before = backend.calls.length, original = backend.state.output;
    const monitor = panel.monitor;
    for (const view of Object.values(panel.audio)) expanded(view, false, 'Compact selectors start collapsed');
    const rect = widget => {
        const [ok, value] = widget.compute_bounds(panel.card);
        assert(ok, 'Compact layout has native widget bounds');
        return value;
    };
    const compactHeight = panel.card.get_height();
    assert(compactHeight < 520, 'Audio card shrinks below the old fixed height');
    const sections = walk(panel.audioPage).filter(widget => widget.has_css_class('audio-section'));
    equal(sections.length, 2, 'One audio viewport contains both natural-height sections');
    for (const [index, kind] of ['output', 'input'].entries()) {
        const section = rect(sections[index]), trigger = rect(panel.audio[kind].devices);
        geometryEqual(section.get_y() + section.get_height(), trigger.get_y() + trigger.get_height(),
            `${kind}: no expanding gap below the collapsed selector`);
    }
    const last = rect(panel.audio.input.devices);
    const padding = panel.content.get_style_context().get_padding();
    geometryEqual(panel.card.get_height() - last.get_y() - last.get_height(), padding.bottom + panel.pages.margin_bottom,
        'Only normal panel padding remains below the collapsed input selector');
    backend.state.output = { ...original, choices: original.choices.filter(item => item.default) };
    panel.render();
    await wait();
    assert(panel.card.get_height() < compactHeight, 'Hiding an unnecessary selector also shrinks the card');
    backend.state.output = { ...original, error: 'A wrapped device error with useful details. '.repeat(60) };
    panel.render();
    await wait();
    assert(panel.card.get_height() <= 520, 'Wrapped errors cannot enlarge the card beyond the budget');
    let adjustment = panel.audioPage.get_vadjustment();
    assert(adjustment.upper > adjustment.page_size, 'Long errors scroll within the single audio viewport');
    backend.state.output = original;
    panel.render();
    await wait();
    equal(panel.card.get_height(), compactHeight, 'Clearing errors restores the compact height');
    panel.setTab(0);
    await wait();
    equal(panel.card.get_height(), 520, 'Bluetooth retains its existing panel height');
    panel.setTab(1);
    await wait();
    equal(panel.card.get_height(), compactHeight, 'Returning to Audio restores content-sized height');
    panel.monitor = { get_geometry: () => ({ width: 340, height: 300 }) };
    panel.render();
    await wait();
    equal(panel.card.get_width(), 324, 'Short/narrow monitor preserves horizontal clearance');
    assert(panel.card.get_height() <= 236, 'Short monitor preserves the existing vertical clearance');
    adjustment = panel.audioPage.get_vadjustment();
    assert(adjustment.upper > adjustment.page_size, 'Short monitor scrolls the compact audio page');
    adjustment.value = adjustment.upper - adjustment.page_size;
    await wait();
    const [ok, inputBounds] = panel.audio.input.devices.compute_bounds(panel.audioPage);
    assert(ok && inputBounds.get_y() >= 0
        && inputBounds.get_y() + inputBounds.get_height() <= panel.audioPage.get_height(),
    'Input selector remains reachable at the bottom of a short monitor');
    panel.monitor = monitor;
    panel.render();
    await wait();
    equal(panel.card.get_height(), compactHeight, 'Restoring monitor geometry restores compact height');
    equal(backend.calls.length, before, 'Layout checks never mutate audio devices');
    print(`LAYOUT compact audio card=400x${compactHeight}; collapsed selector gaps removed; short-monitor scrolling verified`);
}
async function testKind(kind) {
    const view = panel.audio[kind], trigger = view.devices, selection = view.deviceSelection;
    const initial = choices(kind);
    const update = properties => {
        backend.state[kind] = { ...backend.state[kind], ...properties };
        panel.render();
    };
    assert(trigger instanceof Gtk.ToggleButton && trigger.has_css_class('device-dropdown'),
        `${kind}: selector trigger is the native toggle itself`);
    assert(trigger.get_child() instanceof Gtk.Box && view.deviceLabel instanceof Gtk.Label
        && view.deviceArrow instanceof Gtk.Label, `${kind}: trigger owns a label and arrow box`);
    assert(view.deviceLabel.get_parent() === trigger.get_child()
        && view.deviceArrow.get_parent() === trigger.get_child(), `${kind}: both labels belong to the trigger box`);
    assert(selection instanceof Gtk.SingleSelection && !selection.autoselect && selection.can_unselect,
        `${kind}: native single selection does not autoselect`);
    assert(view.deviceList instanceof Gtk.ListView && view.deviceList.model === selection
        && view.deviceList.single_click_activate && view.deviceList.has_css_class('device-list'),
        `${kind}: native list activates on one click`);
    const factory = view.deviceList.factory, bindings = [];
    assert(factory instanceof Gtk.SignalListItemFactory, `${kind}: native row factory`);
    const bindSignal = factory.connect('bind', (_, item) => {
        const widget = item.get_child(), choice = view.choiceItems[item.get_position()];
        const ancestors = [];
        for (let current = widget; current; current = current.get_parent()) ancestors.push(current);
        // Record inside GTK signals; assert outside them so GJS cannot swallow a failure.
        bindings.push({ literal: widget instanceof Gtk.Label && !widget.use_markup && widget.label === choice?.label
            && widget.ellipsize === Pango.EllipsizeMode.END,
            tooltipFree: ancestors.every(value => !value.has_tooltip && !value.tooltip_text && !value.tooltip_markup) });
    });
    assert(view.deviceChoices instanceof Gtk.Revealer, `${kind}: inline choices use a native revealer`);
    equal([view.deviceChoices.transition_type, view.deviceChoices.transition_duration],
        [Gtk.RevealerTransitionType.SLIDE_DOWN, 180], `${kind}: native slide-down animation`);
    const options = view.deviceChoices.get_child();
    assert(options instanceof Gtk.ScrolledWindow && options.get_child() === view.deviceList
        && options.propagate_natural_height && options.max_content_height === 240
        && options.margin_top === 6 && options.has_css_class('device-options'),
        `${kind}: inline list has a naturally sized, capped scroller`);
    noDevicePopups();
    for (let widget = view.deviceChoices; widget && widget !== panel.audioPage; widget = widget.get_parent())
        assert(!(widget instanceof Gtk.Overlay), `${kind}: inline choices are not hosted in an overlay`);
    expanded(view, false, `${kind}: starts collapsed`);
    // Replace metadata after observing the actual factory lifecycle.
    update({ choices: initial.map(item => ({ ...item,
        description: item.description ? item.description + ' [bound]' : '' })) });
    await wait();
    const entries = backend.state[kind].choices;
    assert(selection.model instanceof Gtk.StringList, `${kind}: real native string model`);
    equal(Array.from({ length: selection.model.get_n_items() }, (_, i) => selection.model.get_string(i)),
        entries.map(item => item.label), `${kind}: literal ordered model without an unnecessary placeholder`);
    equal(selection.selected, 1, `${kind}: authoritative default is not the first row`);
    assert(view.deviceLabel.label === entries[1].label && !view.deviceLabel.use_markup
        && view.deviceLabel.ellipsize === Pango.EllipsizeMode.END,
        `${kind}: collapsed trigger shows literal ellipsized default text`);
    for (const widget of walk(trigger)) noInheritedTooltip(widget, `${kind}: trigger has no tooltip`);
    input('move', view.devicesHeading);
    await wait(250);
    const restingPixels = input('pixels', trigger);
    input('move', trigger);
    await wait(250);
    assert(trigger.get_state_flags() & Gtk.StateFlags.PRELIGHT, `${kind}: real pointer enters native toggle`);
    equal(input('pixels', trigger), restingPixels, `${kind}: closed native toggle pixels are hover-invariant`);
    input('move', view.devicesHeading);
    await wait(250);
    assert(!view.heading.use_markup && view.heading.label.includes('<b>'), `${kind}: heading is literal`);
    assert(!('rows' in view) && !('choices' in view), `${kind}: old per-device button rows are absent`);
    assert(!walk(view.deviceList).some(widget => widget instanceof Gtk.Button), `${kind}: choices are native list rows, not buttons`);
    allocation(`${kind}: long collapsed labels retain the compact frame`);
    let before = backend.calls.length;
    const model = selection.model;
    update({ volume: 0.61, muted: true });
    assert(selection.model === model, `${kind}: volume/mute render preserves the model`);
    equal(view.volume.get_layout().get_text(), '61%', `${kind}: native volume snapshot rendered`);
    selection.selected = 0;
    selection.notify('selected');
    equal(backend.calls.length, before, `${kind}: selection notification is read-only`);
    panel.render();
    equal(selection.selected, 1, `${kind}: render restores the authoritative default`);
    panel.setDeviceExpanded(view, true);
    view.deviceList.emit('activate', 0);
    equal(backend.calls.slice(before), [['select', kind, entries[0].key]], `${kind}: activation selects the first duplicate by key`);
    equal(selection.selected, 1, `${kind}: pending activation cannot invent a native default`);
    expanded(view, false, `${kind}: activation collapses the list`);
    focused(trigger, `${kind}: activation focuses the trigger`);
    before = backend.calls.length;
    update({ choices: entries.map((item, i) => ({ ...item, default: i === 0 })) });
    assert(selection.model === model && selection.selected === 0, `${kind}: external default changes selection without rebuilding`);
    equal(backend.calls.length, before, `${kind}: external native default change never mutates`);
    panel.setDeviceExpanded(view, true);
    view.deviceList.emit('activate', 1);
    equal(backend.calls.slice(before), [['select', kind, entries[1].key]], `${kind}: second duplicate retains a distinct key`);
    before = backend.calls.length;
    panel.setDeviceExpanded(view, true);
    update({ busy: true });
    assert(!trigger.sensitive && view.volume.sensitive && view.mute.sensitive, `${kind}: busy disables only device selection`);
    expanded(view, false, `${kind}: busy render collapses choices`);
    panel.setDeviceExpanded(view, true);
    expanded(view, false, `${kind}: busy cannot expand choices`);
    view.deviceList.emit('activate', 2);
    equal(backend.calls.length, before, `${kind}: busy activation is ignored`);
    update({ busy: false });
    await wait(260);

    const compact = bounds(panel.card, panel.root), inputBefore = bounds(panel.audio.input.heading);
    const triggerBefore = bounds(trigger);
    input('click', trigger);
    await wait(260);
    expanded(view, true, `${kind}: real pointer expands inline choices`);
    const grown = bounds(panel.card, panel.root), listBounds = bounds(options);
    assert(grown.height > compact.height, `${kind}: expansion grows the actual card`);
    boundsEqual(bounds(trigger), triggerBefore, `${kind}: expansion does not move the trigger inside the card`);
    assert(bindings.length >= entries.length, `${kind}: real inline factory binds all three rows`);
    const labels = listLabels(view);
    equal(labels.length, entries.length, `${kind}: all small-list choices have real native labels`);
    for (const widget of labels) {
        const row = bounds(widget.get_parent());
        assert(row.y >= listBounds.y && row.bottom <= listBounds.bottom,
            `${kind}: actual native row lies inside the inline scroller`);
        if (kind === 'output') assert(row.bottom <= bounds(panel.audio.input.heading).y,
            'Actual output row bounds do not overlap input controls');
        noInheritedTooltip(widget, `${kind}: expanded labels and ancestors have no tooltip`);
    }
    if (kind === 'output') {
        geometryEqual(bounds(panel.audio.input.heading).y - inputBefore.y, grown.height - compact.height,
            'Output list pushes the actual input section down by the card growth');
        assert(bounds(panel.audio.input.heading).y >= listBounds.bottom,
            'Output choice rows end before the input section, never overlay it');
    } else {
        boundsEqual(bounds(panel.audio.input.heading), inputBefore, 'Input expansion leaves the upper input controls in place');
        assert(grown.y + grown.height > compact.y + compact.height, 'Input choices expand the lower frame edge');
    }
    allocation(`${kind}: inline expansion stays content-sized`);
    input('click', trigger);
    await wait(260);
    expanded(view, false, `${kind}: clicking the active toggle collapses choices`);
    equal(bounds(panel.card, panel.root), compact, `${kind}: toggling closed exactly restores the compact frame`);
    boundsEqual(bounds(panel.audio.input.heading), inputBefore, `${kind}: toggling closed restores input placement within subpixel rounding`);
    equal(backend.calls.length, before, `${kind}: toggling choices never selects a device`);
    input('click', trigger);
    await wait(260);
    input('click', listLabels(view)[2]);
    await wait(260);
    equal(backend.calls.slice(before), [['select', kind, entries[2].key]], `${kind}: real pointer activates the exact row key`);
    expanded(view, false, `${kind}: real pointer selection collapses choices`);
    focused(trigger, `${kind}: pointer selection returns focus to the trigger`);
    equal(bounds(panel.card, panel.root), compact, `${kind}: closing exactly restores the compact frame`);
    boundsEqual(bounds(panel.audio.input.heading), inputBefore, `${kind}: closing restores input placement within subpixel rounding`);
    assert(panel.opened, `${kind}: selecting an inline row does not dismiss the panel`);

    // Description-only replacements must rebind native rows, not leave stale metadata.
    before = backend.calls.length;
    const oldModel = selection.model, oldBinds = bindings.length;
    const rebound = backend.state[kind].choices.map(item => ({ ...item, description: `Rebound <b>${item.key}</b>` }));
    update({ choices: rebound });
    assert(selection.model !== oldModel, `${kind}: metadata replacement replaces the native string model`);
    equal(backend.calls.length, before, `${kind}: replacing model and authoritative selection is read-only`);
    input('click', trigger);
    await wait(260);
    assert(bindings.length > oldBinds, `${kind}: metadata replacement rebinds the actual list factory`);
    noInheritedTooltip(view.deviceLabel, `${kind}: rebound trigger remains tooltip-free`);
    assert(listLabels(view)[0].get_parent().grab_focus(), `${kind}: native first row takes keyboard focus`);
    // Use actual Gtk.ListView keyboard navigation, not selection notifications.
    input('key', host, 'Home');
    await wait();
    input('key', host, 'Down');
    await wait();
    equal(backend.calls.length, before, `${kind}: real keyboard navigation alone never activates a device`);
    input('key', host, 'Return');
    await wait(260);
    equal(backend.calls.slice(before), [['select', kind, rebound[1].key]], `${kind}: real keyboard picks the duplicate by key`);
    expanded(view, false, `${kind}: keyboard selection collapses inline choices`);
    focused(trigger, `${kind}: keyboard selection returns focus to the trigger`);
    equal(bounds(panel.card, panel.root), compact, `${kind}: keyboard selection exactly restores the compact frame`);
    assert(panel.opened, `${kind}: keyboard activation leaves the panel open`);
    before = backend.calls.length;
    input('click', trigger);
    await wait(260);
    input('click', listLabels(view)[0]);
    await wait(260);
    equal(backend.calls.length, before, `${kind}: choosing the existing default does not mutate`);
    expanded(view, false, `${kind}: choosing the existing default still collapses`);
    focused(trigger, `${kind}: existing-default selection focuses the trigger`);

    for (const [properties, placeholder] of [
        [{ available: true, name: 'Unlisted <native> & name' }, 'Unlisted <native> & name'],
        [{ available: false, nodeKey: '', name: 'Stale name' }, 'Select a device…'],
    ]) {
        update({ ...properties, choices: rebound.map(item => ({ ...item, default: false })) });
        equal(selection.model.get_n_items(), rebound.length + 1, `${kind}: missing default prepends one placeholder`);
        equal([selection.selected, selection.model.get_string(0), view.choiceItems[0].key, view.deviceLabel.label],
            [0, placeholder, '', placeholder], `${kind}: placeholder is selected and non-actionable`);
        selection.notify('selected');
        view.deviceList.emit('activate', 0);
        equal(backend.calls.length, before, `${kind}: placeholder cannot call select`);
    }
    update({ available: true, nodeKey: `current:${kind}`, choices: initial });
    panel.setDeviceExpanded(view, true);
    update({ choices: [initial[1]] });
    assert(!trigger.visible && !view.devicesHeading.visible && !trigger.sensitive,
        `${kind}: sole default hides selector and Devices heading`);
    expanded(view, false, `${kind}: hotplug leaving no alternatives collapses choices`);
    panel.setDeviceExpanded(view, true);
    expanded(view, false, `${kind}: hidden selector cannot expand`);
    update({ available: false, nodeKey: '', choices: [] });
    assert(!trigger.visible && !view.devicesHeading.visible && !view.mute.sensitive && !view.volume.sensitive,
        `${kind}: empty hardware leaves no old rows or active controls`);
    equal(selection.model.get_n_items(), 1, `${kind}: removed choices are absent from the native model`);
    equal(backend.calls.length, before, `${kind}: hotplug/placeholder renders never mutate`);
    update({ available: true, nodeKey: `current:${kind}`, choices: initial, name: `${kind} default` });
    await wait(260);
    assert(bindings.length > 0 && bindings.every(item => item.literal && item.tooltipFree),
        `${kind}: every factory binding is literal, ellipsized and tooltip-free, including after rebinding`);
    factory.disconnect(bindSignal);
}

async function testVolume(kind) {
    const view = panel.audio[kind], scale = view.volume, bar = view.volumeBar.widget;
    const backward = bar.get_first_child(), forward = bar.get_last_child();
    const original = { ...backend.state[kind] };
    const noVolumeTooltips = message => {
        for (const control of [view.mute, backward, scale, forward, bar])
            noInheritedTooltip(control, `${kind}: ${message} has no control or ancestor tooltip`);
        for (const control of [view.mute, backward, forward])
            for (const child of walk(control)) noTooltip(child, `${kind}: ${message} has no button-child tooltip`);
    };
    const update = properties => {
        backend.state[kind] = { ...backend.state[kind], ...properties };
        panel.render();
    };
    const geometry = message => {
        equal([bar.get_allocated_height(), scale.get_height(), backward.get_height(), forward.get_height()],
            [20, 20, 20, 20], `${kind}: ${message} keeps the compact flat 20px bar`);
        const [start, end] = scale.get_slider_range();
        const [x, y] = scale.get_layout_offsets();
        const [, text] = scale.get_layout().get_pixel_extents();
        equal(end - start, 43, `${kind}: ${message} retains the compact native thumb with the thin button-matched outline`);
        assert(x >= start && x + text.width <= end && y >= 0 && y + text.height <= scale.get_height(),
            `${kind}: ${message} keeps the native percent inside the native thumb`);
    };
    assert(scale instanceof Gtk.Scale && backward instanceof Gtk.Button && forward instanceof Gtk.Button,
        `${kind}: existing native scale between native stepper buttons`);
    assert(backward.get_next_sibling() === scale && scale.get_next_sibling() === forward,
        `${kind}: no standalone percent widget`);
    assert(!('percent' in view), `${kind}: obsolete view.percent is removed`);
    noVolumeTooltips('constructed volume bar');
    for (const control of [backward, forward]) {
        assert(control.has_css_class('volume-step') && !control.has_css_class('control-button'),
            `${kind}: arrows do not inherit unrelated button sizing`);
        equal(control.get_width(), 20, `${kind}: arrow is 20px wide`);
    }
    equal([backward.get_child().label, forward.get_child().label], ['◁', '▷'], `${kind}: hollow arrows`);
    equal(scale.get_adjustment().step_increment, 0.01, `${kind}: native 1% increment is unchanged`);
    equal([scale.get_adjustment().lower, scale.get_adjustment().upper], [0, view.maximum], `${kind}: native volume limits`);
    equal(scale.round_digits, -1, `${kind}: displaying the percent does not quantize native volume`);
    let before = backend.calls.length;
    // Every percentage, including both endpoints and output amplification, uses
    // only GTK's value layout and slider allocation, never custom geometry.
    for (let percent = 0; percent <= view.maximum * 100; percent++) {
        update({ volume: percent / 100 });
        await wait(20);
        equal(scale.get_layout().get_text(), `${percent}%`, `${kind}: native label at ${percent}%`);
        geometry(`${percent}%`);
    }
    update({ volume: 0.3749 });
    equal(scale.get_value(), 0.3749, `${kind}: native fractional volume is preserved`);
    equal(scale.get_layout().get_text(), '37%', `${kind}: only displayed text rounds`);
    for (const available of [false, true, false, true]) {
        update({ available });
        await wait(30);
        equal(scale.get_layout().get_text(), available ? '37%' : '—', `${kind}: same-value availability invalidates cached text`);
        equal([bar.sensitive, backward.is_sensitive(), forward.is_sensitive()],
            [available, available, available], `${kind}: availability covers the entire bar`);
        geometry(available ? 'available' : 'disabled');
        noVolumeTooltips(available ? 'enabled rerender' : 'disabled rerender');
    }
    for (const flags of [Gtk.StateFlags.PRELIGHT, Gtk.StateFlags.ACTIVE]) {
        scale.set_state_flags(flags, false);
        await wait(30);
        geometry(`native state ${flags}`);
        scale.unset_state_flags(flags);
    }
    scale.grab_focus();
    await wait(30);
    geometry('focused');
    view.dragging = true;
    view.volumeTarget = view.nodeKey;
    scale.set_value(0.63);
    update({ volume: 0.42 });
    equal(scale.get_value(), 0.63, `${kind}: native updates remain suppressed during an owned drag`);
    equal(scale.get_layout().get_text(), '63%', `${kind}: native label follows the live drag, not the suppressed snapshot`);
    equal(backend.calls.length, before, `${kind}: all volume rendering and allocation checks are read-only`);
    view.dragging = false;
    view.volumeTarget = null;
    update({ volume: 0.4 });
    let moves = 0;
    const moveSignal = scale.connect('move-slider', () => { moves++; });
    for (const [name, disabled] of [['scale', scale], ['ancestor', bar.get_parent()]]) {
        const value = scale.get_value(), target = `disabled:${kind}:${name}`;
        view.dragging = true;
        view.volumeTarget = target;
        disabled.sensitive = false;
        try {
            noVolumeTooltips(`disabled ${name}`);
            equal(scale.sensitive, name === 'ancestor', `${kind}: disabling ${name} preserves the scale's own sensitivity semantics`);
            assert(!scale.is_sensitive(), `${kind}: disabled ${name} makes the native scale effectively insensitive`);
            backward.emit('clicked');
            forward.emit('clicked');
            equal(moves, 0, `${kind}: disabled ${name} blocks both native move-slider emissions`);
            equal([view.dragging, view.volumeTarget], [true, target], `${kind}: disabled ${name} cannot clear drag ownership`);
            equal(scale.get_value(), value, `${kind}: disabled ${name} cannot change the native value`);
            equal(backend.calls.length, before, `${kind}: disabled ${name} cannot mutate the backend`);
        } finally { disabled.sensitive = true; }
    }
    scale.disconnect(moveSignal);
    view.dragging = false;
    view.volumeTarget = null;
    const expectVolume = (value, message) => {
        const calls = backend.calls.slice(before);
        equal(calls.length, 1, `${kind}: ${message} makes one guarded mutation`);
        equal([calls[0][0], calls[0][1], calls[0][3]], ['volume', kind, view.nodeKey], `${kind}: ${message} uses the current native identity`);
        assert(Math.abs(calls[0][2] - value) < 1e-9, `${kind}: ${message} uses the native step increment`);
        before = backend.calls.length;
    };
    for (const [control, value, description] of [[forward, 0.41, 'Increase volume'], [backward, 0.4, 'Decrease volume']]) {
        view.dragging = true;
        view.volumeTarget = 'stale-drag-owner';
        control.emit('clicked');
        expectVolume(value, description);
        assert(!view.dragging && view.volumeTarget === null, `${kind}: step clears old drag ownership before the native signal`);
    }
    input('click', forward);
    await wait();
    expectVolume(0.41, 'real pointer step');
    scale.grab_focus();
    view.dragging = true;
    view.volumeTarget = 'stale-keyboard-owner';
    input('key', host, 'Right');
    await wait();
    expectVolume(0.42, 'real keyboard step');
    view.dragging = true;
    view.volumeTarget = 'stale-scroll-owner';
    input('scroll', scale);
    await wait();
    assert(backend.calls.length > before && backend.calls.slice(before).every(call =>
        call[0] === 'volume' && call[1] === kind && call[3] === view.nodeKey), `${kind}: native scroll uses the guarded current identity`);
    assert(!view.dragging && view.volumeTarget === null, `${kind}: scroll clears stale drag ownership`);
    before = backend.calls.length;
    input('click', view.mute);
    await wait();
    equal(backend.calls.slice(before), [['mute', kind, view.nodeKey]], `${kind}: native mute control is unchanged`);
    noVolumeTooltips('mute rerender');
    before = backend.calls.length;
    for (const configure of [
        () => { panel.opened = false; },
        () => { panel.currentTab = 0; },
        () => { backend.state[kind] = { ...original, available: false }; },
        () => { backend.state[kind] = { ...original, nodeKey: 'unrendered-replacement' }; },
    ]) {
        update(original);
        configure();
        forward.emit('clicked');
        equal(backend.calls.length, before, `${kind}: arrow cannot bypass nodeCurrent availability/ownership/lifecycle guards`);
        panel.opened = true;
        panel.currentTab = 1;
    }
    update({ ...original, volume: 0.4 });
    input('press', scale);
    await wait();
    assert(view.dragging && view.volumeTarget === view.nodeKey, `${kind}: real native drag captures its original node`);
    before = backend.calls.length;
    update({ nodeKey: 'replacement-during-drag', volume: 0.2 });
    equal(scale.get_value(), 0.2, `${kind}: new native identity is not suppressed by the old drag`);
    input('move', forward);
    input('release', forward);
    await wait();
    equal(backend.calls.length, before, `${kind}: old native drag cannot mutate the replacement node`);
    assert(!view.dragging, `${kind}: native release ends dragging`);
    view.volumeTarget = null;
    for (const [value, control] of [[0, backward], [view.maximum, forward]]) {
        update({ volume: value });
        before = backend.calls.length;
        control.emit('clicked');
        equal(scale.get_value(), value, `${kind}: native step clamps at ${value * 100}%`);
        assert(backend.calls.slice(before).every(call => call[2] === value), `${kind}: step never writes outside the native range`);
    }
    update(original);
    await wait();
    allocation(`${kind}: volume bar does not resize the frame`);
}

function testButtonTooltips() {
    const original = backend.state.bluetooth;
    const device = { key: 'fake-headset', name: 'Fake <headset>', address: 'AA:01', paired: true };
    const before = backend.calls.length;
    try {
        for (const model of [
            {},
            { pairing: true, pairingMessage: { text: 'Pairing…', prompt: 'Enter PIN:' } },
            { devices: [{ ...device, busy: true }] },
            { powered: false, devices: [{ ...device, name: 'Renamed fake headset' }] },
            {},
        ]) {
            backend.state.bluetooth = { available: true, powered: true, devices: [device], ...model };
            panel.render();
            const row = panel.deviceRows.get(device.key);
            for (const control of [...panel.tabs, panel.dismissPairing, panel.send, panel.cancel, row.action, row.forget]) {
                noInheritedTooltip(control, 'Constructed button and its ancestors have no tooltip after rerender');
                for (const child of walk(control)) noTooltip(child, 'Button children do not introduce tooltips');
            }
            equal(row.name.tooltip_text, backend.state.bluetooth.devices[0].name, 'Bluetooth name metadata tooltip is preserved');
            for (const widget of walk(panel.root).filter(widget => widget instanceof Gtk.Button))
                noTooltip(widget, 'Every constructed native button has no tooltip of its own');
        }
        equal(backend.calls.length, before, 'Button tooltip regression checks never invoke backend operations');
    } finally {
        backend.state.bluetooth = original;
        panel.render();
    }
}

async function testSelectorGuards() {
    const view = panel.audio.output, other = panel.audio.input;
    const original = backend.state.output;
    const before = backend.calls.length;
    panel.setDeviceExpanded(view, true);
    panel.setDeviceExpanded(other, true);
    expanded(view, false, 'Opening input collapses output');
    expanded(other, true, 'Only the input list remains expanded');
    panel.setDeviceExpanded(view, true);
    expanded(other, false, 'Opening output collapses input');
    panel.setTab(0);
    for (const item of Object.values(panel.audio)) expanded(item, false, 'Tab reset collapses every list');
    panel.setDeviceExpanded(view, true);
    expanded(view, false, 'Bluetooth tab cannot expand an audio selector');
    view.deviceList.emit('activate', 0);
    panel.setTab(1);
    for (const item of Object.values(panel.audio)) expanded(item, false, 'Returning to Audio does not reopen choices');
    panel.setDeviceExpanded(view, true);
    panel.close();
    for (const item of Object.values(panel.audio)) expanded(item, false, 'Panel close resets every selector');
    panel.setDeviceExpanded(view, true);
    expanded(view, false, 'Closed panel cannot expand choices');
    view.deviceList.emit('activate', 0);
    panel.opened = true;
    panel.render();
    equal(backend.calls.length, before, 'Expansion, tab and closed-panel guards never mutate audio');

    // Keep the rendered keys stale deliberately: activation must validate the live snapshot.
    panel.setDeviceExpanded(view, true);
    const stale = view.choiceItems[0].key;
    backend.state.output = { ...original, choices: original.choices.filter(item => item.key !== stale) };
    view.deviceList.emit('activate', 0);
    equal(backend.calls.length, before, 'A removed native key never reaches backend.select');
    assert(view.error.visible && /no longer available/i.test(view.error.label),
        'Stale native keys report through the existing action error path');
    expanded(view, false, 'Stale selection collapses choices');
    focused(view.devices, 'Stale selection returns focus to the trigger');
    backend.state.output = original;
    panel.failures.output = '';
    panel.render();
    panel.setDeviceExpanded(view, true);
    backend.state.output = { ...original, busy: true };
    view.deviceList.emit('activate', 0);
    equal(backend.calls.length, before, 'Live busy state blocks activation before a rerender');
    backend.state.output = original;
    panel.render();
    panel.setDeviceExpanded(view, true);
    backend.state.output = { ...original, choices: original.choices.map((item, i) => ({ ...item, default: i === 0 })) };
    view.deviceList.emit('activate', 0);
    equal(backend.calls.length, before, 'A live default change prevents a redundant stale-view mutation');
    expanded(view, false, 'A newly authoritative default still collapses choices');
    focused(view.devices, 'A newly authoritative default returns focus to the trigger');
    backend.state.output = original;
    panel.render();
    panel.setDeviceExpanded(view, true);
    backend.selectionError = 'Fake native device selection failed';
    try {
        view.deviceList.emit('activate', 0);
        equal(backend.calls.slice(before), [['select', 'output', original.choices[0].key]],
            'A failing selection dispatches the validated native key once');
        assert(view.error.visible && view.error.label === backend.selectionError,
            'Backend selection errors use the existing visible action failure');
        expanded(view, false, 'A failed selection collapses choices');
        focused(view.devices, 'A failed selection returns focus to the trigger');
    } finally {
        backend.selectionError = '';
        panel.failures.output = '';
        panel.render();
    }
    for (const position of [Gtk.INVALID_LIST_POSITION, original.choices.length + 10])
        view.deviceList.emit('activate', position);
    equal(backend.calls.length, before + 1, 'Invalid native list positions never dispatch mutations');
    await wait(260);
}

async function testLargeInlineLists() {
    const monitor = panel.monitor, originals = { output: backend.state.output, input: backend.state.input };
    const before = backend.calls.length;
    try {
        for (const kind of ['output', 'input']) {
            const view = panel.audio[kind], seen = new Set(), bindings = [];
            const factory = view.deviceList.factory;
            const signal = factory.connect('bind', (_, item) => {
                const widget = item.get_child();
                seen.add(widget.label);
                bindings.push({ literal: !widget.use_markup && widget.ellipsize === Pango.EllipsizeMode.END,
                    tooltipFree: !widget.has_tooltip && !widget.tooltip_text && !widget.tooltip_markup });
            });
            try {
                for (const short of [false, true]) {
                    const entries = Array.from({ length: 40 }, (_, i) => ({ key: `native:${kind}:inline:${i}`,
                        label: `Inline ${kind} choice ${i} <b>& literal</b> ` + 'long name '.repeat(12),
                        description: `Inline ${kind} route ${i} <literal>`, default: i === 1 }));
                    seen.clear();
                    panel.monitor = short ? { get_geometry: () => ({ width: 340, height: 300 }) } : monitor;
                    backend.state[kind] = { ...originals[kind], choices: entries };
                    panel.render();
                    await wait(260);
                    await scrollIntoView(view.devices, panel.audioPage);
                    input('click', view.devices);
                    await wait(260);
                    expanded(view, true, `${kind}: large list expands on a ${short ? 'short' : 'normal'} monitor`);
                    const options = view.deviceChoices.get_child(), adjustment = options.get_vadjustment();
                    equal(options.max_content_height, 240, `${kind}: large-list viewport retains its native 240px cap`);
                    assert(adjustment.page_size > 0 && adjustment.page_size <= 240
                        && adjustment.upper > adjustment.page_size,
                                            `${kind}: many devices scroll inside the capped list (upper=${adjustment.upper}, page=${adjustment.page_size}, options=${options.get_height()}, card=${panel.card.get_height()})`);
                    assert(panel.card.get_height() <= (short ? 236 : 520), `${kind}: expanded list respects the monitor budget`);
                    equal(panel.card.get_width(), short ? 324 : 400, `${kind}: long inline names cannot widen the card`);
                    if (short) {
                        const page = panel.audioPage.get_vadjustment();
                        assert(page.upper > page.page_size, `${kind}: short monitor also scrolls the whole audio page`);
                    }
                    // Visit the entire native scroll range, including recycled factory rows.
                    const maximum = adjustment.upper - adjustment.page_size;
                    for (let value = 0; value < maximum; value += Math.max(1, adjustment.page_size / 2)) {
                        adjustment.value = value;
                        await wait(30);
                        for (const widget of listLabels(view)) noInheritedTooltip(widget, `${kind}: large-list labels remain tooltip-free`);
                    }
                    adjustment.value = maximum;
                    await wait();
                    const last = listLabels(view).find(widget => widget.label === entries.at(-1).label);
                    assert(last, `${kind}: native scrolling reaches the last device`);
                    const row = bounds(last.get_parent(), options);
                    assert(row.y >= 0 && row.bottom <= options.get_height(), `${kind}: last native row is allocated inside the capped viewport`);
                    assert(entries.every(item => seen.has(item.label)), `${kind}: every large-list label was bound without a tooltip`);
                    panel.setDeviceExpanded(view, false);
                    await wait(260);
                    expanded(view, false, `${kind}: large list collapses within the frame`);
                    noDevicePopups();
                    backend.state[kind] = originals[kind];
                    panel.render();
                    await wait(260);
                }
                assert(bindings.length > 0 && bindings.every(item => item.literal && item.tooltipFree),
                    `${kind}: all large-list and recycled bindings are literal, ellipsized and tooltip-free`);
            } finally { factory.disconnect(signal); }
        }
    } finally {
        panel.monitor = monitor;
        for (const kind of ['output', 'input']) {
            backend.state[kind] = originals[kind];
            panel.setDeviceExpanded(panel.audio[kind], false);
        }
        panel.render();
    }
    await wait(260);
    equal(backend.calls.length, before, 'Large-list layout and scrolling never mutate devices');
    allocation('Large-list cleanup restores the compact frame');
}

async function testRootDismissal() {
    const view = panel.audio.output, before = backend.calls.length;
    const compact = bounds(panel.card, panel.root);
    const activity = backend.activity.length;
    input('click', view.devices);
    await wait(260);
    assert(listLabels(view)[0].get_parent().grab_focus(), 'Escape starts with focus inside a real list row');
    input('key', host, 'Escape');
    await wait(260);
    expanded(view, false, 'First real Escape collapses the open list');
    focused(view.devices, 'First Escape focuses the trigger');
    assert(panel.opened && backend.activity.length === activity, 'First Escape is consumed before panel close');
    equal(bounds(panel.card, panel.root), compact, 'Escape exactly restores compact geometry');
    input('key', host, 'Escape');
    await wait(260);
    assert(!panel.opened && backend.activity.length === activity + 1 && backend.activity.at(-1)?.[0] === false,
        'Second real Escape reaches the hosted root controller and closes the panel');
    for (const item of Object.values(panel.audio)) expanded(item, false, 'Escape close leaves every selector collapsed');
    panel.opened = true;
    panel.render();
    await wait(260);
    input('click', panel.root);
    await wait(260);
    assert(!panel.opened && backend.activity.at(-1)?.[0] === false,
        'An actual root click outside the card still closes the panel');
    equal(backend.calls.length, before, 'Escape and outside dismissal never mutate audio');
}

async function smoke() {
    const services = ['org.bluez', 'org.PulseAudio1', 'org.freedesktop.NetworkManager'];
    assert(services.every(name => !owner(name)), 'Private bus has no live audio/Bluetooth/network services');
    Gtk.init();
    GLib.unsetenv('LD_PRELOAD');
    const palette = loadTheme(entry, GLib.getenv('AUDIO_PANEL_PALETTE'));
    assert(Boolean(palette.yellow), 'Actual custom CSS parses in GTK without parsing errors');
    const css = read(GLib.build_filenamev([GLib.path_get_dirname(entry), 'style.css']));
    assert(/background(?:-color)?:\s*alpha\(\s*shade\(\s*@ap_red\s*,\s*0?\.45\s*\)\s*,\s*0?\.5\s*\)/.test(css),
        'Inline selector stylesheet uses the requested half-transparent dark-red shade');
    app = new Astal.Application({ instance_name: 'audio-dropdown-offline' });
    app.register(null);
    assert(!app.get_is_remote(), 'Application instance belongs to the private bus');
    panel = new AudioPanel({ app, backend, palette });
    assert(!panel.window.visible && backend.starts === 0 && backend.calls.length === 0,
        'Real AudioPanel constructs hidden with fake snapshot only');
    // Never present the layer-shell window on X11; retain the production root
    // and its capture-phase key/click-outside controllers for real XTEST input.
    panel.window.set_child(null);
    host = new Gtk.Window({ child: panel.root, decorated: false, default_width: 1280,
        default_height: 800, css_classes: ['audio-panel'] });
    panel.opened = true;
    panel.monitor = { get_geometry: () => ({ width: 1280, height: 800 }) };
    panel.render();
    host.present();
    await wait(200);
    for (const tab of panel.tabs) {
        const rgb = (tab.has_css_class('selected') ? palette.cyan : palette.pink)
            .slice(1).match(/../g).map(value => parseInt(value, 16) / 255);
        for (const state of [Gtk.StateFlags.NORMAL, Gtk.StateFlags.PRELIGHT, Gtk.StateFlags.FOCUS_VISIBLE]) {
            tab.set_state_flags(state, false);
            await wait(150);
            const color = tab.get_child().get_color();
            assert([color.red, color.green, color.blue].every((value, index) => Math.abs(value - rgb[index]) < 0.01),
                'Selected and inactive tabs must retain their text colors at rest, on hover and keyboard focus');
            tab.unset_state_flags(state);
        }
    }
    await testCompactLayout();
    for (const kind of ['output', 'input']) await testKind(kind);
    await testSelectorGuards();
    await testLargeInlineLists();
    assert(backend.calls.every(call => call[0] === 'select'), 'Selector rendering never invokes volume/mute mutations');
    for (const kind of ['output', 'input']) await testVolume(kind);
    testButtonTooltips();
    await testRootDismissal();
    allocation('Final real GTK frame remains content-sized');
    assert(backend.starts === 0 && !panel.window.visible, 'Backend never started and Astal window stayed hidden');

    assert(services.every(name => !owner(name)), 'No live service appeared during the fake-only test');
    print(`PASS audio inline-selector/volume GTK smoke: ${checks} assertions; fake backend only; actual source, CSS, native scale geometry, inline factories, capped scrolling and root XTEST Escape`);
    print('LIMITATION Xvfb does not validate Wayland layer-shell placement, exclusive key grabs, accessibility announcements, or tooltip painting.');
}

let exitCode = 0;
try { await smoke(); }
catch (error) { console.error(`FAIL audio inline-selector/volume GTK smoke after ${checks} assertions`, error); exitCode = 1; }
finally {
    host?.destroy();
    panel?.dispose();
    app?.quit();
}
System.exit(exitCode);
