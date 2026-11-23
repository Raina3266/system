import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import { LineDecoder } from '../../shared/astal/ipc.js';
import Battery from 'gi://AstalBattery';
import PowerProfiles from 'gi://AstalPowerProfiles';
import Notifd from 'gi://AstalNotifd';
import { emptyMetrics, formatBatteryStatus } from './logic.js';

// Bound native strings before encoding the pipe frame. Never truncate an action ID
// into another valid ID. Lengths allow for JSON's worst-case six-byte escaping.
function field(value, max) {
    return (value || '').slice(0, max).replace(/[\uD800-\uDBFF]$/, '');
}

function snapshot(native) {
    return {
        id: '',
        app: field(native.app_name, 512),
        summary: field(native.summary, 2048),
        body: field(native.body, 12288),
        icon: field(native.image || native.app_icon, 1024),
        urgency: native.urgency,
        transient: native.transient,
        timeout: native.expire_timeout,
        actions: (native.actions || []).slice(0, 16).filter((a) => a.id.length <= 128).map((a) => ({ identifier: a.id, text: field(a.label, 192) })),
        closed: false,
    };
}

// One process per dashboard owner. Never reconnect and replay queued operations.
export class DashboardPipe {
    io = new Gio.Cancellable();
    queue = [];
    writing = false;
    ended = false;
    queuedBytes = 0;
    drained;

    whenDrained(callback) {
        if (this.ended) return;
        this.drained = callback;
        if (!this.writing && !this.queue.length) void Promise.resolve().then(() => this.resume());
    }

    resume() {
        if (this.ended || this.writing || this.queue.length) return;
        const callback = this.drained;
        this.drained = undefined;
        callback?.();
    }

    constructor(receive) {
        this.receive = receive;
        this.child = Gio.Subprocess.new(
            [GLib.getenv('DASHBOARD_CONTROL') || 'dashboard-control'],
            Gio.SubprocessFlags.STDIN_PIPE | Gio.SubprocessFlags.STDOUT_PIPE,
        );
        this.child.wait_async(null, (_, result) => {
            try {
                this.child.wait_finish(result);
            } catch {
                /* Never log raw child data. */
            } finally {
                this.dispose();
            }
        });
        const input = this.child.get_stdout_pipe();
        const frames = new LineDecoder(32 * 1024 * 1024, { fatal: false });
        const read = () =>
            input.read_bytes_async(65536, GLib.PRIORITY_DEFAULT, this.io, (_, result) => {
                try {
                    const bytes = input.read_bytes_finish(result).get_data();
                    if (this.ended) {
                        input.close(null);
                        return;
                    }
                    if (!bytes?.length) {
                        this.dispose();
                        return;
                    }
                    frames.push(bytes, (line) => {
                        this.receive(JSON.parse(line));
                        return !this.ended;
                    });
                    if (!this.ended) read();
                } catch {
                    this.dispose();
                    try {
                        input.close(null);
                    } catch {}
                }
            });
        try {
            read();
        } catch {
            this.dispose();
        }
    }

    send(message) {
        if (this.ended) return;
        const bytes = new TextEncoder().encode(JSON.stringify(message) + '\n');
        if (bytes.length > 131072) {
            console.warn('Dashboard request exceeds frame limit');
            return;
        }
        if (this.queue.length >= 512 || this.queuedBytes + bytes.length > 8 * 1024 * 1024) {
            this.dispose();
            return;
        }
        this.queuedBytes += bytes.length;
        this.queue.push(bytes);
        this.flush();
    }

    flush() {
        if (this.ended || this.writing || !this.queue.length) return;
        this.writing = true;
        const pipe = this.child.get_stdin_pipe();
        const frame = this.queue.shift();
        try {
            pipe.write_all_async(frame, GLib.PRIORITY_DEFAULT, this.io, (_, result) => {
                this.writing = false;
                try {
                    const [ok, n] = pipe.write_all_finish(result);
                    if (!ok || n !== frame.length) this.dispose();
                } catch {
                    this.dispose();
                }
                this.queuedBytes -= frame.length;
                if (this.ended) {
                    try {
                        pipe.close(null);
                    } catch {}
                } else {
                    this.flush();
                    this.resume();
                }
            });
        } catch {
            this.writing = false;
            this.dispose();
        }
    }

    dispose() {
        if (this.ended) return;
        this.ended = true;
        this.drained = undefined;
        this.queue = [];
        this.io.cancel();
        if (!this.writing) {
            try {
                this.child.get_stdin_pipe()?.close(null);
            } catch {}
        }
        try {
            this.child.get_stdout_pipe()?.close(null);
        } catch {}
        this.receive({ event: 'offline' });
    }
}

// Astal bindings only: Rust owns notification history, admission and lifetime policy.
export class NotificationCenter {
    history = [];
    popups = [];
    native = Notifd.get_default();
    settings = new Gio.Settings({ schema_id: 'io.astal.notifd' });
    live = new Map();
    listeners = new Set();
    signals = [];
    disposed = false;
    offline = false;
    pending = new Map();
    pipe;

