use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

/// A single entry in a playlist. `title` and `duration` are optional because
/// M3U, PLS, and XSPF all allow an entry to be nothing more than a path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    pub path: String,
    pub title: Option<String>,
    /// Duration in whole seconds. -1 conventionally means "unknown" in both
    /// formats, but we keep that as a real Some(-1) rather than folding it
    /// into None, since a writer still has to emit something for it.
    pub duration: Option<i64>,
    /// Extended M3U directives (e.g. `#EXTVLCOPT:network-caching=1000`) that
    /// appeared between this track's `#EXTINF` line and its path, kept
    /// verbatim including the leading `#`. Neither PLS nor XSPF has an
    /// equivalent concept, so these only survive an M3U-to-M3U round trip;
    /// converting to PLS or XSPF just drops them rather than inventing
    /// somewhere to put them.
    pub directives: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    M3u,
    Pls,
    Xspf,
}

impl Format {
    /// Picks a format from a file's extension. Returns None for anything we
    /// don't recognize so the caller can fail loudly instead of guessing.
    pub fn from_path<P: AsRef<Path>>(path: P) -> Option<Format> {
        let ext = path.as_ref().extension()?.to_str()?.to_ascii_lowercase();
        Format::from_name(&ext)
    }

    /// Picks a format from an explicit name, e.g. a `--from`/`--to` flag
    /// value. Accepts the same names as the file extensions it mirrors.
    pub fn from_name(name: &str) -> Option<Format> {
        match name.to_ascii_lowercase().as_str() {
            "m3u" | "m3u8" => Some(Format::M3u),
            "pls" => Some(Format::Pls),
            "xspf" => Some(Format::Xspf),
            _ => None,
        }
    }
}

/// Decodes raw playlist bytes into text. Modern exports are UTF-8, but a lot
/// of playlists written by older Winamp/Windows-era tools are plain Latin-1
/// (ISO-8859-1), which has no invalid byte sequences of its own and so just
/// silently misdecodes as mangled text under strict UTF-8 rather than
/// failing outright. We try UTF-8 first since it's dominant today, and only
/// fall back to Latin-1 when the bytes actually aren't valid UTF-8; a byte's
/// value there is its Unicode code point by definition, so the fallback
/// can't fail either.
pub fn decode_playlist_bytes(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_string(),
        Err(_) => bytes.iter().map(|&b| b as char).collect(),
    }
}

/// Parses M3U/M3U8 text into tracks. `#EXTM3U` is dropped as a bare header.
/// Other extended directives (`#EXTVLCOPT:`, `#EXTGRP:`, and anything else
/// starting `#EXT` that isn't `#EXTINF:`) are kept verbatim and attached to
/// whichever track comes next, since that's how players emit and read them.
/// Any other `#`-prefixed line is a plain comment and is dropped.
pub fn parse_m3u(input: &str) -> Vec<Track> {
    let mut tracks = Vec::new();
    let mut pending_meta: Option<(Option<i64>, Option<String>)> = None;
    let mut pending_directives: Vec<String> = Vec::new();

    for raw_line in input.lines() {
        let line = raw_line.trim_end_matches('\r').trim();
        if line.is_empty() {
            continue;
        }

        if let Some(rest) = line.strip_prefix("#EXTINF:") {
            // Titles are free text and may contain commas of their own, so
            // only the first comma separates duration from title.
            let (duration, title) = match rest.split_once(',') {
                Some((dur_str, title)) => {
                    let duration = dur_str.trim().parse::<i64>().ok();
                    let title = title.trim();
                    let title = if title.is_empty() { None } else { Some(title.to_string()) };
                    (duration, title)
                }
                None => (rest.trim().parse::<i64>().ok(), None),
            };
            pending_meta = Some((duration, title));
            continue;
        }

        if line == "#EXTM3U" {
            continue;
        }

        if line.starts_with("#EXT") {
            pending_directives.push(line.to_string());
            continue;
        }

        if line.starts_with('#') {
            continue;
        }

        let (duration, title) = pending_meta.take().unwrap_or((None, None));
        let directives = std::mem::take(&mut pending_directives);
        tracks.push(Track { path: line.to_string(), title, duration, directives });
    }

    tracks
}

