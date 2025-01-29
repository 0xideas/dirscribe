use reqwest::{Client, header};
use serde::{Deserialize, Serialize};
use tokio::time::{sleep, Duration};
use anyhow::{Result, Context};
use std::env;
use std::path::Path;
use std::collections::HashMap;
use tokio::sync::Semaphore;
use std::sync::Arc;
use std::str::FromStr;
use crate::file_processing::filter_dirscribe_sections;

const DEFAULT_CONCURRENT_REQUESTS: usize = 10;
const ANTHROPIC_MAX_TOKENS: i32 = 512;
const ANTHROPIC_TEMPERATURE: f32 = 0.1;
const MAX_RETRIES: u32 = 6;
const INITIAL_BACKOFF_MS: u64 = 1000;

const DEFAULT_DEEPSEEK_MODEL: &str = "deepseek-chat";
const DEFAULT_ANTHROPIC_MODEL: &str = "claude-3-sonnet-20240229";
const DEFAULT_OLLAMA_MODEL: &str = "deepseek-r1:8b";

#[derive(Debug, Clone, Copy)]
pub enum Provider {
    Deepseek,
    Anthropic,
    Ollama,
}

// Implement FromStr for Provider to parse environment variable
impl FromStr for Provider {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "deepseek" => Ok(Provider::Deepseek),
            "anthropic" => Ok(Provider::Anthropic),
            "ollama" => Ok(Provider::Ollama),
            _ => Err(anyhow::anyhow!("Invalid provider: {}. Valid options are: deepseek, anthropic, ollama", s))
        }
    }
}

// Common message structure used across providers
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Message {
    pub role: String,
    pub content: String,
}

// Unified response structure
#[derive(Debug)]
pub struct UnifiedResponse {
    pub content: String,
}

pub struct UnifiedClient {
    client: Client,
    provider: Provider,
    api_key: String,
    base_url: String,
    model: String,
}

impl UnifiedClient {
    pub fn new(provider: Provider) -> Result<Self> {
        let client = Client::new();
        

        let (api_key, base_url, model) = match provider {
            Provider::Deepseek => {
                let key = env::var("PROVIDER_API_KEY")
                    .context("PROVIDER_API_KEY not set")?;
                let model = env::var("DIRSCRIBE_MODEL")
                    .unwrap_or_else(|_| DEFAULT_DEEPSEEK_MODEL.to_string());
                (
                    key,
                    "https://api.deepseek.com/v1/chat/completions".to_string(),
                    model,
                )
            }
            Provider::Anthropic => {
                let key = env::var("PROVIDER_API_KEY")
                    .context("PROVIDER_API_KEY not set")?;
                let model = env::var("DIRSCRIBE_MODEL")
                    .unwrap_or_else(|_| DEFAULT_ANTHROPIC_MODEL.to_string());
                (
                    key,
                    "https://api.anthropic.com/v1/messages".to_string(),
                    model,
                )
            }
            Provider::Ollama => {
                let model = env::var("DIRSCRIBE_MODEL")
                    .unwrap_or_else(|_| DEFAULT_OLLAMA_MODEL.to_string());
                (
                    String::new(), // No API key needed for local Ollama
                    "http://localhost:11434/api/generate".to_string(),
                    model,
                )
            }
        };

        Ok(Self {
            client,
            provider,
            api_key,
            base_url,
            model,
        })
    }

    fn build_headers(&self) -> Result<header::HeaderMap> {
        let mut headers = header::HeaderMap::new();
        
        match self.provider {
            Provider::Deepseek => {
                headers.insert(
                    "Authorization",
                    format!("Bearer {}", self.api_key).parse().unwrap(),
                );
            }
            Provider::Anthropic => {
                headers.insert(
                    "x-api-key",
                    self.api_key.parse().unwrap(),
                );
                headers.insert(
                    "anthropic-version",
                    "2023-06-01".parse().unwrap(),
                );
            }
            Provider::Ollama => {}
        }
        
        headers.insert(
            "Content-Type",
            "application/json".parse().unwrap(),
        );
        
        Ok(headers)
    }

    fn build_request(&self, messages: Vec<Message>, temperature: Option<f32>, max_tokens: Option<i32>) -> serde_json::Value {
        match self.provider {
            Provider::Deepseek => {
                serde_json::json!({
                    "model": self.model,
                    "messages": messages,
                    "temperature": temperature,
                    "max_tokens": max_tokens,
                    "stream": false
                })
            }
            Provider::Anthropic => {
                serde_json::json!({
                    "model": self.model,
                    "messages": messages,
                    "max_tokens": ANTHROPIC_MAX_TOKENS,
                    "temperature": ANTHROPIC_TEMPERATURE
                })
            }
            Provider::Ollama => {
                // For Ollama, we'll concatenate all messages into a single prompt
                let prompt = messages.iter()
                    .map(|m| format!("{}: {}", m.role, m.content))
                    .collect::<Vec<_>>()
                    .join("\n");
                
                serde_json::json!({
                    "model": self.model,
                    "prompt": prompt,
                    "stream": false
                })
            }
        }
    }