    start(pipe) {
        if (this.pipe || this.disposed || this.offline) return;
        this.pipe = pipe;
        this.native.ignore_timeout = true;
        this.native.default_timeout = -1;
        this.settings.set_strv('server-capabilites', ['actions', 'body', 'icon-static', 'persistence']);
        this.command({ kind: 'init', dnd: this.dnd, user: this.settings.get_user_value('dont-disturb') !== null });
        if (this.offline) return;
        for (const object of this.native.notifications) this.pending.set(object.id, object);
        this.signals = [
            this.native.connect('notified', (_, id) => {
                this.restoreOne(id);
                const object = this.native.get_notification(id);
                if (object) this.attach(object, false);
            }),
            this.native.connect('resolved', (_, id, reason) => {
                this.restoreOne(id);
                const live = this.live.get(id);
                if (live) {
                    this.live.delete(id);
                    this.command({ kind: 'resolved', token: live.token, reason });
                }
            }),
            this.native.connect('notify::dont-disturb', () => this.command({ kind: 'dnd', value: this.dnd })),
        ];
        this.scheduleRestore();
        Gio.Settings.sync();
    }

    restoreOne(id) {
        // A signal may overtake the bulk restore. Preserve per-ID ordering without
        // reattaching the old instance a second time after its replacement.
        const object = this.pending.get(id);
        if (!object) return;
        this.pending.delete(id);
        this.attach(object, true);
    }

    scheduleRestore() {
        if (!this.pending.size || this.offline || this.disposed) return;
        this.pipe.whenDrained(() => {
            const item = this.pending.keys().next();
            if (item.done) return;
            this.restoreOne(item.value);
            this.scheduleRestore();
        });
    }

    attach(native, restore) {
        if (this.offline || this.disposed) return;
        const token = GLib.uuid_string_random();
        this.live.set(native.id, { native, token });
        this.command({ kind: 'received', native: native.id, time: native.time, token, row: snapshot(native), restore });
    }

    command(command) {
        if (!this.disposed && !this.offline) this.pipe?.send({ op: 'notify', command });
    }

    receive(e) {
        if (this.disposed) return;
        if (e.event === 'notifications' || e.event === 'offline') {
            this.history = e.event === 'notifications' ? e.history : [];
            this.popups = e.event === 'notifications' ? e.popups : [];
            if (e.event === 'offline') {
                this.offline = true;
                this.live.clear();
                this.disconnect();
            }
            for (const cb of this.listeners) cb();
        } else if (e.event === 'native') {
            const live = this.live.get(e.native);
            if (!live || live.token !== e.token || this.native.get_notification(e.native) !== live.native) return;
            try {
                if (e.operation === 'expire') live.native.expire();
                if (e.operation === 'dismiss') live.native.dismiss();
                if (e.operation === 'invoke') live.native.invoke(e.argument);
            } catch {
                console.warn('Dashboard notification operation failed');
            }
        } else if (e.event === 'toggle' || e.event === 'seed') {
            if (e.event === 'toggle' || this.settings.get_user_value('dont-disturb') === null) {
                this.native.dont_disturb = e.event === 'toggle' ? !this.dnd : e.value;
                Gio.Settings.sync();
            }
            // A failed settings write is not an acknowledgment. Leave the seed
            // pending unless readback matches or an explicit native value wins.
            if (e.event !== 'seed' || this.dnd === e.value || this.settings.get_user_value('dont-disturb') !== null) {
                this.command({ kind: e.event === 'seed' ? 'seedAck' : 'dnd', value: this.dnd });
            }
        }
    }

    get dnd() {
        return this.native.dont_disturb;
    }

    subscribe(cb) {
        this.listeners.add(cb);
        return () => this.listeners.delete(cb);
    }

    setHovered(id, value, token = '') {
        this.command({ kind: 'hover', id, token, value });
    }

    dismissNotification(id, token = '') {
        this.command({ kind: 'dismiss', id, token });
    }

    invokeNotificationAction(id, identifier, token = '') {
        this.command({ kind: 'action', id, token, identifier });
    }

    clearNotifications() {
        if (this.disposed || this.offline) return;
        this.pending.clear(); // Clear includes the full current native collection below.
        let instances = [];
        for (const native of this.native.notifications) {
            let ref = this.live.get(native.id);
            if (!ref || ref.native !== native) {
                ref = { native, token: GLib.uuid_string_random() };
                this.live.set(native.id, ref);
            }
            instances.push({ native: native.id, token: ref.token });
            if (instances.length === 256) {
                this.command({ kind: 'clear', instances });
                instances = [];
            }
        }
        this.command({ kind: 'clear', instances });
    }

    toggleDnd() {
        this.command({ kind: 'toggle' });
    }

