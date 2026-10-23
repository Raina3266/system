import AstalNetwork from 'gi://AstalNetwork?version=0.1';
import NM from 'gi://NM?version=1.0';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import GObject from 'gi://GObject';

const WIFI = NM.SETTING_WIRELESS_SETTING_NAME;
const WIRED = NM.SETTING_WIRED_SETTING_NAME;
const SECURITY = NM.SETTING_WIRELESS_SECURITY_SETTING_NAME;
const FLAGS = NM['80211ApSecurityFlags'];
const MODES = NM['80211Mode'];
const PRIORITY = GLib.PRIORITY_DEFAULT;
const SAFE = Symbol('safe network error');
const IDENTITY_FIELDS = ['interface', 'ssid', 'profileUuid', 'bssid', 'activePath'];
const LABELS = {
    open: 'Open', wep: 'WEP', psk: 'WPA/WPA2', sae: 'WPA3', owe: 'Enhanced Open (OWE)',
    enterprise: 'Enterprise', unknown: 'Unknown',
};

function failure(message, authentication = false) {
    const error = new Error(message);
    error[SAFE] = true;
    if (authentication) error.authentication = true;
    return error;
}

function safeError(error, message = 'The network operation failed. Check permissions or use Advanced.') {
    if (error?.[SAFE]) return error;
    // Never expose an NM/DBus diagnostic: it may include a setting's secret value.
    let authentication = false;
    try {
        authentication = error.matches?.(NM.secret_agent_error_quark(), NM.SecretAgentError.NOSECRETS) ?? false;
        const name = Gio.DBusError.get_remote_error(error) ?? '';
        authentication ||= /\.(NoSecrets|InvalidSecrets|LoginFailed|AuthenticationFailed)$/.test(name);
    } catch (_) { /* Not every failure is a GLib.Error. */ }
    return failure(message, authentication);
}

const path = object => object?.get_path() ?? '';
const sameObject = (left, right) => Boolean(left && right && (left === right || (path(left) && path(left) === path(right))));
const bytesKey = bytes => bytes ? Array.from(bytes.get_data(), byte => byte.toString(16).padStart(2, '0')).join('') : '';
const ssidText = bytes => bytes ? NM.utils_ssid_to_utf8(bytes.get_data()) ?? '' : '';
const identityEqual = (left, right) => Boolean(left && right && IDENTITY_FIELDS.every(field => left[field] === right[field]));
const isChanging = device => device.get_state() >= NM.DeviceState.PREPARE && device.get_state() < NM.DeviceState.ACTIVATED;

function disconnectSignal(object, id) {
    // NM.Device.disconnect is a network mutation, not GObject.disconnect.
    if (object instanceof GObject.Object) GObject.signal_handler_disconnect(object, id);
    else object.disconnect(id);
}

function apSecurity(ap) {
    const flags = ap.get_wpa_flags() | ap.get_rsn_flags();
    if (flags & (FLAGS.KEY_MGMT_802_1X | FLAGS.KEY_MGMT_EAP_SUITE_B_192)) return 'enterprise';
    if (flags & FLAGS.KEY_MGMT_SAE) return 'sae';
    if (flags & FLAGS.KEY_MGMT_PSK) return 'psk';
    if (flags & (FLAGS.KEY_MGMT_OWE | FLAGS.KEY_MGMT_OWE_TM)) return 'owe';
    if (flags !== FLAGS.NONE) return 'unknown';
    return ap.get_flags() & NM['80211ApFlags'].PRIVACY ? 'wep' : 'open';
}

function profileSecurity(profile) {
    const security = profile.get_setting_wireless_security();
    if (profile.get_setting_802_1x()) return 'enterprise';
    if (!security) return 'open';
    switch (security.get_key_mgmt()) {
    case 'none': return 'wep';
    case 'wpa-psk': case 'wpa-none': return 'psk';
    case 'sae': return 'sae';
    case 'owe': return 'owe';
    case 'ieee8021x': case 'wpa-eap': case 'wpa-eap-suite-b-192': return 'enterprise';
    default: return 'unknown';
    }
}

function protocolLabel(security) {
    const protocols = Array.from({ length: security.get_num_protos() }, (_, index) => security.get_proto(index));
    if (protocols.includes('rsn') && !protocols.includes('wpa')) return 'WPA2';
    if (protocols.includes('wpa') && !protocols.includes('rsn')) return 'WPA';
    // Unrestricted/mixed saved profiles do not identify a negotiated protocol.
    return 'WPA/WPA2';
}

function profileLabel(profile) {
    const type = profileSecurity(profile);
    const security = profile.get_setting_wireless_security();
    switch (security?.get_key_mgmt()) {
    case 'wpa-none': return 'WPA';
    case 'wpa-psk': return protocolLabel(security);
    case 'wpa-eap': return `${protocolLabel(security)} Enterprise`;
    case 'wpa-eap-suite-b-192': return 'WPA3 Enterprise';
    case 'ieee8021x': return security.get_auth_alg() === 'leap' ? 'LEAP' : 'Dynamic WEP';
    default: return LABELS[type];
    }
}

