use playlist_bridge::playlist::{decode_playlist_bytes, dir_of, parse_m3u, parse_pls, rebase_path, write_m3u, write_pls, Track};
use std::path::{Path, PathBuf};

fn track(path: &str, title: Option<&str>, duration: Option<i64>) -> Track {
    Track { path: path.to_string(), title: title.map(str::to_string), duration, directives: Vec::new() }
}

fn track_ext(path: &str, title: Option<&str>, duration: Option<i64>, directives: &[&str]) -> Track {
    Track {
        path: path.to_string(),
        title: title.map(str::to_string),
        duration,
        directives: directives.iter().map(|s| s.to_string()).collect(),
    }
}

struct ParseCase {
    name: &'static str,
    input: &'static str,
    expected: Vec<Track>,
}

#[test]
fn parse_m3u_cases() {
    let cases = vec![
        ParseCase {
            name: "bare paths with no EXTINF at all",
            input: "song1.mp3\nsong2.mp3\n",
            expected: vec![
                track("song1.mp3", None, None),
                track("song2.mp3", None, None),
            ],
        },
        ParseCase {
            name: "unknown duration is -1, not dropped",
            input: "#EXTM3U\n#EXTINF:-1,Live Stream\nhttp://example.com/stream\n",
            expected: vec![track("http://example.com/stream", Some("Live Stream"), Some(-1))],
        },
        ParseCase {
            name: "windows line endings",
            input: "#EXTM3U\r\n#EXTINF:120,Some Title\r\nsong.mp3\r\n",
            expected: vec![track("song.mp3", Some("Some Title"), Some(120))],
        },
        ParseCase {
            name: "title containing a comma only splits on the first one",
            input: "#EXTINF:200,Artist, Feat. Someone - Title\nsong.mp3\n",
            expected: vec![track("song.mp3", Some("Artist, Feat. Someone - Title"), Some(200))],
        },
        ParseCase {
            name: "an EXTVLCOPT directive before EXTINF is kept, not treated as a path",
            input: "#EXTM3U\n#EXTVLCOPT:some-option=1\n#EXTINF:90,Track\nsong.mp3\n",
            expected: vec![track_ext("song.mp3", Some("Track"), Some(90), &["#EXTVLCOPT:some-option=1"])],
        },
        ParseCase {
            name: "EXTVLCOPT directives after EXTINF attach to that track and reset for the next",
            input: "#EXTINF:120,Track One\n#EXTVLCOPT:network-caching=1000\n#EXTVLCOPT:start-time=10\n\
                    song1.mp3\n#EXTINF:90,Track Two\nsong2.mp3\n",
            expected: vec![
                track_ext(
                    "song1.mp3",
                    Some("Track One"),
                    Some(120),
                    &["#EXTVLCOPT:network-caching=1000", "#EXTVLCOPT:start-time=10"],
                ),
                track("song2.mp3", Some("Track Two"), Some(90)),
            ],
        },
        ParseCase {
            name: "a plain # comment is dropped, not kept as a directive",
            input: "#EXTINF:90,Track\n# just a note\nsong.mp3\n",
            expected: vec![track("song.mp3", Some("Track"), Some(90))],
        },
        ParseCase {
            name: "blank lines between entries are skipped",
            input: "#EXTM3U\n\n#EXTINF:90,Track One\n\nsong1.mp3\n\n#EXTINF:95,Track Two\nsong2.mp3\n",
            expected: vec![
                track("song1.mp3", Some("Track One"), Some(90)),
                track("song2.mp3", Some("Track Two"), Some(95)),
            ],
        },
        ParseCase {
            name: "unicode titles pass through untouched",
            input: "#EXTINF:180,Café del Mar - Sueño Latino\nsong.mp3\n",
            expected: vec![track("song.mp3", Some("Café del Mar - Sueño Latino"), Some(180))],
        },
        ParseCase {
            name: "EXTINF with a trailing comma and no title",
            input: "#EXTINF:180,\nsong.mp3\n",
            expected: vec![track("song.mp3", None, Some(180))],
        },
    ];

    for case in cases {
        let actual = parse_m3u(case.input);
        assert_eq!(actual, case.expected, "case failed: {}", case.name);
    }
}