    disconnect() {
        this.pending.clear();
        this.signals.splice(0).forEach((id) => this.native.disconnect(id));
    }

    dispose() {
        if (this.disposed) return;
        this.disposed = true;
        this.disconnect();
        this.live.clear();
        this.listeners.clear();
    }
}

export class DashboardBackend {
    battery = { ready: false, present: false, percentage: -1, status: 'READING BATTERY STATUS' };
    profiles = [];
    activeProfile = '';
    batteryDevice = Battery.get_default();
    upower = new Battery.UPower();
    power = PowerProfiles.get_default();
    generation = 0;
    active = false;
    minutes = 0;
    readings = emptyMetrics();
    listeners = new Set();
    signals = [];
    disposed = false;
    failed = false;

    get offline() {
        return this.failed;
    }

    constructor(connect = (cb) => new DashboardPipe(cb), onFatal) {
        this.notifications = new NotificationCenter();
        this.pipe = connect((e) => {
            if (this.disposed || this.failed) return;
            if (e.event === 'offline') {
                this.failed = true;
                this.minutes = 0;
                this.readings = emptyMetrics();
                // EOF can arrive during connect/init, before the application owns this object.
                if (onFatal) {
                    void Promise.resolve().then(() => {
                        if (!this.disposed) onFatal();
                    });
                }
                this.notifications.receive(e);
                return;
            }
            if (e.event === 'metrics' && this.active && e.generation === this.generation) {
                this.readings = e.data;
                this.emit();
            } else if (e.event === 'timeout') {
                this.minutes = e.minutes;
                this.emit();
            } else {
                if (e.event === 'error') console.warn('Dashboard backend operation failed');
                this.notifications.receive(e);
            }
        });
        this.unsubscribeNotifications = this.notifications.subscribe(() => this.emit());
        this.notifications.start(this.pipe);
        const refresh = () => this.refresh();
        this.watch(this.batteryDevice, 'notify', refresh);
        this.watch(this.upower, 'notify', refresh);
        this.watch(this.power, 'notify::profiles', refresh);
        this.watch(this.power, 'notify::active-profile', refresh);
        this.refresh();
    }

    get metrics() {
        return this.readings;
    }

    subscribe(callback) {
        if (this.disposed) return () => {};
        this.listeners.add(callback);
        return () => this.listeners.delete(callback);
    }

    watch(object, signal, callback) {
        if (object) this.signals.push([object, object.connect(signal, callback)]);
    }

    refresh() {
        if (this.disposed) return;
        const device = this.batteryDevice;
        const ready = Boolean(device && this.upower.daemon_version);
        const present = ready && device.is_present;
        let status = 'READING BATTERY STATUS';
        if (ready) {
            if (!present) status = 'NO BATTERY DETECTED';
            else if (device.state === Battery.State.CHARGING) status = 'CHARGING';
            else if (device.state === Battery.State.PENDING_CHARGE) status = 'CONNECTED · CHARGING SOON';
            else if (device.state === Battery.State.FULLY_CHARGED) status = 'FULLY CHARGED';
            else if (this.upower.on_battery) status = 'ON BATTERY';
            else status = 'AC POWER';
        }
        this.battery = { ready, present, status,
            percentage: present && Number.isFinite(device.percentage) ? Math.max(0, Math.min(1, device.percentage)) : -1 };
        this.profiles = (this.power.get_profiles() || []).map((profile) => profile.profile).filter((profile) =>
            typeof profile === 'string' && profile.length > 0
        );
        this.activeProfile = this.power.active_profile || '';
        this.emit();
    }

    emit() {
        if (!this.disposed) {
            for (const callback of this.listeners) callback();
        }
    }

    setActive(active) {
        if (this.disposed || this.failed || this.active === active) return;
        this.active = active;
        this.readings = emptyMetrics();
        this.pipe.send({ op: 'active', active, generation: ++this.generation });
        this.emit();
    }

    get screenTimeoutMinutes() {
        return this.minutes;
    }

    setScreenTimeout(minutes) {
        if (!this.disposed && !this.failed) this.pipe.send({ op: 'timeout', minutes });
    }

    setProfile(profile) {
        if (this.disposed) return;
        if (!this.profiles.includes(profile)) throw new Error(`Unavailable power profile: ${profile}`);
        this.power.active_profile = profile;
    }

    status() {
        return { ...formatBatteryStatus(this.battery, this.activeProfile), historyCount: this.notifications.history.length,
            popupCount: this.notifications.popups.length, dnd: this.notifications.dnd, screenTimeoutMinutes: this.screenTimeoutMinutes,
            activeProfile: this.activeProfile };
    }

    dispose() {
        if (this.disposed) return;
        this.disposed = true;
        this.pipe.dispose();
        this.unsubscribeNotifications();
        this.notifications.dispose();
        for (const [object, signal] of this.signals) object.disconnect(signal);
        this.signals = [];
        this.listeners.clear();
    }
}
