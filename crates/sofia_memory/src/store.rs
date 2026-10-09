use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use rusqlite::{params, Connection};
use uuid::Uuid;

use crate::error::MemoryError;
use crate::models::*;

#[derive(Clone)]
pub struct MemoryStore {
    conn: Arc<Mutex<Connection>>,
}

impl MemoryStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, MemoryError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        let store = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        store.migrate()?;
        Ok(store)
    }

    pub fn open_in_memory() -> Result<Self, MemoryError> {
        let conn = Connection::open_in_memory()?;
        let store = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        store.migrate()?;
        Ok(store)
    }

    pub fn default_path() -> Result<PathBuf, MemoryError> {
        if let Some(path) = std::env::var_os("SOFIA_MEMORY_DB") {
            return Ok(PathBuf::from(path));
        }
        #[cfg(unix)]
        {
            if let Some(home) = std::env::var_os("HOME") {
                return Ok(PathBuf::from(home).join(".local/share/sofia/sofia_memory.db"));
            }
        }
        #[cfg(windows)]
        {
            if let Some(appdata) = std::env::var_os("APPDATA") {
                return Ok(PathBuf::from(appdata).join("sofia").join("sofia_memory.db"));
            }
        }
        Ok(std::env::temp_dir().join("sofia").join("sofia_memory.db"))
    }

    fn migrate(&self) -> Result<(), MemoryError> {
        let conn = self.conn.lock().unwrap();

        // Performance pragmas
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;",
        )?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS entities (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                entity_type TEXT NOT NULL,
                summary TEXT NOT NULL,
                attributes TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS relations (
                id TEXT PRIMARY KEY,
                source_id TEXT NOT NULL,
                relation TEXT NOT NULL,
                target_id TEXT NOT NULL,
                weight REAL NOT NULL DEFAULT 1.0,
                context TEXT,
                created_at INTEGER NOT NULL,
                FOREIGN KEY (source_id) REFERENCES entities(id) ON DELETE CASCADE,
                FOREIGN KEY (target_id) REFERENCES entities(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS experiences (
                id TEXT PRIMARY KEY,
                session_id TEXT,
                timestamp INTEGER NOT NULL,
                user_query TEXT NOT NULL,
                summary TEXT NOT NULL,
                emotion_json TEXT NOT NULL,
                tools_and_docs_json TEXT NOT NULL,
                entities_json TEXT NOT NULL,
                importance REAL NOT NULL DEFAULT 0.5
            );

            CREATE TABLE IF NOT EXISTS user_preferences (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                category TEXT NOT NULL,
                confidence REAL NOT NULL DEFAULT 1.0,
                source_context TEXT NOT NULL,
                updated_at INTEGER NOT NULL
            );

            -- Reverse search & Full-Text Search (FTS5) for Entities and Experiences
            CREATE VIRTUAL TABLE IF NOT EXISTS memory_fts USING fts5(
                target_id UNINDEXED,
                target_type UNINDEXED,
                title,
                content,
                tokenize = 'porter unicode61'
            );",
        )?;

        Ok(())
    }

    // -------------------------------------------------------------
    // Entities & Knowledge Graph
    // -------------------------------------------------------------

    pub fn upsert_entity(
        &self,
        name: &str,
        entity_type: &str,
        summary: &str,
        attributes: serde_json::Value,
    ) -> Result<Entity, MemoryError> {
        let conn = self.conn.lock().unwrap();
        let now = chrono_now();

        let mut stmt = conn.prepare("SELECT id, created_at FROM entities WHERE name = ?1 AND entity_type = ?2")?;
        let existing = stmt.query_row(params![name, entity_type], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        });

        let (id, created_at) = match existing {
            Ok((id, created_at)) => (id, created_at),
            Err(_) => (Uuid::new_v4().to_string(), now),
        };

        let attrs_str = serde_json::to_string(&attributes)?;

        conn.execute(
            "INSERT INTO entities (id, name, entity_type, summary, attributes, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET
                summary = excluded.summary,
                attributes = excluded.attributes,
                updated_at = excluded.updated_at",
            params![id, name, entity_type, summary, attrs_str, created_at, now],
        )?;

        // Update FTS index
        conn.execute("DELETE FROM memory_fts WHERE target_id = ?1", params![id])?;
        conn.execute(
            "INSERT INTO memory_fts (target_id, target_type, title, content) VALUES (?1, 'entity', ?2, ?3)",
            params![id, name, format!("{summary} {attrs_str}")],
        )?;

        Ok(Entity {
            id,
            name: name.to_string(),
            entity_type: entity_type.to_string(),
            summary: summary.to_string(),
            attributes,
            created_at,
            updated_at: now,
        })
    }

    pub fn add_relation(
        &self,
        source_id: &str,
        relation: &str,
        target_id: &str,
        weight: f32,
        context: Option<&str>,
    ) -> Result<Relation, MemoryError> {
        let conn = self.conn.lock().unwrap();
        let id = Uuid::new_v4().to_string();
        let now = chrono_now();

        conn.execute(
            "INSERT INTO relations (id, source_id, relation, target_id, weight, context, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id, source_id, relation, target_id, weight, context, now],
        )?;

        Ok(Relation {
            id,
            source_id: source_id.to_string(),
            relation: relation.to_string(),
            target_id: target_id.to_string(),
            weight,
            context: context.map(|s| s.to_string()),
            created_at: now,
        })
    }

    // -------------------------------------------------------------
    // Experiences & Interactions (Temporal, Emotion, Tools & Docs)
    // -------------------------------------------------------------

    pub fn record_experience(
        &self,
        session_id: Option<&str>,
        user_query: &str,
        summary: &str,
        emotion: EmotionContext,
        tools_and_docs: Vec<ToolDocRef>,
        entities_involved: Vec<String>,
        importance: f32,
    ) -> Result<Experience, MemoryError> {
        let conn = self.conn.lock().unwrap();
        let id = Uuid::new_v4().to_string();
        let now = chrono_now();

        let emotion_json = serde_json::to_string(&emotion)?;
        let tools_docs_json = serde_json::to_string(&tools_and_docs)?;
        let entities_json = serde_json::to_string(&entities_involved)?;

        conn.execute(
            "INSERT INTO experiences (id, session_id, timestamp, user_query, summary, emotion_json, tools_and_docs_json, entities_json, importance)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![id, session_id, now, user_query, summary, emotion_json, tools_docs_json, entities_json, importance],
        )?;

        // Index in FTS5
        let tools_text = tools_and_docs
            .iter()
            .map(|t| format!("{} {} {}", t.kind, t.identifier, t.action))
            .collect::<Vec<_>>()
            .join(" ");
        let content_text = format!("{summary} {tools_text}");

        conn.execute(
            "INSERT INTO memory_fts (target_id, target_type, title, content) VALUES (?1, 'experience', ?2, ?3)",
            params![id, user_query, content_text],
        )?;

        Ok(Experience {
            id,
            session_id: session_id.map(String::from),
            timestamp: now,
            user_query: user_query.to_string(),
            summary: summary.to_string(),
            emotion,
            tools_and_docs,
            entities_involved,
            importance,
        })
    }

    // -------------------------------------------------------------
    // User Preferences (Continuous Adaptation)
    // -------------------------------------------------------------

    pub fn set_user_preference(
        &self,
        key: &str,
        value: &str,
        category: &str,
        confidence: f32,
        source_context: &str,
    ) -> Result<UserPreference, MemoryError> {
        let conn = self.conn.lock().unwrap();
        let now = chrono_now();

        conn.execute(
            "INSERT INTO user_preferences (key, value, category, confidence, source_context, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(key) DO UPDATE SET
                value = excluded.value,
                confidence = excluded.confidence,
                source_context = excluded.source_context,
                updated_at = excluded.updated_at",
            params![key, value, category, confidence, source_context, now],
        )?;

        // Also add preference to FTS for contextual search
        conn.execute("DELETE FROM memory_fts WHERE target_id = ?1", params![key])?;
        conn.execute(
            "INSERT INTO memory_fts (target_id, target_type, title, content) VALUES (?1, 'preference', ?2, ?3)",
            params![key, format!("{category}: {key}"), format!("{value} ({source_context})")],
        )?;

        Ok(UserPreference {
            key: key.to_string(),
            value: value.to_string(),
            category: category.to_string(),
            confidence,
            source_context: source_context.to_string(),
            updated_at: now,
        })
    }

    pub fn list_user_preferences(&self) -> Result<Vec<UserPreference>, MemoryError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT key, value, category, confidence, source_context, updated_at FROM user_preferences ORDER BY updated_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(UserPreference {
                key: row.get(0)?,
                value: row.get(1)?,
                category: row.get(2)?,
                confidence: row.get(3)?,
                source_context: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })?;

        let mut list = Vec::new();
        for r in rows {
            list.push(r?);
        }
        Ok(list)
    }

    // -------------------------------------------------------------
    // Hybrid Recall (Reverse Document Search + Preferences + Entities)
    // -------------------------------------------------------------

    pub fn recall(&self, query: &str, limit: usize) -> Result<RecallResult, MemoryError> {
        let conn = self.conn.lock().unwrap();

        // 1. Get all high-confidence preferences relevant to the context
        let mut pref_stmt = conn.prepare(
            "SELECT key, value, category, confidence, source_context, updated_at FROM user_preferences ORDER BY confidence DESC LIMIT 15",
        )?;
        let pref_rows = pref_stmt.query_map([], |row| {
            Ok(UserPreference {
                key: row.get(0)?,
                value: row.get(1)?,
                category: row.get(2)?,
                confidence: row.get(3)?,
                source_context: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })?;
        let mut preferences = Vec::new();
        for p in pref_rows {
            preferences.push(p?);
        }

        // 2. Perform FTS5 search on query tokens
        let sanitized = sanitize_fts_query(query);
        let mut target_ids: Vec<(String, String)> = Vec::new(); // (target_id, target_type)

        if !sanitized.is_empty() {
            let mut fts_stmt = conn.prepare(
                "SELECT target_id, target_type FROM memory_fts WHERE memory_fts MATCH ?1 ORDER BY rank LIMIT ?2",
            )?;
            let fts_rows = fts_stmt.query_map(params![sanitized, limit as i64], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            });
            if let Ok(rows) = fts_rows {
                for r in rows.flatten() {
                    target_ids.push(r);
                }
            }
        }

        // 3. Hydrate Entities
        let mut entities = Vec::new();
        for (id, kind) in &target_ids {
            if kind == "entity" {
                if let Ok(mut stmt) = conn.prepare("SELECT id, name, entity_type, summary, attributes, created_at, updated_at FROM entities WHERE id = ?1") {
                    if let Ok(entity) = stmt.query_row(params![id], |row| {
                        let attrs: String = row.get(4)?;
                        Ok(Entity {
                            id: row.get(0)?,
                            name: row.get(1)?,
                            entity_type: row.get(2)?,
                            summary: row.get(3)?,
                            attributes: serde_json::from_str(&attrs).unwrap_or_default(),
                            created_at: row.get(5)?,
                            updated_at: row.get(6)?,
                        })
                    }) {
                        entities.push(entity);
                    }
                }
            }
        }

        // 4. Hydrate Experiences (along with tool & emotion states)
        let mut related_experiences = Vec::new();
        for (id, kind) in &target_ids {
            if kind == "experience" {
                if let Ok(mut stmt) = conn.prepare("SELECT id, session_id, timestamp, user_query, summary, emotion_json, tools_and_docs_json, entities_json, importance FROM experiences WHERE id = ?1") {
                    if let Ok(exp) = stmt.query_row(params![id], |row| {
                        let emotion_str: String = row.get(5)?;
                        let tools_str: String = row.get(6)?;
                        let entities_str: String = row.get(7)?;
                        Ok(Experience {
                            id: row.get(0)?,
                            session_id: row.get(1)?,
                            timestamp: row.get(2)?,
                            user_query: row.get(3)?,
                            summary: row.get(4)?,
                            emotion: serde_json::from_str(&emotion_str).unwrap_or_default(),
                            tools_and_docs: serde_json::from_str(&tools_str).unwrap_or_default(),
                            entities_involved: serde_json::from_str(&entities_str).unwrap_or_default(),
                            importance: row.get(8)?,
                        })
                    }) {
                        related_experiences.push(exp);
                    }
                }
            }
        }

        // Also fetch the most recent experiences if we have few matches
        if related_experiences.len() < 3 {
            let mut recent_stmt = conn.prepare(
                "SELECT id, session_id, timestamp, user_query, summary, emotion_json, tools_and_docs_json, entities_json, importance FROM experiences ORDER BY timestamp DESC LIMIT 3",
            )?;
            let rows = recent_stmt.query_map([], |row| {
                let emotion_str: String = row.get(5)?;
                let tools_str: String = row.get(6)?;
                let entities_str: String = row.get(7)?;
                Ok(Experience {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    timestamp: row.get(2)?,
                    user_query: row.get(3)?,
                    summary: row.get(4)?,
                    emotion: serde_json::from_str(&emotion_str).unwrap_or_default(),
                    tools_and_docs: serde_json::from_str(&tools_str).unwrap_or_default(),
                    entities_involved: serde_json::from_str(&entities_str).unwrap_or_default(),
                    importance: row.get(8)?,
                })
            })?;
            for r in rows.flatten() {
                if !related_experiences.iter().any(|e| e.id == r.id) {
                    related_experiences.push(r);
                }
            }
        }

        Ok(RecallResult {
            query: query.to_string(),
            preferences,
            entities,
            related_experiences,
        })
    }
}

