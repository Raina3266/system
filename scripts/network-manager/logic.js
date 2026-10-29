export function signalBars(strength) {
    return Math.ceil(Math.max(0, Math.min(100, Number(strength) || 0)) / 25);
}

export function signalIcon(strength) {
    return ['󰤯', '󰤟', '󰤢', '󰤥', '󰤨'][signalBars(strength)];
}

export function signalLabel(strength) {
    return Number.isFinite(strength) ? `${strength}%` : 'Not currently detected';
}

export function isUndetectedSaved(row) {
    return Boolean(row?.wireless && row.known && !row.connected && !row.changing && !Number.isFinite(row.strength));
}

export function sortRows(rows) {
    const rank = row => row.connected || row.changing ? 0 : row.known ? 1 : 2;
    return rows.slice().sort((left, right) => Number(left.wireless) - Number(right.wireless)
        || rank(left) - rank(right)
        || (left.wireless ? signalBars(right.strength) - signalBars(left.strength) : 0)
        || (isUndetectedSaved(left) && isUndetectedSaved(right)
            ? Number(right.securityType === 'psk') - Number(left.securityType === 'psk') : 0)
        || left.name.toLowerCase().localeCompare(right.name.toLowerCase())
        || left.interface.localeCompare(right.interface) || left.key.localeCompare(right.key));
}

export function primary(rows) {
    return rows.find(row => row.connected && !row.wireless) || rows.find(row => row.connected) || null;
}

export function identityEqual(left, right) {
    return Boolean(left && right && ['interface', 'ssid', 'profileUuid', 'bssid', 'activePath']
        .every(field => left[field] === right[field]));
}

export function profileStamp(row) {
    return JSON.stringify((row?.profiles || []).map(profile => profile.uuid).sort());
}

function savedInfoStamp(row) {
    if (!row?.known || row.connected || !row.profiles?.length) return '';
    // Detection and display-name changes do not change the saved selection.
    return JSON.stringify([row.key, row.interface, row.ssid, row.wireless, row.securityType,
        Boolean(row.changing), profileStamp(row), row.savedIdentity || '']);
}

