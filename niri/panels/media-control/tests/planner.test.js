import test from 'node:test';
import assert from 'node:assert/strict';
import { Planner } from '../logic.js';

function player(bus = 'a', extra = {}) {
    return {
        bus, identity: `App ${bus}`, desktopEntry: `app-${bus}`, title: `Track ${bus}`,
        artist: 'Artist', album: 'Album', url: '', artUrl: '', browserHint: null,
        playing: true, stopped: false, position: 10, length: 200,
        canSeek: true, canNext: true, canPrevious: true,
        trackId: `/track/${bus}`, instance: `${bus}-instance-1`, timelineEpoch: 0,
        positionSupported: true, rate: 1, volume: 0.5, ...extra,
    };
}

function stream(extra = {}) {
    return {
        id: 42, name: 'App a', nickname: '', ready: true,
        properties: { 'application.name': 'App a', 'media.name': 'Track a' },
        instance: 'stream-instance-1', volume: 0.5, ...extra,
    };
}

function harness(players = [player()], streams = []) {
    const h = {
        state: { players, streams }, time: 100, sent: [], applied: [], models: [], errors: [],
        onApply: null, onSend: null, onModel: null,
    };
    h.planner = new Planner({
        capture: () => h.state,
        now: () => h.time,
        send: request => {
            h.sent.push(request);
            return h.onSend?.(request);
        },
        apply: operation => {
            h.applied.push({ ...operation });
            if (h.onApply) return h.onApply(operation);
            if (operation.operation === 'seek') {
                const target = h.state.players.find(p => p.bus === operation.player);
                target.position = operation.value;
                target.timelineEpoch++;
            }
        },
        onModel: (groups, active) => {
            h.models.push({ groups, active });
            h.onModel?.(groups, active);
        },
        onError: error => h.errors.push(error),
    });
    return h;
}

function group(h, primary = 'a', players = [primary], extra = {}) {
    const native = h.state.players.find(p => p.bus === primary);
    return {
        title: native?.title ?? '', artist: native?.artist ?? '', primary, players,
        displayName: native?.identity ?? '', artUrl: '', streamId: null, streamReason: 'No matching audio stream.',
        ...extra,
    };
}

function response(h, options = {}, request = h.sent.at(-1)) {
    const groups = options.groups ?? h.state.players.map(p => group(h, p.bus));
    const active = options.active ?? (groups.find(g => g.players.includes(request.selectedPlayer)) ?? groups[0])?.primary ?? '';
    return {
        requestId: request.requestId, groups, activePlayer: active, actions: [], ...options,
    };
}

const tick = async () => { await Promise.resolve(); await Promise.resolve(); };

async function prime(h, groups) {
    assert.equal(h.planner.refresh(), true);
    await tick();
    assert.equal(h.planner.receive(response(h, groups ? { groups } : {})), true);
    await tick();
    return h;
}

function seek(player, value) { return { player, operation: 'seek', value }; }
function op(player, operation) { return { player, operation }; }

test('refresh coalesces and sends detached JSON OBJECT protocol fields only', async () => {
    const h = harness([player('a', { position: -1, positionSupported: false, length: Infinity })], [stream()]);
    for (let i = 0; i < 20; i++) h.planner.refresh();
    assert.equal(h.sent.length, 0, 'dispatch is deferred');
    await tick();
    assert.equal(h.sent.length, 1);
    const request = h.sent[0];
    assert.equal(typeof request, 'object');
    assert.deepEqual(Object.keys(request).sort(), ['players', 'requestId', 'selectedPlayer', 'streams']);
    assert.equal(request.players[0].position, 0);
    assert.equal(request.players[0].length, null);
    for (const key of ['instance', 'trackId', 'timelineEpoch', 'positionSupported', 'rate', 'volume'])
        assert.equal(key in request.players[0], false);
    assert.equal('instance' in request.streams[0], false);
    assert.equal('volume' in request.streams[0], false);
    assert.equal('routing' in request, false);
    assert.deepEqual(JSON.parse(JSON.stringify(request)), request);
    h.state.streams[0].properties['application.name'] = 'Changed owner';
    assert.equal(request.streams[0].properties['application.name'], 'App a');
    assert.ok(Object.isFrozen(request));
});

