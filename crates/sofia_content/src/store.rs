use crate::{Content, Document, DocumentSummary};
use rusqlite::{Connection, OptionalExtension, params};
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub struct Store {
    connection: Mutex<Connection>,
}
fn error(error: impl std::fmt::Display) -> String {
    error.to_string()
}
fn timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
impl Store {
    pub fn default_path() -> Result<PathBuf, String> {
        if let Some(path) = std::env::var_os("SOFIA_CONTENT_DB") {
            return Ok(path.into());
        }
        let base = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("APPDATA").map(PathBuf::from))
            .or_else(|| {
                std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share"))
            })
            .ok_or("No user data directory")?;
        Ok(base.join("sofia/sofia_mcp.db"))
    }
    pub fn open(path: impl AsRef<Path>) -> Result<Self, String> {
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent).map_err(error)?;
        }
        let connection = Connection::open(path.as_ref()).map_err(error)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path.as_ref(), std::fs::Permissions::from_mode(0o600))
                .map_err(error)?;
        }
        connection
            .busy_timeout(Duration::from_secs(3))
            .map_err(error)?;
        connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;
            CREATE TABLE IF NOT EXISTS documents(id TEXT PRIMARY KEY,title TEXT NOT NULL,kind TEXT NOT NULL,tags TEXT NOT NULL,content TEXT NOT NULL,revision INTEGER NOT NULL,updated_at INTEGER NOT NULL,is_open INTEGER NOT NULL,width_rem REAL NOT NULL,height_rem REAL NOT NULL);
            CREATE INDEX IF NOT EXISTS documents_kind_updated ON documents(kind,updated_at);
            CREATE INDEX IF NOT EXISTS documents_open ON documents(is_open);
            CREATE VIRTUAL TABLE IF NOT EXISTS documents_fts USING fts5(id UNINDEXED,title,tags,body);
            PRAGMA user_version=1;").map_err(error)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }
    pub fn create(
        &self,
        title: String,
        mut tags: Vec<String>,
        content: Content,
        width_rem: f32,
        height_rem: f32,
    ) -> Result<Document, String> {
        if !tags.iter().any(|tag| tag == content.tag()) {
            tags.push(content.tag().into());
        }
        let doc = Document {
            id: uuid::Uuid::new_v4().to_string(),
            title,
            tags,
            content,
            revision: 1,
            updated_at: timestamp(),
            open: false,
            width_rem,
            height_rem,
        };
        doc.validate()?;
        let mut conn = self.connection.lock().map_err(error)?;
        let tx = conn.transaction().map_err(error)?;
        tx.execute(
            "INSERT INTO documents VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                doc.id,
                doc.title,
                doc.content.kind(),
                serde_json::to_string(&doc.tags).map_err(error)?,
                serde_json::to_string(&doc.content).map_err(error)?,
                doc.revision,
                doc.updated_at,
                doc.open,
                doc.width_rem,
                doc.height_rem
            ],
        )
        .map_err(error)?;
        index(&tx, &doc)?;
        tx.commit().map_err(error)?;
        Ok(doc)
    }
    pub fn get(&self, id: &str) -> Result<Document, String> {
        let conn = self.connection.lock().map_err(error)?;
        conn.query_row("SELECT id,title,tags,content,revision,updated_at,is_open,width_rem,height_rem FROM documents WHERE id=?1",[id],row).optional().map_err(error)?.ok_or("Document not found".into())
    }
    pub fn update(&self, mut doc: Document, expected_revision: i64) -> Result<Document, String> {
        doc.validate()?;
        doc.revision = expected_revision
            .checked_add(1)
            .filter(|revision| *revision > 1)
            .ok_or("Invalid revision")?;
        doc.updated_at = timestamp();
        let mut conn = self.connection.lock().map_err(error)?;
        let tx = conn.transaction().map_err(error)?;
        let count=tx.execute("UPDATE documents SET title=?1,kind=?2,tags=?3,content=?4,revision=?5,updated_at=?6,width_rem=?7,height_rem=?8 WHERE id=?9 AND revision=?10",params![doc.title,doc.content.kind(),serde_json::to_string(&doc.tags).map_err(error)?,serde_json::to_string(&doc.content).map_err(error)?,doc.revision,doc.updated_at,doc.width_rem,doc.height_rem,doc.id,expected_revision]).map_err(error)?;
        if count != 1 {
            return Err("Revision conflict: fetch the latest document before editing".into());
        }
        tx.execute("DELETE FROM documents_fts WHERE id=?1", [&doc.id])
            .map_err(error)?;
        index(&tx, &doc)?;
        doc.open = tx
            .query_row(
                "SELECT is_open FROM documents WHERE id=?1",
                [&doc.id],
                |r| r.get(0),
            )
            .map_err(error)?;
        tx.commit().map_err(error)?;
        Ok(doc)
    }
    pub fn set_open(&self, id: &str, open: bool) -> Result<Document, String> {
        let mut conn = self.connection.lock().map_err(error)?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(error)?;
        if open {
            let count: i64 = tx
                .query_row(
                    "SELECT COUNT(*) FROM documents WHERE is_open=1 AND id!=?1",
                    [id],
                    |r| r.get(0),
                )
                .map_err(error)?;
            if count >= 8 {
                return Err("Close a window before opening more than eight".into());
            }
        }
        if tx
            .execute(
                "UPDATE documents SET is_open=?1 WHERE id=?2",
                params![open, id],
            )
            .map_err(error)?
            != 1
        {
            return Err("Document not found".into());
        }
        tx.commit().map_err(error)?;
        drop(conn);
        self.get(id)
    }
    pub fn open_documents(&self) -> Result<Vec<Document>, String> {
        let conn = self.connection.lock().map_err(error)?;
        let mut stmt=conn.prepare("SELECT id,title,tags,content,revision,updated_at,is_open,width_rem,height_rem FROM documents WHERE is_open=1 ORDER BY updated_at,id").map_err(error)?;
        stmt.query_map([], row)
            .map_err(error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(error)
    }
    pub fn closed_documents(&self) -> Result<Vec<Document>, String> {
        let conn = self.connection.lock().map_err(error)?;
        let mut stmt = conn
            .prepare("SELECT id,title,tags,content,revision,updated_at,is_open,width_rem,height_rem FROM documents WHERE is_open=0 ORDER BY updated_at DESC,id")
            .map_err(error)?;
        stmt.query_map([], row)
            .map_err(error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(error)
    }
    pub fn closed_count(&self) -> Result<usize, String> {
        let conn = self.connection.lock().map_err(error)?;
        conn.query_row("SELECT COUNT(*) FROM documents WHERE is_open=0", [], |r| {
            r.get::<_, i64>(0).map(|c| c.max(0) as usize)
        })
        .map_err(error)
    }
    pub fn list(
        &self,
        query: Option<&str>,
        kind: Option<&str>,
        tag: Option<&str>,
    ) -> Result<Vec<DocumentSummary>, String> {
        let conn = self.connection.lock().map_err(error)?;
        let query = query.filter(|q| !q.trim().is_empty()).map(|q| {
            q.split_whitespace()
                .map(|word| format!("\"{}\"", word.replace('"', "\"\"")))
                .collect::<Vec<_>>()
                .join(" AND ")
        });
        let mut stmt=conn.prepare("SELECT id,title,tags,content,revision,updated_at,is_open,width_rem,height_rem FROM documents WHERE (?1 IS NULL OR id IN(SELECT id FROM documents_fts WHERE documents_fts MATCH ?1)) AND (?2 IS NULL OR kind=?2) AND (?3 IS NULL OR EXISTS(SELECT 1 FROM json_each(documents.tags) WHERE value=?3)) ORDER BY updated_at DESC,id LIMIT 100").map_err(error)?;
        stmt.query_map(params![query, kind, tag], row)
            .map_err(error)?
            .map(|row| row.map(DocumentSummary::from))
            .collect::<Result<Vec<_>, _>>()
            .map_err(error)
    }
    pub fn resolve(&self, id: Option<&str>, title: Option<&str>) -> Result<Document, String> {
        if let Some(id) = id {
            return self.get(id);
        }
        let title = title.ok_or("Specify id or title")?;
        let conn = self.connection.lock().map_err(error)?;
        let mut stmt = conn
            .prepare("SELECT id FROM documents WHERE title=?1 LIMIT 2")
            .map_err(error)?;
        let ids = stmt
            .query_map([title], |row| row.get::<_, String>(0))
            .map_err(error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(error)?;
        if ids.len() != 1 {
            return Err("Title was not found or is ambiguous; use a document ID".into());
        }
        drop(stmt);
        drop(conn);
        self.get(&ids[0])
    }
}
fn row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Document> {
    let tags: String = row.get(2)?;
    let content: String = row.get(3)?;
    let decode = |err: serde_json::Error| {
        rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(err))
    };
    Ok(Document {
        id: row.get(0)?,
        title: row.get(1)?,
        tags: serde_json::from_str(&tags).map_err(decode)?,
        content: serde_json::from_str(&content).map_err(decode)?,
        revision: row.get(4)?,
        updated_at: row.get(5)?,
        open: row.get(6)?,
        width_rem: row.get(7)?,
        height_rem: row.get(8)?,
    })
}
fn index(tx: &rusqlite::Transaction<'_>, doc: &Document) -> Result<(), String> {
    tx.execute(
        "INSERT INTO documents_fts(id,title,tags,body) VALUES(?1,?2,?3,?4)",
        params![
            doc.id,
            doc.title,
            doc.tags.join(" "),
            serde_json::to_string(&doc.content).map_err(error)?
        ],
    )
    .map_err(error)?;
    Ok(())
}