export function escapeMarkup(value) {
    return String(value).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
        .replace(/"/g, '&quot;').replace(/'/g, '&apos;');
}

export function waybarStatus(rows, details, available) {
    const row = primary(rows);
    if (row) {
        const ipv4 = details?.ipv4;
        const address = (Array.isArray(ipv4) ? ipv4.join(', ') : ipv4) || (details ? 'No IPv4' : 'Reading IPv4…');
        return {
            text: `<span size='large'>${row.wireless ? signalIcon(row.strength) : '󰈀'}</span>`,
            tooltip: escapeMarkup(row.wireless
                ? `Wi-Fi: ${row.name}\nSignal: ${signalLabel(row.strength)}\nSecurity: ${row.security}\nIPv4: ${address}`
                : `Ethernet: ${row.interface}\nIPv4: ${address}`),
            class: row.wireless ? 'wifi' : 'ethernet',
        };
    }
    if (rows.some(row => row.changing))
        return { text: "<span size='large'>󰔟</span>", tooltip: 'Connecting…', class: 'connecting' };
    return { text: "<span size='large'>󰤭</span>",
        tooltip: available ? 'Network disconnected' : 'NetworkManager unavailable or no network devices', class: 'offline' };
}

export function parseRequest(text) {
    let request;
    try {
        if (typeof text !== 'string' || text.length > 8192) throw new Error();
        request = JSON.parse(text);
        if (!request || Array.isArray(request) || typeof request !== 'object'
            || !['status', 'toggle', 'close', 'toggleWifi'].includes(request.command)
            || Object.keys(request).some(key => key !== 'command' && (key !== 'monitor' || request.command !== 'toggle'))
            || ('monitor' in request && (typeof request.monitor !== 'string' || request.monitor.length > 512
                || /[\x00-\x1f\x7f]/.test(request.monitor)))) throw new Error();
    } catch (_) { throw new Error('Invalid network panel request.'); }
    return request;
}

const acceptsPassword = row => row && ['wep', 'psk', 'sae'].includes(row.securityType);

// Request policy lives here; all service state and network mutations stay in the
// native adapter. Injected runtime hooks also make races testable without NM.
export class NetworkController {
    constructor({ backend, onChange = () => {}, createCancellable }) {
        this.backend = backend;
        this.onChange = onChange;

        this.createCancellable = createCancellable;

        this.rows = [];
        this.opened = false;
        this.monitor = '';
        this.serial = 0;
        this.disposed = false;
        this.read = null;
        this.mutation = null;
        this._reset();
        this.backend.onChange = () => this.changed();
    }

    get available() { return this.backend.available; }
    get wifiAvailable() { return this.backend.wifiAvailable; }
    get wifiEnabled() { return this.backend.wifiEnabled; }
    get wifiHardwareEnabled() { return this.backend.wifiHardwareEnabled; }
    get selectedRow() { return this.row(this.selectedKey); }
    get credentialRow() { return this.row(this.credentialKey); }
    row(key) { return this.rows.find(row => row.key === key) || null; }
    emit() { if (!this.disposed) this.onChange(); }

    start() {
        this.backend.start();
        this.changed();
    }

    dispose() {
        if (this.disposed) return;
        this._reset();
        this.disposed = true;
        this.backend.dispose();
        this.onChange = () => {};
    }

    changed() {
        if (this.disposed) return;
        this.rows = sortRows(this.backend.rows);
        const radios = `${this.wifiEnabled}:${this.wifiHardwareEnabled}`;
        if (this.radios !== undefined && this.radios !== radios) this._reset();
        this.radios = radios;
        if (this.selectedKey) {
            if (!this._selectionCurrent(this.selectedKey, this.selectionIdentity, this.selectionSavedStamp, this.selectedProfileUuid)) {
                this._reset();
                this.error = 'The connection changed. Reopen its details.';
            }
        }
        if (this.credentialKey && (!this.credentialRow || profileStamp(this.credentialRow) !== this.credentialProfiles)) {
            this.credentialKey = '';
            this.credentialProfiles = '';
            this.error = 'Saved profiles changed. Select the network again.';
        }
        if (this.forgetKey && !this.row(this.forgetKey)) this.forgetKey = '';
        if (this.pendingKey && !this.row(this.pendingKey)) {
            this.mutation?.io.cancel();
            this.mutation = null;
            this.pendingKey = '';
            this.error = 'The selected network is no longer available.';
        }
        this.emit();
    }

    _cancelRead() {
        this.read?.io.cancel();
        this.read = null;
        this.secretLoading = false;
    }

    _reset() {
        this.serial++;
        this._cancelRead();
        this.mutation?.io.cancel();
        this.mutation = null;
        this.page = 'list';
        this.error = '';
        this.selectedKey = '';
        this.selectionIdentity = null;
        this.selectionSavedStamp = '';
        this.selectedProfileUuid = '';
        this.credentialKey = '';
        this.credentialProfiles = '';
        this.forgetKey = '';
        this.pendingKey = '';
        this.details = {};
        this.secretRequested = false;
        this.secret = '';
    }

    toggle(monitor = '') {
        if (this.opened && this.monitor === monitor) return this.close();
        this._reset();
        this.monitor = monitor;
        this.opened = true;
        this.backend.setOpened(true);
        this.emit();
    }

    close() {
        this.opened = false;
        this._reset();
        this.backend.setOpened(false);
        this.emit();
    }

    back() { this._reset(); this.emit(); }

    dismissError() {
        this.error = '';
        this.emit();
    }

    toggleWifi() {
        this._reset();
        try { this.backend.toggleWifi(); }
        catch (error) { this.error = error.message; }
        this.emit();
    }


    requestCredentials(row) {
        this.credentialKey = row.key;
        this.credentialProfiles = profileStamp(row);
    }

    async connectNetwork(key, profileUuid = '') {
        const row = this.row(key);
        if (!row || row.changing || this.pendingKey || this.disposed
            || (row.wireless && (!this.wifiEnabled || !this.wifiHardwareEnabled))) return;
        this.error = '';
        this.credentialKey = '';
        if (profileUuid && !row.profiles.some(profile => profile.uuid === profileUuid)) {
            this.error = 'The selected saved profile is no longer available.';
        } else if (row.wireless && !row.known && acceptsPassword(row)) {
            this.requestCredentials(row);
        } else if (row.wireless && !row.known && !['open', 'owe'].includes(row.securityType)) {
            this.error = 'Create a saved authentication profile in a network connection editor before connecting to this network.';
        } else {
            return this._activate(row, { profileUuid });
        }
        this.emit();
    }

    async _activate(row, options) {
        const operation = { key: row.key, serial: this.serial, io: this.createCancellable() };
        this.mutation = operation;
        this.pendingKey = row.key;
        this.credentialKey = '';
        this.emit();
        try {
            await this.backend.activate(row.key, { ...options, cancellable: operation.io });
            if (this.mutation === operation) this.error = '';
        } catch (error) {
            if (this.mutation !== operation || operation.serial !== this.serial || this.disposed) return;
            const current = this.row(row.key);
            if (error.authentication && current?.profiles.length > 1) {
                this.error = 'This saved profile needs credentials. Update it in a network connection editor without changing another profile.';
            } else if (error.authentication && acceptsPassword(current) && this.opened) {
                this.requestCredentials(current);
                this.error = 'A Wi-Fi password is required or was not accepted.';
            } else this.error = error.message;
        } finally {
            if (this.mutation === operation) {
                this.mutation = null;
                this.pendingKey = '';
                this.changed();
            }
        }
    }

    async submitPassword(password) {
        const row = this.credentialRow;
        if (!acceptsPassword(row) || !password || this.pendingKey || !this.wifiEnabled || !this.wifiHardwareEnabled) return;
        if (row.profiles.length > 1 || profileStamp(row) !== this.credentialProfiles) {
            this.cancelCredentials();
            this.error = 'Saved profiles changed or need per-profile credentials. Update the selected profile in a network connection editor.';
            this.emit();
            return;
        }
        this.error = '';
        return this._activate(row, { password });
    }

    cancelCredentials() {
        this.credentialKey = '';
        this.credentialProfiles = '';
        this.error = '';
        this.emit();
    }

    async disconnectNetwork(key) {
        const row = this.row(key);
        if (!row || row.changing) return;
        return this._action(() => this.backend.disconnect(key));
    }

    requestForget(key) {
        if (!this.row(key)?.known) return;
        this._reset();
        this.forgetKey = key;
        this.emit();
    }

    confirmForget() {
        const key = this.forgetKey;
        this.forgetKey = '';
        if (!this.row(key)?.known) { this.emit(); return; }
        return this._action(() => this.backend.forget(key));
    }

    cancelForget() { this.forgetKey = ''; this.emit(); }

    async _action(action) {
        const serial = this.serial;
        this.error = '';
        this.emit();
        try { await action(); }
        catch (error) { if (!this.disposed && serial === this.serial) this.error = error.message; }
        this.changed();
    }

    _select(key) {
        const row = this.row(key);
        const savedStamp = savedInfoStamp(row);
        if (!row || (!row.connected && !savedStamp)) return false;
        this._reset();
        try {
            const identity = this.backend.getIdentity(key);
            if (savedStamp ? identity : !identity) throw new Error('The connection changed. Reopen its details.');
            const details = this.backend.details(key);
            const currentIdentity = this.backend.getIdentity(key);
            const currentRow = this.backend.rows.find(row => row.key === key);
            if (savedStamp ? currentIdentity || savedInfoStamp(currentRow) !== savedStamp
                : !identityEqual(identity, currentIdentity))
                throw new Error('The connection changed. Reopen its details.');
            if (savedStamp) {
                const profile = currentRow.profiles.find(profile => profile.uuid === details.profileUuid) || currentRow.profiles[0];
                this.selectedProfileUuid = profile.uuid;
                this.details = { ...details, profileId: profile.id ?? '', profileUuid: profile.uuid };
            } else this.details = details;
            this.selectedKey = key;
            this.selectionIdentity = identity ? { ...identity } : null;
            this.selectionSavedStamp = savedStamp;
            this.page = 'info';
        } catch (error) { this.error = error.message; this.emit(); return false; }
        this.emit();
        return true;
    }

    info(key) { this._select(key); }

    selectSavedProfile(uuid) {
        if (this.disposed || !this.opened || this.page !== 'info' || this.selectionIdentity
            || !this.selectionSavedStamp || !this.selectedRow?.wireless) return;
        if (!this._selectionCurrent(this.selectedKey, null, this.selectionSavedStamp, this.selectedProfileUuid)) {
            this.changed();
            return;
        }
        const row = this.backend.rows.find(row => row.key === this.selectedKey);
        const profile = row?.profiles.find(profile => profile.uuid === uuid);
        if (!profile || row.changing || uuid === this.selectedProfileUuid) return;
        this._cancelRead();
        this.secretRequested = false;
        this.secret = '';
        this.error = '';
        this.selectedProfileUuid = profile.uuid;
        this.details = { ...this.details, profileId: profile.id ?? '', profileUuid: profile.uuid };
        this.emit();
    }

    revealSecret() {
        const row = this.selectedRow;
        if (this.page !== 'info' || !row?.wireless || this.secretLoading
            || !(row.connected ? this.selectionIdentity
                : row.known && !row.changing && this.selectionSavedStamp && this.selectedProfileUuid)) return;
        this.secretRequested = true;
        return this._readSensitive();
    }

    hideSecret() {
        this.secretRequested = false;
        this.secret = '';
        this._cancelRead();
        this.emit();
    }

    _selectionCurrent(key, identity, savedStamp, profileUuid) {
        // getIdentity refreshes native rows before the saved-profile check, even
        // when a DBus change notification has not reached the controller yet.
        const active = this.backend.getIdentity(key);
        if (identity) return identityEqual(identity, active);
        const row = this.backend.rows.find(row => row.key === key);
        return Boolean(savedStamp && !active && savedInfoStamp(row) === savedStamp
            && row.profiles.some(profile => profile.uuid === profileUuid));
    }

    _readCurrent(request) {
        return !this.disposed && this.opened && this.read === request && request.serial === this.serial
            && !request.io.is_cancelled() && request.radios === `${this.wifiEnabled}:${this.wifiHardwareEnabled}`
            && request.key === this.selectedKey && request.page === this.page
            && this.secretRequested
            && request.savedInfoStamp === this.selectionSavedStamp
            && (request.identity || request.profileUuid === this.selectedProfileUuid)
            && this._selectionCurrent(request.key, request.identity, request.savedInfoStamp, request.profileUuid);
    }

    async _readSensitive() {
        const row = this.selectedRow;
        const saved = this.page === 'info' && this.selectionSavedStamp
            && this.selectedProfileUuid && row?.known && !row.connected && !row.changing;
        if (this.disposed || !this.opened || !row?.wireless
            || (!saved && (!row.connected || !this.selectionIdentity))) return;
        if (this.radios !== `${this.wifiEnabled}:${this.wifiHardwareEnabled}`
            || !this._selectionCurrent(this.selectedKey, this.selectionIdentity, this.selectionSavedStamp, this.selectedProfileUuid)) {
            this.changed();
            return;
        }
        this._cancelRead();
        this.secret = '';
        this.error = '';
        const request = { key: this.selectedKey, page: this.page, serial: this.serial,
            identity: this.selectionIdentity ? { ...this.selectionIdentity } : null,
            savedInfoStamp: this.selectionSavedStamp, profileUuid: this.selectedProfileUuid,
            radios: `${this.wifiEnabled}:${this.wifiHardwareEnabled}`, io: this.createCancellable() };
        this.read = request;
        this.secretLoading = true;
        this.emit();
        try {
            const credentials = request.identity
                ? await this.backend.readSecret(request.key, request.identity, request.io)
                : await this.backend.readSavedSecret(request.key, request.profileUuid, request.io);
            if (!this._readCurrent(request)) return;
            this.secret = credentials.password || 'No password (open network)';
        } catch (error) {
            if (this._readCurrent(request)) {
                this.error = error.message;
                this.secretRequested = false;
                this.secret = '';
            }
        } finally {
            if (this.read === request) {
                this.read = null;
                this.secretLoading = false;
                // A roam, profile replacement, or radio change can precede its
                // queued notification. Do not retain the obsolete selection.
                if (request.radios !== `${this.wifiEnabled}:${this.wifiHardwareEnabled}`
                    || !this._selectionCurrent(request.key, request.identity, request.savedInfoStamp, request.profileUuid)) this.changed();
                else this.emit();
            }
        }
    }


    status() {
        const row = primary(this.rows);
        let details = null;
        if (row) {
            try { details = this.backend.details(row.key); } catch (_) { /* A native transition may race the badge request. */ }
        }
        return JSON.stringify(waybarStatus(this.rows, details, this.available));
    }
}