test('fresh response publishes Rust groups without JS regrouping and remembers selection', async () => {
    const h = harness([player(), player('alias', { playing: false }), player('other')]);
    const groups = [group(h, 'a', ['a', 'alias']), group(h, 'other')];
    await prime(h, groups);
    assert.deepEqual(h.planner.groups, groups);
    assert.equal(h.planner.activePlayer, 'a');
    assert.equal(h.planner.selectedPlayer, 'a');
    assert.ok(Object.isFrozen(h.planner.groups[0].players));
    assert.equal(h.planner.select('alias'), true);
    await tick();
    assert.equal(h.sent.at(-1).selectedPlayer, 'alias');
    assert.equal(h.planner.receive(response(h, { groups, active: 'a' })), true);
    assert.equal(h.planner.selectedPlayer, 'alias', 'retain an explicit member selection');
});

test('only one request is in flight; a pending refresh is not lost', async () => {
    const h = await prime(harness());
    h.planner.refresh();
    await tick();
    const pending = h.sent.at(-1);
    h.planner.refresh();
    h.planner.refresh();
    h.planner.action('next');
    await tick();
    assert.equal(h.sent.length, 2);
    h.planner.receive(response(h, {}, pending));
    await tick();
    assert.equal(h.sent.length, 3);
    assert.equal(h.sent.at(-1).action.kind, 'next');
    h.planner.receive(response(h, { actions: [op('a', 'next')] }));
    await tick();
    assert.equal(h.sent.length, 3, 'action response also satisfies the pending refresh');
    assert.deepEqual(h.applied, [op('a', 'next')]);
});

test('position, volume, and property insertion order are excluded from staleness', async () => {
    const h = await prime(harness([player()], [stream()]));
    h.planner.action('next');
    h.state.players[0].position = 11;
    h.state.players[0].volume = 0.9;
    h.state.streams[0].volume = 0.1;
    h.state.streams[0].properties = { 'media.name': 'Track a', 'application.name': 'App a' };
    await tick();
    assert.equal(h.sent.at(-1).players[0].position, 11, 'send recaptures rather than reusing the queued position');
    h.state.players[0].position = 12;
    h.time++;
    assert.equal(h.planner.receive(response(h, { actions: [op('a', 'next')] })), true);
    assert.deepEqual(h.applied, [op('a', 'next')]);
    assert.deepEqual(h.errors, []);
});

const playerChanges = {
    bus: p => { p.bus = 'replacement-bus'; },
    identity: p => { p.identity = 'Replacement App'; },
    desktopEntry: p => { p.desktopEntry = 'other.desktop'; },
    title: p => { p.title = 'Next track'; },
    artist: p => { p.artist = 'Other artist'; },
    album: p => { p.album = 'Other album'; },
    url: p => { p.url = 'https://example.test/other'; },
    artUrl: p => { p.artUrl = 'file:///other-art'; },
    browserHint: p => { p.browserHint = 'firefox'; },
    playing: p => { p.playing = false; },
    stopped: p => { p.stopped = true; },
    length: p => { p.length = 201; },
    canSeek: p => { p.canSeek = false; },
    canNext: p => { p.canNext = false; },
    canPrevious: p => { p.canPrevious = false; },
    trackId: p => { p.trackId = '/track/next'; },
    instance: p => { p.instance = 'a-instance-2'; },
    timelineEpoch: p => { p.timelineEpoch++; },
    positionSupported: p => { p.positionSupported = false; },
    rate: p => { p.rate = 2; },
};

for (const [field, change] of Object.entries(playerChanges)) {
    test(`player ${field} invalidates both queued and in-flight actions`, async () => {
        for (const inFlight of [false, true]) {
            const h = await prime(harness());
            h.planner.action('next');
            if (inFlight) await tick();
            change(h.state.players[0]);
            if (inFlight) assert.equal(h.planner.receive(response(h, { actions: [op('a', 'next')] })), false);
            await tick();
            assert.equal(h.applied.length, 0);
            assert.match(h.planner.error, /stale|Stale/);
            assert.match(h.planner.error, /not retried/);
            assert.equal(h.sent.at(-1).action, undefined, 'refreshes may recover, actions never retry');
        }
    });
}