    async fn parse_response(&self, response_text: String) -> Result<UnifiedResponse> {
        match self.provider {
            Provider::Deepseek => {
                #[derive(Debug, Deserialize)]
                struct DeepseekResponse {
                    choices: Vec<DeepseekChoice>,
                    #[allow(dead_code)]
                    usage: DeepseekUsage,
                }
                
                #[derive(Debug, Deserialize)]
                struct DeepseekChoice {
                    message: Message,
                }
                
                #[derive(Debug, Deserialize)]
                #[allow(dead_code)]
                struct DeepseekUsage {
                    total_tokens: i32,
                }

                let response: DeepseekResponse = serde_json::from_str(&response_text)?;
                Ok(UnifiedResponse {
                    content: response.choices[0].message.content.clone()
                })
            }
            Provider::Anthropic => {
                #[derive(Debug, Deserialize)]
                struct AnthropicResponse {
                    content: Vec<AnthropicContent>,
                    #[allow(dead_code)]
                    usage: AnthropicUsage,
                }
                
                #[derive(Debug, Deserialize)]
                struct AnthropicContent {
                    #[serde(rename = "type")]
                    #[allow(dead_code)]
                    content_type: String,
                    #[serde(rename = "text")]
                    message: String,
                }
                
                #[derive(Debug, Deserialize)]
                #[allow(dead_code)]
                struct AnthropicUsage {
                    input_tokens: i32,
                    output_tokens: i32,
                }

                let response: AnthropicResponse = serde_json::from_str(&response_text)?;
                Ok(UnifiedResponse {
                    content: response.content[0].message.clone()
                })
            }
            Provider::Ollama => {
                #[derive(Debug, Deserialize)]
                struct OllamaResponse {
                    response: String,
                    #[allow(dead_code)]
                    done: bool,
                }
                let response: OllamaResponse = serde_json::from_str(&response_text)?;
                let content = if response.response.contains("</think>") {
                    response.response
                        .split("</think>")
                        .nth(1)
                        .unwrap_or(&response.response)
                        .trim()
                        .to_string()
                } else {
                    response.response.clone()
                };
                
                Ok(UnifiedResponse {
                    content
                })
            }
        }
    }

    pub async fn chat(&self, suffix_map: &HashMap<&'static str, (&'static str, &'static str)>, diff_only: bool,  file_path: &str, messages: &Vec<Message>, temperature: Option<f32>, max_tokens: Option<i32>) -> Result<UnifiedResponse> {
        let request = self.build_request(messages.clone(), temperature, max_tokens);
        let headers = self.build_headers()?;
        
        let mut retries = 0;
        let mut backoff_ms = INITIAL_BACKOFF_MS;
    
        loop {
            let response = self.client
                .post(&self.base_url)
                .headers(headers.clone())
                .json(&request)
                .send()
                .await?;
    
            let status = response.status();
            let response_text = response.text().await?;
            
            // First check if the request was successful
            if status.is_success() {
                // Try to parse the response
                match self.parse_response(response_text.clone()).await {
                    Ok(parsed_response) => {
                        // Check if the summary is valid
                        if diff_only | check_summary(Path::new(file_path), &parsed_response.content, suffix_map) {
                            return Ok(parsed_response);
                        } else {
                            // If summary validation fails, treat it like a retriable error
                            if retries >= MAX_RETRIES {
                                anyhow::bail!("Max retries exceeded. Could not generate valid summary format");
                            }
                            // Continue to retry logic
                        }
                    }
                    Err(e) => {
                        // If parsing fails and we're out of retries, bail
                        if retries >= MAX_RETRIES {
                            anyhow::bail!("Failed to parse response after {} retries: {}", MAX_RETRIES, e);
                        }
                        // Continue to retry logic
                    }
                }
            } else if !status.is_server_error() && status != 429 {
                // Only bail immediately on non-retriable errors
                anyhow::bail!("API request failed with non-retriable error: {}", response_text);
            }
    
            // Retry logic
            if retries >= MAX_RETRIES {
                anyhow::bail!("Max retries exceeded. Last error: {} {}", status, response_text);
            }
    
            sleep(Duration::from_millis(backoff_ms)).await;
            retries += 1;
            backoff_ms *= 2;
        }
    }
}

