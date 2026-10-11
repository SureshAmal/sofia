use std::sync::Arc;
use rmcp::{
    model::*,
    service::RequestContext,
    ErrorData, RoleServer, ServerHandler,
};
use serde_json::{json, Value};

use crate::models::{EmotionContext, EmotionTone, ToolDocRef};
use crate::store::MemoryStore;
fn silvem_cmd() -> std::process::Command {
    if let Ok(exe) = std::env::current_exe() {
        let sibling = exe.with_file_name(if cfg!(windows) { "silvem.exe" } else { "silvem" });
        if sibling.is_file() {
            return std::process::Command::new(sibling);
        }
    }
    std::process::Command::new(if cfg!(windows) { "silvem.exe" } else { "silvem" })
}

#[derive(Clone)]
pub struct SofiaMemoryMcp {
    store: Arc<MemoryStore>,
}

impl SofiaMemoryMcp {
    pub fn new(store: MemoryStore) -> Self {
        Self {
            store: Arc::new(store),
        }
    }

    pub fn declarations() -> Vec<Tool> {
        vec![
            Tool::new(
                "memory_recall",
                "Recall relevant memories, user preferences, entities, and past experiences (including emotions and tool interactions) matching a query.",
                json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Search query or context to recall memory for" },
                        "limit": { "type": "integer", "description": "Max number of items to retrieve (default 5)" }
                    },
                    "required": ["query"]
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
            Tool::new(
                "memory_store_fact",
                "Store or update a world fact / entity in the knowledge graph (person, tool, document, project, concept).",
                json!({
                    "type": "object",
                    "properties": {
                        "name": { "type": "string", "description": "Entity name" },
                        "entity_type": { "type": "string", "description": "Type: person, project, tool, document, topic, etc." },
                        "summary": { "type": "string", "description": "Detailed description or fact about the entity" },
                        "attributes": { "type": "object", "description": "Optional arbitrary JSON attributes" }
                    },
                    "required": ["name", "entity_type", "summary"]
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
            Tool::new(
                "memory_record_experience",
                "Record an episodic interaction experience, including detected user emotion, tools used, documents modified, and outcome.",
                json!({
                    "type": "object",
                    "properties": {
                        "user_query": { "type": "string", "description": "What the user asked or prompted" },
                        "summary": { "type": "string", "description": "Summary of what happened and what was accomplished" },
                        "emotion_tone": {
                            "type": "string",
                            "enum": ["neutral", "satisfied", "frustrated", "curious", "urgent", "excited", "confused", "disappointed"],
                            "description": "User's detected emotion or sentiment"
                        },
                        "emotion_trigger": { "type": "string", "description": "What caused or triggered this reaction (e.g. tool error, feature delight)" },
                        "tools_and_docs": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "kind": { "type": "string", "description": "tool, document, chart, note, app" },
                                    "identifier": { "type": "string", "description": "Tool name or document id/title" },
                                    "action": { "type": "string", "description": "execute, create, modify, view, close" },
                                    "success": { "type": "boolean" },
                                    "outcome_summary": { "type": "string" }
                                },
                                "required": ["kind", "identifier", "action", "success"]
                            },
                            "description": "Tools and documents interacted with during this turn"
                        },
                        "entities_involved": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Names of entities or projects involved"
                        },
                        "importance": { "type": "number", "description": "Importance score 0.0 to 1.0 (default 0.5)" }
                    },
                    "required": ["user_query", "summary"]
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
            Tool::new(
                "memory_update_preference",
                "Record or adapt a user preference, habit, or instruction so the model never forgets it across sessions.",
                json!({
                    "type": "object",
                    "properties": {
                        "key": { "type": "string", "description": "Preference key (e.g. 'ui.style', 'code.comments', 'tone.conciseness')" },
                        "value": { "type": "string", "description": "Preference value or rule" },
                        "category": { "type": "string", "description": "Category: coding, ui, tone, system, workflow" },
                        "confidence": { "type": "number", "description": "Confidence from 0.1 to 1.0" },
                        "source_context": { "type": "string", "description": "Reason or context why this preference was noted" }
                    },
                    "required": ["key", "value", "category", "source_context"]
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
            Tool::new(
                "memory_list_preferences",
                "List all active learned user preferences.",
                json!({
                    "type": "object",
                    "properties": {}
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
            Tool::new(
                "memory_rag_search",
                "Perform hybrid (Dense Vector + BM25 Sparse) RAG search over synced local project files, code, and agent memory.",
                json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Search query or question to retrieve local project context for" },
                        "top_k": { "type": "integer", "description": "Max number of RAG search results to return (default 5)" },
                        "folder": { "type": "string", "description": "Optional folder path filter to limit search to a specific synced directory" }
                    },
                    "required": ["query"]
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        ]
    }

    pub fn execute(&self, name: &str, args: Value) -> Result<Value, String> {
        match name {
            "memory_recall" => {
                let query = args.get("query").and_then(Value::as_str).ok_or("query required")?;
                let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(5) as usize;
                let result = self.store.recall(query, limit).map_err(|e| e.to_string())?;
                serde_json::to_value(result).map_err(|e| e.to_string())
            }
            "memory_store_fact" => {
                let entity_name = args.get("name").and_then(Value::as_str).ok_or("name required")?;
                let entity_type = args.get("entity_type").and_then(Value::as_str).ok_or("entity_type required")?;
                let summary = args.get("summary").and_then(Value::as_str).ok_or("summary required")?;
                let attributes = args.get("attributes").cloned().unwrap_or(json!({}));
                let entity = self.store.upsert_entity(entity_name, entity_type, summary, attributes).map_err(|e| e.to_string())?;
                serde_json::to_value(entity).map_err(|e| e.to_string())
            }
            "memory_record_experience" => {
                let user_query = args.get("user_query").and_then(Value::as_str).ok_or("user_query required")?;
                let summary = args.get("summary").and_then(Value::as_str).ok_or("summary required")?;
                let tone_str = args.get("emotion_tone").and_then(Value::as_str).unwrap_or("neutral");
                let tone = match tone_str {
                    "satisfied" => EmotionTone::Satisfied,
                    "frustrated" => EmotionTone::Frustrated,
                    "curious" => EmotionTone::Curious,
                    "urgent" => EmotionTone::Urgent,
                    "excited" => EmotionTone::Excited,
                    "confused" => EmotionTone::Confused,
                    "disappointed" => EmotionTone::Disappointed,
                    _ => EmotionTone::Neutral,
                };
                let trigger = args.get("emotion_trigger").and_then(Value::as_str).map(String::from);
                let valence = match tone {
                    EmotionTone::Satisfied | EmotionTone::Excited => 0.7,
                    EmotionTone::Frustrated | EmotionTone::Disappointed => -0.7,
                    EmotionTone::Urgent => 0.0,
                    _ => 0.0,
                };
                let arousal = match tone {
                    EmotionTone::Urgent | EmotionTone::Excited | EmotionTone::Frustrated => 0.8,
                    _ => 0.3,
                };
                let emotion = EmotionContext {
                    tone,
                    valence,
                    arousal,
                    trigger,
                };
                let tools_and_docs: Vec<ToolDocRef> = args
                    .get("tools_and_docs")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default();
                let entities_involved: Vec<String> = args
                    .get("entities_involved")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default();
                let importance = args.get("importance").and_then(Value::as_f64).unwrap_or(0.5) as f32;

                let experience = self.store.record_experience(
                    None,
                    user_query,
                    summary,
                    emotion,
                    tools_and_docs,
                    entities_involved,
                    importance,
                ).map_err(|e| e.to_string())?;

                serde_json::to_value(experience).map_err(|e| e.to_string())
            }
            "memory_update_preference" => {
                let key = args.get("key").and_then(Value::as_str).ok_or("key required")?;
                let value = args.get("value").and_then(Value::as_str).ok_or("value required")?;
                let category = args.get("category").and_then(Value::as_str).ok_or("category required")?;
                let confidence = args.get("confidence").and_then(Value::as_f64).unwrap_or(1.0) as f32;
                let source_context = args.get("source_context").and_then(Value::as_str).ok_or("source_context required")?;

                let pref = self.store.set_user_preference(key, value, category, confidence, source_context).map_err(|e| e.to_string())?;
                serde_json::to_value(pref).map_err(|e| e.to_string())
            }
            "memory_list_preferences" => {
                let prefs = self.store.list_user_preferences().map_err(|e| e.to_string())?;
                serde_json::to_value(prefs).map_err(|e| e.to_string())
            }
            "memory_rag_search" => {
                let query = args.get("query").and_then(Value::as_str).ok_or("query required")?;
                let limit = args.get("limit").or_else(|| args.get("top_k")).and_then(Value::as_u64).unwrap_or(5);
                let folder = args.get("folder").and_then(Value::as_str);

                // 1. Sync target folder into silvem vector database if specified
                if let Some(f) = folder {
                    let _ = silvem_cmd()
                        .args(["sync", f])
                        .output();
                }

                // 2. Execute silvem binary for EmbeddingGemma-2 + sqlite-vec hybrid RAG search
                let silvem_res = silvem_cmd()
                    .args(["search", query, "--hybrid", "--json", "-k", &limit.to_string()])
                    .output();

                if let Ok(output) = silvem_res {
                    if output.status.success() {
                        if let Ok(mut json_val) = serde_json::from_slice::<Value>(&output.stdout) {
                            let mut rag_block = format!("# RAG Retrieval Context for Query: \"{}\"\n\n", query);
                            let mut formatted_matches = Vec::new();

                            if let Some(results) = json_val.get("results").and_then(Value::as_array) {
                                for (idx, item) in results.iter().enumerate() {
                                    let title = item.get("title").and_then(Value::as_str).unwrap_or("Untitled Document");
                                    let uri = item.get("uri").and_then(Value::as_str).unwrap_or("N/A");
                                    let content = item.get("content").and_then(Value::as_str).unwrap_or("");
                                    let score = item.get("score").and_then(Value::as_f64).unwrap_or(0.0);
                                    let modality = item.get("modality").and_then(Value::as_str).unwrap_or("text");

                                    rag_block.push_str(&format!(
                                        "### Match {}: {}\n- **Source Path**: {}\n- **Modality**: {}\n- **Relevance Score**: {:.1}%\n\n```\n{}\n```\n\n",
                                        idx + 1, title, uri, modality, score * 100.0, content
                                    ));

                                    formatted_matches.push(json!({
                                        "rank": idx + 1,
                                        "title": title,
                                        "uri": uri,
                                        "relevance_score": format!("{:.1}%", score * 100.0),
                                        "modality": modality,
                                        "content_snippet": content,
                                    }));
                                }
                            }

                            if let Some(obj) = json_val.as_object_mut() {
                                obj.insert("ok".into(), json!(true));
                                obj.insert("engine".into(), json!("silvem_embedding_gemma2_hybrid"));
                                obj.insert("rag_context_block".into(), json!(rag_block));
                                obj.insert("formatted_matches".into(), json!(formatted_matches));
                                if let Some(f) = folder {
                                    obj.insert("folder_filter".into(), json!(f));
                                }
                            }
                            return Ok(json_val);
                        }
                    }
                }

                let result = self.store.recall(query, limit as usize).map_err(|e| e.to_string())?;
                let response = json!({
                    "ok": true,
                    "engine": "fallback_store",
                    "query": query,
                    "folder_filter": folder,
                    "recalled_items": result
                });
                Ok(response)
            }
            _ => Err(format!("Unknown memory tool: {name}")),
        }
    }
}

impl ServerHandler for SofiaMemoryMcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
    }

    async fn list_tools(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult {
            tools: Self::declarations(),
            ..Default::default()
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let store = self.clone();
        let name = request.name.clone();
        let arguments = Value::Object(request.arguments.unwrap_or_default());

        let res = tokio::task::spawn_blocking(move || store.execute(&name, arguments)).await;

        match res {
            Ok(Ok(val)) => {
                let content = vec![ContentBlock::text(serde_json::to_string_pretty(&val).unwrap_or_default())];
                Ok(CallToolResult::success(content).into())
            }
            Ok(Err(err)) => {
                let content = vec![ContentBlock::text(err)];
                Ok(CallToolResult::error(content).into())
            }
            Err(_) => {
                let content = vec![ContentBlock::text("Internal memory execution worker error".to_string())];
                Ok(CallToolResult::error(content).into())
            }
        }
    }
}
