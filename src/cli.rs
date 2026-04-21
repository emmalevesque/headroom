use anyhow::{Context, Result};
use clap::Parser;
use console::{style, Style};
use dialoguer::{theme::ColorfulTheme, Confirm, Input};
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;
use std::path::{Path, PathBuf};

use crate::analyzer::{self, AudioAnalysis, GainMethod, MIN_EFFECTIVE_GAIN};
use crate::args::Cli;
use crate::processor;
use crate::report::{self, AnalysisSummary};
use crate::scanner;

pub fn run() -> Result<()> {
    let cli = Cli::parse();

    print_banner();

    analyzer::check_ffmpeg()?;

    if cli.is_non_interactive() {
        run_scriptable(&cli)
    } else {
        run_interactive()
    }
}

fn run_interactive() -> Result<()> {
    let target_dir = std::env::current_dir().context("Failed to get current directory")?;

    println!(
        "{} Target directory: {}",
        style("▸").cyan(),
        style(target_dir.display()).bold()
    );

    let files = scanner::scan_audio_files(&target_dir);

    if files.is_empty() {
        println!("\n{} No audio files found", style("⚠").yellow());
        println!(
            "  Supported formats: {}",
            scanner::get_supported_extensions().join(", ")
        );
        return Ok(());
    }

    println!(
        "\n{} Found {} audio files",
        style("✓").green(),
        style(files.len()).cyan()
    );

    let all_analyses = analyze_files(&files)?;

    let summary = AnalysisSummary::from_analyses(&all_analyses);

    if !summary.has_processable() {
        println!(
            "\n{} No files with enough headroom found.",
            style("ℹ").blue()
        );
        println!("  All files are already at or above the target ceiling.");
        return Ok(());
    }

    report::print_analysis_report(&all_analyses);

    let processable_analyses: Vec<_> = all_analyses
        .iter()
        .filter(|a| a.has_headroom())
        .collect();

    let csv_path = report::generate_csv(&processable_analyses, &target_dir, None)?;
    println!(
        "{} Report saved: {}",
        style("✓").green(),
        csv_path.display()
    );

    let has_lossless = summary.total_lossless() > 0;
    let has_reencode = summary.total_reencode() > 0;

    let tag_only = Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt("Write suggested gain to comment tags only (no audio change)?")
        .default(false)
        .interact()?;

    if tag_only {
        let files_to_tag: Vec<_> = all_analyses.iter().filter(|a| a.has_headroom()).collect();
        tag_files_only(&files_to_tag)?;
        println!(
            "\n{} Done! Gain tag written to {} files (no audio modified).",
            style("✓").green().bold(),
            files_to_tag.len()
        );
        return Ok(());
    }

    let use_soft_clip = Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt("Use soft clip mode instead? (boost to target LUFS-I with soft clipping)")
        .default(false)
        .interact()?;

    if use_soft_clip {
        let target_lufs: f64 = Input::with_theme(&ColorfulTheme::default())
            .with_prompt("Target LUFS-I")
            .default(-14.0)
            .interact_text()?;
        let threshold_db: f64 = Input::with_theme(&ColorfulTheme::default())
            .with_prompt("Clip threshold (dBFS)")
            .default(-1.0)
            .interact_text()?;
        let clip_type: String = Input::with_theme(&ColorfulTheme::default())
            .with_prompt("Clip type (tanh/atan/cubic/exp/alg/quintic/sin/erf)")
            .default("tanh".to_string())
            .interact_text()?;

        let candidates = filter_soft_clip_candidates(&all_analyses, target_lufs);
        if candidates.is_empty() {
            println!(
                "{} All files already at or above {:.1} LUFS-I.",
                style("ℹ").blue(),
                target_lufs
            );
            return Ok(());
        }

        report::print_soft_clip_report(&candidates, target_lufs, threshold_db, &clip_type);

        let create_backup = Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt("Create backup before processing?")
            .default(true)
            .interact()?;
        let backup_dir = if create_backup {
            let dir = processor::create_backup_dir(&target_dir)?;
            println!("{} Backup directory: {}", style("✓").green(), dir.display());
            Some(dir)
        } else {
            None
        };

        let tag_comment = Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt("Prepend effective gain to ID3v2 comment field?")
            .default(false)
            .interact()?;

        soft_clip_files(
            &candidates,
            target_lufs,
            threshold_db,
            &clip_type,
            &target_dir,
            backup_dir.as_deref(),
            tag_comment,
        )?;

        println!(
            "\n{} Done! {} {} soft-clipped to {:.1} LUFS-I.",
            style("✓").green().bold(),
            candidates.len(),
            if candidates.len() == 1 { "file" } else { "files" },
            target_lufs
        );
        return Ok(());
    }

    if has_lossless && !prompt_lossless_processing(&summary)? {
        println!("Done. No files were modified.");
        return Ok(());
    }

    let allow_reencode = if has_reencode {
        prompt_reencode_processing(&summary)?
    } else {
        false
    };

    let create_backup = Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt("Create backup before processing?")
        .default(true)
        .interact()?;

    let backup_dir = if create_backup {
        let dir = processor::create_backup_dir(&target_dir)?;
        println!("{} Backup directory: {}", style("✓").green(), dir.display());
        Some(dir)
    } else {
        None
    };

    let tag_comment = Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt("Prepend effective gain to ID3v2 comment field?")
        .default(false)
        .interact()?;

    let files_to_process: Vec<_> = all_analyses
        .iter()
        .filter(|a| a.has_headroom() && (!a.requires_reencode() || allow_reencode))
        .collect();

    if files_to_process.is_empty() {
        println!("No files to process.");
        return Ok(());
    }

    process_files(&files_to_process, &target_dir, backup_dir.as_deref(), tag_comment)?;

    print_final_summary(&files_to_process);

    Ok(())
}