function securityLabel(entry, ap) {
    const profiles = entry.profiles.filter(profile => !ap || apCompatible(ap, profile));
    const activeUuid = entry.device.get_active_connection()?.get_uuid();
    const activeProfile = profiles.find(profile => profile.get_uuid() === activeUuid);
    const labels = [...new Set(profiles.map(profileLabel))];
    const savedLabel = activeProfile ? profileLabel(activeProfile) : labels.length === 1 ? labels[0] : null;
    if (!ap) return savedLabel ?? LABELS[entry.securityType];
    const wpa = ap.get_wpa_flags();
    const rsn = ap.get_rsn_flags();
    switch (apSecurity(ap)) {
    case 'psk': return rsn & FLAGS.KEY_MGMT_PSK ? 'WPA2' : 'WPA';
    case 'enterprise':
        if (rsn & FLAGS.KEY_MGMT_EAP_SUITE_B_192) return 'WPA3 Enterprise';
        if (rsn & FLAGS.KEY_MGMT_802_1X) return 'WPA2 Enterprise';
        if (wpa & FLAGS.KEY_MGMT_802_1X) return 'WPA Enterprise';
        return 'Enterprise';
    case 'wep':
        // Beacon privacy flags cannot distinguish static WEP from legacy 802.1X.
        // Only a compatible, unambiguous (or active) saved profile supplies that.
        return ['Dynamic WEP', 'LEAP'].includes(savedLabel) ? savedLabel : 'WEP';
    default: return LABELS[apSecurity(ap)];
    }
}

function compatible(device, profile) {
    try { return device.connection_valid(profile); } catch (_) { return false; }
}

function apCompatible(ap, profile) {
    try { return ap.connection_valid(profile); } catch (_) { return false; }
}

function authReason(device, active) {
    return [NM.DeviceStateReason.NO_SECRETS, NM.DeviceStateReason.SUPPLICANT_DISCONNECT,
        NM.DeviceStateReason.SUPPLICANT_CONFIG_FAILED, NM.DeviceStateReason.SUPPLICANT_FAILED,
        NM.DeviceStateReason.SUPPLICANT_TIMEOUT].includes(device.get_state_reason())
        || [NM.ActiveConnectionStateReason.NO_SECRETS, NM.ActiveConnectionStateReason.LOGIN_FAILED]
            .includes(active?.get_state_reason());
}

// Always call the matching finish method, including after cancellation/disposal.
function nativeAsync(object, method, finish, args, cancellable) {
    return new Promise((resolve, reject) => {
        try {
            object[method](...args, cancellable, (source, result) => {
                try { resolve(source[finish](result)); } catch (error) { reject(safeError(error)); }
            });
        } catch (error) { reject(safeError(error)); }
    });
}

function unpack(value) {
    if (value instanceof GLib.Variant) return unpack(value.deep_unpack());
    if (Array.isArray(value)) return value.map(unpack);
    if (value && typeof value === 'object')
        return Object.fromEntries(Object.entries(value).map(([key, child]) => [key, unpack(child)]));
    return value;
}

function setPassword(connection, password) {
    const type = profileSecurity(connection);
    const security = connection.get_setting_wireless_security();
    if (typeof password !== 'string' || !password || password.includes('\0'))
        throw failure('Enter a valid network password.', true);
    if (type === 'wep') {
        let keyType;
        if (NM.utils_wep_key_valid(password, NM.WepKeyType.KEY)) keyType = NM.WepKeyType.KEY;
        else if (NM.utils_wep_key_valid(password, NM.WepKeyType.PASSPHRASE)) keyType = NM.WepKeyType.PASSPHRASE;
        else throw failure('Enter a valid WEP key or passphrase.', true);
        security.wep_key_type = keyType;
        security.set_wep_key(security.get_wep_tx_keyidx(), password);
        security.wep_key_flags = NM.SettingSecretFlags.NONE;
    } else if (type === 'psk' || type === 'sae') {
        if (type === 'psk' && !NM.utils_wpa_psk_valid(password))
            throw failure('Enter a valid WPA password (8–63 characters or 64 hexadecimal digits).', true);
        security.psk = password;
        security.psk_flags = NM.SettingSecretFlags.NONE;
    } else {
        throw failure('Configure authentication for this network with Advanced.');
    }
    try { connection.verify_secrets(); } catch (_) { throw failure('Enter a valid network password.', true); }
}