fn chrono_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn sanitize_fts_query(input: &str) -> String {
    input
        .chars()
        .map(|c| if c.is_alphanumeric() || c == ' ' { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .map(|word| format!("\"{word}\"*"))
        .collect::<Vec<_>>()
        .join(" OR ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_lifecycle_and_recall() {
        let store = MemoryStore::open_in_memory().unwrap();

        // 1. Record User Preference
        let pref = store.set_user_preference(
            "ui.style",
            "minimal direct without heavy backgrounds",
            "ui",
            0.95,
            "User requested cleaner UI with minimal padding",
        ).unwrap();
        assert_eq!(pref.key, "ui.style");

        // 2. Record Entity
        let ent = store.upsert_entity(
            "Sofia Desktop",
            "project",
            "Voice assistant written in Rust with GPUI and Gemini Live",
            serde_json::json!({"version": "0.1.0", "os": ["linux", "windows"]}),
        ).unwrap();
        assert_eq!(ent.name, "Sofia Desktop");

        // 3. Record Experience with Emotion and Tool Interaction
        let exp = store.record_experience(
            Some("session-123"),
            "fix the repeated chart starting animation",
            "User noted charts animate every time the window is resized or moved; fixed by setting appear(false) and stable ids.",
            EmotionContext {
                tone: EmotionTone::Frustrated,
                valence: -0.4,
                arousal: 0.6,
                trigger: Some("repeated chart re-animation on resize".into()),
            },
            vec![ToolDocRef {
                kind: "chart".into(),
                identifier: "chart-001".into(),
                action: "modify".into(),
                success: true,
                outcome_summary: Some("disabled appear key on resize".into()),
            }],
            vec!["Sofia Desktop".into()],
            0.85,
        ).unwrap();
        assert_eq!(exp.emotion.tone, EmotionTone::Frustrated);

        // 4. Recall
        let recall = store.recall("chart animation", 5).unwrap();
        assert!(!recall.preferences.is_empty());
        assert!(!recall.related_experiences.is_empty());
        assert_eq!(recall.related_experiences[0].emotion.tone, EmotionTone::Frustrated);
    }
}
