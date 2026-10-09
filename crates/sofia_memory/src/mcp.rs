use std::sync::Arc;
use rmcp::{
    model::*,
    service::RequestContext,
    ErrorData, RoleServer, ServerHandler,
};
use serde_json::{json, Value};

use crate::models::{EmotionContext, EmotionTone, ToolDocRef};
use crate::store::MemoryStore;

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
