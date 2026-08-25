use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

/// A single entry in a playlist. `title` and `duration` are optional because
/// both M3U and PLS allow an entry to be nothing more than a path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    pub path: String,
    pub title: Option<String>,
    /// Duration in whole seconds. -1 conventionally means "unknown" in both
    /// formats, but we keep that as a real Some(-1) rather than folding it
    /// into None, since a writer still has to emit something for it.
    pub duration: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    M3u,
    Pls,
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
            _ => None,
        }
    }
}

/// Parses M3U/M3U8 text into tracks. Any line starting with `#` that isn't
/// `#EXTINF:` (including the `#EXTM3U` header) is treated as a comment and
/// dropped, matching how real players behave.
pub fn parse_m3u(input: &str) -> Vec<Track> {
    let mut tracks = Vec::new();
    let mut pending: Option<(Option<i64>, Option<String>)> = None;

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
            pending = Some((duration, title));
            continue;
        }

        if line.starts_with('#') {
            continue;
        }

        let (duration, title) = pending.take().unwrap_or((None, None));
        tracks.push(Track { path: line.to_string(), title, duration });
    }

    tracks
}

/// Writes tracks back out as M3U. We always emit an `#EXTINF` line so the
/// output is unambiguous even when the source had none; unknown title and
/// duration become "" and -1, the same defaults most players use.
pub fn write_m3u(tracks: &[Track]) -> String {
    let mut out = String::from("#EXTM3U\n");
    for track in tracks {
        let duration = track.duration.unwrap_or(-1);
        let title = track.title.as_deref().unwrap_or("");
        out.push_str(&format!("#EXTINF:{},{}\n", duration, title));
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
