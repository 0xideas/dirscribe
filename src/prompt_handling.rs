use std::collections::HashMap;
use std::fs;

pub fn load_prompts(dir: &str) -> std::io::Result<HashMap<String, String>> {
    Ok(fs::read_dir(dir)?
        .filter_map(Result::ok)
        .filter(|e| e.path().is_file())
        .filter_map(|e| {
            let path = e.path();
            Some((
                path.file_stem()?.to_str()?.to_string(),
                fs::read_to_string(path).ok()?
            ))
        })
        .collect())
}