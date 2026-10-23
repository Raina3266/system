import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Notifd from 'gi://AstalNotifd';
import Gtk from 'gi://Gtk?version=4.0';
import Gdk from 'gi://Gdk?version=4.0';
import Pango from 'gi://Pango';
import Astal from 'gi://Astal?version=4.0';
import Gtk4LayerShell from 'gi://Gtk4LayerShell';
import type { DashboardBackend } from './power.ts';

export type NotificationRow = {
    id: string;
    app: string;
    summary: string;
    body: string;
    icon: string;
    urgency: number;
    transient: boolean;
    timeout: number;
    actions: { identifier: string; text: string }[];
    closed: boolean;
};

export const HISTORY_LIMIT = 200;
export const POPUP_LIMIT = 5;
export const DEFAULT_TIMEOUT = 8000;

export function newestFirst(rows: NotificationRow[], row: NotificationRow, limit: number): NotificationRow[] {
    return [row, ...rows.filter(existing => existing.id !== row.id)].slice(0, limit);
}

export function archived(row: NotificationRow): NotificationRow {
    return { ...row, actions: [], closed: true, icon: row.icon.startsWith('image://') ? '' : row.icon };
}

export function notificationTimeout(row: Pick<NotificationRow, 'timeout' | 'urgency'>): number {
    if (row.timeout === 0 || row.urgency === 2) return 0;
    return row.timeout > 0 ? row.timeout : DEFAULT_TIMEOUT;
}

export function restoreArchive(text: string, limit = HISTORY_LIMIT): { history: NotificationRow[]; dnd: boolean } {
    if (!text.trim()) return { history: [], dnd: false };
    const state = JSON.parse(text);
    if (!state || state.version !== 1 || !Array.isArray(state.history))
        throw new Error('Unsupported notification history format.');
    const history: NotificationRow[] = [];
    const ids = new Set<string>();
    for (const value of state.history) {
        if (!value || typeof value !== 'object' || Array.isArray(value)
            || !['id', 'app', 'summary', 'body'].every(key => typeof value[key] === 'string')
            || !value.id || ids.has(value.id) || value.transient === true) continue;
        ids.add(value.id);
        history.push(archived({
            id: value.id, app: value.app, summary: value.summary, body: value.body,
            icon: typeof value.icon === 'string' ? value.icon : '',
            urgency: [0, 1, 2].includes(value.urgency) ? value.urgency : 1,
            transient: false,
            timeout: Number.isInteger(value.timeout) && value.timeout >= -1 && value.timeout <= 0x7fffffff ? value.timeout : -1,
            actions: [], closed: true,
        }));
        if (history.length >= limit) break;
    }
    return { history, dnd: state.dnd === true };
}

export function serializeArchive(history: NotificationRow[], dnd: boolean): string {
    return JSON.stringify({ version: 1, dnd, history: history.filter(row => !row.transient).slice(0, HISTORY_LIMIT).map(archived) });
}

type LiveNotification = { native: Notifd.Notification; key: string; timer: number; hovered: boolean };

function snapshot(native: Notifd.Notification, id: string): NotificationRow {
    return {
        id, app: native.app_name || '', summary: native.summary || '', body: native.body || '',
        icon: native.image || native.app_icon || '', urgency: native.urgency,
        transient: native.transient, timeout: native.expire_timeout,
        actions: (native.actions || []).map((action: Notifd.Action) => ({ identifier: action.id, text: action.label })),
        closed: false,
    };
}

function readArchive(path: string): string | null {
    const file = Gio.File.new_for_path(path);
    try {
        // A malformed/unbounded file must not turn startup into an unbounded read.
        if (file.query_info('standard::size', Gio.FileQueryInfoFlags.NONE, null).get_size() > 8 * 1024 * 1024)
            throw new Error('Notification archive exceeds 8 MiB.');
        const [ok, bytes] = file.load_contents(null);
        if (!ok) throw new Error('Could not load notification archive.');
        return new TextDecoder().decode(bytes);
    } catch (error: any) {
        if (error.matches?.(Gio.io_error_quark(), Gio.IOErrorEnum.NOT_FOUND)) return null;
        console.warn(`notifications: could not read ${path}: ${error.message}`);
        return '';
    }
}

