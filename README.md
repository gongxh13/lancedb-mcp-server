# LanceDB MCP Server

一个基于 LanceDB 的模型上下文协议 (MCP) 服务器，使 LLM 能够与本地向量数据库进行交互。

## 功能特性

- **Embedding 支持**：
  - **本地模式**：自动下载并运行 embedding 模型（默认：`Qwen/Qwen3-Embedding-0.6B`）。
  - **API 模式**：支持连接 OpenAI 兼容接口或 TEI 服务。
- **多种传输协议**：
  - **Stdio**：标准输入输出（默认，适合本地 LLM 客户端）。
  - **Streamable HTTP**：支持 HTTP SSE/Post 模式，方便远程部署。
- **MCP 工具集**：
  - `add_documents`：添加文本并自动生成向量，支持自定义元数据。
  - `search`：对文档进行语义搜索。
  - `list_tables`：列出所有可用的表。

## 安装

```bash
cargo install --path .
```

## 使用方法

### 1. 本地运行 (默认)

默认情况下，服务器会自动下载模型（Qwen/Qwen3-Embedding-0.6B）并在本地进行推理。使用 stdio 传输协议（适合 Claude Desktop）。

```bash
lancedb-mcp-server
```

### 2. Streamable HTTP 模式

支持通过 HTTP 协议提供服务（Streamable HTTP）：

```bash
lancedb-mcp-server --transport streamable-http --port 3000
```

### 3. 连接远程 Embedding 服务 (如 TEI 或 OpenAI)

你可以通过指定 endpoint 来使用远程 Embedding 服务：

**使用 TEI (Text Embeddings Inference):**

```bash
lancedb-mcp-server \
  --embedding-endpoint http://localhost:8080 \
  --embedding-model your-model-id
```

**使用 OpenAI 兼容接口:**

```bash
lancedb-mcp-server \
  --embedding-endpoint https://api.openai.com/v1 \
  --embedding-model text-embedding-3-small \
  --api-key sk-your-api-key
```

### Claude Desktop 配置

在你的 `claude_desktop_config.json` 中添加以下配置：

```json
{
  "mcpServers": {
    "lancedb": {
      "command": "/absolute/path/to/lancedb-mcp-server",
      "args": ["--db-path", "/absolute/path/to/lancedb_data"]
    }
  }
}
```

## 统一响应结构

所有接口的返回结果都遵循以下统一 JSON 结构：

```json
{
  "code": 0,          // 0 表示成功，非 0 表示错误
  "message": "success", // 状态描述
  "data": { ... }     // 具体数据载荷
}
```

## 工具列表与参数结构

### 1. add_documents

向指定的 LanceDB 表中添加文档。

**输入参数 (Input):**

```json
{
  "table_name": "string", // (可选) 表名，默认 "knowledge_base"
  "documents": [          // 文档列表
    {
      "name": "string",       // (必填) 文档名称
      "description": "string",// (可选) 文档描述
      "chunks": ["..."],      // (必填) 该文档的所有切片文本
      "metadata": {           // (可选) 其他自定义元数据
        "author": "string" 
      }
    }
  ]
}
```

**输出结果 (Output):**

```json
{
  "code": 0,
  "message": "success",
  "data": "Successfully added N documents (M chunks) to table 'knowledge_base'"
}
```

### 2. search

基于语义向量搜索相似文档。

**输入参数 (Input):**

```json
{
  "table_name": "string", // (可选) 表名，默认 "knowledge_base"
  "query": "string",      // 搜索查询文本
  "limit": 5              // (可选) 返回结果数量，默认 5
}
```

**输出结果 (Output):**

```json
{
  "code": 0,
  "message": "success",
  "data": [
    {
      "id": "uuid",
      "name": "manual.pdf",
      "description": "User Manual",
      "content": "content...",
      "score": 0.87,
      "metadata": {"author": "admin"}
    }
  ]
}
```

### 3. list_tables

列出当前数据库中所有的表。

**输入参数 (Input):**

无 (空对象 `{}`)

**输出结果 (Output):**

```json
{
  "code": 0,
  "message": "success",
  "data": [
    "knowledge_base",
    "other_table"
  ]
}
```

## 架构说明

- **数据库**: LanceDB (本地文件向量数据库)
- **Embeddings**: `text-embeddings-inference` (基于 Candle 的 Rust 推理后端) 或 HTTP API
- **协议**: MCP (Model Context Protocol) via `rmcp`

## 开发指南

依赖项:
- Rust 1.75+
- `protoc` (用于编译 LanceDB 依赖)

### 安装 protoc（按平台）
 
macOS:
 
```bash
brew install protobuf
```
 
Linux（Debian/Ubuntu）:
 
```bash
sudo apt-get update
sudo apt-get install -y protobuf-compiler
```
 
Linux（CentOS/RHEL/Fedora）:
 
```bash
sudo dnf install -y protobuf-compiler
# 或
sudo yum install -y protobuf-compiler
```
 
Windows（使用 Chocolatey）:
 
```powershell
choco install protoc -y
```
 
无法通过包管理器安装时，可从官方发布页面下载二进制包：
 
https://github.com/protocolbuffers/protobuf/releases
 
### 配置并验证
 
如果 `protoc` 不在系统 PATH 中，请设置环境变量：
 
macOS/Linux:
 
```bash
export PROTOC=$(which protoc)
cargo check
```
 
Windows（PowerShell）:
 
```powershell
$env:PROTOC = (Get-Command protoc).Source
cargo check
```