fn run_scriptable(cli: &Cli) -> Result<()> {
    let (files, base_dir) = if cli.paths.is_empty() {
        let cwd = std::env::current_dir().context("Failed to get current directory")?;
        (scanner::scan_audio_files(&cwd), cwd)
    } else {
        let files = scanner::resolve_inputs(&cli.paths)?;
        let base = common_base_dir(&files)
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."));
        (files, base)
    };

    if files.is_empty() {
        println!("{} No audio files matched.", style("⚠").yellow());
        println!(
            "  Supported formats: {}",
            scanner::get_supported_extensions().join(", ")
        );
        return Ok(());
    }

    println!(
        "{} Found {} audio files",
        style("✓").green(),
        style(files.len()).cyan()
    );

    let all_analyses = analyze_files(&files)?;

    let summary = AnalysisSummary::from_analyses(&all_analyses);

    if !summary.has_processable() {
        println!(
            "\n{} No files with enough headroom found.",
            style("ℹ").blue()
        );
        return Ok(());
    }

    report::print_analysis_report(&all_analyses);

    let processable_analyses: Vec<_> = all_analyses.iter().filter(|a| a.has_headroom()).collect();

    if cli.report_enabled() {
        let explicit_path = cli.report.as_ref().and_then(|p| {
            if p.as_os_str().is_empty() {
                None
            } else {
                Some(p.as_path())
            }
        });
        let csv_path = report::generate_csv(&processable_analyses, &base_dir, explicit_path)?;
        println!(
            "{} Report saved: {}",
            style("✓").green(),
            csv_path.display()
        );
    }

    if cli.analyze_only {
        println!("{} Analyze-only mode; no files modified.", style("ℹ").blue());
        return Ok(());
    }

    if cli.tag_comment_only {
        let files_to_tag: Vec<_> = all_analyses.iter().filter(|a| a.has_headroom()).collect();
        if files_to_tag.is_empty() {
            println!("{} No files with enough headroom to tag.", style("ℹ").blue());
            return Ok(());
        }
        tag_files_only(&files_to_tag)?;
        println!(
            "\n{} Done! Gain tag written to {} files (no audio modified).",
            style("✓").green().bold(),
            files_to_tag.len()
        );
        return Ok(());
    }

    if cli.soft_clip {
        let candidates = filter_soft_clip_candidates(&all_analyses, cli.soft_clip_target);

        if candidates.is_empty() {
            println!(
                "{} All files already at or above {:.1} LUFS-I.",
                style("ℹ").blue(),
                cli.soft_clip_target
            );
            return Ok(());
        }

        report::print_soft_clip_report(
            &candidates,
            cli.soft_clip_target,
            cli.soft_clip_threshold,
            &cli.soft_clip_type,
        );

        if cli.report_enabled() {
            let explicit_path = cli.report.as_ref().and_then(|p| {
                if p.as_os_str().is_empty() { None } else { Some(p.as_path()) }
            });
            let csv_path = report::generate_soft_clip_csv(
                &candidates,
                cli.soft_clip_target,
                cli.soft_clip_threshold,
                &cli.soft_clip_type,
                &base_dir,
                explicit_path,
            )?;
            println!("{} Report saved: {}", style("✓").green(), csv_path.display());
        }

        let backup_dir = if let Some(path) = &cli.backup {
            let dir = if path.as_os_str().is_empty() {
                processor::create_backup_dir(&base_dir)?
            } else {
                std::fs::create_dir_all(path).context("Failed to create backup directory")?;
                path.clone()
            };
            println!("{} Backup directory: {}", style("✓").green(), dir.display());
            Some(dir)
        } else {
            None
        };

        soft_clip_files(
            &candidates,
            cli.soft_clip_target,
            cli.soft_clip_threshold,
            &cli.soft_clip_type,
            &base_dir,
            backup_dir.as_deref(),
            cli.tag_comment,
        )?;

        println!(
            "\n{} Done! {} {} soft-clipped to {:.1} LUFS-I.",
            style("✓").green().bold(),
            candidates.len(),
            if candidates.len() == 1 { "file" } else { "files" },
            cli.soft_clip_target
        );
        return Ok(());
    }

    let lossless_on = cli.lossless_enabled();
    let reencode_on = cli.reencode_enabled();

    let files_to_process: Vec<_> = all_analyses
        .iter()
        .filter(|a| {
            if !a.has_headroom() {
                return false;
            }
            if a.requires_reencode() {
                reencode_on
            } else {
                lossless_on
            }
        })
        .collect();

    if files_to_process.is_empty() {
        println!("{} No files to process with current flags.", style("ℹ").blue());
        return Ok(());
    }

    let backup_dir = if let Some(path) = &cli.backup {
        let dir = if path.as_os_str().is_empty() {
            processor::create_backup_dir(&base_dir)?
        } else {
            std::fs::create_dir_all(path).context("Failed to create backup directory")?;
            path.clone()
        };
        println!("{} Backup directory: {}", style("✓").green(), dir.display());
        Some(dir)
    } else {
        None
    };

    process_files(&files_to_process, &base_dir, backup_dir.as_deref(), cli.tag_comment)?;

    print_final_summary(&files_to_process);

    Ok(())
}