export class NotificationCenter {
    history: NotificationRow[] = [];
    popups: NotificationRow[] = [];
    private native = Notifd.get_default();
    private settings = new Gio.Settings({ schema_id: 'io.astal.notifd' });
    private live = new Map<number, LiveNotification>();
    private listeners = new Set<() => void>();
    private signals: number[] = [];
    private disposed = false;
    private depth = 0;
    private dirty = false;
    private path: string;
    private lastSaved = '';

    constructor() {
        const stateHome = GLib.getenv('XDG_STATE_HOME') || GLib.build_filenamev([GLib.get_home_dir(), '.local', 'state']);
        this.path = GLib.getenv('NOTIFICATION_HISTORY') || GLib.build_filenamev([stateHome, 'dashboard', 'notifications.json']);
        const legacy = GLib.getenv('LEGACY_NOTIFICATION_HISTORY') || GLib.build_filenamev([stateHome, 'quickshell', 'notifications.json']);
        let text = readArchive(this.path);
        // The new file, including an empty/corrupt one, is the migration marker.
        if (text === null && legacy !== this.path) text = readArchive(legacy);
        let restoredDnd = false;
        try {
            const restored = restoreArchive(text || '');
            this.history = restored.history;
            restoredDnd = restored.dnd;
        } catch (error) { console.warn(`notifications: could not restore history: ${String(error)}`); }

        // Native settings always win. The archive only seeds DND on first migration.
        if (this.settings.get_user_value('dont-disturb') === null) this.native.dont_disturb = restoredDnd;
        this.native.ignore_timeout = true;
        // Preserve protocol -1 in rows; this frontend owns the 8000 ms default.
        this.native.default_timeout = -1;
        // The pinned schema spells this key "capabilites". Static image hints
        // use icon-static; plaintext bodies do not support body-images/markup.
        this.settings.set_strv('server-capabilites', ['actions', 'body', 'icon-static', 'persistence']);
        this.signals = [
            this.native.connect('notified', (_service: Notifd.Notifd, id: number) => this.transaction(() => this.received(id))),
            this.native.connect('resolved', (_service: Notifd.Notifd, id: number, reason: number) => this.transaction(() => this.resolved(id, reason))),
            this.native.connect('notify::dont-disturb', () => this.transaction(() => {
                if (this.dnd) {
                    const visible = new Set(this.popups.map(row => row.id));
                    this.popups = [];
                    for (const live of this.live.values()) {
                        if (visible.has(live.key)) { live.hovered = false; this.startTimer(live); }
                    }
                    this.prune();
                }
                this.changed();
            })),
        ];

        this.transaction(() => {
            // Astal persists unresolved objects in GSettings. Reattach only rows
            // already admitted by our archive, in archive order, without popups.
            // Absent rows may be a stale native flush after clear/eviction.
            for (const native of [...this.native.notifications]) {
                const row = this.history.find(row => {
                    const match = /^astal:(\d+):(\d+):/.exec(row.id);
                    if (!match || Number(match[1]) !== native.id || Number(match[2]) > native.time) return false;
                    return JSON.stringify(archived(snapshot(native, row.id))) === JSON.stringify(archived(row));
                });
                if (!row) { native.expire(); continue; }
                const live = { native, key: row.id, timer: 0, hovered: false };
                this.live.set(native.id, live);
                this.history = this.history.map(existing => existing.id === row.id ? snapshot(native, row.id) : existing);
                this.startTimer(live);
            }
            this.changed();
        });
        Gio.Settings.sync();
    }

    get dnd(): boolean { return this.native.dont_disturb; }

    subscribe(callback: () => void): () => void {
        if (this.disposed) return () => {};
        this.listeners.add(callback);
        return () => this.listeners.delete(callback);
    }