/// Writes tracks back out as M3U. We always emit an `#EXTINF` line so the
/// output is unambiguous even when the source had none; unknown title and
/// duration become "" and -1, the same defaults most players use. Any
/// directives carried on the track (see `Track::directives`) are re-emitted
/// right after `#EXTINF` and before the path, matching where players put
/// them on write.
pub fn write_m3u(tracks: &[Track]) -> String {
    let mut out = String::from("#EXTM3U\n");
    for track in tracks {
        let duration = track.duration.unwrap_or(-1);
        let title = track.title.as_deref().unwrap_or("");
        out.push_str(&format!("#EXTINF:{},{}\n", duration, title));
        for directive in &track.directives {
            out.push_str(directive);
            out.push('\n');
        }
        out.push_str(&track.path);
        out.push('\n');
    }
    out
}

/// Parses PLS text into tracks. PLS keys are read case-insensitively and
/// entries are reassembled by their numeric suffix rather than trusting
/// `NumberOfEntries`, since that field is often stale or just wrong.
pub fn parse_pls(input: &str) -> Vec<Track> {
    let mut files: BTreeMap<u32, String> = BTreeMap::new();
    let mut titles: BTreeMap<u32, String> = BTreeMap::new();
    let mut lengths: BTreeMap<u32, i64> = BTreeMap::new();

    for raw_line in input.lines() {
        let line = raw_line.trim_end_matches('\r').trim();
        if line.is_empty() || line.starts_with('[') {
            continue;
        }

        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        let value = value.trim();

        if let Some(idx) = key.strip_prefix("file").and_then(|s| s.parse::<u32>().ok()) {
            files.insert(idx, value.to_string());
        } else if let Some(idx) = key.strip_prefix("title").and_then(|s| s.parse::<u32>().ok()) {
            if !value.is_empty() {
                titles.insert(idx, value.to_string());
            }
        } else if let Some(idx) = key.strip_prefix("length").and_then(|s| s.parse::<u32>().ok()) {
            if let Ok(len) = value.parse::<i64>() {
                lengths.insert(idx, len);
            }
        }
        // NumberOfEntries, Version, and anything else are ignored on read;
        // we recompute the count and write our own Version on the way out.
    }

    files
        .into_iter()
        .map(|(idx, path)| Track {
            path,
            title: titles.get(&idx).cloned(),
            duration: lengths.get(&idx).copied(),
            directives: Vec::new(),
        })
        .collect()
}

/// Writes tracks back out as PLS, numbering entries from 1 in list order.
pub fn write_pls(tracks: &[Track]) -> String {
    let mut out = String::from("[playlist]\n");
    for (i, track) in tracks.iter().enumerate() {
        let n = i + 1;
        out.push_str(&format!("File{}={}\n", n, track.path));
        if let Some(title) = &track.title {
            out.push_str(&format!("Title{}={}\n", n, title));
        }
        out.push_str(&format!("Length{}={}\n", n, track.duration.unwrap_or(-1)));
    }
    out.push_str(&format!("NumberOfEntries={}\n", tracks.len()));
    out.push_str("Version=2\n");
    out
}

/// Parses XSPF (XML Shareable Playlist Format) text into tracks. XSPF is
/// XML rather than the line-based text of M3U/PLS, so parsing here is a
/// small hand-rolled scan for the handful of elements we care about (see
/// `next_element`) rather than a general XML parser; nothing else in an
/// XSPF file (its exact wrapping, extension elements, attributes) matters
/// to us.
///
/// Two of XSPF's fields don't line up with the simpler M3U/PLS model:
/// - `duration` is milliseconds, not seconds, and gets divided down.
/// - track metadata is split into separate `creator` (artist) and `title`
///   elements rather than one free-text title. We combine them the same
///   way M3U's `#EXTINF` convention does, as "Artist - Title", so a
///   track's `title` field means the same thing across all three formats.
pub fn parse_xspf(input: &str) -> Vec<Track> {
    let mut tracks = Vec::new();
    let mut pos = 0;
    while let Some((block, end)) = next_element(input, pos, "track") {
        pos = end;
        let Some(location) = child_text(block, "location") else { continue };
        let creator = child_text(block, "creator").map(decode_xml_text);
        let title = child_text(block, "title").map(decode_xml_text);
        let duration = child_text(block, "duration").and_then(|s| s.parse::<i64>().ok()).map(|ms| ms / 1000);
        tracks.push(Track {
            path: location_to_path(&decode_xml_text(location)),
            title: combine_title(creator, title),
            duration,
            directives: Vec::new(),
        });
    }
    tracks
}

