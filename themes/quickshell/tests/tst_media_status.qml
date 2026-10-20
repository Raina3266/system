import QtQuick
import QtTest
import "../MediaStatus.js" as MediaStatus

TestCase {
    name: "MediaStatus"

    function test_truncation_data() {
        return [
            { tag: "exact Latin limit", input: "a".repeat(50), output: "a".repeat(50) },
            { tag: "Latin overflow", input: "a".repeat(51), output: "a".repeat(50) + "…" },
            { tag: "exact CJK limit", input: "真".repeat(35), output: "真".repeat(35) },
            { tag: "CJK overflow", input: "真".repeat(36), output: "真".repeat(35) + "…" },
            { tag: "mixed Latin limit", input: "真".repeat(35) + "abcdefghijklmnop",
              output: "真".repeat(35) + "abcdefghijklmno…" },
            { tag: "supplementary ideographs", input: "\u{20000}".repeat(36),
              output: "\u{20000}".repeat(35) + "…" },
            { tag: "emoji count as one scalar", input: "😀".repeat(51),
              output: "😀".repeat(50) + "…" },
            { tag: "kana are not ideographs", input: "あ".repeat(51),
              output: "あ".repeat(50) + "…" }
        ]
    }

    function test_truncation(data) {
        compare(MediaStatus.truncateTitle(data.input), data.output)
    }

    function test_inactive() {
        compare(MediaStatus.badge(null), {
            text: "\u{f040a}", class: "inactive", alt: "inactive", tooltip: "No media playing"
        })
    }

    function test_selectedSource() {
        const selected = { primary: {
            trackTitle: " Song ", trackArtist: " Artist ", identity: "YouTube", isPlaying: false
        } }
        compare(MediaStatus.badge(selected), {
            text: "\u{f040a}  Song", class: "paused", alt: "paused", tooltip: "Song — Artist"
        })
        selected.primary.isPlaying = true
        compare(MediaStatus.badge(selected).class, "playing")
        compare(MediaStatus.badge(selected).alt, "playing")
    }

    function test_fallbacks() {
        const group = { primary: { trackTitle: " ", trackArtist: "", identity: " Browser ", isPlaying: false } }
        compare(MediaStatus.badge(group).tooltip, "Browser")
        group.primary.identity = " "
        compare(MediaStatus.badge(group).tooltip, "Unknown title")
    }

    function test_tooltipIsNotTruncatedOrEscaped() {
        const title = "<&\"".repeat(30)
        const result = MediaStatus.badge({ primary: {
            trackTitle: title, trackArtist: " A&B ", identity: "", isPlaying: true
        } })
        compare(result.tooltip, title + " — A&B")
        compare(result.text, "\u{f040a}  " + MediaStatus.truncateTitle(title))
        compare(JSON.parse(JSON.stringify(result)), result)
    }
}
