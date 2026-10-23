import GLib from 'gi://GLib';
import Gio from 'gi://Gio';
import Gtk from 'gi://Gtk?version=4.0';
import Gdk from 'gi://Gdk?version=4.0';
import GdkPixbuf from 'gi://GdkPixbuf';
import Pango from 'gi://Pango';
import Astal from 'gi://Astal?version=4.0';
import AstalIO from 'gi://AstalIO';
import GObject from 'gi://GObject';
import { exit } from 'system';
import { MediaBackend } from './backend.js';
import { formatTime } from './logic.js';

// Theme and metadata presentation.
function read(path) {
    if (!path) throw new Error('Run through media-panel, or set MEDIA_PANEL_CONFIG and MEDIA_PANEL_PALETTE.');
    const [ok, contents] = GLib.file_get_contents(path);
    if (!ok) throw new Error(`Could not read media theme: ${path}`);
    return new TextDecoder().decode(contents);
}

function loadTheme(entry, palettePath) {
    const palette = JSON.parse(read(palettePath));
    for (const key of ['yellow', 'cyan', 'pink', 'red', 'dimPink', 'separator', 'foreground', 'secondaryBackground'])
        if (!/^#[a-f\d]{6}$/i.test(palette[key])) throw new Error(`Invalid media palette colour: ${key}`);
    const definitions = Object.entries(palette)
        .filter(([key, value]) => /^[a-zA-Z]+$/.test(key) && /^#[a-f\d]{6}$/i.test(value))
        .map(([key, value]) => `@define-color mp_${key} ${value};`).join('\n');
    const css = `${definitions}\n${read(GLib.build_filenamev([GLib.path_get_dirname(entry), 'style.css']))}`;
    const provider = new Gtk.CssProvider();
    const errors = [];
    provider.connect('parsing-error', (_, section, error) => errors.push(error.message));
    provider.load_from_string(css);
    if (errors.length) throw new Error(`Media stylesheet: ${errors.join('; ')}`);
    // The desktop's gtk-4.0/gtk.css is USER priority. Scope our independent
    // layer surface above it without changing themes for any other application.
    Gtk.StyleContext.add_provider_for_display(Gdk.Display.get_default(), provider, Gtk.STYLE_PROVIDER_PRIORITY_USER + 1);
    return { palette };
}

class Artwork {
    constructor(backend, size) {
        this.backend = backend;
        this.size = size;
        this.key = '';
        this.generation = 0;
        this.widget = new Gtk.Overlay({ width_request: size, height_request: size,
            halign: Gtk.Align.CENTER, valign: Gtk.Align.CENTER, css_classes: ['artwork'] });
        this.fallback = new Gtk.Label({ label: '󰝚', css_classes: [size > 40 ? 'art-glyph' : 'art-glyph-small'] });
        this.picture = new Gtk.Picture({ content_fit: Gtk.ContentFit.COVER, can_shrink: true, visible: false });
        this.widget.set_child(this.fallback);
        this.widget.add_overlay(this.picture);
    }

    set(cover, url, active = true) {
        const key = active ? JSON.stringify([cover, url]) : '';
        if (key === this.key) return;
        this.key = key;
        const generation = ++this.generation;
        this.cancel?.cancel();
        const cancel = this.cancel = new Gio.Cancellable();
        this.picture.set_paintable(null);
        this.picture.visible = false;
        if (!active) return;
        const load = async () => {
            if (cover) {
                try { await this.load(cover, generation); return; }
                catch (error) {
                    if (generation !== this.generation || cancel.is_cancelled()) return;
                    console.warn(`Media artwork ${cover}: ${error.message}`);
                }
            }
            // Remote artwork is cached by Astal; Rust only extracts local covers.
            if (!url || !(url.startsWith('file://') || url.startsWith('/'))) return;
            try {
                const path = await this.backend.resolveArtwork(url, cancel);
                if (generation !== this.generation) return;
                if (path) await this.load(path, generation);
            } catch (error) {
                if (generation === this.generation && !cancel.is_cancelled()) console.warn(`Media artwork: ${error.message}`);
            }
        };
        load();
    }

    async load(path, generation) {
        const cancel = this.cancel;
        const file = Gio.File.new_for_path(path);
        const info = await new Promise((resolve, reject) => file.query_info_async(
            'standard::size,standard::type', Gio.FileQueryInfoFlags.NONE, GLib.PRIORITY_DEFAULT, cancel,
            (source, result) => { try { resolve(source.query_info_finish(result)); } catch (error) { reject(error); } },
        ));
        if (info.get_file_type() !== Gio.FileType.REGULAR || info.get_size() > 16 * 1024 * 1024)
            throw new Error('Artwork is not a regular image smaller than 16 MiB.');
        const stream = await new Promise((resolve, reject) => file.read_async(GLib.PRIORITY_DEFAULT, cancel,
            (source, result) => { try { resolve(source.read_finish(result)); } catch (error) { reject(error); } }));
        try {
            const pixbuf = await new Promise((resolve, reject) => GdkPixbuf.Pixbuf.new_from_stream_at_scale_async(
                stream, this.size * 2, this.size * 2, true, cancel,
                (_, result) => { try { resolve(GdkPixbuf.Pixbuf.new_from_stream_finish(result)); } catch (error) { reject(error); } },
            ));
            if (generation !== this.generation) return;
            this.picture.set_paintable(Gdk.Texture.new_for_pixbuf(pixbuf));
            this.picture.visible = true;
        } finally {
            stream.close_async(GLib.PRIORITY_DEFAULT, null, null);
        }
    }

    dispose() {
        this.generation++;
        this.cancel?.cancel();
    }
}

class Credit {
    constructor(backend, update) {
        this.backend = backend;
        this.update = update;
        this.key = '';
        this.generation = 0;
    }

    set(player, active) {
        const data = player ? { url: player.url, artist: player.artist, album: player.album } : null;
        const key = active && data ? JSON.stringify(data) : '';
        if (key === this.key) return;
        this.key = key;
        const generation = ++this.generation;
        this.cancel?.cancel();
        this.cancel = new Gio.Cancellable();
        this.update('');
        if (!key) return;
        this.backend.resolveCredit(data, this.cancel).then(credit => {
            if (generation === this.generation) this.update(credit);
        }).catch(error => {
            if (generation === this.generation && !this.cancel.is_cancelled()) console.warn(`Media credit: ${error.message}`);
        });
    }

    dispose() {
        this.generation++;
        this.cancel?.cancel();
    }
}

// Card layout and interaction.

function box(vertical = false, spacing = 0, children = []) {
    const widget = new Gtk.Box({ orientation: vertical ? Gtk.Orientation.VERTICAL : Gtk.Orientation.HORIZONTAL, spacing });
    children.forEach(child => widget.append(child));
    return widget;
}
function label(text = '', classes = [], expand = false) {
    return new Gtk.Label({ label: text, css_classes: classes, hexpand: expand, xalign: 0, use_markup: false });
}
function ellipsis(text = '', classes = [], expand = true) {
    return new Gtk.Label({ label: text, css_classes: classes, hexpand: expand, xalign: 0,
        ellipsize: Pango.EllipsizeMode.END, max_width_chars: 1, use_markup: false });
}
function button(glyph, label, clicked, classes = []) {
    const widget = new Gtk.Button({ child: new Gtk.Label({ label: glyph, use_markup: false, xalign: 0.5 }),
        css_classes: ['icon-button', ...classes], valign: Gtk.Align.CENTER });
    widget.update_property([Gtk.AccessibleProperty.LABEL], [label]);
    widget.connect('clicked', clicked);
    return widget;
}
function scale(upper, classes = []) {
    return new Gtk.Scale({ orientation: Gtk.Orientation.HORIZONTAL, hexpand: true,
        draw_value: false, css_classes: classes, valign: Gtk.Align.CENTER,
        adjustment: new Gtk.Adjustment({ lower: 0, upper, step_increment: upper / 100, page_increment: upper / 20 }) });
}
function volumeBar(scale, beforeStep) {
    // Enabling the value label changes Gtk.Range rounding; retain 1% precision.
    const roundDigits = scale.round_digits;
    scale.draw_value = true;
    scale.round_digits = roundDigits;
    scale.value_pos = Gtk.PositionType.TOP;
    scale.add_css_class('volume-track');
    let available = true;
    const formatValue = (_, value) => available ? `${Math.round(value * 100)}%` : '—';
    scale.set_format_value_func(formatValue);
    const step = (glyph, label, direction) => {
        const widget = new Gtk.Button({ label: glyph,
            css_classes: ['volume-step'], valign: Gtk.Align.CENTER });
        widget.update_property([Gtk.AccessibleProperty.LABEL], [label]);
        widget.connect('clicked', () => {
            if (!scale.is_sensitive()) return;
            beforeStep();
            scale.emit('move-slider', direction);
        });
        return widget;
    };
    const widget = new Gtk.Box({ css_classes: ['volume-bar'], hexpand: true, valign: Gtk.Align.CENTER });
    widget.append(step('◁', 'Decrease volume', Gtk.ScrollType.STEP_BACKWARD));
    widget.append(scale);
    widget.append(step('▷', 'Increase volume', Gtk.ScrollType.STEP_FORWARD));
    return {
        widget,
        setAvailable(value) {
            widget.sensitive = scale.sensitive;
            const next = Boolean(value);
            if (next === available) return;
            available = next;
            // GTK caches formatted text even when the numeric value is unchanged.
            scale.set_format_value_func(formatValue);
        },
    };
}
function trackKey(player) {
    return player ? JSON.stringify([player.bus, player.instance, player.trackKey]) : '';
}

function frame(palette) {
    const drawing = new Gtk.DrawingArea({ can_target: false, hexpand: true, vexpand: true });
    drawing.set_draw_func((_, cr, width, height) => {
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
        const rgb = palette.yellow.slice(1).match(/../g).map(part => parseInt(part, 16) / 255);
        cr.setSourceRGBA(...rgb, 1);
        cr.setLineWidth(1);
        cr.stroke();
    });
    return drawing;
}

class MediaPanel {
    constructor({ app, backend, palette }) {
        this.backend = backend;

        this.opened = false;
        this.groups = [];
        this.activePlayer = '';
        this.failure = '';
        this.rows = new Map();
        this.seekDragging = false;
        this.seekPending = false;
        this.seekTarget = null;
        this.volumeDragging = false;
        this.volumeTarget = null;

        this.monitor = null;
        this.window = new Astal.Window({
            application: app, name: 'media-panel', namespace: 'astal:media',
            anchor: Astal.WindowAnchor.TOP | Astal.WindowAnchor.BOTTOM | Astal.WindowAnchor.LEFT | Astal.WindowAnchor.RIGHT,
            exclusivity: Astal.Exclusivity.IGNORE, layer: Astal.Layer.OVERLAY,
            keymode: Astal.Keymode.NONE, visible: false,
        });
        this.window.add_css_class('media-panel');
        this.window.connect('close-request', () => { this.close(); return true; });
        const keys = new Gtk.EventControllerKey();
        keys.connect('key-pressed', (_, key) => {
            if (key !== Gdk.KEY_Escape) return false;
            this.close();
            return true;
        });
        this.window.add_controller(keys);
        this.root = new Gtk.Overlay({ hexpand: true, vexpand: true });
        this.root.set_child(new Gtk.Box({ hexpand: true, vexpand: true }));
        this.window.set_child(this.root);
        this.card = new Gtk.Overlay({ width_request: 460, halign: Gtk.Align.CENTER, valign: Gtk.Align.START,
            margin_top: 40, css_classes: ['media-card'] });
        this.card.set_child(frame(palette));
        this.content = box(true, 12);
        this.content.add_css_class('media-content');
        this.card.add_overlay(this.content);
        this.card.set_measure_overlay(this.content, true);
        this.revealer = new Gtk.Revealer({ child: this.card, halign: Gtk.Align.CENTER, valign: Gtk.Align.START,
            transition_type: Gtk.RevealerTransitionType.CROSSFADE, transition_duration: 220 });
        this.revealer.connect('notify::child-revealed', () => {
            if (!this.opened && !this.revealer.child_revealed) this.window.visible = false;
        });
        this.root.add_overlay(this.revealer);
        const outside = new Gtk.GestureClick({ propagation_phase: Gtk.PropagationPhase.CAPTURE });
        outside.connect('released', (_, count, x, y) => {
            const [ok, bounds] = this.card.compute_bounds(this.root);
            if (ok && (x < bounds.get_x() || x > bounds.get_x() + bounds.get_width()
                || y < bounds.get_y() || y > bounds.get_y() + bounds.get_height())) this.close();
        });
        this.root.add_controller(outside);
        const release = new Gtk.EventControllerLegacy({ propagation_phase: Gtk.PropagationPhase.CAPTURE });
        release.connect('event', (_, event) => {
            const type = event.get_event_type();
            if (type === Gdk.EventType.BUTTON_RELEASE || type === Gdk.EventType.TOUCH_END)
                this.finishSeek(true);
            else if (type === Gdk.EventType.TOUCH_CANCEL)
                this.finishSeek(false);
            if ([Gdk.EventType.BUTTON_RELEASE, Gdk.EventType.TOUCH_END, Gdk.EventType.TOUCH_CANCEL].includes(type)) {
                this.volumeDragging = false;
                this.volumeTarget = null;
            }
            return false;
        });
        this.root.add_controller(release);
        this.build();
    }

    build() {
        this.empty = label('󰝚  NOTHING PLAYING', ['empty']);
        this.empty.xalign = 0.5;
        this.content.append(this.empty);
        this.details = box(true, 12);
        this.content.append(this.details);
        this.art = new Artwork(this.backend, 68);
        this.title = label('', ['title'], true);
        this.title.wrap = true;
        this.title.wrap_mode = Pango.WrapMode.WORD_CHAR;
        this.title.lines = 2;
        this.title.width_chars = 20;
        this.title.max_width_chars = 20;
        this.title.ellipsize = Pango.EllipsizeMode.END;
        this.creditLabel = ellipsis('', ['metadata']);
        this.credit = new Credit(this.backend, text => this.creditLabel.set_label(text));
        const metadata = box(true, 4, [this.title, this.creditLabel]);
        metadata.valign = Gtk.Align.CENTER;
        metadata.hexpand = true;
        this.details.append(box(false, 12, [this.art.widget, metadata]));
        this.details.append(new Gtk.Separator({ css_classes: ['divider'] }));
        this.seek = scale(1, ['seek']);
        this.seek.connect('change-value', (_, scroll, value) => {
            const player = this.group?.primary;
            const target = this.seekTarget || trackKey(player);
            if (player && target === trackKey(this.backend.player(player.bus)) && this.modelCurrent()) {
                this.elapsed.set_label(formatTime(value));
                if (this.seekDragging) this.seekPending = true;
                else this.backend.action('seek', player.bus, value);
            }
            return false;
        });
        this.drag(this.seek, () => {
            this.seekDragging = true;
            this.seekPending = false;
            this.seekTarget = trackKey(this.group?.primary);
        });
        this.elapsed = label('--:--', ['elapsed-time']);
        this.total = label('--:--', ['total-time']);
        this.total.xalign = 1;
        this.details.append(box(false, 6, [this.elapsed, this.seek, this.total]));
        this.previous = button('󰒮', 'Previous', () => this.action('previous'));
        this.play = button('󰐊', 'Play / pause', () => this.action('toggle'), ['play-button']);
        this.next = button('󰒭', 'Next', () => this.action('next'));
        const transport = box(false, 14, [this.previous, this.play, this.next]);
        this.volumeGlyph = label('󰕾', ['volume-glyph']);
        this.volumeButton = new Gtk.Button({ css_classes: ['volume-button'], valign: Gtk.Align.CENTER,
            child: this.volumeGlyph });
        this.volumeButton.update_property([Gtk.AccessibleProperty.LABEL], ['Mute / unmute']);
        this.volumeButton.connect('clicked', () => {
            if (!this.volumeControl?.toggleMute()) this.setFailure('Native mute is unavailable or the media source changed.');
        });
        this.volume = scale(1);
        this.volume.update_property([Gtk.AccessibleProperty.LABEL], ['Volume']);
        this.volume.width_request = 80;
        this.volume.connect('change-value', (_, scroll, value) => {
            const control = this.backend.volume(this.group);
            // Revalidate ownership on each motion without retaining a descriptor
            // whose metadata stamp can expire during a drag.
            if ((this.volumeTarget && this.volumeTarget !== control.key)
                || !control.enabled || !control.set(value)) {
                this.setFailure('The media source changed. Try the volume again.');
                return true;
            }
            return false;
        });
        this.drag(this.volume, () => { this.volumeDragging = true; this.volumeTarget = this.volumeControl?.key; });
        this.volumeBar = volumeBar(this.volume, () => { this.volumeDragging = false; this.volumeTarget = null; });
        this.details.append(box(false, 14, [transport, this.volumeButton, this.volumeBar.widget]));
        this.details.append(new Gtk.Separator({ css_classes: ['divider'] }));
        this.sourceName = ellipsis('', ['source-name']);
        this.chevron = label('󰅃', ['chevron']);
        this.section = new Gtk.Button({ css_classes: ['section-button'], child: box(false, 10, [
            label('󰐊', ['section-glyph']), label('MEDIA', ['section-label']), this.sourceName, this.chevron,
        ]) });
        this.section.connect('clicked', () => {
            this.sources.reveal_child = !this.sources.reveal_child;
            this.chevron.set_label(this.sources.reveal_child ? '󰅀' : '󰅃');
        });
        this.sourceRows = box(true, 2);
        const scroll = new Gtk.ScrolledWindow({ child: this.sourceRows, hscrollbar_policy: Gtk.PolicyType.NEVER,
            vscrollbar_policy: Gtk.PolicyType.AUTOMATIC, propagate_natural_height: true, max_content_height: 320 });
        this.sources = new Gtk.Revealer({ child: scroll, transition_type: Gtk.RevealerTransitionType.SLIDE_DOWN,
            transition_duration: 180, reveal_child: false, css_classes: ['sources'] });
        this.sources.connect('notify::reveal-child', () => this.renderRows(this.modelCurrent()));
        this.details.append(box(true, 0, [this.section, this.sources]));
        this.errorLabel = label('', ['failure']);
        this.errorLabel.wrap = true;
        this.errorLabel.max_width_chars = 52;
        this.content.append(this.errorLabel);
        this.render();
    }

    drag(widget, begin) {
        const controller = new Gtk.EventControllerLegacy({ propagation_phase: Gtk.PropagationPhase.CAPTURE });
        controller.connect('event', (_, event) => {
            if ([Gdk.EventType.BUTTON_PRESS, Gdk.EventType.TOUCH_BEGIN].includes(event.get_event_type())) begin();
            return false;
        });
        widget.add_controller(controller);
    }

    finishSeek(commit) {
        const player = this.group?.primary;
        if (commit && this.seekDragging && this.seekPending && player
            && this.seekTarget === trackKey(this.backend.player(player.bus)) && this.modelCurrent()) {
            this.backend.action('seek', player.bus, this.seek.get_value());
        }
        this.seekDragging = false;
        this.seekPending = false;
        this.seekTarget = null;
        if (!commit) this.position();
    }

    modelCurrent() { return this.backend.current(); }

    setModel(groups, activePlayer) {
        this.groups = groups;
        this.activePlayer = activePlayer;
        this.render();
    }

    setFailure(text) {
        this.failure = text;
        this.errorLabel.set_label(text);
        this.errorLabel.visible = Boolean(text);
    }

    reportFailure(text) {
        this.backend.reportFailure(text);
        this.setFailure(text);
    }

    action(kind, group = this.group) {
        if (!group) {
            this.reportFailure('No media source is selected.');
            return false;
        }
        if (!this.modelCurrent()) {
            this.reportFailure('The media source changed. Refreshing; try the action again.');
            this.backend.refresh();
            return false;
        }
        if (kind === 'toggle') {
            const transport = this.backend.transport(group);
            if (!transport.enabled) {
                this.reportFailure(transport.reason);
                return false;
            }
        }
        const queued = this.backend.action(kind, group.primary.bus);
        if (!queued) this.reportFailure(this.backend.failure || 'The media action was not queued. Try again.');
        return queued;
    }

    render() {
        this.group = this.groups.find(group => group.primary?.bus === this.activePlayer) || null;
        const player = this.group?.primary;
        const current = this.modelCurrent();
        this.empty.visible = !player;
        this.details.visible = Boolean(player);
        this.title.set_label(player?.title || 'Unknown title');
        this.title.tooltip_text = player?.title || '';
        this.credit.set(player, this.opened);
        this.art.set(player?.cover || '', player?.url || '', this.opened && Boolean(player));
        this.play.get_child().set_label(player?.playing ? '󰏤' : '󰐊');
        const transport = this.backend.transport(this.group);
        this.play.sensitive = current && transport.enabled;
        this.play.update_property([Gtk.AccessibleProperty.DESCRIPTION], [transport.reason]);
        this.previous.sensitive = current && Boolean(player?.canPrevious);
        this.next.sensitive = current && Boolean(player?.canNext);
        this.seek.sensitive = current && Boolean(player?.canSeek && player?.length);
        this.seek.adjustment.upper = player?.length || 1;
        this.seek.adjustment.step_increment = 1;
        this.seek.adjustment.page_increment = Math.max(1, (player?.length || 20) / 20);
        this.volumeControl = this.backend.volume(this.group);
        this.volume.sensitive = this.volumeControl.enabled;
        this.volumeBar.setAvailable(this.volumeControl.enabled);
        this.volumeButton.sensitive = this.volumeControl.muteEnabled;
        this.volume.update_property([Gtk.AccessibleProperty.DESCRIPTION], [this.volumeControl.reason]);
        this.volumeBar.widget.update_property([Gtk.AccessibleProperty.DESCRIPTION], [this.volumeControl.reason]);
        this.volumeButton.update_property([Gtk.AccessibleProperty.DESCRIPTION], [this.volumeControl.muteReason]);
        if (!this.volumeDragging) this.volume.set_value(this.volumeControl.value);
        const volume = this.volumeControl.value;
        this.volumeGlyph.set_label(this.volumeControl.muted || volume < 0.01 ? '󰝟' : volume < 0.5 ? '󰖀' : '󰕾');
        this.sourceName.set_label(this.group?.displayName || '');
        this.section.sensitive = this.groups.length > 1;
        if (this.groups.length < 2) { this.sources.reveal_child = false; this.chevron.set_label('󰅃'); }
        this.renderRows(current);
        this.setFailure(this.failure);
        this.position();
    }

    renderRows(current) {
        const wanted = new Set(this.groups.map(group => group.primary.bus));
        for (const [bus, row] of this.rows) {
            if (!wanted.has(bus)) {
                row.art.dispose();
                this.sourceRows.remove(row.widget);
                this.rows.delete(bus);
            }
        }
        let previous = null;
        for (const group of this.groups) {
            const bus = group.primary.bus;
            let row = this.rows.get(bus);
            if (!row) {
                row = { group };
                row.art = new Artwork(this.backend, 34);
                row.name = ellipsis('', ['source-name']);
                row.title = ellipsis('', ['source-title']);
                row.play = button('󰐊', 'Play / pause this source', () => this.action('toggle', row.group), ['source-play']);
                const select = new Gtk.Button({ css_classes: ['source-select'], hexpand: true,
                    child: box(false, 8, [row.art.widget, box(true, 1, [row.name, row.title])]) });
                select.connect('clicked', () => {
                    if (!this.modelCurrent()) {
                        this.reportFailure('The media source changed. Refreshing; select it again.');
                        this.backend.refresh();
                        return;
                    }
                    if (!this.backend.select(row.group.primary.bus)) {
                        this.reportFailure(this.backend.failure || 'The media source could not be selected.');
                        return;
                    }
                    this.reportFailure('');
                    this.sources.reveal_child = false;
                    this.chevron.set_label('󰅃');
                });
                row.widget = box(false, 8, [row.play, select]);
                row.widget.add_css_class('source-row');
                this.sourceRows.append(row.widget);
                this.rows.set(bus, row);
            }
            row.group = group;
            row.name.set_label(group.displayName);
            row.title.set_label(group.title);
            row.widget.update_property([Gtk.AccessibleProperty.DESCRIPTION], [`${group.displayName}\n${group.title}`]);
            row.play.get_child().set_label(group.primary.playing ? '󰏤' : '󰐊');
            const transport = this.backend.transport(group);
            row.play.sensitive = current && transport.enabled;
            row.play.update_property([Gtk.AccessibleProperty.DESCRIPTION], [transport.reason]);
            if (bus === this.activePlayer) row.widget.add_css_class('current');
            else row.widget.remove_css_class('current');
            row.art.set(group.primary.cover, group.primary.url, this.opened && this.sources.reveal_child);
            this.sourceRows.reorder_child_after(row.widget, previous);
            previous = row.widget;
        }
    }

    position() {
        const player = this.group && this.backend.player(this.group.primary.bus);
        if (!this.seekDragging) this.seek.set_value(player?.position || 0);
        this.elapsed.set_label(formatTime(this.seekDragging ? this.seek.get_value()
            : player?.positionSupported ? player.position : null));
        this.total.set_label(formatTime(player?.length));
    }

    toggle(monitor) {
        if (this.opened && monitor === this.monitor) return this.close();
        this.monitor = monitor;
        if (monitor) this.window.gdkmonitor = monitor;
        const width = monitor?.get_geometry().width || 492;
        this.card.width_request = Math.min(460, Math.max(240, width - 32));
        this.opened = true;
        this.window.keymode = Astal.Keymode.EXCLUSIVE;
        this.window.visible = true;
        closeNetwork();
        this.backend.refresh();
        this.revealer.reveal_child = true;
        if (!this.positionTimer) this.positionTimer = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 1000, () => {
            this.position();
            return GLib.SOURCE_CONTINUE;
        });
        this.render();
    }

    close() {
        if (!this.opened) return;
        this.opened = false;
        if (this.positionTimer) GLib.source_remove(this.positionTimer);
        this.positionTimer = 0;
        this.window.keymode = Astal.Keymode.NONE;
        this.seekDragging = this.volumeDragging = false;
        this.seekPending = false;
        this.seekTarget = this.volumeTarget = null;
        this.sources.reveal_child = false;
        this.chevron.set_label('󰅃');
        this.revealer.reveal_child = false;
        this.render();
    }


    dispose() {
        if (this.positionTimer) GLib.source_remove(this.positionTimer);
        this.positionTimer = 0;
        this.credit.dispose();
        this.art.dispose();
        this.rows.forEach(row => row.art.dispose());
        this.window.destroy();
    }
}

// Application entry and UI-facing IPC.
let backend, panel;

function render() {
    if (!panel) return;
    panel.setModel(backend.resolveGroups(), backend.activePlayer);
    panel.setFailure(backend.failure);
}

function monitorFor(name) {
    return App.get_monitors().find(monitor => !name || monitor.get_connector() === name) || null;
}

function handleRequest(message) {
    if (!message || typeof message !== 'object' || Array.isArray(message))
        throw new Error('A media panel command object is required.');
    if (!backend || !panel) throw new Error('Media panel is starting.');
    switch (message.command) {
    case 'status': return backend.status();
    case 'toggle': {
        if (message.monitor !== undefined && typeof message.monitor !== 'string')
            throw new Error('Monitor must be a connector name.');
        const monitor = monitorFor(message.monitor);
        if (!monitor) throw new Error('The requested monitor is no longer connected.');
        panel.toggle(monitor);
        break;
    }
    case 'close': panel.close(); break;
    case 'pauseAll': backend.action('pauseAll'); break;
    case 'togglePauseAll': backend.action('togglePauseAll'); break;
    case 'cycleMedia': backend.cycle(); break;
    default: throw new Error('Unknown media panel command.');
    }
    return 'ok';
}

function closeNetwork() {
    try {
        AstalIO.Process.exec_asyncv([GLib.getenv('NETWORK_PANEL') || 'network-panel',
            'close'], (_, result) => {
            try { AstalIO.Process.exec_asyncv_finish(result); }
            catch (error) { console.warn(`Could not dismiss the network panel: ${error.message}`); }
        });
    } catch (error) { console.warn(`Could not dismiss the network panel: ${error.message}`); }
}

const MediaApplication = GObject.registerClass(class MediaApplication extends Astal.Application {
    vfunc_request(request, connection) {
        let response;
        try { response = handleRequest(JSON.parse(request)); }
        catch (error) { response = JSON.stringify({ error: error.message }); }
        const close = () => {
            try { connection.close(null); }
            catch (error) { console.warn(`Media request close: ${error.message}`); }
        };
        try {
            // Native request API exposes the connection, so its owner can close
            // it after Astal finishes writing instead of relying on GJS GC.
            AstalIO.write_sock(connection, response, (_, result) => {
                try { AstalIO.write_sock_finish(result); }
                catch (error) { console.warn(`Media request reply: ${error.message}`); }
                finally { close(); }
            });
        } catch (error) {
            close();
            console.warn(`Media request reply: ${error.message}`);
        }
    }
});

Gtk.init();
// AGS preloads GTK4 layer-shell; do not inherit that preload in Rust helpers.
GLib.unsetenv('LD_PRELOAD');
const App = new MediaApplication({ instance_name: 'media-panel' });
let initialized = false, monitorSignal = 0;
function disposeApplication() {
    if (monitorSignal) App.disconnect(monitorSignal);
    monitorSignal = 0;
    try { panel?.dispose(); }
    finally {
        panel = null;
        backend?.dispose();
        backend = null;
    }
}
App.connect('shutdown', disposeApplication);
App.connect('activate', () => {
    if (initialized) return;
    try {
        const { palette } = loadTheme(GLib.getenv('MEDIA_PANEL_CONFIG'), GLib.getenv('MEDIA_PANEL_PALETTE'));
        backend = new MediaBackend({ onChange: render });
        panel = new MediaPanel({ app: App, backend, palette });
        backend.start();
        monitorSignal = App.connect('notify::monitors', () => {
            if (panel.monitor && !monitorFor(panel.monitor.get_connector())) panel.close();
        });
        initialized = true;
    } catch (error) {
        console.error(`Media panel initialization: ${error.message}`);
        try { disposeApplication(); }
        finally { try { App.quit(); } finally { exit(1); } }
    }
});
try { App.acquire_socket(); }
catch (error) { console.error(`Media panel startup: ${error.message}`); exit(1); }
App.hold();
await App.runAsync([]);
