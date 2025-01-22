use std::io;
use std::path::{Path, PathBuf};
use git2::{Repository, Tree, Diff, DiffFormat};

pub fn get_diff_list(
    repo: &Repository,
    start_commit_id: Option<&str>,
    end_commit_id: Option<&str>,
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
    let diff = match (start_commit_id, end_commit_id) {
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

pub fn get_diff_str(diff: &Diff) -> io::Result<String> {
    let mut diff_output = Vec::new();
    diff.print(DiffFormat::Patch, |_delta, _hunk, line| {
        if let Ok(content) = std::str::from_utf8(line.content()) {
            diff_output.extend_from_slice(content.as_bytes());
        }
        true
    }).map_err(|e| io::Error::new(io::ErrorKind::Other, e.message().to_string()))?;

    String::from_utf8(diff_output).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

pub fn filter_diff_for_file(diff_str: &str, file_path: &Path) -> String {
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