fn common_base_dir(files: &[PathBuf]) -> Option<PathBuf> {
    let mut iter = files.iter().filter_map(|f| f.parent().map(Path::to_path_buf));
    let first = iter.next()?;
    let base = iter.fold(first, |acc, p| common_prefix(&acc, &p));
    Some(base)
}

fn common_prefix(a: &Path, b: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for (x, y) in a.components().zip(b.components()) {
        if x == y {
            out.push(x);
        } else {
            break;
        }
    }
    out
}

fn print_final_summary(files_to_process: &[&AudioAnalysis]) {
    println!(
        "\n{} Done! {} files processed.",
        style("✓").green().bold(),
        files_to_process.len()
    );

    for (method, label) in [
        (GainMethod::FfmpegLossless, "lossless files (ffmpeg)"),
        (GainMethod::Mp3Lossless, "MP3 files (native, lossless)"),
        (GainMethod::AacLossless, "AAC/M4A files (native, lossless)"),
        (GainMethod::Mp3Reencode, "MP3 files (re-encoded)"),
        (GainMethod::AacReencode, "AAC/M4A files (re-encoded)"),
    ] {
        let count = files_to_process
            .iter()
            .filter(|a| a.gain_method == method)
            .count();
        if count > 0 {
            println!("  {} {} {}", style("•").dim(), count, label);
        }
    }
}

