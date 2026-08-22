use crate::playlist::Format;

pub const USAGE: &str = "usage: playlist-bridge [--from FORMAT] [--to FORMAT] <input> <output>";

/// Parsed command-line arguments, before file extensions are consulted.
/// `from`/`to` are only set when the caller passed an explicit flag, so
/// the binary can still fall back to extension detection when they're not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub input: String,
    pub output: String,
    pub from: Option<Format>,
    pub to: Option<Format>,
}

/// Parses `argv[1..]`. `--from`/`--to` let the caller override extension
/// detection, which matters for playlists that don't use a recognized
/// extension (e.g. exported from something that names them `.txt`).
pub fn parse_args(args: &[String]) -> Result<Args, String> {
    let mut from_name: Option<&str> = None;
    let mut to_name: Option<&str> = None;
    let mut positional = Vec::new();

    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--from" => {
                from_name = Some(iter.next().ok_or("--from requires a value")?.as_str());
            }
            "--to" => {
                to_name = Some(iter.next().ok_or("--to requires a value")?.as_str());
            }
            other => positional.push(other),
        }
    }

    if positional.len() != 2 {
        return Err(USAGE.to_string());
    }

    let from = from_name
        .map(|name| Format::from_name(name).ok_or_else(|| format!("unknown format: {}", name)))
        .transpose()?;
    let to = to_name
        .map(|name| Format::from_name(name).ok_or_else(|| format!("unknown format: {}", name)))
        .transpose()?;

    Ok(Args { input: positional[0].to_string(), output: positional[1].to_string(), from, to })
}
