use clap::Parser;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use ignore::WalkBuilder;
use git2::{Repository, DiffFormat, Tree, Diff};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Directory to process
    directory_path: String,

    /// Comma-separated list of file extensions to process (e.g., "txt,md,rs")
    suffixes: String,

    /// Ignore files based on .gitignore rules
    #[arg(long, default_value_t = false)]
    use_gitignore: bool,

    /// Comma-separated list of paths to exclude
    #[arg(long)]
    exclude_paths: Option<String>,

    /// Comma-separated list of paths to include
    #[arg(long)]
    include_paths: Option<String>,

    /// Comma-separated list of keywords - only include files containing at least one keyword
    #[arg(long)]
    or_keywords: Option<String>,

    /// Comma-separated list of keywords - only include files containing all keywords
    #[arg(long)]
    and_keywords: Option<String>,

    /// Comma-separated list of keywords - exclude files containing any of these keywords
    #[arg(long)]
    exclude_keywords: Option<String>,

    /// Only show files that have differences
    #[arg(long, default_value_t = false)]
    diff_only: bool,

    /// Starting commit hash for diff comparison
    #[arg(long)]
    start_commit_id: Option<String>,

    /// Ending commit hash for diff comparison
    #[arg(long)]
    end_commit_id: Option<String>,
}

fn main() -> io::Result<()> {
    let cli = Cli::parse();
    
    let suffixes: Vec<String> = cli.suffixes.split(',').map(String::from).collect();
    
    let exclude_paths: Vec<PathBuf> = match cli.exclude_paths {
        Some(s) => {
            if s.contains(',') {
                s.split(',').map(PathBuf::from).collect()
            } else {
                vec![PathBuf::from(s)]
            }
        }
        None => Vec::new()
    };

    let include_paths: Vec<PathBuf> = match cli.include_paths {
        Some(s) => {
            if s.contains(',') {
                s.split(',').map(PathBuf::from).collect()
            } else {
                vec![PathBuf::from(s)]
            }
        }
        None => Vec::new()
    };
    

// comment kjdalkdlaslkjnalsknd kjlkl kl

    let or_keywords: Vec<String> = cli.or_keywords
        .map(|s| if s.contains(',') {
            s.split(',').map(String::from).collect()
        } else {
            vec![s]
        })
        .unwrap_or_default();

    let and_keywords: Vec<String> = cli.and_keywords
        .map(|s| if s.contains(',') {
            s.split(',').map(String::from).collect()
        } else {
            vec![s]
        })
        .unwrap_or_default();

    let exclude_keywords: Vec<String> = cli.exclude_keywords
        .map(|s| if s.contains(',') {
            s.split(',').map(String::from).collect()
        } else {
            vec![s]
        })
        .unwrap_or_default();

    match process_directory(
        &cli.directory_path,
        &suffixes,
        cli.use_gitignore,
        cli.diff_only,
        &exclude_paths,
        &include_paths,
        &or_keywords,
        &and_keywords,
        &exclude_keywords,
        cli.start_commit_id.as_deref(),
        cli.end_commit_id.as_deref()
    ) {
        Ok(_) => println!("Successfully processed directory and created dirscribe.txt"),
        Err(e) => eprintln!("Error processing directory: {}", e),
    }

    Ok(())
}


