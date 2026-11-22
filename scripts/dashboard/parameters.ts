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

export function emptyMetrics(): Metrics {
    return {
        cpuPercent: -1, memoryPercent: -1, memoryUsedGiB: -1, memoryTotalGiB: -1,
        diskPercent: -1, diskUsedGiB: -1, diskTotalGiB: -1, cpuTemperatureC: -1,
    };
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
