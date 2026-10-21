const COMMANDS = new Set(['status', 'toggle', 'close', 'bluetoothPower']);

export function parseRequest(raw) {
    const request = JSON.parse(raw);
    if (!request || typeof request !== 'object' || Array.isArray(request)
        || !COMMANDS.has(request.command)) throw new Error('Unknown audio panel command.');
    if (Object.keys(request).some(key => !['command', 'monitor'].includes(key)))
        throw new Error('Unexpected audio panel request field.');
    if (request.monitor !== undefined && (request.command !== 'toggle'
        || typeof request.monitor !== 'string' || request.monitor.length > 256 || /[\x00-\x1f]/.test(request.monitor)))
        throw new Error('Monitor must be a connector name.');
    return request;
}

export function volumeValue(kind, value) {
    if (!['output', 'input'].includes(kind) || !Number.isFinite(value))
        throw new Error('Invalid audio volume.');
    return Math.min(kind === 'output' ? 1.5 : 1, Math.max(0, value));
}

const nativeId = value => Number.isInteger(value) && value > 0 && value <= 0xffffffff;
const index = value => Number.isInteger(value) && value >= 0 && value <= 0x7fffffff;
const serial = value => typeof value === 'string' && /^\d{1,20}$/.test(value);
const text = value => typeof value === 'string' && value.length <= 8192 && !value.includes('\0');
const identity = value => value && typeof value === 'object' && !Array.isArray(value)
    && Object.keys(value).length === 2 && nativeId(value.id) && serial(value.serial);

export function parseChoices(raw) {
    const entries = typeof raw === 'string' ? JSON.parse(raw) : raw;
    if (!Array.isArray(entries) || entries.length > 4096) throw new Error('Invalid audio device list.');
    const keys = new Set();
    return entries.map(entry => {
        if (!entry || typeof entry !== 'object' || Array.isArray(entry)
            || ['key', 'label', 'description', 'name', 'portName', 'card'].some(key => !text(entry[key]))
            || !entry.key || keys.has(entry.key) || typeof entry.portActive !== 'boolean'
            || typeof entry.port !== 'boolean' || entry.port !== Boolean(entry.portName)
            || !(entry.nodeId === null ? entry.nodeSerial === '' : nativeId(entry.nodeId) && serial(entry.nodeSerial))
            || !(entry.cardId === null ? entry.cardSerial === '' : nativeId(entry.cardId) && serial(entry.cardSerial))
            || !(entry.routeId === null ? !entry.port : index(entry.routeId) && entry.port)
            || (entry.nodeId === null && (!entry.port || entry.cardId === null)))
            throw new Error('Invalid audio device entry.');
        keys.add(entry.key);
        return { key: entry.key, label: entry.label, description: entry.description, name: entry.name,
            portName: entry.portName, card: entry.card, port: entry.port, portActive: entry.portActive,
            nodeId: entry.nodeId, nodeSerial: entry.nodeSerial, cardId: entry.cardId,
            cardSerial: entry.cardSerial, routeId: entry.routeId };
    });
}

export function policyEvent(raw) {
    const event = JSON.parse(raw);
    if (!event || typeof event !== 'object' || Array.isArray(event)) throw new Error('Invalid audio policy message.');
    let fields;
    switch (event.type) {
    case 'result':
        fields = event.rows !== undefined ? ['type', 'rows'] : event.ok === true ? ['type', 'ok'] : ['type', 'ok', 'error'];
        if (event.rows !== undefined) {
            if (!Array.isArray(event.rows)) throw new Error('Invalid audio device list.');
            parseChoices(event.rows);
        }
        else if (event.ok !== true && !(event.ok === false && text(event.error) && event.error))
            throw new Error('Invalid audio policy result.');
        break;
    case 'snapshot': fields = ['type', 'id']; break;
    case 'action':
        fields = ['type', 'id', 'operation'];
        if (event.operation === 'profile' && identity(event.card) && index(event.profile)) fields.push('card', 'profile');
        else if (event.operation === 'route' && identity(event.node) && index(event.route)) fields.push('node', 'route');
        else if (event.operation === 'default' && identity(event.node)) fields.push('node');
        else throw new Error('Invalid native audio action.');
        break;
    default: throw new Error('Unknown audio policy message.');
    }
    if (Object.keys(event).some(key => !fields.includes(key)) || fields.some(key => !(key in event))
        || (event.type !== 'result' && (!Number.isSafeInteger(event.id) || event.id < 1 || event.id > 256)))
        throw new Error('Invalid audio policy message fields.');
    return event;
}

