use std::fs::File;
mod cli;
mod git;
mod file_processing;
mod output;
mod prompt_handling;
mod summary; 
mod validation;
use cli::Cli;
use file_processing::process_directory;
use output::{write_to_clipboard, process_with_template};
use clap::Parser;
use validation::validate_cli_args;
use anyhow::{Result, Context};
use std::io::Write;
use std::path::PathBuf;
use prompt_handling::load_prompts;



#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    assert!(
        cli.suffixes == "*" || !cli.suffixes.chars().any(|s| s == '*'),
        "\"*\" can only be used alone, file extensions are specified without wildcard, like 'py,toml,js'"
    );

    if let Err(e) = validate_cli_args(&cli) {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }

    
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

    // Read the file contents into a String
    let summarize_prompt_templates = load_prompts("prompts").context("Failed to load prompt templates")?;
    // Process directory and get the content string
    let content = process_directory(
        ".",
        &suffixes,
        cli.dont_use_gitignore,
        cli.summarize,
        summarize_prompt_templates,
        cli.apply,
        cli.retrieve,
        cli.diff_only,
        &exclude_paths,
        &include_paths,
        &or_keywords,
        &and_keywords,
        &exclude_keywords,
        cli.start_commit_id.as_deref(),
        cli.end_commit_id.as_deref()
    ).await?;

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