fn get_diff_list(
    repo: &Repository,
    start_commit_id: Option<&str>,
    end_commit_id_id: Option<&str>,
) -> io::Result<Vec<PathBuf>> {
    let mut diff_list = Vec::new();
    
    // Helper function to get tree from commit ID
    let get_tree = |commit_id: &str| -> io::Result<Tree> {
        repo.revparse_single(commit_id)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.message().to_string()))?
            .peel_to_commit()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.message().to_string()))?
            .tree()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.message().to_string()))
    };

    // Get the diff based on provided arguments
    let diff = match (start_commit_id, end_commit_id_id) {
        // Both None: compare working directory with HEAD
        (None, None) => {
            let head_tree = repo.head()
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e.message().to_string()))?
                .peel_to_tree()
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e.message().to_string()))?;
            
            repo.diff_tree_to_workdir_with_index(
                Some(&head_tree),
                None
            )
        },
        // Only old_commit provided: compare that commit with working directory
        (Some(old_id), None) => {
            let old_tree = get_tree(old_id)?;
            repo.diff_tree_to_workdir_with_index(
                Some(&old_tree),
                None
            )
        },
        // Both provided: compare the two commits directly
        (Some(old_id), Some(new_id)) => {
            let old_tree = get_tree(old_id)?;
            let new_tree = get_tree(new_id)?;
            repo.diff_tree_to_tree(
                Some(&old_tree),
                Some(&new_tree),
                None
            )
        },
        // Invalid case: old None but new Some - treat as comparing HEAD to new commit
        (None, Some(new_id)) => {
            let head_tree = repo.head()
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e.message().to_string()))?
                .peel_to_tree()
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e.message().to_string()))?;
            let new_tree = get_tree(new_id)?;
            repo.diff_tree_to_tree(
                Some(&head_tree),
                Some(&new_tree),
                None
            )
        }
    }.map_err(|e| io::Error::new(io::ErrorKind::Other, e.message().to_string()))?;
    
    // Collect changed files
    diff.foreach(
        &mut |delta, _| {
            if let Some(new_file) = delta.new_file().path() {
                diff_list.push(new_file.to_path_buf());
            }
            true
        },
        None,
        None,
        None,
    ).map_err(|e| io::Error::new(io::ErrorKind::Other, e.message().to_string()))?;
    
    Ok(diff_list)
}

fn process_directory(
    dir_path: &str,
    suffixes: &[String],
    use_gitignore: bool,
    diff_only: bool,
    exclude_paths: &[PathBuf],
    include_paths: &[PathBuf],
    or_keywords: &[String],
    and_keywords: &[String],
    exclude_keywords: &[String],
    start_commit_id: Option<&str>,
    end_commit_id: Option<&str>
) -> io::Result<()> {
    let mut output_file = File::create("dirscribe.txt")?;
    let dir_path = Path::new(dir_path);
    
    let repo = if diff_only {
        Some(Repository::open(dir_path).map_err(|e| 
            io::Error::new(io::ErrorKind::Other, e.message().to_string())
        )?)
    } else {
        None
    };

    if !dir_path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "Directory not found",
        ));
    }

    let mut diff_list = Vec::new();
    if diff_only {
        if let Some(repo) = &repo {
            diff_list = get_diff_list(repo, start_commit_id, end_commit_id)?;
        }
    }

    let walker = WalkBuilder::new(dir_path)
        .hidden(false)  // Show hidden files
        .git_ignore(use_gitignore)  // Use .gitignore based on CLI argument
        .build();

    for result in walker {
        match result {
            Ok(entry) => {
                let path = entry.path();
                
                // Skip if diff_only is true and path is not in diff_list
                if diff_only {
                    if let Ok(relative_path) = path.strip_prefix(dir_path) {
                        if !diff_list.contains(&relative_path.to_path_buf()) {
                            continue;
                        }
                    }
                }

                if let Some(file_suffix) = path.extension() {
                    if suffixes.iter().any(|s| s == file_suffix.to_str().unwrap_or("")) {
                        // Get relative path from base directory
                        if let Ok(relative_path) = path.strip_prefix(dir_path) {
                            let relative_path_str = relative_path.to_string_lossy();
                            
                            // Skip if path matches any exclude pattern
                            if exclude_paths.iter().any(|excluded| 
                                relative_path_str.starts_with(&excluded.to_string_lossy().as_ref())
                            ) {
                                continue;
                            }
                            
                            // Skip if include patterns exist and path doesn't match any
                            if !include_paths.is_empty() {
                                let is_included = include_paths.iter().any(|included|
                                    relative_path_str.starts_with(&included.to_string_lossy().as_ref())
                                );
                                if !is_included {
                                    continue;
                                }
                            }
                        }
                        
                        process_file(
                            &path.to_path_buf(),
                            &mut output_file,
                            or_keywords,
                            and_keywords,
                            exclude_keywords,
                            diff_only,
                            repo.as_ref(),
                            start_commit_id,
                            end_commit_id
                        )?;
                    }
                }
            }
            Err(err) => eprintln!("Error walking directory: {}", err),
        }
    }

    Ok(())
}

