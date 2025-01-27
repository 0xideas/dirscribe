use std::fs;
use anyhow::{Result, Context, bail};
use clipboard::{ClipboardContext, ClipboardProvider};

pub fn write_to_clipboard(content: &str) -> Result<()> {
    let mut ctx: ClipboardContext = ClipboardProvider::new()
        .context("Failed to create clipboard context")?;
    
    ctx.set_contents(content.to_owned())
        .context("Failed to set clipboard contents")?;
    
    Ok(())
}

pub fn process_with_template(content: &str, template_path: &str) -> Result<String> {
    // Read the template file
    let template = fs::read_to_string(template_path)
        .context("Failed to read template file")?;

    // Check for the required placeholder
    if !template.contains("${${CONTENT}$}$") {
        bail!("Template file must contain the placeholder '${${CONTENT}$}$'");
    }

    // Replace the placeholder with the content
    Ok(template.replace("${${CONTENT}$}$", content))
}
