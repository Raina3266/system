import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Gtk from 'gi://Gtk?version=4.0';
import Gdk from 'gi://Gdk?version=4.0';

// Theme loaders often return only a palette. Keep their native monitors (and
// reload closures/providers) alive for the panel process, even while hidden.
const monitors = new Set();

/** Load CSS once, then reload only CSS; palette definitions stay fixed until restart. */
export function loadPanelStyle({ entry, definitions = '', label = 'Panel' }) {
    const override = GLib.getenv('PANEL_CSS_FILE');
    if (!override && !entry) throw new Error(`${label} stylesheet needs an entry file or PANEL_CSS_FILE.`);
    const path = override || GLib.build_filenamev([GLib.path_get_dirname(entry), 'style.css']);
    const file = Gio.File.new_for_path(path);
    const provider = new Gtk.CssProvider();
    const errors = [];
    provider.connect('parsing-error', (_provider, _section, error) => errors.push(error.message));
    const report = error => console.error(`${label} stylesheet (${path}): ${error.message || error}`);
    let previous = null;

    function apply(css) {
        errors.length = 0;
        provider.load_from_string(css);
        if (errors.length) throw new Error(errors.join('; '));
    }

    function reload() {
        try {
            const [ok, bytes] = GLib.file_get_contents(path);
            if (!ok) throw new Error('Could not read CSS file.');
            const css = `${definitions}\n${new TextDecoder().decode(bytes)}`;
            if (css === previous) return;
            try { apply(css); }
            catch (error) {
                // Loading replaces provider contents even on a parsing error.
                if (previous !== null) apply(previous);
                throw error;
            }
            previous = css;
        } catch (error) {
            report(error);
            if (previous === null) throw error;
        }
    }

    reload();
    const display = Gdk.Display.get_default();
    if (!display) throw new Error(`${label} stylesheet needs a graphical display.`);
    // The desktop stylesheet has USER priority; panel-scoped rules override it.
    Gtk.StyleContext.add_provider_for_display(display, provider, Gtk.STYLE_PROVIDER_PRIORITY_USER + 1);
    try {
        // Watch the directory, not the inode: editors commonly rename a temp
        // file over style.css. Either side of a move can name our target.
        const monitor = file.get_parent().monitor_directory(Gio.FileMonitorFlags.WATCH_MOVES, null);
        let timer = 0;
        monitor.connect('changed', (_monitor, changed, other) => {
            if (!changed?.equal(file) && !other?.equal(file)) return;
            if (timer) GLib.source_remove(timer);
            timer = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 120, () => {
                timer = 0;
                reload();
                return GLib.SOURCE_REMOVE;
            });
        });
        monitors.add(monitor);
    } catch (error) {
        report(new Error(`Could not monitor CSS directory: ${error.message || error}`));
    }
    return provider;
}