fn get_diff_str(diff: &Diff) -> io::Result<String> {
    let mut diff_output = Vec::new();
    diff.print(DiffFormat::Patch, |_delta, _hunk, line| {
        if let Ok(content) = std::str::from_utf8(line.content()) {
            diff_output.extend_from_slice(content.as_bytes());
        }
        true
    }).map_err(|e| io::Error::new(io::ErrorKind::Other, e.message().to_string()))?;

    String::from_utf8(diff_output).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

fn filter_diff_for_file(diff_str: &str, file_path: &Path) -> String {
    let lines: Vec<&str> = diff_str.lines().collect();
    let mut result = Vec::new();
    let mut current_file_section = false;
    // Get just the filename component
    let file_name = file_path.file_name()
        .map(|s| s.to_string_lossy())
        .unwrap_or_default();

    for line in lines {
        if line.starts_with("diff --git") {
            // Check if this section is for our file
            current_file_section = line.contains(&*file_name);
            if current_file_section {
                result.push(line);
            }
        } else if current_file_section {
            // Keep adding lines until we hit the next diff section
            if line.starts_with("diff --git") {
                break;
            }
            result.push(line);
        }
    }

    result.join("\n")
}


fn process_file(
    file_path: &PathBuf,
    output_file: &mut File,
    or_keywords: &[String],
    and_keywords: &[String],
    exclude_keywords: &[String],
    diff_only: bool,
    repo: Option<&Repository>,
    start_commit_id: Option<&str>,
    end_commit_id: Option<&str>
) -> io::Result<()> {
    // Read file contents
    let contents = fs::read_to_string(file_path)?;

    // Check exclude keywords - skip if any are present
    if exclude_keywords.iter().any(|keyword| contents.contains(keyword)) {
        return Ok(());
    }
    
    // Check OR keywords - at least one must be present
    if !or_keywords.is_empty() {
        let contains_or_keyword = or_keywords.iter().any(|keyword| contents.contains(keyword));
        if !contains_or_keyword {
            return Ok(());
        }
    }

    // Check AND keywords - all must be present
    if !and_keywords.is_empty() {
        let contains_all_keywords = and_keywords.iter().all(|keyword| contents.contains(keyword));
        if !contains_all_keywords {
            return Ok(());
        }
    }
    
    // Write file path and contents
    writeln!(output_file, "File: {}", file_path.display())?;
    writeln!(output_file, "{}", contents)?;




    if diff_only {
        if let Some(repo) = repo {
            writeln!(output_file, "\nDiff:")?;
            
            // Helper function to get tree from commit ID
            let get_tree = |commit_id: &str| -> io::Result<Tree> {
                repo.revparse_single(commit_id)
                    .map_err(|e| io::Error::new(io::ErrorKind::Other, e.message().to_string()))?
                    .peel_to_commit()
                    .map_err(|e| io::Error::new(io::ErrorKind::Other, e.message().to_string()))?
                    .tree()
                    .map_err(|e| io::Error::new(io::ErrorKind::Other, e.message().to_string()))
            };

            let diff = match (start_commit_id, end_commit_id) {
                (None, None) => {
                    let head_tree = repo.head()
                        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.message().to_string()))?
                        .peel_to_tree()
                        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.message().to_string()))?;
                    
                    repo.diff_tree_to_workdir_with_index(Some(&head_tree), None)
                },
                (Some(old_id), None) => {
                    let old_tree = get_tree(old_id)?;
                    repo.diff_tree_to_workdir_with_index(Some(&old_tree), None)
                },
                (Some(old_id), Some(new_id)) => {
                    let old_tree = get_tree(old_id)?;
                    let new_tree = get_tree(new_id)?;
                    repo.diff_tree_to_tree(Some(&old_tree), Some(&new_tree), None)
                },
                (None, Some(new_id)) => {
                    let head_tree = repo.head()
                        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.message().to_string()))?
                        .peel_to_tree()
                        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.message().to_string()))?;
                    let new_tree = get_tree(new_id)?;
                    repo.diff_tree_to_tree(Some(&head_tree), Some(&new_tree), None)
                }
            }.map_err(|e| io::Error::new(io::ErrorKind::Other, e.message().to_string()))?;

            let diff_str = get_diff_str(&diff)?;
            let filtered_diff = filter_diff_for_file(&diff_str, file_path);
            writeln!(output_file, "{}", filtered_diff)?;
        }
    }
    
    writeln!(output_file)?;
    Ok(())
}