const streamChanges = {
    id: s => { s.id = 43; },
    name: s => { s.name = 'Other'; },
    nickname: s => { s.nickname = 'Other'; },
    ready: s => { s.ready = false; },
    properties: s => { s.properties['application.process.id'] = '5678'; },
    instance: s => { s.instance = 'stream-instance-2'; },
};
for (const [field, change] of Object.entries(streamChanges)) {
    test(`stream ${field} invalidates both queued and in-flight plans`, async () => {
        for (const inFlight of [false, true]) {
            const h = await prime(harness([player()], [stream()]));
            h.planner.action('toggle');
            if (inFlight) await tick();
            change(h.state.streams[0]);
            if (inFlight) assert.equal(h.planner.receive(response(h, { actions: [op('a', 'pause')] })), false);
            await tick();
            assert.equal(h.applied.length, 0);
            assert.match(h.planner.error, /ownership changed/);
        }
    });
}

test('player/stream addition, removal, and order changes invalidate pending work', async () => {
    const changes = [
        h => h.state.players.push(player('c')),
        h => { h.state.players = []; },
        h => h.state.players.reverse(),
        h => h.state.streams.push(stream({ id: 44, instance: 3 })),
        h => { h.state.streams = []; },
        h => h.state.streams.reverse(),
    ];
    for (const change of changes) {
        const h = await prime(harness([player(), player('b')], [stream(), stream({ id: 43, instance: 2 })]));
        h.planner.action('next');
        await tick();
        change(h);
        assert.equal(h.planner.receive(response(h, { actions: [op('a', 'next')] })), false);
        assert.equal(h.applied.length, 0);
    }
});

test('a replaced native object with a reused bus or stream ID is not mistaken for the old instance', async () => {
    for (const replace of [
        h => { h.state.players[0] = { ...h.state.players[0], instance: 'new-player-object' }; },
        h => { h.state.streams[0] = { ...h.state.streams[0], instance: 'new-stream-object' }; },
    ]) {
        const h = await prime(harness([player()], [stream()]));
        h.planner.action('next');
        await tick();
        replace(h);
        assert.equal(h.planner.receive(response(h, { actions: [op('a', 'next')] })), false);
        assert.equal(h.applied.length, 0);
    }
});

test('selection revision detects an A -> B -> A change before send or response', async () => {
    for (const inFlight of [false, true]) {
        const h = await prime(harness([player(), player('b')]));
        h.planner.action('next');
        if (inFlight) await tick();
        h.planner.select('b');
        h.planner.select('a');
        if (inFlight) assert.equal(h.planner.receive(response(h, { actions: [op('a', 'next')] })), false);
        await tick();
        assert.equal(h.applied.length, 0);
        assert.match(h.planner.error, /selection changed/);
    }
});

test('cycle follows the selected group, including a selection whose model reply is still pending', async () => {
    const h = await prime(harness([player(), player('b'), player('c')]));
    assert.equal(h.planner.select('b'), true);
    assert.equal(h.planner.cycle(), true);
    assert.equal(h.planner.selectedPlayer, 'c');
    assert.equal(h.planner.cycle(), true);
    assert.equal(h.planner.selectedPlayer, 'a');
    assert.equal(h.planner.select('gone'), false);
    assert.match(h.planner.error, /no longer available/);
    assert.equal(harness([]).planner.cycle(), false);
});

test('seek gesture coalesces to the final queued seek and rebases only our own epoch change', async () => {
    const h = await prime(harness());
    h.planner.action('seek', 'a', 20);
    await tick();
    const first = h.sent.at(-1);
    for (const value of [30, 40, 50]) h.planner.action('seek', 'a', value);
    assert.equal(h.planner.receive(response(h, { actions: [seek('a', 20)] }, first)), true);
    assert.equal(h.state.players[0].timelineEpoch, 1);
    await tick();
    assert.equal(h.sent.length, 3);
    assert.equal(h.sent.at(-1).action.position, 50);
    assert.equal(h.sent.at(-1).players[0].position, 20);
    assert.equal(h.planner.receive(response(h, { actions: [seek('a', 50)] })), true);
    assert.deepEqual(h.applied, [seek('a', 20), seek('a', 50)]);
    assert.equal(h.state.players[0].timelineEpoch, 2);
    assert.equal(h.planner.error, '');
});

test('seeks also coalesce before the first deferred send', async () => {
    const h = await prime(harness());
    for (let position = 0; position < 100; position++) h.planner.action('seek', '', position);
    await tick();
    assert.equal(h.sent.length, 2);
    assert.equal(h.sent.at(-1).action.position, 99);
});

