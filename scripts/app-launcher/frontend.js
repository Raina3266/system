import GLib from 'gi://GLib';
import Gio from 'gi://Gio';
import GObject from 'gi://GObject';
import Gtk from 'gi://Gtk?version=4.0';
import Gdk from 'gi://Gdk?version=4.0';
import Graphene from 'gi://Graphene';
import Pango from 'gi://Pango';
import Astal from 'gi://Astal?version=4.0';
import System from 'system';
import { LauncherBackend, monitors, focusedMonitor } from './backend.js';
import { loadPanelStyle } from '../shared/astal/live-style.js';
import { replyAndClose } from '../shared/astal/socket.js';
import { drawCutCornerFrame } from '../shared/astal/drawing.js';

const ANIMATION_MS = 260;
const BAR_GAP = 48;
const PANEL_LEFT = 12;
const TILE_WIDTH = 66;
const TILE_HEIGHT = 90;
const TILE_GAP = 10;
const GRID_COLUMNS = 4;
const GRID_ROWS = 2;
const PANEL_PADDING = 12;
const SEARCH_HEIGHT = 34;
const SEARCH_GAP = 10;

function parseRequest(text) {
    const request = JSON.parse(text);
    if (!request || typeof request !== 'object' || Array.isArray(request)
        || !['toggle', 'close', 'status'].includes(request.command)
        || (request.monitor !== undefined && (typeof request.monitor !== 'string' || !request.monitor.trim()))) {
        throw new Error('Expected toggle [monitor], close, or status.');
    }
    return request;
}

function panelLayout(width, height) {
    const available = Math.max(1, width - PANEL_LEFT * 2);
    const columns = Math.max(1, Math.min(GRID_COLUMNS,
        Math.floor((available - PANEL_PADDING * 2 + TILE_GAP) / (TILE_WIDTH + TILE_GAP))));
    return {
        columns,
        width: Math.min(available, columns * TILE_WIDTH + (columns - 1) * TILE_GAP + PANEL_PADDING * 2),
        gridHeight: Math.max(1, Math.min(GRID_ROWS * TILE_HEIGHT + (GRID_ROWS - 1) * TILE_GAP,
            height - BAR_GAP - PANEL_PADDING * 2 - SEARCH_HEIGHT - SEARCH_GAP)),
    };
}

function box(vertical, spacing, children = []) {
    const widget = new Gtk.Box({ orientation: vertical ? Gtk.Orientation.VERTICAL : Gtk.Orientation.HORIZONTAL, spacing });
    children.forEach(child => widget.append(child));
    return widget;
}

function label(text, css, props = {}) {
    return new Gtk.Label({ label: text, css_classes: [css], ...props });
}

// GTK draws the geometry directly: no CSS clip-path support or external artwork required.
function frame(draw) {
    const area = new Gtk.DrawingArea({ hexpand: true, vexpand: true, can_target: false });
    area.set_draw_func((_area, cr, width, height) => draw(cr, width, height));
    return area;
}

function drawPanelFrame(cr, width, height) {
    drawCutCornerFrame(cr, { width: width - 1, height: height - 1, color: '#fcee0a', cut: 10 });
}

function drawTileFrame(cr, width, height) {
    const inset = 4, cut = 5;
    const x = inset, y = inset, right = width - inset, bottom = height - inset;
    cr.moveTo(x, y);
    cr.lineTo(right - cut, y);
    cr.lineTo(right, y + cut);
    cr.lineTo(right, bottom);
    cr.lineTo(x + cut, bottom);
    cr.lineTo(x, bottom - cut);
    cr.closePath();
    cr.setSourceRGBA(0.015, 0.045, 0.05, 0.94);
    cr.fillPreserve();
    cr.setSourceRGBA(0.20, 0.87, 0.92, 0.12);
    cr.setLineWidth(4);
    cr.strokePreserve();
    cr.setSourceRGBA(0.24, 0.85, 0.91, 0.85);
    cr.setLineWidth(1);
    cr.stroke();
    cr.setSourceRGBA(0.20, 0.85, 0.90, 0.055);
    for (let row = y + 4; row < bottom - 3; row += 4) {
        cr.moveTo(x + 3, row);
        cr.lineTo(right - 3, row);
    }
    cr.setLineWidth(1);
    cr.stroke();
    cr.setSourceRGBA(0.42, 0.97, 1, 0.95);
    cr.moveTo(x, y + 24);
    cr.lineTo(x, y);
    cr.lineTo(x + 29, y);
    cr.moveTo(right - 33, bottom);
    cr.lineTo(right, bottom);
    cr.lineTo(right, bottom - 20);
    cr.stroke();
    cr.setSourceRGBA(0.27, 0.88, 0.93, 0.55);
    cr.rectangle(width * 0.46, 2, width * 0.43, 1);
    cr.rectangle(width * 0.56, height - 3, width * 0.32, 2);
    for (let i = 0; i < 6; i++) cr.rectangle(width - 14 - i * 4, height - 8, 2, 2);
    cr.fill();
}

