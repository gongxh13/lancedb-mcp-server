use anyhow::Result;
use clap::Parser;
use rmcp::{
    ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
    ServiceExt, transport::{
        stdio,
        streamable_http_server::{
            StreamableHttpService,
            session::local::LocalSessionManager,
        },
    },
};
use std::sync::Arc;
use tokio::sync::Mutex;
use axum::{
    Router,
};
use tower_http::trace::TraceLayer;

mod db;
mod embeddings;

use db::VectorDB;
use embeddings::EmbeddingModel;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[arg(long, default_value = "./lancedb_data")]
    db_path: String,

    #[arg(long)]
    embedding_endpoint: Option<String>,

    #[arg(long)]
    embedding_model: Option<String>,

    #[arg(long, env = "OPENAI_API_KEY")]
    api_key: Option<String>,

    #[arg(long, default_value = "stdio")]
    transport: String, // stdio, streamable-http

    #[arg(long, default_value = "3000")]
    port: u16,
}

const DEFAULT_TABLE_NAME: &str = "knowledge_base";

#[derive(Debug, serde::Serialize)]
struct ApiResponse<T> {
    code: i32,
    message: String,
    data: Option<T>,
}

impl<T> ApiResponse<T> {
    fn success(data: T) -> Self {
        Self {
            code: 0,
            message: "success".to_string(),
            data: Some(data),
        }
    }
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct DocumentInput {
    #[schemars(description = "The name of the document")]
    name: String,
    #[schemars(description = "Optional description of the document")]
    description: Option<String>,
    #[schemars(description = "List of text chunks belonging to this document")]
    chunks: Vec<String>,
    #[schemars(description = "Additional custom metadata shared by all chunks in this document")]
    metadata: Option<serde_json::Value>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct AddDocumentsRequest {
    #[schemars(description = "The name of the table to add documents to (default: knowledge_base)")]
    table_name: Option<String>,
    #[schemars(description = "List of documents to add")]
    documents: Vec<DocumentInput>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct SearchRequest {
    #[schemars(description = "The name of the table to search in (default: knowledge_base)")]
    table_name: Option<String>,
    #[schemars(description = "The query text")]
    query: String,
    #[schemars(description = "Number of results to return")]
    limit: Option<usize>,
}

#[derive(Clone)]
struct LanceDBServer {
    db: Arc<VectorDB>,
    model: Arc<Mutex<EmbeddingModel>>,
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl LanceDBServer {
    fn new(db: Arc<VectorDB>, model: Arc<Mutex<EmbeddingModel>>) -> Self {
        Self {
            db,
            model,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(description = "Add documents to a LanceDB table. Supports batching multiple documents, where each document can have multiple chunks sharing the same metadata.")]
    async fn add_documents(&self, Parameters(req): Parameters<AddDocumentsRequest>) -> Result<String, String> {
        let table_name = req.table_name.as_deref().unwrap_or(DEFAULT_TABLE_NAME);
        
        let mut all_texts = Vec::new();
        let mut all_metadatas = Vec::new();
        let mut total_chunks = 0;
        let total_docs = req.documents.len();

        for doc in req.documents {
            // Prepare base metadata
            let mut base_metadata = doc.metadata.unwrap_or_else(|| serde_json::json!({}));
            
            // Inject name and description into metadata
            if let serde_json::Value::Object(ref mut map) = base_metadata {
                map.insert("name".to_string(), serde_json::Value::String(doc.name.clone()));
                if let Some(desc) = &doc.description {
                    map.insert("description".to_string(), serde_json::Value::String(desc.clone()));
                }
            }

            for chunk in doc.chunks {
                all_texts.push(chunk);
                all_metadatas.push(base_metadata.clone());
                total_chunks += 1;
            }
        }

        let model = self.model.lock().await;
        
        self.db.add_texts(table_name, all_texts, all_metadatas, &*model)
            .await
            .map_err(|e| e.to_string())?;
            
        let msg = format!("Successfully added {} documents ({} chunks) to table '{}'", total_docs, total_chunks, table_name);
        let resp = ApiResponse::success(msg);
        
        serde_json::to_string_pretty(&resp).map_err(|e| e.to_string())
    }

    #[tool(description = "Search for similar documents in a LanceDB table using semantic vector search.")]
    async fn search(&self, Parameters(req): Parameters<SearchRequest>) -> Result<String, String> {
        let table_name = req.table_name.as_deref().unwrap_or(DEFAULT_TABLE_NAME);
        let model = self.model.lock().await;
        let limit = req.limit.unwrap_or(5);
        
        let results = self.db.search(table_name, &req.query, limit, &*model)
            .await
            .map_err(|e| e.to_string())?;
            
        let resp = ApiResponse::success(results);
        serde_json::to_string_pretty(&resp).map_err(|e| e.to_string())
    }

    #[tool(description = "List all tables in the LanceDB database.")]
    async fn list_tables(&self) -> Result<String, String> {
        let tables = self.db.list_tables()
            .await
            .map_err(|e| e.to_string())?;
        
        let resp = ApiResponse::success(tables);
        serde_json::to_string_pretty(&resp).map_err(|e| e.to_string())
    }
}

#[tool_handler]
    impl ServerHandler for LanceDBServer {
        fn get_info(&self) -> ServerInfo {
            ServerInfo {
                instructions: Some("A generic LanceDB MCP server with local embedding support (Qwen 0.5B default).".into()),
                capabilities: ServerCapabilities::builder().enable_tools().build(),
                ..Default::default()
            }
        }
    }

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter("info")
        .init();

    let args = Cli::parse();

    tracing::info!("Initializing LanceDB at {}", args.db_path);
    let db = Arc::new(VectorDB::new(&args.db_path).await?);

    tracing::info!("Loading embedding model...");
    let model = Arc::new(Mutex::new(EmbeddingModel::new(
        args.embedding_endpoint,
        args.embedding_model,
        args.api_key
    ).await?));

    let server = LanceDBServer::new(db, model);

    match args.transport.as_str() {
        "stdio" => {
            tracing::info!("Starting MCP server on stdio...");
            let service = match server.serve(stdio()).await {
                Ok(s) => s,
                Err(e) => {
                    tracing::error!("Server error: {:?}", e);
                    return Err(e.into());
                }
            };
            service.waiting().await?;
        }
        "streamable-http" => {
            tracing::info!("Starting MCP server on Streamable HTTP transport at http://0.0.0.0:{}", args.port);
            let service = StreamableHttpService::new(
                move || Ok(server.clone()),
                LocalSessionManager::default().into(),
                Default::default()
            );

            let app = Router::new()
                .fallback_service(service)
                .layer(TraceLayer::new_for_http());

            let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", args.port)).await?;
            axum::serve(listener, app).await?;
        }
        _ => anyhow::bail!("Unknown transport: {}", args.transport),
    }

    Ok(())
}
