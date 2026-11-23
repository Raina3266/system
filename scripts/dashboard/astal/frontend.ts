import GLib from 'gi://GLib';
import Gio from 'gi://Gio';
import GObject from 'gi://GObject';
import Gtk from 'gi://Gtk?version=4.0';
import Gdk from 'gi://Gdk?version=4.0';
import Pango from 'gi://Pango';
import Astal from 'gi://Astal?version=4.0';
import AstalIO from 'gi://AstalIO';
import Gtk4LayerShell from 'gi://Gtk4LayerShell';
import System from 'system';
import type Cairo from 'cairo';

import { DashboardBackend } from './backend.ts';
import {
    parseRequest, SCREEN_TIMEOUT_CHOICES,
    type Palette, type NotificationRow, type NotificationController,
} from './logic.ts';
import { loadPanelStyle } from '../../live-style.js';

function read(path: string | null): string {
    if (!path) throw new Error('Run through dashboard-panel, or set DASHBOARD_PANEL_CONFIG and DASHBOARD_PANEL_PALETTE.');
    const [ok, contents] = GLib.file_get_contents(path);
    if (!ok) throw new Error(`Could not read dashboard theme: ${path}`);
    return new TextDecoder().decode(contents);
}

function loadTheme(entry: string | null, palettePath: string | null) {
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

function box(vertical = false, spacing = 0, children: Gtk.Widget[] = []): Gtk.Box {
    const widget = new Gtk.Box({ orientation: vertical ? Gtk.Orientation.VERTICAL : Gtk.Orientation.HORIZONTAL, spacing });
    children.forEach(child => widget.append(child));
    return widget;
}

function label(text = '', classes: string[] = [], expand = false): Gtk.Label {
    return new Gtk.Label({ label: text, css_classes: classes, use_markup: false, xalign: 0, hexpand: expand });
}

function ellipsis(text = '', classes: string[] = []): Gtk.Label {
    return new Gtk.Label({ label: text, css_classes: classes, use_markup: false, xalign: 0,
        hexpand: true, ellipsize: Pango.EllipsizeMode.END, max_width_chars: 1 });
}

function wrapped(classes: string[], lines: number): Gtk.Label {
    return new Gtk.Label({ css_classes: classes, use_markup: false, xalign: 0, yalign: 0,
        hexpand: true, wrap: true, wrap_mode: Pango.WrapMode.WORD_CHAR,
        ellipsize: Pango.EllipsizeMode.END, lines, max_width_chars: 1 });
}

function scroll(child: Gtk.Widget, maxHeight: number): Gtk.ScrolledWindow {
    return new Gtk.ScrolledWindow({ child, hscrollbar_policy: Gtk.PolicyType.NEVER,
        vscrollbar_policy: Gtk.PolicyType.AUTOMATIC, min_content_width: 0, min_content_height: 0,
        max_content_height: Math.max(1, maxHeight), propagate_natural_width: false,
        propagate_natural_height: true, hexpand: true, overlay_scrolling: true });
}

function style(widget: Gtk.Widget, name: string, enabled: boolean): void {
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

class Section {
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
class CutCornerFrame {
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
        const wanted = new Set(notifications.map(row => row.id));
        for (const [id, row] of this.rows) {
            if (wanted.has(id)) continue;
            this.rows.delete(id);
            this.widget.remove(row.widget);
            row.entry.dispose();
        }
        let previous: Gtk.Widget | null = null;
        for (const notification of notifications) {
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

function connectedMonitors(): Gdk.Monitor[] {
    const model = Gdk.Display.get_default()?.get_monitors();
    if (!model) return [];
    return Array.from({ length: model.get_n_items() }, (_value, index) => model.get_item(index) as Gdk.Monitor);
}

class NotificationPopups {
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

class MetricBar {
    readonly widget: Gtk.Box;
    readonly value: Gtk.Label;
    readonly bar: Gtk.ProgressBar;
    private target = 0;
    private tick = 0;

    constructor(title: string) {
        this.value = label('—', ['metric-value']);
        this.value.xalign = 1;
        this.bar = new Gtk.ProgressBar({ css_classes: ['metric-bar'], hexpand: true, show_text: false });
        this.widget = box(true, 4, [box(false, 8, [label(title, ['metric-label'], true), this.value]), this.bar]);
    }

    update(progress: number, value: string): void {
        this.value.label = value;
        style(this.bar, 'critical', progress >= 90);
        const next = Number.isFinite(progress) ? Math.max(0, Math.min(100, progress)) / 100 : 0;
        if (next === this.target) return;
        this.target = next;
        if (this.tick) this.bar.remove_tick_callback(this.tick);
        const from = this.bar.fraction;
        const started = GLib.get_monotonic_time();
        this.tick = this.bar.add_tick_callback(() => {
            const t = Math.min(1, (GLib.get_monotonic_time() - started) / 180000);
            this.bar.fraction = from + (next - from) * (1 - (1 - t) ** 3);
            if (t < 1) return GLib.SOURCE_CONTINUE;
            this.tick = 0;
            return GLib.SOURCE_REMOVE;
        });
    }

    dispose(): void {
        if (this.tick) this.bar.remove_tick_callback(this.tick);
        this.tick = 0;
    }
}

export class ButtonSwitcher {
    readonly widget: Gtk.Box;
    readonly previous: Gtk.Button;
    readonly next: Gtk.Button;
    readonly value: Gtk.Label;
    readonly indicators: Gtk.Box;
    private active = '';
    private available: string[] = [];

    constructor(title: string, private readonly choices: readonly { id: string; label: string }[],
        private readonly changed: (id: string) => unknown, palette: Palette) {

        this.value = label('Unavailable', ['switcher-value'], true);
        this.value.xalign = 0.5;
        this.indicators = box(false, 4, choices.map(() => new Gtk.Box({ css_classes: ['switcher-tick'] })));
        this.indicators.halign = Gtk.Align.CENTER;
        const arrow = (text: string, direction: -1 | 1) => {
            const button = new Gtk.Button({ child: label(text, ['switcher-arrow']),
                css_classes: ['switcher-step'], width_request: 24, height_request: 38 });
            (button.get_child() as Gtk.Label).xalign = 0.5;
            button.update_property([Gtk.AccessibleProperty.LABEL],
                [`${direction < 0 ? 'Previous' : 'Next'} ${title.toLowerCase()}`]);
            button.connect('clicked', () => {
                if (!button.sensitive || !this.available.length) return;
                const index = this.available.indexOf(this.active);
                const target = index < 0 ? (direction > 0 ? 0 : this.available.length - 1)
                    : (index + direction + this.available.length) % this.available.length;
                invoke(() => this.changed(this.available[target]));
            });
            return button;
        };
        this.previous = arrow('◁', -1);
        this.next = arrow('▷', 1);
        const control = box(false, 4, [this.previous,
            box(true, 4, [this.value, this.indicators]), this.next]);
        control.valign = Gtk.Align.CENTER;
        const frame = new Gtk.Overlay({ hexpand: true, css_classes: ['button-switcher'] });
        const drawing = new Gtk.DrawingArea({ can_target: false, hexpand: true, vexpand: true });
        const [red, green, blue] = rgb(palette.foreground);

        drawing.set_draw_func((_area: Gtk.DrawingArea, cr: Cairo.Context, width: number, height: number) => {
            const right = width - 0.5, bottom = height - 0.5, cut = Math.min(9, width / 2, height / 2);
            cr.moveTo(0.5, 0.5);
            cr.lineTo(right, 0.5);
            cr.lineTo(right, bottom - cut);
            cr.lineTo(right - cut, bottom);
            cr.lineTo(0.5, bottom);
            cr.closePath();

            cr.setSourceRGBA(red, green, blue, 0.5);
            cr.setLineWidth(0.35);
            cr.stroke();
        });
        frame.set_child(drawing);
        frame.add_overlay(control);
        frame.set_measure_overlay(control, true);
        this.widget = box(false, 0, [frame]);
        this.widget.hexpand = true;
        this.update('');
    }

    update(active: string, available: readonly string[] = this.choices.map(choice => choice.id)): void {
        this.active = active;
        this.available = this.choices.filter(choice => available.includes(choice.id)).map(choice => choice.id);
        this.value.label = this.choices.find(choice => choice.id === active)?.label || 'Unavailable';
        this.value.update_property([Gtk.AccessibleProperty.LABEL], [this.value.label]);
        const canStep = this.available.length > 1 || (this.available.length === 1 && this.available[0] !== active);
        this.previous.sensitive = this.next.sensitive = canStep;
        let tick = this.indicators.get_first_child();
        for (const choice of this.choices) {
            if (!tick) break;
            tick.visible = this.available.includes(choice.id);
            style(tick, 'active', choice.id === active);
            tick = tick.get_next_sibling();
        }
    }
}

const CARD_WIDTH = 440;
const CARD_TOP = 48;
const CARD_RIGHT = 24;
const CARD_PADDING = 16;
const CARD_GLOW = 12;


function monitorFor(name?: string): Gdk.Monitor | null {
    const monitors = connectedMonitors();
    return monitors.find(monitor => monitor.get_connector() === name) || monitors[0] || null;
}

function sameMonitor(left: Gdk.Monitor | null, right: Gdk.Monitor | null): boolean {
    return left === right || Boolean(left && right && left.get_connector()
        && left.get_connector() === right.get_connector());
}

function closeNetwork(): void {
    try {
        AstalIO.Process.exec_asyncv([GLib.getenv('NETWORK_PANEL') || 'network-panel', 'close'], (_process: GObject.Object | null, result: Gio.AsyncResult) => {
            try { AstalIO.Process.exec_asyncv_finish(result); }
            catch (error) { console.warn(`Could not dismiss the network panel: ${String(error)}`); }
        });
    } catch (error) { console.warn(`Could not dismiss the network panel: ${String(error)}`); }
}

export class DashboardPanel {
    readonly window: Astal.Window;
    readonly root: Gtk.Overlay;
    readonly card: Gtk.Overlay;
    readonly viewport: Gtk.ScrolledWindow;
    readonly content: Gtk.Box;
    readonly screenTimeout: ButtonSwitcher;
    readonly profile: ButtonSwitcher;
    readonly metrics: Record<'cpu' | 'memory' | 'disk' | 'temperature', MetricBar>;
    readonly history: NotificationStack;
    readonly archive: Gtk.ScrolledWindow;
    readonly historyCount: Gtk.Label;
    readonly empty: Gtk.Label;
    readonly dnd: Gtk.Button;
    readonly clear: Gtk.Button;

    opened = false;
    monitor: Gdk.Monitor | null = null;
    private animation = 0;
    private offset = -12;

    constructor({ app, backend, palette }: { app: Astal.Application; backend: DashboardBackend; palette: Palette }) {
        this.backend = backend;
        this.window = new Astal.Window({ application: app, name: 'dashboard-panel', namespace: 'astal:dashboard',
            anchor: Astal.WindowAnchor.TOP | Astal.WindowAnchor.BOTTOM | Astal.WindowAnchor.LEFT | Astal.WindowAnchor.RIGHT,
            exclusivity: Astal.Exclusivity.IGNORE, layer: Astal.Layer.OVERLAY,
            keymode: Astal.Keymode.NONE, visible: false, css_classes: ['dashboard-ui', 'dashboard-panel'] });
        this.window.connect('close-request', () => { this.close(); return true; });
        const keys = new Gtk.EventControllerKey({ propagation_phase: Gtk.PropagationPhase.CAPTURE });
        keys.connect('key-pressed', (_controller: Gtk.EventControllerKey, key: number) => {
            if (!this.opened || key !== Gdk.KEY_Escape) return false;
            this.close();
            return true;
        });
        this.window.add_controller(keys);
        this.root = new Gtk.Overlay({ hexpand: true, vexpand: true, focusable: true });
        this.root.set_child(new Gtk.Box({ hexpand: true, vexpand: true }));
        this.window.set_child(this.root);
        this.window.connect('map', () => { if (this.opened) this.root.grab_focus(); });

        const power = new Section('󰚥', 'POWER');

        this.profile = new ButtonSwitcher('Power profile', [
            { id: 'power-saver', label: 'Power saver' }, { id: 'balanced', label: 'Balanced' },
            { id: 'performance', label: 'Performance' },
        ], profile => backend.setProfile(profile), palette);

        this.screenTimeout = new ButtonSwitcher('Sleep timer', SCREEN_TIMEOUT_CHOICES,
            value => backend.setScreenTimeout(Number(value)), palette);
        this.screenTimeout.widget.update_property([Gtk.AccessibleProperty.DESCRIPTION],
            ['Suspend the computer after inactivity. None disables automatic sleep. Locking is unchanged; keyboard/mouse wake depends on hardware.']);
        const powerControls = box(false, 10, [this.profile.widget, this.screenTimeout.widget]);
        powerControls.homogeneous = true;
        power.append(powerControls);

        const system = new Section('󰍛', 'SYSTEM PARAMETERS');
        this.metrics = { cpu: new MetricBar('CPU'), memory: new MetricBar('MEMORY'),
            disk: new MetricBar('ROOT DISK'), temperature: new MetricBar('CPU TEMP') };
        Object.values(this.metrics).forEach(metric => system.append(metric.widget));

        this.historyCount = label('', ['history-count']);
        this.dnd = iconButton('󰂚', 'Enable do not disturb', () => backend.notifications.toggleDnd());
        this.clear = iconButton('󰆴', 'Clear all', () => backend.notifications.clearNotifications());
        const notifications = new Section('󰂚', 'NOTIFICATIONS', [this.historyCount, this.dnd, this.clear]);
        this.empty = label('NO RECENT ALERTS', ['notifications-empty'], true);
        this.empty.xalign = 0.5;
        notifications.append(this.empty);
        this.history = new NotificationStack(backend.notifications, palette);
        this.archive = scroll(this.history.widget, 234);
        this.archive.add_css_class('notification-archive');
        notifications.append(this.archive);

        this.content = box(true, 10, [power.widget, system.widget, notifications.widget]);
        this.viewport = scroll(this.content, 800);
        this.viewport.add_css_class('dashboard-viewport');
        this.card = new CutCornerFrame(palette, this.viewport,
            { padding: CARD_PADDING, glow: CARD_GLOW, width: CARD_WIDTH }).widget;
        this.card.add_css_class('dashboard-card');
        this.card.halign = Gtk.Align.END;
        this.card.valign = Gtk.Align.START;
        this.card.margin_end = CARD_RIGHT - CARD_GLOW;
        this.card.margin_top = CARD_TOP - CARD_GLOW + this.offset;
        this.card.opacity = 0;
        this.root.add_overlay(this.card);
        const outside = new Gtk.GestureClick({ propagation_phase: Gtk.PropagationPhase.CAPTURE, button: 0 });
        outside.connect('released', (_gesture: Gtk.GestureClick, _count: number, x: number, y: number) => {
            if (!this.opened) return;
            const [ok, bounds] = this.card.compute_bounds(this.root);
            if (ok && (x < bounds.get_x() + CARD_GLOW || x > bounds.get_x() + bounds.get_width() - CARD_GLOW
                || y < bounds.get_y() + CARD_GLOW || y > bounds.get_y() + bounds.get_height() - CARD_GLOW)) this.close();
        });
        this.root.add_controller(outside);
    }

    private readonly backend: DashboardBackend;

    get connector(): string { return this.monitor?.get_connector() || ''; }

    render(): void {
        const { metrics, notifications } = this.backend;
        this.screenTimeout.update(String(this.backend.screenTimeoutMinutes));
        this.profile.update(this.backend.activeProfile, this.backend.profiles);
        this.metrics.cpu.update(metrics.cpuPercent, metrics.cpuPercent < 0 ? '—' : `${metrics.cpuPercent}%`);
        this.metrics.memory.update(metrics.memoryPercent, metrics.memoryPercent < 0 ? '—'
            : `${metrics.memoryUsedGiB.toFixed(1)} / ${metrics.memoryTotalGiB.toFixed(1)} GiB`);
        this.metrics.disk.update(metrics.diskPercent, metrics.diskPercent < 0 ? '—'
            : `${metrics.diskUsedGiB.toFixed(1)} / ${metrics.diskTotalGiB.toFixed(1)} GiB`);
        this.metrics.temperature.update(metrics.cpuTemperatureC, metrics.cpuTemperatureC < 0 ? '—'
            : `${Math.round(metrics.cpuTemperatureC)} °C`);
        this.historyCount.label = `HISTORY · ${notifications.history.length}`;
        (this.dnd.get_child() as Gtk.Label).label = notifications.dnd ? '󰂛' : '󰂚';
        this.dnd.update_property([Gtk.AccessibleProperty.LABEL],
            [notifications.dnd ? 'Disable do not disturb' : 'Enable do not disturb']);
        this.clear.sensitive = notifications.history.length > 0;
        this.empty.visible = notifications.history.length === 0;
        this.archive.visible = notifications.history.length > 0;
        this.history.sync(notifications.history);
    }

    private resize(): void {
        if (!this.monitor) return;
        const { width, height } = this.monitor.get_geometry();
        this.card.width_request = Math.min(CARD_WIDTH, Math.max(1, width - CARD_RIGHT * 2)) + CARD_GLOW * 2;
        this.viewport.max_content_height = Math.max(1, height - 64 - CARD_PADDING * 2);
    }

    updateMonitors(monitors: Gdk.Monitor[]): void {
        if (!this.monitor) return;
        const current = monitors.find(monitor => sameMonitor(monitor, this.monitor));
        if (!current) {
            this.monitor = null;
            this.close(false);
            return;
        }
        if (current !== this.monitor) {
            this.monitor = current;
            this.window.gdkmonitor = current;
        }
        this.resize();
    }

    toggle(monitor: Gdk.Monitor): void {
        if (this.opened && sameMonitor(monitor, this.monitor)) { this.close(); return; }
        this.monitor = monitor;
        this.window.gdkmonitor = monitor;
        this.resize();
        this.opened = true;
        this.window.keymode = Astal.Keymode.EXCLUSIVE;
        this.window.visible = true;
        this.root.grab_focus();
        this.backend.setActive(true);
        closeNetwork();
        this.animate(true);
    }

    private stopAnimation(): void {
        if (this.animation) this.root.remove_tick_callback(this.animation);
        this.animation = 0;
    }

    private animate(open: boolean): void {
        this.stopAnimation();
        const started = GLib.get_monotonic_time();
        const opacity = this.card.opacity, offset = this.offset;
        const targetOpacity = open ? 1 : 0, targetOffset = open ? 0 : -12;
        this.animation = this.root.add_tick_callback(() => {
            const elapsed = (GLib.get_monotonic_time() - started) / 1000;
            const fade = 1 - (1 - Math.min(1, elapsed / 180)) ** 3;
            const slide = 1 - (1 - Math.min(1, elapsed / 220)) ** 3;
            this.card.opacity = opacity + (targetOpacity - opacity) * fade;
            this.offset = offset + (targetOffset - offset) * slide;
            this.card.margin_top = Math.round(CARD_TOP - CARD_GLOW + this.offset);
            if (elapsed < 220) return GLib.SOURCE_CONTINUE;
            this.animation = 0;
            if (!this.opened) this.window.visible = false;
            return GLib.SOURCE_REMOVE;
        });
    }

    close(animate = true): void {
        if (!this.opened && animate) return;
        const wasOpen = this.opened;
        this.opened = false;
        this.window.keymode = Astal.Keymode.NONE;
        if (wasOpen) invoke(() => this.backend.setActive(false));
        if (animate && this.window.visible) this.animate(false);
        else {
            this.stopAnimation();
            this.card.opacity = 0;
            this.offset = -12;
            this.card.margin_top = CARD_TOP - CARD_GLOW + this.offset;
            this.window.visible = false;
        }
    }

    dispose(): void {
        this.close(false);
        Object.values(this.metrics).forEach(metric => metric.dispose());
        this.history.dispose();
        this.window.destroy();
    }
}


export const DashboardApplication = GObject.registerClass({ GTypeName: 'DashboardPanelApplication' },
    class DashboardApplication extends Astal.Application {
        backend?: DashboardBackend;
        panel?: DashboardPanel;
        popups?: NotificationPopups;
        private unsubscribe?: () => void;
        private monitorList?: Gio.ListModel;
        private monitorSignal = 0;
        private geometrySignals: [Gdk.Monitor, number][] = [];
        private theme?: ReturnType<typeof loadTheme>;

        handleRequest(request: string): string {
            const command = parseRequest(request);
            if (!this.backend || !this.panel) throw new Error('Dashboard panel is starting.');
            switch (command.command) {
            case 'status': {
                return JSON.stringify({ ...this.backend.status(), opened: this.panel.opened, monitor: this.panel.connector || null });
            }
            case 'toggle': {
                const monitor = monitorFor(command.monitor);
                if (!monitor) throw new Error('No connected monitor is available.');
                this.panel.toggle(monitor);
                break;
            }
            case 'close': this.panel.close(); break;
            case 'toggleDnd': this.backend.notifications.toggleDnd(); break;
            case 'clearNotifications': this.backend.notifications.clearNotifications(); break;
            }
            return 'ok';
        }

        vfunc_request(request: string, connection: Gio.SocketConnection): void {
            let response: string;
            try { response = this.handleRequest(request); }
            catch (error) { response = JSON.stringify({ error: error instanceof Error ? error.message : String(error) }); }
            const close = () => {
                try {
                    connection.close_async(GLib.PRIORITY_DEFAULT, null, (_source: Gio.IOStream | null, result: Gio.AsyncResult) => {
                        try { connection.close_finish(result); }
                        catch (error) { console.warn(`Dashboard request close: ${String(error)}`); }
                    });
                } catch (error) { console.warn(`Dashboard request close: ${String(error)}`); }
            };
            try {
                AstalIO.write_sock(connection, response, (_source: GObject.Object | null, result: Gio.AsyncResult) => {
                    try { AstalIO.write_sock_finish(result); }
                    catch (error) { console.warn(`Dashboard request reply: ${String(error)}`); }
                    finally { close(); }
                });
            } catch (error) {
                close();
                console.warn(`Dashboard request reply: ${String(error)}`);
            }
        }

        private render(): void {
            this.panel?.render();
            this.popups?.render();
        }

        private watchGeometry(): void {
            this.geometrySignals.forEach(([monitor, signal]) => monitor.disconnect(signal));
            this.geometrySignals = connectedMonitors().map((monitor): [Gdk.Monitor, number] => [monitor, monitor.connect('notify::geometry', () => {
                this.panel?.updateMonitors(connectedMonitors());
                this.popups?.render();
            })]);
        }

        startPanel(onFatal: () => void): void {
            if (this.backend) return;
            this.backend = new DashboardBackend(undefined, onFatal);
            if (this.backend.offline) return;
            this.theme = loadTheme(GLib.getenv('DASHBOARD_PANEL_CONFIG'), GLib.getenv('DASHBOARD_PANEL_PALETTE'));
            this.panel = new DashboardPanel({ app: this, backend: this.backend, palette: this.theme.palette });
            this.popups = new NotificationPopups({ app: this, backend: this.backend, palette: this.theme.palette });
            this.unsubscribe = this.backend.subscribe(() => this.render());
            this.monitorList = Gdk.Display.get_default()!.get_monitors();
            this.monitorSignal = this.monitorList.connect('items-changed', () => {
                this.watchGeometry();
                this.panel?.updateMonitors(connectedMonitors());
                this.popups?.render();
            });
            this.watchGeometry();
            this.render();
        }

        disposePanel(): void {
            this.unsubscribe?.();
            this.unsubscribe = undefined;
            if (this.monitorSignal) this.monitorList?.disconnect(this.monitorSignal);
            this.monitorSignal = 0;
            this.monitorList = undefined;
            this.geometrySignals.forEach(([monitor, signal]) => monitor.disconnect(signal));
            this.geometrySignals = [];
            try {
                this.popups?.dispose();
                this.panel?.dispose();
            } finally {
                this.popups = undefined;
                this.panel = undefined;
                this.backend?.dispose();
                this.backend = undefined;
                if (this.theme) {
                    const display = Gdk.Display.get_default();
                    if (display) Gtk.StyleContext.remove_provider_for_display(display, this.theme.provider);
                    this.theme = undefined;
                }
            }
        }
    });

export async function main(): Promise<number> {
    Gtk.init();
    // AGS preloads GTK4 layer-shell. Child processes must not inherit it.
    GLib.unsetenv('LD_PRELOAD');
    const app = new DashboardApplication({ instance_name: 'dashboard-panel' });
    let exitCode = 0;
    app.connect('activate', () => {
        try { app.startPanel(() => {
            console.error('Dashboard helper disconnected; exiting notification owner.');
            exitCode = 1;
            app.quit();
        }); }
        catch (error) {
            console.error(`Dashboard initialization: ${String(error)}`);
            exitCode = 1;
            app.quit();
        }
    });
    app.connect('shutdown', () => app.disposePanel());
    try { app.acquire_socket(); }
    catch (error) {
        console.error(`Dashboard startup: ${String(error)}`);
        return 1;
    }
    app.hold();
    const status = await app.runAsync([]);
    return exitCode || status;
}

export function isMainModule(url: string): boolean {
    // File identity works for both frontend.ts and AGS bundles; GJS has no import.meta.main.
    return Boolean(System.programPath && Gio.File.new_for_uri(url).equal(Gio.File.new_for_path(System.programPath)));
}

// AGS/esbuild merges imported modules into one main file. Bundled importers
// explicitly opt out before evaluation; ordinary GJS imports need no flag.
if (isMainModule(import.meta.url) && GLib.getenv('DASHBOARD_PANEL_IMPORT_ONLY') !== '1') System.exit(await main());
