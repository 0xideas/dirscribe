use std::collections::HashMap;

pub fn load_prompts(_dir: &str) -> std::io::Result<HashMap<String, String>> {
    let mut prompts = HashMap::new();
    
    // Include prompt files at compile time
    prompts.insert(
        "summary-0.2".to_string(),
        include_str!("../prompts/summary-0.2.txt").to_string()
    );

    prompts.insert(
        "summary-keywords-0.1".to_string(),
        include_str!("../prompts/summary-keywords-0.1.txt").to_string()
    );
    
    prompts.insert(
        "summary-diff-0.1".to_string(),
        include_str!("../prompts/summary-diff-0.1.txt").to_string()
    );
    
    Ok(prompts)
}