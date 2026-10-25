import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Gtk from 'gi://Gtk?version=4.0';
import { box, label, style } from './notification.ts';

export type Metrics = {
    cpuPercent: number;
    memoryPercent: number;
    memoryUsedGiB: number;
    memoryTotalGiB: number;
    diskPercent: number;
    diskUsedGiB: number;
    diskTotalGiB: number;
    cpuTemperatureC: number;
};

const GiB = 1024 ** 3;
const clamp = (value: number, low: number, high: number) => Math.max(low, Math.min(high, value));

export function emptyMetrics(): Metrics {
    return {
        cpuPercent: -1, memoryPercent: -1, memoryUsedGiB: -1, memoryTotalGiB: -1,
        diskPercent: -1, diskUsedGiB: -1, diskTotalGiB: -1, cpuTemperatureC: -1,
    };
}

export type CpuSample = { total: number; idle: number };
export function parseCpu(text: string): CpuSample | null {
    const fields = (text.split(/\r?\n/, 1)[0] || '').trim().split(/\s+/);
    if (fields.shift() !== 'cpu' || fields.length < 4 || fields.some(value => !/^\d+$/.test(value)))
        return null;
    // Linux guest/guest_nice are already included in user/nice, not extra time.
    const values = fields.slice(0, 8).map(Number);
    const total = values.reduce((sum, value) => sum + value, 0);
    if (!Number.isSafeInteger(total)) return null;
    return { total, idle: values[3] + (values[4] ?? 0) };
}

export function cpuPercent(previous: CpuSample | null, current: CpuSample | null): number {
    if (!previous || !current || current.total <= previous.total || current.idle < previous.idle)
        return -1;
    const total = current.total - previous.total;
    const idle = current.idle - previous.idle;
    if (idle > total) return -1;
    return clamp(Math.round((1 - idle / total) * 100), 0, 100);
}

export function parseMemory(text: string): Pick<Metrics, 'memoryPercent' | 'memoryUsedGiB' | 'memoryTotalGiB'> {
    const missing = { memoryPercent: -1, memoryUsedGiB: -1, memoryTotalGiB: -1 };
    const values: Record<string, number> = {};
    for (const line of text.split(/\r?\n/)) {
        const match = /^([A-Za-z_]+):\s+(\d+)\s+kB\s*$/.exec(line);
        if (match) values[match[1]] = Number(match[2]) * 1024;
    }
    const total = values.MemTotal;
    const available = values.MemAvailable ?? (values.MemFree === undefined ? NaN
        : values.MemFree + (values.Buffers ?? 0) + (values.Cached ?? 0));
    if (!Number.isFinite(total) || total <= 0 || !Number.isFinite(available)) return missing;
    const used = total - clamp(available, 0, total);
    return { memoryPercent: Math.round(used / total * 100), memoryUsedGiB: used / GiB, memoryTotalGiB: total / GiB };
}

export function parseDisk(text: string): Pick<Metrics, 'diskPercent' | 'diskUsedGiB' | 'diskTotalGiB'> {
    const missing = { diskPercent: -1, diskUsedGiB: -1, diskTotalGiB: -1 };
    const lines = text.trim().split(/\r?\n/);
    const match = lines.length === 2 && /^\s*(\d+)\s+(\d+)\s+(\d+)%\s*$/.exec(lines[1]);
    if (!match) return missing;
    const [, used, total, percent] = match.map(Number);
    if (![used, total, percent].every(Number.isSafeInteger) || total <= 0) return missing;
    return { diskPercent: percent, diskUsedGiB: used / GiB, diskTotalGiB: total / GiB };
}

export function parseTemperature(text: string): number {
    if (!/^\s*\d+(?:\.\d+)?\s*$/.test(text)) return -1;
    const value = Number(text.trim());
    if (!Number.isFinite(value) || value <= 0) return -1;
    return value > 1000 ? value / 1000 : value;
}

const PRIORITY = GLib.PRIORITY_DEFAULT;
type DiskOperation = { io: Gio.Cancellable; child: Gio.Subprocess; timeout: number };

function readText(path: string, cancellable: Gio.Cancellable): Promise<string> {
    return new Promise((resolve, reject) => {
        const file = Gio.File.new_for_path(path);
                file.load_contents_async(cancellable, (_file: Gio.File | null, result: Gio.AsyncResult) => {
            try {
                const [ok, bytes] = file.load_contents_finish(result);
                if (!ok) throw new Error(`Could not read ${path}`);
                resolve(new TextDecoder().decode(bytes));
            } catch (error) { reject(error); }
        });
    });
}

