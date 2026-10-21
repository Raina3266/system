import test from 'node:test';
import assert from 'node:assert/strict';
import { Timeline } from '../logic.js';

function timeline(fields = {}, now = 0) {
    const result = new Timeline();
    result.update({ position: 10, playing: true, rate: 1, length: 200, identity: 'track-a', ...fields }, now);
    return result;
}

function close(actual, expected) {
    assert.ok(Math.abs(actual - expected) < 1e-8, `${actual} != ${expected}`);
}

test('sampled one-second positions extrapolate between samples without changing epoch', () => {
    const clock = timeline({}, 10);
    close(clock.value(10.75), 10.75);
    for (let second = 11; second <= 20; second++) {
        clock.update({ position: second }, second);
        assert.equal(clock.epoch, 0);
        close(clock.value(second + 0.5), second + 0.5);
    }
});

test('cached non-position notifications do not reanchor playback', () => {
    const clock = timeline();
    clock.update({ position: 10, sampled: false }, 0.5);
    clock.update({ position: 10, length: 210, sampled: false }, 0.9);
    close(clock.value(1), 11);
    assert.equal(clock.epoch, 0);
    clock.update({ position: 11 }, 1);
    assert.equal(clock.epoch, 0);
});

test('pause, repeated paused samples, resume, and rate changes have explicit epochs', () => {
    const clock = timeline();
    clock.update({ playing: false, position: 10, sampled: false }, 0.5);
    close(clock.value(10), 10.5);
    assert.equal(clock.epoch, 1);
    clock.update({ position: 10.5 }, 1);
    clock.update({ position: 10.5 }, 2);
    assert.equal(clock.epoch, 1);
    clock.update({ rate: 2, sampled: false }, 3);
    assert.equal(clock.epoch, 2);
    close(clock.value(4), 10.5);
    clock.update({ playing: true, sampled: false }, 4);
    close(clock.value(4.5), 11.5);
    clock.update({ position: 12.5 }, 5);
    assert.equal(clock.epoch, 3);
    close(clock.value(5.5), 13.5);
});

test('a rate-only update preserves the old-rate distance and subsequent normal samples', () => {
    const clock = timeline();
    clock.update({ rate: 2, position: 10, sampled: false }, 0.5);
    close(clock.value(1), 11.5);
    clock.update({ position: 12.5 }, 1.5);
    assert.equal(clock.epoch, 1);
    clock.update({ position: 14.5 }, 2.5);
    assert.equal(clock.epoch, 1);
    close(clock.value(3), 15.5);
});

test('repeated cached playing samples do not reset the anchor or create epochs', () => {
    const clock = timeline();
    clock.update({ position: 10 }, 0.8);
    close(clock.value(1), 11);
    assert.equal(clock.epoch, 0);
});

test('a genuine sample back at the last sampled position after a long gap is a discontinuity', () => {
    const clock = timeline();
    clock.update({ position: 10 }, 10);
    assert.equal(clock.epoch, 1);
    assert.equal(clock.value(10), 10);
});

test('unknown -1/nonfinite positions never extrapolate or become zero-position support', () => {
    for (const position of [-1, -5, NaN, Infinity, null, undefined]) {
        const clock = timeline({ position });
        assert.equal(clock.supported, false);
        assert.equal(clock.value(50), 0);
        assert.match(clock.seek(12, 50).error, /unsupported/);
        assert.equal(clock.epoch, 0);
        clock.update({ position: 0 }, 51);
        assert.equal(clock.supported, true);
        assert.equal(clock.epoch, 1);
        clock.update({ position: -1 }, 52);
        assert.equal(clock.supported, false);
        assert.equal(clock.epoch, 2);
    }
});

test('zero, reverse, and invalid rates; known and unknown length clamps', () => {
    const clock = timeline({ position: 198, rate: 2 });
    assert.equal(clock.value(10), 200);
    clock.update({ rate: -2, sampled: false }, 10);
    assert.equal(clock.value(11), 198);
    assert.equal(clock.value(200), 0);
    clock.update({ position: 5, rate: 0 }, 201);
    assert.equal(clock.value(220), 5);
    clock.update({ rate: NaN, length: 0, sampled: false }, 220);
    assert.equal(clock.value(500), 285);
    clock.update({ length: 100, sampled: false }, 500);
    assert.equal(clock.value(501), 100);
});

test('own seek ignores unrelated cached notifications AND a later advancing old sample', () => {
    const clock = timeline();
    const result = clock.seek(80, 0.2);
    assert.equal(result.seek, 'pending');
    assert.equal(result.error, null);
    assert.equal(clock.epoch, 1);
    clock.update({ position: 10, playing: true, sampled: false }, 0.3);
    close(clock.value(0.5), 80.3);
    assert.equal(clock.update({ position: 10 }, 0.5).seek, 'pending');
    assert.equal(clock.update({ position: 11 }, 1).seek, 'pending');
    close(clock.value(1), 80.8);
    const ack = clock.update({ position: 81 }, 1.2);
    assert.equal(ack.seek, 'acknowledged');
    assert.equal(ack.error, null);
    assert.equal(clock.epoch, 1);
    clock.update({ position: 82 }, 2.2);
    assert.equal(clock.epoch, 1);
    close(clock.value(2.7), 82.5);
});