test('grouped seek validates and rebases exact per-player own epoch increments', async () => {
    const h = harness([player(), player('alias', { playing: false })]);
    const groups = [group(h, 'a', ['a', 'alias'])];
    await prime(h, groups);
    h.planner.action('seek', 'alias', 20);
    await tick();
    h.planner.action('seek', 'alias', 40);
    assert.equal(h.planner.receive(response(h, { groups, actions: [seek('a', 20), seek('alias', 20)] })), true);
    await tick();
    assert.equal(h.sent.at(-1).action.position, 40);
    assert.deepEqual(h.state.players.map(p => p.timelineEpoch), [1, 1]);
});

test('an external change before, during, or after own seek apply never rebases a queued seek', async () => {
    for (const when of ['before', 'during', 'after']) {
        const h = await prime(harness());
        h.planner.action('seek', 'a', 20);
        await tick();
        const request = h.sent.at(-1);
        h.planner.action('seek', 'a', 50);
        if (when === 'before') h.state.players[0].timelineEpoch++;
        if (when === 'during') h.onApply = operation => {
            h.state.players[0].position = operation.value;
            h.state.players[0].timelineEpoch += 2; // own seek plus external discontinuity
        };
        h.planner.receive(response(h, { actions: [seek('a', 20)] }, request));
        if (when === 'after') h.state.players[0].timelineEpoch++;
        await tick();
        assert.equal(h.sent.at(-1).action, undefined);
        assert.equal(h.applied.length, when === 'before' ? 0 : 1);
        assert.match(h.planner.error, /not retried/);
    }
});

test('seek rebase cannot cross a different target or an intervening non-seek action', async () => {
    for (const barrier of ['other-bus', 'next']) {
        const h = await prime(harness([player(), player('b')]));
        h.planner.action('seek', 'a', 20);
        await tick();
        if (barrier === 'other-bus') h.planner.action('seek', 'b', 30);
        else {
            h.planner.action('next', 'a');
            h.planner.action('seek', 'a', 30);
        }
        h.planner.receive(response(h, { actions: [seek('a', 20)] }));
        await tick();
        assert.deepEqual(h.applied, [seek('a', 20)]);
        assert.equal(h.sent.at(-1).action, undefined);
        assert.match(h.planner.error, /stale queued/);
    }
});

test('pending queue is bounded to eight actions, with no blind retries', async () => {
    const h = await prime(harness());
    h.planner.refresh();
    await tick();
    for (let i = 0; i < 8; i++) assert.equal(h.planner.action('next'), true);
    assert.equal(h.planner.action('next'), false);
    assert.match(h.planner.error, /queue is full/);
    h.planner.receive(response(h));
    await tick();
    assert.equal(h.sent.at(-1).action.kind, 'next');
});

test('five-second action age is checked before send and before applying a response', async () => {
    for (const inFlight of [false, true]) {
        const h = await prime(harness());
        h.planner.action('next');
        if (inFlight) await tick();
        h.time += 5.01;
        if (inFlight) assert.equal(h.planner.receive(response(h, { actions: [op('a', 'next')] })), false);
        await tick();
        assert.equal(h.applied.length, 0);
        assert.match(h.planner.error, /expired/);
    }
});

test('stale refresh cannot publish old groups, and errors survive a successful recovery refresh', async () => {
    const h = await prime(harness());
    const previous = h.planner.groups;
    h.planner.refresh();
    await tick();
    h.state.players[0].trackId = '/new-track';
    assert.equal(h.planner.receive(response(h)), false);
    assert.equal(h.planner.groups, previous);
    const error = h.planner.error;
    await tick();
    assert.equal(h.planner.receive(response(h)), true);
    assert.equal(h.planner.error, error);
});

test('invalid action kinds/routing, invalid seconds, and unavailable bus do not dispatch native calls', async () => {
    const h = await prime(harness());
    for (const args of [['move', 'a'], ['seek', 'a', -1], ['seek', 'a', NaN], ['next', {}, 0]])
        assert.equal(h.planner.action(...args), false);
    assert.equal(h.sent.length, 1);
    h.planner.action('next', 'gone');
    await tick();
    assert.equal(h.planner.receive(response(h, { error: 'The requested media source is no longer available.' })), true);
    assert.equal(h.applied.length, 0);
    assert.match(h.planner.error, /no longer available/);
});

