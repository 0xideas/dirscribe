use reqwest::{Client, header};
use serde::{Deserialize, Serialize};
use tokio::time::{sleep, Duration};
use anyhow::Result;
use std::env;
use std::collections::HashMap;
use tokio::sync::Semaphore;
use std::sync::Arc;
use anyhow::Context;

const MAX_CONCURRENT_REQUESTS: usize = 1;
const ANTHROPIC_MAX_TOKENS: i32 = 300;
const ANTHROPIC_TEMPERATURE: f32 = 0.1;
const MAX_RETRIES: u32 = 6;
const INITIAL_BACKOFF_MS: u64 = 1000;

#[derive(Debug, Clone, Copy)]
pub enum Provider {
    Deepseek,
    Anthropic,
    Ollama,
}

// Common message structure used across providers
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Message {
    pub role: String,
    pub content: String,
}

// Trait for provider-specific request structures
trait ProviderRequest {
    fn build_request(&self, messages: Vec<Message>, temperature: Option<f32>, max_tokens: Option<i32>) -> serde_json::Value;
}

// Provider-specific request structures
#[derive(Debug, Serialize)]
struct DeepseekRequest {
    model: String,
    messages: Vec<Message>,
    temperature: Option<f32>,
    max_tokens: Option<i32>,
    stream: Option<bool>,
}

#[derive(Debug, Serialize)]
struct AnthropicRequest {
    model: String,
    messages: Vec<Message>,
    max_tokens: Option<i32>,
    temperature: Option<f32>,
}

#[derive(Debug, Serialize)]
struct OllamaRequest {
    model: String,
    prompt: String,
    stream: bool,
}



// Unified response structure
#[derive(Debug)]
pub struct UnifiedResponse {
    pub content: String,
    pub total_tokens: i32,
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
                let key = env::var("DEEPSEEK_API_KEY")
                    .context("DEEPSEEK_API_KEY not set")?;
                (
                    key,
                    "https://api.deepseek.com/v1/chat/completions".to_string(),
                    "deepseek-chat".to_string(),
                )
            }
            Provider::Anthropic => {
                let key = env::var("ANTHROPIC_API_KEY")
                    .context("ANTHROPIC_API_KEY not set")?;
                (
                    key,
                    "https://api.anthropic.com/v1/messages".to_string(),
                    "claude-3-sonnet-20240229".to_string(),
                )
            }
            Provider::Ollama => {
                (
                    String::new(), // No API key needed for local Ollama
                    "http://localhost:11434/api/generate".to_string(),
                    "deepseek-r1:8b".to_string(), // Default model, can be made configurable
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
                    usage: DeepseekUsage,
                }
                
                #[derive(Debug, Deserialize)]
                struct DeepseekChoice {
                    message: Message,
                }
                
                #[derive(Debug, Deserialize)]
                struct DeepseekUsage {
                    total_tokens: i32,
                }

                let response: DeepseekResponse = serde_json::from_str(&response_text)?;
                Ok(UnifiedResponse {
                    content: response.choices[0].message.content.clone(),
                    total_tokens: response.usage.total_tokens,
                })
            }
            Provider::Anthropic => {
                #[derive(Debug, Deserialize)]
                struct AnthropicResponse {
                    content: Vec<AnthropicContent>,
                    usage: AnthropicUsage,
                }
                
                #[derive(Debug, Deserialize)]
                struct AnthropicContent {
                    #[serde(rename = "type")]
                    content_type: String,
                    #[serde(rename = "text")]
                    message: String,
                }
                
                #[derive(Debug, Deserialize)]
                struct AnthropicUsage {
                    input_tokens: i32,
                    output_tokens: i32,
                }

                let response: AnthropicResponse = serde_json::from_str(&response_text)?;
                Ok(UnifiedResponse {
                    content: response.content[0].message.clone(),
                    total_tokens: response.usage.input_tokens + response.usage.output_tokens,
                })
            }
            Provider::Ollama => {
                #[derive(Debug, Deserialize)]
                struct OllamaResponse {
                    response: String,
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
                    content,
                    total_tokens: 0, // Ollama doesn't provide token counts
                })
            }
        }
    }

    pub async fn chat(&self, file_path: &str, messages: &Vec<Message>, temperature: Option<f32>, max_tokens: Option<i32>) -> Result<UnifiedResponse> {
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

            if response.status().is_success() {
                let response_text = response.text().await?;
                println!("\n\nRaw API Response {}: {}", file_path, response_text);
                return self.parse_response(response_text).await;
            }

            let status = response.status();
            if !status.is_server_error() && status != 429 {
                let error_text = response.text().await?;
                anyhow::bail!("API request failed: {}", error_text);
            }

            if retries >= MAX_RETRIES {
                let error_text = response.text().await?;
                anyhow::bail!("Max retries exceeded. Last error: {}", error_text);
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
    prompt_template: String
) -> Result<Vec<String>> {
    let provider = Provider::Ollama;
    let client = Arc::new(UnifiedClient::new(provider)?);
    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_REQUESTS));
    
    let mut handles = Vec::new();
    
    for file_path in valid_files {
        let permit = semaphore.clone().acquire_owned().await?;
        let content = file_contents.get(&file_path).unwrap_or(&String::new()).clone();
        let file_path_clone = file_path.clone();
        let client = client.clone();
        
        let prompt = prompt_template.replace("${${CONTENT}$}$", &content);

        let messages: Vec<Message> = vec![Message {
            role: "user".to_string(),
            content: prompt,
        }];

        let handle = tokio::spawn(async move {
            let result = client.chat(&file_path, &messages, None, None).await;
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