    private transaction(action: () => void) {
        if (this.disposed) return;
        this.depth++;
        try { action(); }
        finally {
            this.depth--;
            if (!this.depth && this.dirty) {
                this.dirty = false;
                this.persist();
                for (const callback of this.listeners) callback();
            }
        }
    }

    private changed() { this.dirty = true; }

    private persist() {
        const text = serializeArchive(this.history, this.dnd);
        if (text === this.lastSaved) return;
        try {
            const parent = GLib.path_get_dirname(this.path);
            if (GLib.mkdir_with_parents(parent, 0o700) !== 0) throw new Error(`Could not create ${parent}`);
            // PRIVATE + REPLACE_DESTINATION gives an atomic 0600 replacement,
            // including when migrating a file that previously had broader modes.
            Gio.File.new_for_path(this.path).replace_contents(new TextEncoder().encode(text), null, false,
                Gio.FileCreateFlags.PRIVATE | Gio.FileCreateFlags.REPLACE_DESTINATION, null);
            this.lastSaved = text;
        } catch (error) { console.warn(`notifications: could not save history: ${String(error)}`); }
    }

    private received(id: number) {
        const native = this.native.get_notification(id);
        if (!native) return;
        const previous = this.live.get(id);
        if (previous) this.stopTimer(previous);
        const live: LiveNotification = { native,
            key: previous?.key || `astal:${id}:${native.time}:${GLib.uuid_string_random()}`,
            timer: 0, hovered: previous?.hovered || false };
        this.live.set(id, live);
        const row = snapshot(native, live.key);
        this.history = row.transient ? this.history.filter(existing => existing.id !== row.id)
            : newestFirst(this.history, row, HISTORY_LIMIT);
        this.popups = this.dnd ? this.popups.filter(existing => existing.id !== row.id)
            : newestFirst(this.popups, row, POPUP_LIMIT);
        // A hovered popup that was evicted no longer receives a leave event.
        const visible = new Set(this.popups.map(row => row.id));
        for (const tracked of this.live.values()) {
            if (tracked.hovered && !visible.has(tracked.key)) { tracked.hovered = false; this.startTimer(tracked); }
        }
        this.startTimer(live);
        this.prune();
        this.changed();
    }

    private resolved(id: number, reason: number) {
        const live = this.live.get(id);
        if (!live) return;
        this.stopTimer(live);
        this.live.delete(id);
        this.popups = this.popups.filter(row => row.id !== live.key);
        // Pinned Astal: EXPIRED=1 archives; DISMISSED_BY_USER=2,
        // CLOSED=3 (also non-resident actions), UNDEFINED=4 remove history.
        this.history = reason === Notifd.ClosedReason.EXPIRED
            ? this.history.map(row => row.id === live.key ? archived(row) : row)
            : this.history.filter(row => row.id !== live.key);
        this.changed();
    }

    private stopTimer(live: LiveNotification) {
        if (live.timer) GLib.source_remove(live.timer);
        live.timer = 0;
    }

    private startTimer(live: LiveNotification) {
        this.stopTimer(live);
        const timeout = notificationTimeout({ timeout: live.native.expire_timeout, urgency: live.native.urgency });
        if (!timeout || live.hovered) return;
        live.timer = GLib.timeout_add(GLib.PRIORITY_DEFAULT, timeout, () => {
            live.timer = 0;
            // Replacements are new native objects; never expire an old instance.
            if (!this.disposed && this.live.get(live.native.id) === live) live.native.expire();
            return GLib.SOURCE_REMOVE;
        });
    }

    private prune() {
        const retained = new Set([...this.history, ...this.popups].map(row => row.id));
        for (const live of [...this.live.values()]) {
            if (!retained.has(live.key)) live.native.expire();
        }
    }

    setHovered(id: string, hovered: boolean) {
        if (this.disposed) return;
        const live = [...this.live.values()].find(live => live.key === id);
        if (!live || live.hovered === hovered || (hovered && !this.popups.some(row => row.id === id))) return;
        live.hovered = hovered;
        // Leaving starts a full interval, not the remainder of the old timeout.
        this.startTimer(live);
    }

