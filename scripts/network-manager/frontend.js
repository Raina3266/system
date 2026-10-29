import GLib from 'gi://GLib';
import GObject from 'gi://GObject';
import Gtk from 'gi://Gtk?version=4.0';
import Gdk from 'gi://Gdk?version=4.0';
import Pango from 'gi://Pango';
import Astal from 'gi://Astal?version=4.0';
import AstalIO from 'gi://AstalIO';
import System from 'system';
import { NetworkBackend, controllerRuntime, dismissOtherPanels } from './backend.js';
import { NetworkController, isUndetectedSaved, parseRequest, signalLabel } from './logic.js';
import { loadPanelStyle } from '../live-style.js';

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

function iconButton(icon, action, classes = []) {
    const widget = new Gtk.Button({ valign: Gtk.Align.CENTER, css_classes: ['control-button', 'icon-button', ...classes],
        child: new Gtk.Image({ icon_name: icon, pixel_size: 16 }) });
    widget.connect('clicked', action);
    return widget;
}

function profileDropDown() {
    const widget = new Gtk.DropDown({ hexpand: true, enable_search: false, css_classes: ['profile-choice'] });
    // GTK still builds a hidden search entry with a Clear Entry tooltip when
    // search is disabled. Clear defaults on this instance, including its popup.
    const clear = child => {
        child.set_tooltip_text(null);
        child.set_has_tooltip(false);
        for (let nested = child.get_first_child(); nested; nested = nested.get_next_sibling()) clear(nested);
    };
    clear(widget);
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
        this.listTab = 'nearby';
        this.monitor = null;
        this.monitorSignal = 0;
        this.presented = false;
        this.interactionIdentity = '';
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
        this.card = new Gtk.Overlay({ width_request: 380, height_request: 480,
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
        this.build(palette);
        this.monitors = Gdk.Display.get_default().get_monitors();
        this.monitorsSignal = this.monitors.connect('items-changed', () => this.updateMonitors());
        this.render();
    }

    build(palette) {

        this.wifiNotice = label('', ['warning']);
        this.error = label('', ['warning']);
        this.error.lines = 3;
        this.error.ellipsize = Pango.EllipsizeMode.END;
        this.dismissError = button('×', () => this.controller.dismissError(), ['icon-button']);
        this.dismissError.update_property([Gtk.AccessibleProperty.LABEL], ['Dismiss error message']);
        this.dismissError.valign = Gtk.Align.START;
        this.errorRow = box(false, 8, [this.error, this.dismissError]);
        this.content.append(this.wifiNotice);
        this.content.append(this.errorRow);
        this.pages = new Gtk.Stack({ transition_type: Gtk.StackTransitionType.NONE,
            hhomogeneous: false, vhomogeneous: false, hexpand: true, vexpand: true });
        this.networkRows = box(true, 8);
        this.networkSeparator = new Gtk.Separator({ css_classes: ['network-divider'], visible: false });
        this.networkRows.append(this.networkSeparator);
        this.empty = label('No networks available');
        this.empty.halign = Gtk.Align.CENTER;
        this.empty.valign = Gtk.Align.CENTER;
        this.list = new Gtk.Overlay({ hexpand: true, vexpand: true });
        this.list.set_child(scroll(this.networkRows, { vexpand: true }));
        this.list.add_overlay(this.empty);
        this.list.set_measure_overlay(this.empty, false);
        this.savedRows = box(true, 8);
        this.savedEmpty = label('No undetected saved networks');
        this.savedEmpty.halign = Gtk.Align.CENTER;
        this.savedEmpty.valign = Gtk.Align.CENTER;
        this.savedList = new Gtk.Overlay({ hexpand: true, vexpand: true });
        this.savedList.set_child(scroll(this.savedRows, { vexpand: true }));
        this.savedList.add_overlay(this.savedEmpty);
        this.savedList.set_measure_overlay(this.savedEmpty, false);
        this.networkLists = new Gtk.Stack({ transition_type: Gtk.StackTransitionType.NONE,
            hhomogeneous: false, vhomogeneous: false, hexpand: true, vexpand: true });
        this.networkLists.add_named(this.list, 'nearby');
        this.networkLists.add_named(this.savedList, 'saved');
        this.nearbyTab = button('NEARBY', () => this.selectListTab('nearby'), ['network-tab']);
        this.savedTab = button('SAVED', () => this.selectListTab('saved'), ['network-tab']);
        this.savedTab.update_property([Gtk.AccessibleProperty.DESCRIPTION],
            ['Saved Wi-Fi networks that are not currently detected']);
        const tabs = box(false, 4, [this.nearbyTab, this.savedTab]);
        tabs.homogeneous = true;
        this.listTabs = tabs;
        this.nearbyTab.hexpand = this.savedTab.hexpand = true;
        this.nearbyTab.height_request = this.savedTab.height_request = 32;
        this.listFrame = new Gtk.Overlay({ hexpand: true, vexpand: true });
        this.listFrame.set_child(new Gtk.Box({ hexpand: true, vexpand: true }));
        this.networkLists.margin_top = this.networkLists.margin_bottom = 6;
        this.networkLists.margin_start = this.networkLists.margin_end = 6;
        this.listFrame.add_overlay(this.networkLists);
        this.listFrame.set_measure_overlay(this.networkLists, false);
        this.pages.add_named(box(true, 0, [tabs, this.listFrame]), 'list');
        this.buildInfo();
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
        this.forgetMessage = label();
        this.forget = box(true, 8, [this.forgetMessage, box(false, 8, [this.forgetConfirm, this.forgetCancel])]);
        this.content.append(this.forget);

    }

    buildInfo() {
        this.summary = label();
        this.savedProfiles = label();
        this.infoProfile = profileDropDown();
        this.infoProfile.update_property([Gtk.AccessibleProperty.LABEL, Gtk.AccessibleProperty.DESCRIPTION],
            ['Saved profile', 'Saved profile whose password will be revealed']);
        const profileFactory = () => {
            const factory = new Gtk.SignalListItemFactory();
            factory.connect('setup', (_, item) => item.set_child(label('', [], false)));
            factory.connect('bind', (_, item) => item.get_child().set_label(item.get_item().get_string()));
            return factory;
        };
        this.infoProfile.factory = profileFactory();
        this.infoProfile.list_factory = profileFactory();
        this.infoProfileUuids = [];
        this.infoProfile.connect('notify::selected', () => {
            if (!this.updatingInfoProfile) this.controller.selectSavedProfile(this.infoProfileUuids[this.infoProfile.selected]);
        });
        this.detailLabels = new Map(detailFields.map(([key]) => [key, label()]));
        this.reveal = iconButton('view-reveal-symbolic', () => {
            if (this.controller.secretRequested) {
                this.maskSecret();
                this.controller.hideSecret();
            } else this.controller.revealSecret();
        }, ['password-reveal']);
        this.secretLoading = label('Loading…');
        this.secret = label('*******', ['network-secret']);
        this.passwordCaption = label('Password', ['network-info-heading']);
        this.passwordLine = box(false, 8, [this.secret, this.reveal]);
        const group = (title, children) => box(true, 4,
            [label(title, ['network-info-heading']), ...children]);
        this.profileGroup = group('Profiles', [this.savedProfiles, this.infoProfile,
            this.detailLabels.get('profileId'), this.detailLabels.get('profileUuid')]);
        this.addressGroup = group('IP addresses', ['ipv4', 'gateway4', 'ipv6', 'gateway6', 'dns']
            .map(key => this.detailLabels.get(key)));
        this.linkHeading = label('Wireless link', ['network-info-heading']);
        this.linkGroup = box(true, 4, [this.linkHeading, ...['mac', 'bssid', 'frequency', 'band', 'channel', 'mode', 'rate']
            .map(key => this.detailLabels.get(key))]);
        this.infoContent = box(true, 12, [group('Network', [this.summary]), this.profileGroup,
            this.addressGroup, this.linkGroup, this.passwordCaption, this.passwordLine, this.secretLoading]);
        this.infoContent.add_css_class('network-details');
        this.maskSecret();
    }

    createRow(key) {
        const row = { key, profilesSignature: '', profileUuids: [''], updating: false };
        row.marker = new Gtk.Image({ icon_name: 'network-wireless-symbolic', pixel_size: 12,
            can_target: false, focusable: false, css_classes: ['network-marker'] });
        row.name = label('', ['network-name']);
        row.metadata = label('', ['metadata']);
        row.status = label('', ['network-status'], false);
        row.status.hexpand = false;
        row.status.max_width_chars = 20;
        row.profile = profileDropDown();
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
        row.connect = iconButton('network-wireless-symbolic', () => {
            const current = this.controller.rows.find(value => value.key === key);
            if (!current) return;
            // Inline details leave connection controls visible; restore the credential form's list state first.
            if (this.controller.page === 'info') this.toggleInfo(this.controller.selectedKey);
            if (current.connected) this.controller.disconnectNetwork(key);
            else this.controller.connectNetwork(key, this.profileChoices.get(key) || '');
        });
        row.forget = button('×', () => this.controller.requestForget(key), ['network-remove']);
        row.remove = button('×', () => this.controller.requestForget(key), ['network-remove']);
        row.info = iconButton('pan-down-symbolic', () => this.toggleInfo(key), ['info-toggle']);
        row.info.hexpand = true;
        row.info.halign = Gtk.Align.END;
        row.heading = box(false, 4, [row.marker, row.name, row.status, row.connect, row.forget, row.remove]);
        row.secondary = box(false, 4, [row.metadata, row.info]);
        row.details = new Gtk.Revealer({ transition_type: Gtk.RevealerTransitionType.SLIDE_DOWN,
            transition_duration: 180, reveal_child: false });
        row.widget = box(true, 4, [row.heading, row.secondary, row.profile, row.details]);
        row.widget.add_css_class('network-row');
        this.rows.set(key, row);
        return row;
    }

    toggleInfo(key) {
        this.maskSecret();
        this.clearPassword();
        if (this.controller.page === 'info' && this.controller.selectedKey === key) {
            this.controller.back();
        } else {
            const row = this.controller.rows.find(row => row.key === key);
            if (!row || isUndetectedSaved(row) || this.listTab === 'saved') return;
            this.listTab = 'nearby';
            this.controller.info(key);
        }
    }

    selectListTab(tab) {
        if (!['nearby', 'saved'].includes(tab) || tab === this.listTab) return;
        this.maskSecret();
        this.listTab = tab;
        this.clearPassword();
        this.controller.back();
    }

    renderRows() {
        const c = this.controller;
        const saved = (c.rows || []).filter(isUndetectedSaved);
        const nearby = (c.rows || []).filter(row => !isUndetectedSaved(row));
        const remembered = nearby.filter(row => row.known || row.connected);
        const available = nearby.filter(row => !row.known && !row.connected);
        const wanted = new Set((c.rows || []).map(row => row.key));
        this.networkSeparator.visible = remembered.length > 0 && available.length > 0;
        for (const [key, row] of this.rows) {
            if (wanted.has(key)) continue;
            if (row.details.get_child() === this.infoContent) {
                this.maskSecret();
                row.details.set_child(null);
            }
            row.widget.remove_css_class('expanded');
            row.details.reveal_child = false;
            row.widget.get_parent()?.remove(row.widget);
            this.rows.delete(key);
            this.profileChoices.delete(key);
        }
        for (const [container, data] of [[this.networkRows, [...remembered, ...available]], [this.savedRows, saved]]) {
            let previous = null, separated = false;
            const undetected = container === this.savedRows;
            for (const dataRow of data) {
                if (!undetected && this.networkSeparator.visible && !dataRow.known && !dataRow.connected && !separated) {
                    this.networkRows.reorder_child_after(this.networkSeparator, previous);
                    previous = this.networkSeparator;
                    separated = true;
                }
                const row = this.rows.get(dataRow.key) || this.createRow(dataRow.key);
                if (row.widget.get_parent() !== container) {
                    // Keep the same widget and profile choice as scan results move it between tabs.
                    row.widget.get_parent()?.remove(row.widget);
                    container.append(row.widget);
                }
                row.data = dataRow;
                row.marker.icon_name = dataRow.wireless ? 'network-wireless-symbolic' : 'network-wired-symbolic';
                if (dataRow.wireless) row.marker.add_css_class('network-wifi-marker');
                else row.marker.remove_css_class('network-wifi-marker');
                const title = undetected ? `Wi-Fi - ${dataRow.security || 'Open'} - ${dataRow.name}`
                    : `${dataRow.kind === 'WIRED' ? 'Wired' : 'Wi-Fi'} · ${dataRow.name}`;
                row.name.set_label(title);
                const attributes = new Pango.AttrList();
                if (dataRow.wireless) {
                    const ranges = [[0, 5]];
                    if (undetected) ranges.push([8, 8 + new TextEncoder().encode(dataRow.security || 'Open').length]);
                    for (const [start, end] of ranges) {
                        const white = Pango.attr_foreground_new(65535, 65535, 65535);
                        white.start_index = start;
                        white.end_index = end;
                        attributes.insert(white);
                    }
                }
                row.name.set_attributes(attributes);
                row.name.wrap = !undetected;
                row.name.ellipsize = undetected ? Pango.EllipsizeMode.END : Pango.EllipsizeMode.NONE;
                row.status.visible = Boolean(dataRow.wireless && !undetected);
                row.status.set_label([dataRow.security || 'Open',
                    Number.isFinite(dataRow.strength) ? signalLabel(dataRow.strength) : ''].filter(Boolean).join(' · '));

                row.widget.spacing = undetected ? 0 : 4;
                for (const [name, enabled] of [['saved-network-row', undetected], ['connected', Boolean(dataRow.connected)]]) {
                    if (enabled) row.widget.add_css_class(name);
                    else row.widget.remove_css_class(name);
                }
                row.metadata.set_label([dataRow.connected ? 'Connected' : dataRow.wireless ? '' : dataRow.known ? 'Saved' : 'Not saved',
                    dataRow.wireless ? '' : dataRow.interface,
                    dataRow.wireless ? '' : dataRow.security, dataRow.changing ? 'Working…' : ''].filter(Boolean).join(' · '));
                row.metadata.visible = !undetected && Boolean(row.metadata.label);
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
                row.profile.update_property([Gtk.AccessibleProperty.LABEL], [`Saved profile for ${dataRow.name}`]);
                row.profile.visible = !undetected && profiles.length > 1;
                row.connect.visible = !undetected;
                row.connect.get_child().icon_name = dataRow.connected ? 'network-offline-symbolic'
                    : dataRow.wireless ? 'network-wireless-symbolic' : 'network-wired-symbolic';
                row.connect.update_property([Gtk.AccessibleProperty.LABEL],
                    [`${dataRow.connected ? 'Disconnect' : 'Connect'} ${dataRow.name}`]);
                row.connect.sensitive = !dataRow.changing && (!dataRow.wireless || (c.wifiEnabled && c.wifiHardwareEnabled));
                row.forget.visible = Boolean(dataRow.known && !undetected);
                row.forget.sensitive = Boolean(dataRow.known && !dataRow.changing);
                row.forget.update_property([Gtk.AccessibleProperty.LABEL], [`Forget saved network ${dataRow.name}`]);
                row.remove.visible = undetected;
                row.remove.sensitive = Boolean(dataRow.known && !dataRow.changing);
                row.remove.update_property([Gtk.AccessibleProperty.LABEL], [`Forget saved network ${dataRow.name}`]);
                row.info.visible = Boolean(!undetected && (dataRow.connected || dataRow.known));
                row.secondary.visible = row.metadata.visible || row.info.visible;
                if (row.widget.get_prev_sibling() !== previous) container.reorder_child_after(row.widget, previous);
                previous = row.widget;
            }
        }
        this.empty.visible = nearby.length === 0;
        this.savedEmpty.visible = saved.length === 0;
        this.savedTab.get_child().set_label(`SAVED (${saved.length})`);
        this.savedTab.update_property([Gtk.AccessibleProperty.LABEL], [`Saved networks (${saved.length})`]);
        this.networkLists.visible_child_name = this.listTab;

        for (const [tab, button] of [['nearby', this.nearbyTab], ['saved', this.savedTab]]) {
            if (tab === this.listTab) button.add_css_class('active');
            else button.remove_css_class('active');
        }
    }

    maskSecret() {
        this.secret.set_label('*******');
        this.secret.selectable = false;
        this.secretLoading.visible = false;
        this.reveal.get_child().icon_name = 'view-reveal-symbolic';
        this.reveal.update_property([Gtk.AccessibleProperty.LABEL], ['Reveal password']);
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
        const infoRow = c.page === 'info' ? c.selectedRow : null;
        if (c.page === 'info' && (!infoRow || isUndetectedSaved(infoRow) || this.listTab !== 'nearby')) {
            // A scan can move a saved selection into the hidden tab without invalidating its identity.
            this.maskSecret();
            this.clearPassword();
            c.back();
            return;
        }
        if (!c.opened || !infoRow || this.infoContent.get_parent() !== this.rows.get(infoRow.key)?.details
            || !c.secretRequested || c.secretLoading || !c.secret) this.maskSecret();
        if (!c.opened) this.listTab = 'nearby';
        const identity = JSON.stringify([Boolean(c.opened), c.monitor, c.page, c.credentialKey || '',
            c.credentialRow?.key || '', c.selectedRow?.key || '']);
        const changed = identity !== this.interactionIdentity;
        if (changed) this.clearPassword();
        this.interactionIdentity = identity;
        const shown = Boolean(c.opened && (this.monitor || monitorFor(c.monitor)));
        if (!shown) this.maskSecret();
        if (shown && (!this.monitor || this.monitor.get_connector() !== c.monitor)) this.setMonitor(monitorFor(c.monitor));
        if (shown !== this.presented) this.window.keymode = shown ? Astal.Keymode.EXCLUSIVE : Astal.Keymode.NONE;
        if (this.window.visible !== shown) this.window.visible = shown;

        message(this.wifiNotice, !c.wifiAvailable ? 'No Wi-Fi adapter'
            : !c.wifiHardwareEnabled ? 'Wi-Fi is hardware blocked' : !c.wifiEnabled ? 'Wi-Fi is off' : '');
        message(this.error, c.error);
        this.errorRow.visible = Boolean(c.error);
        this.pages.visible_child_name = 'list';
        this.renderRows();
        this.credentials.visible = Boolean(shown && c.page === 'list' && c.credentialRow);
        this.credentialName.set_label(this.credentials.visible ? `Password · ${c.credentialRow.name}` : '');
        this.submit.sensitive = this.password.get_text().length > 0;
        this.forget.visible = Boolean(shown && c.page === 'list' && c.forgetKey);
        const forgetting = c.rows.find(row => row.key === c.forgetKey);
        this.forgetMessage.set_label(forgetting
            ? `Forget all saved profiles for ${forgetting.name}${forgetting.connected ? ' and disconnect it' : ''}?` : '');
        this.renderInfo(shown && c.page === 'info');
        this.position();
        if (shown && changed && this.credentials.visible) this.password.grab_focus();
        else if (shown && !this.presented) this.root.grab_focus();
        this.presented = shown;
    }

    renderInfo(active) {
        const c = this.controller, row = active ? c.selectedRow : null;
        const selected = row ? this.rows.get(row.key) : null;
        const parent = this.infoContent.get_parent();
        // Clear the only plaintext widget before any revealer starts closing or changes owners.
        if (!selected || parent !== selected.details || !c.secretRequested || c.secretLoading || !c.secret)
            this.maskSecret();
        for (const current of this.rows.values()) {
            const expanded = current === selected;
            if (expanded) current.widget.add_css_class('expanded');
            else current.widget.remove_css_class('expanded');
            if (!expanded) current.details.reveal_child = false;
            current.info.get_child().icon_name = expanded ? 'pan-up-symbolic' : 'pan-down-symbolic';
            current.info.update_property([Gtk.AccessibleProperty.LABEL],
                [`${expanded ? 'Collapse' : 'Expand'} details for ${current.data?.name || current.key}`]);
            current.info.update_state([Gtk.AccessibleState.EXPANDED], [expanded]);
            if (expanded) current.info.add_css_class('expanded');
            else current.info.remove_css_class('expanded');
        }
        if (!selected) {
            this.infoProfile.visible = false;
            this.reveal.sensitive = false;
            return;
        }
        if (parent !== selected.details) {
            parent?.set_child(null);
            selected.details.set_child(this.infoContent);
        }
        this.summary.set_label(row ? `• SSID / name: ${formatted(row.name)}\n• Interface: ${formatted(row.interface)}\n• Security: ${formatted(row.security)}\n• Signal: ${signalLabel(row.strength)}` : '');
        message(this.savedProfiles, row?.profiles?.length > 1
            ? `Saved profiles (ID / UUID):\n${row.profiles.map(profile => `• ${formatted(profile.id)} (${formatted(profile.uuid)})`).join('\n')}` : '');
        const profiles = row?.wireless && row.known && !row.connected ? row.profiles || [] : [];
        const signature = JSON.stringify(profiles);
        this.updatingInfoProfile = true;
        if (signature !== this.infoProfileSignature) {
            this.infoProfileUuids = profiles.map(profile => profile.uuid);
            this.infoProfile.set_model(Gtk.StringList.new(profiles.map(profile => `${formatted(profile.id)} · ${profile.uuid}`)));
            this.infoProfileSignature = signature;
        }
        this.infoProfile.selected = Math.max(0, this.infoProfileUuids.indexOf(c.selectedProfileUuid));
        this.updatingInfoProfile = false;
        this.infoProfile.visible = profiles.length > 1;
        this.infoProfile.sensitive = Boolean(row && !row.changing);
        for (const [key, title, wireless] of detailFields) {
            const widget = this.detailLabels.get(key);
            widget.visible = Boolean(row && (row.connected || ['profileId', 'profileUuid'].includes(key))
                && (!wireless || row.wireless));
            widget.set_label(row ? `• ${title}: ${formatted(c.details?.[key])}` : '');
        }
        this.addressGroup.visible = this.linkGroup.visible = Boolean(row.connected);
        this.linkHeading.set_label(row.wireless ? 'Wireless link' : 'Wired link');
        const hasPassword = Boolean(row.wireless && !['open', 'owe'].includes(row.securityType)
                    && (row.connected || row.known));
        const requested = Boolean(hasPassword && c.secretRequested);
        const revealed = Boolean(requested && !c.secretLoading && c.secret);
        this.reveal.get_child().icon_name = requested ? 'view-conceal-symbolic' : 'view-reveal-symbolic';
        this.reveal.update_property([Gtk.AccessibleProperty.LABEL], [requested ? 'Hide password' : 'Reveal password']);
        this.passwordCaption.visible = this.passwordLine.visible = this.reveal.visible = hasPassword;
        // Keep Hide usable while authorization is pending, so it can cancel the read.
        this.reveal.sensitive = Boolean(requested || (hasPassword && !row.changing));
        this.secretLoading.visible = Boolean(hasPassword && c.secretLoading);
        this.secret.set_label(revealed ? c.secret : '*******');
        this.secret.selectable = revealed;
        selected.details.reveal_child = true;
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

    preferredHeight(width) {
        const nearby = (this.controller.rows || []).filter(row => !isUndetectedSaved(row));
        const remembered = nearby.filter(row => row.known || row.connected);
        const available = nearby.filter(row => !row.known && !row.connected).slice(0, 6);
        if (!available.length || available.some(row => !this.rows.has(row.key))) return 480;
        const padding = this.content.get_style_context().get_padding();
        const rowWidth = Math.max(1, width - padding.left - padding.right
            - this.networkLists.margin_start - this.networkLists.margin_end);
        const rowHeight = data => {
            const row = this.rows.get(data.key);
            if (!row) return 0;
            const style = row.widget.get_style_context();
            const padding = style.get_padding(), border = style.get_border();
            const innerWidth = Math.max(1, rowWidth - padding.left - padding.right - border.left - border.right);
            // Expanding details should scroll inside the list, not resize the whole card.
            return row.widget.measure(Gtk.Orientation.VERTICAL, rowWidth)[1]
                - row.details.measure(Gtk.Orientation.VERTICAL, innerWidth)[1];
        };
        const heights = [...remembered, ...available].map(rowHeight);
        while (heights.length < remembered.length + 6) heights.push(rowHeight(available.at(-1)));
        let height = heights.reduce((sum, value) => sum + value, 0)
            + Math.max(0, heights.length - 1) * this.networkRows.spacing;
        if (remembered.length) height += this.networkSeparator.measure(Gtk.Orientation.VERTICAL, rowWidth)[1]
            + this.networkRows.spacing;
        return Math.ceil(height + padding.top + padding.bottom
            + this.listTabs.measure(Gtk.Orientation.VERTICAL, width - padding.left - padding.right)[1]
            + this.networkLists.margin_top + this.networkLists.margin_bottom);
    }

    position() {
        if (!this.monitor) return;
        const { width, height } = this.monitor.get_geometry();
        const cardW = Math.max(0, Math.min(380, width - 16));
        const cardH = Math.max(0, Math.min(this.preferredHeight(cardW), height - 54));
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
        this.maskSecret();
        this.clearPassword();
        this.setMonitor(monitor);
        this.controller.toggle(connector);
        if (this.controller.opened) this.dismissOtherPanels();
        this.render();
    }

    close() {
        this.maskSecret();
        this.clearPassword();
        if (this.controller.credentialKey || this.controller.credentialRow) this.controller.cancelCredentials();
        this.controller.hideSecret();
        this.controller.close();
        this.render();
    }

    dispose() {
        this.maskSecret();
        this.clearPassword();
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
