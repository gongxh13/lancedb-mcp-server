use anyhow::Result;
use arrow::array::{FixedSizeListBuilder, Float32Builder, RecordBatch, RecordBatchIterator, StringArray, StringBuilder};
use arrow::datatypes::{DataType, Field, Schema};
use futures::TryStreamExt;
use lancedb::connection::Connection;
use lancedb::query::{ExecutableQuery, QueryBase};
use lancedb::{connect, Table, DistanceType};
use std::sync::Arc;
use crate::embeddings::EmbeddingModel;

pub struct VectorDB {
    connection: Connection,
}

impl VectorDB {
    pub async fn new(path: &str) -> Result<Self> {
        let connection = connect(path).execute().await?;
        Ok(Self { connection })
    }

    pub async fn create_table(&self, name: &str, dim: usize) -> Result<Table> {
        // Define schema: id, text, vector, metadata (json string)
        let schema = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Utf8, false),
            Field::new("text", DataType::Utf8, false),
            Field::new("vector", DataType::FixedSizeList(
                Arc::new(Field::new("item", DataType::Float32, true)),
                dim as i32
            ), false),
            Field::new("metadata", DataType::Utf8, true),
        ]));

        // Create empty table if not exists
        // LanceDB requires data to create table usually, or create_empty_table
        // create_empty_table is available in newer versions.
        
        // If table exists, open it.
        if self.connection.table_names().execute().await?.contains(&name.to_string()) {
            return Ok(self.connection.open_table(name).execute().await?);
        }

        self.connection.create_empty_table(name, schema).execute().await.map_err(|e| anyhow::anyhow!(e))
    }

    pub async fn list_tables(&self) -> Result<Vec<String>> {
        Ok(self.connection.table_names().execute().await?)
    }

    pub async fn add_texts(
        &self,
        table_name: &str,
        texts: Vec<String>,
        metadatas: Vec<serde_json::Value>,
        model: &EmbeddingModel,
    ) -> Result<()> {
        if texts.is_empty() {
            return Ok(());
        }

        // 1. Compute embeddings
        let embeddings = model.embed(texts.clone()).await?;
        if embeddings.is_empty() {
            return Ok(());
        }
        let dim = embeddings[0].len();

        // 2. Ensure table exists
        let table = self.create_table(table_name, dim).await?;

        // 3. Create RecordBatch
        let len = texts.len();
        
        // ID Builder
        let mut id_builder = StringBuilder::new();
        // Text Builder
        let mut text_builder = StringBuilder::new();
        // Metadata Builder
        let mut meta_builder = StringBuilder::new();
        // Vector Builder
        let values_builder = Float32Builder::new();
        let mut vector_builder = FixedSizeListBuilder::new(values_builder, dim as i32);

        for i in 0..len {
            id_builder.append_value(uuid::Uuid::new_v4().to_string());
            text_builder.append_value(&texts[i]);
            meta_builder.append_value(metadatas.get(i).map(|v| v.to_string()).unwrap_or("{}".to_string()));
            
            // Vector
            let vec_ref = &embeddings[i];
            vector_builder.values().append_slice(vec_ref);
            vector_builder.append(true);
        }

        let schema = table.schema().await?;
        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![
                Arc::new(id_builder.finish()),
                Arc::new(text_builder.finish()),
                Arc::new(vector_builder.finish()),
                Arc::new(meta_builder.finish()),
            ],
        )?;

        // 4. Add to table
        // We need an iterator of RecordBatches
        let stream = RecordBatchIterator::new(vec![Ok(batch)], schema.clone());
        table.add(stream).execute().await?;

        Ok(())
    }

    pub async fn search(
        &self,
        table_name: &str,
        query: &str,
        limit: usize,
        model: &EmbeddingModel,
    ) -> Result<Vec<serde_json::Value>> {
        let table = self.connection.open_table(table_name).execute().await?;
        
        // Embed query
        let query_vecs = model.embed(vec![query.to_string()]).await?;
        let query_vec = &query_vecs[0];

        // Search
        let results = table
            .vector_search(query_vec.clone())?
            .distance_type(DistanceType::Cosine)
            .limit(limit)
            .execute()
            .await?;

        // Parse results
        let mut output = Vec::new();
        let record_batches: Vec<RecordBatch> = results.try_collect().await?;
        
        for batch in record_batches {
            let id_col = batch.column_by_name("id").unwrap().as_any().downcast_ref::<StringArray>().unwrap();
            let text_col = batch.column_by_name("text").unwrap().as_any().downcast_ref::<StringArray>().unwrap();
            let meta_col = batch.column_by_name("metadata").unwrap().as_any().downcast_ref::<StringArray>().unwrap();
            // _distance column is added by vector search
            let dist_col = batch.column_by_name("_distance").unwrap().as_any().downcast_ref::<arrow::array::Float32Array>().unwrap();
            
            for i in 0..batch.num_rows() {
                let id = id_col.value(i);
                let text = text_col.value(i);
                let meta_str = meta_col.value(i);
                let mut meta: serde_json::Value = serde_json::from_str(meta_str).unwrap_or(serde_json::json!({}));
                let distance = dist_col.value(i);
                let score = 1.0 - distance; // Convert distance to score (assuming cosine distance)

                // Extract name and description
                let mut name = String::new();
                let mut description = None;
                
                if let serde_json::Value::Object(ref mut map) = meta {
                    if let Some(n) = map.remove("name") {
                        if let Some(s) = n.as_str() {
                            name = s.to_string();
                        }
                    }
                    if let Some(d) = map.remove("description") {
                        if let Some(s) = d.as_str() {
                            description = Some(s.to_string());
                        }
                    }
                }
                
                let mut result = serde_json::json!({
                    "id": id,
                    "name": name,
                    "content": text,
                    "score": score,
                    "metadata": meta
                });

                if let Some(desc) = description {
                    result["description"] = serde_json::Value::String(desc);
                }
                
                output.push(result);
            }
        }

        Ok(output)
    }
}
