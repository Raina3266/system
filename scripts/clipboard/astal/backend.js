import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Gdk from 'gi://Gdk?version=4.0';

export function historyRoot() {
    return GLib.getenv('ROFI_CLIPBOARD_DATA_DIR')
        || GLib.build_filenamev([GLib.get_user_data_dir(), 'rofi-clipboard']);
}

export class ClipboardBackend {
    constructor({ onChange = () => {} } = {}) {
        this.onChange = onChange;
        this.processes = new Set();
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
                    resolve(reply);
                } catch (error) { reject(error); }
            });
        });
    }
    watch() {
        if (this.monitor || this.disposed) return;
        this.monitor = Gio.File.new_for_path(historyRoot()).monitor_directory(Gio.FileMonitorFlags.NONE, null);
        this.monitor.connect('changed', (_, file, other) => {
            if (![file, other].some(file => file?.get_basename() === 'history.json')) return;
            if (this.debounce) GLib.source_remove(this.debounce);
            this.debounce = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 100, () => {
                this.debounce = 0;
                this.onChange();
                return GLib.SOURCE_REMOVE;
            });
        });
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
        this.monitor?.cancel();
        for (const process of this.processes) process.force_exit();
        this.processes.clear();
    }
}