// Missing associations need policy, not a second implementation of native writes.
// Ambiguous/stale native targets fail instead of falling back to another backend.
export function selectionPlan(entry, endpoints) {
    if (entry.nodeId === null && entry.port && entry.cardId !== null) return { type: 'policy', key: entry.key };
    const matches = endpoints.filter(endpoint => endpoint.id === entry.nodeId && endpoint.serial === entry.nodeSerial);
    if (matches.length !== 1) throw new Error('The selected audio device disappeared or was replaced.');
    const endpoint = matches[0];
    if (!entry.port) return { type: 'native', nodeKey: endpoint.key, route: null };
    const routes = endpoint.routes.filter(route => route.index === entry.routeId && route.name === entry.portName && route.available);
    if (routes.length !== 1) throw new Error('The selected audio port is no longer available.');
    if (endpoint.routesReliable === false) return { type: 'policy', key: entry.key };
    return { type: 'native', nodeKey: endpoint.key, route: routes[0].index };
}

export function deviceChoices(entries, endpoints, cards = []) {
    const choices = [];
    for (const endpoint of endpoints) {
        const rows = entries.filter(entry => entry.nodeId === endpoint.id && entry.nodeSerial === endpoint.serial
            && entry.name === endpoint.name && entry.cardId === endpoint.cardId && entry.cardSerial === endpoint.cardSerial
            && (!entry.port || endpoint.routes.some(route => route.index === entry.routeId
                && route.name === entry.portName && route.available)));
        for (const row of rows) choices.push({ ...row, default: Boolean(endpoint.isDefault
            && (!row.port || (endpoint.routesReliable ? endpoint.routeName === row.portName : row.portActive))) });
        if (rows.length) continue;
        choices.push({ key: `native:${endpoint.key}`, label: endpoint.description || endpoint.name,
            description: endpoint.description || endpoint.name, name: endpoint.name, portName: '', card: '',
            port: false, portActive: false, nodeId: endpoint.id, nodeSerial: endpoint.serial,
            cardId: endpoint.cardId, cardSerial: endpoint.cardSerial, routeId: null, default: Boolean(endpoint.isDefault) });
    }
    for (const entry of entries) {
        if (entry.nodeId === null && cards.some(card => card.id === entry.cardId && card.serial === entry.cardSerial))
            choices.push({ ...entry, default: false });
    }
    // Native objects decide which rows exist; retain Rust's presentation order
    // so profile changes do not move the inactive choices to the end of the list.
    const order = new Map(entries.map((entry, index) => [entry.key, index]));
    return choices.sort((left, right) => (order.get(left.key) ?? entries.length) - (order.get(right.key) ?? entries.length));
}

export function bluetoothDevices(devices) {
    const rank = device => device.connected ? 0 : device.paired ? 1 : 2;
    return [...devices].sort((a, b) => rank(a) - rank(b) || a.name.localeCompare(b.name));
}

export function pairingAnswer(text) {
    if (typeof text !== 'string' || !text.trim() || text.length > 512 || /[\r\n\0]/.test(text))
        throw new Error('Enter one PIN/passkey or yes/no answer.');
    return `${text}\n`;
}

export function waybarStatus({ output, input, bluetooth }) {
    const percent = node => Math.round(Math.max(0, node.volume || 0) * 100);
    const deviceLine = (label, node) => !node.available ? `${label}: none`
        : `${label}: ${node.statusName || node.name} (${percent(node)}%${node.muted ? ', muted' : ''})`;
    const connected = bluetooth.devices.filter(device => device.connected).map(device => device.name || device.address).sort();
    const powered = Boolean(bluetooth.available && bluetooth.powered);
    const glyph = !output.available || output.muted ? '󰝟' : percent(output) === 0 ? '󰕿' : percent(output) <= 50 ? '󰖀' : '󰕾';
    return JSON.stringify({
        text: `<span size='large'>${glyph}</span>`,
        tooltip: [deviceLine('Output', output), deviceLine('Input', input), !powered ? 'Bluetooth: off'
            : connected.length ? `Bluetooth: ${connected.join(', ')}` : 'Bluetooth: on, nothing connected'].join('\n'),
        class: powered ? connected.length ? 'bluetooth-connected' : 'bluetooth-on'
            : !output.available ? 'unavailable' : output.muted ? 'muted' : 'active',
    });
}
