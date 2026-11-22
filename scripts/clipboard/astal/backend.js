import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Gdk from 'gi://Gdk?version=4.0';

const helper = () => GLib.getenv('CLIPBOARD_CUSTOM_HELPER') || 'clipboard-custom';

// Transport/cancellation and native clipboard ownership only. Rust owns history,
// linked-source watches, deletion guards, Trash, and MIME/payload selection.
export class ClipboardBackend {
    constructor({ onChange = () => {} } = {}) {
        this.onChange = onChange;
        this.processes = new Set();
        this.cancellables = new Set();
        this.watching = false;
        this.observer = null;
        this.disposed = false;
    }
    request(message) {
        if (this.disposed) return Promise.reject(new Error('Clipboard panel is shutting down.'));
        return new Promise((resolve, reject) => {
            let process;
            try {
                process = Gio.Subprocess.new([helper()],
                    Gio.SubprocessFlags.STDIN_PIPE | Gio.SubprocessFlags.STDOUT_PIPE | Gio.SubprocessFlags.STDERR_SILENCE);
            } catch (error) { reject(error); return; }
            const cancellable = new Gio.Cancellable();
            this.processes.add(process);
            this.cancellables.add(cancellable);
            let timedOut = false;
            let timer = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 10000, () => {
                timer = 0;
                timedOut = true;
                cancellable.cancel();
                process.force_exit();
                return GLib.SOURCE_REMOVE;
            });
            process.communicate_utf8_async(JSON.stringify(message), cancellable, (source, result) => {
                if (timer) GLib.source_remove(timer);
                this.processes.delete(process);
                this.cancellables.delete(cancellable);
                try {
                    const [, stdout] = source.communicate_utf8_finish(result);
                    if (this.disposed) throw new Error('Clipboard panel is shutting down.');
                    if (timedOut) throw new Error('Clipboard history request timed out. Reopen the panel to check its result.');
                    if (!source.get_successful()) throw new Error('Clipboard history helper failed.');
                    const reply = JSON.parse(stdout);
                    if (reply.error) throw new Error(reply.error);
                    // Optional observer setup must never reject a committed save.
                    if (this.watching) this.watch();
                    resolve(reply);
                } catch (error) {
                    reject(this.disposed ? new Error('Clipboard panel is shutting down.')
                        : timedOut ? new Error('Clipboard history request timed out. Reopen the panel to check its result.') : error);
                }
            });
        });
    }
    watch() {
        if (this.disposed) return;
        this.watching = true;
        if (this.observer) return;
        const observer = { process: null, stream: null, cancellable: new Gio.Cancellable() };
        try {
            observer.process = Gio.Subprocess.new([helper(), 'observe'],
                Gio.SubprocessFlags.STDOUT_PIPE | Gio.SubprocessFlags.STDERR_SILENCE);
            observer.stream = new Gio.DataInputStream({ base_stream: observer.process.get_stdout_pipe() });
            this.observer = observer;
            const read = () => observer.stream.read_line_async(GLib.PRIORITY_DEFAULT, observer.cancellable, (source, result) => {
                try {
                    const [line] = source.read_line_finish_utf8(result);
                    if (this.observer !== observer || this.disposed) return;
                    if (line === null) { this.stopObserver(observer); return; }
                    const change = JSON.parse(line);
                    if (typeof change.pruneMissing !== 'boolean') throw new Error('Invalid observer response.');
                    this.onChange(change);
                    if (this.observer === observer && !this.disposed) read();
                } catch {
                    // No polling/restart loop: retry on the next request or opening.
                    this.stopObserver(observer);
                }
            });
            read();
        } catch {
            this.stopObserver(observer);
        }
    }
    stopObserver(observer = this.observer) {
        if (!observer || observer.stopped) return;
        observer.stopped = true;
        if (this.observer === observer) this.observer = null;
        observer.cancellable.cancel();
        observer.process?.force_exit();
        try { observer.stream?.close(null); } catch { /* Native read may still be completing. */ }
    }
    unwatch() {
        this.watching = false;
        this.stopObserver();
    }
    async publish(payload) {
        if (this.disposed) throw new Error('Clipboard panel is shutting down.');
        const cancellable = new Gio.Cancellable();
        this.cancellables.add(cancellable);
        try {
            // The exact file/MIME/text is selected and checked by Rust. Loading
            // its cached image directly into GLib.Bytes avoids JSON byte copies.
            const bytes = payload.file ? await new Promise((resolve, reject) => {
                Gio.File.new_for_path(payload.file).load_bytes_async(cancellable, (file, result) => {
                    try { resolve(file.load_bytes_finish(result)[0]); } catch (error) { reject(error); }
                });
            }) : new GLib.Bytes(new TextEncoder().encode(payload.text));
            if (this.disposed || cancellable.is_cancelled()) throw new Error('Clipboard publication was cancelled.');
            const provider = Gdk.ContentProvider.new_for_bytes(payload.mime, bytes);
            if (!Gdk.Display.get_default().get_clipboard().set_content(provider))
                throw new Error('The compositor rejected the clipboard content.');
            // The resident native application retains ownership while hidden.
        } finally {
            this.cancellables.delete(cancellable);
        }
    }
    dispose() {
        this.disposed = true;
        this.unwatch();
        for (const cancellable of this.cancellables) cancellable.cancel();
        this.cancellables.clear();
        for (const process of this.processes) process.force_exit();
        this.processes.clear();
    }
}