function tile(application) {
    const iconFrame = new Gtk.Overlay({ width_request: 56, height_request: 56, halign: Gtk.Align.CENTER });
    iconFrame.set_child(frame(drawTileFrame));
    const image = new Gtk.Image({ pixel_size: 34, halign: Gtk.Align.CENTER, valign: Gtk.Align.CENTER });
    image.set_from_gicon(application.app.get_icon() || Gio.ThemedIcon.new('application-x-executable'));
    iconFrame.add_overlay(image);
    const caption = label(application.name.toUpperCase(), 'app-name', {
        wrap: true, wrap_mode: Pango.WrapMode.WORD_CHAR, lines: 2,
        ellipsize: Pango.EllipsizeMode.END, justify: Gtk.Justification.CENTER,
        max_width_chars: 9, height_request: 24,
    });
    const content = box(true, 4, [iconFrame, caption]);
    content.valign = Gtk.Align.CENTER;
    const child = new Gtk.FlowBoxChild({ child: content, width_request: TILE_WIDTH, height_request: TILE_HEIGHT,
        css_classes: ['app-tile'] });
    child.application = application;
    return child;
}

export class LauncherPanel {
    constructor(app) {
        this.backend = new LauncherBackend(() => { if (this.opened) this.render(); });
        this.opened = false;
        this.monitor = null;
        this.columns = GRID_COLUMNS;
        this.results = [];
        this.tiles = [];
        this.tileCache = new Map();
        this.resultOrder = new Map();
        this.catalog = null;
        this.window = new Astal.Window({ application: app, name: 'app-launcher', namespace: 'astal:app-launcher',
            anchor: Astal.WindowAnchor.TOP | Astal.WindowAnchor.BOTTOM | Astal.WindowAnchor.LEFT | Astal.WindowAnchor.RIGHT,
            exclusivity: Astal.Exclusivity.IGNORE, layer: Astal.Layer.OVERLAY,
            keymode: Astal.Keymode.NONE, visible: false, css_classes: ['app-launcher'] });
        this.window.connect('close-request', () => { this.close(); return true; });
        this.root = new Gtk.Overlay({ hexpand: true, vexpand: true, focusable: true });
        this.root.set_child(new Gtk.Box({ hexpand: true, vexpand: true }));
        this.window.set_child(this.root);
        this.revealer = new Gtk.Revealer({ transition_type: Gtk.RevealerTransitionType.SLIDE_UP,
            transition_duration: ANIMATION_MS, halign: Gtk.Align.START, valign: Gtk.Align.END,
            margin_start: PANEL_LEFT, margin_bottom: BAR_GAP });
        this.revealer.connect('notify::child-revealed', () => {
            if (!this.opened && !this.revealer.child_revealed) this.window.visible = false;
        });
        this.card = new Gtk.Overlay({ css_classes: ['launcher-card'] });
        this.card.set_child(frame(drawPanelFrame));
        this.search = new Gtk.SearchEntry({ placeholder_text: 'Search applications',
            height_request: SEARCH_HEIGHT, hexpand: true, search_delay: 80,
            css_classes: ['launcher-search'] });
        // Gtk.SearchEntry's built-in clear icon has its own hover tooltip.
        const clear = this.search.get_last_child();
        if (clear) {
            clear.set_tooltip_text(null);
            clear.update_property([Gtk.AccessibleProperty.LABEL], ['Clear search']);
        }
        this.search.connect('search-changed', () => this.render());
        this.search.connect('activate', () => this.launch(this.selected));
        this.grid = new Gtk.FlowBox({ column_spacing: TILE_GAP, row_spacing: TILE_GAP,
            homogeneous: true, min_children_per_line: 1, max_children_per_line: GRID_COLUMNS,
            selection_mode: Gtk.SelectionMode.SINGLE, activate_on_single_click: true,
            halign: Gtk.Align.CENTER, valign: Gtk.Align.START, css_classes: ['launcher-grid'] });
        this.grid.set_filter_func(child => this.resultOrder.has(child.application));
        // GTK expects a gint comparator; nonmatches must have a finite fallback rank.
        this.grid.set_sort_func((left, right) =>
            (this.resultOrder.get(left.application) ?? this.results.length)
                        - (this.resultOrder.get(right.application) ?? this.results.length));
        this.grid.connect('child-activated', (_grid, child) => this.launch(this.resultOrder.get(child.application)));
        this.empty = label('NO APPLICATIONS', 'launcher-empty', { vexpand: true });
        this.gridStack = new Gtk.Stack();
        this.gridStack.add_named(this.grid, 'apps');
        this.gridStack.add_named(this.empty, 'empty');
        this.scroll = new Gtk.ScrolledWindow({ hscrollbar_policy: Gtk.PolicyType.NEVER,
            vscrollbar_policy: Gtk.PolicyType.AUTOMATIC, child: this.gridStack,
            css_classes: ['launcher-scroll'], propagate_natural_height: false });
        this.grid.set_vadjustment(this.scroll.get_vadjustment());
        this.content = box(true, SEARCH_GAP, [this.search, this.scroll]);
        this.content.add_css_class('launcher-content');
        this.card.add_overlay(this.content);
        // The overlay's contents determine the panel's size, not its decorative drawing area.
        this.card.set_measure_overlay(this.content, true);
        this.revealer.set_child(this.card);
        this.root.add_overlay(this.revealer);
        const outside = new Gtk.GestureClick({ propagation_phase: Gtk.PropagationPhase.CAPTURE, button: 0 });
        outside.connect('released', (_gesture, _count, x, y) => {
            if (!this.opened) return;
            const [valid, bounds] = this.card.compute_bounds(this.root);
            if (valid && !bounds.contains_point(new Graphene.Point({ x, y }))) this.close();
        });
        this.root.add_controller(outside);
        const keys = new Gtk.EventControllerKey({ propagation_phase: Gtk.PropagationPhase.CAPTURE });
        keys.connect('key-pressed', (_controller, key, _code, state) => this.keyPressed(key, state));
        this.window.add_controller(keys);
        this.window.connect('map', () => { if (this.opened) this.search.grab_focus(); });

        this.monitorList = Gdk.Display.get_default().get_monitors();
        this.monitorSignal = this.monitorList.connect('items-changed', () => this.updateMonitors());
        this.geometrySignal = 0;
    }

