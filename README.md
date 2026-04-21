# headroom

Audio loudness analyzer and gain adjustment tool for mastering and DJ workflows.

## What is this?

**headroom** simulates the behavior of Rekordbox's Auto Gain feature, but with a key difference: it identifies files with available headroom (True Peak below the target ceiling) and applies gain adjustment **without using a limiter**.

This tool is designed for DJs and producers who want to maximize loudness while preserving dynamics, ensuring tracks hit the optimal True Peak ceiling without clipping.

## Key Features

- **Single binary**: mp3rgain is built-in as a library — only ffmpeg required as external dependency
- **Smart True Peak ceiling**: Based on AES TD1008, uses -0.5 dBTP for high-quality files, -1.0 dBTP for low-bitrate
- **Multiple processing methods**: ffmpeg for lossless formats, built-in mp3rgain for lossless MP3/AAC gain, ffmpeg re-encode for precise gain
- **Soft clip mode**: Alternative to pure gain — boost to a target LUFS-I and shape peaks with ffmpeg's `asoftclip` filter (ideal for DJ/club mastering targets like -7.5 LUFS)
- **Non-destructive workflow**: Original files are backed up before processing
- **Metadata preservation**: Audio tags (ID3v2, Vorbis comment, BWF) are preserved during processing, and files are overwritten in place so Rekordbox cue points and other external metadata remain linked
- **ID3v2 comment tagging**: Optionally prepend the applied gain to the COMM field (MP3/AIFF), useful for tracking processing history in DJ software
- **No limiter (gain mode)**: Pure gain adjustment only — dynamics are preserved
- **Interactive CLI**: Guided step-by-step process with two-stage confirmation
- **Scriptable CLI**: Non-interactive mode for pipelines and CI (paths, globs, and flags)
- **Config file**: Set your preferred defaults in `~/.headroom.toml`

## Processing Methods

### Gain Mode (default)

headroom selects the optimal method for each file based on format and headroom:

| Format | Method | Precision | Quality Loss |
|--------|--------|-----------|--------------|
| FLAC, AIFF, WAV | ffmpeg | Arbitrary | None |
| MP3, AAC/M4A | mp3rgain (built-in) | 1.5dB steps | **None** (global_gain modification) |
| MP3, AAC/M4A | ffmpeg re-encode | Arbitrary | Inaudible at ≥256kbps |

#### Three-Tier Approach for Lossy Formats (MP3/AAC)

Each MP3 and AAC/M4A file is categorized into one of three tiers:

