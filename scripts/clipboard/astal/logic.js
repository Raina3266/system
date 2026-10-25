export const TABS = ['memo', 'text', 'file'];
export const isDraft = item => item?.kind === 'memo' && !item.text;
export const editable = item => item && ['memo', 'text'].includes(item.kind);
export const localFilePath = item => item?.kind === 'file' && typeof item.name === 'string'
    && item.name.startsWith('/') && !item.name.includes('\0') ? item.name : null;

export function title(item, home = '') {
    if (!item) return 'No item selected';
    if (isDraft(item)) return 'New memo';
    let text = item.kind === 'file'
        ? item.name || item.text || `Image · ${item.mime}`
        : item.text || '';
    if (home && (text === home || text.startsWith(`${home}/`))) text = `~${text.slice(home.length)}`;
    // Only the row preview is bounded; search, editing and copying use the full
    // value. Stop early instead of normalizing a potentially enormous note.
    const preview = [];
    for (const character of text) {
        if (/\s/u.test(character)) {
            if (preview.length && preview.at(-1) !== ' ') preview.push(' ');
        } else preview.push(character);
        if (preview.length > 160) return `${preview.slice(0, 160).join('').trimEnd()}…`;
    }
    return preview.join('').trimEnd() || 'Empty item';
}

export function visibleItems(items, tab, query) {
    const words = query.toLocaleLowerCase().trim().split(/\s+/).filter(Boolean);
    return items.filter(item => {
        if (item.kind !== tab) return false;
        if (!words.length || isDraft(item)) return true;
        const text = `${item.text || ''} ${item.name || ''} ${item.mime}`.toLocaleLowerCase();
        return words.every(word => text.includes(word));
    })
        .sort((a, b) => Number(isDraft(a)) - Number(isDraft(b)) || Number(b.pinned) - Number(a.pinned));
}

export function preferred(items, id) {
    return items.find(item => item.id === id) || items.find(item => !item.pinned) || items[0] || null;
}