    dismissNotification(id: string) {
        this.transaction(() => {
            const live = [...this.live.values()].find(live => live.key === id);
            live?.native.dismiss();
            this.history = this.history.filter(row => row.id !== id);
            this.popups = this.popups.filter(row => row.id !== id);
            this.changed();
        });
    }

    invokeNotificationAction(id: string, identifier: string) {
        if (this.disposed) return;
        const live = [...this.live.values()].find(live => live.key === id);
        if (live?.native.actions.some((action: Notifd.Action) => action.id === identifier)) live.native.invoke(identifier);
    }

    clearNotifications() {
        this.transaction(() => {
            // Include native rows that have not yet reached our notified callback.
            for (const native of [...this.native.notifications]) native.dismiss();
            this.history = [];
            this.popups = [];
            this.changed();
        });
    }

    toggleDnd() {
        if (this.disposed) return;
        this.native.dont_disturb = !this.native.dont_disturb;
        Gio.Settings.sync();
    }

    dispose() {
        if (this.disposed) return;
        this.persist();
        this.disposed = true;
        this.signals.forEach(signal => this.native.disconnect(signal));
        this.signals = [];
        for (const live of this.live.values()) this.stopTimer(live);
        this.live.clear();
        this.listeners.clear();
    }
}

export type Palette = Record<'background' | 'secondaryBackground' | 'elevated' | 'foreground'
    | 'separator' | 'cyan' | 'pink' | 'dimPink' | 'yellow' | 'red', string>;

export interface NotificationController {
    setHovered(id: string, hovered: boolean): void;
    dismissNotification(id: string): void;
    invokeNotificationAction(id: string, identifier: string): void;
}

function read(path: string | null): string {
    if (!path) throw new Error('Run through dashboard-panel, or set DASHBOARD_PANEL_CONFIG and DASHBOARD_PANEL_PALETTE.');
    const [ok, contents] = GLib.file_get_contents(path);
    if (!ok) throw new Error(`Could not read dashboard theme: ${path}`);
    return new TextDecoder().decode(contents);
}

