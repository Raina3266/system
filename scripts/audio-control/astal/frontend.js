import GLib from 'gi://GLib';
import GObject from 'gi://GObject';
import Gtk from 'gi://Gtk?version=4.0';
import Gdk from 'gi://Gdk?version=4.0';
import Pango from 'gi://Pango';
import Astal from 'gi://Astal?version=4.0';
import AstalIO from 'gi://AstalIO';
import System from 'system';
import { AudioBackend } from './backend.js';
import { parseRequest } from './logic.js';

function read(path) {
    if (!path) throw new Error('Run through audio-panel, or set AUDIO_PANEL_CONFIG and AUDIO_PANEL_PALETTE.');
    const [ok, contents] = GLib.file_get_contents(path);
    if (!ok) throw new Error(`Could not read audio theme: ${path}`);
    return new TextDecoder().decode(contents);
}

function loadTheme(entry, palettePath) {
    const palette = JSON.parse(read(palettePath));
    for (const key of ['yellow', 'cyan', 'pink', 'red', 'dimPink', 'separator', 'foreground', 'background', 'secondaryBackground', 'chromeBackground'])
        if (!/^#[a-f\d]{6}$/i.test(palette[key])) throw new Error(`Invalid audio palette colour: ${key}`);
    const definitions = Object.entries(palette)
        .filter(([key, value]) => /^[a-zA-Z]+$/.test(key) && /^#[a-f\d]{6}$/i.test(value))
        .map(([key, value]) => `@define-color ap_${key} ${value};`).join('\n');
    const css = `${definitions}\n${read(GLib.build_filenamev([GLib.path_get_dirname(entry), 'style.css']))}`;
    const provider = new Gtk.CssProvider();
    const errors = [];
    provider.connect('parsing-error', (_, section, error) => errors.push(error.message));
    provider.load_from_string(css);
    if (errors.length) throw new Error(`Audio stylesheet: ${errors.join('; ')}`);
    // The desktop stylesheet has USER priority; override it only inside this panel.
    Gtk.StyleContext.add_provider_for_display(Gdk.Display.get_default(), provider, Gtk.STYLE_PROVIDER_PRIORITY_USER + 1);
    return palette;
}

function box(vertical = false, spacing = 0, children = []) {
    const widget = new Gtk.Box({ orientation: vertical ? Gtk.Orientation.VERTICAL : Gtk.Orientation.HORIZONTAL, spacing });
    children.forEach(child => widget.append(child));
    return widget;
}

function label(text = '', classes = []) {
    return new Gtk.Label({ label: text, css_classes: classes, xalign: 0, use_markup: false });
}

function ellipsis(text = '', classes = []) {
    return new Gtk.Label({ label: text, css_classes: classes, xalign: 0, use_markup: false,
        ellipsize: Pango.EllipsizeMode.END, max_width_chars: 1, hexpand: true });
}

function wrapped(text = '', classes = []) {
    return new Gtk.Label({ label: text, css_classes: classes, xalign: 0, use_markup: false,
        wrap: true, wrap_mode: Pango.WrapMode.WORD_CHAR, max_width_chars: 1, hexpand: true });
}

function button(text, clicked, classes = []) {
    const widget = new Gtk.Button({ css_classes: ['control-button', ...classes], valign: Gtk.Align.CENTER,
        child: new Gtk.Label({ label: text, use_markup: false, ellipsize: Pango.EllipsizeMode.END }) });
    widget.connect('clicked', clicked);
    return widget;
}

function scroll(child, properties = {}) {
    return new Gtk.ScrolledWindow({ child, hscrollbar_policy: Gtk.PolicyType.NEVER,
        vscrollbar_policy: Gtk.PolicyType.AUTOMATIC, min_content_width: 0, min_content_height: 0,
        propagate_natural_width: false, propagate_natural_height: false, hexpand: true, ...properties });
}

function style(widget, name, enabled) {
    if (enabled) widget.add_css_class(name);
    else widget.remove_css_class(name);
}

function message(widget, text) {
    widget.set_label(text || '');
    widget.visible = Boolean(text);
}

function volumeValue(value, maximum) {
    return Number.isFinite(value) ? Math.min(maximum, Math.max(0, value)) : 0;
}

function frame(palette) {
    const drawing = new Gtk.DrawingArea({ can_target: false, hexpand: true, vexpand: true });
    drawing.set_draw_func((_, cr, width, height) => {
        const cut = Math.min(10, width / 2, height / 2), inset = 0.5;
        cr.moveTo(cut + inset, inset);
        cr.lineTo(width - inset, inset);
        cr.lineTo(width - inset, height - cut - inset);
        cr.lineTo(width - cut - inset, height - inset);
        cr.lineTo(inset, height - inset);
        cr.lineTo(inset, cut + inset);
        cr.closePath();
        cr.setSourceRGBA(37 / 255, 37 / 255, 47 / 255, 0.9);
        cr.fillPreserve();
        const rgb = palette.yellow.slice(1).match(/../g).map(part => parseInt(part, 16) / 255);
        cr.setSourceRGBA(...rgb, 1);
        cr.setLineWidth(1);
        cr.stroke();
    });
    return drawing;
}

export class AudioPanel {
    constructor({ app, backend, palette }) {
        this.backend = backend;
        this.opened = false;
        this.currentTab = 1;
        this.monitor = null;
        this.monitorName = '';
        this.monitorSignal = 0;
        this.audio = {};
        this.deviceRows = new Map();
        this.failures = { output: '', input: '', bluetooth: '' };
        this.pairing = false;
        this.cardWidth = 400;
        this.window = new Astal.Window({ application: app, name: 'audio-panel', namespace: 'audio-control',
            anchor: Astal.WindowAnchor.TOP | Astal.WindowAnchor.BOTTOM | Astal.WindowAnchor.LEFT | Astal.WindowAnchor.RIGHT,
            exclusivity: Astal.Exclusivity.IGNORE, layer: Astal.Layer.OVERLAY,
            keymode: Astal.Keymode.NONE, visible: false });
        this.window.add_css_class('audio-panel');
        this.window.connect('close-request', () => { this.close(); return true; });
        const keys = new Gtk.EventControllerKey({ propagation_phase: Gtk.PropagationPhase.CAPTURE });
        keys.connect('key-pressed', (_, key) => {
            if (key !== Gdk.KEY_Escape) return false;
            this.close();
            return true;
        });
        this.window.add_controller(keys);
        this.root = new Gtk.Overlay({ hexpand: true, vexpand: true });
        this.root.set_child(new Gtk.Box({ hexpand: true, vexpand: true }));
        this.window.set_child(this.root);
        this.card = new Gtk.Overlay({ width_request: 400, height_request: 520,
            halign: Gtk.Align.END, valign: Gtk.Align.START, margin_top: 40, margin_end: 96,
            overflow: Gtk.Overflow.HIDDEN, css_classes: ['audio-card'] });
        this.card.set_child(frame(palette));
        this.content = box(true);
        this.content.add_css_class('audio-content');
        this.card.add_overlay(this.content);
        // Neither a long device name nor a page's natural height may resize the frame.
        this.card.set_measure_overlay(this.content, false);
        this.root.add_overlay(this.card);
        this.root.set_measure_overlay(this.card, false);
        const outside = new Gtk.GestureClick({ propagation_phase: Gtk.PropagationPhase.CAPTURE });
        outside.connect('released', (_, count, x, y) => {
            const [ok, bounds] = this.card.compute_bounds(this.root);
            if (ok && (x < bounds.get_x() || x > bounds.get_x() + bounds.get_width()
                || y < bounds.get_y() || y > bounds.get_y() + bounds.get_height())) this.close();
        });
        this.root.add_controller(outside);
        const release = new Gtk.EventControllerLegacy({ propagation_phase: Gtk.PropagationPhase.CAPTURE });
        release.connect('event', (_, event) => {
            if ([Gdk.EventType.BUTTON_RELEASE, Gdk.EventType.TOUCH_END, Gdk.EventType.TOUCH_CANCEL].includes(event.get_event_type())) {
                for (const view of Object.values(this.audio)) view.dragging = false;
                // Keep the captured key through GTK's final release-time value change.
            }
            return false;
        });
        this.root.add_controller(release);
        this.build(palette);
        this.monitors = Gdk.Display.get_default().get_monitors();
        this.monitorsSignal = this.monitors.connect('items-changed', () => {
            for (let index = 0; index < this.monitors.get_n_items(); index++)
                if (this.monitors.get_item(index) === this.monitor) return;
            if (this.opened) this.close();
        });
        this.render();
    }

    build(palette) {
        const tabs = box();
        tabs.homogeneous = true;
        this.tabs = ['Bluetooth', 'Audio'].map((name, index) => {
            const tab = button(name.toUpperCase(), () => this.setTab(index), ['tab']);
            tab.hexpand = true;
            tab.height_request = 40;
            tabs.append(tab);
            return tab;
        });
        this.content.append(tabs);
        this.pageFrame = new Gtk.Overlay({ hexpand: true, vexpand: true });
        this.pageBorder = new Gtk.DrawingArea({ can_target: false, hexpand: true, vexpand: true });
        this.pageBorder.set_draw_func((_, cr, width, height) => {
            const rgb = palette.cyan.slice(1).match(/../g).map(part => parseInt(part, 16) / 255);
            cr.setSourceRGBA(...rgb, 1);
            cr.setLineWidth(1);
            cr.moveTo(0.5, 0.5);
            cr.lineTo(0.5, height - 0.5);
            cr.lineTo(width - 0.5, height - 0.5);
            cr.lineTo(width - 0.5, 0.5);
            const left = width * this.currentTab / 2, right = width * (this.currentTab + 1) / 2;
            cr.moveTo(0, 0.5);
            cr.lineTo(left, 0.5);
            cr.moveTo(right, 0.5);
            cr.lineTo(width, 0.5);
            cr.stroke();
        });
        this.pageFrame.set_child(this.pageBorder);
        this.pages = new Gtk.Stack({ transition_type: Gtk.StackTransitionType.NONE,
            hhomogeneous: false, vhomogeneous: false, hexpand: true, vexpand: true,
            margin_top: 12, margin_bottom: 12, margin_start: 12, margin_end: 12 });
        this.pages.add_named(this.buildBluetooth(), 'bluetooth');
        const audio = box(true, 12, [this.buildAudio('output'),
            new Gtk.Separator({ css_classes: ['audio-divider'] }), this.buildAudio('input')]);
        audio.vexpand = true;
        this.pages.add_named(audio, 'audio');
        this.pageFrame.add_overlay(this.pages);
        this.pageFrame.set_measure_overlay(this.pages, false);
        this.content.append(this.pageFrame);
    }

    buildAudio(kind) {
        const view = { kind, maximum: kind === 'output' ? 1.5 : 1, rows: new Map(),
            nodeKey: '', volumeTarget: null, muteTarget: null, dragging: false };
        this.audio[kind] = view;
        view.heading = ellipsis('', ['audio-heading']);
        view.mute = button('', () => {
            const key = view.muteTarget ?? view.nodeKey;
            view.muteTarget = null;
            if (this.nodeCurrent(view, key)) this.action(kind, () => this.backend.toggleMute(kind, key));
        }, ['icon-button', 'mute-button']);
        this.press(view.mute, () => { view.muteTarget = view.nodeKey; });
        const muteKeys = new Gtk.EventControllerKey({ propagation_phase: Gtk.PropagationPhase.CAPTURE });
        muteKeys.connect('key-pressed', () => { view.muteTarget = view.nodeKey; return false; });
        view.mute.add_controller(muteKeys);
        view.volume = new Gtk.Scale({ orientation: Gtk.Orientation.HORIZONTAL, hexpand: true,
            draw_value: false, digits: 2, valign: Gtk.Align.CENTER, css_classes: ['volume-slider'],
            adjustment: new Gtk.Adjustment({ lower: 0, upper: view.maximum, step_increment: 0.01, page_increment: 0.05 }) });
        this.press(view.volume, () => { view.dragging = true; view.volumeTarget = view.nodeKey; });
        const volumeKeys = new Gtk.EventControllerKey({ propagation_phase: Gtk.PropagationPhase.CAPTURE });
        volumeKeys.connect('key-pressed', () => { view.dragging = false; view.volumeTarget = null; return false; });
        view.volume.add_controller(volumeKeys);
        const volumeScroll = new Gtk.EventControllerScroll({ flags: Gtk.EventControllerScrollFlags.BOTH_AXES,
            propagation_phase: Gtk.PropagationPhase.CAPTURE });
        volumeScroll.connect('scroll', () => { view.dragging = false; view.volumeTarget = null; return false; });
        view.volume.add_controller(volumeScroll);
        view.percent = label('0%', ['volume-value']);
        view.percent.xalign = 1;
        view.volume.connect('change-value', (_, scrollType, value) => {
            const key = view.volumeTarget ?? view.nodeKey;
            if (!this.nodeCurrent(view, key)) return true;
            const next = volumeValue(value, view.maximum);
            view.volume.set_value(next);
            view.percent.set_label(`${Math.round(next * 100)}%`);
            this.action(kind, () => this.backend.setVolume(kind, next, key));
            return true;
        });
        view.choices = box(true, 6);
        view.empty = wrapped('No available devices.', ['muted']);
        view.choices.append(view.empty);
        const choices = scroll(view.choices, { vexpand: true, min_content_height: 32 });
        view.error = wrapped('', ['failure']);
        view.error.visible = false;
        const content = box(true, 10, [view.heading, box(false, 6, [view.mute, view.volume, view.percent])]);
        if (kind === 'output') content.append(wrapped('0–150% · above 100% amplifies audio', ['volume-hint', 'muted']));
        content.append(label('Devices', ['section-heading']));
        content.append(choices);
        content.append(view.error);
        // Both outer scrollers have zero size hints, so Gtk.Box assigns equal halves.
        // The inner list normally fills its half; the outer scroll keeps controls reachable on short monitors.
        return scroll(content, { vexpand: true, css_classes: ['audio-section'] });
    }

    buildBluetooth() {
        this.scan = button('Scan', () => this.action('bluetooth', () => this.backend.toggleDiscovery()));
        this.scan.halign = Gtk.Align.END;
        this.bluetoothState = wrapped('', ['muted']);
        this.pairingMessages = wrapped('');
        this.pairingMessages.selectable = true;
        const log = scroll(this.pairingMessages, { propagate_natural_height: true, max_content_height: 160,
            css_classes: ['pairing-log'] });
        this.answer = new Gtk.Entry({ placeholder_text: 'PIN/passkey or yes/no', width_chars: 1,
            max_width_chars: 1, hexpand: true, css_classes: ['pairing-answer'] });
        this.send = button('Send', () => this.sendAnswer());
        this.send.sensitive = false;
        this.cancel = button('Cancel', () => this.action('bluetooth', () => this.backend.cancelPairing()));
        this.answer.connect('changed', () => { this.send.sensitive = this.pairing && this.answer.text.trim() !== ''; });
        this.answer.connect('activate', () => this.sendAnswer());
        this.pairingButtons = box(false, 6, [this.send, this.cancel]);
        this.pairingControls = box(false, 6, [this.answer, this.pairingButtons]);
        this.pairingCard = box(true, 6, [log, this.pairingControls]);
        this.pairingCard.add_css_class('pairing-card');
        this.pairingCard.visible = false;
        this.bluetoothRows = box(true, 6);
        const devices = scroll(this.bluetoothRows, { vexpand: true, min_content_height: 32 });
        this.bluetoothError = wrapped('', ['failure']);
        this.bluetoothError.visible = false;
        const content = box(true, 10, [this.scan, this.bluetoothState, this.pairingCard, devices, this.bluetoothError]);
        return scroll(content, { vexpand: true, css_classes: ['bluetooth-page'] });
    }

    press(widget, callback) {
        const events = new Gtk.EventControllerLegacy({ propagation_phase: Gtk.PropagationPhase.CAPTURE });
        events.connect('event', (_, event) => {
            if ([Gdk.EventType.BUTTON_PRESS, Gdk.EventType.TOUCH_BEGIN].includes(event.get_event_type())) callback();
            return false;
        });
        widget.add_controller(events);
    }

    nodeCurrent(view, key) {
        const live = this.backend.snapshot()?.[view.kind];
        return this.opened && this.currentTab === 1 && Boolean(key) && live?.available && live.nodeKey === key;
    }

    action(kind, callback) {
        this.failures[kind] = '';
        try { callback(); }
        catch (error) { this.failures[kind] = error.message || String(error); }
        this.render();
    }

    sendAnswer() {
        const text = this.answer.text;
        if (!this.pairing || !text.trim()) return;
        this.action('bluetooth', () => {
            this.backend.sendPairingAnswer(text);
            this.answer.set_text('');
        });
    }

    render() {
        const state = this.backend.snapshot() || {};
        this.tabs.forEach((tab, index) => style(tab, 'selected', index === this.currentTab));
        this.pages.visible_child_name = this.currentTab === 0 ? 'bluetooth' : 'audio';
        for (const kind of ['output', 'input']) this.renderAudio(this.audio[kind], state[kind] || {});
        this.renderBluetooth(state.bluetooth || {});
    }

    renderAudio(view, model) {
        view.nodeKey = model.nodeKey || '';
        const title = `${view.kind === 'output' ? 'Output' : 'Input'} · ${model.available ? model.name || 'No device' : 'No device'}`;
        view.heading.set_label(title);
        view.heading.tooltip_text = title;
        const enabled = Boolean(model.available && view.nodeKey);
        view.mute.sensitive = view.volume.sensitive = enabled;
        view.mute.get_child().set_label(view.kind === 'input' ? (model.muted ? '󰍭' : '󰍬') : (model.muted ? '󰝟' : '󰕾'));
        view.mute.tooltip_text = model.muted ? 'Unmute' : 'Mute';
        const value = volumeValue(model.volume, view.maximum);
        if (!view.dragging || view.volumeTarget !== view.nodeKey) view.volume.set_value(value);
        view.percent.set_label(`${Math.round(view.volume.get_value() * 100)}%`);
        const choices = model.choices || [];
        const wanted = new Set(choices.map(choice => choice.key));
        for (const [key, row] of view.rows) {
            if (wanted.has(key)) continue;
            view.choices.remove(row);
            view.rows.delete(key);
        }
        let previous = null;
        for (const choice of choices) {
            let row = view.rows.get(choice.key);
            if (!row) {
                row = button('', () => this.action(view.kind, () => this.backend.select(view.kind, choice.key)), ['device-choice']);
                row.hexpand = true;
                row.get_child().xalign = 0;
                row.get_child().max_width_chars = 1;
                view.choices.append(row);
                view.rows.set(choice.key, row);
            }
            row.get_child().set_label(`${choice.default ? '✓  ' : ''}${choice.label}`);
            row.tooltip_text = choice.description || choice.label;
            row.sensitive = !model.busy;
            style(row, 'selected', choice.default);
            view.choices.reorder_child_after(row, previous);
            previous = row;
        }
        view.choices.reorder_child_after(view.empty, previous);
        view.empty.visible = choices.length === 0 && !model.loading;
        message(view.error, this.failures[view.kind] || model.error);
    }

    renderBluetooth(model) {
        this.scan.get_child().set_label(model.discovering ? 'Stop scan' : 'Scan');
        this.scan.sensitive = Boolean(model.available && model.powered);
        message(this.bluetoothState, !model.available ? 'No Bluetooth adapter.' : !model.powered ? 'Bluetooth is off.' : '');
        const pairing = Boolean(model.pairing);
        if (pairing !== this.pairing) this.answer.set_text('');
        this.pairing = pairing;
        this.pairingMessages.set_label(model.pairingLog || '');
        this.pairingCard.visible = pairing || Boolean(model.pairingLog);
        this.pairingControls.visible = pairing;
        this.send.sensitive = pairing && this.answer.text.trim() !== '';
        const narrow = this.cardWidth < 320;
        this.pairingControls.orientation = narrow ? Gtk.Orientation.VERTICAL : Gtk.Orientation.HORIZONTAL;
        this.pairingButtons.halign = narrow ? Gtk.Align.END : Gtk.Align.FILL;
        const devices = model.devices || [];
        const wanted = new Set(devices.map(device => device.key));
        for (const [key, row] of this.deviceRows) {
            if (wanted.has(key)) continue;
            this.bluetoothRows.remove(row.widget);
            this.deviceRows.delete(key);
        }
        let previous = null;
        for (const device of devices) {
            let row = this.deviceRows.get(device.key);
            if (!row) {
                row = { name: ellipsis('', ['device-name']), status: ellipsis('', ['device-status']) };
                row.action = button('', () => this.action('bluetooth', () => this.backend.deviceAction(device.key)));
                row.forget = button('×', () => this.action('bluetooth', () => this.backend.forget(device.key)), ['icon-button', 'forget-button']);
                row.buttons = box(false, 6, [row.action, row.forget]);
                const details = box(true, 4, [row.name, row.status]);
                details.hexpand = true;
                row.widget = box(false, 6, [details, row.buttons]);
                row.widget.add_css_class('bluetooth-device');
                this.bluetoothRows.append(row.widget);
                this.deviceRows.set(device.key, row);
            }
            row.name.set_label(device.name || device.address || '');
            row.name.tooltip_text = device.name || device.address || '';
            const battery = Number.isFinite(device.battery) ? ` · ${Math.round(device.battery * 100)}%` : '';
            row.status.set_label(`${device.connected ? 'Connected' : device.paired ? 'Paired' : 'Available'}${battery}`);
            row.action.get_child().set_label(device.connected ? 'Disconnect' : device.paired ? 'Connect' : 'Pair');
            row.action.sensitive = Boolean(model.available && model.powered && !pairing && !device.busy);
            row.forget.sensitive = !pairing && !device.busy;
            row.forget.tooltip_text = `Forget ${device.name || device.address}`;
            row.widget.orientation = narrow ? Gtk.Orientation.VERTICAL : Gtk.Orientation.HORIZONTAL;
            row.buttons.halign = narrow ? Gtk.Align.END : Gtk.Align.FILL;
            style(row.widget, 'connected', device.connected);
            this.bluetoothRows.reorder_child_after(row.widget, previous);
            previous = row.widget;
        }
        message(this.bluetoothError, this.failures.bluetooth || model.error);
    }

    resetInteractions() {
        for (const view of Object.values(this.audio)) {
            view.dragging = false;
            view.volumeTarget = view.muteTarget = null;
        }
    }

    setTab(index) {
        if (index === this.currentTab) return;
        this.currentTab = index;
        this.resetInteractions();
        this.pageBorder.queue_draw();
        this.backend.setActive(this.opened, this.currentTab);
        this.render();
    }

    position() {
        if (!this.monitor) return;
        const { width, height } = this.monitor.get_geometry();
        this.cardWidth = Math.max(1, Math.min(400, width - 16));
        this.card.width_request = this.cardWidth;
        this.card.height_request = Math.max(1, Math.min(520, height - 64));
        this.card.margin_end = Math.round(Math.min(96, Math.max(8, (width - this.cardWidth) / 2)));
    }

    toggle(monitor, name = monitor?.get_connector() || '') {
        if (this.opened && this.monitorName === name) return this.close();
        if (!monitor) throw new Error('No monitor is connected.');
        this.resetInteractions();
        if (this.monitorSignal) this.monitor.disconnect(this.monitorSignal);
        this.monitor = monitor;
        this.monitorName = name;
        this.monitorSignal = monitor.connect('notify::geometry', () => { this.position(); this.render(); });
        this.window.gdkmonitor = monitor;
        this.position();
        this.opened = true;
        this.backend.setActive(true, this.currentTab);
        this.render();
        this.window.keymode = Astal.Keymode.EXCLUSIVE;
        this.window.visible = true;
        this.tabs[this.currentTab].grab_focus();
    }

    close() {
        this.opened = false;
        this.window.keymode = Astal.Keymode.NONE;
        this.window.visible = false;
        this.resetInteractions();
        this.backend.setActive(false, this.currentTab);
        this.render();
    }

    dispose() {
        this.monitors.disconnect(this.monitorsSignal);
        if (this.monitorSignal) this.monitor.disconnect(this.monitorSignal);
        this.window.destroy();
    }
}

function monitorFor(name) {
    const monitors = Gdk.Display.get_default().get_monitors();
    for (let index = 0; index < monitors.get_n_items(); index++) {
        const monitor = monitors.get_item(index);
        if (monitor.get_connector() === name) return monitor;
    }
    return monitors.get_n_items() ? monitors.get_item(0) : null;
}

const AudioApplication = GObject.registerClass({ GTypeName: 'AudioPanelApplication' },
    class AudioApplication extends Astal.Application {
        vfunc_request(request, connection) {
            let response = 'ok';
            try {
                const command = parseRequest(request);
                if (!this.backend || !this.panel) throw new Error('Audio panel is starting.');
                switch (command.command) {
                case 'status': response = this.backend.status(); break;
                case 'toggle': this.panel.toggle(monitorFor(command.monitor), command.monitor || ''); break;
                case 'close': this.panel.close(); break;
                case 'bluetoothPower': this.backend.toggleBluetoothPower(); break;
                default: throw new Error('Unknown audio panel command.');
                }
            } catch (error) { response = JSON.stringify({ error: error.message || String(error) }); }
            try {
                AstalIO.write_sock(connection, response, (_, result) => {
                    try { AstalIO.write_sock_finish(result); }
                    catch (error) { console.error(error); }
                    finally { this.closeConnection(connection); }
                });
            } catch (error) {
                console.error(error);
                this.closeConnection(connection);
            }
        }

        closeConnection(connection) {
            connection.close_async(GLib.PRIORITY_DEFAULT, null, (source, result) => {
                try { source.close_finish(result); }
                catch (error) { console.error(error); }
            });
        }

        startPanel() {
            if (this.started) return;
            this.started = true;
            const palette = loadTheme(GLib.getenv('AUDIO_PANEL_CONFIG'), GLib.getenv('AUDIO_PANEL_PALETTE'));
            this.backend = new AudioBackend({ onChange: () => this.panel?.render() });
            this.panel = new AudioPanel({ app: this, backend: this.backend, palette });
            this.backend.start();
            this.backend.setActive(false, this.panel.currentTab);
            this.panel.render();
        }

        disposePanel() {
            this.backend?.dispose();
            this.panel?.dispose();
            this.panel = null;
            this.backend = null;
        }
    });

Gtk.init();
// Do not pass the layer-shell preload on to helpers or GIO-launched processes.
GLib.unsetenv('LD_PRELOAD');

const app = new AudioApplication({ instance_name: 'audio-panel' });
let exitCode = 0;
app.connect('activate', () => {
    try { app.startPanel(); }
    catch (error) {
        console.error(error);
        exitCode = 1;
        app.quit();
    }
});
app.connect('shutdown', () => app.disposePanel());
try { app.acquire_socket(); }
catch (error) {
    printerr(`audio-panel: ${error.message || String(error)}`);
    System.exit(1);
}
app.hold();
const status = await app.runAsync([]);
System.exit(exitCode || status);
