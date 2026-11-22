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

// Structural transport validation only. Guards carry Rust-selected raw field
// values; interpreting their domain meaning belongs exclusively to Rust.
const object = value => value && typeof value === 'object' && !Array.isArray(value);
const operations = new Set(['retain', 'release', 'profile', 'route', 'default', 'volume', 'mute', 'follow',
    'power', 'startDiscovery', 'stopDiscovery', 'trust', 'connect', 'disconnect', 'forget', 'cancelPairing']);
export function nativeEvent(raw) {
    const event = JSON.parse(raw);
    if (!object(event)) throw new Error('Invalid audio policy message.');
    let fields;
    if (event.type === 'closed') fields = ['type'];
    else if (event.type === 'model' && object(event.model)
        && ['output', 'input', 'bluetooth'].every(key => object(event.model[key]))) fields = ['type', 'model'];
    else if (['snapshot', 'action'].includes(event.type) && Number.isSafeInteger(event.id) && event.id > 0) {
        fields = ['type', 'id'];
        if (event.type === 'action') {
            fields.push('operation', 'target', 'args', 'guards', 'cleanup');
            if (!operations.has(event.operation) || typeof event.target !== 'string' || !event.target || !object(event.args)
                || typeof event.cleanup !== 'boolean' || !Array.isArray(event.guards) || event.guards.length > 64
                || event.guards.some(guard => !object(guard) || !(guard.key === null || typeof guard.key === 'string')
                    || !object(guard.expect) || Object.keys(guard).some(key => !['key', 'expect', 'held'].includes(key))
                    || (guard.held !== undefined && typeof guard.held !== 'boolean')))
                throw new Error('Invalid guarded native action.');
        }
    } else throw new Error('Unknown audio policy message.');
    if (Object.keys(event).some(key => !fields.includes(key)) || fields.some(key => !(key in event)))
        throw new Error('Invalid audio policy message fields.');
    return event;
}

export function rawEqual(left, right) {
    if (left === right) return true;
    if (Array.isArray(left) && Array.isArray(right))
        return left.length === right.length && left.every((value, index) => rawEqual(value, right[index]));
    if (!object(left) || !object(right)) return false;
    const keys = Object.keys(left);
    return keys.length === Object.keys(right).length && keys.every(key => Object.hasOwn(right, key) && rawEqual(left[key], right[key]));
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
