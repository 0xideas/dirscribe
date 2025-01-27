use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json;
use anyhow::{Context, Result, bail};
use std::env;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
use backoff::{ExponentialBackoff, retry};

const API_TIMEOUT_SECONDS: u64 = 30;
const MAX_RETRIES: u32 = 1;
const MAX_CONCURRENT_REQUESTS: usize = 5;
const DEFAULT_MODEL: &str = "deepseek-chat";
const MAX_INPUT_LENGTH: usize = 4096;  // Example limit, check actual API docs

#[derive(Debug, Serialize)]
struct DeepseekRequest {
    model: String,
    messages: Vec<Message>
}

#[derive(Debug, Serialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct DeepseekResponse {
    choices: Vec<Choice>,
    #[serde(default)]
    error: Option<DeepseekError>,
}

#[derive(Debug, Deserialize)]
struct DeepseekError {
    message: String,
    #[serde(default)]
    code: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ResponseMessage,
}

#[derive(Debug, Deserialize)]
struct ResponseMessage {
    content: String,
}

struct DeepseekClient {
    client: Client,
    api_key: String,
    model: String,
    base_url: String,
}

impl DeepseekClient {
    pub fn new() -> Result<Self> {
        // Check for API key at initialization
        let api_key = env::var("DEEPSEEK_API_KEY")
            .context("DEEPSEEK_API_KEY not set")?;
            
        // Create client with timeout
        let client = Client::builder()
            .timeout(Duration::from_secs(API_TIMEOUT_SECONDS))
            .build()
            .context("Failed to create HTTP client")?;
            
        Ok(Self {
            client,
            api_key,
            model: DEFAULT_MODEL.to_string(),
            base_url: format!("https://api.deepseek.com/chat/completions"),
        })
    }

    pub fn with_model(mut self, model: String) -> Self {
        self.model = model;
        self
    }

    async fn get_summary_with_retry(&self, text: &str, prompt_template: &str) -> Result<String> {
        let mut attempts = 0;
        let max_attempts = MAX_RETRIES;
        let mut delay_ms = 1000; // Start with 1 second delay

        loop {
            attempts += 1;
            match self.get_summary_once(text, prompt_template).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    if attempts >= max_attempts {
                        return Err(e.context("Max retry attempts reached"));
                    }

                    // Only retry on certain errors (like rate limits)
                    if !matches!(e.downcast_ref::<reqwest::Error>(), Some(e) if e.is_timeout() || e.is_connect()) 
                        && !e.to_string().contains("Rate limit exceeded") {
                        return Err(e);
                    }

                    // Exponential backoff with jitter
                    let jitter = rand::random::<u64>() % 100;
                    tokio::time::sleep(Duration::from_millis(delay_ms + jitter)).await;
                    delay_ms *= 2; // Double the delay for next attempt
                }
            }
        }
    }

    async fn get_summary_once(&self, text: &str, prompt_template: &str) -> Result<String> {
        // Validate input length
        if text.len() > MAX_INPUT_LENGTH {
            bail!("Input text exceeds maximum length of {} characters", MAX_INPUT_LENGTH);
        }

        // Replace placeholder in template
        let prompt = prompt_template.replace("${${CONTENT}$}$", text);

        // Construct request
        let request = DeepseekRequest {
            model: self.model.clone(),
            messages: vec![Message {
                role: "user".to_string(),
                content: prompt,
            }]
        };

        let request_body = serde_json::to_string(&request)
            .unwrap_or_else(|_| String::from("{}"));
            
        println!("\ncurl -X POST \"{}\" \\", self.base_url);
        println!("  -H \"Authorization: Bearer {}\" \\", self.api_key);
        println!("  -H \"Content-Type: application/json\" \\");
        println!("  -d '{}'", request_body.replace("'", "'\"'\"'"));
            
        // Send request
        let response = self.client
            .post(&self.base_url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&request)
            .send()
            .await
            .context("Failed to send request to Deepseek API")?;
            
        println!("\n=== Received response ===");
        println!("Status: {}", response.status());

        // Handle different status codes
        match response.status() {
            StatusCode::OK => {
                let deepseek_response = response
                    .json::<DeepseekResponse>()
                    .await
                    .context("Failed to parse Deepseek API response")?;

                // Check for API-level errors
                if let Some(error) = deepseek_response.error {
                    bail!("Deepseek API error: {} (code: {:?})", 
                          error.message, error.code);
                }

                // Validate response
                let summary = deepseek_response
                    .choices
                    .first()
                    .context("No response choices available")?
                    .message
                    .content
                    .clone();

                if summary.trim().is_empty() {
                    bail!("Received empty summary from API");
                }

                Ok(summary)
            },
            StatusCode::TOO_MANY_REQUESTS => {
                bail!("Rate limit exceeded");
            },
            StatusCode::UNAUTHORIZED => {
                bail!("Invalid API key");
            },
            status => {
                bail!("Unexpected status code: {}", status);
            }
        }
    }
}

pub async fn get_summaries(
    valid_files: Vec<String>, 
    file_contents: HashMap<String, String>, 
    prompt_template: String
) -> Result<Vec<String>> {
    // Initialize API client once
    let client = Arc::new(DeepseekClient::new()?);
    
    // Use a smaller number of concurrent requests to avoid rate limits
    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_REQUESTS));
    
    let mut handles = Vec::new();
    
    for file_path in valid_files {
        let permit = semaphore.clone().acquire_owned().await?;
        let content = file_contents.get(&file_path).unwrap_or(&String::new()).clone();
        let template = prompt_template.clone();
        let file_path_clone = file_path.clone();
        let client = client.clone();
        
        let handle = tokio::spawn(async move {
            let result = client.get_summary_with_retry(&content, &template).await;
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