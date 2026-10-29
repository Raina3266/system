import GLib from 'gi://GLib';
import Gtk from 'gi://Gtk?version=4.0';
import Gdk from 'gi://Gdk?version=4.0';
import Pango from 'gi://Pango';
import Astal from 'gi://Astal?version=4.0';
import AstalIO from 'gi://AstalIO';
import GObject from 'gi://GObject';
import { exit } from 'system';
import { ClipboardBackend } from './backend.js';
import { ClipboardModel, TABS, editable, localFilePath, title } from './logic.js';
import { loadPanelStyle } from '../../live-style.js';

const PANEL_WIDTH = 450;

function read(path) {
    const [ok, bytes] = GLib.file_get_contents(path);
    if (!ok) throw new Error('Cannot load the clipboard theme.');
    return new TextDecoder().decode(bytes);
}

export function loadTheme(entry, palettePath) {
    const palette = JSON.parse(read(palettePath));
    for (const key of ['yellow', 'cyan', 'pink', 'red', 'separator', 'foreground', 'secondaryBackground', 'dimPink'])
        if (!/^#[a-f\d]{6}$/i.test(palette[key])) throw new Error(`Invalid clipboard palette: ${key}`);
    const definitions = '.clipboard-panel {\n' + Object.entries(palette)
        .filter(([key, value]) => /^[a-zA-Z]+$/.test(key) && /^#[a-f\d]{6}$/i.test(value))
        .map(([key, value]) => `    --cp_${key}: ${value};`).join('\n') + '\n}';
    loadPanelStyle({ entry, definitions, label: 'Clipboard' });
    return palette;
}

export function scheduleAutosave(callback) {
    const timer = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 400, () => {
        void callback();
        return GLib.SOURCE_REMOVE;
    });
    return () => GLib.source_remove(timer);
}

function box(vertical = false, spacing = 8) {
    return new Gtk.Box({ orientation: vertical ? Gtk.Orientation.VERTICAL : Gtk.Orientation.HORIZONTAL, spacing });
}
function label(text = '', css = '') {
    return new Gtk.Label({ label: text, xalign: 0, use_markup: false, css_classes: css ? [css] : [] });
}
function button(text, description, action) {
    const widget = new Gtk.Button({ label: text });
    widget.update_property([Gtk.AccessibleProperty.LABEL, Gtk.AccessibleProperty.DESCRIPTION], [text, description]);
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
        this.viewRows = [];
        this.rowTitles = [];
        this.titleCache = new WeakMap();
        this.home = GLib.get_home_dir();
        this.bufferText = '';
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
        this.card = new Gtk.Overlay({ width_request: PANEL_WIDTH, height_request: 560,
            halign: Gtk.Align.END, valign: Gtk.Align.START, margin_top: 40, css_classes: ['clipboard-card'] });
        this.card.set_child(frame(palette));
        this.content = box(true, 0);
        this.content.add_css_class('clipboard-content');
        this.card.add_overlay(this.content);
        this.revealer = new Gtk.Revealer({ child: this.card, halign: Gtk.Align.END, valign: Gtk.Align.START, margin_end: 16,
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
        this.search = new Gtk.SearchEntry({ placeholder_text: 'Search clipboard history…', hexpand: true,
            margin_bottom: 6, css_classes: ['clipboard-search'] });
        // SearchEntry has no public icon API for its built-in clear-icon tooltip.
        const clear = this.search.get_last_child();
        if (clear) {
            clear.set_tooltip_text(null);
            clear.update_property([Gtk.AccessibleProperty.LABEL], ['Clear search']);
        }
        this.search.connect('search-changed', () => {
            if (!this.rendering) void this.model.filter(this.search.get_text());
        });
        this.content.append(this.search);
        this.tabs = box(false, 4);
        this.tabs.homogeneous = true;
        this.tabButtons = TABS.map((tab, index) => {
            const widget = button(['MEMO', 'TEXT', 'FILES'][index], `Show ${tab} entries`,
                () => void this.model.changeTab(tab));
            widget.add_css_class('tab');
            widget.hexpand = true;
            widget.height_request = 32;
            this.tabs.append(widget);
            return widget;
        });
        const page = box(true, 0);
        page.add_css_class('clipboard-page');
        page.vexpand = true;
        page.append(this.tabs);
        this.content.append(page);
        this.body = box(true, 0);
        this.body.homogeneous = false;
        this.body.add_css_class('clipboard-page-body');
        this.body.vexpand = true;
        this.strings = Gtk.StringList.new([]);
        this.selection = new Gtk.SingleSelection({ model: this.strings, autoselect: false, can_unselect: true });
        const factory = new Gtk.SignalListItemFactory();
        factory.connect('setup', (_, item) => {
            const row = box(false, 6);
            row.add_css_class('history-item');
            row.append(new Gtk.Label({ xalign: 0, use_markup: false, ellipsize: Pango.EllipsizeMode.END,
                max_width_chars: 32, hexpand: true }));
            const badge = new Gtk.Label({ label: 'PINNED', css_classes: ['pin-badge'], valign: Gtk.Align.CENTER });
            badge.update_property([Gtk.AccessibleProperty.DESCRIPTION], ['Pinned — kept at the top of history']);
            row.append(badge);
            const actions = box(false, 2);
            const action = (text, css, perform) => {
                const control = button(text, '', () => {
                    const entry = item.get_item() && this.viewRows[item.get_position()];
                    if (entry) void perform(entry.id);
                });
                control.set_css_classes(['row-action', css]);
                control.width_request = 22;
                control.height_request = 22;
                control.valign = Gtk.Align.CENTER;
                actions.append(control);
            };
            action('󰐃', 'row-pin', id => this.model.pin(id));
            action('×', 'row-delete', id => this.model.remove(id));
            row.append(actions);
            item.set_child(row);
        });
        factory.connect('bind', (_, item) => {
            const row = item.get_child();
            const entry = this.viewRows[item.get_position()];
            const pinned = Boolean(entry?.pinned);
            const badge = row.get_first_child().get_next_sibling();
            const pin = row.get_last_child().get_first_child();
            const remove = row.get_last_child().get_last_child();
            row.get_first_child().set_label(item.get_item().get_string());
            badge.visible = pinned;
            pin.update_property([Gtk.AccessibleProperty.LABEL, Gtk.AccessibleProperty.DESCRIPTION],
                [pinned ? 'Unpin item' : 'Pin item', pinned ? 'Unpin this item' : 'Pin this item to the top']);
            const localFile = Boolean(localFilePath(entry));
            remove.update_property([Gtk.AccessibleProperty.LABEL, Gtk.AccessibleProperty.DESCRIPTION],
                [localFile ? 'Move file to Trash' : 'Delete item',
                    localFile ? 'Move the local file to Trash and remove this entry' : 'Delete this history entry']);
            if (pinned) { row.add_css_class('pinned'); pin.add_css_class('pinned'); }
            else { row.remove_css_class('pinned'); pin.remove_css_class('pinned'); }
        });
        this.list = new Gtk.ListView({ model: this.selection, factory, single_click_activate: false, css_classes: ['history'] });
        this.selection.connect('notify::selected', () => {
            const item = this.viewRows[this.selection.selected];
            if (!this.rendering && item && item.id !== this.model.editor?.id) void this.model.select(item.id);
        });
        this.list.connect('activate', (_, position) => {
            const entry = this.viewRows[position];
            if (entry) void this.copy(entry.id);
        });
        // Match five 40px history rows; only the preview takes extra height.
                this.listScroll = new Gtk.ScrolledWindow({ child: this.list, vexpand: false, height_request: 200,
            hscrollbar_policy: Gtk.PolicyType.NEVER, vscrollbar_policy: Gtk.PolicyType.AUTOMATIC });
        this.body.append(this.listScroll);

        const preview = box(true);
        preview.hexpand = true;
        preview.vexpand = true;

        this.editor = new Gtk.TextView({ monospace: true, wrap_mode: Gtk.WrapMode.WORD_CHAR,
            accepts_tab: true, hexpand: true, vexpand: true, left_margin: 8, right_margin: 8,
            top_margin: 8, bottom_margin: 8, pixels_below_lines: 2, pixels_inside_wrap: 2,
                        css_classes: ['editor'] });
        this.editor.buffer.connect('changed', () => {
            if (!this.rendering) {
                const buffer = this.editor.buffer;
                this.bufferText = buffer.get_text(buffer.get_start_iter(), buffer.get_end_iter(), true);
                this.model.edit(this.bufferText);
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
        page.append(this.body);

        this.failure = label('', 'failure');
        this.failure.margin_top = 12;
        this.failure.wrap = true;
        this.failure.max_width_chars = 40;
        this.content.append(this.failure);
    }
    renderRows(rows) {
        if (rows === this.viewRows) return;
        // Snapshot records are immutable; reuse previews when revisiting a tab.
        const titles = rows.map(item => {
            if (!this.titleCache.has(item))
                this.titleCache.set(item, title(item, this.home));
            return this.titleCache.get(item);
        });
        let start = 0, oldEnd = this.viewRows.length, newEnd = rows.length;
        const same = (oldIndex, newIndex) => this.viewRows[oldIndex].id === rows[newIndex].id
            && this.viewRows[oldIndex].pinned === rows[newIndex].pinned
            && this.rowTitles[oldIndex] === titles[newIndex];
        while (start < oldEnd && start < newEnd && same(start, start)) start++;
        while (oldEnd > start && newEnd > start && same(oldEnd - 1, newEnd - 1)) { oldEnd--; newEnd--; }
        this.viewRows = rows;
        this.rowTitles = titles;
        if (oldEnd > start || newEnd > start)
            this.strings.splice(start, oldEnd - start, titles.slice(start, newEnd));
    }
    render() {
        this.rendering = true;
        try {
            const model = this.model, item = model.editor;
            if (this.opened) this.backend.watchFiles(model.items);
            const rows = model.rows;
            this.renderRows(rows);
            const index = rows.findIndex(row => row.id === item?.id);
            const selected = index < 0 ? Gtk.INVALID_LIST_POSITION : index;
            if (this.selection.selected !== selected) {
                this.selection.selected = selected;
                if (index >= 0) this.list.scroll_to(index, Gtk.ListScrollFlags.NONE, null);
            }
            this.list.sensitive = !model.busy;
            this.tabButtons.forEach((button, index) => {
                button.sensitive = !model.busy;
                if (TABS[index] === model.tab) button.add_css_class('selected');
                else button.remove_css_class('selected');
            });
            if (this.search.get_text() !== model.query) this.search.set_text(model.query);

            if (item?.image_path) {
                if (this.imagePath !== item.image_path) this.picture.set_filename(item.image_path);
                this.imagePath = item.image_path;
                this.preview.visible_child_name = 'image';
            } else {
                this.picture.set_paintable(null);
                this.imagePath = null;
                const text = item ? (editable(item) ? item.text : item.name || item.text || '') : '';
                if (this.bufferText !== text) {
                    this.bufferText = text;
                    this.editor.buffer.set_text(text, -1);
                }
                this.preview.visible_child_name = item ? 'text' : 'empty';
            }
            this.editor.editable = Boolean(editable(item) && model.activity !== 'action');
            this.editor.cursor_visible = Boolean(editable(item));

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
            for (let target = focus; target && target !== this.list; target = target.get_parent())
                if (target instanceof Gtk.Button && target.has_css_class('row-action')) return false;
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
    async copy(id) {
        if (await this.model.copy(id)) this.hide();
    }
    async toggle(monitor) {
        if (this.opened && this.monitor === monitor) return this.close();
        if (this.model.busy) return false;
        this.monitor = monitor;
        this.window.gdkmonitor = monitor;
        const geometry = monitor.get_geometry();
        const width = Math.max(280, Math.min(PANEL_WIDTH, geometry.width - 32));
        this.card.width_request = width;
        const height = Math.max(280, Math.min(600, geometry.height - 80));
                const [contentHeight] = this.content.measure(Gtk.Orientation.VERTICAL, width);
        const [previewHeight] = this.preview.get_parent().measure(Gtk.Orientation.VERTICAL, width);
        const fixedHeight = contentHeight - previewHeight;
        // Two successive 20% preview reductions preserve the search, tabs and five-row list.
        this.card.height_request = Math.max(contentHeight, fixedHeight + Math.round((height - fixedHeight) * 0.64));
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
        this.backend.unwatchFiles();
        this.window.keymode = Astal.Keymode.NONE;
        this.revealer.reveal_child = false;
    }
    async close() {
        if (!this.opened) return true;
        if (!(await this.model.save())) return false;
        this.hide();
        return true;
    }
    dispose() {
            this.model.dispose();
            this.window.destroy();
        }
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
            app.backend = new ClipboardBackend({ onChange: options => { if (app.panel.opened) void app.model.refresh(options); } });
            app.model = new ClipboardModel({ request: message => app.backend.request(message),
                publish: payload => app.backend.publish(payload), scheduleAutosave,

                trashFile: path => app.backend.trashFile(path), onChange: () => app.panel?.render() });
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
