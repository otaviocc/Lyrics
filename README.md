# lyrics

Never search for lyrics again. Point `lyrics` at your music library and it drops a `.lrc` or
`.txt` file next to every track, pulled from [LRCLIB](https://lrclib.net) or
[lrcmux](https://lrcmux.dev), free and key-less lyrics providers. Synced, timestamped lyrics
whenever they exist; plain text otherwise.

**Your audio files are never touched.** `lyrics` only reads tags and writes sidecar files next
to them.

## Install

```sh
git clone https://github.com/otaviocc/Lyrics.git
cd Lyrics
make install
```

This puts a `lyrics` binary on your `PATH` (`~/.cargo/bin`). Requires
[Rust](https://rustup.rs) to build. `make uninstall` removes it again.

## Quick start

```sh
lyrics scan ~/Music              # your whole library
lyrics scan "~/Music/Radiohead"  # just one artist or album
lyrics track "~/Music/Radiohead/OK Computer/01 Airbag.flac"   # a single file
```

Run it again whenever you like: already-synced tracks are skipped, and anything still missing
gets tried again in case it's shown up since.

Want to see what would happen first?

```sh
lyrics scan ~/Music --dry-run -v
```

## How it works

Title, artist, album, and duration come straight from your files' embedded tags, so there's no
required folder structure. If some of your files are missing tags, add `--path-fallback` to
fill the gaps in from `Artist/Album/NN Title.ext`-style paths instead of skipping them.

Titles with a version marker, like `Machine Gun Man (Acoustic) [Bonus Track]` or
`The Wizard [Live]`, are handled automatically: if the exact title comes up empty, `lyrics`
retries with the marker stripped.

## Options

```text
Selection
  --force                   Re-fetch even tracks that already have a synced .lrc
  --path-fallback           Fill in missing tags from the file path
  --no-search-fallback      Don't fall back to a fuzzy search when the exact lookup misses
  --no-marker-fallback      Don't retry with title markers like (Acoustic)/[Live] stripped
  --duration-tolerance <S>  Max duration mismatch to still accept a match  [default: 2]

Output
  --dry-run                 Show what would happen; write nothing
  --keep-plain              Keep the old .txt around after upgrading to synced
  -v, --verbose             Show per-track detail  (-vv adds request timing)
  -q, --quiet               Only print the final summary

Network
  --provider <NAME>         lrclib or lrcmux  [default: lrclib]
  --delay-ms <MS>           Delay between requests  [default: 300]
  --user-agent <STR>        Override the identifying User-Agent
```

Run `lyrics scan --help` or `lyrics track --help` for the full, always up-to-date list.

## Providers

- **`lrclib`** (default): [LRCLIB](https://lrclib.net), a community-sourced lyrics database.
- **`lrcmux`**: [lrcmux](https://lrcmux.dev), which aggregates several sources (Genius, KuGou,
  LRCLIB, Musixmatch, YouTube Music) and can turn up lyrics LRCLIB alone doesn't have.

Got `missing` for a track? Try the other provider:

```sh
lyrics track "~/Music/Artist/Album/01 Track.flac" --provider lrcmux
```

## A good citizen

Both providers are free services run by volunteers. `lyrics` identifies itself, keeps requests
sequential with a short delay between them, and backs off properly if it's ever rate-limited. If
this tool saves you time, consider supporting [LRCLIB](https://lrclib.net) or
[lrcmux](https://lrcmux.dev).

## License

[MIT](LICENSE).