function newConnection(entry, password) {
    const connection = NM.SimpleConnection.new();
    const wireless = entry.wireless;
    connection.add_setting(new NM.SettingConnection({
        id: wireless ? ssidText(entry.ssidBytes) : `Wired ${entry.device.get_iface()}`,
        uuid: GLib.uuid_string_random(), type: wireless ? WIFI : WIRED,
        interface_name: entry.device.get_iface(), autoconnect: true,
    }));
    connection.add_setting(new NM.SettingIP4Config({ method: 'auto' }));
    connection.add_setting(new NM.SettingIP6Config({ method: 'auto' }));
    if (wireless) {
        const ap = bestAP(entry);
        if (!ap) throw failure('This network is no longer in range. Refresh and try again.');
        if (ap.get_mode() !== MODES.INFRA)
            throw failure('Configure this network mode with Advanced.');
        connection.add_setting(new NM.SettingWireless({ ssid: entry.ssidBytes, mode: 'infrastructure' }));
        switch (entry.securityType) {
        case 'open': break;
        case 'wep':
            connection.add_setting(new NM.SettingWirelessSecurity({ key_mgmt: 'none', auth_alg: 'open' }));
            break;
        case 'psk': connection.add_setting(new NM.SettingWirelessSecurity({ key_mgmt: 'wpa-psk' })); break;
        case 'sae':
            connection.add_setting(new NM.SettingWirelessSecurity({
                key_mgmt: 'sae', pmf: NM.SettingWirelessSecurityPmf.REQUIRED,
            }));
            break;
        case 'owe':
            connection.add_setting(new NM.SettingWirelessSecurity({
                key_mgmt: 'owe', pmf: NM.SettingWirelessSecurityPmf.REQUIRED,
            }));
            break;
        default: throw failure('Create an authentication profile with Advanced before connecting.');
        }
        if (['wep', 'psk', 'sae'].includes(entry.securityType)) {
            if (password === null) throw failure('A password is required for this network.', true);
            setPassword(connection, password);
        } else if (password !== null) {
            throw failure('This network does not accept an inline password. Use Advanced.');
        }
    } else {
        if (password !== null) throw failure('Configure wired authentication with Advanced.');
        connection.add_setting(new NM.SettingWired());
    }
    try { connection.verify(); } catch (_) { throw failure('The network settings are not supported. Use Advanced.'); }
    return connection;
}

function bestAP(entry, profile = null) {
    const active = entry.wireless ? entry.device.get_active_access_point() : null;
    return entry.aps.filter(ap => !profile || apCompatible(ap, profile)).sort((left, right) =>
        Number(sameObject(right, active)) - Number(sameObject(left, active))
        || right.get_strength() - left.get_strength() || path(left).localeCompare(path(right)))[0] ?? null;
}

export class NetworkBackend {
    constructor({ onChange = () => {}, network = AstalNetwork.get_default() } = {}) {
        this.onChange = onChange;
        this.network = network;
        this.client = network?.client ?? null;
        this._rows = [];
        this._entries = new Map();
        this._watches = new Map();
        this._operations = new Set();
        this._busy = new Map();
        this._scans = new Map();
        this._scanMembers = new Set();
        this._scanTimer = 0;
        this._idle = 0;
        this._opened = false;
        this._started = false;
        this._disposed = false;
    }

    get rows() { return this._rows.map(row => ({ ...row, profiles: row.profiles.map(profile => ({ ...profile })) })); }
    get available() { return Boolean(this.client?.get_nm_running() && this._devices().length); }
    get wifiAvailable() { return Boolean(this.client?.get_nm_running() && this._devices().some(device => device.get_device_type() === NM.DeviceType.WIFI)); }
    get wifiEnabled() { return Boolean(this.client?.wireless_get_enabled()); }
    get wifiHardwareEnabled() { return Boolean(this.client?.wireless_hardware_get_enabled()); }

    _devices() {
        return (this.client?.get_devices() ?? []).filter(device =>
            [NM.DeviceType.WIFI, NM.DeviceType.ETHERNET].includes(device.get_device_type()));
    }

    start() {
        if (this._started || this._disposed) return;
        this._started = true;
        this._sync();
    }

    dispose() {
        if (this._disposed) return;
        this._disposed = true;
        this._opened = false;
        if (this._idle) GLib.source_remove(this._idle);
        this._idle = 0;
        this._stopScanning();
        for (const operation of this._operations) operation.io.cancel();
        for (const [object, ids] of this._watches) for (const id of ids) disconnectSignal(object, id);
        this._watches.clear();
        this._entries.clear();
        this._rows = [];
        this.onChange = () => {};
    }

    setOpened(opened) {
        if (this._disposed) return;
        this._opened = Boolean(opened);
        if (this._started) this._sync();
    }

    async refresh() {
        if (this._disposed) throw failure('The network panel is closed.');
        this._sync();
        if (!this._canScan()) return;
        await Promise.all(this._devices().filter(device => device.get_device_type() === NM.DeviceType.WIFI)
            .map(device => this._scan(device)));
    }

    toggleWifi() {
        if (this._disposed || !this.client?.get_nm_running() || !this.wifiAvailable)
            throw failure('No Wi-Fi device is available.');
        if (!this.wifiHardwareEnabled) throw failure('Wi-Fi is blocked by the hardware switch.');
        try { this.client.wireless_set_enabled(!this.wifiEnabled); }
        catch (error) { throw safeError(error, 'Unable to change the Wi-Fi radio state.'); }
    }

    _queue() {
        if (this._disposed || !this._started || this._idle) return;
        this._idle = GLib.idle_add(PRIORITY, () => {
            this._idle = 0;
            this._sync();
            return GLib.SOURCE_REMOVE;
        });
    }

