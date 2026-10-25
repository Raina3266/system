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
import { loadPanelStyle } from '../../live-style.js';

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
    const definitions = '.audio-panel {\n' + Object.entries(palette)
        .filter(([key, value]) => /^[a-zA-Z]+$/.test(key) && /^#[a-f\d]{6}$/i.test(value))
        .map(([key, value]) => `    --ap_${key}: ${value};`).join('\n') + '\n}';
    loadPanelStyle({ entry, definitions, label: 'Audio' });
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

function volumeBar(scale, beforeStep) {
    let available = true;
    // Enabling the native value must not change the range's mutation precision.
    const rounding = scale.round_digits;
    scale.draw_value = true;
    scale.round_digits = rounding;
    scale.value_pos = Gtk.PositionType.TOP;
    scale.add_css_class('volume-track');
    const format = (_, value) => available ? `${Math.round(value * 100)}%` : '—';
    scale.set_format_value_func(format);
    const widget = new Gtk.Box({ hexpand: true, valign: Gtk.Align.CENTER, css_classes: ['volume-bar'] });
    const step = (text, direction, description) => {
        const control = new Gtk.Button({ css_classes: ['volume-step'],
            width_request: 20, height_request: 20, valign: Gtk.Align.CENTER,
            child: new Gtk.Label({ label: text, use_markup: false }) });
        control.update_property([Gtk.AccessibleProperty.LABEL], [description]);
        control.connect('clicked', () => {
            if (!scale.is_sensitive()) return;
            beforeStep();
            scale.emit('move-slider', direction);
        });
        return control;
    };
    widget.append(step('◁', Gtk.ScrollType.STEP_BACKWARD, 'Decrease volume'));
    widget.append(scale);
    widget.append(step('▷', Gtk.ScrollType.STEP_FORWARD, 'Increase volume'));
    return { widget, setAvailable(value) {
        widget.sensitive = scale.sensitive;
        const next = Boolean(value);
        if (available === next) return;
        available = next;
        // Gtk.Scale caches formatted values, including the unavailable placeholder.
        scale.set_format_value_func(format);
    } };
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
            const expanded = Object.values(this.audio).find(view => view.deviceChoices.reveal_child);
            if (expanded) {
                this.setDeviceExpanded(expanded, false);
                expanded.devices.grab_focus();
            } else this.close();
            return true;
        });
        this.root = new Gtk.Overlay({ hexpand: true, vexpand: true });
        this.root.add_controller(keys);
        this.root.set_child(new Gtk.Box({ hexpand: true, vexpand: true }));
        this.window.set_child(this.root);
        this.card = new Gtk.Overlay({ width_request: 400,
            halign: Gtk.Align.END, valign: Gtk.Align.START, margin_top: 40, margin_end: 96,
            overflow: Gtk.Overflow.HIDDEN, css_classes: ['audio-card'] });
        this.card.set_child(frame(palette));
        this.content = box(true);
        this.content.add_css_class('audio-content');
        this.card.add_overlay(this.content);
        this.card.set_measure_overlay(this.content, true);
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
        this.audioPage = scroll(audio, { vexpand: true, propagate_natural_height: true,
            max_content_height: 520, css_classes: ['audio-page'] });
        this.pages.add_named(this.audioPage, 'audio');
        this.pageFrame.add_overlay(this.pages);
        this.pageFrame.set_measure_overlay(this.pages, true);
        this.content.append(this.pageFrame);
    }

    buildAudio(kind) {
        const view = { kind, maximum: kind === 'output' ? 1.5 : 1,
            choiceItems: [], choicesSignature: '',
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
            draw_value: false, digits: 2, valign: Gtk.Align.CENTER,
            adjustment: new Gtk.Adjustment({ lower: 0, upper: view.maximum, step_increment: 0.01, page_increment: 0.05 }) });
        this.press(view.volume, () => { view.dragging = true; view.volumeTarget = view.nodeKey; });
        const volumeKeys = new Gtk.EventControllerKey({ propagation_phase: Gtk.PropagationPhase.CAPTURE });
        volumeKeys.connect('key-pressed', () => { view.dragging = false; view.volumeTarget = null; return false; });
        view.volume.add_controller(volumeKeys);
        const volumeScroll = new Gtk.EventControllerScroll({ flags: Gtk.EventControllerScrollFlags.BOTH_AXES,
            propagation_phase: Gtk.PropagationPhase.CAPTURE });
        volumeScroll.connect('scroll', () => { view.dragging = false; view.volumeTarget = null; return false; });
        view.volume.add_controller(volumeScroll);
        view.volumeBar = volumeBar(view.volume, () => { view.dragging = false; view.volumeTarget = null; });
        view.volume.connect('change-value', (_, scrollType, value) => {
            const key = view.volumeTarget ?? view.nodeKey;
            if (!this.nodeCurrent(view, key)) return true;
            const next = volumeValue(value, view.maximum);
            view.volume.set_value(next);
            this.action(kind, () => this.backend.setVolume(kind, next, key));
            return true;
        });
        view.devicesHeading = label('Devices', ['section-heading']);
        view.deviceLabel = ellipsis();
        view.deviceArrow = label('⌄');
        view.devices = new Gtk.ToggleButton({ hexpand: true, css_classes: ['device-dropdown'],
            child: box(false, 8, [view.deviceLabel, view.deviceArrow]) });
        view.devices.update_property([Gtk.AccessibleProperty.LABEL],
            [kind === 'output' ? 'Output device' : 'Input device']);
        view.deviceSelection = new Gtk.SingleSelection({ autoselect: false, can_unselect: true });
        const factory = new Gtk.SignalListItemFactory();
        factory.connect('setup', (_, item) => item.set_child(ellipsis()));
        factory.connect('bind', (_, item) => {
            const text = item.get_item().get_string();
            const widget = item.get_child();
            widget.set_label(text);
            widget.update_property([Gtk.AccessibleProperty.DESCRIPTION],
                [view.choiceItems[item.get_position()]?.description || text]);
        });
        view.deviceList = new Gtk.ListView({ model: view.deviceSelection, factory,
            single_click_activate: true, css_classes: ['device-list'] });
        view.deviceChoices = new Gtk.Revealer({ transition_type: Gtk.RevealerTransitionType.SLIDE_DOWN,
            transition_duration: 180, reveal_child: false,
            child: scroll(view.deviceList, { propagate_natural_height: true, max_content_height: 240,
                margin_top: 6, css_classes: ['device-options'] }) });
        view.devices.connect('toggled', () => this.setDeviceExpanded(view, view.devices.active));
        view.deviceList.connect('activate', (_, position) => this.selectAudioDevice(view, position));
        this.setDeviceExpanded(view, false);
        view.error = wrapped('', ['failure']);
        view.error.visible = false;
        const controls = box(true, 4, [view.heading,
            box(false, 6, [view.mute, view.volumeBar.widget])]);
        const content = box(true, 10, [controls,
            box(true, 6, [view.devicesHeading, box(true, 0, [view.devices, view.deviceChoices])]), view.error]);
        content.add_css_class('audio-section');
        return content;
    }

    buildBluetooth() {
        this.bluetoothState = wrapped('', ['muted']);
        this.pairingStatus = wrapped('');
        this.pairingStatus.selectable = true;
        this.pairingPrompt = wrapped('');
        this.pairingPrompt.selectable = true;
        this.dismissPairing = button('×', () => this.action('bluetooth', () => this.backend.dismissPairingMessage()), ['icon-button', 'pairing-dismiss']);
        this.dismissPairing.update_property([Gtk.AccessibleProperty.LABEL], ['Dismiss pairing message']);
        this.dismissPairing.valign = Gtk.Align.START;
        const status = box(false, 6, [this.pairingStatus, this.dismissPairing]);
        this.answer = new Gtk.Entry({ placeholder_text: 'PIN/passkey or yes/no', width_chars: 1,
            max_width_chars: 1, hexpand: true, visible: false, css_classes: ['pairing-answer'] });
        this.send = button('Send', () => this.sendAnswer());
        this.send.visible = this.send.sensitive = false;
        this.cancel = button('Cancel', () => this.action('bluetooth', () => this.backend.cancelPairing()));
        this.answer.connect('changed', () => { this.send.sensitive = this.pairing && this.answer.visible && this.answer.text.trim() !== ''; });
        this.answer.connect('activate', () => this.sendAnswer());
        this.pairingButtons = box(false, 6, [this.send, this.cancel]);
        this.pairingControls = box(false, 6, [this.answer, this.pairingButtons]);
        this.pairingCard = box(true, 6, [status, this.pairingPrompt, this.pairingControls]);
        this.pairingCard.add_css_class('pairing-card');
        this.pairingCard.visible = false;
        this.bluetoothRows = box(true, 6);
        this.bluetoothSeparator = new Gtk.Separator({ css_classes: ['audio-divider'], visible: false });
        this.bluetoothRows.append(this.bluetoothSeparator);
        const devices = scroll(this.bluetoothRows, { vexpand: true, min_content_height: 32 });
        this.bluetoothError = wrapped('', ['failure']);
        this.bluetoothError.visible = false;
        const content = box(true, 10, [this.bluetoothState, this.pairingCard, devices, this.bluetoothError]);
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
        if (!this.pairing || !this.answer.visible || !text.trim()) return;
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
        this.position();
    }

    renderAudio(view, model) {
        view.nodeKey = model.nodeKey || '';
        const title = `${view.kind === 'output' ? 'Output' : 'Input'} · ${model.available ? model.name || 'No device' : 'No device'}`;
        view.heading.set_label(title);
        view.heading.tooltip_text = title;
        const enabled = Boolean(model.available && view.nodeKey);
        view.mute.sensitive = view.volume.sensitive = enabled;
        view.mute.get_child().set_label(view.kind === 'input' ? (model.muted ? '󰍭' : '󰍬') : (model.muted ? '󰝟' : '󰕾'));
        const description = enabled ? '' : 'No default audio device is available.';
        view.mute.update_property([Gtk.AccessibleProperty.LABEL, Gtk.AccessibleProperty.DESCRIPTION],
            [model.muted ? 'Unmute' : 'Mute', description]);
        view.volume.update_property([Gtk.AccessibleProperty.LABEL, Gtk.AccessibleProperty.DESCRIPTION],
            [view.kind === 'output' ? 'Output volume' : 'Input volume', description]);
        const value = volumeValue(model.volume, view.maximum);
        if (!view.dragging || view.volumeTarget !== view.nodeKey) view.volume.set_value(value);
        view.volumeBar.setAvailable(enabled);
        const choices = model.choices || [];
        const hasAlternative = choices.some(choice => !choice.default);
        view.devicesHeading.visible = view.devices.visible = hasAlternative;
        view.devices.sensitive = hasAlternative && !model.busy;
        const selected = choices.findIndex(choice => choice.default);
        const items = choices.map(choice => ({ key: choice.key, label: choice.label, description: choice.description || '' }));
        if (selected < 0) items.unshift({ key: '',
            label: model.available ? model.name || 'Select a device…' : 'Select a device…', description: '' });
        const signature = JSON.stringify(items);
        view.choiceItems = items;
        if (signature !== view.choicesSignature) {
            view.deviceSelection.set_model(Gtk.StringList.new(items.map(item => item.label)));
            const choicesScroll = view.deviceChoices.get_child();
            // The outer page scrolls on short screens; do not let it squeeze this viewport to zero.
            choicesScroll.min_content_height = Math.min(choicesScroll.max_content_height,
                view.deviceList.measure(Gtk.Orientation.VERTICAL, -1)[1]);
            view.choicesSignature = signature;
        }
        view.deviceSelection.selected = selected < 0 ? 0 : selected;
        const current = items[view.deviceSelection.selected];
        view.deviceLabel.set_label(current?.label || '');
        view.devices.update_property([Gtk.AccessibleProperty.DESCRIPTION],
            [current?.description || current?.label || '']);
        if (!hasAlternative || model.busy || !this.opened || this.currentTab !== 1)
            this.setDeviceExpanded(view, false);
        message(view.error, this.failures[view.kind] || model.error);
    }

    setDeviceExpanded(view, expanded) {
        const show = Boolean(expanded && this.opened && this.currentTab === 1
            && view.devices.visible && view.devices.sensitive);
        if (show) for (const other of Object.values(this.audio))
            if (other !== view) this.setDeviceExpanded(other, false);
        view.devices.active = show;
        view.deviceChoices.reveal_child = show;
        view.deviceArrow.set_label(show ? '⌃' : '⌄');
        view.devices.update_state([Gtk.AccessibleState.EXPANDED], [show]);
    }

    selectAudioDevice(view, position = view.deviceSelection.selected) {
        if (!this.opened || this.currentTab !== 1
            || !view.devices.visible || !view.devices.sensitive) return;
        const choice = view.choiceItems[position];
        const live = this.backend.snapshot()?.[view.kind];
        if (!choice?.key || !live || live.busy) return;
        const current = live.choices?.find(item => item.key === choice.key);
        this.setDeviceExpanded(view, false);
        view.devices.grab_focus();
        if (current?.default) { this.render(); return; }
        this.action(view.kind, () => {
            if (!current) throw new Error('The selected audio device is no longer available.');
            this.backend.select(view.kind, current.key);
        });
    }

    renderBluetooth(model) {
        message(this.bluetoothState, !model.available ? 'No Bluetooth adapter.' : !model.powered ? 'Bluetooth is off.' : '');
        const pairing = Boolean(model.pairing), notice = model.pairingMessage;
        const hasPrompt = Boolean(pairing && notice?.prompt);
        // Display-only codes are entered on the device, not in this panel.
        const needsAnswer = hasPrompt && !/^Type .+ on the device(?: and press Enter)? to finish pairing\.$/.test(notice.prompt);
        if (pairing !== this.pairing || (!needsAnswer && this.answer.text)) this.answer.set_text('');
        this.pairing = pairing;
        this.answer.visible = this.send.visible = needsAnswer;
        message(this.pairingStatus, notice?.text || '');
        message(this.pairingPrompt, notice?.prompt || '');
        style(this.pairingStatus, 'failure', notice?.state === 'failed');
        this.pairingCard.visible = Boolean(notice);
        this.dismissPairing.sensitive = !pairing || !notice?.prompt;
        this.dismissPairing.update_property([Gtk.AccessibleProperty.DESCRIPTION],
            [this.dismissPairing.sensitive ? '' : 'Answer or cancel pairing before dismissing this message']);
        this.pairingControls.visible = hasPrompt;
        this.send.sensitive = needsAnswer && this.answer.text.trim() !== '';
        const narrow = this.cardWidth < 320;
        this.pairingControls.orientation = narrow ? Gtk.Orientation.VERTICAL : Gtk.Orientation.HORIZONTAL;
        this.pairingButtons.halign = narrow ? Gtk.Align.END : Gtk.Align.FILL;
        const allDevices = model.devices || [];
        const remembered = allDevices.filter(device => device.paired || device.connected);
        const available = allDevices.filter(device => !device.paired && !device.connected);
        const devices = [...remembered, ...available];
        this.bluetoothSeparator.visible = remembered.length > 0 && available.length > 0;
        const wanted = new Set(devices.map(device => device.key));
        for (const [key, row] of this.deviceRows) {
            if (wanted.has(key)) continue;
            this.bluetoothRows.remove(row.widget);
            this.deviceRows.delete(key);
        }
        let previous = null, separated = false;
        for (const device of devices) {
            if (this.bluetoothSeparator.visible && !device.paired && !device.connected && !separated) {
                this.bluetoothRows.reorder_child_after(this.bluetoothSeparator, previous);
                previous = this.bluetoothSeparator;
                separated = true;
            }
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
            row.forget.visible = Boolean(device.paired);
            row.forget.sensitive = Boolean(device.paired && !pairing && !device.busy);
            row.forget.update_property([Gtk.AccessibleProperty.LABEL], [`Forget ${device.name || device.address}`]);
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
            this.setDeviceExpanded(view, false);
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
        const maximumHeight = Math.max(1, Math.min(520, height - 64));
        this.card.height_request = this.currentTab === 0 ? maximumHeight : -1;
        if (this.currentTab === 1) {
            // Measure tabs, padding and margins without reserving space for audio contents.
            this.audioPage.propagate_natural_height = false;
            const [, chromeHeight] = this.content.measure(Gtk.Orientation.VERTICAL, this.cardWidth);
            const [, viewportHeight] = this.audioPage.measure(Gtk.Orientation.VERTICAL, this.cardWidth);
            this.audioPage.max_content_height = Math.max(1, maximumHeight - chromeHeight + viewportHeight);
            this.audioPage.propagate_natural_height = true;
        }
        this.card.margin_end = Math.round(Math.min(96, Math.max(8, (width - this.cardWidth) / 2)));
    }

    toggle(monitor, name = monitor?.get_connector() || '') {
        if (this.opened && this.monitorName === name) return this.close();
        if (!monitor) throw new Error('No monitor is connected.');
        this.resetInteractions();
        if (this.monitorSignal) this.monitor.disconnect(this.monitorSignal);
        this.monitor = monitor;
        this.monitorName = name;
        this.monitorSignal = monitor.connect('notify::geometry', () => this.render());
        this.window.gdkmonitor = monitor;
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