/// Writes tracks back out as XSPF. A `<duration>` is only emitted when we
/// have a genuine non-negative value; XSPF defines duration as a
/// non-negative integer, so the -1 "unknown" sentinel M3U/PLS use has
/// nowhere to go and is dropped rather than written as a nonsense value.
/// The combined "Artist - Title" produced by `parse_xspf` can't be
/// reliably split back apart, so this never emits a separate `<creator>`.
pub fn write_xspf(tracks: &[Track]) -> String {
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str("<playlist version=\"1\" xmlns=\"http://xspf.org/ns/0/\">\n  <trackList>\n");
    for track in tracks {
        out.push_str("    <track>\n");
        out.push_str(&format!("      <location>{}</location>\n", escape_xml(&path_to_location(&track.path))));
        if let Some(title) = &track.title {
            out.push_str(&format!("      <title>{}</title>\n", escape_xml(title)));
        }
        if let Some(duration) = track.duration {
            if duration >= 0 {
                out.push_str(&format!("      <duration>{}</duration>\n", duration * 1000));
            }
        }
        out.push_str("    </track>\n");
    }
    out.push_str("  </trackList>\n</playlist>\n");
    out
}

fn combine_title(creator: Option<String>, title: Option<String>) -> Option<String> {
    match (creator, title) {
        (Some(creator), Some(title)) => Some(format!("{} - {}", creator, title)),
        (Some(creator), None) => Some(creator),
        (None, Some(title)) => Some(title),
        (None, None) => None,
    }
}

/// `location` is a URI in XSPF. Local files are commonly written as either
/// a `file://` URI or, just as often in the wild, a bare relative path with
/// no scheme at all; both are percent-decoded, since spaces and other
/// reserved characters show up escaped either way. Anything else with a
/// recognizable scheme (`http://` and the like) is a stream URL and is left
/// exactly as written, since decoding it could change what it points at.
fn location_to_path(location: &str) -> String {
    if let Some(rest) = location.strip_prefix("file://") {
        return percent_decode(rest);
    }
    if is_url(location) {
        return location.to_string();
    }
    percent_decode(location)
}

/// The reverse of `location_to_path`. A bare relative or absolute path is
/// already a valid URI reference under RFC 3986, and every other format
/// this tool reads/writes stores paths the same bare way, so there's no
/// need to wrap local paths in a `file://` URI on the way out.
fn path_to_location(path: &str) -> String {
    path.to_string()
}

fn escape_xml(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}

/// Decodes the handful of XML entities likely to show up in track text: the
/// five predefined entities plus decimal/hex numeric character references.
/// An unrecognized or unterminated `&...;` is left exactly as it appeared
/// rather than guessed at.
fn decode_xml_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '&' {
            out.push(c);
            continue;
        }
        let mut entity = String::new();
        let mut closed = false;
        while let Some(&next) = chars.peek() {
            if next == ';' {
                chars.next();
                closed = true;
                break;
            }
            if entity.len() > 10 {
                break;
            }
            entity.push(next);
            chars.next();
        }
        if !closed {
            out.push('&');
            out.push_str(&entity);
            continue;
        }
        match entity.as_str() {
            "amp" => out.push('&'),
            "lt" => out.push('<'),
            "gt" => out.push('>'),
            "quot" => out.push('"'),
            "apos" => out.push('\''),
            _ if entity.starts_with('#') => {
                let code = entity.strip_prefix("#x").or_else(|| entity.strip_prefix("#X"));
                let parsed = match code {
                    Some(hex) => u32::from_str_radix(hex, 16).ok(),
                    None => entity[1..].parse::<u32>().ok(),
                };
                match parsed.and_then(char::from_u32) {
                    Some(ch) => out.push(ch),
                    None => {
                        out.push('&');
                        out.push_str(&entity);
                        out.push(';');
                    }
                }
            }
            _ => {
                out.push('&');
                out.push_str(&entity);
                out.push(';');
            }
        }
    }
    out
}

/// Decodes `%XX` percent-encoding. XSPF locations are URIs, so this undoes
/// escaping of spaces and other reserved characters back into the literal
/// bytes a filesystem path expects.
fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""), 16) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| input.to_string())
}

/// Returns the trimmed inner text of the first `<tag>` element in `block`,
/// or `None` if it's absent or empty. Used for the leaf elements
/// (`location`, `title`, `creator`, `duration`) inside a `<track>` block.
fn child_text<'a>(block: &'a str, tag: &str) -> Option<&'a str> {
    let (content, _) = next_element(block, 0, tag)?;
    let trimmed = content.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

