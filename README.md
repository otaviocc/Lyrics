# lyrics

[![CI](https://img.shields.io/github/actions/workflow/status/otaviocc/Lyrics/ci.yml?branch=main)](https://github.com/otaviocc/Lyrics/actions/workflows/ci.yml)
[![GitHub release](https://img.shields.io/github/v/release/otaviocc/Lyrics)](https://github.com/otaviocc/Lyrics/releases/latest)
[![crates.io](https://img.shields.io/crates/v/lyrics-sidecar.svg)](https://crates.io/crates/lyrics-sidecar)
[![license](https://img.shields.io/crates/l/lyrics-sidecar.svg)](https://github.com/otaviocc/Lyrics/blob/main/LICENSE)
[![homebrew](https://img.shields.io/badge/homebrew-lyrics-blue.svg)](https://github.com/otaviocc/homebrew-tap)

Never search for lyrics again. Point `lyrics` at your music library and it drops a `.lrc` or
`.txt` file next to every track, pulled from [LRCLIB](https://lrclib.net) or
[lrcmux](https://lrcmux.dev), free and key-less lyrics providers. Synced, timestamped lyrics
whenever they exist; plain text otherwise.

**Your audio files are never touched.** `lyrics` only reads tags and writes sidecar files next
to them.

## Install

### Homebrew

```sh
brew install otaviocc/tap/lyrics
```

### Cargo

```sh
cargo install lyrics-sidecar
```

### From source

```sh
git clone https://github.com/otaviocc/Lyrics.git
cd Lyrics
make install
```

Requires [Rust](https://rustup.rs) to build. `make uninstall` removes it.

## Quick start

```sh
lyrics scan ~/Music                                           # your whole library
lyrics scan "~/Music/Radiohead"                               # just one artist or album
lyrics track "~/Music/Radiohead/OK Computer/01 Airbag.flac"   # a single file
lyrics show "Airbag" --artist "Radiohead"                     # no audio file needed
lyrics stats ~/Music                                          # coverage census, read-only
lyrics lint "~/Music/Radiohead/OK Computer/01 Airbag.lrc"     # check a sidecar's sync format
```

`show` looks up lyrics by artist and track name (no audio file required) and displays them in a
pager. Timestamps are dimmed by default; pass `--no-color` to disable that.

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

## Checking your library

Two read-only commands make no network requests and never write anything:

- **`lyrics stats <dir>`** surveys a directory tree's coverage — how many tracks are synced,
  plain, or missing, broken down by format, plus a count of orphaned sidecars (a `.lrc`/`.txt`
  left behind after its audio file was renamed or deleted). Pass `-v` to list the orphan
  paths. A fast way to check whether a `scan` is even worth running.
- **`lyrics lint <path>...`** checks one or more `.lrc` files (or directories, searched
  recursively) for format and sync problems: malformed timestamps, out-of-order or duplicate
  timestamps, non-canonical `[MM:SS.xx]` formatting, and unknown metadata tags. Useful after
  hand-editing a sidecar to fix a typo. Exits non-zero on any error, or on any warning under
  `--strict`.

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
  --no-color                Disable colored output in `show`

Network
  --provider <NAME>         lrclib or lrcmux  [default: lrclib]
  --delay-ms <MS>           Delay between requests  [default: 300]
  --max-retries <N>         Retries for 429/5xx responses before giving up  [default: 3]
  --user-agent <STR>        Override the identifying User-Agent

Config file
  --config <PATH>           Load config from this path instead of the default location
  --no-config               Ignore the config file; use only built-in defaults and CLI flags
```

Run `lyrics scan --help` or `lyrics track --help` for the full, always up-to-date list. These
apply to `scan`, `track`, and `show`; `stats` and `lint` are read-only and take no network or
selection options.

## Configuration

Persistent defaults live at `$XDG_CONFIG_HOME/lyrics/config.toml`, or
`~/.config/lyrics/config.toml` when `$XDG_CONFIG_HOME` isn't set — the same path on every
platform, including macOS. Precedence is: **built-in default → config file → CLI flag**, so a
flag on the command line always wins.

```toml
[options]
provider = "lrclib"
delay_ms = 500
path_fallback = true
keep_plain = true

[lrclib]
user_agent = "MyPrivateLyricsBot/1.0"
```

Every value option (`provider`, `delay_ms`, `max_retries`, `duration_tolerance`,
`user_agent`) and boolean flag (`path_fallback`, `keep_plain`, `no_search_fallback`,
`no_marker_fallback`, `no_color`) under `[options]` is supported. `[lrclib]`/`[lrcmux]` accept
a provider-specific `user_agent` that overrides `[options].user_agent` when that provider is
selected.

`force`, `dry_run`, `verbose`, and `quiet` are **not** configurable — a config that silently
forces every run to re-fetch, silently makes every run a no-op, or fights with itself over
verbosity is a footgun, not a convenience; those stay CLI-only.

Because a plain CLI flag can't distinguish "not passed" from "explicitly false", a boolean
enabled in the config **can't be turned back off from the CLI** — edit the config instead.
Use `--config <path>` to load from somewhere else, or `--no-config` to ignore the file
entirely for one run.

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
