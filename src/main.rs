use std::fs::{self, File};
mod cli;
mod git;
use cli::Cli;
use git::{get_diff_list, get_diff_str, filter_diff_for_file};
use clap::Parser;
use std::io::{self, Write, Cursor};
use std::path::{Path, PathBuf};
use ignore::WalkBuilder;
use git2::{Repository, DiffFormat, Tree, Diff};
use clipboard::{ClipboardContext, ClipboardProvider};



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

    // Process directory and get the content string
    let content = process_directory(
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
    )?;

    let final_content = if let Some(template_path) = cli.prompt_template_path {
        process_with_template(&content, &template_path)?
    } else {
        content
    };


    if let Some(output_path) = cli.output_path {
        let mut output_file = File::create(&output_path)?;
        output_file.write_all(final_content.as_bytes())?;
        println!("Successfully processed directory and written output to {}", output_path);
    } else {
        write_to_clipboard(&final_content)?;
        println!("Successfully processed directory and copied output to clipboard");
    };
    Ok(())
}



fn write_to_clipboard(content: &str) -> io::Result<()> {
    let mut ctx: ClipboardContext = ClipboardProvider::new().map_err(|e| {
        io::Error::new(
            io::ErrorKind::Other,
            format!("Failed to create clipboard context: {}", e)
        )
    })?;
    
    ctx.set_contents(content.to_owned()).map_err(|e| {
        io::Error::new(
            io::ErrorKind::Other,
            format!("Failed to set clipboard contents: {}", e)
        )
    })?;
    
    Ok(())
}


fn process_with_template(content: &str, template_path: &str) -> io::Result<String> {
    // Read the template file
    let template = fs::read_to_string(template_path).map_err(|e| {
        io::Error::new(
            io::ErrorKind::Other,
            format!("Failed to read template file: {}", e)
        )
    })?;

    // Check for the required placeholder
    if !template.contains("${${CONTENT}$}$") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Template file must contain the placeholder '${${CONTENT}$}$'"
        ));
    }

    // Replace the placeholder with the content
    Ok(template.replace("${${CONTENT}$}$", content))
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
        .git_ignore(use_gitignore)
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

                            // Check keyword filters before adding to valid files
                            if should_include_file(
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
    writeln!(output, "File Contents:")?;
    writeln!(output)?;

    // Process each file
    for file_path in valid_files {
        process_file(
            &file_path,
            &mut output,
            diff_only,
            repo.as_ref(),
            start_commit_id,
            end_commit_id
        )?;
    }

    // Convert the output buffer to a string
    String::from_utf8(output.into_inner())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}


fn should_include_file(
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






// First, let's modify the process_file function signature to accept Write instead of File
fn process_file(
    file_path: &PathBuf,
    output_file: &mut impl Write,  // Changed from &mut File to &mut impl Write
    diff_only: bool,
    repo: Option<&Repository>,
    start_commit_id: Option<&str>,
    end_commit_id: Option<&str>
) -> io::Result<()> {
    // Read file contents
    let contents = fs::read_to_string(file_path)?;
    
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