#[test]
fn parse_pls_cases() {
    let cases = vec![
        ParseCase {
            name: "entries out of order in the file are sorted by index",
            input: "[playlist]\nFile2=song2.mp3\nTitle2=Second\nFile1=song1.mp3\nTitle1=First\nNumberOfEntries=2\n",
            expected: vec![
                track("song1.mp3", Some("First"), None),
                track("song2.mp3", Some("Second"), None),
            ],
        },
        ParseCase {
            name: "missing NumberOfEntries is fine, we count keys ourselves",
            input: "[playlist]\nFile1=song1.mp3\nTitle1=Only One\nLength1=42\n",
            expected: vec![track("song1.mp3", Some("Only One"), Some(42))],
        },
        ParseCase {
            name: "keys are case-insensitive",
            input: "[playlist]\nfile1=song1.mp3\nTITLE1=Loud Title\nlEnGtH1=30\n",
            expected: vec![track("song1.mp3", Some("Loud Title"), Some(30))],
        },
        ParseCase {
            name: "an entry can be missing Length entirely",
            input: "[playlist]\nFile1=song1.mp3\nTitle1=No Duration\nNumberOfEntries=1\n",
            expected: vec![track("song1.mp3", Some("No Duration"), None)],
        },
        ParseCase {
            name: "unrelated keys are ignored",
            input: "[playlist]\nFile1=song1.mp3\nSomeRandomKey=whatever\nVersion=2\n",
            expected: vec![track("song1.mp3", None, None)],
        },
    ];

    for case in cases {
        let actual = parse_pls(case.input);
        assert_eq!(actual, case.expected, "case failed: {}", case.name);
    }
}

struct WriteCase {
    name: &'static str,
    tracks: Vec<Track>,
    expected: &'static str,
}

#[test]
fn write_m3u_cases() {
    let cases = vec![
        WriteCase {
            name: "missing title and duration fall back to empty and -1",
            tracks: vec![track("song.mp3", None, None)],
            expected: "#EXTM3U\n#EXTINF:-1,\nsong.mp3\n",
        },
        WriteCase {
            name: "full metadata is preserved",
            tracks: vec![track("song.mp3", Some("Artist - Title"), Some(210))],
            expected: "#EXTM3U\n#EXTINF:210,Artist - Title\nsong.mp3\n",
        },
        WriteCase {
            name: "directives are re-emitted between EXTINF and the path",
            tracks: vec![track_ext(
                "song.mp3",
                Some("Title"),
                Some(100),
                &["#EXTVLCOPT:network-caching=1000"],
            )],
            expected: "#EXTM3U\n#EXTINF:100,Title\n#EXTVLCOPT:network-caching=1000\nsong.mp3\n",
        },
    ];

    for case in cases {
        let actual = write_m3u(&case.tracks);
        assert_eq!(actual, case.expected, "case failed: {}", case.name);
    }
}

#[test]
fn write_pls_cases() {
    let cases = vec![
        WriteCase {
            name: "missing title is omitted, missing duration defaults to -1",
            tracks: vec![track("song.mp3", None, None)],
            expected: "[playlist]\nFile1=song.mp3\nLength1=-1\nNumberOfEntries=1\nVersion=2\n",
        },
        WriteCase {
            name: "full metadata is preserved and numbered from 1",
            tracks: vec![
                track("song1.mp3", Some("First"), Some(100)),
                track("song2.mp3", Some("Second"), Some(200)),
            ],
            expected: "[playlist]\nFile1=song1.mp3\nTitle1=First\nLength1=100\n\
                       File2=song2.mp3\nTitle2=Second\nLength2=200\n\
                       NumberOfEntries=2\nVersion=2\n",
        },
    ];

    for case in cases {
        let actual = write_pls(&case.tracks);
        assert_eq!(actual, case.expected, "case failed: {}", case.name);
    }
}