test('model policy errors can publish valid groups but cannot apply operations', async () => {
    const h = await prime(harness());
    h.planner.action('seek', 'a', 20);
    await tick();
    assert.equal(h.planner.receive(response(h, { error: 'Source cannot seek.' })), true);
    assert.match(h.planner.error, /cannot seek/);
    assert.equal(h.applied.length, 0);
    h.planner.action('next');
    await tick();
    assert.equal(h.planner.receive(response(h, { actions: [op('a', 'next')] })), true);
    assert.equal(h.planner.error, '');
    assert.equal(h.errors.at(-1), '');
});

test('the entire native plan is validated before the first operation is applied', async () => {
    const h = harness([player(), player('alias')]);
    const groups = [group(h, 'a', ['a', 'alias'])];
    await prime(h, groups);
    h.planner.action('toggle', 'a');
    await tick();
    assert.equal(h.planner.receive(response(h, { groups, actions: [op('a', 'pause'), op('alias', 'move')] })), false);
    assert.equal(h.applied.length, 0);
    assert.match(h.planner.error, /routing is not supported/);
});

const invalidPlans = [
    ['wrong operation', {}, 'next', [op('a', 'pause')]],
    ['wrong target', {}, 'next', [op('b', 'next')]],
    ['missing next capability', { canNext: false }, 'next', [op('a', 'next')]],
    ['missing previous capability', { canPrevious: false }, 'previous', [op('a', 'previous')]],
    ['missing seek capability', { canSeek: false }, 'seek', [seek('a', 20)]],
    ['unsupported position', { position: -1, positionSupported: false }, 'seek', [seek('a', 20)]],
    ['wrong seek value', {}, 'seek', [seek('a', 21)]],
    ['negative seek value', {}, 'seek', [seek('a', -1)]],
    ['nonfinite seek value', {}, 'seek', [seek('a', NaN)]],
    ['unrequested native value', {}, 'next', [{ ...op('a', 'next'), value: 20 }]],
    ['routing field', {}, 'next', [{ ...op('a', 'next'), target: 'speaker' }]],
    ['duplicate native target', {}, 'next', [op('a', 'next'), op('a', 'next')]],
];
for (const [name, extra, kind, actions] of invalidPlans) {
    test(`reject invalid plan: ${name}`, async () => {
        const h = await prime(harness([player('a', extra), player('b')]));
        h.planner.action(kind, 'a', kind === 'seek' ? 20 : 0);
        await tick();
        assert.equal(h.planner.receive(response(h, { actions })), false);
        assert.equal(h.applied.length, 0);
        assert.match(h.planner.error, /rejected/);
    });
}

test('refresh actions, error actions, malformed groups, invalid active bus, and unavailable streams are rejected', async () => {
    const modifications = [
        h => ({ actions: [op('a', 'pause')] }),
        h => ({ actions: [op('a', 'pause')], error: 'Failure' }),
        h => ({ groups: [group(h, 'gone')] }),
        h => ({ groups: [group(h, 'a', ['a', 'a'])] }),
        h => ({ groups: [group(h), group(h)] }),
        h => ({ groups: [group(h, 'a', ['b'])] }),
        h => ({ activePlayer: 'gone' }),
        h => ({ groups: [group(h, 'a', ['a'], { streamId: 999 })] }),
        h => ({ groups: [group(h, 'a', ['a'], { streamId: 0 })] }),
        h => ({ groups: [group(h, 'a', ['a'], { streamId: 42 })] }),
        h => ({ groups: null }),
        h => ({ actions: null }),
        h => ({ error: 42 }),
    ];
    for (const modify of modifications) {
        const h = await prime(harness([player(), player('b')], [stream({ ready: false })]));
        h.planner.refresh();
        await tick();
        assert.equal(h.planner.receive(response(h, modify(h))), false);
        assert.equal(h.applied.length, 0);
    }
});

test('valid ready stream ownership and ungrouped playing pause-all targets are accepted', async () => {
    const h = harness([player(), player('filtered')], [stream()]);
    const groups = [group(h, 'a', ['a'], { streamId: 42 })];
    await prime(h, groups);
    h.planner.action('pauseAll');
    await tick();
    assert.equal(h.planner.receive(response(h, { groups, actions: [op('a', 'pause'), op('filtered', 'pause')] })), true);
    assert.deepEqual(h.applied, [op('a', 'pause'), op('filtered', 'pause')]);
});

