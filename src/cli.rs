use clap::Parser;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// Comma-separated list of file extensions to process (e.g., "txt,md,rs")
    pub suffixes: String,

    /// Path to prompt template file
    #[arg(long)]
    pub prompt_template_path: Option<String>,

    /// Path to output path
    #[arg(long)]
    pub output_path: Option<String>,

    /// Include files that are ignored by default based on .gitignore rules
    #[arg(long, default_value_t = false)]
    pub dont_use_gitignore: bool,

    /// Comma-separated list of paths to exclude
    #[arg(long)]
    pub exclude_paths: Option<String>,

    /// Comma-separated list of paths to include
    #[arg(long)]
    pub include_paths: Option<String>,

    /// Comma-separated list of keywords - only include files containing at least one keyword
    #[arg(long)]
    pub or_keywords: Option<String>,

    /// Comma-separated list of keywords - only include files containing all keywords
    #[arg(long)]
    pub and_keywords: Option<String>,

    /// Comma-separated list of keywords - exclude files containing any of these keywords
    #[arg(long)]
    pub exclude_keywords: Option<String>,

    /// Only show files that have differences
    #[arg(long, default_value_t = false)]
    pub diff_only: bool,

    /// Starting commit hash for diff comparison
    #[arg(long)]
    pub start_commit_id: Option<String>,

    /// Ending commit hash for diff comparison
    #[arg(long)]
    pub end_commit_id: Option<String>,
}
