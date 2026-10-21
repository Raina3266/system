// GTK-independent status formatting.

const ICON = '\u{f040a}';

function isCjkIdeograph(code) {
    return (code >= 0x3400 && code <= 0x4dbf)
        || (code >= 0x4e00 && code <= 0x9fff)
        || (code >= 0xf900 && code <= 0xfaff)
        || (code >= 0x20000 && code <= 0x2ebef)
        || (code >= 0x30000 && code <= 0x3134f);
}

/** Original badge limits: 50 Unicode scalars, at most 35 CJK ideographs.
 * Latin/kana may follow the 35th ideograph until the scalar limit is reached.
 * Ellipsis is added only when a character would actually exceed a limit.
 */
export function truncateTitle(title) {
    let output = '';
    let characters = 0;
    let ideographs = 0;
    for (const character of title ?? '') {
        const cjk = isCjkIdeograph(character.codePointAt(0));
        if (characters === 50 || (cjk && ideographs === 35))
            return output + '…';
        output += character;
        characters++;
        if (cjk) ideographs++;
    }
    return output;
}

/** group.primary is an Astal snapshot OBJECT, not the Rust model's bus string.
 * Main resolves that bus before calling. Only {title, artist, identity, playing}
 * are read; no native/UI objects are required. Text and full tooltip are plain
 * unescaped strings: the eventual UI must not interpret them as markup.
 */
export function badge(group) {
    const player = group?.primary;
    if (!player || typeof player !== 'object')
        return { text: ICON, class: 'inactive', alt: 'inactive', tooltip: 'No media playing' };
    const title = (player.title || '').trim() || (player.identity || '').trim() || 'Unknown title';
    const artist = (player.artist || '').trim();
    const state = player.playing ? 'playing' : 'paused';
    return {
        text: ICON + '  ' + truncateTitle(title), class: state, alt: state,
        tooltip: title + (artist ? ' — ' + artist : ''),
    };
}

/** Floor finite nonnegative seconds to m:ss or h:mm:ss; unknown/-1 is '--:--'. */
export function formatTime(seconds) {
    if (!Number.isFinite(seconds) || seconds < 0) return '--:--';
    const total = Math.floor(seconds);
    const hours = Math.floor(total / 3600);
    const minutes = Math.floor(total / 60) % 60;
    const remainder = String(total % 60).padStart(2, '0');
    return hours
        ? `${hours}:${String(minutes).padStart(2, '0')}:${remainder}`
        : `${minutes}:${remainder}`;
}
