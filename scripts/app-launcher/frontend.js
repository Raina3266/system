import GLib from 'gi://GLib';
import Gio from 'gi://Gio';
import GObject from 'gi://GObject';
import Gtk from 'gi://Gtk?version=4.0';
import Gdk from 'gi://Gdk?version=4.0';
import Pango from 'gi://Pango';
import Astal from 'gi://Astal?version=4.0';
import AstalIO from 'gi://AstalIO';
import System from 'system';
import { LauncherBackend, monitors, focusedMonitor } from './backend.js';
import { ANIMATION_MS, BAR_GAP, PANEL_LEFT, TILE_WIDTH, TILE_HEIGHT, TILE_GAP, GRID_COLUMNS, SEARCH_HEIGHT, SEARCH_GAP,
    parseRequest, panelLayout, nextIndex } from './logic.js';

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
    // Match the media panel's outline and translucent background.
    const cut = 10, inset = 0.5;
    cr.moveTo(cut + inset, inset);
    cr.lineTo(width - inset, inset);
    cr.lineTo(width - inset, height - cut - inset);
    cr.lineTo(width - cut - inset, height - inset);
    cr.lineTo(inset, height - inset);
    cr.lineTo(inset, cut + inset);
    cr.closePath();
    cr.setSourceRGBA(37 / 255, 37 / 255, 47 / 255, 0.9);
    cr.fillPreserve();
    cr.setSourceRGBA(252 / 255, 238 / 255, 10 / 255, 1);
    cr.setLineWidth(1);
    cr.stroke();
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

function tile(application, activate) {
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
    const button = new Gtk.Button({ child: content, width_request: TILE_WIDTH, height_request: TILE_HEIGHT,
        css_classes: ['app-tile'] });
    button.connect('clicked', activate);
    return button;
}

export class LauncherPanel {
    constructor(app) {
        this.backend = new LauncherBackend(() => { if (this.opened) this.render(); });
        this.opened = false;
        this.monitor = null;
        this.columns = GRID_COLUMNS;
        this.selected = 0;
        this.results = [];
        this.tiles = [];
        this.tileCache = new WeakMap();
        this.hideTimer = 0;
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
        this.card = new Gtk.Overlay({ css_classes: ['launcher-card'] });
        this.card.set_child(frame(drawPanelFrame));
        this.search = new Gtk.SearchEntry({ placeholder_text: 'Search applications',
            height_request: SEARCH_HEIGHT, hexpand: true, search_delay: 80,
            css_classes: ['launcher-search'] });
        this.search.connect('search-changed', () => this.render());
        this.search.connect('activate', () => this.launch(this.selected));
        this.grid = new Gtk.Grid({ column_spacing: TILE_GAP, row_spacing: TILE_GAP,
            column_homogeneous: true, row_homogeneous: true,
            halign: Gtk.Align.CENTER, valign: Gtk.Align.START });
        this.empty = label('NO APPLICATIONS', 'launcher-empty', { vexpand: true });
        this.gridStack = new Gtk.Stack();
        this.gridStack.add_named(this.grid, 'apps');
        this.gridStack.add_named(this.empty, 'empty');
        this.scroll = new Gtk.ScrolledWindow({ hscrollbar_policy: Gtk.PolicyType.NEVER,
            vscrollbar_policy: Gtk.PolicyType.AUTOMATIC, child: this.gridStack,
            css_classes: ['launcher-scroll'], propagate_natural_height: false });
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
            if (valid && (x < bounds.get_x() || y < bounds.get_y()
                || x > bounds.get_x() + bounds.get_width() || y > bounds.get_y() + bounds.get_height())) this.close();
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
        if (this.hideTimer) GLib.source_remove(this.hideTimer);
        this.hideTimer = 0;
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
        if (this.hideTimer) GLib.source_remove(this.hideTimer);
        this.hideTimer = 0;
        this.opened = false;
        this.window.keymode = Astal.Keymode.NONE;
        this.revealer.reveal_child = false;
        if (animate && this.window.visible) {
            // Keep the surface mapped until the reverse slide finishes.
            this.hideTimer = GLib.timeout_add(GLib.PRIORITY_DEFAULT, ANIMATION_MS + 30, () => {
                this.hideTimer = 0;
                if (!this.opened) this.window.visible = false;
                return GLib.SOURCE_REMOVE;
            });
        } else this.window.visible = false;
    }

