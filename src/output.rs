use std::fs;
use std::io;
use clipboard::{ClipboardContext, ClipboardProvider};

pub fn write_to_clipboard(content: &str) -> io::Result<()> {
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

pub fn process_with_template(content: &str, template_path: &str) -> io::Result<String> {
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
