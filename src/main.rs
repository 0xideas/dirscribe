use std::fs::{self, File};
mod cli;
mod git;
mod file_processing;
use cli::Cli;
use file_processing::{process_directory, process_file, should_include_file};
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



