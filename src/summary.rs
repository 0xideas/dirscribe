use reqwest::{Client, header};
use serde::{Deserialize, Serialize};
use anyhow::Result;
use std::env;
use std::collections::HashMap;
use tokio::sync::Semaphore;
use std::sync::Arc;
use anyhow::Context;


const MAX_CONCURRENT_REQUESTS: usize = 10;


#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    temperature: Option<f32>,
    max_tokens: Option<i32>,
    stream: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Message {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    id: String,
    model: String,
    choices: Vec<Choice>,
    usage: Usage,
}

#[derive(Debug, Deserialize)]
struct Choice {
    index: i32,
    message: Message,
    finish_reason: String,
}

#[derive(Debug, Deserialize)]
struct Usage {
    prompt_tokens: i32,
    completion_tokens: i32,
    total_tokens: i32,
}

pub struct DeepseekClient {
    client: Client,
    api_key: String,
    base_url: String,
}

impl DeepseekClient {
    pub fn new() -> Result<Self> {
        let client = Client::new();
        let api_key = env::var("DEEPSEEK_API_KEY")
        .context("DEEPSEEK_API_KEY not set")?;
        Ok(Self {
            client,
            api_key,
            base_url: "https://api.deepseek.com/chat/completions".to_string(),
        })
    }

    pub async fn chat(&self, file_path: &str, messages: &Vec<Message>, temperature: Option<f32>, max_tokens: Option<i32>) -> Result<ChatResponse> {
        let request = ChatRequest {
            model: "deepseek-chat".to_string(),
            messages: messages.clone(), // Clone the messages to own them
            temperature,
            max_tokens,
            stream: Some(false),
        };

        let mut headers = header::HeaderMap::new();
        headers.insert(
            "Authorization",
            format!("Bearer {}", self.api_key).parse().unwrap(),
        );
        headers.insert(
            "Content-Type",
            "application/json".parse().unwrap(),
        );

        let response = self.client
            .post(&self.base_url)
            .headers(headers)
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            anyhow::bail!("API request failed: {}", error_text);
        }

        // Print the raw response for debugging
        let response_text = response.text().await?;
        println!("\n\nRaw API Response {}: {}", file_path, response_text);
        
        // Parse the response text into our structure
        let chat_response: ChatResponse = serde_json::from_str(&response_text)
            .map_err(|e| anyhow::anyhow!("Failed to parse response: {}. Response body: {}", e, response_text))?;
        Ok(chat_response)
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
        
        let prompt = prompt_template.replace("${${CONTENT}$}$",  &content);

        let messages: Vec<Message> = vec![Message {
            role: "user".to_string(),
            content: prompt,
        }];

        let handle = tokio::spawn(async move {
            let result = client.chat(&file_path, &messages, None, None).await;
            drop(permit);
            match result {
                Ok(response) => Ok(response.choices[0].message.content.clone()),
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