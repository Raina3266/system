import GLib from 'gi://GLib';
import GObject from 'gi://GObject';
import Gtk from 'gi://Gtk?version=4.0';
import Gdk from 'gi://Gdk?version=4.0';
import GdkPixbuf from 'gi://GdkPixbuf';
import Pango from 'gi://Pango';
import Astal from 'gi://Astal?version=4.0';
import AstalIO from 'gi://AstalIO';
import System from 'system';
import { NetworkBackend, controllerRuntime, dismissOtherPanels } from './backend.js';
import { NetworkController, parseRequest } from './logic.js';
import { loadPanelStyle } from '../../panels/style.js';

function read(path) {
    if (!path) throw new Error('Run through network-panel, or set NETWORK_PANEL_CONFIG and NETWORK_PANEL_PALETTE.');
    const [ok, contents] = GLib.file_get_contents(path);
    if (!ok) throw new Error(`Could not read network theme: ${path}`);
    return new TextDecoder().decode(contents);
}

export function loadTheme(entry, palettePath) {
    const palette = JSON.parse(read(palettePath));
    for (const key of ['yellow', 'cyan', 'pink', 'red', 'dimPink', 'separator', 'foreground', 'background', 'chromeBackground'])
        if (!/^#[a-f\d]{6}$/i.test(palette[key])) throw new Error(`Invalid network palette colour: ${key}`);
    const definitions = Object.entries(palette)
        .filter(([key, value]) => /^[a-zA-Z]+$/.test(key) && /^#[a-f\d]{6}$/i.test(value))
        .map(([key, value]) => `@define-color np_${key} ${value};`).join('\n');
    loadPanelStyle({ entry, definitions, label: 'Network' });
    return palette;
}

function box(vertical = false, spacing = 8, children = []) {
    const widget = new Gtk.Box({ orientation: vertical ? Gtk.Orientation.VERTICAL : Gtk.Orientation.HORIZONTAL, spacing });
    children.forEach(child => widget.append(child));
    return widget;
}

function label(text = '', classes = [], wrap = true) {
    return new Gtk.Label({ label: text, use_markup: false, xalign: 0, hexpand: true,
        wrap, wrap_mode: Pango.WrapMode.WORD_CHAR, max_width_chars: 1,
        ellipsize: wrap ? Pango.EllipsizeMode.NONE : Pango.EllipsizeMode.END, css_classes: classes });
}

function button(text, action, classes = []) {
    const widget = new Gtk.Button({ valign: Gtk.Align.CENTER, css_classes: ['control-button', ...classes],
        child: new Gtk.Label({ label: text, use_markup: false, ellipsize: Pango.EllipsizeMode.END }) });
    widget.connect('clicked', action);
    return widget;
}

function scroll(child, properties = {}) {
    return new Gtk.ScrolledWindow({ child, hscrollbar_policy: Gtk.PolicyType.NEVER,
        vscrollbar_policy: Gtk.PolicyType.AUTOMATIC, min_content_width: 0, min_content_height: 0,
        propagate_natural_width: false, propagate_natural_height: false, hexpand: true, ...properties });
}

function message(widget, text) {
    widget.set_label(text || '');
    widget.visible = Boolean(text);
}

function formatted(value) {
    if (value === null || value === undefined || value === '') return '—';
    if (Array.isArray(value)) return value.map(formatted).join('\n');
    if (typeof value === 'object') return Object.entries(value).map(([key, item]) => `${key}: ${formatted(item)}`).join('\n');
    return String(value);
}

function frame(palette) {
    const drawing = new Gtk.DrawingArea({ can_target: false, hexpand: true, vexpand: true });
    const rgb = palette.yellow.slice(1).match(/../g).map(part => parseInt(part, 16) / 255);
    drawing.set_draw_func((_, cr, width, height) => {
        if (width < 1 || height < 1) return;
        const cut = Math.min(10, Math.max(0, (width - 1) / 2), Math.max(0, (height - 1) / 2)), inset = 0.5;
        cr.moveTo(cut + inset, inset);
        cr.lineTo(width - inset, inset);
        cr.lineTo(width - inset, height - cut - inset);
        cr.lineTo(width - cut - inset, height - inset);
        cr.lineTo(inset, height - inset);
        cr.lineTo(inset, cut + inset);
        cr.closePath();
        cr.setSourceRGBA(37 / 255, 37 / 255, 47 / 255, 0.9);
        cr.fillPreserve();
        cr.setSourceRGBA(...rgb, 1);
        cr.setLineWidth(1);
        cr.stroke();
    });
    return drawing;
}

const detailFields = [
    ['profileId', 'Profile'], ['profileUuid', 'Profile UUID'], ['ipv4', 'IPv4'], ['gateway4', 'IPv4 gateway'],
    ['ipv6', 'IPv6'], ['gateway6', 'IPv6 gateway'], ['dns', 'DNS'], ['mac', 'MAC'],
    ['bssid', 'BSSID', true], ['frequency', 'Frequency', true], ['band', 'Band', true],
    ['channel', 'Channel', true], ['mode', 'Mode', true], ['rate', 'Link rate', true],
];

export function monitorFor(name = '') {
    const monitors = Gdk.Display.get_default().get_monitors();
    for (let index = 0; index < monitors.get_n_items(); index++) {
        const monitor = monitors.get_item(index);
        if (monitor.get_connector() === name) return monitor;
    }
    return monitors.get_n_items() ? monitors.get_item(0) : null;
}

export class NetworkPanel {
    constructor({ app, controller, palette, dismiss = dismissOtherPanels }) {
        this.controller = controller;
        this.dismissOtherPanels = dismiss;
        this.rows = new Map();
        this.profileChoices = new Map();
        this.monitor = null;
        this.monitorSignal = 0;
        this.presented = false;
        this.interactionIdentity = '';
        this.qrKey = '';
        this.qrPixbuf = null;
        this.window = new Astal.Window({ application: app, name: 'network-panel', namespace: 'network-panel',
            anchor: Astal.WindowAnchor.TOP | Astal.WindowAnchor.BOTTOM | Astal.WindowAnchor.LEFT | Astal.WindowAnchor.RIGHT,
            exclusivity: Astal.Exclusivity.IGNORE, layer: Astal.Layer.OVERLAY,
            keymode: Astal.Keymode.NONE, visible: false, css_classes: ['network-panel'] });
        this.window.connect('close-request', () => { this.close(); return true; });
        const keys = new Gtk.EventControllerKey({ propagation_phase: Gtk.PropagationPhase.CAPTURE });
        keys.connect('key-pressed', (_, key) => {
            if (key !== Gdk.KEY_Escape) return false;
            this.close();
            return true;
        });
        this.window.add_controller(keys);
        this.root = new Gtk.Overlay({ hexpand: true, vexpand: true, focusable: true });
        this.root.set_child(new Gtk.Box({ hexpand: true, vexpand: true }));
        this.window.set_child(this.root);
        this.card = new Gtk.Overlay({ width_request: 420, height_request: 520,
            halign: Gtk.Align.END, valign: Gtk.Align.START, overflow: Gtk.Overflow.HIDDEN,
            css_classes: ['network-card'] });
        this.card.set_child(frame(palette));
        this.content = box(true);
        this.content.add_css_class('network-content');
        // Normally only the list/details scroll. This outer viewport is a safety
        // valve for monitors too short even for the credential form.
        this.contentScroll = scroll(this.content);
        this.card.add_overlay(this.contentScroll);
        this.card.set_measure_overlay(this.contentScroll, false);
        this.root.add_overlay(this.card);
        this.root.set_measure_overlay(this.card, false);
        const outside = new Gtk.GestureClick({ propagation_phase: Gtk.PropagationPhase.CAPTURE });
        outside.connect('released', (_, count, x, y) => {
            const [ok, bounds] = this.card.compute_bounds(this.root);
            if (!this.card.visible || !ok || x < bounds.get_x() || x >= bounds.get_x() + bounds.get_width()
                || y < bounds.get_y() || y >= bounds.get_y() + bounds.get_height()) this.close();
        });
        this.root.add_controller(outside);
        this.build();
        this.monitors = Gdk.Display.get_default().get_monitors();
        this.monitorsSignal = this.monitors.connect('items-changed', () => this.updateMonitors());
        this.render();
    }

    build() {

        this.wifiNotice = label('', ['warning']);
        this.error = label('', ['warning']);
        this.error.lines = 3;
        this.error.ellipsize = Pango.EllipsizeMode.END;
        this.content.append(this.wifiNotice);
        this.content.append(this.error);
        this.backButton = button('Back', () => { this.clearPassword(); this.controller.hideSecret(); this.controller.back(); });
        this.selectedName = label('', [], false);
        this.navigation = box(false, 8, [this.backButton, this.selectedName]);
        this.content.append(this.navigation);
        this.pages = new Gtk.Stack({ transition_type: Gtk.StackTransitionType.NONE,
            hhomogeneous: false, vhomogeneous: false, hexpand: true, vexpand: true });
        this.networkRows = box(true);
        this.networkSeparator = new Gtk.Separator({ css_classes: ['network-divider'], visible: false });
        this.networkRows.append(this.networkSeparator);
        this.empty = label('No networks available');
        this.empty.halign = Gtk.Align.CENTER;
        this.empty.valign = Gtk.Align.CENTER;
        this.list = new Gtk.Overlay({ hexpand: true, vexpand: true });
        this.list.set_child(scroll(this.networkRows, { vexpand: true }));
        this.list.add_overlay(this.empty);
        this.list.set_measure_overlay(this.empty, false);
        this.pages.add_named(this.list, 'list');
        this.buildInfo();
        this.buildQr();
        this.content.append(this.pages);
        this.credentialName = label();
        this.password = new Gtk.Entry({ visibility: false, placeholder_text: 'Network password',
            input_purpose: Gtk.InputPurpose.PASSWORD, input_hints: Gtk.InputHints.PRIVATE,
            width_chars: 1, max_width_chars: 1, hexpand: true, css_classes: ['network-password'] });
        this.submit = button('Connect', () => this.submitPassword());
        this.password.connect('changed', () => { this.submit.sensitive = this.password.get_text().length > 0; });
        this.password.connect('activate', () => this.submitPassword());
        const cancel = button('Cancel', () => { this.clearPassword(); this.controller.cancelCredentials(); });
        this.credentials = box(true, 8, [this.credentialName, this.password, box(false, 8, [this.submit, cancel])]);
        this.content.append(this.credentials);
        this.forgetConfirm = button('Forget', () => this.controller.confirmForget());
        this.forgetCancel = button('Cancel', () => this.controller.cancelForget());
        this.forget = box(true, 8, [label('Forget ALL saved profiles for this network and disconnect it?'),
            box(false, 8, [this.forgetConfirm, this.forgetCancel])]);
        this.content.append(this.forget);

    }

    buildInfo() {
        this.summary = label();
        this.savedProfiles = label();
        this.detailLabels = new Map(detailFields.map(([key]) => [key, label()]));
        this.reveal = button('Reveal password', () => {
            if (this.controller.secretRequested) this.controller.hideSecret();
            else this.controller.revealSecret();
        });
        this.secretLoading = label('Loading…');
        this.secret = label();
        this.infoContent = box(true, 8, [this.summary, this.savedProfiles,
            ...this.detailLabels.values(), box(false, 8, [this.reveal, this.secretLoading]), this.secret]);
        this.pages.add_named(scroll(this.infoContent, { vexpand: true }), 'info');
    }

    buildQr() {
        this.qrStatus = label();
        this.qrDrawing = new Gtk.DrawingArea({ hexpand: true, vexpand: true, can_target: false });
        this.qrDrawing.set_draw_func((_, cr, width, height) => {
            if (!this.qrPixbuf) return;
            const side = Math.floor(Math.min(width, height));
            if (side < 1) return;
            const pixbuf = this.qrPixbuf.scale_simple(side, side, GdkPixbuf.InterpType.NEAREST);
            Gdk.cairo_set_source_pixbuf(cr, pixbuf, Math.floor((width - side) / 2), Math.floor((height - side) / 2));
            cr.paint();
        });
        this.pages.add_named(box(true, 8, [this.qrStatus, this.qrDrawing]), 'qr');
    }

    createRow(key) {
        const row = { key, profilesSignature: '', profileUuids: [''], updating: false };
        row.name = label('', ['network-name']);
        row.metadata = label('', ['metadata']);
        row.profile = new Gtk.DropDown({ hexpand: true, enable_search: false, css_classes: ['profile-choice'] });
        const factory = popup => {
            const value = new Gtk.SignalListItemFactory();
            value.connect('setup', (_, item) => item.set_child(label('', [], false)));
            value.connect('bind', (_, item) => {
                const index = item.get_position();
                const profile = row.data?.profiles?.[index - 1];
                item.get_child().set_label(index && profile
                    ? popup ? `${formatted(profile.id)} · ${profile.uuid}` : formatted(profile.id)
                    : 'Automatic (default)');
            });
            return value;
        };
        row.profile.factory = factory(false);
        row.profile.list_factory = factory(true);
        row.profile.connect('notify::selected', () => {
            if (!row.updating) this.profileChoices.set(key, row.profileUuids[row.profile.selected] || '');
        });
        row.connect = button('Connect', () => {
            const current = this.controller.rows.find(value => value.key === key);
            if (!current) return;
            if (current.connected) this.controller.disconnectNetwork(key);
            else this.controller.connectNetwork(key, this.profileChoices.get(key) || '');
        });
        row.forget = button('Forget', () => this.controller.requestForget(key));
        row.info = button('Info', () => { this.clearPassword(); this.controller.info(key); });
        row.qr = button('QR', () => { this.clearPassword(); this.controller.qr(key); });
        row.heading = box(false, 8, [row.name, row.connect]);
        row.actions = scroll(box(false, 8, [row.forget, row.info, row.qr]), {
            hscrollbar_policy: Gtk.PolicyType.AUTOMATIC, vscrollbar_policy: Gtk.PolicyType.NEVER,
            min_content_height: 28 });
        row.widget = box(true, 4, [row.heading, row.metadata, row.profile, row.actions]);
        this.networkRows.append(row.widget);
        this.rows.set(key, row);
        return row;
    }

    renderRows() {
        const c = this.controller;
        const remembered = (c.rows || []).filter(row => row.known || row.connected);
        const available = (c.rows || []).filter(row => !row.known && !row.connected);
        const data = [...remembered, ...available];
        this.networkSeparator.visible = remembered.length > 0 && available.length > 0;
        const wanted = new Set(data.map(row => row.key));
        for (const [key, row] of this.rows) {
            if (wanted.has(key)) continue;
            this.networkRows.remove(row.widget);
            this.rows.delete(key);
            this.profileChoices.delete(key);
        }
        let previous = null, separated = false;
        for (const dataRow of data) {
            if (this.networkSeparator.visible && !dataRow.known && !dataRow.connected && !separated) {
                this.networkRows.reorder_child_after(this.networkSeparator, previous);
                previous = this.networkSeparator;
                separated = true;
            }
            const row = this.rows.get(dataRow.key) || this.createRow(dataRow.key);
            row.data = dataRow;
            row.name.set_label(`${dataRow.kind === 'WIRED' ? 'Wired' : 'Wi-Fi'} · ${dataRow.name}`);
            row.metadata.set_label([dataRow.connected ? 'Connected' : dataRow.known ? 'Saved' : 'Not saved',
                dataRow.interface, dataRow.wireless ? `${dataRow.strength}%` : '',
                dataRow.security || (dataRow.wireless ? 'Open' : ''), dataRow.changing ? 'Working…' : ''].filter(Boolean).join(' · '));
            const profiles = dataRow.profiles || [];
            const signature = JSON.stringify(profiles);
            if (signature !== row.profilesSignature) {
                row.updating = true;
                row.profileUuids = ['', ...profiles.map(profile => profile.uuid)];
                const choice = this.profileChoices.get(dataRow.key) || '';
                if (!row.profileUuids.includes(choice)) this.profileChoices.delete(dataRow.key);
                row.profile.set_model(Gtk.StringList.new(['Automatic (default)', ...profiles.map(profile => formatted(profile.id))]));
                row.profile.selected = Math.max(0, row.profileUuids.indexOf(this.profileChoices.get(dataRow.key) || ''));
                row.profilesSignature = signature;
                row.updating = false;
            }
            row.profile.visible = profiles.length > 1;
            row.connect.get_child().set_label(dataRow.connected ? 'Disconnect' : 'Connect');
            row.connect.sensitive = !dataRow.changing && (!dataRow.wireless || (c.wifiEnabled && c.wifiHardwareEnabled));
            row.forget.visible = Boolean(dataRow.known);
            row.forget.sensitive = Boolean(dataRow.known && !dataRow.changing);
            row.info.visible = Boolean(dataRow.connected);
            row.qr.visible = Boolean(dataRow.connected && dataRow.wireless);
            row.actions.visible = row.forget.visible || row.info.visible || row.qr.visible;
            if (row.widget.get_prev_sibling() !== previous) this.networkRows.reorder_child_after(row.widget, previous);
            previous = row.widget;
        }
        this.empty.visible = data.length === 0;
    }

    clearPassword() { this.password.set_text(''); }

    submitPassword() {
        if (!this.controller.opened || !this.controller.credentialRow || !this.password.get_text()) return;
        try { this.controller.submitPassword(this.password.get_text()); }
        finally { this.clearPassword(); }
    }

    render() {
        if (this.disposed) return;
        const c = this.controller;
        const identity = JSON.stringify([Boolean(c.opened), c.monitor, c.page, c.credentialKey || '',
            c.credentialRow?.key || '', c.selectedRow?.key || '']);
        const changed = identity !== this.interactionIdentity;
        if (changed) this.clearPassword();
        this.interactionIdentity = identity;
        const shown = Boolean(c.opened && (this.monitor || monitorFor(c.monitor)));
        if (shown && (!this.monitor || this.monitor.get_connector() !== c.monitor)) this.setMonitor(monitorFor(c.monitor));
        if (shown !== this.presented) this.window.keymode = shown ? Astal.Keymode.EXCLUSIVE : Astal.Keymode.NONE;
        if (this.window.visible !== shown) this.window.visible = shown;

        message(this.wifiNotice, !c.wifiAvailable ? 'No Wi-Fi adapter'
            : !c.wifiHardwareEnabled ? 'Wi-Fi is hardware blocked' : !c.wifiEnabled ? 'Wi-Fi is off' : '');
        message(this.error, c.error);
        this.navigation.visible = c.page !== 'list';
        this.selectedName.set_label(c.selectedRow?.name || '');
        this.pages.visible_child_name = ['list', 'info', 'qr'].includes(c.page) ? c.page : 'list';
        this.renderRows();
        this.credentials.visible = Boolean(shown && c.page === 'list' && c.credentialRow);
        this.credentialName.set_label(this.credentials.visible ? `Password · ${c.credentialRow.name}` : '');
        this.submit.sensitive = this.password.get_text().length > 0;
        this.forget.visible = Boolean(shown && c.page === 'list' && c.forgetKey);
        this.renderInfo(shown && c.page === 'info');
        this.renderQr(shown && c.page === 'qr' ? c.qrImage || '' : '');
        if (shown && changed && this.credentials.visible) this.password.grab_focus();
        else if (shown && !this.presented) this.root.grab_focus();
        this.presented = shown;
    }

    renderInfo(active) {
        const c = this.controller, row = active ? c.selectedRow : null;
        this.summary.set_label(row ? `SSID / name: ${formatted(row.name)}\nInterface: ${formatted(row.interface)}\nSecurity: ${formatted(row.security)}\nSignal: ${formatted(row.strength)}%` : '');
        message(this.savedProfiles, row?.profiles?.length > 1
            ? `Saved profiles (ID / UUID):\n${formatted(row.profiles.map(profile => `${profile.id} (${profile.uuid})`))}` : '');
        for (const [key, title, wireless] of detailFields) {
            const widget = this.detailLabels.get(key);
            widget.visible = Boolean(row && (!wireless || row.wireless));
            widget.set_label(row ? `${title}: ${formatted(c.details?.[key])}` : '');
        }
        this.reveal.get_child().set_label(c.secretRequested ? 'Hide password' : 'Reveal password');
        this.reveal.sensitive = Boolean(active && !c.secretLoading && row?.wireless);
        this.secretLoading.visible = Boolean(active && c.secretLoading);
        message(this.secret, active && c.secretRequested ? c.secret || '' : '');
    }

    renderQr(data) {
        if (data !== this.qrKey) {
            this.qrKey = data;
            this.qrPixbuf = null;
            if (data) {
                try {
                    if (!/^data:image\/png;base64,[a-z\d+/=\s]+$/i.test(data) || data.length > 8 * 1024 * 1024)
                        throw new Error('Invalid QR image');
                    const loader = GdkPixbuf.PixbufLoader.new_with_type('png');
                    try { loader.write(GLib.base64_decode(data.slice(data.indexOf(',') + 1))); }
                    finally { loader.close(); }
                    const image = loader.get_pixbuf();
                    if (!image || image.width !== image.height || image.width > 4096) throw new Error('Invalid QR dimensions');
                    this.qrPixbuf = image;
                } catch (_) { /* Show a bounded, non-sensitive message rather than image data. */ }
            }
            this.qrDrawing.queue_draw();
        }
        const c = this.controller;
        this.qrStatus.set_label(c.qrLoading ? 'Generating QR code…' : data && !this.qrPixbuf ? 'Could not display QR code'
            : !data ? 'QR code unavailable' : 'Scan to join this network');
    }

    setMonitor(monitor) {
        if (this.monitor === monitor) return;
        if (this.monitorSignal) this.monitor.disconnect(this.monitorSignal);
        this.monitorSignal = 0;
        this.monitor = monitor;
        if (!monitor) return;
        this.monitorSignal = monitor.connect('notify::geometry', () => this.position());
        this.window.gdkmonitor = monitor;
        this.position();
    }

    position() {
        if (!this.monitor) return;
        const { width, height } = this.monitor.get_geometry();
        const cardW = Math.max(0, Math.min(420, width - 16));
        const cardH = Math.max(0, Math.min(520, height - 54));
        this.card.width_request = cardW;
        this.card.height_request = cardH;
        this.card.margin_top = Math.min(46, Math.max(0, height - cardH - 8));
        this.card.margin_end = Math.min(72, Math.max(8, width - cardW - 8));
        this.card.visible = cardW > 0 && cardH > 0;
    }

    updateMonitors() {
        if (!this.monitor) return;
        for (let index = 0; index < this.monitors.get_n_items(); index++)
            if (this.monitors.get_item(index) === this.monitor) return;
        this.close();
        this.setMonitor(null);
    }

    toggle(name = '') {
        const monitor = monitorFor(name);
        if (!monitor) throw new Error('No monitor is connected.');
        // Resolve even an omitted/stale connector before comparing in the controller.
        const connector = monitor.get_connector() || '';
        if (this.controller.opened && this.controller.monitor === connector) return this.close();
        this.clearPassword();
        this.setMonitor(monitor);
        this.controller.toggle(connector);
        if (this.controller.opened) this.dismissOtherPanels();
        this.render();
    }

    close() {
        this.clearPassword();
        if (this.controller.credentialKey || this.controller.credentialRow) this.controller.cancelCredentials();
        this.controller.hideSecret();
        this.controller.close();
        this.render();
    }

    dispose() {
        this.clearPassword();
        this.qrPixbuf = null;
        this.qrKey = '';
        this.secret.set_label('');
        this.disposed = true;
        this.monitors.disconnect(this.monitorsSignal);
        if (this.monitorSignal) this.monitor.disconnect(this.monitorSignal);
        this.window.keymode = Astal.Keymode.NONE;
        this.window.destroy();
    }
}

export const NetworkApplication = GObject.registerClass({ GTypeName: 'NetworkPanelApplication' },
    class NetworkApplication extends Astal.Application {
        handleRequest(request) {
            const command = parseRequest(request);
            if (!this.controller || !this.panel) throw new Error('Network panel is starting.');
            switch (command.command) {
            case 'status': return this.controller.status();
            case 'toggle': this.panel.toggle(command.monitor || ''); break;
            case 'close': this.panel.close(); break;
            case 'toggleWifi':
                this.controller.toggleWifi();
                if (this.controller.error) throw new Error(this.controller.error);
                break;
            default: throw new Error('Unknown network panel command.');
            }
            return 'ok';
        }

        vfunc_request(request, connection) {
            let response;
            try { response = this.handleRequest(request); }
            catch (error) { response = JSON.stringify({ error: error.message || String(error) }); }
            const close = () => connection.close_async(GLib.PRIORITY_DEFAULT, null, (source, result) => {
                try { source.close_finish(result); }
                catch (error) { console.warn(`Network request close: ${error.message}`); }
            });
            try {
                AstalIO.write_sock(connection, response, (_, result) => {
                    try { AstalIO.write_sock_finish(result); }
                    catch (error) { console.warn(`Network request reply: ${error.message}`); }
                    finally { close(); }
                });
            } catch (error) {
                console.warn(`Network request reply: ${error.message}`);
                close();
            }
        }

        startPanel() {
            if (this.started) return;
            this.started = true;
            const palette = loadTheme(GLib.getenv('NETWORK_PANEL_CONFIG'), GLib.getenv('NETWORK_PANEL_PALETTE'));
            const backend = new NetworkBackend({ onChange: () => this.panel?.render() });
            this.controller = new NetworkController({ backend, onChange: () => this.panel?.render(), ...controllerRuntime() });
            this.panel = new NetworkPanel({ app: this, controller: this.controller, palette });
            this.controller.start();
            this.panel.render();
        }

        disposePanel() {
            this.panel?.dispose();
            this.panel = null;
            this.controller?.dispose();
            this.controller = null;
        }
    });

// The same direct Astal entry is used by AGS; importing widgets for offline GTK
// tests must not acquire a socket, construct a backend, or contact other panels.
if (GLib.getenv('NETWORK_PANEL_IMPORT_ONLY') !== '1') {
    Gtk.init();
    GLib.unsetenv('LD_PRELOAD');
    const app = new NetworkApplication({ instance_name: 'network-panel' });
    let exitCode = 0;
    app.connect('activate', () => {
        try { app.startPanel(); }
        catch (error) { console.error(error); exitCode = 1; app.quit(); }
    });
    app.connect('shutdown', () => app.disposePanel());
    try {
        app.acquire_socket();
        // Astal acquires its IPC socket before GApplication registration. Do not
        // hold an unregistered process that can only answer "starting" forever.
        if (!app.register(null) || app.get_is_remote())
            throw new Error('Could not register the network panel application.');
    } catch (error) {
        try { app.quit(); } catch (_) { /* Registration may have failed before a connection existed. */ }
        printerr(`network-panel: ${error.message || String(error)}`);
        System.exit(1);
    }
    app.hold();
    const status = await app.runAsync([]);
    System.exit(exitCode || status);
}
