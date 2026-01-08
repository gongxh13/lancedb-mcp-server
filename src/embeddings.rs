use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::cmp::max;
use std::sync::Arc;
use tokio::sync::Mutex;
use text_embeddings_backend::{ModelType, Pool};
use text_embeddings_backend_core::{Backend, Batch, Embedding};
use tokenizers::Tokenizer;

pub enum EmbeddingEngine {
    Api {
        client: reqwest::Client,
        base_url: String,
        model_id: String,
    },
    Local {
        // We use Arc<Mutex<>> because the backend might not be Send/Sync or we need mutability
        backend: Arc<Mutex<text_embeddings_backend_candle::CandleBackend>>,
        tokenizer: Arc<Tokenizer>,
    },
}

pub struct EmbeddingModel {
    engine: EmbeddingEngine,
}

impl EmbeddingModel {
    pub async fn new(
        endpoint: Option<String>,
        model_id: Option<String>,
        api_key: Option<String>,
    ) -> Result<Self> {
        let model_id = model_id.unwrap_or_else(|| "Qwen/Qwen3-Embedding-0.6B".to_string());

        if let Some(base_url) = endpoint {
            let mut headers = reqwest::header::HeaderMap::new();
            if let Some(key) = api_key {
                let mut auth_value = reqwest::header::HeaderValue::from_str(&format!("Bearer {}", key))?;
                auth_value.set_sensitive(true);
                headers.insert(reqwest::header::AUTHORIZATION, auth_value);
            }

            Ok(Self {
                engine: EmbeddingEngine::Api {
                    client: reqwest::Client::builder()
                        .default_headers(headers)
                        .build()?,
                    base_url,
                    model_id,
                },
            })
        } else {
            // Local mode
            // Download model using hf_hub
            let api = hf_hub::api::tokio::Api::new()?;
            let repo = api.repo(hf_hub::Repo::new(
                model_id.clone(),
                hf_hub::RepoType::Model,
            ));
            
            let model_path = repo.get("model.safetensors").await?;
            // Ensure other files are present
            let _ = repo.get("config.json").await?;
            let tokenizer_path = repo.get("tokenizer.json").await?;
            
            let model_dir = model_path.parent().context("No parent dir")?.to_path_buf();

            // Load tokenizer
            let mut tokenizer = Tokenizer::from_file(tokenizer_path).map_err(|e| anyhow::anyhow!("Failed to load tokenizer: {}", e))?;
            // Configure tokenizer as in TEI
            if let Some(_pre_tokenizer) = tokenizer.get_pre_tokenizer() {
                // Simplified tokenizer setup for now, assuming standard config works
            }
            tokenizer.with_padding(None);

            // CandleBackend::new is synchronous and takes:
            // path: &Path
            // dtype: String (e.g., "float32")
            // model_type: ModelType
            // trust_remote_code: Option<Vec<String>> (or similar)
            let backend = text_embeddings_backend_candle::CandleBackend::new(
                &model_dir,
                "float32".to_string(),
                ModelType::Embedding(Pool::Mean),
                None,
            )?;

            Ok(Self {
                engine: EmbeddingEngine::Local {
                    backend: Arc::new(Mutex::new(backend)),
                    tokenizer: Arc::new(tokenizer),
                },
            })
        }
    }

    pub async fn embed(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
        match &self.engine {
            EmbeddingEngine::Api { client, base_url, model_id } => {
                if texts.is_empty() {
                    return Ok(Vec::new());
                }
                let url = format!("{}/v1/embeddings", base_url);
                let req = EmbeddingsRequest {
                    model: model_id.clone(),
                    input: texts,
                };
                let resp: EmbeddingsResponse = client
                    .post(url)
                    .json(&req)
                    .send()
                    .await?
                    .error_for_status()?
                    .json()
                    .await?;
                let vecs = resp
                    .data
                    .into_iter()
                    .map(|d| d.embedding)
                    .collect();
                Ok(vecs)
            }
            EmbeddingEngine::Local { backend, tokenizer } => {
                let backend = backend.lock().await;
                
                // Encode texts
                let encodings = tokenizer
                    .encode_batch(texts, true)
                    .map_err(|e| anyhow::anyhow!("Tokenization failed: {}", e))?;

                // Create Batch
                let mut input_ids = Vec::new();
                let mut token_type_ids = Vec::new();
                let mut position_ids = Vec::new();
                let mut cumulative_seq_lengths = Vec::with_capacity(encodings.len() + 1);
                cumulative_seq_lengths.push(0);
            
                let mut max_length = 0;
                let mut cumulative_length = 0;
            
                for encoding in encodings.iter() {
                    let encoding_length = encoding.len() as u32;
                    input_ids.extend(encoding.get_ids().to_vec());
                    token_type_ids.extend(encoding.get_type_ids().to_vec());
                    position_ids.extend(0..encoding_length);
                    cumulative_length += encoding_length;
                    cumulative_seq_lengths.push(cumulative_length);
                    max_length = max(max_length, encoding_length);
                }

                // We want pooled embeddings for all inputs
                let pooled_indices: Vec<u32> = (0..encodings.len() as u32).collect();
                let raw_indices = Vec::new();
            
                let batch = Batch {
                    input_ids,
                    token_type_ids,
                    position_ids,
                    cumulative_seq_lengths,
                    max_length,
                    pooled_indices,
                    raw_indices,
                };

                // Backend::embed is synchronous and returns Result<Embeddings>
                let embeddings_map = backend.embed(batch)?;
                
                // Convert map to ordered vector
                let mut results = vec![Vec::new(); encodings.len()];
                for (idx, embedding) in embeddings_map {
                    if idx < results.len() {
                        match embedding {
                            Embedding::Pooled(vec) => results[idx] = vec,
                            Embedding::All(_) => {
                                // We expect pooled embeddings
                            }
                        }
                    }
                }
                
                Ok(results)
            }
        }
    }
}

#[derive(Serialize)]
struct EmbeddingsRequest {
    model: String,
    input: Vec<String>,
}

#[derive(Deserialize)]
struct EmbeddingsResponse {
    data: Vec<EmbeddingData>,
}

#[derive(Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
}