    resize() {
        if (!this.monitor) return;
        const { width, height } = this.monitor.get_geometry();
        const layout = panelLayout(width, height);
        this.columns = layout.columns;
        this.grid.max_children_per_line = layout.columns;
        this.card.width_request = layout.width;
        this.scroll.height_request = layout.gridHeight;
    }

    updateMonitors() {
        if (!this.monitor) return;
        if (!monitors().includes(this.monitor)) { this.close(false); this.setMonitor(null); return; }
        this.resize();
        if (this.opened) this.render();
    }

    setMonitor(monitor) {
        if (this.geometrySignal) this.monitor.disconnect(this.geometrySignal);
        this.geometrySignal = 0;
        this.monitor = monitor;
        if (monitor) {
            this.window.gdkmonitor = monitor;
            this.geometrySignal = monitor.connect('notify::geometry', () => this.updateMonitors());
        }
    }

    toggle(monitor) {
        if (this.opened && this.monitor?.get_connector() === monitor.get_connector()) { this.close(); return; }
        this.setMonitor(monitor);
        this.resize();
        this.search.set_text('');
        this.opened = true;
        this.render();
        this.select(-1);
        this.window.keymode = Astal.Keymode.EXCLUSIVE;
        this.window.visible = true;
        this.revealer.reveal_child = true;
        this.search.grab_focus();
    }

    close(animate = true) {
        this.opened = false;
        this.window.keymode = Astal.Keymode.NONE;
        this.revealer.reveal_child = false;
        if (!animate || !this.revealer.child_revealed) this.window.visible = false;
    }

    get selected() {
        return this.resultOrder.get(this.grid.get_selected_children()[0]?.application) ?? -1;
    }

    render() {
        const focused = this.window.get_focus();
        const gridFocused = focused === this.grid || Boolean(focused?.is_ancestor(this.grid));
        const query = this.search.get_text().trim();
        this.results = this.backend.query(query);
        this.resultOrder = new Map(this.results.map((application, index) => [application, index]));
        const catalog = this.backend.applications;
        if (catalog !== this.catalog) {
            // Only a native catalog reload replaces widgets; searches filter and reorder them in place.
            this.grid.remove_all();
            this.tileCache.clear();
            for (const application of catalog) {
                const child = tile(application);
                this.tileCache.set(application, child);
                this.grid.append(child);
            }
            this.catalog = catalog;
        }
        this.grid.unselect_all();
        this.grid.invalidate_sort();
        this.grid.invalidate_filter();
        this.tiles = this.results.map(application => this.tileCache.get(application));
        this.tiles.forEach(child => child.remove_css_class('launch-error'));
        this.gridStack.visible_child_name = this.results.length ? 'apps' : 'empty';
        this.empty.label = query ? 'NO MATCHING APPLICATIONS' : 'NO APPLICATIONS';
        const selectFirst = this.results.length && (query || gridFocused);
        this.select(selectFirst ? 0 : -1, this.opened && gridFocused);
        this.scroll.get_vadjustment().set_value(0);
    }