// One serialized user transaction at a time. File-watch refreshes never replace
// dirty editor contents. Saves compare the original text under the Rust flock.
export class ClipboardModel {
    constructor({ request, publish, scheduleAutosave = null,
        trashFile = async () => { throw new Error('Local file deletion is unavailable.'); }, onChange = () => {} }) {
        Object.assign(this, { request, publish, trashFile, onChange, scheduleAutosave });
        this.cancelScheduledSave = null;
        this.pendingAutosave = false;
        this.items = [];
        this.tab = 'memo';
        this.query = '';
        this.editor = null;
        this.activity = null;
        this.failure = '';
        this.pendingRefresh = false;
        this.pendingPrune = false;
    }
    get rows() {
        const cache = this.rowCache;
        if (!cache || cache.items !== this.items || cache.tab !== this.tab || cache.query !== this.query)
            this.rowCache = { items: this.items, tab: this.tab, query: this.query,
                rows: visibleItems(this.items, this.tab, this.query) };
        return this.rowCache.rows;
    }
    get dirty() { return editable(this.editor) && this.editor.text !== this.editor.base; }
    get busy() { return this.activity !== null; }
    changed() { this.onChange(); }
    async act(work, { background = false, autosave = false } = {}) {
        if (this.busy) return false;
        this.activity = background ? 'refresh' : autosave ? 'save' : 'action';
        if (!background) this.failure = '';
        this.changed();
        try {
            await work();
            while (this.pendingRefresh) {
                const pruneMissing = this.pendingPrune;
                this.pendingRefresh = false;
                this.pendingPrune = false;
                await this.load({ pruneMissing });
            }
            return true;
        } catch (error) {
            this.failure = error.message;
            return false;
        } finally {
            this.activity = null;
            this.changed();
            if (this.pendingAutosave) this.queueAutosave();
        }
    }
    setEditor(item) {
        this.cancelAutosave();
        this.editor = item ? { ...item, base: item.text || '', text: item.text || '' } : null;
    }
    requireItem(id) {
        const item = this.items.find(item => item.id === id);
        if (!item) throw new Error('This clipboard item no longer exists.');
        return item;
    }
    setItems(items, { selectedId = this.editor?.id, discardDraft = false } = {}) {
        this.items = items;
        if (discardDraft || !this.dirty)
            this.setEditor(selectedId == null ? null
                : items.find(item => item.id === selectedId) || preferred(this.rows));
    }
    async load({ pruneMissing = false } = {}) {
        const { items } = await this.request({ command: pruneMissing ? 'prepare' : 'snapshot' });
        this.setItems(items);
    }
    async refresh({ pruneMissing = false } = {}) {
        if (this.busy) {
            this.pendingRefresh = true;
            this.pendingPrune ||= pruneMissing;
            return false;
        }
        return this.act(() => this.load({ pruneMissing }), { background: true });
    }
    async open() {
        return this.act(async () => {
            await this.commit();
            this.tab = 'memo';
            this.setEditor(null);
            this.changed();
            this.setItems((await this.request({ command: 'prepare' })).items);
        });
    }
    edit(text) {
        // A read-only file-watch refresh must not disable typing or lose a
        // keystroke. load() rechecks dirty state after its asynchronous read.
        if (this.activity === 'action' || !editable(this.editor)) return;
        this.editor.text = text;
        this.queueAutosave();
        this.changed();
    }
    cancelAutosave() {
        this.cancelScheduledSave?.();
        this.cancelScheduledSave = null;
        this.pendingAutosave = false;
    }
    queueAutosave() {
        this.cancelAutosave();
        if (!this.scheduleAutosave || !this.dirty) return;
        this.cancelScheduledSave = this.scheduleAutosave(() => {
            this.cancelScheduledSave = null;
            if (this.busy) {
                this.pendingAutosave = true;
                return;
            }
            return this.autosave();
        });
    }
    async autosave() {
        if (!this.dirty) return true;
        return this.act(async () => {
            try { await this.commit(); }
            catch (error) {
                this.cancelAutosave();
                throw error;
            }
        }, { autosave: true });
    }
    dispose() {
        this.scheduleAutosave = null;
        this.cancelAutosave();
    }
    async commit() {
        this.cancelAutosave();
        if (!this.dirty) return;
        const editor = this.editor;
        const { id, base, text } = editor;
        const reply = await this.request({ command: 'save', id, base, text });
        // Typing stays enabled during autosave; only advance the saved baseline,
        // never replace text entered while the request was in flight.
        editor.base = text;
        this.setItems(reply.items, { selectedId: id });
        if (this.dirty && !this.cancelScheduledSave && !this.pendingAutosave) this.queueAutosave();
    }
    async save() {
        return this.act(() => this.commit());
    }
    async select(id) {
        if (id === this.editor?.id) return true;
        return this.act(async () => {
            await this.commit();
            const item = this.items.find(item => item.id === id);
            if (!item) throw new Error('This clipboard item no longer exists.');
            this.setEditor(item);
        });
    }
    filter(query) {
        // Filtering does not switch the editor, so typing stays synchronous
        // and cannot race a slow helper or discard an in-progress draft.
        if (query !== this.query) {
            this.query = query;
            this.changed();
        }
        return true;
    }
    async changeTab(tab) {
        if (!TABS.includes(tab)) return false;
        if (tab === this.tab) return !this.busy;
        return this.act(async () => {
            await this.commit();
            if (tab === 'memo' && !this.items.some(isDraft))
                this.setItems((await this.request({ command: 'memo' })).items);
            this.tab = tab;
            this.setEditor(preferred(this.rows));
        });
    }
    async newMemo() {
        return this.act(async () => {
            await this.commit();
            const { id, items } = await this.request({ command: 'memo' });
            this.tab = 'memo';
            this.query = '';
            this.setItems(items, { selectedId: id });
        });
    }
    async pin(id = this.editor?.id) {
        if (id == null) return false;
        return this.act(async () => {
            this.requireItem(id);
            await this.commit();
            this.requireItem(id);
            this.setItems((await this.request({ command: 'pin', id })).items);
        });
    }
    async remove(id = this.editor?.id) {
        if (id == null) return false;
        return this.act(async () => {
            const item = this.requireItem(id);
            const selected = id === this.editor?.id;
            const path = localFilePath(item);
            let trashed = false;
            if (path) {
                // Recheck the persisted association before touching a local file.
                const snapshot = await this.request({ command: 'snapshot' });
                if (localFilePath(snapshot.items.find(item => item.id === id)) !== path)
                    throw new Error('This file changed in clipboard history. Close and reopen the panel before deleting it.');
                trashed = await this.trashFile(path);
            }
            // Only deleting the selected row explicitly discards its draft.
            const rows = this.rows, index = rows.findIndex(item => item.id === id);
            const replacement = rows[index + 1] || rows[index - 1];
            try {
                const { items } = await this.request({ command: 'delete', id });
                if (selected)
                    this.setItems(items, { selectedId: replacement?.id, discardDraft: true });
                else this.items = items;
            } catch (error) {
                if (trashed) throw new Error('The file was moved to Trash, but its clipboard entry could not be removed. Close and reopen the panel, then try again.');
                throw error;
            }
        });
    }

    async copy(id = this.editor?.id) {
        if (id == null) return false;
        return this.act(async () => {
            this.requireItem(id);
            await this.commit();
            this.setEditor(this.requireItem(id));
            await this.publish(await this.request({ command: 'payload', id }));
        });
    }
}
