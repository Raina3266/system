import GLib from 'gi://GLib';
import Gio from 'gi://Gio';
import Gdk from 'gi://Gdk?version=4.0';
import Apps from 'gi://AstalApps';
import { communicate } from '../shared/astal/ipc.js';

function applicationsFor(apps, query = '') {
    const search = query.trim();
    if (search) return apps.fuzzy_query(search);
    return [...apps.list].sort((a, b) =>
        a.name.localeCompare(b.name) || a.entry.localeCompare(b.entry));
}

export class LauncherBackend {
    constructor(onChanged) {
        this.apps = new Apps.Apps({ show_hidden: false });
        this.sortedApplications = null;
        this.refreshIdle = 0;
        this.appMonitor = Gio.AppInfoMonitor.get();
        // Astal reloads on this signal first; its read-only list does not emit notify::list.
        this.appSignal = this.appMonitor.connect('changed', () => {
            this.sortedApplications = null;
            if (this.refreshIdle) return;
            this.refreshIdle = GLib.idle_add(GLib.PRIORITY_DEFAULT_IDLE, () => {
                this.refreshIdle = 0;
                onChanged();
                return GLib.SOURCE_REMOVE;
            });
        });
    }

    get applications() { return this.sortedApplications ??= applicationsFor(this.apps); }

    query(search) {
        return search.trim() ? applicationsFor(this.apps, search) : this.applications;
    }

    launch(application) { return application.launch(); }

    dispose() {
        if (this.refreshIdle) GLib.source_remove(this.refreshIdle);
        this.refreshIdle = 0;
        this.appMonitor.disconnect(this.appSignal);
    }
}

export function monitors() {
    return [...Gdk.Display.get_default().get_monitors()];
}

export async function focusedMonitor(name) {
    let connector = name;
    if (!connector) {
        try {
            const stdout = await communicate(
                [GLib.getenv('PANEL_BACKEND') || 'panel-launcher', 'runtime', 'focused-output'],
                {
                    maxBytes: 64 * 1024,
                    timeoutMs: 10000,
                    flags: Gio.SubprocessFlags.STDERR_SILENCE,
                    errors: { failed: 'Focused-monitor helper failed.' },
                });
            const reply = JSON.parse(stdout);
            if (!reply || (reply.name !== null && typeof reply.name !== 'string'))
                throw new Error('Invalid focused-monitor helper response.');
            connector = reply.name;
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