    getTile(application, index) {
        let button = this.tileCache.get(application);
        if (!button) {
            button = tile(application, () => this.launch(button.resultIndex));
            const focus = new Gtk.EventControllerFocus();
            focus.connect('enter', () => this.select(button.resultIndex));
            button.add_controller(focus);
            this.tileCache.set(application, button);
        }
        // Native reloads create new application objects; old tiles can then be collected.
        button.resultIndex = index;
        button.remove_css_class('selected');
        button.remove_css_class('launch-error');
        return button;
    }

    render() {
        const gridFocused = this.tiles.includes(this.window.get_focus());
        const query = this.search.get_text().trim();
        this.results = this.backend.query(query);
        while (this.grid.get_first_child()) this.grid.remove(this.grid.get_first_child());
        this.tiles = this.results.map((application, index) => {
            const button = this.getTile(application, index);
            this.grid.attach(button, index % this.columns, Math.floor(index / this.columns), 1, 1);
            return button;
        });
        this.gridStack.visible_child_name = this.results.length ? 'apps' : 'empty';
        this.empty.label = query ? 'NO MATCHING APPLICATIONS' : 'NO APPLICATIONS';
        const selectFirst = this.results.length && (query || gridFocused);
        this.select(selectFirst ? 0 : -1, this.opened && gridFocused);
        this.scroll.get_vadjustment().set_value(0);
    }

    select(index, focus = false) {
        this.tiles[this.selected]?.remove_css_class('selected');
        this.selected = index;
        this.tiles[index]?.add_css_class('selected');
        if (focus) {
            const button = this.tiles[index];
            if (!button) { this.root.grab_focus(); return; }
            button.grab_focus();
            const [valid, bounds] = button.compute_bounds(this.grid);
            if (valid) {
                const adjustment = this.scroll.get_vadjustment();
                const top = bounds.get_y(), bottom = top + bounds.get_height();
                if (top < adjustment.value) adjustment.value = top;
                else if (bottom > adjustment.value + adjustment.page_size) adjustment.value = bottom - adjustment.page_size;
            }
        }
    }

    keyPressed(key, state = 0) {
        if (!this.opened) return false;
        if (key === Gdk.KEY_Escape) { this.close(); return true; }
        if (key === Gdk.KEY_Return || key === Gdk.KEY_KP_Enter) { this.launch(this.selected); return true; }
        const focused = this.window.get_focus();
        const searchFocused = focused === this.search || Boolean(focused?.is_ancestor(this.search));
        const name = Gdk.keyval_name(key);
        if (name === 'f' && (state & Gdk.ModifierType.CONTROL_MASK)) { this.search.grab_focus(); return true; }
        if (searchFocused) {
            if (name === 'Down' && this.results.length) { this.select(0, true); return true; }
            return false;
        }
        if (['Left', 'Right', 'Up', 'Down', 'Home', 'End'].includes(name)) {
            if (name === 'Up' && this.selected < this.columns) this.search.grab_focus();
            else this.select(nextIndex(this.selected, name, this.columns, this.results.length), true);
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
                AstalIO.write_sock(connection, response, (_source, result) => {
                    try { AstalIO.write_sock_finish(result); }
                    catch (error) { console.warn(`Launcher reply: ${error.message}`); }
                    finally { connection.close_async(GLib.PRIORITY_DEFAULT, null, (source, result) => {
                        try { source.close_finish(result); }
                        catch (error) { console.warn(`Launcher connection: ${error.message}`); }
                    }); }
                });
            }).catch(error => console.error(`Launcher request: ${error.message}`));
        }
    });

export async function main() {
    Gtk.init();
    GLib.unsetenv('LD_PRELOAD');
    const app = new LauncherApplication({ instance_name: 'app-launcher' });
    let exitCode = 0;
    let provider;
    app.connect('activate', () => {
        if (app.panel) return;
        try {
            const config = GLib.getenv('APP_LAUNCHER_CONFIG') || System.programPath;
            provider = new Gtk.CssProvider();
            provider.load_from_path(GLib.build_filenamev([GLib.path_get_dirname(config), 'style.css']));
            Gtk.StyleContext.add_provider_for_display(Gdk.Display.get_default(), provider, Gtk.STYLE_PROVIDER_PRIORITY_USER + 1);
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