export function loadTheme(entry: string | null, palettePath: string | null) {
    const palette = JSON.parse(read(palettePath)) as Palette;
    for (const key of ['background', 'secondaryBackground', 'elevated', 'foreground',
        'separator', 'cyan', 'pink', 'dimPink', 'yellow', 'red'] as const) {
        if (!/^#[a-f\d]{6}$/i.test(palette[key])) throw new Error(`Invalid dashboard palette colour: ${key}`);
    }
    if (!entry) throw new Error('DASHBOARD_PANEL_CONFIG must name the dashboard entry file.');
    const definitions = Object.entries(palette)
        .filter(([key, value]) => /^[a-zA-Z]+$/.test(key) && /^#[a-f\d]{6}$/i.test(value))
        .map(([key, value]) => `@define-color dp_${key} ${value};`).join('\n');
    const provider = new Gtk.CssProvider();
    const errors: string[] = [];
    provider.connect('parsing-error', (_provider, _section, error) => errors.push(error.message));
    provider.load_from_string(`${definitions}\n${read(GLib.build_filenamev([GLib.path_get_dirname(entry), 'style.css']))}`);
    if (errors.length) throw new Error(`Dashboard stylesheet: ${errors.join('; ')}`);
    const display = Gdk.Display.get_default();
    if (!display) throw new Error('The dashboard needs a graphical display.');
    // The desktop theme is installed at USER priority. Every selector in this
    // provider is scoped to our windows, so it can safely take precedence.
    Gtk.StyleContext.add_provider_for_display(display, provider, Gtk.STYLE_PROVIDER_PRIORITY_USER + 1);
    return { palette, provider };
}

export function box(vertical = false, spacing = 0, children: Gtk.Widget[] = []): Gtk.Box {
    const widget = new Gtk.Box({ orientation: vertical ? Gtk.Orientation.VERTICAL : Gtk.Orientation.HORIZONTAL, spacing });
    children.forEach(child => widget.append(child));
    return widget;
}

export function label(text = '', classes: string[] = [], expand = false): Gtk.Label {
    return new Gtk.Label({ label: text, css_classes: classes, use_markup: false, xalign: 0, hexpand: expand });
}

export function ellipsis(text = '', classes: string[] = []): Gtk.Label {
    return new Gtk.Label({ label: text, css_classes: classes, use_markup: false, xalign: 0,
        hexpand: true, ellipsize: Pango.EllipsizeMode.END, max_width_chars: 1 });
}

function wrapped(classes: string[], lines: number): Gtk.Label {
    return new Gtk.Label({ css_classes: classes, use_markup: false, xalign: 0, yalign: 0,
        hexpand: true, wrap: true, wrap_mode: Pango.WrapMode.WORD_CHAR,
        ellipsize: Pango.EllipsizeMode.END, lines, max_width_chars: 1 });
}

export function scroll(child: Gtk.Widget, maxHeight: number): Gtk.ScrolledWindow {
    return new Gtk.ScrolledWindow({ child, hscrollbar_policy: Gtk.PolicyType.NEVER,
        vscrollbar_policy: Gtk.PolicyType.AUTOMATIC, min_content_width: 0, min_content_height: 0,
        max_content_height: Math.max(1, maxHeight), propagate_natural_width: false,
        propagate_natural_height: true, hexpand: true, overlay_scrolling: true });
}

export function style(widget: Gtk.Widget, name: string, enabled: boolean): void {
    if (enabled) widget.add_css_class(name);
    else widget.remove_css_class(name);
}

export function invoke(action: () => unknown): void {
    try {
        void Promise.resolve(action()).catch(error => console.warn(`Dashboard action: ${error.message || error}`));
    } catch (error) { console.warn(`Dashboard action: ${error.message || error}`); }
}

export function iconButton(glyph: string, tooltip: string | null, clicked: () => unknown, size = 28): Gtk.Button {
    const child = label(glyph, ['icon-glyph']);
    child.xalign = 0.5;
    const widget = new Gtk.Button({ child, tooltip_text: tooltip, css_classes: ['icon-button'],
        width_request: size, height_request: size, halign: Gtk.Align.CENTER, valign: Gtk.Align.CENTER });
    widget.connect('clicked', () => invoke(clicked));
    return widget;
}

export class Section {
    readonly widget: Gtk.Box;

    constructor(glyph: string, title: string, controls: Gtk.Widget[] = []) {
        const heading = box(false, 8, [label(glyph, ['section-glyph']), label(title, ['section-title'], true), ...controls]);
        this.widget = box(true, 10, [heading, new Gtk.Box({ css_classes: ['section-divider'], height_request: 1 })]);
        this.widget.add_css_class('section');
    }

    append(child: Gtk.Widget): void { this.widget.append(child); }
}

function rgb(colour: string): [number, number, number] {
    return [1, 3, 5].map(offset => parseInt(colour.slice(offset, offset + 2), 16) / 255) as [number, number, number];
}

// Glow has its own allocation, rather than clipping a CSS shadow or rounding
// the six-sided outline into a toolkit rectangle. Width excludes that halo.
export class CutCornerFrame {
    readonly widget: Gtk.Overlay;
    readonly drawing: Gtk.DrawingArea;
    private colour: string;

    constructor(palette: Palette, child: Gtk.Widget, { padding = 0, glow = 0, width = 0 } = {}) {
        this.colour = palette.yellow;
        this.widget = new Gtk.Overlay({ css_classes: ['cut-frame'], overflow: Gtk.Overflow.VISIBLE });
        if (width) this.widget.width_request = width + glow * 2;
        this.drawing = new Gtk.DrawingArea({ can_target: false, hexpand: true, vexpand: true });
        this.drawing.set_draw_func((_area, cr, allocatedWidth, allocatedHeight) => {
            const x = glow + 0.5, y = glow + 0.5;
            const width = allocatedWidth - glow * 2 - 1, height = allocatedHeight - glow * 2 - 1;
            if (width <= 0 || height <= 0) return;
            const cut = Math.min(10, width / 2, height / 2);
            const path = () => {
                cr.moveTo(x + cut, y);
                cr.lineTo(x + width, y);
                cr.lineTo(x + width, y + height - cut);
                cr.lineTo(x + width - cut, y + height);
                cr.lineTo(x, y + height);
                cr.lineTo(x, y + cut);
                cr.closePath();
            };
            const [red, green, blue] = rgb(this.colour);
            if (glow) {
                cr.save();
                cr.translate(0, 3);
                for (let radius = glow; radius > 0; radius--) {
                    path();
                    cr.setSourceRGBA(red, green, blue, 0.025);
                    cr.setLineWidth(radius * 2);
                    cr.stroke();
                }
                cr.restore();
            }
            path();
            // Match the audio panel's neutral, translucent surface.
            const [backgroundRed, backgroundGreen, backgroundBlue] = rgb('#25252F');
            cr.setSourceRGBA(backgroundRed, backgroundGreen, backgroundBlue, 0.9);
            cr.fillPreserve();
            cr.setSourceRGBA(red, green, blue, 1);
            cr.setLineWidth(1);
            cr.stroke();
        });
        this.widget.set_child(this.drawing);
        child.margin_top = child.margin_bottom = child.margin_start = child.margin_end = padding + glow;
        this.widget.add_overlay(child);
        this.widget.set_measure_overlay(child, true);
    }

    setUrgency(urgency: number, palette: Palette): void {
        const colour = urgency === 2 ? palette.red : palette.yellow;
        if (this.colour === colour) return;
        this.colour = colour;
        this.drawing.queue_draw();
    }
}

export class NotificationEntry {
    readonly widget: Gtk.Overlay;
    readonly copy: Gtk.Box;
    readonly artwork: Gtk.Image;
    readonly app: Gtk.Label;
    readonly summary: Gtk.Label;
    readonly body: Gtk.Label;
    readonly actions: Gtk.FlowBox;
    readonly close: Gtk.Button;
    private row: NotificationRow;
    private actionKey = '';
    private iconKey: string | null = null;
    private hovered = false;

    constructor(row: NotificationRow, private readonly center: NotificationController, readonly popup = false) {
        this.row = row;
        this.widget = new Gtk.Overlay({ css_classes: ['notification-entry', ...(popup ? ['popup-entry'] : [])],
            hexpand: true, height_request: 64, overflow: Gtk.Overflow.HIDDEN });
        this.artwork = new Gtk.Image({ pixel_size: 64, width_request: 64, height_request: 64, valign: Gtk.Align.START });
        this.app = ellipsis('', ['notification-app']);
        this.summary = wrapped(['notification-summary'], 2);
        this.body = wrapped(['notification-body'], 3);
        this.actions = new Gtk.FlowBox({ selection_mode: Gtk.SelectionMode.NONE, homogeneous: false,
            column_spacing: 6, row_spacing: 6, min_children_per_line: 1, max_children_per_line: 20,
            activate_on_single_click: false, css_classes: ['notification-actions'], hexpand: true });
        this.copy = box(true, 3, [this.app, this.summary, this.body, this.actions]);
        this.copy.hexpand = true;
        const content = box(false, 8, [this.artwork, this.copy]);
        content.add_css_class('notification-layout');
        this.widget.set_child(content);
        if (!popup) this.widget.add_overlay(new Gtk.Box({ css_classes: ['notification-stripe'], width_request: 2,
            halign: Gtk.Align.START, can_target: false }));
        this.close = iconButton('󰅖', null, () => this.center.dismissNotification(this.row.id), 16);
        this.close.update_property([Gtk.AccessibleProperty.LABEL], ['Dismiss notification']);
        this.close.add_css_class('notification-close');
        this.close.halign = Gtk.Align.END;
        this.close.valign = Gtk.Align.START;
        this.close.margin_top = 4;
        this.close.margin_end = 3;
        this.widget.add_overlay(this.close);
        const click = new Gtk.GestureClick({ button: Gdk.BUTTON_PRIMARY, propagation_phase: Gtk.PropagationPhase.BUBBLE });
        click.connect('released', (_gesture, count, x, y) => {
            if (count !== 1) return;
            // A click on a close/action button must never also invoke default.
            for (let target = this.widget.pick(x, y, Gtk.PickFlags.DEFAULT); target && target !== this.widget; target = target.get_parent()) {
                if (target instanceof Gtk.Button) return;
            }
            this.activateDefault();
        });
        this.widget.add_controller(click);
        const keys = new Gtk.EventControllerKey();
        keys.connect('key-pressed', (_controller, key) => {
            if (!this.widget.has_focus || ![Gdk.KEY_Return, Gdk.KEY_KP_Enter, Gdk.KEY_space].includes(key)) return false;
            this.activateDefault();
            return true;
        });
        this.widget.add_controller(keys);
        const motion = new Gtk.EventControllerMotion();
        motion.connect('enter', () => this.setHovered(true));
        motion.connect('leave', () => this.setHovered(false));
        this.widget.add_controller(motion);
        this.update(row);
    }

    private activateDefault(): void {
        if (this.row.actions.some(action => action.identifier === 'default'))
            invoke(() => this.center.invokeNotificationAction(this.row.id, 'default'));
    }

    private setHovered(hovered: boolean): void {
        if (this.hovered === hovered) return;
        this.hovered = hovered;
        style(this.widget, 'hovered', hovered);
        if (this.popup) invoke(() => this.center.setHovered(this.row.id, hovered));
    }

    update(row: NotificationRow): void {
        this.row = row;
        this.app.label = (row.app || 'SYSTEM').toUpperCase();
        this.summary.label = row.summary || 'Notification';
        this.body.label = row.body || '';
        this.body.visible = Boolean(row.body);
        this.widget.focusable = row.actions.some(action => action.identifier === 'default');
        style(this.widget, 'critical', row.urgency === 2);
        if (this.iconKey !== row.icon) {
            this.iconKey = row.icon;
            this.artwork.clear();
            this.artwork.visible = Boolean(row.icon);
            if (row.icon) {
                try { this.artwork.set_from_gicon(Gio.Icon.new_for_string(row.icon)); }
                catch (error) {
                    this.artwork.visible = false;
                    console.warn(`Dashboard notification icon: ${error.message || error}`);
                }
            }
        }
        // Keep controls (and their focus/pressed state) alive across metrics,
        // timeout and hover updates that do not change notification actions.
        const actions = row.actions.filter(action => action.identifier !== 'default');
        const actionKey = JSON.stringify(actions);
        if (actionKey !== this.actionKey) {
            this.actionKey = actionKey;
            for (let child = this.actions.get_first_child(); child;) {
                const next = child.get_next_sibling();
                this.actions.remove(child);
                child = next;
            }
            for (const action of actions) {
                const text = new Gtk.Label({ label: action.text, use_markup: false, ellipsize: Pango.EllipsizeMode.END,
                    max_width_chars: 24, css_classes: ['notification-action-label'] });
                const button = new Gtk.Button({ child: text, css_classes: ['notification-action'],
                    height_request: 28, tooltip_text: action.text, valign: Gtk.Align.START });
                button.connect('clicked', () => invoke(() => this.center.invokeNotificationAction(this.row.id, action.identifier)));
                this.actions.insert(button, -1);
                button.get_parent()?.set_focusable(false);
            }
        }
        this.actions.visible = actions.length > 0;
    }

    resetHover(): void { this.setHovered(false); }
    dispose(): void { this.resetHover(); }
}

export class NotificationStack {
    readonly widget = box(true, 6);
    readonly rows = new Map<string, { entry: NotificationEntry; widget: Gtk.Widget; frame?: CutCornerFrame }>();

    constructor(private readonly center: NotificationController, private readonly palette: Palette, readonly popup = false) {}

    sync(notifications: readonly NotificationRow[]): void {
        // The center supplies newest-first ordering for both native views.
        const selected = this.popup ? notifications.slice(0, 5) : notifications;
        const wanted = new Set(selected.map(row => row.id));
        for (const [id, row] of this.rows) {
            if (wanted.has(id)) continue;
            this.rows.delete(id);
            this.widget.remove(row.widget);
            row.entry.dispose();
        }
        let previous: Gtk.Widget | null = null;
        for (const notification of selected) {
            let row = this.rows.get(notification.id);
            if (!row) {
                const entry = new NotificationEntry(notification, this.center, this.popup);
                const frame = this.popup ? new CutCornerFrame(this.palette, entry.widget, { padding: 8 }) : undefined;
                row = { entry, frame, widget: frame?.widget || entry.widget };
                this.rows.set(notification.id, row);
                this.widget.append(row.widget);
            }
            row.entry.update(notification);
            row.frame?.setUrgency(notification.urgency, this.palette);
            this.widget.reorder_child_after(row.widget, previous);
            previous = row.widget;
        }
    }

    resetHover(): void { this.rows.forEach(row => row.entry.resetHover()); }

    dispose(): void {
        const rows = [...this.rows.values()];
        this.rows.clear();
        rows.forEach(row => row.entry.dispose());
    }
}

export function connectedMonitors(): Gdk.Monitor[] {
    const model = Gdk.Display.get_default()?.get_monitors();
    if (!model) return [];
    return Array.from({ length: model.get_n_items() }, (_value, index) => model.get_item(index) as Gdk.Monitor);
}

export class NotificationPopups {
    readonly window: Astal.Window;
    readonly stack: NotificationStack;
    readonly viewport: Gtk.ScrolledWindow;
    private monitor: Gdk.Monitor | null = null;

    constructor({ app, backend, palette }: { app: Astal.Application; backend: DashboardBackend; palette: Palette }) {
        this.backend = backend;
        this.window = new Astal.Window({ application: app, name: 'dashboard-notifications', namespace: 'astal:notifications',
            anchor: Astal.WindowAnchor.TOP | Astal.WindowAnchor.RIGHT, exclusivity: Astal.Exclusivity.IGNORE,
            layer: Astal.Layer.OVERLAY, keymode: Astal.Keymode.NONE, visible: false,
            css_classes: ['dashboard-ui', 'notification-popups'] });
        // These are layer-surface margins, not GTK widget padding. Keeping a
        // content-sized surface also leaves the desktop beneath it clickable.
        Gtk4LayerShell.set_margin(this.window, Gtk4LayerShell.Edge.TOP, 46);
        Gtk4LayerShell.set_margin(this.window, Gtk4LayerShell.Edge.RIGHT, 6);
        this.stack = new NotificationStack(backend.notifications, palette, true);
        this.viewport = scroll(this.stack.widget, 800);
        const content = box(true, 0, [this.viewport]);
        content.margin_top = content.margin_bottom = content.margin_start = content.margin_end = 6;
        this.window.set_child(content);
    }

    private readonly backend: DashboardBackend;

    render(monitors = connectedMonitors()): void {
        this.stack.sync(this.backend.notifications.popups);
        const monitor = monitors[0] || null;
        const changedMonitor = this.monitor !== monitor;
        this.monitor = monitor;
        if (changedMonitor) this.stack.resetHover();
        if (!monitor) {
            this.window.visible = false;
            this.stack.resetHover();
            return;
        }
        if (changedMonitor) this.window.gdkmonitor = monitor;
        const { width, height } = monitor.get_geometry();
        const popupWidth = Math.max(1, Math.min(420, width - 12));
        this.viewport.width_request = Math.max(1, popupWidth - 12);
        this.viewport.max_content_height = Math.max(1, height - 72 - 12);
        this.window.set_default_size(popupWidth, -1);
        this.window.visible = this.backend.notifications.popups.length > 0;
        if (!this.window.visible) this.stack.resetHover();
    }

    dispose(): void {
        this.stack.dispose();
        this.window.destroy();
    }
}