/// Finds the next `<tag ...>...</tag>` element in `input` at or after
/// `from`, tolerating attributes on the opening tag and treating a
/// self-closing `<tag ... />` as having empty content. This is not a real
/// XML parser: it doesn't track nesting depth, but none of the elements
/// XSPF playlists need from us (`track`, `location`, `title`, `creator`,
/// `duration`) contain a same-named child, so a linear scan is enough.
/// Returns the element's content and the byte offset just past it, so
/// callers can resume scanning for repeated elements like `<track>`.
fn next_element<'a>(input: &'a str, from: usize, tag: &str) -> Option<(&'a str, usize)> {
    let rest = &input[from..];
    let open_marker = format!("<{}", tag);
    let mut search_at = 0;
    loop {
        let found = rest[search_at..].find(&open_marker)?;
        let tag_start = search_at + found;
        let after_name = tag_start + open_marker.len();
        let next_char = rest[after_name..].chars().next();
        let name_ends_here = matches!(next_char, Some(c) if c.is_whitespace() || c == '>' || c == '/');
        if !name_ends_here {
            search_at = after_name;
            continue;
        }

        let close_angle = after_name + rest[after_name..].find('>')?;
        let self_closing = rest[..close_angle].ends_with('/');
        if self_closing {
            return Some(("", from + close_angle + 1));
        }

        let content_start = close_angle + 1;
        let close_marker = format!("</{}>", tag);
        let close_found = rest[content_start..].find(&close_marker)?;
        let content_end = content_start + close_found;
        let element_end = content_end + close_marker.len();
        return Some((&rest[content_start..content_end], from + element_end));
    }
}

/// Returns the directory a playlist file lives in, as a base for resolving
/// its entries' relative paths. A bare filename with no directory component
/// is anchored to `.` rather than an empty path, so joins behave the same
/// as they would from a shell.
pub fn dir_of(path: &str) -> PathBuf {
    match Path::new(path).parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

/// Rewrites a track's path so that, read from `to_dir`, it still points at
/// the same file it pointed at when read from `from_dir`. This is what
/// makes converting `music/road_trip.m3u` into `out/road_trip.pls` still
/// resolve `boc/roygbiv.mp3` correctly, since the entry now has to be
/// reached from a different directory.
///
/// Absolute paths, Windows drive-letter paths, and URLs are left alone,
/// since none of those are anchored to the playlist's own location to
/// begin with.
pub fn rebase_path(path: &str, from_dir: &Path, to_dir: &Path) -> String {
    if is_url(path) || is_absolute_like(path) {
        return path.to_string();
    }

    let target = lexically_normalize(&from_dir.join(path));
    let base = lexically_normalize(to_dir);

    match relative_from(&base, &target) {
        Some(rel) => rel.to_string_lossy().into_owned(),
        None => target.to_string_lossy().into_owned(),
    }
}

fn is_url(path: &str) -> bool {
    match path.split_once("://") {
        Some((scheme, _)) => {
            !scheme.is_empty() && scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
        }
        None => false,
    }
}

/// `Path::is_absolute` only understands the host platform's own convention,
/// so a Windows drive-letter path like `C:\Music\song.mp3` reads as
/// "relative" when we're running on Unix. Playlists move between platforms
/// more often than the paths in them do, so we check for that shape too.
fn is_absolute_like(path: &str) -> bool {
    if Path::new(path).is_absolute() {
        return true;
    }
    let bytes = path.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/')
}

/// Resolves `.` and `..` components against each other without touching the
/// filesystem. The paths being rebased point at media files that don't need
/// to exist for this tool to run, so this can't be a real `canonicalize`.
fn lexically_normalize(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !result.pop() {
                    result.push("..");
                }
            }
            other => result.push(other.as_os_str()),
        }
    }
    result
}

/// Expresses `target` relative to `base`, both already lexically normalized.
/// Returns None if the two don't share an absolute/relative footing, since
/// there's no sound way to relate them without hitting the filesystem.
fn relative_from(base: &Path, target: &Path) -> Option<PathBuf> {
    if base.is_absolute() != target.is_absolute() {
        return None;
    }

    let base_parts: Vec<_> = base.components().collect();
    let target_parts: Vec<_> = target.components().collect();
    let common = base_parts.iter().zip(target_parts.iter()).take_while(|(a, b)| a == b).count();

    let mut result = PathBuf::new();
    for _ in &base_parts[common..] {
        result.push("..");
    }
    for component in &target_parts[common..] {
        result.push(component.as_os_str());
    }

    if result.as_os_str().is_empty() {
        result.push(".");
    }

    Some(result)
}
