use reqwest::Client;
use serde::{Deserialize, Serialize};
use anyhow::{Context, Result};
use std::env;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Semaphore;

#[derive(Serialize)]
struct DeepseekRequest {
    model: String,
    messages: Vec<Message>,
    temperature: f32,
}

#[derive(Serialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct DeepseekResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: ResponseMessage,
}

#[derive(Deserialize)]
struct ResponseMessage {
    content: String,
}

pub async fn get_summaries(valid_files: Vec<String>, file_contents: HashMap<String, String>, prompt_template: String) -> Result<Vec<String>> {
    let semaphore = Arc::new(Semaphore::new(10));
    
    let mut handles = Vec::new();
    
    for file_path in valid_files {
        let permit = semaphore.clone().acquire_owned().await?;
        let content = file_contents.get(&file_path).unwrap_or(&String::new()).clone();
        let template = prompt_template.clone();
        let file_path_clone = file_path.clone();
        
        let handle = tokio::spawn(async move {
            let result = get_summary(content, template).await;
            drop(permit);
            match result {
                Ok(summary) => summary,
                Err(e) => format!("Error processing file {}: {}", file_path_clone, e)
            }
        });
        
        handles.push(handle);
    }
    
    let mut results = Vec::new();
    for handle in handles {
        results.push(handle.await?);
    }
    
    Ok(results)
}

pub async fn get_summary(text: String, prompt_template: String) -> Result<String> {
    // Create API client
    let client = Client::new();
    
    // Replace placeholder in template with actual text
    let prompt = prompt_template.replace("{text}", &text);
    
    // Construct request payload
    let request = DeepseekRequest {
        model: "deepseek-chat".to_string(),
        messages: vec![Message {
            role: "user".to_string(),
            content: prompt,
        }],
        temperature: 0.7,
    };
    
    // Send request to Deepseek API
    let response = client
        .post("https://api.deepseek.com/v1/chat/completions")
        .header(
            "Authorization",
            format!("Bearer {}", env::var("DEEPSEEK_API_KEY").context("DEEPSEEK_API_KEY not set")?)
        )
        .json(&request)
        .send()
        .await
        .context("Failed to send request to Deepseek API")?;
    
    // Parse response
    let deepseek_response = response
        .json::<DeepseekResponse>()
        .await
        .context("Failed to parse Deepseek API response")?;
    
    // Extract summary from response
    let summary = deepseek_response
        .choices
        .first()
        .context("No response choices available")?
        .message
        .content
        .clone();
    
    Ok(summary)
}

// Example usage:
#[tokio::main]
async fn main() -> Result<()> {
    let text = "Your text to summarize".to_string();
    let template = std::fs::read_to_string("prompt_template.txt")?;
    
    let summary = get_summary(text, template).await?;
    println!("Summary: {}", summary);
    
    Ok(())
}
