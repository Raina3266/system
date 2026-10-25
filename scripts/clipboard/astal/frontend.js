import GLib from 'gi://GLib';
import Gtk from 'gi://Gtk?version=4.0';
import Gdk from 'gi://Gdk?version=4.0';
import Pango from 'gi://Pango';
import Astal from 'gi://Astal?version=4.0';
import AstalIO from 'gi://AstalIO';
import GObject from 'gi://GObject';
import { exit } from 'system';
import { ClipboardBackend } from './backend.js';
import { ClipboardModel, TABS, editable, isDraft, title } from './logic.js';

function read(path) {
    const [ok, bytes] = GLib.file_get_contents(path);
    if (!ok) throw new Error('Cannot load the clipboard theme.');
    return new TextDecoder().decode(bytes);
}

export function loadTheme(entry, palettePath) {
    const palette = JSON.parse(read(palettePath));
    for (const key of ['yellow', 'cyan', 'pink', 'red', 'separator', 'foreground', 'secondaryBackground', 'dimPink'])
        if (!/^#[a-f\d]{6}$/i.test(palette[key])) throw new Error(`Invalid clipboard palette: ${key}`);
    const definitions = Object.entries(palette)
        .filter(([key, value]) => /^[a-zA-Z]+$/.test(key) && /^#[a-f\d]{6}$/i.test(value))
        .map(([key, value]) => `@define-color cp_${key} ${value};`).join('\n');
    const provider = new Gtk.CssProvider();
    const errors = [];
    provider.connect('parsing-error', (_, section, error) => errors.push(error.message));
    provider.load_from_string(`${definitions}\n${read(GLib.build_filenamev([GLib.path_get_dirname(entry), 'style.css']))}`);
    if (errors.length) throw new Error(`Clipboard stylesheet: ${errors.join('; ')}`);
    Gtk.StyleContext.add_provider_for_display(Gdk.Display.get_default(), provider, Gtk.STYLE_PROVIDER_PRIORITY_USER + 1);
    return palette;
}

function box(vertical = false, spacing = 8) {
    return new Gtk.Box({ orientation: vertical ? Gtk.Orientation.VERTICAL : Gtk.Orientation.HORIZONTAL, spacing });
}
function label(text = '', css = '') {
    return new Gtk.Label({ label: text, xalign: 0, use_markup: false, css_classes: css ? [css] : [] });
}
function button(text, tooltip, action) {
    const widget = new Gtk.Button({ label: text, tooltip_text: tooltip });
    widget.connect('clicked', action);
    return widget;
}
function frame(palette) {
    const drawing = new Gtk.DrawingArea({ hexpand: true, vexpand: true, can_target: false });
    drawing.set_draw_func((_, cr, width, height) => {
        cr.moveTo(10.5, 0.5);
        cr.lineTo(width - 0.5, 0.5);
        cr.lineTo(width - 0.5, height - 10.5);
        cr.lineTo(width - 10.5, height - 0.5);
        cr.lineTo(0.5, height - 0.5);
        cr.lineTo(0.5, 10.5);
        cr.closePath();
        cr.setSourceRGBA(37 / 255, 37 / 255, 47 / 255, 0.95);
        cr.fillPreserve();
        cr.setSourceRGBA(...palette.yellow.slice(1).match(/../g).map(value => parseInt(value, 16) / 255), 1);
        cr.setLineWidth(1);
        cr.stroke();
    });
    return drawing;
}

export class ClipboardPanel {
    constructor({ app, model, backend, palette }) {
        Object.assign(this, { model, backend });
        this.opened = false;
        this.rendering = false;
        this.rowWidgets = new Map();
        this.window = new Astal.Window({
            application: app, name: 'clipboard-panel', namespace: 'astal:clipboard',
            anchor: Astal.WindowAnchor.TOP | Astal.WindowAnchor.BOTTOM | Astal.WindowAnchor.LEFT | Astal.WindowAnchor.RIGHT,
            layer: Astal.Layer.OVERLAY, exclusivity: Astal.Exclusivity.IGNORE,
            keymode: Astal.Keymode.NONE, visible: false,
        });
        this.window.add_css_class('clipboard-panel');
        this.window.connect('close-request', () => { void this.close(); return true; });
        this.root = new Gtk.Overlay({ hexpand: true, vexpand: true });
        this.root.set_child(new Gtk.Box({ hexpand: true, vexpand: true }));
        this.card = new Gtk.Overlay({ width_request: 900, height_request: 560,
            halign: Gtk.Align.CENTER, valign: Gtk.Align.START, margin_top: 40, css_classes: ['clipboard-card'] });
        this.card.set_child(frame(palette));
        this.content = box(true, 12);
        this.content.add_css_class('clipboard-content');
        this.card.add_overlay(this.content);
        this.revealer = new Gtk.Revealer({ child: this.card, halign: Gtk.Align.CENTER, valign: Gtk.Align.START,
            transition_type: Gtk.RevealerTransitionType.CROSSFADE, transition_duration: 180 });
        this.revealer.connect('notify::child-revealed', () => {
            if (!this.opened && !this.revealer.child_revealed) this.window.visible = false;
        });
        this.root.add_overlay(this.revealer);
        this.window.set_child(this.root);
        this.build();
        const outside = new Gtk.GestureClick({ propagation_phase: Gtk.PropagationPhase.CAPTURE });
        outside.connect('released', (_, count, x, y) => {
            const [ok, bounds] = this.card.compute_bounds(this.root);
            if (ok && (x < bounds.get_x() || x > bounds.get_x() + bounds.get_width()
                || y < bounds.get_y() || y > bounds.get_y() + bounds.get_height())) void this.close();
        });
        this.root.add_controller(outside);
        const keys = new Gtk.EventControllerKey({ propagation_phase: Gtk.PropagationPhase.CAPTURE });
        keys.connect('key-pressed', (_, key, code, state) => this.key(key, state));
        this.window.add_controller(keys);
        this.render();
    }
    build() {
        const header = box();
        const heading = label('󰅌  Clipboard', 'title');
        heading.hexpand = true;
        header.append(heading);
        this.newButton = button('+ Memo', 'Create a memo (Ctrl+N)', () => void this.newMemo());
        this.closeButton = button('󰅖', 'Save and close (Escape)', () => void this.close());
        header.append(this.newButton);
        header.append(this.closeButton);
        this.content.append(header);

        this.search = new Gtk.SearchEntry({ placeholder_text: 'Search clipboard history…', hexpand: true });
        this.search.connect('search-changed', () => {
            if (!this.rendering) void this.model.filter(this.search.get_text());
        });
        this.content.append(this.search);
        this.tabs = box();
        this.tabButtons = TABS.map((tab, index) => {
            const widget = button(['󰍩  Memo', '󰦨  Text', '󰈔  Files'][index], `Show ${tab} entries`,
                () => void this.model.changeTab(tab));
            widget.hexpand = true;
            this.tabs.append(widget);
            return widget;
        });
        this.content.append(this.tabs);
        this.body = box(false, 14);
        this.body.vexpand = true;
        this.list = new Gtk.ListBox({ selection_mode: Gtk.SelectionMode.SINGLE, css_classes: ['history'] });
        this.list.connect('row-selected', (_, row) => {
            if (!this.rendering && row && row.itemId !== this.model.editor?.id) void this.model.select(row.itemId);
        });
        this.list.connect('row-activated', () => void this.copy());
        this.listScroll = new Gtk.ScrolledWindow({ child: this.list, vexpand: true,
            hscrollbar_policy: Gtk.PolicyType.NEVER, vscrollbar_policy: Gtk.PolicyType.AUTOMATIC });
        this.body.append(this.listScroll);

        const preview = box(true);
        preview.hexpand = true;
        this.detail = label('', 'metadata');
        this.detail.ellipsize = Pango.EllipsizeMode.MIDDLE;
        preview.append(this.detail);
        this.editor = new Gtk.TextView({ monospace: true, wrap_mode: Gtk.WrapMode.WORD_CHAR,
            accepts_tab: true, hexpand: true, vexpand: true, left_margin: 12, right_margin: 12,
            top_margin: 12, bottom_margin: 12, css_classes: ['editor'] });
        this.editor.buffer.connect('changed', () => {
            if (!this.rendering) {
                const buffer = this.editor.buffer;
                this.model.edit(buffer.get_text(buffer.get_start_iter(), buffer.get_end_iter(), true));
            }
        });
        this.picture = new Gtk.Picture({ can_shrink: true, content_fit: Gtk.ContentFit.CONTAIN,
            hexpand: true, vexpand: true, css_classes: ['image-preview'] });
        this.preview = new Gtk.Stack({ hexpand: true, vexpand: true });
        this.preview.add_named(new Gtk.ScrolledWindow({ child: this.editor,
            hscrollbar_policy: Gtk.PolicyType.NEVER, vscrollbar_policy: Gtk.PolicyType.AUTOMATIC }), 'text');
        this.preview.add_named(this.picture, 'image');
        this.preview.add_named(label('Select an item to preview', 'empty'), 'empty');
        preview.append(this.preview);
        this.body.append(preview);
        this.content.append(this.body);
        const toolbar = box();
        this.pinButton = button('󰐃 Pin', 'Pin/unpin (Alt+P)', () => void this.model.pin());
        this.deleteButton = button('󰆴 Delete', 'Delete this item (Alt+D); discards its unsaved draft',
            () => void this.model.remove());
        this.reloadButton = button('Reload', 'Discard your draft and reload this item', () => {
            if (!this.model.dirty) { void this.model.reload(); return; }
            const dialog = new Gtk.AlertDialog({ message: 'Discard your unsaved changes?',
                detail: 'Reload the stored item without saving your draft.',
                buttons: ['Cancel', 'Discard and reload'], cancel_button: 0, default_button: 0 });
            dialog.choose(this.window, null, (source, result) => {
                try { if (source.choose_finish(result) === 1) void this.model.reload(); } catch (_) { /* Cancelled. */ }
            });
        });
        this.saveButton = button('Save', 'Save changes (Ctrl+S)', () => void this.model.save());
        this.copyButton = button('󰅍 Copy', 'Copy and close (Enter; Ctrl+Enter in the editor)', () => void this.copy());
        const spacer = new Gtk.Box({ hexpand: true });
        for (const child of [this.pinButton, this.deleteButton, this.reloadButton, spacer, this.saveButton, this.copyButton])
            toolbar.append(child);
        this.content.append(toolbar);
        this.status = label('', 'metadata');
        this.failure = label('', 'failure');
        this.failure.wrap = true;
        this.failure.max_width_chars = 80;
        this.content.append(this.status);
        this.content.append(this.failure);
    }
    render() {
        this.rendering = true;
        try {
            const model = this.model, item = model.editor;
            const rows = model.rows, wanted = new Set(rows.map(row => row.id));
            for (const [id, row] of this.rowWidgets) {
                if (!wanted.has(id)) { this.list.remove(row); this.rowWidgets.delete(id); }
            }
            rows.forEach((item, index) => {
                let row = this.rowWidgets.get(item.id);
                if (!row) {
                    row = new Gtk.ListBoxRow();
                    row.itemId = item.id;
                    const text = label();
                    text.ellipsize = Pango.EllipsizeMode.END;
                    text.max_width_chars = 32;
                    text.set_margin_start(8);
                    text.set_margin_end(8);
                    row.set_child(text);
                    this.rowWidgets.set(item.id, row);
                }
                const text = `${item.pinned ? '󰐃 ' : ''}${title(item, GLib.get_home_dir())}`;
                if (row.get_child().label !== text) row.get_child().set_label(text);
                if (row.get_index() !== index) {
                    if (row.get_parent()) this.list.remove(row);
                    this.list.insert(row, index);
                }
            });
            this.list.select_row(this.rowWidgets.get(item?.id) || null);
            this.list.sensitive = !model.busy;
            this.tabButtons.forEach((button, index) => {
                button.sensitive = !model.busy;
                if (TABS[index] === model.tab) button.add_css_class('active');
                else button.remove_css_class('active');
            });
            if (this.search.get_text() !== model.query) this.search.set_text(model.query);
            this.detail.set_label(item ? `${isDraft(item) ? 'New memo' : item.mime}${model.dirty ? ' · Unsaved' : ''}` : 'Preview');
            if (item?.image_path) {
                if (this.imagePath !== item.image_path) this.picture.set_filename(item.image_path);
                this.imagePath = item.image_path;
                this.preview.visible_child_name = 'image';
            } else {
                this.picture.set_paintable(null);
                this.imagePath = null;
                const text = item ? (editable(item) ? item.text : item.name || item.text || '') : '';
                const buffer = this.editor.buffer;
                if (buffer.get_text(buffer.get_start_iter(), buffer.get_end_iter(), true) !== text) buffer.set_text(text, -1);
                this.preview.visible_child_name = item ? 'text' : 'empty';
            }
            this.editor.editable = Boolean(editable(item) && model.activity !== 'action');
            this.editor.cursor_visible = Boolean(editable(item));
            this.pinButton.set_label(item?.pinned ? '󰐃 Unpin' : '󰐃 Pin');
            for (const widget of [this.pinButton, this.deleteButton, this.copyButton])
                widget.sensitive = Boolean(item && !model.busy);
            this.saveButton.sensitive = Boolean(model.dirty && !model.busy);
            this.reloadButton.sensitive = Boolean(item && !model.busy);
            this.newButton.sensitive = !model.busy;
            this.closeButton.sensitive = !model.busy;
            this.status.set_label(model.busy ? 'Working…' : `${rows.length} ${rows.length === 1 ? 'item' : 'items'} · Changes save on selection or close`);
            this.failure.set_label(model.failure);
            this.failure.visible = Boolean(model.failure);
        } finally { this.rendering = false; }
    }
    key(key, state) {
        const ctrl = Boolean(state & Gdk.ModifierType.CONTROL_MASK);
        const alt = Boolean(state & Gdk.ModifierType.ALT_MASK);
        const lower = Gdk.keyval_to_lower(key);
        if (key === Gdk.KEY_Escape) { void this.close(); return true; }
        if (ctrl && lower === Gdk.KEY_s) { void this.model.save(); return true; }
        if (ctrl && lower === Gdk.KEY_n) { void this.newMemo(); return true; }
        if (ctrl && lower === Gdk.KEY_f) { this.search.grab_focus(); return true; }
        if (alt && lower === Gdk.KEY_p) { void this.model.pin(); return true; }
        if (alt && lower === Gdk.KEY_d) { void this.model.remove(); return true; }
        if (alt && lower === Gdk.KEY_e) { this.editor.grab_focus(); return true; }
        const focus = this.window.get_focus();
        const inEditor = focus === this.editor || focus?.is_ancestor(this.editor);
        if ([Gdk.KEY_Return, Gdk.KEY_KP_Enter].includes(key) && (ctrl || !inEditor)) {
            void this.copy();
            return true;
        }
        if (!inEditor && [Gdk.KEY_Down, Gdk.KEY_Up].includes(key)) {
            const rows = this.model.rows;
            const index = rows.findIndex(item => item.id === this.model.editor?.id);
            const target = rows[Math.max(0, Math.min(rows.length - 1, index + (key === Gdk.KEY_Down ? 1 : -1)))];
            if (target) void this.model.select(target.id);
            return true;
        }
        return false;
    }
    async newMemo() {
        if (await this.model.newMemo()) this.editor.grab_focus();
    }
    async copy() {
        if (await this.model.copy()) this.hide();
    }
    async toggle(monitor) {
        if (this.opened && this.monitor === monitor) return this.close();
        if (this.model.busy) return false;
        this.monitor = monitor;
        this.window.gdkmonitor = monitor;
        const geometry = monitor.get_geometry();
        const width = Math.max(280, Math.min(900, geometry.width - 32));
        this.card.width_request = width;
        this.card.height_request = Math.max(280, Math.min(600, geometry.height - 80));
        this.body.orientation = width >= 740 ? Gtk.Orientation.HORIZONTAL : Gtk.Orientation.VERTICAL;
        this.listScroll.width_request = width >= 740 ? Math.floor(width * 0.34) : -1;
        this.listScroll.height_request = width >= 740 ? -1 : 120;
        this.opened = true;
        this.window.keymode = Astal.Keymode.EXCLUSIVE;
        this.window.visible = true;
        this.revealer.reveal_child = true;
        if (await this.model.open()) this.backend.watch();
        this.search.grab_focus();
        return true;
    }
    hide() {
        this.opened = false;
        this.window.keymode = Astal.Keymode.NONE;
        this.revealer.reveal_child = false;
    }
    async close() {
        if (!this.opened) return true;
        if (!(await this.model.save())) return false;
        this.hide();
        return true;
    }
    dispose() { this.window.destroy(); }
}

export function createApplication() {
    const Application = GObject.registerClass(class ClipboardApplication extends Astal.Application {
        vfunc_request(request, connection) {
            const reply = async () => {
                const message = JSON.parse(request);
                if (!this.panel) throw new Error('Clipboard panel is starting.');
                switch (message.command) {
                case 'status': return JSON.stringify({ text: '󰅌', tooltip: 'Clipboard panel · history and preview',
                    class: this.panel.opened ? 'open' : 'closed' });
                case 'close': if (!(await this.panel.close())) throw new Error(this.model.failure || 'Clipboard panel is busy.'); break;
                case 'toggle': {
                    if (message.monitor !== undefined && typeof message.monitor !== 'string') throw new Error('Invalid monitor.');
                    const monitor = this.get_monitors().find(monitor => !message.monitor || monitor.get_connector() === message.monitor);
                    if (!monitor) throw new Error('The requested monitor is no longer connected.');
                    if (!(await this.panel.toggle(monitor))) throw new Error(this.model.failure || 'Clipboard panel is busy.');
                    break;
                }
                default: throw new Error('Unknown clipboard panel command.');
                }
                return 'ok';
            };
            void reply().catch(error => JSON.stringify({ error: error.message })).then(response => {
                const close = () => {
                    try { connection.close(null); }
                    catch (error) { console.warn(`Clipboard request close: ${error.message}`); }
                };
                try {
                    AstalIO.write_sock(connection, response, (_, result) => {
                        try { AstalIO.write_sock_finish(result); }
                        catch (error) { console.warn(`Clipboard request reply: ${error.message}`); }
                        finally { close(); }
                    });
                } catch (error) {
                    close();
                    console.warn(`Clipboard request reply: ${error.message}`);
                }
            });
        }
    });
    return new Application({ instance_name: 'clipboard-panel' });
}

export function main() {
    Gtk.init();
    GLib.unsetenv('LD_PRELOAD');
    const app = createApplication();
    app.connect('activate', () => {
        if (app.panel) return;
        try {
            const palette = loadTheme(GLib.getenv('CLIPBOARD_PANEL_CONFIG'), GLib.getenv('CLIPBOARD_PANEL_PALETTE'));
            app.backend = new ClipboardBackend({ onChange: () => { if (app.panel.opened) void app.model.refresh(); } });
            app.model = new ClipboardModel({ request: message => app.backend.request(message),
                publish: payload => app.backend.publish(payload), onChange: () => app.panel?.render() });
            app.panel = new ClipboardPanel({ app, model: app.model, backend: app.backend, palette });
            app.connect('notify::monitors', () => {
                if (app.panel.monitor && !app.get_monitors().includes(app.panel.monitor)) void app.panel.close();
            });
        } catch (error) {
            console.error(`Clipboard panel initialization: ${error.message}`);
            app.quit();
            exit(1);
        }
    });
    app.connect('shutdown', () => { app.backend?.dispose(); app.panel?.dispose(); });
    try { app.acquire_socket(); }
    catch (error) { console.error(`Clipboard panel startup: ${error.message}`); exit(1); }
    app.hold();
    return app.runAsync([]);
}

// Tests import the real classes without acquiring a socket or touching history.
if (GLib.getenv('CLIPBOARD_PANEL_IMPORT_ONLY') !== '1') await main();