    _watch(objects) {
        if (!this._started) return;
        for (const [object, ids] of this._watches) {
            if (objects.has(object)) continue;
            for (const id of ids) disconnectSignal(object, id);
            this._watches.delete(object);
        }
        for (const [object, signals] of objects) {
            if (!object || this._watches.has(object)) continue;
            this._watches.set(object, signals.map(signal => object.connect(signal, () => this._queue())));
        }
    }

    _sync(notify = true) {
        if (this._disposed) return;
        const objects = new Map();
        const watch = (object, signals = ['notify']) => { if (object) objects.set(object, signals); };
        watch(this.client, ['notify', 'device-added', 'device-removed', 'connection-added', 'connection-removed']);
        const entries = new Map();
        const devices = this.client?.get_nm_running() ? this._devices() : [];
        for (const operation of this._operations) {
            if (operation.device && !devices.includes(operation.device)) {
                operation.error = failure('The network device is no longer available.');
                operation.io.cancel();
            }
        }
        const profiles = this.client?.get_nm_running() ? this.client.get_connections() : [];
        for (const profile of profiles) watch(profile, ['changed', 'notify']);
        const add = (device, wireless, ssidBytes = null, securityType = 'open') => {
            const ssidKey = bytesKey(ssidBytes);
            const key = JSON.stringify(wireless ? [device.get_iface(), 'wifi', ssidKey, securityType] : [device.get_iface(), 'wired']);
            if (!entries.has(key)) entries.set(key, { key, device, wireless, ssidBytes, ssidKey, securityType, aps: [], profiles: [] });
            return entries.get(key);
        };
        for (const device of devices) {
            const wireless = device.get_device_type() === NM.DeviceType.WIFI;
            watch(device, wireless ? ['notify', 'access-point-added', 'access-point-removed'] : ['notify']);
            const active = device.get_active_connection();
            watch(active);
            watch(device.get_ip4_config());
            watch(device.get_ip6_config());
            if (!wireless) add(device, false);
            else {
                const activeAP = device.get_active_access_point();
                const aps = [...device.get_access_points()];
                if (activeAP && !aps.some(ap => sameObject(ap, activeAP))) aps.push(activeAP);
                for (const ap of aps) {
                    watch(ap);
                    let ssid = ap.get_ssid();
                    if (!bytesKey(ssid) && sameObject(ap, activeAP)) ssid = active?.get_connection()?.get_setting_wireless()?.get_ssid();
                    if (!bytesKey(ssid)) continue;
                    add(device, true, ssid, apSecurity(ap)).aps.push(ap);
                }
            }
            for (const profile of profiles) {
                if (profile.get_connection_type() !== (wireless ? WIFI : WIRED) || !compatible(device, profile)) continue;
                if (!wireless) { add(device, false).profiles.push(profile); continue; }
                const ssid = profile.get_setting_wireless()?.get_ssid();
                if (!bytesKey(ssid)) continue;
                // AP.connection_valid also handles transition modes, BSSID, band,
                // protocol/cipher restrictions, and SSID bytes without lossy decoding.
                const matches = [...entries.values()].filter(entry => entry.device === device
                    && entry.ssidKey === bytesKey(ssid) && entry.aps.some(ap => apCompatible(ap, profile)));
                if (matches.length) for (const entry of matches) entry.profiles.push(profile);
                else add(device, true, ssid, profileSecurity(profile)).profiles.push(profile);
            }
        }
        this._entries = entries;
        this._rows = [...entries.values()].map(entry => {
            const { device, wireless } = entry;
            const active = device.get_active_connection();
            entry.profiles.sort((left, right) => Number(right.get_uuid() === active?.get_uuid()) - Number(left.get_uuid() === active?.get_uuid())
                || Number(right.get_setting_connection()?.get_timestamp() ?? 0) - Number(left.get_setting_connection()?.get_timestamp() ?? 0)
                || left.get_uuid().localeCompare(right.get_uuid()));
            const target = this._targets(entry);
            const ap = bestAP(entry);
            const ssid = wireless ? ssidText(entry.ssidBytes) : '';
            const connected = target && device.get_state() === NM.DeviceState.ACTIVATED && active?.get_state() === NM.ActiveConnectionState.ACTIVATED;
            return {
                key: entry.key, name: wireless ? ssid : device.get_iface(), ssid,
                interface: device.get_iface(), kind: wireless ? 'WI-FI' : 'WIRED', wireless,
                connected: Boolean(connected),
                changing: this._busy.get(device)?.key === entry.key || Boolean(target && (isChanging(device) || device.get_state() === NM.DeviceState.DEACTIVATING)),
                known: entry.profiles.length > 0, strength: wireless ? Math.max(0, Math.min(100, ap?.get_strength() ?? 0)) : 0,
                security: wireless ? securityLabel(entry, ap) : 'Wired', securityType: entry.securityType,
                profiles: entry.profiles.map(profile => ({ id: profile.get_id(), uuid: profile.get_uuid() })),
            };
        });
        this._watch(objects);
        this._configureScanning();
        if (notify) this.onChange();
    }

