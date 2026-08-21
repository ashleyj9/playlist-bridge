# playlist-bridge

A command-line tool that converts audio playlists between M3U/M3U8 and PLS.

## Why

M3U and M3U8 are what most modern players write, but a lot of older
software, some car head units, and a handful of streaming clients still only
speak PLS. Moving a playlist from one world to the other by hand means
manually renumbering `FileN=` lines, which nobody wants to do for a
200-track playlist. This does that conversion in one step, in either
direction.

## Usage

```
playlist-bridge <input> <output>
```

The format on each side is picked from the file extension: `.m3u` and
`.m3u8` are treated as M3U, `.pls` as PLS.

```
$ cat road_trip.m3u
#EXTM3U
#EXTINF:245,Boards of Canada - Roygbiv
music/boc/roygbiv.mp3
#EXTINF:-1,Live Radio
http://stream.example.com/live

$ playlist-bridge road_trip.m3u road_trip.pls
wrote 2 track(s) to road_trip.pls

$ cat road_trip.pls
[playlist]
File1=music/boc/roygbiv.mp3
Title1=Boards of Canada - Roygbiv
Length1=245
File2=http://stream.example.com/live
Title2=Live Radio
Length2=-1
NumberOfEntries=2
Version=2
```

Converting the other way works the same:

```
$ playlist-bridge road_trip.pls road_trip.m3u
```

## What it handles

Both formats look simple but have a handful of quirks in the wild:

- Entries with no metadata at all, just a bare path.
- `Length` / duration of `-1`, which conventionally means "unknown", not
  an error.
- PLS files where `NumberOfEntries` is missing or wrong; entries are found
  by scanning the numbered `FileN`/`TitleN`/`LengthN` keys directly.
- PLS entries written out of order, or with mixed-case keys
  (`file1` vs `File1`).
- Titles that themselves contain a comma, which would otherwise break a
  naive split on the M3U `#EXTINF:` line.
- Windows-style line endings.

These cases are covered by a table-driven test suite in
`tests/conversion.rs`.

## Building

Standard library only, no dependencies:

```
cargo build --release
```

## License

MIT, see [LICENSE](LICENSE).
