import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Notifd from 'gi://AstalNotifd';
import Gtk from 'gi://Gtk?version=4.0';
import Gdk from 'gi://Gdk?version=4.0';
import Pango from 'gi://Pango';
import Astal from 'gi://Astal?version=4.0';
import Gtk4LayerShell from 'gi://Gtk4LayerShell';
import type Cairo from 'cairo';
import type { DashboardBackend } from './power.ts';
import { loadPanelStyle } from '../live-style.js';

export type NotificationRow = {
    token?: string;
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

// Bound native strings before encoding the pipe frame. Never truncate an action ID
// into another valid ID. Lengths allow for JSON's worst-case six-byte escaping.
function field(value: string, max: number): string {
    return (value || '').slice(0, max).replace(/[\uD800-\uDBFF]$/, '');
}
function snapshot(native: Notifd.Notification): NotificationRow {
    return {
        id: '', app: field(native.app_name, 512), summary: field(native.summary, 2048), body: field(native.body, 12288),
        icon: field(native.image || native.app_icon, 1024), urgency: native.urgency,
        transient: native.transient, timeout: native.expire_timeout,
        actions: (native.actions || []).slice(0, 16).filter((a: Notifd.Action) => a.id.length <= 128)
            .map((a: Notifd.Action) => ({ identifier: a.id, text: field(a.label, 192) })),
        closed: false,
    };
}

export type Notice = { kind: 'init'; dnd: boolean; user: boolean }
    | { kind: 'received'; native: number; token: string; time: number; restore: boolean; row: NotificationRow }
    | { kind: 'resolved'; token: string; reason: number } | { kind: 'dnd' | 'seedAck'; value: boolean }
    | { kind: 'hover'; id: string; token: string; value: boolean } | { kind: 'action'; id: string; token: string; identifier: string }
    | { kind: 'dismiss'; id: string; token: string } | { kind: 'toggle' }
    | { kind: 'clear'; instances: { native: number; token: string }[] };
export type RequestFrame = { op: 'active'; active: boolean; generation: number } | { op: 'timeout'; minutes: number }
    | { op: 'notify'; command: Notice };
export type EventFrame = { event: 'offline' } | { event: 'toggle' } | { event: 'seed'; value: boolean }
    | { event: 'error'; code: string } | { event: 'timeout'; minutes: number }
    | { event: 'metrics'; generation: number; data: import('./parameters.ts').Metrics }
    | { event: 'notifications'; history: NotificationRow[]; popups: NotificationRow[] }
    | { event: 'native'; operation: 'expire' | 'invoke' | 'dismiss'; native: number; token: string; argument: string };
export interface Transport {
    send(frame: RequestFrame): void;
    whenDrained(callback: () => void): void;
    dispose(): void;
}
// One process per dashboard owner. Never reconnect and replay queued operations.
export class DashboardPipe {
    private child: Gio.Subprocess;
    private io = new Gio.Cancellable();
    private queue: Uint8Array[] = [];
    private writing = false;
    private ended = false;
    private queuedBytes = 0;
    private drained?: () => void;
    whenDrained(callback: () => void) {
        if (this.ended) return;
        this.drained = callback;
        if (!this.writing && !this.queue.length) void Promise.resolve().then(() => this.resume());
    }
    private resume() {
        if (this.ended || this.writing || this.queue.length) return;
        const callback = this.drained;
        this.drained = undefined;
        callback?.();
    }
    constructor(private receive: (event: EventFrame) => void) {
        this.child = Gio.Subprocess.new([GLib.getenv('PANEL_BACKEND') || 'panel-launcher', 'backend', 'dashboard'],
            Gio.SubprocessFlags.STDIN_PIPE | Gio.SubprocessFlags.STDOUT_PIPE);
        this.child.wait_async(null, (_, result) => {
            try { this.child.wait_finish(result); } catch { /* Never log raw child data. */ } finally { this.dispose(); }
        });
        const input = this.child.get_stdout_pipe()!;
        let parts: Uint8Array[] = [], size = 0;
        const read = () => input.read_bytes_async(65536, GLib.PRIORITY_DEFAULT, this.io, (_, result) => {
            try {
                const bytes = input.read_bytes_finish(result).get_data();
                if (this.ended) { input.close(null); return; }
                if (!bytes?.length) { this.dispose(); return; }
                let pos = 0;
                while (pos < bytes.length) {
                    const newline = bytes.indexOf(10, pos);
                    const end = newline < 0 ? bytes.length : newline;
                    const part = bytes.subarray(pos, end); size += part.length;
                    if (size > 32 * 1024 * 1024) { this.dispose(); return; }
                    parts.push(part); pos = end + 1;
                    if (newline < 0) break;
                    const line = new Uint8Array(size); let offset = 0;
                    for (const p of parts) { line.set(p, offset); offset += p.length; }
                    parts = []; size = 0;
                    this.receive(JSON.parse(new TextDecoder().decode(line)));
                    if (this.ended) return;
                }
                read();
            } catch { this.dispose(); try { input.close(null); } catch {} }
        });
        try { read(); } catch { this.dispose(); }
    }
    send(message: RequestFrame) {
        if (this.ended) return;
        const bytes = new TextEncoder().encode(JSON.stringify(message) + '\n');
        if (bytes.length > 131072) { console.warn('Dashboard request exceeds frame limit'); return; }
        if (this.queue.length >= 512 || this.queuedBytes + bytes.length > 8 * 1024 * 1024) { this.dispose(); return; }
        this.queuedBytes += bytes.length; this.queue.push(bytes); this.flush();
    }
    private flush() {
        if (this.ended || this.writing || !this.queue.length) return;
        this.writing = true;
        const pipe = this.child.get_stdin_pipe()!;
        const frame = this.queue.shift()!;
        try {
            pipe.write_all_async(frame, GLib.PRIORITY_DEFAULT, this.io, (_, result) => {
                this.writing = false;
                try {
                    const [ok, n] = pipe.write_all_finish(result);
                    if (!ok || n !== frame.length) this.dispose();
                } catch { this.dispose(); }
                this.queuedBytes -= frame.length;
                if (this.ended) { try { pipe.close(null); } catch {} } else { this.flush(); this.resume(); }
            });
        } catch {
            this.writing = false;
            this.dispose();
        }
    }
    dispose() {
        if (this.ended) return;
        this.ended = true; this.drained = undefined; this.queue = []; this.io.cancel();
        if (!this.writing) { try { this.child.get_stdin_pipe()?.close(null); } catch {} }
        try { this.child.get_stdout_pipe()?.close(null); } catch {}
        this.receive({ event: 'offline' });
    }
}

export class NotificationCenter {
    history: NotificationRow[] = [];
    popups: NotificationRow[] = [];
    private native = Notifd.get_default();
    private settings = new Gio.Settings({ schema_id: 'io.astal.notifd' });
    private live = new Map<number, { native: Notifd.Notification; token: string }>();
    private listeners = new Set<() => void>();
    private signals: number[] = [];
    private disposed = false;
    private offline = false;
    private pending = new Map<number, Notifd.Notification>();
    private pipe?: Transport;
    start(pipe: Transport) {
        if (this.pipe || this.disposed || this.offline) return;
        this.pipe = pipe;
        this.native.ignore_timeout = true;
        this.native.default_timeout = -1;
        this.settings.set_strv('server-capabilites', ['actions', 'body', 'icon-static', 'persistence']);
        this.command({ kind: 'init', dnd: this.dnd, user: this.settings.get_user_value('dont-disturb') !== null });
        if (this.offline) return;
        for (const object of this.native.notifications) this.pending.set(object.id, object);
        this.signals = [this.native.connect('notified', (_, id: number) => {
            this.restoreOne(id);
            const object = this.native.get_notification(id);
            if (object) this.attach(object, false);
        }), this.native.connect('resolved', (_, id: number, reason: number) => {
            this.restoreOne(id);
            const live = this.live.get(id);
            if (live) { this.live.delete(id); this.command({ kind: 'resolved', token: live.token, reason }); }
        }), this.native.connect('notify::dont-disturb', () => this.command({ kind: 'dnd', value: this.dnd }))];
        this.scheduleRestore();
        Gio.Settings.sync();
    }
    private restoreOne(id: number) {
        // A signal may overtake the bulk restore. Preserve per-ID ordering without
        // reattaching the old instance a second time after its replacement.
        const object = this.pending.get(id);
        if (!object) return;
        this.pending.delete(id);
        this.attach(object, true);
    }
    private scheduleRestore() {
        if (!this.pending.size || this.offline || this.disposed) return;
        this.pipe!.whenDrained(() => {
            const item = this.pending.keys().next();
            if (item.done) return;
            this.restoreOne(item.value);
            this.scheduleRestore();
        });
    }
    private attach(native: Notifd.Notification, restore: boolean) {
        if (this.offline || this.disposed) return;
        const token = GLib.uuid_string_random();
        this.live.set(native.id, { native, token });
        this.command({ kind: 'received', native: native.id, time: native.time, token, row: snapshot(native), restore });
    }
    private command(command: Notice) { if (!this.disposed && !this.offline) this.pipe?.send({ op: 'notify', command }); }
    receive(e: EventFrame) {
        if (this.disposed) return;
        if (e.event === 'notifications' || e.event === 'offline') {
            this.history = e.event === 'notifications' ? e.history : []; this.popups = e.event === 'notifications' ? e.popups : [];
            if (e.event === 'offline') { this.offline = true; this.live.clear(); this.disconnect(); }
            for (const cb of this.listeners) cb();
        } else if (e.event === 'native') {
            const live = this.live.get(e.native);
            if (!live || live.token !== e.token || this.native.get_notification(e.native) !== live.native) return;
            try {
                if (e.operation === 'expire') live.native.expire();
                if (e.operation === 'dismiss') live.native.dismiss();
                if (e.operation === 'invoke') live.native.invoke(e.argument);
            } catch { console.warn('Dashboard notification operation failed'); }
        } else if (e.event === 'toggle' || e.event === 'seed') {
            if (e.event === 'toggle' || this.settings.get_user_value('dont-disturb') === null) {
                this.native.dont_disturb = e.event === 'toggle' ? !this.dnd : e.value;
                Gio.Settings.sync();
            }
            // A failed settings write is not an acknowledgment. Leave the seed
            // pending unless readback matches or an explicit native value wins.
            if (e.event !== 'seed' || this.dnd === e.value || this.settings.get_user_value('dont-disturb') !== null)
                this.command({ kind: e.event === 'seed' ? 'seedAck' : 'dnd', value: this.dnd });
        }
    }
    get dnd() { return this.native.dont_disturb; }
    subscribe(cb: () => void) { this.listeners.add(cb); return () => this.listeners.delete(cb); }
    setHovered(id: string, value: boolean, token = '') { this.command({ kind: 'hover', id, token, value }); }
    dismissNotification(id: string, token = '') { this.command({ kind: 'dismiss', id, token }); }
    invokeNotificationAction(id: string, identifier: string, token = '') { this.command({ kind: 'action', id, token, identifier }); }
    clearNotifications() {
        if (this.disposed || this.offline) return;
        this.pending.clear(); // Clear includes the full current native collection below.
        let instances: { native: number; token: string }[] = [];
        for (const native of this.native.notifications) {
            let ref = this.live.get(native.id);
            if (ref?.native !== native) {
                ref = { native, token: GLib.uuid_string_random() }; this.live.set(native.id, ref);
            }
            instances.push({ native: native.id, token: ref.token });
            if (instances.length === 256) { this.command({ kind: 'clear', instances }); instances = []; }
        }
        this.command({ kind: 'clear', instances });
    }
    toggleDnd() { this.command({ kind: 'toggle' }); }
    private disconnect() { this.pending.clear(); this.signals.splice(0).forEach(id => this.native.disconnect(id)); }
    dispose() {
        if (this.disposed) return;
        this.disposed = true;
        this.disconnect(); this.live.clear(); this.listeners.clear();
    }
}

export type Palette = Record<'background' | 'secondaryBackground' | 'elevated' | 'foreground'
    | 'separator' | 'cyan' | 'pink' | 'dimPink' | 'yellow' | 'red', string>;

export interface NotificationController {
    setHovered(id: string, hovered: boolean, token?: string): void;
    dismissNotification(id: string, token?: string): void;
    invokeNotificationAction(id: string, identifier: string, token?: string): void;
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
    const definitions = '.dashboard-ui {\n' + Object.entries(palette)
        .filter(([key, value]) => /^[a-zA-Z]+$/.test(key) && /^#[a-f\d]{6}$/i.test(value))
        .map(([key, value]) => `    --dp_${key}: ${value};`).join('\n') + '\n}';
    const provider = loadPanelStyle({ entry, definitions, label: 'Dashboard' });
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
        void Promise.resolve(action()).catch((error: unknown) => console.warn(`Dashboard action: ${String(error)}`));
    } catch (error) { console.warn(`Dashboard action: ${String(error)}`); }
}

export function iconButton(glyph: string, accessibleLabel: string | null, clicked: () => unknown, size = 28): Gtk.Button {
    const child = label(glyph, ['icon-glyph']);
    child.xalign = 0.5;
    const widget = new Gtk.Button({ child, css_classes: ['icon-button'],
        width_request: size, height_request: size, halign: Gtk.Align.CENTER, valign: Gtk.Align.CENTER });
    if (accessibleLabel !== null) widget.update_property([Gtk.AccessibleProperty.LABEL], [accessibleLabel]);
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
        this.drawing.set_draw_func((_area: Gtk.DrawingArea, cr: Cairo.Context, allocatedWidth: number, allocatedHeight: number) => {
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
        const artworkSize = popup ? 66 : 64;
        this.artwork = new Gtk.Image({ pixel_size: artworkSize, width_request: artworkSize,
            height_request: artworkSize, valign: Gtk.Align.START });
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
        this.close = iconButton('󰅖', null, () => this.center.dismissNotification(this.row.id, this.row.token), 16);
        this.close.update_property([Gtk.AccessibleProperty.LABEL], ['Dismiss notification']);
        this.close.add_css_class('notification-close');
        this.close.halign = Gtk.Align.END;
        this.close.valign = Gtk.Align.START;
        this.close.margin_top = 4;
        this.close.margin_end = 3;
        this.widget.add_overlay(this.close);
        const click = new Gtk.GestureClick({ button: Gdk.BUTTON_PRIMARY, propagation_phase: Gtk.PropagationPhase.BUBBLE });
        click.connect('released', (_gesture: Gtk.GestureClick, count: number, x: number, y: number) => {
            if (count !== 1) return;
            // A click on a close/action button must never also invoke default.
            for (let target = this.widget.pick(x, y, Gtk.PickFlags.DEFAULT); target && target !== this.widget; target = target.get_parent()) {
                if (target instanceof Gtk.Button) return;
            }
            this.activateDefault();
        });
        this.widget.add_controller(click);
        const keys = new Gtk.EventControllerKey();
        keys.connect('key-pressed', (_controller: Gtk.EventControllerKey, key: number) => {
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
            invoke(() => this.center.invokeNotificationAction(this.row.id, 'default', this.row.token));
    }

    private setHovered(hovered: boolean): void {
        if (this.hovered === hovered) return;
        this.hovered = hovered;
        style(this.widget, 'hovered', hovered);
        if (this.popup) invoke(() => this.center.setHovered(this.row.id, hovered, this.row.token));
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
                    console.warn('Invalid dashboard notification icon');
                }
            }
        }
        // Keep controls (and their focus/pressed state) alive across metrics,
        // timeout and hover updates that do not change notification actions.
        const actions = row.actions.filter(action => action.identifier !== 'default');
        const actionKey = JSON.stringify([row.token, actions]);
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
                    height_request: 28, valign: Gtk.Align.START });
                button.update_property([Gtk.AccessibleProperty.LABEL], [action.text]);
                button.connect('clicked', () => invoke(() => this.center.invokeNotificationAction(row.id, action.identifier, row.token)));
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
    readonly rows = new Map<string, {
        entry: NotificationEntry;
        widget: Gtk.Widget;
        notification: NotificationRow;
        frame?: CutCornerFrame;
    }>();

    constructor(private readonly center: NotificationController, private readonly palette: Palette, readonly popup = false) {}

    sync(notifications: readonly NotificationRow[]): void {
        // The center supplies newest-first ordering for both native views.
        const selected = notifications;
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
                row = { entry, frame, notification, widget: frame?.widget || entry.widget };
                frame?.setUrgency(notification.urgency, this.palette);
                this.rows.set(notification.id, row);
                this.widget.append(row.widget);
            } else if (row.notification !== notification) {
                row.notification = notification;
                row.entry.update(notification);
                row.frame?.setUrgency(notification.urgency, this.palette);
            }
            if (row.widget.get_prev_sibling() !== previous)
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
