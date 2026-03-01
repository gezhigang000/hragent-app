//! Custom OpenAI-compatible provider.
//!
//! Allows users to connect to any OpenAI-compatible API endpoint
//! (e.g. Ollama, Groq, LM Studio, vLLM) with a custom base URL,
//! model name, and optional API key.
//!
//! Reuses the shared helpers from `openai.rs`.

use anyhow::Result;
use reqwest::Client;

use crate::llm::providers::LlmProviderTrait;
use crate::llm::providers::openai::{
    send_openai_compat, stream_openai_compat, validate_key_openai_compat,
};
use crate::llm::streaming::{LlmRequest, LlmResponse, StreamBox};

/// Custom OpenAI-compatible provider.
pub struct CustomOpenAiProvider {
    api_key: String,
    base_url: String,
    model: String,
    tools_supported: bool,
    client: Client,
}

impl CustomOpenAiProvider {
    pub fn new(api_key: String, base_url: String, model: String, tools_supported: bool) -> Self {
        Self {
            api_key,
            base_url,
            model,
            tools_supported,
            client: super::build_http_client(),
        }
    }

    /// Build the full chat completions URL from the base URL.
    fn completions_url(&self) -> String {
        let base = self.base_url.trim_end_matches('/');
        // If the user already included /v1/chat/completions, use as-is
        if base.ends_with("/chat/completions") {
            base.to_string()
        } else if base.ends_with("/v1") {
            format!("{}/chat/completions", base)
        } else {
            format!("{}/v1/chat/completions", base)
        }
    }
}

impl LlmProviderTrait for CustomOpenAiProvider {
    fn name(&self) -> &str {
        "custom-openai"
    }

    fn supports_tools(&self) -> bool {
        self.tools_supported
    }

    async fn send(&self, request: LlmRequest) -> Result<LlmResponse> {
        let url = self.completions_url();
        send_openai_compat(
            &self.client,
            &self.api_key,
            &url,
            &self.model,
            &request,
            self.tools_supported,
        )
        .await
    }

    async fn stream(&self, request: LlmRequest) -> Result<StreamBox> {
        let url = self.completions_url();
        stream_openai_compat(
            &self.client,
            &self.api_key,
            &url,
            &self.model,
            &request,
            self.tools_supported,
            false,
        )
        .await
    }

    async fn validate_key(&self) -> Result<bool> {
        let url = self.completions_url();
        validate_key_openai_compat(&self.client, &self.api_key, &url, &self.model).await
    }
}
