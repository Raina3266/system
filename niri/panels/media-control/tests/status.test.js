import test from 'node:test';
import assert from 'node:assert/strict';
import { badge, truncateTitle, formatTime } from '../logic.js';

const icon = '\u{f040a}';
const truncation = [
    ['empty', '', ''],
    ['exact Latin limit', 'a'.repeat(50), 'a'.repeat(50)],
    ['Latin overflow', 'a'.repeat(51), 'a'.repeat(50) + '…'],
    ['exact CJK limit', '真'.repeat(35), '真'.repeat(35)],
    ['CJK overflow', '真'.repeat(36), '真'.repeat(35) + '…'],
    ['mixed Latin limit', '真'.repeat(35) + 'abcdefghijklmnop', '真'.repeat(35) + 'abcdefghijklmno…'],
    ['mixed exact limit', '真'.repeat(35) + 'abcdefghijklmno', '真'.repeat(35) + 'abcdefghijklmno'],
    ['CJK after intervening Latin', '真'.repeat(35) + 'abc真', '真'.repeat(35) + 'abc…'],
    ['supplementary ideographs', '\u{20000}'.repeat(36), '\u{20000}'.repeat(35) + '…'],
    ['extension G ideographs', '\u{30000}'.repeat(36), '\u{30000}'.repeat(35) + '…'],
    ['emoji count as one scalar', '😀'.repeat(51), '😀'.repeat(50) + '…'],
    ['kana are not ideographs', 'あ'.repeat(51), 'あ'.repeat(50) + '…'],
];
for (const [name, input, output] of truncation) {
    test(`truncateTitle: ${name}`, () => assert.equal(truncateTitle(input), output));
}

test('original CJK range boundaries include astral scalars without splitting pairs', () => {
    const ranges = [[0x3400, 0x4dbf], [0x4e00, 0x9fff], [0xf900, 0xfaff], [0x20000, 0x2ebef], [0x30000, 0x3134f]];
    for (const [start, end] of ranges) {
        for (const code of [start, end]) {
            const character = String.fromCodePoint(code);
            assert.equal(truncateTitle(character.repeat(36)), character.repeat(35) + '…');
        }
        for (const code of [start - 1, end + 1]) {
            const character = String.fromCodePoint(code);
            assert.equal(truncateTitle(character.repeat(36)), character.repeat(36));
        }
    }
});

test('inactive badge preserves text, class, alt, and full tooltip contract', () => {
    const expected = { text: icon, class: 'inactive', alt: 'inactive', tooltip: 'No media playing' };
    assert.deepEqual(badge(null), expected);
    assert.deepEqual(badge(undefined), expected);
    assert.deepEqual(badge({ primary: null }), expected);
    assert.deepEqual(badge({ primary: 'bus-is-not-a-snapshot' }), expected);
});

test('badge uses the resolved selected primary snapshot and Astal snapshot field names', () => {
    const primary = { bus: 'native', title: ' Song ', artist: ' Artist ', identity: 'YouTube', playing: false, length: 200, position: 10 };
    assert.deepEqual(badge({ primary }), {
        text: icon + '  Song', class: 'paused', alt: 'paused', tooltip: 'Song — Artist',
    });
    primary.playing = true;
    assert.equal(badge({ primary }).class, 'playing');
    assert.equal(badge({ primary }).alt, 'playing');
});

test('badge title falls back to trimmed identity, then Unknown title', () => {
    const primary = { title: ' ', artist: '', identity: ' Browser ', playing: false };
    assert.equal(badge({ primary }).tooltip, 'Browser');
    primary.identity = ' ';
    assert.equal(badge({ primary }).tooltip, 'Unknown title');
});

test('tooltip stays full and unescaped, while badge text alone is truncated', () => {
    const title = '<&"'.repeat(30);
    const result = badge({ primary: { title, artist: ' A&B ', identity: '', playing: true } });
    assert.equal(result.tooltip, title + ' — A&B');
    assert.equal(result.text, icon + '  ' + truncateTitle(title));
    assert.deepEqual(JSON.parse(JSON.stringify(result)), result);
});

test('formatTime floors seconds and formats minute/hour boundaries', () => {
    for (const [seconds, expected] of [
        [0, '0:00'], [0.9, '0:00'], [9.99, '0:09'], [59.99, '0:59'], [60, '1:00'],
        [3599.99, '59:59'], [3600, '1:00:00'], [3661.8, '1:01:01'], [360000, '100:00:00'],
    ]) assert.equal(formatTime(seconds), expected);
});

test('formatTime preserves unknown/unsupported distinction from genuine zero', () => {
    for (const seconds of [-1, -0.1, NaN, Infinity, -Infinity, null, undefined, '12'])
        assert.equal(formatTime(seconds), '--:--');
});
