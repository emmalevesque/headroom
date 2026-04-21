use anyhow::{anyhow, Context, Result};
use id3::{TagLike, Version};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::analyzer::{AudioAnalysis, GainMethod};

pub fn create_backup_dir(base_dir: &Path) -> Result<PathBuf> {
    let backup_dir = base_dir.join("backup");
    fs::create_dir_all(&backup_dir).context("Failed to create backup directory")?;
    Ok(backup_dir)
}

pub fn backup_file(file_path: &Path, base_dir: &Path, backup_dir: &Path) -> Result<PathBuf> {
    // Preserve directory structure relative to base_dir so sibling files with
    // the same name in different folders don't collide in the backup.
    let relative_path = file_path
        .strip_prefix(base_dir)
        .unwrap_or(file_path.file_name().map(Path::new).unwrap_or(file_path));

    let backup_path = backup_dir.join(relative_path);

    if let Some(parent) = backup_path.parent() {
        fs::create_dir_all(parent).context("Failed to create backup subdirectory")?;
    }

    fs::copy(file_path, &backup_path).context("Failed to backup file")?;

    Ok(backup_path)
}

fn replace_file_with_temp(file_path: &Path, temp_path: &Path) -> Result<()> {
    fs::remove_file(file_path).context("Failed to remove original file")?;
    fs::rename(temp_path, file_path).context("Failed to rename processed file")?;
    Ok(())
}

fn path_str(path: &Path) -> Result<&str> {
    path.to_str().ok_or_else(|| anyhow!("Invalid path: {}", path.display()))
}

/// Apply gain to lossless files using ffmpeg volume filter
pub fn apply_gain_ffmpeg(file_path: &Path, gain_db: f64) -> Result<()> {
    let extension = file_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("wav");

    let temp_path = file_path.with_extension(format!("tmp.{}", extension));

    let mut args = vec![
        "-y".to_string(),
        "-i".to_string(),
        path_str(file_path)?.to_string(),
        "-af".to_string(),
        format!("volume={}dB", gain_db),
    ];

    match extension.to_lowercase().as_str() {
        "flac" => {
            args.extend(["-c:a".to_string(), "flac".to_string()]);
        }
        "aiff" | "aif" => {
            args.extend([
                "-c:a".to_string(),
                "pcm_s24be".to_string(),
                "-write_id3v2".to_string(),
                "1".to_string(),
            ]);
        }
        "wav" => {
            args.extend([
                "-c:a".to_string(),
                "pcm_s24le".to_string(),
                "-write_bext".to_string(),
                "1".to_string(),
            ]);
        }
        _ => {}
    }

    args.push(path_str(&temp_path)?.to_string());

    let output = Command::new("ffmpeg")
        .args(&args)
        .output()
        .context("Failed to execute ffmpeg for gain adjustment")?;

    if !output.status.success() {
        let _ = fs::remove_file(&temp_path);
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("ffmpeg failed: {}", stderr));
    }

    replace_file_with_temp(file_path, &temp_path)
}

/// Apply lossless gain to MP3 files using mp3rgain library (1.5dB steps)
pub fn apply_gain_mp3_native(file_path: &Path, gain_steps: i32) -> Result<()> {
    if gain_steps == 0 {
        return Ok(());
    }
    mp3rgain::apply_gain(file_path, gain_steps)
        .context("mp3rgain failed to apply MP3 gain")?;
    Ok(())
}

/// Apply lossless gain to AAC/M4A files using mp3rgain library (1.5dB steps)
pub fn apply_gain_aac_native(file_path: &Path, gain_steps: i32) -> Result<()> {
    if gain_steps == 0 {
        return Ok(());
    }
    mp3rgain::aac::apply_aac_gain(file_path, gain_steps)
        .context("mp3rgain failed to apply AAC gain")?;
    Ok(())
}

fn apply_gain_reencode(
    file_path: &Path,
    gain_db: f64,
    bitrate_kbps: Option<u32>,
    temp_ext: &str,
    default_bitrate: &str,
    encoders: &[&str],
    label: &str,
) -> Result<()> {
    let temp_path = file_path.with_extension(temp_ext);
    let bitrate = bitrate_kbps
        .map(|kbps| format!("{}k", kbps))
        .unwrap_or_else(|| default_bitrate.to_string());

    let input = path_str(file_path)?;
    let temp = path_str(&temp_path)?;

    for encoder in encoders {
        let args = [
            "-y",
            "-i",
            input,
            "-af",
            &format!("volume={}dB", gain_db),
            "-c:a",
            encoder,
            "-b:a",
            &bitrate,
            temp,
        ];

        let output = Command::new("ffmpeg")
            .args(args)
            .output()
            .with_context(|| format!("Failed to execute ffmpeg for {} re-encode", label))?;

        if output.status.success() {
            return replace_file_with_temp(file_path, &temp_path);
        }

        let _ = fs::remove_file(&temp_path);
    }

    Err(anyhow!(
        "ffmpeg {} re-encode failed with all available encoders",
        label
    ))
}