    _targets(entry) {
        const active = entry.device.get_active_connection();
        if (!active) return false;
        if (!entry.wireless) return active.get_connection_type() === WIRED;
        const ap = entry.device.get_active_access_point();
        if (ap && bytesKey(ap.get_ssid()))
            return entry.ssidKey === bytesKey(ap.get_ssid()) && entry.securityType === apSecurity(ap);
        return entry.profiles.some(profile => profile.get_uuid() === active.get_uuid());
    }

    _canScan() { return this._started && !this._disposed && this._opened && this.wifiAvailable && this.wifiEnabled && this.wifiHardwareEnabled; }

    _stopScanning() {
        if (this._scanTimer) GLib.source_remove(this._scanTimer);
        this._scanTimer = 0;
        this._scanMembers.clear();
        for (const scan of this._scans.values()) scan.io.cancel();
    }

    _configureScanning() {
        if (!this._canScan()) { this._stopScanning(); return; }
        const devices = new Set(this._devices().filter(device => device.get_device_type() === NM.DeviceType.WIFI));
        for (const [device, scan] of this._scans) if (!devices.has(device)) scan.io.cancel();
        for (const device of devices) if (!this._scanMembers.has(device)) this._scan(device).catch(() => {});
        this._scanMembers = devices;
        if (!this._scanTimer) this._scanTimer = GLib.timeout_add_seconds(PRIORITY, 15, () => {
            if (!this._canScan()) { this._scanTimer = 0; return GLib.SOURCE_REMOVE; }
            for (const device of this._scanMembers) this._scan(device).catch(() => {});
            return GLib.SOURCE_CONTINUE;
        });
    }

    _scan(device) {
        if (!this._canScan()) return Promise.resolve();
        if (this._scans.has(device)) return this._scans.get(device).promise;
        const scan = { io: new Gio.Cancellable() };
        this._scans.set(device, scan);
        scan.promise = nativeAsync(device, 'request_scan_async', 'request_scan_finish', [], scan.io)
            .finally(() => { if (this._scans.get(device) === scan) this._scans.delete(device); });
        return scan.promise;
    }

    _entry(key, device = null) {
        if (this._disposed) throw failure('The network panel is closed.');
        this._sync(false);
        const entry = this._entries.get(key);
        if (!entry || (device && entry.device !== device)) throw failure('The selected network is no longer available. Refresh and try again.');
        return entry;
    }

    getIdentity(key) {
        if (this._disposed) return null;
        this._sync(false);
        const entry = this._entries.get(key);
        if (!entry || !this._targets(entry) || entry.device.get_state() !== NM.DeviceState.ACTIVATED) return null;
        const active = entry.device.get_active_connection();
        if (active?.get_state() !== NM.ActiveConnectionState.ACTIVATED) return null;
        return {
            interface: entry.device.get_iface(), ssid: entry.wireless ? ssidText(entry.ssidBytes) : '',
            profileUuid: active.get_uuid() ?? '',
            bssid: entry.wireless ? entry.device.get_active_access_point()?.get_bssid() ?? '' : '',
            activePath: path(active),
        };
    }

    details(key) {
        const entry = this._entry(key);
        const identity = this.getIdentity(key);
        if (!identity) throw failure('This network is no longer connected.');
        const device = entry.device;
        const active = device.get_active_connection();
        const ip4 = device.get_ip4_config();
        const ip6 = device.get_ip6_config();
        const addresses = config => (config?.get_addresses() ?? []).map(address => `${address.get_address()}/${address.get_prefix()}`);
        const ap = entry.wireless ? device.get_active_access_point() : null;
        const frequency = ap?.get_frequency() ?? 0;
        const rate = entry.wireless ? device.get_bitrate() / 1000 : device.get_speed();
        return {
            ...identity, identity: { ...identity }, profileId: active.get_id() ?? '',
            ipv4: addresses(ip4), ipv6: addresses(ip6), prefix: ip4?.get_addresses()[0]?.get_prefix() ?? '',
            gateway4: ip4?.get_gateway() ?? '', gateway6: ip6?.get_gateway() ?? '',
            dns: [...new Set([...(ip4?.get_nameservers() ?? []), ...(ip6?.get_nameservers() ?? [])])],
            mac: device.get_hw_address() ?? '', frequency: frequency ? `${frequency} MHz` : '',
            band: !frequency ? '' : frequency >= 59400 ? '60 GHz' : frequency >= 5925 ? '6 GHz' : frequency >= 4900 ? '5 GHz' : '2.4 GHz',
            channel: frequency ? String(NM.utils_wifi_freq_to_channel(frequency)) : '',
            mode: ap ? ({ [MODES.INFRA]: 'Infrastructure', [MODES.ADHOC]: 'Ad-Hoc', [MODES.AP]: 'Access point', [MODES.MESH]: 'Mesh' }[ap.get_mode()] ?? 'Unknown') : '',
            rate: rate ? `${Number(rate.toFixed(1))} Mbit/s` : '',
        };
    }

    _assertIdentity(key, expected) {
        if (!identityEqual(this.getIdentity(key), expected))
            throw failure('The active connection changed. Reopen network information and try again.');
    }