    select(index, focus = false) {
        const child = this.tiles[index];
        if (child) this.grid.select_child(child);
        else this.grid.unselect_all();
        if (focus) {
            // FlowBox's focus vfunc updates its navigation cursor as well as keyboard focus.
            if (child) child.child_focus(Gtk.DirectionType.TAB_FORWARD);
            else this.root.grab_focus();
        }
    }

    keyPressed(key, state = 0) {
        if (!this.opened) return false;
        if (key === Gdk.KEY_Escape) { this.close(); return true; }

        const focused = this.window.get_focus();
        const searchFocused = focused === this.search || Boolean(focused?.is_ancestor(this.search));
        const name = Gdk.keyval_name(key);
        if (name === 'f' && (state & Gdk.ModifierType.CONTROL_MASK)) { this.search.grab_focus(); return true; }
        if (searchFocused) {
            if (name === 'Down' && this.results.length) { this.select(0, true); return true; }
            return false;
        }
        const index = this.resultOrder.get(focused?.application) ?? -1;
        if (name === 'Up' && index >= 0 && index < this.columns) {
            this.search.grab_focus();
            return true;
        }
        // GTK stays in the same column at a missing cell; the launcher clamps to the last result.
        if (name === 'Down' && index >= 0 && index + this.columns >= this.results.length) {
            this.select(this.results.length - 1, true);
            return true;
        }
        return false;
    }

    launch(index) {
        const application = this.results[index];
        if (!application) return;
        if (this.backend.launch(application)) this.close();
        else {
            this.tiles[index]?.add_css_class('launch-error');
            console.error(`Could not launch ${application.entry}`);
        }
    }

    dispose() {
        this.close(false);
        this.backend.dispose();
        this.monitorList.disconnect(this.monitorSignal);
        this.setMonitor(null);
        this.window.destroy();
    }
}

export const LauncherApplication = GObject.registerClass({ GTypeName: 'CyberpunkAppLauncher' },
    class LauncherApplication extends Astal.Application {
        async handleRequest(text) {
            const request = parseRequest(text);
            if (!this.panel) throw new Error('Application launcher is starting.');
            if (request.command === 'status') return JSON.stringify({ opened: this.panel.opened,
                monitor: this.panel.monitor?.get_connector() || null, applications: this.panel.backend.applications.length });
            if (request.command === 'close') this.panel.close();
            else this.panel.toggle(await focusedMonitor(request.monitor));
            return 'ok';
        }

        vfunc_request(text, connection) {
            // Serialize monitor lookups so rapid toggles/close requests retain their arrival order.
            this.requests = (this.requests || Promise.resolve()).then(async () => {
                let response;
                try { response = await this.handleRequest(text); }
                catch (error) { response = JSON.stringify({ error: error.message }); }
                replyAndClose(connection, response, {
                    label: 'Launcher', asyncClose: true,
                    onError: (phase, error) => console.warn(`Launcher ${phase === 'reply' ? 'reply' : 'connection'}: ${error.message}`),
                });
            }).catch(error => console.error(`Launcher request: ${error.message}`));
        }
    });

async function main() {
    Gtk.init();
    GLib.unsetenv('LD_PRELOAD');
    const app = new LauncherApplication({ instance_name: 'app-launcher' });
    let exitCode = 0;
    let provider;
    app.connect('activate', () => {
        if (app.panel) return;
        try {
            const config = GLib.getenv('APP_LAUNCHER_CONFIG') || System.programPath;
            provider = loadPanelStyle({ entry: config, label: 'Launcher' });
            app.panel = new LauncherPanel(app);
        } catch (error) {
            console.error(`Launcher initialization: ${error.stack || error}`);
            exitCode = 1;
            app.quit();
        }
    });
    app.connect('shutdown', () => {
        app.panel?.dispose();
        if (provider) Gtk.StyleContext.remove_provider_for_display(Gdk.Display.get_default(), provider);
    });
    try { app.acquire_socket(); }
    catch (error) { console.error(`Launcher startup: ${error.message}`); return 1; }
    app.hold();
    const status = await app.runAsync([]);
    return exitCode || status;
}

if (GLib.getenv('APP_LAUNCHER_IMPORT_ONLY') !== '1') System.exit(await main());