fn prompt_lossless_processing(summary: &AnalysisSummary) -> Result<bool> {
    let mut prompt_parts = Vec::new();

    if summary.lossless_count > 0 {
        prompt_parts.push(format!("{} lossless", summary.lossless_count));
    }
    if summary.mp3_lossless_count > 0 {
        prompt_parts.push(format!(
            "{} MP3 (lossless gain)",
            summary.mp3_lossless_count
        ));
    }
    if summary.aac_lossless_count > 0 {
        prompt_parts.push(format!(
            "{} AAC/M4A (lossless gain)",
            summary.aac_lossless_count
        ));
    }

    let prompt = format!(
        "Apply lossless gain adjustment to {} files?",
        prompt_parts.join(" + ")
    );

    Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt(&prompt)
        .default(false)
        .interact()
        .map_err(Into::into)
}

fn prompt_reencode_processing(summary: &AnalysisSummary) -> Result<bool> {
    let mut reencode_parts = Vec::new();
    if summary.mp3_reencode_count > 0 {
        reencode_parts.push(format!("{} MP3", summary.mp3_reencode_count));
    }
    if summary.aac_reencode_count > 0 {
        reencode_parts.push(format!("{} AAC/M4A", summary.aac_reencode_count));
    }

    println!(
        "\n{} {} files have headroom but require re-encoding for precise gain.",
        style("ℹ").magenta(),
        reencode_parts.join(" + ")
    );
    println!(
        "  {} Re-encoding causes minor quality loss (inaudible at 256kbps+)",
        style("•").dim()
    );
    println!("  {} Original bitrate will be preserved", style("•").dim());

    Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt("Also process these files with re-encoding?")
        .default(false)
        .interact()
        .map_err(Into::into)
}

fn print_banner() {
    let banner_style = Style::new().cyan().bold();
    let version = env!("CARGO_PKG_VERSION");
    let title = format!("headroom v{}", version);
    let padding = (37 - title.len() - 2) / 2;
    let title_line = format!(
        "│{:padding$}{}{:padding$}│",
        "",
        title,
        "",
        padding = padding
    );
    // Ensure exactly 39 chars wide
    let title_line = format!("{:<39}", title_line);
    println!();
    println!(
        "{}",
        banner_style.apply_to("╭─────────────────────────────────────╮")
    );
    println!("{}", banner_style.apply_to(&title_line));
    println!(
        "{}",
        banner_style.apply_to("│   Audio Loudness Analyzer & Gain    │")
    );
    println!(
        "{}",
        banner_style.apply_to("╰─────────────────────────────────────╯")
    );
    println!();
}

fn analyze_files(files: &[PathBuf]) -> Result<Vec<AudioAnalysis>> {
    let pb = ProgressBar::new(files.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} Analyzing... [{bar:40.cyan/blue}] {pos}/{len}")
            .unwrap()
            .progress_chars("█▓░"),
    );

    // par_iter preserves input order in the collected Vec, so indexing is unnecessary.
    let results: Vec<Result<AudioAnalysis, (PathBuf, anyhow::Error)>> = files
        .par_iter()
        .map(|file| {
            let result = analyzer::analyze_file(file).map_err(|e| (file.clone(), e));
            pb.inc(1);
            result
        })
        .collect();

    pb.finish_and_clear();

    let mut analyses = Vec::with_capacity(results.len());
    for result in results {
        match result {
            Ok(a) => analyses.push(a),
            Err((path, e)) => println!(
                "{} Failed to analyze {}: {}",
                style("⚠").yellow(),
                path.display(),
                e
            ),
        }
    }

    println!("{} Analyzed {} files", style("✓").green(), analyses.len());

    Ok(analyses)
}

