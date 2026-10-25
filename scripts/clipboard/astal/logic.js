export const TABS = ['memo', 'text', 'file'];
export const isDraft = item => item?.kind === 'memo' && !item.text;
export const editable = item => item && ['memo', 'text'].includes(item.kind);

export function title(item, home = '') {
    if (!item) return 'No item selected';
    if (isDraft(item)) return 'New memo';
    let text = item.kind === 'file'
        ? item.name || item.text || `Image · ${item.mime}`
        : item.text || '';
    if (home && (text === home || text.startsWith(`${home}/`))) text = `~${text.slice(home.length)}`;
    return text.replace(/\s+/g, ' ').trim() || 'Empty item';
}

export function visibleItems(items, tab, query) {
    const words = query.toLocaleLowerCase().trim().split(/\s+/).filter(Boolean);
    return items.filter(item => item.kind === tab && (isDraft(item)
        || words.every(word => `${item.text || ''} ${item.name || ''} ${item.mime}`.toLocaleLowerCase().includes(word))))
        .sort((a, b) => Number(isDraft(a)) - Number(isDraft(b)) || Number(b.pinned) - Number(a.pinned));
}

export function preferred(items, id) {
    return items.find(item => item.id === id) || items.find(item => !item.pinned) || items[0] || null;
}

// One serialized user transaction at a time. File-watch refreshes never replace
// dirty editor contents. Saves compare the original text under the Rust flock.
export class ClipboardModel {
    constructor({ request, publish, onChange = () => {} }) {
        Object.assign(this, { request, publish, onChange });
        this.items = [];
        this.tab = 'memo';
        this.query = '';
        this.editor = null;
        this.activity = null;
        this.failure = '';
        this.pendingRefresh = false;
    }
    get rows() { return visibleItems(this.items, this.tab, this.query); }
    get selected() { return this.items.find(item => item.id === this.editor?.id) || this.editor; }
    get dirty() { return editable(this.editor) && this.editor.text !== this.editor.base; }
    get busy() { return this.activity !== null; }
    changed() { this.onChange(); }
    async act(work, { background = false } = {}) {
        if (this.busy) return false;
        this.activity = background ? 'refresh' : 'action';
        if (!background) this.failure = '';
        this.changed();
        try {
            await work();
            if (this.pendingRefresh) {
                this.pendingRefresh = false;
                await this.load();
            }
            return true;
        } catch (error) {
            this.failure = error.message;
            return false;
        } finally {
            this.activity = null;
            this.changed();
        }
    }
    setEditor(item) {
        this.editor = item ? { ...item, base: item.text || '', text: item.text || '' } : null;
    }
    async load() {
        const { items } = await this.request({ command: 'snapshot' });
        this.items = items;
        if (!this.dirty) this.setEditor(preferred(this.rows, this.editor?.id));
    }
    async refresh() {
        if (this.busy) { this.pendingRefresh = true; return false; }
        return this.act(() => this.load(), { background: true });
    }
    async open() {
        return this.act(async () => {
            await this.request({ command: 'prepare' });
            if (this.tab === 'memo') await this.request({ command: 'memo' });
            await this.load();
        });
    }
    edit(text) {
        // A read-only file-watch refresh must not disable typing or lose a
        // keystroke. load() rechecks dirty state after its asynchronous read.
        if (this.activity === 'action' || !editable(this.editor)) return;
        this.editor.text = text;
        this.changed();
    }
    async commit() {
        if (!this.dirty) return;
        const { id, base, text } = this.editor;
        await this.request({ command: 'save', id, base, text });
        this.editor.base = text;
    }
    async save() {
        return this.act(async () => { await this.commit(); await this.load(); });
    }
    async select(id) {
        return this.act(async () => {
            await this.commit();
            await this.load();
            const item = this.items.find(item => item.id === id);
            if (!item) throw new Error('This clipboard item no longer exists.');
            this.setEditor(item);
        });
    }
    filter(query) {
        // Filtering does not switch the editor, so typing stays synchronous
        // and cannot race a slow helper or discard an in-progress draft.
        this.query = query;
        this.changed();
        return true;
    }
    async changeTab(tab) {
        if (!TABS.includes(tab)) return false;
        return this.act(async () => {
            await this.commit();
            this.tab = tab;
            if (tab === 'memo') await this.request({ command: 'memo' });
            await this.load();
        });
    }
    async newMemo() {
        return this.act(async () => {
            await this.commit();
            const { id } = await this.request({ command: 'memo' });
            this.tab = 'memo';
            this.query = '';
            await this.load();
            this.setEditor(this.items.find(item => item.id === id));
        });
    }
    async pin() {
        const id = this.editor?.id;
        if (!id) return false;
        return this.act(async () => {
            await this.commit();
            await this.request({ command: 'pin', id });
            await this.load();
        });
    }
    async remove() {
        const id = this.editor?.id;
        if (!id) return false;
        return this.act(async () => {
            // Delete is the explicit discard action; do not save this draft.
            const index = this.rows.findIndex(item => item.id === id);
            const replacement = this.rows[index + 1] || this.rows[index - 1];
            await this.request({ command: 'delete', id });
            this.editor = null;
            if (this.tab === 'memo') await this.request({ command: 'memo' });
            await this.load();
            this.setEditor(preferred(this.rows, replacement?.id));
        });
    }
    async reload() {
        return this.act(async () => {
            // Fetch first: a read failure must not discard the unsaved draft.
            const { items } = await this.request({ command: 'snapshot' });
            this.items = items;
            this.setEditor(preferred(this.rows, this.editor?.id));
        });
    }
    async copy() {
        const id = this.editor?.id;
        if (!id) return false;
        return this.act(async () => {
            await this.commit();
            await this.publish(await this.request({ command: 'payload', id }));
        });
    }
}
