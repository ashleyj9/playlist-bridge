use playlist_bridge::cli::{parse_args, USAGE};
use playlist_bridge::playlist::{
    decode_playlist_bytes, dir_of, parse_m3u, parse_pls, rebase_path, write_m3u, write_pls, Format, Track,
};
use std::env;
use std::fs;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    let parsed = match parse_args(&args[1..]) {
        Ok(parsed) => parsed,
        Err(err) => {
            eprintln!("{}", err);
            eprintln!("format is chosen from the file extension unless --from/--to is given");
            return ExitCode::FAILURE;
        }
    };

    let input_path = &parsed.input;
    let output_path = &parsed.output;

    let input_format = match parsed.from.or_else(|| Format::from_path(input_path)) {
        Some(format) => format,
        None => {
            eprintln!("cannot tell playlist format from input extension: {}", input_path);
            eprintln!("{}", USAGE);
            return ExitCode::FAILURE;
        }
    };
    let output_format = match parsed.to.or_else(|| Format::from_path(output_path)) {
        Some(format) => format,
        None => {
            eprintln!("cannot tell playlist format from output extension: {}", output_path);
            eprintln!("{}", USAGE);
            return ExitCode::FAILURE;
        }
    };

    let contents = match fs::read(input_path) {
        Ok(bytes) => decode_playlist_bytes(&bytes),
        Err(err) => {
            eprintln!("failed to read {}: {}", input_path, err);
            return ExitCode::FAILURE;
        }
    };

    let tracks = match input_format {
        Format::M3u => parse_m3u(&contents),
        Format::Pls => parse_pls(&contents),
    };

    // Entries with a relative path are anchored to the input playlist's own
    // directory; if the output file lands somewhere else, those paths have
    // to be rewritten or they'll point at the wrong place.
    let from_dir = dir_of(input_path);
    let to_dir = dir_of(output_path);
    let tracks: Vec<Track> = tracks
        .into_iter()
        .map(|track| Track { path: rebase_path(&track.path, &from_dir, &to_dir), ..track })
        .collect();

    let output = match output_format {
        Format::M3u => write_m3u(&tracks),
        Format::Pls => write_pls(&tracks),
    };

    if let Err(err) = fs::write(output_path, output) {
        eprintln!("failed to write {}: {}", output_path, err);
        return ExitCode::FAILURE;
    }

    eprintln!("wrote {} track(s) to {}", tracks.len(), output_path);
    ExitCode::SUCCESS
}
