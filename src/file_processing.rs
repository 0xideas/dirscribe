use std::fs;
use std::io::{self, Write, Cursor};
use std::path::{Path, PathBuf};
use ignore::WalkBuilder;
use std::collections::HashMap;
use git2::{Repository, Tree};
use tokio::sync::Semaphore;
use std::sync::Arc;
use rayon::prelude::*; // Added missing import for parallel iteration
use crate::git::{get_diff_list, get_diff_str, filter_diff_for_file};
use crate::summary::get_summary;


pub fn process_directory(
    dir_path: &str,
    suffixes: &[String],
    dont_use_gitignore: bool,
    summarize: bool,
    summarize_prompt_templates: HashMap<String, String>,
    apply: bool,
    diff_only: bool,
    exclude_paths: &[PathBuf],
    include_paths: &[PathBuf],
    or_keywords: &[String],
    and_keywords: &[String],
    exclude_keywords: &[String],
    start_commit_id: Option<&str>,
    end_commit_id: Option<&str>
) -> io::Result<String> {
    let mut output = Cursor::new(Vec::new());
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

    // First, collect all valid file paths
    let mut valid_files = Vec::new();
    
    let walker = WalkBuilder::new(dir_path)
        .hidden(false)
        .git_ignore(!dont_use_gitignore)
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

                let should_include = if path.is_dir() {
                    false
                } else if suffixes.contains(&"*".to_string()) {
                    // If wildcard is specified, check if it's a text-like file
                    is_likely_text_file(path)
                } else if let Some(file_suffix) = path.extension() {
                    suffixes.iter().any(|s| s == file_suffix.to_str().unwrap_or(""))
                } else {
                    if let Some(filename) = path.file_name() {
                        suffixes.iter().any(|s| s == filename.to_str().unwrap_or(""))
                    } else {
                        false
                    }
                };

                if should_include {
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

                        // Check keyword filters before adding to valid files
                        if check_for_keywords(
                            &path.to_path_buf(),
                            or_keywords,
                            and_keywords,
                            exclude_keywords,
                        )? {
                            valid_files.push(path.to_path_buf());
                        }
                    }
                }
            }
            Err(err) => eprintln!("Error walking directory: {}", err),
        }
    }

    // Write all file paths at the top
    writeln!(output, "File Paths:")?;
    for file_path in &valid_files {
        writeln!(output, "{}", file_path.display())?;
    }
    writeln!(output)?;
    if !summarize {
        writeln!(output, "File Contents:")?;
    } else {
        writeln!(output, "File Summaries:")?;
    }
    writeln!(output)?;

    // Process each file
    let semaphore = Arc::new(Semaphore::new(5));

    let strings: Vec<String> = valid_files.par_iter().map(|file_path| { // Fixed double par_iter() and collect type
        let permit = semaphore.clone().try_acquire_owned().unwrap();
        let result = process_file(
            file_path,
            summarize,
            summarize_prompt_templates,
            diff_only,
            repo.as_ref(),
            start_commit_id,
            end_commit_id
        );
        drop(permit);
        result.unwrap_or_else(|e| format!("Error processing file: {}", e))
    }).collect();

    // Fixed incorrect parameter order and brace style
    let strings2: Vec<String> = if summarize {
        valid_files.iter().zip(strings).map(|(file, s)| 
            format!("\nSummary of {}:\n\n{}\n", file.display(), s)).collect()
    } else if diff_only {
        valid_files.iter().zip(strings).map(|(file, s)| 
            format!("\nDiff of {}:\n\n{}\n", file.display(), s)).collect()
    } else {
        valid_files.iter().zip(strings).map(|(file, s)| 
            format!("\nFile Content of {}:\n\n{}\n", file.display(), s)).collect()
    };

    for string in strings2 {
        write!(output, "{}", string)?;
    }
    
    String::from_utf8(output.into_inner())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