/// Apply gain to MP3 files by re-encoding (lossy, but precise control)
pub fn apply_gain_mp3_reencode(
    file_path: &Path,
    gain_db: f64,
    bitrate_kbps: Option<u32>,
) -> Result<()> {
    apply_gain_reencode(
        file_path,
        gain_db,
        bitrate_kbps,
        "tmp.mp3",
        "320k",
        &["libmp3lame"],
        "MP3",
    )
}

/// Apply gain to AAC/M4A files by re-encoding (always required, no lossless option).
/// Tries libfdk_aac first (higher quality), falls back to built-in aac.
pub fn apply_gain_aac_reencode(
    file_path: &Path,
    gain_db: f64,
    bitrate_kbps: Option<u32>,
) -> Result<()> {
    apply_gain_reencode(
        file_path,
        gain_db,
        bitrate_kbps,
        "tmp.m4a",
        "256k",
        &["libfdk_aac", "aac"],
        "AAC",
    )
}

/// Prepend the effective gain to the ID3v2 COMM frame of the file.
///
/// Only applies to formats that natively embed ID3v2 tags (MP3, AIFF).
/// Other formats (FLAC, WAV) are silently skipped.
/// Any existing comments with non-empty descriptions are preserved.
pub fn write_gain_comment(path: &Path, gain_db: f64, separator: &str) -> Result<()> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    if !matches!(ext.as_str(), "mp3" | "aif" | "aiff") {
        return Ok(());
    }

    let mut tag = id3::Tag::read_from_path(path).unwrap_or_default();

    // Build the gain prefix, always showing sign (e.g. "+2.7 dB" or "-1.5 dB")
    let gain_prefix = format!("{:+.1} dB", gain_db);

    // Read the current default comment (empty description) if present
    let existing_text = tag
        .comments()
        .find(|c| c.description.is_empty())
        .map(|c| c.text.clone())
        .unwrap_or_default();

    let new_text = if existing_text.is_empty() {
        gain_prefix
    } else {
        format!("{}{}{}", gain_prefix, separator, existing_text)
    };

    // Preserve comments with non-empty descriptions; replace the default one
    let named_comments: Vec<_> = tag
        .comments()
        .filter(|c| !c.description.is_empty())
        .cloned()
        .collect();

    tag.remove("COMM");
    for c in named_comments {
        tag.add_frame(c);
    }
    tag.add_frame(id3::frame::Comment {
        lang: "eng".to_string(),
        description: String::new(),
        text: new_text,
    });

    tag.write_to_path(path, Version::Id3v24)
        .context("Failed to write ID3v2 comment tag")?;

    Ok(())
}

/// Build the ffmpeg filter chain string for soft clipping.
/// Extracted as a pure function for testability.
fn build_soft_clip_filter(gain_db: f64, threshold_db: f64, clip_type: &str) -> String {
    let threshold_linear = 10_f64.powf(threshold_db / 20.0);
    format!(
        "volume={:.2}dB,asoftclip=type={}:threshold={:.6}",
        gain_db, clip_type, threshold_linear
    )
}