    async readSecret(key, expectedIdentity, cancellable = null) {
        // Copy the request stamp so callers cannot change it while authorization runs.
        const expected = expectedIdentity && Object.fromEntries(IDENTITY_FIELDS.map(field => [field, expectedIdentity[field]]));
        return this._run(cancellable, 30000, async operation => {
            this._assertIdentity(key, expected);
            const entry = this._entry(key);
            operation.device = entry.device;
            if (!entry.wireless || ['owe', 'enterprise', 'unknown'].includes(entry.securityType))
                throw failure('Sharing credentials is not supported for this network. Use Advanced.');
            const profile = entry.device.get_active_connection()?.get_connection();
            if (!profile || profile.get_uuid() !== expected.profileUuid)
                throw failure('The active saved profile is no longer available.');
            const type = profileSecurity(profile);
            if (!['open', 'wep', 'psk', 'sae'].includes(type))
                throw failure('Sharing credentials is not supported for this network. Use Advanced.');
            const hidden = Boolean(profile.get_setting_wireless()?.get_hidden());
            let password = '';
            if (type !== 'open') {
                const result = await nativeAsync(profile, 'get_secrets_async', 'get_secrets_finish', [SECURITY], operation.io);
                this._check(operation);
                this._assertIdentity(key, expected);
                const secrets = unpack(result)?.[SECURITY] ?? {};
                const security = profile.get_setting_wireless_security();
                password = type === 'wep' ? secrets[`wep-key${security.get_wep_tx_keyidx()}`] : secrets.psk;
                if (typeof password !== 'string' || !password)
                    throw failure('The saved password is not available. Use Advanced.');
            }
            this._check(operation);
            this._assertIdentity(key, expected);
            return { password, authentication: type === 'open' ? 'nopass' : type === 'wep' ? 'WEP' : 'WPA', hidden };
        });
    }

    _check(operation) {
        if (operation.io.is_cancelled() || this._disposed)
            throw operation.error ?? failure('The network request was cancelled.');
    }

    async _run(cancellable, timeout, body) {
        if (this._disposed) throw failure('The network panel is closed.');
        const operation = { io: new Gio.Cancellable(), error: null };
        this._operations.add(operation);
        let externalId = 0;
        let cancelId = 0;
        let timer = 0;
        const aborted = new Promise((_, reject) => {
            cancelId = operation.io.connect(() => reject(operation.error ?? failure('The network request was cancelled.')));
        });
        try {
            if (cancellable) externalId = cancellable.connect(() => operation.io.cancel());
            if (timeout) timer = GLib.timeout_add(PRIORITY, timeout, () => {
                timer = 0;
                operation.error = failure('The network request timed out. Try again or use Advanced.');
                operation.io.cancel();
                return GLib.SOURCE_REMOVE;
            });
            return await Promise.race([aborted, Promise.resolve().then(() => {
                this._check(operation);
                return body(operation);
            })]);
        } catch (error) { throw safeError(error); }
        finally {
            if (timer) GLib.source_remove(timer);
            if (externalId) cancellable.disconnect(externalId);
            if (cancelId) operation.io.disconnect(cancelId);
            this._operations.delete(operation);
        }
    }

    async _mutate(key, cancellable, body) {
        const entry = this._entry(key);
        if (this._busy.has(entry.device)) throw failure('Wait for the current network operation to finish.');
        const pending = { key };
        this._busy.set(entry.device, pending);
        this._queue();
        try {
            return await this._run(cancellable, 30000, operation => {
                operation.device = entry.device;
                return body(entry, operation);
            });
        }
        finally {
            if (this._busy.get(entry.device) === pending) this._busy.delete(entry.device);
            this._queue();
        }
    }

    async activate(key, { profileUuid = '', password = null, cancellable = null } = {}) {
        return this._mutate(key, cancellable, async (original, operation) => {
            let entry = this._entry(key, original.device);
            if (entry.wireless && (!this.wifiEnabled || !this.wifiHardwareEnabled))
                throw failure('Enable Wi-Fi and check the hardware switch before connecting.');
            if (password !== null && entry.profiles.length > 1)
                throw failure('Multiple saved profiles match this network. Update credentials with Advanced.');
            let profile = profileUuid ? entry.profiles.find(profile => profile.get_uuid() === profileUuid) : entry.profiles[0];
            if (profileUuid && !profile) throw failure('The selected saved profile is no longer available.');
            if (profile && password !== null) {
                const clone = NM.SimpleConnection.new_clone(profile);
                setPassword(clone, password);
                await nativeAsync(profile, 'update2', 'update2_finish', [
                    clone.to_dbus(NM.ConnectionSerializationFlags.ALL), NM.SettingsUpdate2Flags.TO_DISK,
                    new GLib.Variant('a{sv}', { 'version-id': new GLib.Variant('t', profile.get_version_id()) }),
                ], operation.io);
                this._check(operation);
                entry = this._entry(key, original.device);
                profile = entry.profiles.find(current => current.get_uuid() === profile.get_uuid() && sameObject(current, profile));
                if (!profile) throw failure('The selected saved profile changed. Refresh and try again.');
                if (entry.profiles.length > 1) throw failure('The saved profile selection changed. Update credentials with Advanced.');
            }
            this._check(operation);
            entry = this._entry(key, original.device);
            const ap = entry.wireless ? bestAP(entry, profile) : null;
            const connection = profile ?? newConnection(entry, password);
            const requestedUuid = connection.get_uuid();
            const active = profile
                ? await nativeAsync(this.client, 'activate_connection_async', 'activate_connection_finish', [connection, entry.device, path(ap) || null], operation.io)
                : await nativeAsync(this.client, 'add_and_activate_connection_async', 'add_and_activate_connection_finish', [connection, entry.device, path(ap) || null], operation.io);
            this._check(operation);
            if (!active) throw failure('The connection could not be started. Try again or use Advanced.');
            if (active.get_uuid() !== requestedUuid)
                throw failure('The selected saved profile changed during activation. Refresh and try again.');
            await this._waitActivated(entry, active, operation);
        });
    }

