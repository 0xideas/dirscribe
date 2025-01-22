use std::path::{Path, PathBuf};
use std::io;
use git2::Repository;
use cli::Cli;


pub struct ValidationError {
    pub message: String,
}

impl From<String> for ValidationError {
    fn from(message: String) -> Self {
        ValidationError { message }
    }
}

pub fn validate_cli_args(cli: &Cli) -> Result<(), ValidationError> {
    // Validate suffixes
    validate_suffixes(&cli.suffixes)?;

    // Validate paths
    if let Some(template_path) = &cli.prompt_template_path {
        validate_template_path(template_path)?;
    }

    if let Some(output_path) = &cli.output_path {
        validate_output_path(output_path)?;
    }

    // Validate git-related arguments
    if cli.diff_only {
        validate_git_args(
            &cli.start_commit_id,
            &cli.end_commit_id,
        )?;
    }

    // Validate keywords
    validate_keywords(&cli.or_keywords, "or_keywords")?;
    validate_keywords(&cli.and_keywords, "and_keywords")?;
    validate_keywords(&cli.exclude_keywords, "exclude_keywords")?;

    // Validate exclude/include paths
    validate_path_filters(
        &cli.exclude_paths,
        &cli.include_paths,
    )?;

    Ok(())
}

fn validate_suffixes(suffixes: &str) -> Result<(), ValidationError> {
    if suffixes.is_empty() {
        return Err("Suffixes cannot be empty".into());
    }

    let parts: Vec<&str> = suffixes.split(',').collect();
    
    for suffix in parts {
        if suffix.is_empty() {
            return Err("Empty suffix found after splitting".into());
        }

        if !suffix.chars().all(|c| c.is_alphanumeric()) {
            return Err(format!("Invalid suffix '{}': must be alphanumeric", suffix).into());
        }

        if suffix.len() > 10 {
            return Err(format!("Suffix '{}' exceeds maximum length of 10", suffix).into());
        }
    }

    Ok(())
}

fn validate_template_path(path: &str) -> Result<(), ValidationError> {
    let path = Path::new(path);
    
    if !path.exists() {
        return Err(format!("Template file does not exist: {}", path.display()).into());
    }

    if !path.is_file() {
        return Err(format!("Template path is not a file: {}", path.display()).into());
    }

    // Check file size (e.g., max 1MB)
    if let Ok(metadata) = path.metadata() {
        if metadata.len() > 1_000_000 {
            return Err("Template file is too large (max 1MB)".into());
        }
    }

    Ok(())
}

fn validate_output_path(path: &str) -> Result<(), ValidationError> {
    let path = Path::new(path);
    
    // Check if parent directory exists or can be created
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            return Err(format!("Output directory does not exist: {}", parent.display()).into());
        }
    }

    // Check if path points to a directory
    if path.is_dir() {
        return Err(format!("Output path is a directory: {}", path.display()).into());
    }

    Ok(())
}

fn validate_git_args(
    start_commit: &Option<String>,
    end_commit: &Option<String>,
) -> Result<(), ValidationError> {
    // Verify we're in a git repository
    let repo = match Repository::open(".") {
        Ok(repo) => repo,
        Err(_) => return Err("Not a git repository".into()),
    };

    if let Some(start) = start_commit {
        validate_commit(&repo, start, "start_commit_id")?;
    }

    if let Some(end) = end_commit {
        validate_commit(&repo, end, "end_commit_id")?;
    }

    // If both commits provided, verify start is ancestor of end
    if let (Some(start), Some(end)) = (start_commit, end_commit) {
        let start_commit = repo
            .revparse_single(start)
            .map_err(|_| format!("Invalid start commit: {}", start))?
            .peel_to_commit()
            .map_err(|_| "Failed to parse start commit")?;

        let end_commit = repo
            .revparse_single(end)
            .map_err(|_| format!("Invalid end commit: {}", end))?
            .peel_to_commit()
            .map_err(|_| "Failed to parse end commit")?;

        if !repo.graph_descendant_of(end_commit.id(), start_commit.id())
            .map_err(|_| "Failed to check commit relationship")? {
            return Err("start_commit_id must be an ancestor of end_commit_id".into());
        }
    }

    Ok(())
}

fn validate_commit(repo: &Repository, commit_id: &str, arg_name: &str) -> Result<(), ValidationError> {
    match repo.revparse_single(commit_id) {
        Ok(obj) => {
            if obj.as_commit().is_none() {
                return Err(format!("{} is not a valid commit", arg_name).into());
            }
        }
        Err(_) => {
            return Err(format!("Invalid {}: {}", arg_name, commit_id).into());
        }
    }
    Ok(())
}

fn validate_keywords(keywords: &Option<String>, field_name: &str) -> Result<(), ValidationError> {
    if let Some(keywords) = keywords {
        let parts: Vec<&str> = keywords.split(',').collect();
        
        for keyword in parts {
            if keyword.is_empty() {
                return Err(format!("Empty keyword found in {}", field_name).into());
            }

            if keyword.len() > 100 {
                return Err(format!("Keyword in {} exceeds maximum length of 100", field_name).into());
            }

            // Check for invalid characters (optional - adjust as needed)
            if keyword.chars().any(|c| !c.is_ascii()) {
                return Err(format!("Non-ASCII characters found in {} keyword: {}", field_name, keyword).into());
            }
        }
    }
    Ok(())
}

fn validate_path_filters(
    exclude_paths: &Option<String>,
    include_paths: &Option<String>,
) -> Result<(), ValidationError> {
    let mut all_paths = Vec::new();

    // Helper function to process paths
    let process_paths = |paths_str: &str, is_exclude: bool| -> Result<Vec<PathBuf>, ValidationError> {
        let paths: Vec<PathBuf> = paths_str
            .split(',')
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .collect();

        for path in &paths {
            // Normalize path
            let normalized = path.canonicalize().map_err(|_| {
                format!("{} path does not exist: {}", 
                    if is_exclude { "Exclude" } else { "Include" },
                    path.display()
                )
            })?;

            // Verify path is within project directory
            let current_dir = std::env::current_dir().map_err(|_| "Failed to get current directory")?;
            if !normalized.starts_with(current_dir) {
                return Err(format!("Path is outside project directory: {}", path.display()).into());
            }
        }

        Ok(paths)
    };

    if let Some(exclude) = exclude_paths {
        all_paths.extend(process_paths(exclude, true)?);
    }

    if let Some(include) = include_paths {
        let include_paths = process_paths(include, false)?;
        
        // Check for conflicts between include and exclude paths
        for include_path in &include_paths {
            if all_paths.iter().any(|p| include_path.starts_with(p)) {
                return Err(format!(
                    "Include path conflicts with exclude path: {}", 
                    include_path.display()
                ).into());
            }
        }
        
        all_paths.extend(include_paths);
    }

    Ok(())
}