/// Apply soft clip processing via ffmpeg's `asoftclip` filter.
///
/// Boosts the signal by `gain_db` (caller computes this as `target_lufs - analysis.input_i`)
/// then applies soft saturation so peaks above `threshold_db` dBFS are shaped rather than
/// hard-clipped. All formats are re-encoded since the audio data changes.
pub fn apply_soft_clip(
    file_path: &Path,
    gain_db: f64,
    threshold_db: f64,
    clip_type: &str,
    bitrate_kbps: Option<u32>,
) -> Result<()> {
    let extension = file_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("wav");
    let ext_lower = extension.to_lowercase();
    let temp_path = file_path.with_extension(format!("tmp.{}", extension));

    let filter = build_soft_clip_filter(gain_db, threshold_db, clip_type);
    let input = path_str(file_path)?;
    let temp = path_str(&temp_path)?;

    // MP3
    if ext_lower == "mp3" {
        let bitrate = bitrate_kbps
            .map(|k| format!("{}k", k))
            .unwrap_or_else(|| "320k".to_string());
        let args = [
            "-y", "-i", input, "-af", &filter, "-c:a", "libmp3lame", "-b:a", &bitrate, temp,
        ];
        let output = Command::new("ffmpeg")
            .args(args)
            .output()
            .context("Failed to execute ffmpeg for soft clip (MP3)")?;
        if !output.status.success() {
            let _ = fs::remove_file(&temp_path);
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("ffmpeg soft clip (MP3) failed: {}", stderr));
        }
        return replace_file_with_temp(file_path, &temp_path);
    }

    // AAC — try libfdk_aac first, fall back to built-in aac
    if matches!(ext_lower.as_str(), "m4a" | "aac" | "mp4") {
        let bitrate = bitrate_kbps
            .map(|k| format!("{}k", k))
            .unwrap_or_else(|| "256k".to_string());
        for encoder in ["libfdk_aac", "aac"] {
            let args = [
                "-y", "-i", input, "-af", &filter, "-c:a", encoder, "-b:a", &bitrate, temp,
            ];
            let output = Command::new("ffmpeg").args(args).output();
            if let Ok(out) = output {
                if out.status.success() {
                    return replace_file_with_temp(file_path, &temp_path);
                }
            }
            let _ = fs::remove_file(&temp_path);
        }
        return Err(anyhow!(
            "ffmpeg AAC soft clip failed with all available encoders"
        ));
    }

    // Lossless: FLAC / AIFF / WAV — mirror apply_gain_ffmpeg codec selection
    let mut args = vec!["-y", "-i", input, "-af", &filter];
    match ext_lower.as_str() {
        "flac" => {
            args.extend(["-c:a", "flac"]);
        }
        "aiff" | "aif" => {
            args.extend(["-c:a", "pcm_s24be", "-write_id3v2", "1"]);
        }
        "wav" => {
            args.extend(["-c:a", "pcm_s24le", "-write_bext", "1"]);
        }
        _ => {}
    }
    args.push(temp);

    let output = Command::new("ffmpeg")
        .args(&args)
        .output()
        .context("Failed to execute ffmpeg for soft clip (lossless)")?;

    if !output.status.success() {
        let _ = fs::remove_file(&temp_path);
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("ffmpeg soft clip (lossless) failed: {}", stderr));
    }

    replace_file_with_temp(file_path, &temp_path)
}

pub fn process_file(
    file_path: &Path,
    analysis: &AudioAnalysis,
    base_dir: &Path,
    backup_dir: Option<&Path>,
) -> Result<()> {
    if !analysis.has_headroom() {
        return Ok(());
    }

    if let Some(backup) = backup_dir {
        backup_file(file_path, base_dir, backup).context("Backup failed")?;
    }

    match analysis.gain_method {
        GainMethod::FfmpegLossless => apply_gain_ffmpeg(file_path, analysis.effective_gain),
        GainMethod::Mp3Lossless => apply_gain_mp3_native(file_path, analysis.lossless_gain_steps),
        GainMethod::AacLossless => apply_gain_aac_native(file_path, analysis.lossless_gain_steps),
        GainMethod::Mp3Reencode => {
            apply_gain_mp3_reencode(file_path, analysis.effective_gain, analysis.bitrate_kbps)
        }
        GainMethod::AacReencode => {
            apply_gain_aac_reencode(file_path, analysis.effective_gain, analysis.bitrate_kbps)
        }
        GainMethod::None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_soft_clip_filter() {
        let f = build_soft_clip_filter(3.5, -1.0, "tanh");
        assert!(f.starts_with("volume=3.50dB"), "unexpected filter: {}", f);
        assert!(f.contains("asoftclip=type=tanh"), "unexpected filter: {}", f);
        // threshold_linear for -1.0 dBFS ≈ 0.891251
        assert!(f.contains("threshold=0.891"), "unexpected filter: {}", f);
    }

    #[test]
    fn test_build_soft_clip_filter_zero_threshold() {
        let f = build_soft_clip_filter(0.0, 0.0, "atan");
        assert!(f.contains("volume=0.00dB"), "unexpected filter: {}", f);
        assert!(f.contains("asoftclip=type=atan"), "unexpected filter: {}", f);
        // threshold_linear for 0.0 dBFS = 1.000000
        assert!(f.contains("threshold=1.000000"), "unexpected filter: {}", f);
    }
}