    async _waitActivated(entry, active, operation) {
        const signals = [];
        let cancelId = 0;
        let settled = false;
        const device = entry.device;
        let seenTarget = sameObject(device.get_active_connection(), active);
        let seenDeviceProgress = false;
        const wait = new Promise((resolve, reject) => {
            const finish = error => {
                if (settled) return;
                settled = true;
                if (error) reject(error); else resolve();
            };
            const check = () => {
                if (operation.io.is_cancelled()) { finish(operation.error ?? failure('The network request was cancelled.')); return; }
                if (this._disposed || !this.client.get_nm_running() || !this._devices().includes(device)) {
                    finish(failure('The network device is no longer available.')); return;
                }
                const state = active.get_state();
                const deviceState = device.get_state();
                const current = device.get_active_connection();
                const target = sameObject(current, active);
                if (target && isChanging(device)) seenDeviceProgress = true;
                // Device/ActiveConnection properties are not an atomic snapshot.
                // Initial FAILED state can still describe the previous request.
                if (state === NM.ActiveConnectionState.DEACTIVATED || state === NM.ActiveConnectionState.DEACTIVATING
                    || (target && seenDeviceProgress && deviceState === NM.DeviceState.FAILED)) {
                    finish(failure('The connection failed. Check authentication or use Advanced.', authReason(device, active))); return;
                }
                if (target) seenTarget = true;
                else if (current && seenTarget) {
                    finish(failure('Another connection replaced this request.')); return;
                }
                if (entry.wireless && (!this.wifiEnabled || !this.wifiHardwareEnabled)) {
                    finish(failure('Wi-Fi was disabled during the connection request.')); return;
                }
                if (state === NM.ActiveConnectionState.ACTIVATED && deviceState === NM.DeviceState.ACTIVATED && sameObject(current, active)) {
                    this._sync(false);
                    const currentEntry = this._entries.get(entry.key);
                    if (!currentEntry || currentEntry.device !== device || !this._targets(currentEntry))
                        finish(failure('The selected network changed during activation.'));
                    else finish();
                }
            };
            for (const object of [device, active, this.client]) signals.push([object, object.connect('notify', check)]);
            signals.push([this.client, this.client.connect('device-removed', check)]);
            cancelId = operation.io.connect(check);
            check();
        });
        try { await wait; }
        finally {
            for (const [object, id] of signals) disconnectSignal(object, id);
            if (cancelId) operation.io.disconnect(cancelId);
        }
    }

    async disconnect(key) {
        return this._mutate(key, null, async (original, operation) => {
            const entry = this._entry(key, original.device);
            if (!this._targets(entry)) return;
            await nativeAsync(entry.device, 'disconnect_async', 'disconnect_finish', [], operation.io);
            this._check(operation);
        });
    }

    async forget(key) {
        return this._mutate(key, null, async (original, operation) => {
            const profiles = [...original.profiles];
            if (this._targets(original)) {
                await nativeAsync(original.device, 'disconnect_async', 'disconnect_finish', [], operation.io);
                this._check(operation);
            }
            for (const profile of profiles) {
                this._check(operation);
                const current = this.client.get_connections().find(candidate => candidate.get_uuid() === profile.get_uuid());
                if (!current) continue;
                const entry = this._entry(key, original.device);
                if (!entry.profiles.some(candidate => sameObject(candidate, current)) || !sameObject(profile, current))
                    throw failure('The saved profile selection changed. Refresh and try again.');
                await nativeAsync(current, 'delete_async', 'delete_finish', [], operation.io);
                this._check(operation);
            }
        });
    }
}

const MAX_REPLY = 2 * 1024 * 1024;

function closeStream(stream) {
    if (!stream || stream.is_closed() || stream.has_pending()) return;
    stream.close_async(PRIORITY, null, (source, result) => {
        try { source.close_finish(result); } catch (_) { /* A cancelled pipe may already be closed. */ }
    });
}