pub async fn get_summaries(
    valid_files: Vec<String>, 
    file_contents: HashMap<String, String>, 
    prompt_template: String,
    suffix_map: HashMap<&'static str, (&'static str, &'static str)>,
    diff_only:bool
) -> Result<Vec<String>> {
    // Get provider from environment variable, default to Ollama if not set
    let provider = env::var("DIRSCRIBE_PROVIDER")
        .map(|p| Provider::from_str(&p))
        .unwrap_or(Ok(Provider::Ollama))?;

    let client = Arc::new(UnifiedClient::new(provider)?);
    let max_concurrent_requests: usize =  env::var("DIRSCRIBE_CONCURRENT_REQUESTS").unwrap_or_else(|_| DEFAULT_CONCURRENT_REQUESTS.to_string()).parse().unwrap_or(DEFAULT_CONCURRENT_REQUESTS);

    let semaphore = Arc::new(Semaphore::new(max_concurrent_requests));
    let suffix_map = Arc::new(suffix_map);
    
    // Rest of the function remains the same
    let mut handles = Vec::new();
    
    for file_path in valid_files {
        let permit = semaphore.clone().acquire_owned().await?;
        let content = file_contents.get(&file_path).unwrap_or(&String::new()).clone();
        let processed_content = filter_dirscribe_sections(&content, true);
        let file_path_clone = file_path.clone();
        let client = client.clone();
        let suffix_map = Arc::clone(&suffix_map);
        let prompt_template = prompt_template.clone();

        let extension = Path::new(&file_path)
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or(""); 

        let prompt_base = prompt_template.replace("${${CONTENT}$}$", &processed_content);
        let prompt = if !diff_only {
            if let Some((multi_line_comment_start, multi_line_comment_end)) = suffix_map.get(extension) {
                if multi_line_comment_end != &"single line" {
                    prompt_base.to_owned() + &format!("\n\nPlease use the following structure: line 1: '{}', line 2: '[DIRSCRIBE]', lines 3 to N -2: *the summary*, line N-1: '[/DIRSCRIBE]', line N: '{}'", 
                        multi_line_comment_start, multi_line_comment_end)
                } else {
                    prompt_base.to_owned() + &format!("\n\nPlease make sure to start every line of the summary with '{}'. Please use the following structure: line 1: '{}', line 2: '{} [DIRSCRIBE]', lines 3 to N -2: *the summary*, line N-1: '{} [/DIRSCRIBE]', line N: '{}'", 
                        multi_line_comment_start, multi_line_comment_start, multi_line_comment_start, multi_line_comment_start, multi_line_comment_start)
                }
            } else {
                prompt_base.to_owned() + &"\n\nPlease make sure to return the summary as a comment block appropriately formatted for the language, with this structure: line 1: , line 2: [DIRSCRIBE], line N-1: [/DIRSCRIBE], line N: . Lines 1 and N should be empty."
            }
        } else {
            prompt_base.to_string()
        };

        let messages: Vec<Message> = vec![Message {
            role: "user".to_string(),
            content: prompt,
        }];

        let handle = tokio::spawn(async move {
            let result = client.chat(&suffix_map, diff_only, &file_path_clone, &messages, None, None).await;
            drop(permit);
            match result {
                Ok(response) => Ok(response.content),
                Err(e) => Err(anyhow::anyhow!("Error processing file {}: {}", file_path_clone, e))
            }
        });
        
        handles.push(handle);
    }
    
    let mut results = Vec::new();
    for handle in handles {
        match handle.await? {
            Ok(content) => results.push(content),
            Err(e) => results.push(format!("Error: {}", e)),
        }
    }
    Ok(results)
}

pub fn check_summary(file_path: &Path, s: &str, suffix_map: &HashMap<&'static str, (&'static str, &'static str)>) -> bool {
    let extension = file_path.extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or(""); 
    if let Some((multi_line_comment_start, multi_line_comment_end)) = suffix_map.get(extension) {
        let lines: Vec<&str> = s.trim().split('\n').collect();
        if lines.len() < 4 {
            return false;
        }
        if multi_line_comment_end != &"single line" {
            let comment_start = lines[0].trim() == *multi_line_comment_start;
            let dirscribe_start = lines[1].trim() == "[DIRSCRIBE]";
            let dirscribe_end = lines[lines.len() - 2].trim() == "[/DIRSCRIBE]";
            let comment_end = lines[lines.len() - 1].trim() == *multi_line_comment_end;
            comment_start && dirscribe_start && dirscribe_end && comment_end
        } else {
            let comment_start = lines[0].trim() == *multi_line_comment_start;
            let dirscribe_start = lines[1].trim() == format!("{} [DIRSCRIBE]", multi_line_comment_start);
            let dirscribe_end = lines[lines.len() - 2].trim() == format!("{} [/DIRSCRIBE]", multi_line_comment_start);
            let comment_end = lines[lines.len() - 1].trim() == *multi_line_comment_start;
            comment_start && dirscribe_start && dirscribe_end && comment_end
        }

    } else {
        false
    }
}