#[test]
fn decode_playlist_bytes_cases() {
    struct Case {
        name: &'static str,
        input: &'static [u8],
        expected: &'static str,
    }

    let cases = vec![
        Case { name: "plain ascii decodes unchanged", input: b"song.mp3", expected: "song.mp3" },
        Case { name: "valid utf-8 passes through untouched", input: "Café del Mar.mp3".as_bytes(), expected: "Café del Mar.mp3" },
        Case {
            name: "a leading utf-8 bom is stripped",
            input: &[0xEF, 0xBB, 0xBF, b's', b'o', b'n', b'g', b'.', b'm', b'p', b'3'],
            expected: "song.mp3",
        },
        Case {
            name: "bytes that aren't valid utf-8 fall back to latin-1",
            input: &[b'c', b'a', b'f', 0xE9, b'.', b'm', b'p', b'3'],
            expected: "café.mp3",
        },
    ];

    for case in cases {
        let actual = decode_playlist_bytes(case.input);
        assert_eq!(actual, case.expected, "case failed: {}", case.name);
    }
}

#[test]
fn dir_of_cases() {
    struct Case {
        name: &'static str,
        input: &'static str,
        expected: &'static str,
    }

    let cases = vec![
        Case { name: "bare filename has no directory", input: "playlist.m3u", expected: "." },
        Case { name: "nested path keeps its parent", input: "music/playlist.m3u", expected: "music" },
        Case { name: "deeply nested path keeps its full parent", input: "a/b/c/playlist.m3u", expected: "a/b/c" },
    ];

    for case in cases {
        let actual = dir_of(case.input);
        assert_eq!(actual, PathBuf::from(case.expected), "case failed: {}", case.name);
    }
}

#[test]
fn rebase_path_cases() {
    struct Case {
        name: &'static str,
        path: &'static str,
        from_dir: &'static str,
        to_dir: &'static str,
        expected: &'static str,
    }

    let cases = vec![
        Case {
            name: "same directory leaves a relative path untouched",
            path: "song.mp3",
            from_dir: "music",
            to_dir: "music",
            expected: "song.mp3",
        },
        Case {
            name: "output moving to a sibling directory climbs back out",
            path: "boc/roygbiv.mp3",
            from_dir: "music",
            to_dir: "out",
            expected: "../music/boc/roygbiv.mp3",
        },
        Case {
            name: "output moving into a subdirectory of the input climbs up once",
            path: "song.mp3",
            from_dir: ".",
            to_dir: "playlists",
            expected: "../song.mp3",
        },
        Case {
            name: "output moving deeper than the input shares no climb",
            path: "song.mp3",
            from_dir: "music",
            to_dir: "music/playlists",
            expected: "../song.mp3",
        },
        Case {
            name: "absolute unix paths pass through untouched",
            path: "/library/song.mp3",
            from_dir: "music",
            to_dir: "out",
            expected: "/library/song.mp3",
        },
        Case {
            name: "windows drive-letter paths pass through untouched",
            path: "C:\\Music\\song.mp3",
            from_dir: "music",
            to_dir: "out",
            expected: "C:\\Music\\song.mp3",
        },
        Case {
            name: "http urls pass through untouched",
            path: "http://stream.example.com/live",
            from_dir: "music",
            to_dir: "out",
            expected: "http://stream.example.com/live",
        },
        Case {
            name: "a redundant ./ in the source directory is ignored",
            path: "song.mp3",
            from_dir: "./music",
            to_dir: "out",
            expected: "../music/song.mp3",
        },
    ];

    for case in cases {
        let actual = rebase_path(case.path, Path::new(case.from_dir), Path::new(case.to_dir));
        assert_eq!(actual, case.expected, "case failed: {}", case.name);
    }
}
