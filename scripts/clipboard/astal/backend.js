import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Gdk from 'gi://Gdk?version=4.0';
import { localFilePath } from './logic.js';

export function historyRoot() {
    return GLib.getenv('CLIPBOARD_DATA_DIR')
        || GLib.build_filenamev([GLib.get_user_data_dir(), 'clipboard']);
}

export class ClipboardBackend {
    constructor({ onChange = () => {} } = {}) {
        this.onChange = onChange;
        this.processes = new Set();
        this.fileWatches = new Map();
        this.pruneMissing = false;
        this.disposed = false;
    }
    request(message) {
        if (this.disposed) return Promise.reject(new Error('Clipboard panel is shutting down.'));
        return new Promise((resolve, reject) => {
            let process;
            try {
                process = Gio.Subprocess.new([GLib.getenv('CLIPBOARD_CUSTOM_HELPER') || 'clipboard-custom'],
                    Gio.SubprocessFlags.STDIN_PIPE | Gio.SubprocessFlags.STDOUT_PIPE | Gio.SubprocessFlags.STDERR_PIPE);
            } catch (error) { reject(error); return; }
            this.processes.add(process);
            let timer = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 10000, () => {
                timer = 0;
                process.force_exit();
                return GLib.SOURCE_REMOVE;
            });
            process.communicate_utf8_async(JSON.stringify(message), null, (source, result) => {
                if (timer) GLib.source_remove(timer);
                this.processes.delete(process);
                try {
                    const [, stdout] = source.communicate_utf8_finish(result);
                    if (!source.get_successful()) throw new Error('Clipboard history helper failed.');
                    const reply = JSON.parse(stdout);
                    if (reply.error) throw new Error(reply.error);
                    if (this.disposed) throw new Error('Clipboard panel is shutting down.');
                    // Helper discovery migrates/creates the directory first. Watch
                    // before handing its snapshot to the model, closing the gap
                    // between the first prepare request and frontend.watch().
                    if (this.watch()) this.invalidate();
                    resolve(reply);
                } catch (error) { reject(error); }
            });
        });
    }
    watch() {
        if (this.monitor || this.disposed) return;
        let monitor;
        try {
            monitor = Gio.File.new_for_path(historyRoot()).monitor_directory(Gio.FileMonitorFlags.NONE, null);
            monitor.connect('changed', (_, file, other) => {
                if (this.monitor !== monitor || this.disposed) return;
                if (![file, other].some(file => file?.get_basename() === 'history.json')) return;
                this.invalidate();
            });
            this.monitor = monitor;
            return true;
        } catch {
            // Monitoring is optional: a resource/setup failure must not turn a
            // committed helper mutation into a failed save with a stale base.
            monitor?.cancel();
        }
    }
    invalidate(pruneMissing = false) {
        if (this.disposed) return;
        this.pruneMissing ||= pruneMissing;
        if (this.debounce) GLib.source_remove(this.debounce);
        this.debounce = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 100, () => {
            this.debounce = 0;
            const pruneMissing = this.pruneMissing;
            this.pruneMissing = false;
            if (!this.disposed) this.onChange({ pruneMissing });
            return GLib.SOURCE_REMOVE;
        });
    }
    watchFiles(items) {
        if (this.disposed || this.watchedItems === items) return;
        this.watchedItems = items;
        const directories = new Map();
        for (const item of items) {
            const path = localFilePath(item);
            if (!path) continue;
            const file = Gio.File.new_for_path(path), parent = file.get_parent();
            if (!parent) continue;
            const directory = parent.get_path();
            if (!directories.has(directory)) directories.set(directory, { parent, paths: new Set() });
            directories.get(directory).paths.add(file.get_path());
        }
        for (const [directory, watch] of this.fileWatches) {
            if (directories.has(directory)) continue;
            this.fileWatches.delete(directory);
            watch.monitor.cancel();
        }
        for (const [directory, { parent, paths }] of directories) {
            const existing = this.fileWatches.get(directory);
            if (existing) { existing.paths = paths; continue; }
            const watch = { paths, monitor: null };
            try {
                watch.monitor = parent.monitor_directory(Gio.FileMonitorFlags.WATCH_MOVES, null);
                watch.monitor.connect('changed', (_, file, other, event) => {
                    if (this.fileWatches.get(directory) !== watch) return;
                    const events = Gio.FileMonitorEvent;
                    if ([events.PRE_UNMOUNT, events.UNMOUNTED].includes(event)) {
                        this.invalidate(true);
                        return;
                    }
                    if (![events.DELETED, events.MOVED, events.RENAMED, events.MOVED_OUT].includes(event)) return;
                    if ([file, other].some(file => {
                        const path = file?.get_path();
                        return path === directory || watch.paths.has(path);
                    })) this.invalidate(true);
                });
                this.fileWatches.set(directory, watch);
            } catch {
                // Missing or inaccessible parents must not prevent the panel from opening.
                watch.monitor?.cancel();
            }
        }
    }
    unwatchFiles() {
        this.watchedItems = null;
        const watches = [...this.fileWatches.values()];
        this.fileWatches.clear();
        for (const watch of watches) watch.monitor.cancel();
    }
    async trashFile(path) {
        if (this.disposed) throw new Error('Clipboard panel is shutting down.');
        if (typeof path !== 'string' || !GLib.path_is_absolute(path) || path.includes('\0'))
            throw new Error('Cannot delete an invalid local file path.');
        const file = Gio.File.new_for_path(path);
        try {
            const info = await new Promise((resolve, reject) => file.query_info_async('standard::type',
                Gio.FileQueryInfoFlags.NOFOLLOW_SYMLINKS, GLib.PRIORITY_DEFAULT, null, (source, result) => {
                    try { resolve(source.query_info_finish(result)); } catch (error) { reject(error); }
                }));
            if (![Gio.FileType.REGULAR, Gio.FileType.SYMBOLIC_LINK].includes(info.get_file_type()))
                throw new Error('Only files and symbolic links can be moved to Trash here. Use your file manager for folders.');
            if (this.disposed) throw new Error('Clipboard panel is shutting down.');
            await new Promise((resolve, reject) => file.trash_async(GLib.PRIORITY_DEFAULT, null, (source, result) => {
                try { source.trash_finish(result); resolve(); } catch (error) { reject(error); }
            }));
            return true;
        } catch (error) {
            // A stale history entry can still be removed without touching disk.
            if (error.matches?.(Gio.IOErrorEnum, Gio.IOErrorEnum.NOT_FOUND)) return false;
            throw error;
        }
    }
    async publish(payload) {
        const bytes = payload.file ? await new Promise((resolve, reject) => {
            Gio.File.new_for_path(payload.file).load_bytes_async(null, (file, result) => {
                try { resolve(file.load_bytes_finish(result)[0]); } catch (error) { reject(error); }
            });
        }) : new GLib.Bytes(new TextEncoder().encode(payload.text || ''));
        const provider = Gdk.ContentProvider.new_for_bytes(payload.mime, bytes);
        if (!Gdk.Display.get_default().get_clipboard().set_content(provider))
            throw new Error('The compositor rejected the clipboard content.');
        // This resident application, not a short-lived Rust process, retains
        // clipboard ownership after the window is hidden.
    }
    dispose() {
        this.disposed = true;
        if (this.debounce) GLib.source_remove(this.debounce);
        this.debounce = 0;
        this.pruneMissing = false;
        this.monitor?.cancel();
        this.unwatchFiles();
        for (const process of this.processes) process.force_exit();
        this.processes.clear();
    }
}