pub fn process_file(
    file_path: &PathBuf,
    summarize: bool, // Fixed parameter order to match usage
    summarize_prompt_templates: HashMap<String, String>    ,
    diff_only: bool,
    repo: Option<&Repository>,
    start_commit_id: Option<&str>,
    end_commit_id: Option<&str>
) -> io::Result<String> {
    let relative_path = if let Some(repo) = repo {
        let repo_workdir = repo.workdir().ok_or_else(|| {
            io::Error::new(io::ErrorKind::Other, "Could not get repository working directory")
        })?;
        
        let full_path = fs::canonicalize(file_path)?;
        let relative_path = full_path.strip_prefix(fs::canonicalize(repo_workdir)?)
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "File not in repository"))?;
            
        relative_path.to_path_buf()
    } else {
        file_path.clone()
    };

    let contents = if !diff_only {
        fs::read_to_string(file_path)?
    } else {
        if let Some(repo) = repo {
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
            filter_diff_for_file(&diff_str, file_path) // Removed unnecessary semicolon
        } else {
            String::new() // Added else branch for when repo is None
        }
    };

    if summarize {
        if !diff_only{
            Ok(get_summary(&contents, &summarize_prompt_templates.get("summary-0.1.txt").unwrap()))
        } else {
            Ok(get_summary(&contents, &summarize_prompt_templates.get("summary-diff-0.1.txt").unwrap()))
        }
    } else if !diff_only {
        Ok(contents)
    } 
}


pub fn check_for_keywords(
    file_path: &PathBuf,
    or_keywords: &[String],
    and_keywords: &[String],
    exclude_keywords: &[String],
) -> io::Result<bool> {


    let contents = fs::read_to_string(file_path)?;

    // Check exclude keywords - skip if any are present
    if exclude_keywords.iter().any(|keyword| contents.contains(keyword)) {
        return Ok(false);
    }
    
    // Check OR keywords - at least one must be present
    if !or_keywords.is_empty() {
        let contains_or_keyword = or_keywords.iter().any(|keyword| contents.contains(keyword));
        if !contains_or_keyword {
            return Ok(false);
        }
    }

    // Check AND keywords - all must be present
    if !and_keywords.is_empty() {
        let contains_all_keywords = and_keywords.iter().all(|keyword| contents.contains(keyword));
        if !contains_all_keywords {
            return Ok(false);
        }
    }

    Ok(true)
}

// Add this function at the top of file_processing.rs
fn is_likely_text_file(path: &Path) -> bool {
    // Common text file extensions
    const TEXT_EXTENSIONS: &[&str] = &[
        // Programming languages
        "rs", "py", "js", "ts", "java", "c", "cpp", "h", "hpp", "cs", "go", "rb", "php", "swift",
        "kt", "scala", "sh", "bash", "pl", "r", "sql", "m", "mm",
        // Web
        "html", "htm", "css", "scss", "sass", "less", "xml", "svg",
        // Data formats
        "json", "yaml", "yml", "toml", "ini", "conf", "config",
        // Documentation
        "md", "markdown", "txt", "rtf", "rst", "asciidoc", "adoc",
        // Config files
        "gitignore", "env", "dockerignore", "editorconfig",
        // Build files
        "cmake", "make", "mak", "gradle",
    ];

    // Always consider files without extension that are commonly text
    const EXTENSION_LESS_TEXT_FILES: &[&str] = &[
        "Dockerfile", "Makefile", "README", "LICENSE", "Cargo.lock", "package.json",
        ".gitignore", ".env", ".dockerignore", ".editorconfig"
    ];

    if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
        // Check extension-less files first
        if EXTENSION_LESS_TEXT_FILES.contains(&file_name) {
            return true;
        }
    }

    // Check file extension
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        if TEXT_EXTENSIONS.contains(&ext.to_lowercase().as_str()) {
            return true;
        }
    }

    // For files without extension or unknown extensions, try to read a small sample
    // and check if it contains only valid UTF-8 text
    if let Ok(file) = std::fs::File::open(path) {
        use std::io::Read;
        let mut buffer = [0u8; 1024];
        let mut handle = file;
        
        // Read first 1024 bytes
        if handle.read(&mut buffer).is_ok() {
            // Check if content is valid UTF-8
            return String::from_utf8(buffer.to_vec()).is_ok();
        }
    }

    false
}