test('toggle follows the Rust group, next/previous only its primary, and idle togglePauseAll resumes selected group', async () => {
    const h = harness([player('a', { playing: false }), player('alias', { playing: false }), player('b', { playing: false })]);
    const groups = [group(h, 'a', ['a', 'alias']), group(h, 'b')];
    await prime(h, groups);
    for (const [kind, actions] of [
        ['toggle', [op('a', 'play'), op('alias', 'play')]],
        ['next', [op('a', 'next')]],
        ['previous', [op('a', 'previous')]],
        ['togglePauseAll', [op('a', 'play'), op('alias', 'play')]],
    ]) {
        h.planner.action(kind, 'alias');
        await tick();
        assert.equal(h.planner.receive(response(h, { groups, actions })), true);
    }
    assert.equal(h.applied.length, 6);
});

test('togglePauseAll must not mix play and pause; pauseAll must not play', async () => {
    for (const [kind, actions] of [
        ['togglePauseAll', [op('a', 'pause'), op('b', 'play')]],
        ['pauseAll', [op('a', 'play')]],
    ]) {
        const h = await prime(harness([player(), player('b', { playing: false })]));
        h.planner.action(kind);
        await tick();
        assert.equal(h.planner.receive(response(h, { actions })), false);
        assert.equal(h.applied.length, 0);
    }
});

test('snapshot is guarded between native operations; external changes stop the remaining plan', async () => {
    const h = harness([player(), player('alias')]);
    const groups = [group(h, 'a', ['a', 'alias'])];
    await prime(h, groups);
    h.onApply = () => { h.state.players[1].instance = 'new-alias-instance'; };
    h.planner.action('toggle');
    await tick();
    assert.equal(h.planner.receive(response(h, { groups, actions: [op('a', 'pause'), op('alias', 'pause')] })), false);
    assert.deepEqual(h.applied, [op('a', 'pause')]);
    assert.match(h.planner.error, /remaining actions were not retried/);
});

test('apply errors, false, and explicit error results are useful and never retry the operation', async () => {
    for (const apply of [
        () => { throw new Error('native object disappeared'); },
        () => false,
        () => ({ error: 'unsupported native operation' }),
    ]) {
        const h = await prime(harness());
        h.onApply = apply;
        h.planner.action('next');
        await tick();
        assert.equal(h.planner.receive(response(h, { actions: [op('a', 'next')] })), false);
        await tick();
        assert.deepEqual(h.applied, [op('a', 'next')]);
        assert.equal(h.sent.at(-1).action, undefined);
        assert.match(h.planner.error, /native object disappeared|returned false|unsupported native/);
        assert.equal(h.planner.ready, true);
    }
});

test('failed native seek discards its queued final seek rather than assuming it can rebase', async () => {
    const h = await prime(harness());
    h.onApply = () => { throw new Error('native seek failed'); };
    h.planner.action('seek', 'a', 20);
    await tick();
    h.planner.action('seek', 'a', 50);
    assert.equal(h.planner.receive(response(h, { actions: [seek('a', 20)] })), false);
    await tick();
    assert.deepEqual(h.applied, [seek('a', 20)]);
    assert.equal(h.sent.at(-1).action, undefined);
    assert.match(h.planner.error, /pending actions were discarded, not retried/);
});

test('Promise send/apply and an explicit false send fail safely', async () => {
    for (const send of [() => false, () => Promise.reject(new Error('async send failure'))]) {
        const h = await prime(harness());
        h.onSend = send;
        h.planner.action('next');
        await tick();
        assert.equal(h.planner.ready, false);
        assert.equal(h.applied.length, 0);
        assert.match(h.planner.error, /send failed/);
    }
    const h = await prime(harness());
    h.onApply = () => Promise.reject(new Error('async apply failure'));
    h.planner.action('next');
    await tick();
    assert.equal(h.planner.receive(response(h, { actions: [op('a', 'next')] })), false);
    await tick();
    assert.match(h.planner.error, /apply\(\) must be synchronous/);
    assert.equal(h.sent.at(-1).action, undefined);
});

test('seek apply must synchronously advance exactly its own timeline epoch', async () => {
    const h = await prime(harness());
    h.onApply = () => {};
    h.planner.action('seek', 'a', 20);
    await tick();
    assert.equal(h.planner.receive(response(h, { actions: [seek('a', 20)] })), false);
    assert.match(h.planner.error, /advance only its own timeline epoch once/);
});