test('paused seek ignores repeated old samples and accepts its delayed acknowledgement', () => {
    const clock = timeline({ playing: false });
    clock.seek(90, 0.1);
    assert.equal(clock.update({ position: 10 }, 0.2).seek, 'pending');
    assert.equal(clock.update({ position: 10 }, 1).seek, 'pending');
    assert.equal(clock.value(1), 90);
    assert.equal(clock.update({ position: 90 }, 2).seek, 'acknowledged');
    clock.update({ position: 90 }, 10);
    assert.equal(clock.value(100), 90);
    assert.equal(clock.epoch, 1);
});

test('rate/playback notifications do not mistake a cached pre-seek position for acknowledgement', () => {
    const clock = timeline();
    clock.seek(80, 0.1);
    clock.update({ position: 10, rate: 2, sampled: false }, 0.5);
    close(clock.value(1), 81.4);
    assert.equal(clock.update({ position: 11.5 }, 1).seek, 'pending');
    clock.update({ position: 11.5, playing: false, sampled: false }, 1.1);
    const expected = clock.value(1.1);
    close(clock.value(1.4), expected);
    assert.equal(clock.epoch, 3);
    assert.equal(clock.update({ position: expected }, 1.4).seek, 'acknowledged');
    assert.equal(clock.epoch, 3);
});

test('unacknowledged seek accepts even the exact repeated old sample after bounded grace', () => {
    const clock = timeline();
    clock.seek(80, 0.1);
    assert.equal(clock.update({ position: 10 }, 1).seek, 'pending');
    const result = clock.update({ position: 10 }, 1.6);
    assert.equal(result.seek, 'timeout');
    assert.match(result.error, /not acknowledged/);
    assert.equal(result.position, 10);
    assert.equal(clock.epoch, 2);
});

test('non-sampled timeout cannot roll back optimism using a cached position', () => {
    const clock = timeline();
    clock.seek(80, 0.1);
    clock.update({ position: 10, sampled: false }, 5);
    close(clock.value(5), 84.9);
    const result = clock.update({ position: 15 }, 5.1);
    assert.equal(result.seek, 'timeout');
    assert.equal(result.position, 15);
});

test('a real external seek cancels optimism immediately and advances epoch', () => {
    const clock = timeline();
    clock.seek(80, 0.1);
    const result = clock.update({ position: 140 }, 0.5);
    assert.equal(result.seek, 'cancelled');
    assert.match(result.error, /external/);
    assert.equal(result.position, 140);
    assert.equal(clock.epoch, 2);
});

test('external forward/backward seeks and paused discontinuities change epochs', () => {
    const clock = timeline();
    clock.update({ position: 60 }, 1);
    assert.equal(clock.epoch, 1);
    clock.update({ position: 5 }, 2);
    assert.equal(clock.epoch, 2);
    clock.update({ playing: false, sampled: false }, 2);
    clock.update({ position: 35 }, 3);
    assert.equal(clock.epoch, 4);
    clock.update({ position: 35 }, 4);
    assert.equal(clock.epoch, 4);
});

test('identity change invalidates the previous anchor, including while seek is pending', () => {
    const clock = timeline();
    clock.seek(80, 0.1);
    const change = clock.update({ identity: 'track-b', position: 10, sampled: false }, 0.2);
    assert.equal(change.seek, 'cancelled');
    assert.match(change.error, /identity/);
    assert.equal(clock.supported, false);
    assert.equal(clock.value(1), 0);
    assert.equal(clock.epoch, 2);
    clock.update({ position: 2 }, 1);
    assert.equal(clock.epoch, 3);
    clock.update({ identity: 'track-c', position: 0, playing: false }, 2);
    assert.equal(clock.epoch, 4, 'at most one increment per update');
    assert.equal(clock.value(3), 0);
    assert.equal(clock.supported, true);
});

test('rapid own seeks retain old native and previous optimistic trajectories', () => {
    const clock = timeline();
    clock.seek(60, 0.1);
    clock.seek(90, 0.2);
    assert.equal(clock.update({ position: 60 }, 0.4).seek, 'pending');
    assert.equal(clock.update({ position: 11 }, 1).seek, 'pending');
    assert.equal(clock.update({ position: 90.8 }, 1).seek, 'acknowledged');
    assert.equal(clock.epoch, 2);
});

test('unsupported sample cancels a pending seek; invalid seek does not change the clock', () => {
    const clock = timeline();
    for (const target of [-1, NaN, Infinity]) {
        assert.match(clock.seek(target, 0).error, /finite and nonnegative/);
        assert.equal(clock.epoch, 0);
    }
    assert.equal(clock.seek(999, 0).position, 200);
    assert.equal(clock.update({ position: -1 }, 0.5).seek, 'cancelled');
    assert.equal(clock.supported, false);
    assert.equal(clock.epoch, 2);
});

test('finite-time contract and value purity', () => {
    const clock = timeline();
    const epoch = clock.epoch;
    assert.equal(clock.value(-1), 10, 'never extrapolate backwards in time');
    assert.equal(clock.value(100), 110);
    assert.equal(clock.epoch, epoch);
    assert.throws(() => clock.value(NaN), /finite monotonic/);
    assert.throws(() => clock.update({ position: 1 }, Infinity), /finite monotonic/);
    assert.throws(() => clock.seek(1, NaN), /finite monotonic/);
});