fn filter_soft_clip_candidates(analyses: &[AudioAnalysis], target_lufs: f64) -> Vec<&AudioAnalysis> {
    analyses
        .iter()
        .filter(|a| target_lufs - a.input_i > MIN_EFFECTIVE_GAIN)
        .collect()
}

fn soft_clip_files(
    analyses: &[&AudioAnalysis],
    target_lufs: f64,
    threshold_db: f64,
    clip_type: &str,
    base_dir: &Path,
    backup_dir: Option<&Path>,
    tag_comment: bool,
) -> Result<()> {
    let pb = ProgressBar::new(analyses.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.blue} Soft clipping... [{bar:40.blue/cyan}] {pos}/{len}")
            .unwrap()
            .progress_chars("█▓░"),
    );

    for analysis in analyses {
        if let Some(backup) = backup_dir {
            if let Err(e) = processor::backup_file(&analysis.path, base_dir, backup) {
                pb.println(format!(
                    "{} {}: backup failed: {}",
                    style("⚠").yellow(),
                    analysis.filename,
                    e
                ));
            }
        }

        let gain_db = target_lufs - analysis.input_i;

        match processor::apply_soft_clip(
            &analysis.path,
            gain_db,
            threshold_db,
            clip_type,
            analysis.bitrate_kbps,
        ) {
            Err(e) => pb.println(format!(
                "{} {}: {}",
                style("⚠").yellow(),
                analysis.filename,
                e
            )),
            Ok(()) if tag_comment => {
                if let Err(e) = processor::write_gain_comment(&analysis.path, gain_db) {
                    pb.println(format!(
                        "{} {}: comment tag: {}",
                        style("⚠").yellow(),
                        analysis.filename,
                        e
                    ));
                }
            }
            Ok(()) => {}
        }
        pb.inc(1);
    }

    pb.finish_and_clear();
    Ok(())
}

fn tag_files_only(analyses: &[&AudioAnalysis]) -> Result<()> {
    let pb = ProgressBar::new(analyses.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} Tagging...  [{bar:40.cyan/blue}] {pos}/{len}")
            .unwrap()
            .progress_chars("█▓░"),
    );

    for analysis in analyses {
        if let Err(e) = processor::write_gain_comment(&analysis.path, analysis.effective_gain) {
            pb.println(format!(
                "{} {}: comment tag: {}",
                style("⚠").yellow(),
                analysis.filename,
                e
            ));
        }
        pb.inc(1);
    }

    pb.finish_and_clear();
    Ok(())
}

fn process_files(
    analyses: &[&AudioAnalysis],
    base_dir: &std::path::Path,
    backup_dir: Option<&std::path::Path>,
    tag_comment: bool,
) -> Result<()> {
    let pb = ProgressBar::new(analyses.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} Processing... [{bar:40.cyan/blue}] {pos}/{len}")
            .unwrap()
            .progress_chars("█▓░"),
    );

    for analysis in analyses {
        match processor::process_file(&analysis.path, analysis, base_dir, backup_dir) {
            Err(e) => pb.println(format!(
                "{} {}: {}",
                style("⚠").yellow(),
                analysis.filename,
                e
            )),
            Ok(()) if tag_comment => {
                if let Err(e) =
                    processor::write_gain_comment(&analysis.path, analysis.effective_gain)
                {
                    pb.println(format!(
                        "{} {}: comment tag: {}",
                        style("⚠").yellow(),
                        analysis.filename,
                        e
                    ));
                }
            }
            Ok(()) => {}
        }
        pb.inc(1);
    }

    pb.finish_and_clear();

    Ok(())
}