test('synchronous send replies and callback refresh/action calls drain without recursive send', async () => {
    const h = harness();
    let depth = 0;
    let maxDepth = 0;
    let didAction = false;
    h.onSend = request => {
        depth++;
        maxDepth = Math.max(depth, maxDepth);
        const actions = request.action ? [op('a', 'next')] : [];
        h.planner.receive(response(h, { actions }, request));
        depth--;
    };
    h.onModel = () => {
        if (!didAction) {
            didAction = true;
            h.planner.refresh();
            h.planner.action('next');
        }
    };
    h.planner.refresh();
    await tick();
    await tick();
    assert.equal(h.sent.length, 2);
    assert.equal(maxDepth, 1);
    assert.deepEqual(h.applied, [op('a', 'next')]);
});

test('reset clears model/errors/selection/work and late replies cannot match restarted IDs', async () => {
    const h = await prime(harness());
    h.planner.action('next');
    await tick();
    const old = h.sent.at(-1);
    h.planner.action('seek', 'a', 50);
    h.planner.action('bad-kind');
    h.planner.reset();
    assert.equal(h.planner.ready, false);
    assert.deepEqual(h.planner.groups, []);
    assert.equal(h.planner.activePlayer, '');
    assert.equal(h.planner.selectedPlayer, '');
    assert.equal(h.planner.error, '');
    assert.equal(h.errors.at(-1), '');
    assert.equal(h.planner.receive(response(h, { actions: [op('a', 'next')] }, old)), false);
    h.planner.setReady(true);
    await tick();
    assert.ok(h.sent.at(-1).requestId > old.requestId);
    assert.equal(h.sent.at(-1).action, undefined);
    assert.equal(h.planner.receive(response(h, {}, old)), false);
    assert.equal(h.planner.receive(response(h)), true);
    assert.equal(h.applied.length, 0);
});

test('reset during native apply cancels the rest of the plan and never republishes the old model', async () => {
    const h = harness([player(), player('alias')]);
    const groups = [group(h, 'a', ['a', 'alias'])];
    await prime(h, groups);
    h.onApply = () => h.planner.reset();
    h.planner.action('toggle');
    await tick();
    assert.equal(h.planner.receive(response(h, { groups, actions: [op('a', 'pause'), op('alias', 'pause')] })), false);
    assert.equal(h.applied.length, 1);
    assert.deepEqual(h.planner.groups, []);
    assert.equal(h.models.at(-1).active, '');
});

test('send failure disables worker, clears model, and never replays actions after restart', async () => {
    const h = await prime(harness());
    h.onSend = () => { throw new Error('worker stdin closed'); };
    h.planner.action('next');
    h.planner.action('seek', 'a', 40);
    await tick();
    assert.equal(h.planner.ready, false);
    assert.deepEqual(h.planner.groups, []);
    assert.match(h.planner.error, /stdin closed/);
    assert.match(h.planner.error, /not retried/);
    h.onSend = null;
    h.planner.setReady(true);
    await tick();
    assert.equal(h.sent.at(-1).action, undefined);
    assert.equal(h.applied.length, 0);
});

test('setReady(false) prevents capture/send, and actions are rejected instead of retained', async () => {
    const h = harness();
    h.planner.setReady(false);
    assert.equal(h.planner.refresh(), false);
    assert.equal(h.planner.action('next'), false);
    assert.match(h.planner.error, /not ready/);
    await tick();
    assert.equal(h.sent.length, 0);
});

test('orphan, duplicate, and mismatched request responses cannot apply or clear the in-flight request', async () => {
    const h = await prime(harness());
    const duplicate = response(h);
    h.planner.action('next');
    await tick();
    assert.equal(h.planner.receive(duplicate), false);
    assert.equal(h.planner.receive(null), false);
    assert.equal(h.planner.receive({ requestId: 0, error: 'Invalid request' }), false);
    assert.equal(h.planner.receive(response(h, { actions: [op('a', 'next')] })), true);
    assert.equal(h.applied.length, 1);
});

test('capture contract errors are reported before send; no unsafe serialization defaults', async () => {
    for (const extra of [
        { instance: undefined }, { trackId: undefined }, { timelineEpoch: -1 },
        { positionSupported: undefined }, { rate: Infinity }, { position: NaN },
    ]) {
        const h = harness([player('a', extra)]);
        assert.equal(h.planner.action('next'), false);
        assert.match(h.planner.error, /Cannot capture/);
        h.planner.refresh();
        await tick();
        assert.equal(h.sent.length, 0);
    }
});
