import GLib from 'gi://GLib';
import Gio from 'gi://Gio';
import Gdk from 'gi://Gdk?version=4.0';
import Apps from 'gi://AstalApps';
import { applicationsFor } from './logic.js';

export class LauncherBackend {
    constructor(onChanged) {
        this.apps = new Apps.Apps({ show_hidden: false });
        this.refreshIdle = 0;
        this.appMonitor = Gio.AppInfoMonitor.get();
        // Astal reloads on this signal first; its read-only list does not emit notify::list.
        this.appSignal = this.appMonitor.connect('changed', () => {
            if (this.refreshIdle) return;
            this.refreshIdle = GLib.idle_add(GLib.PRIORITY_DEFAULT_IDLE, () => {
                this.refreshIdle = 0;
                onChanged();
                return GLib.SOURCE_REMOVE;
            });
        });
    }

    get applications() { return applicationsFor(this.apps); }

    query(search) { return applicationsFor(this.apps, search); }

    launch(application) { return application.launch(); }

    dispose() {
        if (this.refreshIdle) GLib.source_remove(this.refreshIdle);
        this.refreshIdle = 0;
        this.appMonitor.disconnect(this.appSignal);
    }
}

export function monitors() {
    const list = Gdk.Display.get_default().get_monitors();
    return Array.from({ length: list.get_n_items() }, (_, index) => list.get_item(index));
}

export async function focusedMonitor(name) {
    let connector = name;
    if (!connector) {
        try {
            const process = Gio.Subprocess.new([GLib.getenv('NIRI') || 'niri', 'msg', '-j', 'focused-output'],
                Gio.SubprocessFlags.STDOUT_PIPE | Gio.SubprocessFlags.STDERR_PIPE);
            const [, stdout] = await new Promise((resolve, reject) => {
                process.communicate_utf8_async(null, null, (source, result) => {
                    try { resolve(source.communicate_utf8_finish(result)); }
                    catch (error) { reject(error); }
                });
            });
            if (!process.get_successful()) throw new Error('niri focused-output failed');
            connector = JSON.parse(stdout)?.name;
        } catch (error) { console.warn(`Launcher monitor lookup: ${error.message}`); }
    }
    const available = monitors();
    if (name && !available.some(monitor => monitor.get_connector() === name)) {
        throw new Error(`No connected monitor named ${name}.`);
    }
    const monitor = available.find(item => item.get_connector() === connector) || available[0];
    if (!monitor) throw new Error('No connected monitor is available.');
    return monitor;
}
