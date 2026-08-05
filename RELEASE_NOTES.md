# Bake'n Deck 3.0.2 — mp3rgain 3.0

## Highlights

- **Updated built-in mp3rgain library from 2.10 to 3.0.** The 3.0 major release trims unused public API surface; gain application itself is unchanged and output stays bit-identical. Adapted the AAC gain call to the new API.

## Other Changes

- Restructured README top: badges, subcommand table, quick start
- Dependency updates: clap 4.6.2, quinn-proto 0.11.16, and other minor/patch bumps
- CI: bumped actions/checkout, softprops/action-gh-release, dtolnay/rust-toolchain

**Full Changelog**: https://github.com/M-Igashi/baken/compare/v3.0.1...v3.0.2
