use playlist_bridge::playlist::{parse_m3u, parse_pls, write_m3u, write_pls, Format};
use std::env;
use std::fs;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: playlist-bridge <input> <output>");
        eprintln!("format is chosen from the file extension: .m3u, .m3u8, or .pls");
        return ExitCode::FAILURE;
    }

    let input_path = &args[1];
    let output_path = &args[2];

    let Some(input_format) = Format::from_path(input_path) else {
        eprintln!("cannot tell playlist format from input extension: {}", input_path);
        return ExitCode::FAILURE;
    };
    let Some(output_format) = Format::from_path(output_path) else {
        eprintln!("cannot tell playlist format from output extension: {}", output_path);
        return ExitCode::FAILURE;
    };

    let contents = match fs::read_to_string(input_path) {
        Ok(contents) => contents,
        Err(err) => {
            eprintln!("failed to read {}: {}", input_path, err);
            return ExitCode::FAILURE;
        }
    };

    let tracks = match input_format {
        Format::M3u => parse_m3u(&contents),
        Format::Pls => parse_pls(&contents),
    };

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
