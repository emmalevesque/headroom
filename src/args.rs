use clap::Parser;
use std::path::PathBuf;

/// Audio loudness analyzer and gain adjustment tool.
///
/// Run without arguments for interactive mode in the current directory.
/// Provide paths or any flag to run in non-interactive (scriptable) mode.
#[derive(Parser, Debug)]
#[command(
    version,
    about,
    long_about = None,
    after_help = "\
EXAMPLES:
  Analyze files without modifying them:
    headroom --analyze-only ~/Music/DJ-Tracks

  Apply lossless gain to all files in the current directory:
    headroom --lossless

  Soft clip a folder to -7.5 LUFS (e.g. club/DJ mastering target):
    headroom --soft-clip --soft-clip-target -7.5 ~/Music/DJ-Tracks

  Soft clip to -7.5 LUFS with a tighter threshold and atan curve:
    headroom --soft-clip --soft-clip-target -7.5 --soft-clip-threshold -2.0 --soft-clip-type atan ~/Music/DJ-Tracks

  Soft clip to -7.5 LUFS and write the boost to the ID3v2 comment tag:
    headroom --soft-clip --soft-clip-target -7.5 --tag-comment ~/Music/DJ-Tracks

  Apply lossless gain + re-encode lossy files with a backup:
    headroom --lossless --reencode --backup ~/Music/DJ-Tracks

  Process specific files or glob patterns:
    headroom --lossless --no-report track1.mp3 track2.flac \"./albums/**/*.mp3\"
"
)]
pub struct Cli {
    /// Files, directories, or glob patterns to process. Defaults to current directory.
    pub paths: Vec<String>,

    /// Apply lossless gain adjustment (default in non-interactive mode)
    #[arg(long, conflicts_with = "no_lossless")]
    pub lossless: bool,

    /// Skip lossless gain adjustment
    #[arg(long)]
    pub no_lossless: bool,

    /// Apply re-encoding for MP3/AAC files needing precise gain
    #[arg(long, conflicts_with = "no_reencode")]
    pub reencode: bool,

    /// Skip re-encoding (default in non-interactive mode)
    #[arg(long)]
    pub no_reencode: bool,

    /// Create backup before processing (optional DIR; default: <target>/backup)
    #[arg(long, value_name = "DIR", num_args = 0..=1, default_missing_value = "")]
    pub backup: Option<PathBuf>,

    /// Generate CSV report at PATH (default: <target>/headroom_report_<timestamp>.csv)
    #[arg(long, value_name = "PATH", num_args = 0..=1, default_missing_value = "", conflicts_with = "no_report")]
    pub report: Option<PathBuf>,

    /// Skip CSV report
    #[arg(long)]
    pub no_report: bool,

    /// Analyze files only, do not modify anything
    #[arg(long)]
    pub analyze_only: bool,

    /// Prepend effective gain to the ID3v2 comment (COMM) field of each processed file
    #[arg(long)]
    pub tag_comment: bool,

    /// Write suggested gain to ID3v2 comment tag without applying gain to audio
    #[arg(long, conflicts_with_all = ["analyze_only", "tag_comment"])]
    pub tag_comment_only: bool,

    /// Boost to target LUFS-I and apply soft clipping (alternative to lossless gain)
    #[arg(long, conflicts_with_all = ["lossless", "no_lossless", "reencode", "no_reencode", "tag_comment_only"])]
    pub soft_clip: bool,

    /// Target integrated loudness in LUFS-I for soft clip mode (default: -14.0)
    #[arg(long, value_name = "LUFS", default_value_t = -14.0)]
    pub soft_clip_target: f64,

    /// Soft clip threshold in dBFS — point at which clipping begins (default: -1.0)
    #[arg(long, value_name = "DBFS", default_value_t = -1.0)]
    pub soft_clip_threshold: f64,

    /// Soft clip algorithm: tanh, atan, cubic, exp, alg, quintic, sin, erf (default: tanh)
    #[arg(long, value_name = "TYPE", default_value = "tanh")]
    pub soft_clip_type: String,

    /// Skip writing gain to ID3v2 comment field (overrides config default)
    #[arg(long, conflicts_with = "tag_comment")]
    pub no_tag_comment: bool,

    /// Skip backup creation (overrides config default)
    #[arg(long, conflicts_with = "backup")]
    pub no_backup: bool,
}

impl Cli {
    /// Returns true if any non-interactive option or path was provided.
    pub fn is_non_interactive(&self) -> bool {
        !self.paths.is_empty()
            || self.lossless
            || self.no_lossless
            || self.reencode
            || self.no_reencode
            || self.backup.is_some()
            || self.report.is_some()
            || self.no_report
            || self.analyze_only
            || self.tag_comment
            || self.tag_comment_only
            || self.soft_clip
    }

    /// Whether lossless processing is enabled. Explicit flags beat the config default.
    pub fn lossless_enabled(&self, default: bool) -> bool {
        if self.lossless {
            true
        } else if self.no_lossless {
            false
        } else {
            default
        }
    }

    /// Whether re-encode processing is enabled. Explicit flags beat the config default.
    pub fn reencode_enabled(&self, default: bool) -> bool {
        if self.reencode {
            true
        } else if self.no_reencode {
            false
        } else {
            default
        }
    }

    /// Whether CSV report should be generated. Explicit flags beat the config default.
    pub fn report_enabled(&self, default: bool) -> bool {
        if self.no_report {
            false
        } else if self.report.is_some() {
            true
        } else {
            default
        }
    }
}
