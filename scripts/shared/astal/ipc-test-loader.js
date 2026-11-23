// Node simulations execute the real shared module with each fixture's GI doubles.
import { readFileSync } from 'node:fs';

const source = readFileSync(new URL('./ipc.js', import.meta.url), 'utf8')
    .replace(/^import .*;\r?\n/gm, '').replace(/^export /gm, '');
const factory = new Function('GLib', 'Gio', `${source}\nreturn { LineDecoder, writeBytes, closeStream, communicate };`);

export function loadIPC(GLib = {}, Gio = {}) { return factory(GLib, Gio); }