1. **Native Lossless** — ≥1.5 dB headroom to bitrate-aware ceiling
   - Truly lossless global_gain header modification in 1.5dB steps
   - Uses built-in [mp3rgain](https://github.com/M-Igashi/mp3rgain) library
   - Applied automatically (no user confirmation needed)

2. **Re-encode** — headroom exists but <1.5 dB to ceiling
   - Uses ffmpeg for arbitrary precision gain
   - MP3: `libmp3lame` / AAC: `libfdk_aac` (falls back to built-in `aac`)
   - Preserves original bitrate; requires explicit user confirmation

3. **Skip** — no headroom available

### Soft Clip Mode (`--soft-clip`)

An alternative to the gain-only path. Instead of stopping at the True Peak ceiling, soft clip mode:

1. Boosts the signal to a target **LUFS-I** loudness (e.g. -7.5 LUFS for DJ/club use, -14.0 LUFS for streaming)
2. Shapes any peaks above a dBFS threshold using ffmpeg's `asoftclip` filter — a smooth saturation curve rather than a hard limiter

All formats are re-encoded since the audio data changes. The filter chain applied is:

```
volume={boost}dB,asoftclip=type={type}:threshold={linear}
```

Available clip curve types: `tanh` (default), `atan`, `cubic`, `exp`, `alg`, `quintic`, `sin`, `erf`

## True Peak Ceiling (Gain Mode)

Based on [AES TD1008](https://www.aes.org/technical/documentDownloads.cfm?docID=731) recommendations:

| Bitrate | Ceiling | Native lossless requires |
|---------|---------|------------------------|
| Lossless (FLAC, AIFF, WAV) | **-0.5 dBTP** | — |
| Lossy ≥256kbps | **-0.5 dBTP** | TP ≤ -2.0 dBTP |
| Lossy <256kbps | **-1.0 dBTP** | TP ≤ -2.5 dBTP |

## How It Works

### Gain Mode
1. Scans the target for audio files (FLAC, AIFF, WAV, MP3, AAC/M4A)
2. Measures LUFS-I and True Peak using ffmpeg's `loudnorm` filter
3. Categorizes files by processing method and displays the report
4. Two-stage confirmation (interactive) or processes immediately (scriptable):
   - First: apply lossless gain (lossless files + native MP3/AAC)
   - Second: optionally re-encode MP3/AAC needing precise gain
5. Creates backups and processes files

### Soft Clip Mode
1. Scans and analyzes all audio files
2. Filters files that are below the target LUFS-I
3. Displays a soft clip report showing per-file boost required
4. Boosts + soft clips each file via ffmpeg

## Installation

headroom requires ffmpeg. Package managers install it automatically.

| Platform | Command |
|----------|---------|
| **macOS (Homebrew)** | `brew install M-Igashi/tap/headroom` |
| **Windows (winget)** | `winget install M-Igashi.headroom` |
| **Arch Linux (AUR)** | `yay -S headroom-bin` |
| **Cargo** | `cargo install headroom` (ffmpeg must be installed separately) |

Pre-built binaries are available on the [Releases](https://github.com/M-Igashi/headroom/releases) page (ffmpeg must be installed separately).

### Build from Source

```bash
git clone https://github.com/M-Igashi/headroom.git
cd headroom
cargo build --release
```

## Usage

### Interactive Mode

Run without arguments to use the guided workflow in the current directory:

```bash
cd ~/Music/DJ-Tracks
headroom
```

The tool will guide you through:
1. Scanning and analyzing all audio files
2. Reviewing the categorized report
3. Choosing between gain mode, soft clip mode, or tag-only
4. Confirming lossless processing (gain mode) or soft clip parameters
5. Optionally enabling MP3/AAC re-encoding (gain mode)
6. Creating backups (recommended)

### Scriptable Mode

Pass paths, globs, or flags to run non-interactively (useful for pipelines and scripts):

```bash
# Analyze files without modifying them
headroom --analyze-only ~/Music/DJ-Tracks

# Apply lossless gain to all files in the current directory
headroom --lossless

# Soft clip a folder to -7.5 LUFS (e.g. club/DJ mastering target)
headroom --soft-clip --soft-clip-target -7.5 ~/Music/DJ-Tracks

# Soft clip to -7.5 LUFS with a tighter threshold and atan curve
headroom --soft-clip --soft-clip-target -7.5 --soft-clip-threshold -2.0 --soft-clip-type atan ~/Music/DJ-Tracks

# Soft clip to -7.5 LUFS and write the boost to the ID3v2 comment tag
headroom --soft-clip --soft-clip-target -7.5 --tag-comment ~/Music/DJ-Tracks

# Apply lossless gain + re-encode lossy files with a backup
headroom --lossless --reencode --backup ~/Music/DJ-Tracks

# Process specific files or glob patterns
headroom --lossless --no-report track1.mp3 track2.flac "./albums/**/*.mp3"
```

### ID3v2 Comment Tagging

Write the applied gain to the `COMM` frame (MP3 and AIFF only):

```bash
# Prepend gain to comment after processing
headroom --lossless --tag-comment ~/Music/DJ-Tracks

# Write suggested gain to comment WITHOUT applying gain to audio
headroom --tag-comment-only ~/Music/DJ-Tracks
```

The gain is prepended as `+2.7 dB | <existing comment>`. The separator can be customized in the config file.

### Flags Reference

| Flag | Default | Description |
|------|---------|-------------|
| `--lossless` / `--no-lossless` | on | Apply lossless gain adjustment |
| `--reencode` / `--no-reencode` | off | Re-encode MP3/AAC for precise gain |
| `--soft-clip` | off | Boost to target LUFS-I with soft clipping (replaces gain mode) |
| `--soft-clip-target LUFS` | -14.0 | Target integrated loudness for soft clip mode |
| `--soft-clip-threshold DBFS` | -1.0 | dBFS point at which clipping begins |
| `--soft-clip-type TYPE` | tanh | Clip curve: tanh, atan, cubic, exp, alg, quintic, sin, erf |
| `--analyze-only` | off | Analyze and report only, do not modify files |
| `--tag-comment` | off | Prepend effective gain to ID3v2 COMM field |
| `--tag-comment-only` | off | Write suggested gain to COMM without applying audio gain |
| `--no-tag-comment` | — | Override config default to skip comment tagging |
| `--backup [DIR]` | off | Create backup before processing (default dir: `<target>/backup`) |
| `--no-backup` | — | Override config default to skip backup |
| `--report [PATH]` / `--no-report` | on | Generate CSV report (default: `<target>/headroom_report_*.csv`) |

Run `headroom --help` for the full flag reference.

## Configuration File

Create `~/.headroom.toml` to set persistent defaults:

```toml
[comment]
# String inserted between the gain value and any existing comment text
separator = " | "

[defaults]
# Apply lossless gain by default in scriptable mode
lossless    = true
# Apply re-encoding by default in scriptable mode
reencode    = false
# Prepend gain to ID3v2 comment by default
tag_comment = false
# Create a backup by default before processing
backup      = false
# Generate a CSV report by default
report      = true
```

**Precedence:** explicit CLI flag > config default > built-in default

Use `--no-tag-comment` or `--no-backup` to override a `true` config default for a single run.

## Output

### Gain Mode CSV Report

| Filename | Format | Bitrate (kbps) | LUFS | True Peak (dBTP) | Target (dBTP) | Headroom (dB) | Method | Effective Gain (dB) |
|----------|--------|----------------|------|------------------|---------------|---------------|--------|---------------------|
| track01.flac | Lossless | - | -13.3 | -3.2 | -0.5 | +2.7 | ffmpeg | +2.7 |
| track04.mp3 | MP3 | 320 | -14.0 | -5.5 | -2.0 | +3.5 | mp3rgain | +3.0 |
| track06.mp3 | MP3 | 320 | -12.0 | -1.5 | -0.5 | +1.0 | re-encode | +1.0 |

### Soft Clip CSV Report

| Filename | Format | Bitrate (kbps) | LUFS | Target LUFS | Boost (dB) | Threshold (dBFS) | Clip Type |
|----------|--------|----------------|------|-------------|------------|-----------------|-----------|
| track01.flac | Lossless | - | -13.3 | -7.5 | +5.8 | -1.0 | tanh |
| track04.mp3 | MP3 | 320 | -14.0 | -7.5 | +6.5 | -1.0 | tanh |

### Backup Structure

```
./
├── track01.flac             ← Modified
├── track04.mp3              ← Modified
├── subfolder/
│   └── track06.mp3          ← Modified
└── backup/                  ← Created by headroom
    ├── track01.flac         ← Original
    ├── track04.mp3          ← Original
    └── subfolder/
        └── track06.mp3      ← Original
```

## Important Notes

- **Files are overwritten in place** after backup — Rekordbox metadata remains linked
- **Gain mode**: Only files with positive effective gain are shown and processed
- **Soft clip mode**: Only files below the target LUFS-I are processed
- MP3/AAC native lossless requires at least **1.5dB headroom** to be processed
- MP3/AAC re-encoding is **opt-in** and requires explicit confirmation
- ID3v2 comment tagging applies to **MP3 and AIFF only**; other formats are skipped silently
- macOS resource fork files (`._*`) are automatically ignored
- ffmpeg ≥ 4.4 is required for the `asoftclip` filter (soft clip mode)

## Technical Details

### Why 1.5dB Steps?

Both MP3 and AAC store a "global_gain" value as an integer. Each ±1 increment changes the gain by `2^(1/4)` = **±1.5 dB**. This is a format-level constraint, not a tool limitation.

headroom uses the built-in [mp3rgain](https://github.com/M-Igashi/mp3rgain) library to directly modify this field — no decoding or re-encoding involved.

### Bitrate-Aware Ceiling for Native Lossless

Since native lossless gain only works in 1.5dB steps, at least 1.5dB of headroom to the target ceiling is required:
- **≥256kbps**: Target -0.5 dBTP → requires TP ≤ -2.0 dBTP
- **<256kbps**: Target -1.0 dBTP → requires TP ≤ -2.5 dBTP

Example: 320kbps file at -3.5 dBTP gets 2 steps (+3.0dB) → -0.5 dBTP (optimal)

### Re-encode Quality

At ≥256kbps, re-encoding introduces quantization noise below -90dB — far below audible threshold. Only gain is applied (no EQ, compression, or dynamics processing), and original bitrate is preserved.

### Soft Clip Algorithm

The ffmpeg `asoftclip` filter applies smooth saturation instead of hard clipping. The `threshold` parameter (converted from dBFS to linear: `10^(dBFS/20)`) sets where shaping begins. The `type` selects the curve shape — `tanh` is the default and produces a smooth, musical-sounding saturation.

## License

MIT
