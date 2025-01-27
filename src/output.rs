use std::fs;
use anyhow::{Result, bail};
use clipboard::{ClipboardContext, ClipboardProvider};

pub fn write_to_clipboard(content: &str) -> Result<()> {
    let mut ctx: ClipboardContext = ClipboardProvider::new()
        .map_err(|e| anyhow::anyhow!("Failed to create clipboard context: {}", e))?;
    
    ctx.set_contents(content.to_owned())
        .map_err(|e| anyhow::anyhow!("Failed to set clipboard contents: {}", e))?;
    
    Ok(())
}

pub fn process_with_template(content: &str, template_path: &str) -> Result<String> {
    // Read the template file
    let template = fs::read_to_string(template_path)
        .map_err(|e| anyhow::anyhow!("Failed to read template file: {}", e))?;

    // Check for the required placeholder
    if !template.contains("${${CONTENT}$}$") {
        bail!("Template file must contain the placeholder '${{${{CONTENT}}$}}$'");
    }

    // Replace the placeholder with the content
    Ok(template.replace("${${CONTENT}$}$", content))
}
