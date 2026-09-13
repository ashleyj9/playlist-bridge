use playlist_bridge::cli::{parse_args, Args};
use playlist_bridge::playlist::Format;

fn args(strs: &[&str]) -> Vec<String> {
    strs.iter().map(|s| s.to_string()).collect()
}

#[test]
fn parse_args_ok_cases() {
    struct Case {
        name: &'static str,
        input: Vec<String>,
        expected: Args,
    }

    let cases = vec![
        Case {
            name: "plain input and output, no flags",
            input: args(&["in.m3u", "out.pls"]),
            expected: Args { input: "in.m3u".to_string(), output: "out.pls".to_string(), from: None, to: None },
        },
        Case {
            name: "--from overrides input format",
            input: args(&["--from", "m3u", "in.txt", "out.pls"]),
            expected: Args { input: "in.txt".to_string(), output: "out.pls".to_string(), from: Some(Format::M3u), to: None },
        },
        Case {
            name: "--to overrides output format",
            input: args(&["in.m3u", "--to", "pls", "out.txt"]),
            expected: Args { input: "in.m3u".to_string(), output: "out.txt".to_string(), from: None, to: Some(Format::Pls) },
        },
        Case {
            name: "both flags together, format names are case-insensitive",
            input: args(&["--from", "M3U8", "--to", "PLS", "in.txt", "out.txt"]),
            expected: Args { input: "in.txt".to_string(), output: "out.txt".to_string(), from: Some(Format::M3u), to: Some(Format::Pls) },
        },
        Case {
            name: "flags can come after the positional arguments",
            input: args(&["in.txt", "out.txt", "--from", "pls", "--to", "m3u"]),
            expected: Args { input: "in.txt".to_string(), output: "out.txt".to_string(), from: Some(Format::Pls), to: Some(Format::M3u) },
        },
        Case {
            name: "xspf is a recognized format name",
            input: args(&["--from", "xspf", "in.txt", "out.pls"]),
            expected: Args { input: "in.txt".to_string(), output: "out.pls".to_string(), from: Some(Format::Xspf), to: None },
        },
    ];

    for case in cases {
        let actual = parse_args(&case.input).unwrap_or_else(|err| panic!("case failed: {}: {}", case.name, err));
        assert_eq!(actual, case.expected, "case failed: {}", case.name);
    }
}

#[test]
fn parse_args_error_cases() {
    struct Case {
        name: &'static str,
        input: Vec<String>,
    }

    let cases = vec![
        Case { name: "no arguments at all", input: args(&[]) },
        Case { name: "only one positional argument", input: args(&["in.m3u"]) },
        Case { name: "three positional arguments", input: args(&["a", "b", "c"]) },
        Case { name: "--from with no value", input: args(&["in.m3u", "out.pls", "--from"]) },
        Case { name: "--to with no value", input: args(&["--to"]) },
        Case { name: "--from with an unknown format name", input: args(&["--from", "ogg", "in.txt", "out.pls"]) },
        Case { name: "--to with an unknown format name", input: args(&["in.m3u", "--to", "wav", "out.txt"]) },
    ];

    for case in cases {
        assert!(parse_args(&case.input).is_err(), "case should have failed: {}", case.name);
    }
}