// Only kernel files and df lack an Astal service. No work starts before setActive.
export class MetricsSampler {
    data: Metrics = emptyMetrics();
    private active = false;
    private disposed = false;
    private generation = 0;
    private timers: number[] = [];
    private previousCpu: CpuSample | null = null;
    private reading: Gio.Cancellable | null = null;
    private disk: DiskOperation | null = null;
    private onChange: () => void;

    constructor(onChange: () => void) { this.onChange = onChange; }

    setActive(active: boolean) {
        if (this.disposed || active === this.active) return;
        this.active = active;
        this.generation++;
        this.previousCpu = null;
        if (!active) {
            this.timers.forEach(timer => GLib.source_remove(timer));
            this.timers = [];
            this.reading?.cancel();
            if (this.disk) {
                this.disk.io.cancel();
                this.disk.child.force_exit();
                if (this.disk.timeout) GLib.source_remove(this.disk.timeout);
                this.disk.timeout = 0;
            }
            return;
        }
        this.data = emptyMetrics();
        this.onChange();
        void this.refresh();
        void this.refreshDisk();
        for (const [interval, refresh] of [[2500, () => void this.refresh()], [30000, () => void this.refreshDisk()]] as const) {
            this.timers.push(GLib.timeout_add(PRIORITY, interval, () => {
                refresh();
                return GLib.SOURCE_CONTINUE;
            }));
        }
    }

    private async refresh() {
        if (!this.active || this.reading) return;
        const generation = this.generation;
        const io = new Gio.Cancellable();
        this.reading = io;
        try {
            // Missing readings replace stale data rather than freezing a previous sample.
            const [cpu, memory, temperature] = await Promise.all([
                '/proc/stat', '/proc/meminfo', '/sys/class/thermal/thermal_zone0/temp',
            ].map(path => readText(path, io).catch(() => '')));
            if (!this.active || generation !== this.generation) return;
            const sample = parseCpu(cpu);
            this.data = { ...this.data, cpuPercent: cpuPercent(this.previousCpu, sample),
                ...parseMemory(memory), cpuTemperatureC: parseTemperature(temperature) };
            this.previousCpu = sample;
            this.onChange();
        } finally {
            this.reading = null;
            // Reopening during cancellation waits for the old reads to finish first.
            if (this.active && generation !== this.generation) void this.refresh();
        }
    }

    private async refreshDisk() {
        if (!this.active || this.disk) return;
        const generation = this.generation;
        let operation: DiskOperation | null = null;
        try {
            const launcher = new Gio.SubprocessLauncher({ flags: Gio.SubprocessFlags.STDOUT_PIPE | Gio.SubprocessFlags.STDERR_SILENCE });
            launcher.setenv('LC_ALL', 'C', true);
            const child = launcher.spawnv([GLib.getenv('COREUTILS_DF') || 'df', '-B1', '--output=used,size,pcent', '/']);
            operation = { child, io: new Gio.Cancellable(), timeout: 0 };
            this.disk = operation;
            operation.timeout = GLib.timeout_add(PRIORITY, 10000, () => {
                operation!.timeout = 0;
                operation!.io.cancel();
                child.force_exit();
                return GLib.SOURCE_REMOVE;
            });
            // Waiting is never cancelled: force-exited children are still reaped.
            const waited = new Promise<void>((resolve, reject) => child.wait_async(null, (_process: Gio.Subprocess | null, result: Gio.AsyncResult) => {
                try { child.wait_finish(result); resolve(); } catch (error) { reject(error); }
            }));
            const output = new Promise<string>((resolve, reject) => child.communicate_utf8_async(null, operation!.io, (_process: Gio.Subprocess | null, result: Gio.AsyncResult) => {
                try {
                    const [ok, stdout] = child.communicate_utf8_finish(result);
                    if (!ok) throw new Error('df communication failed');
                    resolve(stdout || '');
                } catch (error) { reject(error); }
            }));
            const [waitResult, outputResult] = await Promise.allSettled([waited, output]);
            const text = waitResult.status === 'fulfilled' && outputResult.status === 'fulfilled' && child.get_successful()
                ? outputResult.value : '';
            if (this.active && generation === this.generation) {
                this.data = { ...this.data, ...parseDisk(text) };
                this.onChange();
            }
        } catch (error) {
            if (this.active && generation === this.generation) {
                this.data = { ...this.data, ...parseDisk('') };
                this.onChange();
            }
        } finally {
            if (operation?.timeout) GLib.source_remove(operation.timeout);
            this.disk = null;
            if (this.active && generation !== this.generation) void this.refreshDisk();
        }
    }

    dispose() {
        if (this.disposed) return;
        this.setActive(false);
        this.disposed = true;
    }
}

export class MetricBar {
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