// Only the encoder runs out of process. Native identity/secrets never leave
// Astal/libnm except as request-local QR input over a private stdin pipe.
export function encodeQr(request, cancellable = null) {
    return new Promise((resolve, reject) => {
        let input = new TextEncoder().encode(JSON.stringify(request));
        if (input.length > 16 * 1024) { reject(new Error('The QR request is too large.')); return; }
        const io = new Gio.Cancellable();
        let child, stdin, stdout, timer = 0, cancelId = 0, done = false, exited = false, eof = false;
        let bytes = 0;
        let chunks = [];
        const finish = error => {
            if (done) return;
            done = true;
            if (timer) GLib.source_remove(timer);
            if (cancelId) cancellable.disconnect(cancelId);
            timer = cancelId = 0;
            io.cancel();
            if (error && child && !exited) child.force_exit();
            input = null;
            closeStream(stdin);
            closeStream(stdout);
            if (error) { chunks = []; reject(error); return; }
            try {
                const all = new Uint8Array(bytes);
                let offset = 0;
                for (const chunk of chunks) { all.set(chunk, offset); offset += chunk.length; }
                chunks = [];
                const value = JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(all));
                if (typeof value.image !== 'string' || !/^data:image\/png;base64,[a-zA-Z0-9+/]+={0,2}$/.test(value.image))
                    throw new Error();
                const encoded = value.image.slice('data:image/png;base64,'.length);
                const png = GLib.base64_decode(encoded);
                if (encoded.length % 4 || GLib.base64_encode(png) !== encoded
                    || ![137, 80, 78, 71, 13, 10, 26, 10].every((byte, index) => png[index] === byte))
                    throw new Error();
                resolve(value.image);
            } catch (_) { reject(new Error('Could not generate the Wi-Fi QR code.')); }
        };
        const complete = () => {
            if (done || !eof || !exited) return;
            finish(child.get_successful() ? null : new Error('Could not generate the Wi-Fi QR code.'));
        };
        const read = () => stdout.read_bytes_async(8192, PRIORITY, io, (stream, result) => {
            try {
                const chunk = stream.read_bytes_finish(result);
                if (done) return;
                if (!chunk.get_size()) { eof = true; complete(); return; }
                bytes += chunk.get_size();
                if (bytes > MAX_REPLY) throw new Error();
                chunks.push(chunk.toArray());
                read();
            } catch (_) { finish(new Error('Could not read the Wi-Fi QR image.')); }
            finally { if (done || eof) closeStream(stream); }
        });
        const write = () => stdin.write_bytes_async(new GLib.Bytes(input.subarray(0, 4096)), PRIORITY, io, (stream, result) => {
            try {
                const count = stream.write_bytes_finish(result);
                if (done) return;
                if (count <= 0) throw new Error();
                input = input.slice(count);
                if (input.length) write();
                else { input = null; closeStream(stream); }
            } catch (_) { finish(new Error('Could not send the Wi-Fi QR request.')); }
            finally { if (done) closeStream(stream); }
        });
        try {
            if (cancellable?.is_cancelled()) throw new Error();
            child = Gio.Subprocess.new([GLib.getenv('NETWORK_CUSTOM_HELPER') || 'network-manager', 'qr'],
                Gio.SubprocessFlags.STDIN_PIPE | Gio.SubprocessFlags.STDOUT_PIPE | Gio.SubprocessFlags.STDERR_SILENCE);
            stdin = child.get_stdin_pipe();
            stdout = child.get_stdout_pipe();
            // Never cancel the wait: even a cancelled encoder must be reaped.
            child.wait_async(null, (source, result) => {
                try { source.wait_finish(result); exited = true; complete(); }
                catch (_) { finish(new Error('The Wi-Fi QR encoder failed.')); }
            });
            if (cancellable) cancelId = cancellable.connect(() => {
                io.cancel();
                // Disconnecting a Gio.Cancellable from its own callback deadlocks.
                Promise.resolve().then(() => finish(new Error('The QR request was cancelled.')));
            });
            timer = GLib.timeout_add(PRIORITY, 12000, () => {
                timer = 0;
                finish(new Error('The Wi-Fi QR encoder timed out.'));
                return GLib.SOURCE_REMOVE;
            });
            read();
            write();
        } catch (_) { finish(new Error('Could not start the Wi-Fi QR encoder.')); }
    });
}

function spawn(argv) {
    const child = Gio.Subprocess.new(argv, Gio.SubprocessFlags.STDOUT_SILENCE | Gio.SubprocessFlags.STDERR_SILENCE);
    child.wait_async(null, (source, result) => {
        try { source.wait_finish(result); } catch (_) { /* Other panels may be shutting down. */ }
    });
}

export function dismissOtherPanels() {
    for (const argv of [[GLib.getenv('DASHBOARD_PANEL') || 'dashboard-panel', 'close'],
        [GLib.getenv('MEDIA_PANEL') || 'media-panel', 'close']]) {
        try { spawn(argv); } catch (_) { console.warn('Could not dismiss another panel.'); }
    }
}

export function controllerRuntime() {
    return {
        encodeQr,
        createCancellable: () => new Gio.Cancellable(),
        launchEditor: () => spawn([GLib.getenv('NETWORK_EDITOR') || 'nm-connection-editor']),
    };
}
