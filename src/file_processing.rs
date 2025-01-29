use std::fs;
use std::io::{self, Write, Cursor};
use std::path::{Path, PathBuf};
use anyhow::Context;
use ignore::WalkBuilder;
use std::collections::HashMap;
use git2::{Repository, Tree};
use chrono::Local;
use crate::git::{get_diff_list, get_diff_str, filter_diff_for_file};
use crate::summary::{get_summaries, check_summary};


pub async fn process_directory(
    dir_path: &str,
    suffixes: &[String],
    dont_use_gitignore: bool,
    summarize: bool,
    summarize_prompt_templates: HashMap<String, String>,
    apply: bool,
    retrieve: bool,
    diff_only: bool,
    exclude_paths: &[PathBuf],
    include_paths: &[PathBuf],
    or_keywords: &[String],
    and_keywords: &[String],
    exclude_keywords: &[String],
    start_commit_id: Option<&str>,
    end_commit_id: Option<&str>
) -> anyhow::Result<String> {
    let mut output = Cursor::new(Vec::new());
    let dir_path = Path::new(dir_path);
    
    let repo = if diff_only {
        Some(Repository::open(dir_path).context("Failed to open git repository")?)
    } else {
        None
    };

    if !dir_path.exists() {
        return Err(anyhow::anyhow!("Directory not found"));
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

    let file_contents: HashMap<String, String> = valid_files
        .iter()
        .filter_map(|file_path| {
            let path_string = file_path.to_string_lossy().into_owned();
            match process_file(
                file_path,
                diff_only,
                repo.as_ref(),
                start_commit_id,
                end_commit_id
            ) {
                Ok(content) => Some((path_string, content)),
                Err(e) => {
                    eprintln!("Error processing file {}: {}", file_path.display(), e);
                    None
                }
            }
        })
        .collect();

    // Generate output string maintaining file path order
    let result = if summarize {
        let valid_file_strings: Vec<String> = valid_files.iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect();
        

        let suffix_map = create_comment_map();

        let summaries = if !diff_only {
            if !retrieve {
                get_summaries(valid_file_strings.clone(), file_contents.clone(), summarize_prompt_templates["summary-0.1"].clone(), suffix_map.clone()).await?
            } else {
                get_summaries_from_files(valid_file_strings.clone(), file_contents.clone())
            }
        } else {
            get_summaries(valid_file_strings, file_contents.clone(), summarize_prompt_templates["summary-diff-0.1"].clone(), suffix_map.clone()).await?
        };
        
        if apply && !diff_only {
            // Zip together the files and their summaries
            for (file_path, summary) in valid_files.iter().zip(summaries.iter()) {
                if let Err(e) = write_summary_to_file(file_path, summary, suffix_map.clone()) {
                    eprintln!("Error writing summary to {}: {}", file_path.display(), e);
                }
            }
            
        }
    
        // Use the original valid_files order
        valid_files.iter().zip(summaries.iter())
            .map(|(file, summary)| {
                format!("\nSummary of {}:\n\n{}\n", file.display(), summary)
            })
            .collect::<Vec<String>>()
            .join("")
    } else if diff_only {
        valid_files.iter()
            .filter_map(|file| {
                let path_string = file.to_string_lossy().into_owned();
                file_contents.get(&path_string)
                    .map(|content| format!("\nDiff of {}:\n\n{}\n", file.display(), content))
            })
            .collect::<Vec<String>>()
            .join("")
    } else {
        valid_files.iter()
            .filter_map(|file| {
                let path_string = file.to_string_lossy().into_owned();
                file_contents.get(&path_string)
                    .map(|content| format!("\nFile Content of {}:\n\n{}\n", file.display(), content))
            })
            .collect::<Vec<String>>()
            .join("")
    };

    write!(output, "{}", result)?;
    
    String::from_utf8(output.into_inner())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
        .map_err(Into::into)
}

fn check_prefix(s: &str) -> bool {
    let lines: Vec<_> = s.split('\n').collect();
    if lines.is_empty() { return true; }
    let first = lines[0].trim_start();
    let is_hash = first.starts_with('#');
    lines.iter().all(|l| l.trim_start().starts_with(if is_hash { "#" } else { "//" }))
}

fn get_summaries_from_files(
    valid_files: Vec<String>, 
    file_contents: HashMap<String, String>
) -> Vec<String> {
    let mut summaries = Vec::new();

    for file_path in valid_files {
        let content = file_contents.get(&file_path).unwrap_or(&String::new()).clone();
        
        let summary =  filter_dirscribe_sections(&content, false);
        summaries.push(summary)
    }

    summaries
}

pub fn filter_dirscribe_sections(content: &str, exclude: bool) -> String {
    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() {
        return String::new();
    }

    // Create iterator tuples with (previous, current, next) lines
    let with_context = (0..lines.len()).map(|i| {
        let prev = if i > 0 { Some(lines[i - 1]) } else { None };
        let current = lines[i];
        let next = if i < lines.len() - 1 { Some(lines[i + 1]) } else { None };
        (prev, current, next)
    });
    let mut line_number = 0;
    let mut in_dirscribe = false;
    let filtered_lines: Vec<&str> = with_context
        .filter(|(prev, current, next)| {
            line_number += 1;

            if let Some(next_line) = next {
                if line_number < 3 && next_line.contains("[DIRSCRIBE]") {
                    in_dirscribe = true;
                    if exclude {
                        return false;
                    } else {
                        return true
                    }
                }
            }

            if let Some(prev_line) = prev {
                if in_dirscribe && prev_line.contains("[/DIRSCRIBE]"){
                    in_dirscribe = false;
                    if exclude {
                        return false;
                    } else {
                        return true
                    }
                }
            }

            if exclude {
                !in_dirscribe
            } else {
                in_dirscribe
            }
        })
        .map(|(_, current, _)| current)
        .collect();

    filtered_lines.join("\n")
}


fn insert_timestamp(input: &str) -> String {
    let mut lines: Vec<&str> = input.lines().collect();
    let timestamp = Local::now().to_rfc3339();
    lines.insert(lines.len() - 2, &timestamp);
    lines.join("\n")
}

pub fn write_summary_to_file(file_path: &Path, summary: &str, suffix_map: HashMap<&'static str, (&'static str, &'static str)>) -> anyhow::Result<()> {
    if check_summary(file_path, summary, &suffix_map) | check_prefix(summary) {
        let content = fs::read_to_string(file_path)?;    
        let processed_content = filter_dirscribe_sections(&content, true);
        let summary_ts = insert_timestamp(summary);
        let summary_block = format!("{}\n", summary_ts);
        let new_content = summary_block + &processed_content;
        fs::write(file_path, new_content)?;
        Ok(())
    } else {
        return Err(anyhow::anyhow!("Summary is not a correctly formatted comment. (doesn't start with a comment char on every line or doesn't have starting or ending line with multi line comment enclosure)"));

    }
}


pub fn process_file(
    file_path: &PathBuf,
    diff_only: bool,
    repo: Option<&Repository>,
    start_commit_id: Option<&str>,
    end_commit_id: Option<&str>
) -> io::Result<String> {
    let _relative_path = if let Some(repo) = repo {
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

    Ok(contents)
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



fn create_comment_map() -> HashMap<&'static str, (&'static str, &'static str)> {
    let mut map = HashMap::new();
    
    // Existing mappings
    // ActionScript
    map.insert("as", ("/*", "*/"));
    
    // Ada
    map.insert("ada", ("/*", "*/"));
    map.insert("adb", ("/*", "*/"));
    map.insert("ads", ("/*", "*/"));
    
    // AppleScript
    map.insert("scpt", ("(*", "*)"));
    map.insert("applescript", ("(*", "*)"));
    
    // Assembly
    map.insert("asm", ("/*", "*/"));
    map.insert("s", ("/*", "*/"));
    
    // AWK
    map.insert("awk", ("/*", "*/"));
    
    // Bash
    map.insert("sh", (":'", "'"));
    map.insert("bash", (":'", "'"));
    
    // C
    map.insert("c", ("/*", "*/"));
    map.insert("h", ("/*", "*/"));
    
    // C#
    map.insert("cs", ("/*", "*/"));
    
    // C++
    map.insert("cpp", ("/*", "*/"));
    map.insert("hpp", ("/*", "*/"));
    map.insert("cc", ("/*", "*/"));
    map.insert("hh", ("/*", "*/"));
    map.insert("cxx", ("/*", "*/"));
    map.insert("hxx", ("/*", "*/"));
    
    // COBOL
    map.insert("cob", ("/*", "*/"));
    map.insert("cbl", ("/*", "*/"));
    
    // CoffeeScript
    map.insert("coffee", ("###", "###"));
    
    // CSS
    map.insert("css", ("/*", "*/"));
    
    // D
    map.insert("d", ("/*", "*/"));
    
    // Dart
    map.insert("dart", ("/*", "*/"));
    
    // Delphi
    map.insert("pas", ("{", "}"));
    map.insert("dpr", ("{", "}"));
    
    // Elixir
    map.insert("ex", ("#=", "=#"));
    map.insert("exs", ("#=", "=#"));
    
    // Erlang
    map.insert("erl", ("%%%", "%%%"));
    map.insert("hrl", ("%%%", "%%%"));
    
    // F#
    map.insert("fs", ("(*", "*)"));
    map.insert("fsx", ("(*", "*)"));
    
    // Go
    map.insert("go", ("/*", "*/"));
    
    // Groovy
    map.insert("groovy", ("/*", "*/"));
    map.insert("gvy", ("/*", "*/"));
    
    // Haskell
    map.insert("hs", ("{-", "-}"));
    map.insert("lhs", ("{-", "-}"));
    
    // HTML
    map.insert("html", ("<!--", "-->"));
    map.insert("htm", ("<!--", "-->"));
    
    // Java
    map.insert("java", ("/*", "*/"));
    
    // JavaScript
    map.insert("js", ("/*", "*/"));
    map.insert("mjs", ("/*", "*/"));
    
    // Julia
    map.insert("jl", ("#=", "=#"));
    
    // Kotlin
    map.insert("kt", ("/*", "*/"));
    map.insert("kts", ("/*", "*/"));
    
    // LISP
    map.insert("lisp", ("#|", "|#"));
    map.insert("lsp", ("#|", "|#"));
    map.insert("cl", ("#|", "|#"));
    
    // Lua
    map.insert("lua", ("--[[", "]]"));
    
    // MATLAB
    map.insert("m", ("%{", "%}"));
    map.insert("mat", ("%{", "%}"));
    
    // OCaml
    map.insert("ml", ("(*", "*)"));
    map.insert("mli", ("(*", "*)"));
    
    // Pascal
    map.insert("pas", ("{", "}"));
    map.insert("pp", ("{", "}"));
    
    // Perl
    map.insert("pl", ("=pod", "=cut"));
    map.insert("pm", ("=pod", "=cut"));
    
    // PHP
    map.insert("php", ("/*", "*/"));
    
    // PowerShell
    map.insert("ps1", ("<#", "#>"));
    map.insert("psm1", ("<#", "#>"));
    map.insert("psd1", ("<#", "#>"));
    
    // Python
    map.insert("py", ("'''", "'''"));
    map.insert("pyw", ("'''", "'''"));
    
    // R
    map.insert("r", ("/*", "*/"));
    map.insert("R", ("/*", "*/"));
    
    // Ruby
    map.insert("rb", ("=begin", "=end"));
    map.insert("rbw", ("=begin", "=end"));
    
    // Rust
    map.insert("rs", ("/*", "*/"));
    
    // Scala
    map.insert("scala", ("/*", "*/"));
    map.insert("sc", ("/*", "*/"));
    
    // SQL
    map.insert("sql", ("/*", "*/"));
    
    // Swift
    map.insert("swift", ("/*", "*/"));
    
    // TypeScript
    map.insert("ts", ("/*", "*/"));
    map.insert("tsx", ("/*", "*/"));
    
    // VB.NET
    map.insert("vb", ("'''", "'''"));
    
    // XML
    map.insert("xml", ("<!--", "-->"));
    map.insert("xsl", ("<!--", "-->"));
    map.insert("xsd", ("<!--", "-->"));
    
    // YAML
    map.insert("yml", ("'''", "'''"));
    map.insert("yaml", ("'''", "'''"));
    
    // New mappings
    // HCL (Terraform)
    map.insert("tf", ("/*", "*/"));
    map.insert("tfvars", ("#", "single line"));
    map.insert("hcl", ("/*", "*/"));
    
    // TOML
    map.insert("toml", ("#", "single line"));
    
    // Kubernetes YAML
    map.insert("yaml", ("#", "single line"));
    map.insert("yml", ("#", "single line"));
    
    // Dockerfile
    map.insert("dockerfile", ("#", "single line"));
    map.insert("containerfile", ("#", "single line"));
    
    // Nginx config
    map.insert("nginx", ("#", "single line"));
    map.insert("conf", ("#", "single line"));
    
    // Apache config
    map.insert("htaccess", ("#", "single line"));
    map.insert("apache2.conf", ("#", "single line"));
    map.insert("httpd.conf", ("#", "single line"));
    
    // INI
    map.insert("ini", (";", "single line"));
    map.insert("cfg", (";", "single line"));
    map.insert("conf", (";", "single line"));
    
    // Properties files
    map.insert("properties", ("#", "single line"));
    map.insert("prop", ("#", "single line"));
    
    // Ansible YAML
    map.insert("ansible", ("#", "single line"));
    map.insert("yml", ("#", "single line"));
    
    // CloudFormation
    map.insert("template", ("#", "single line"));
    map.insert("cf.json", ("//", "single line"));
    map.insert("cf.yaml", ("#", "single line"));
    map.insert("cf.yml", ("#", "single line"));
    
    // ARM templates (Azure)
    map.insert("json", ("//", "single line"));
    map.insert("arm.json", ("//", "single line"));
    
    // Puppet
    map.insert("pp", ("/*", "*/"));
    map.insert("puppet", ("#", "single line"));
    
    // Chef
    map.insert("rb", ("#", "single line"));
    map.insert("chef", ("#", "single line"));
    
    // Salt
    map.insert("sls", ("#", "single line"));
    map.insert("salt", ("#", "single line"));
    
    // Bicep (Azure)
    map.insert("bicep", ("/*", "*/"));
    
    // Jsonnet
    map.insert("jsonnet", ("/*", "*/"));
    map.insert("libsonnet", ("/*", "*/"));
    
    // Groovy (Jenkins Pipeline)
    map.insert("jenkinsfile", ("/*", "*/"));
    map.insert("Jenkinsfile", ("/*", "*/"));
    
    // CircleCI config
    map.insert("circleci", ("#", "single line"));
    map.insert(".circleci", ("#", "single line"));
    
    // GitHub Actions YAML
    map.insert("workflow", ("#", "single line"));
    map.insert("github-action", ("#", "single line"));
    
    // Docker Compose
    map.insert("docker-compose.yml", ("#", "single line"));
    map.insert("docker-compose.yaml", ("#", "single line"));
    
    map
}