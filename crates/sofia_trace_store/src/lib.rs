//! Persistent run and tool timeline records for the desktop trace viewer.
use rusqlite::{Connection, params};
use sofia_protocol::{ServerEvent, TurnState};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub id: String,
    pub started_ms: i64,
    pub duration_ms: Option<i64>,
    pub status: String,
    pub input: String,
    pub output: String,
    pub error: String,
    pub tool_count: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    pub started_ms: i64,
    pub kind: String,
    pub name: String,
    pub detail: String,
    pub duration_ms: Option<i64>,
}

pub struct Store(Connection);

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn bounded(value: &str) -> String {
    value.chars().take(4096).collect()
}

impl Store {
    pub fn default_path() -> Result<PathBuf, String> {
        if let Some(path) = std::env::var_os("SOFIA_TRACE_DB") {
            return Ok(path.into());
        }
        let base = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("APPDATA").map(PathBuf::from))
            .or_else(|| {
                std::env::var_os("HOME").map(|path| PathBuf::from(path).join(".local/share"))
            })
            .ok_or("No user data directory")?;
        Ok(base.join("sofia/traces.db"))
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self, String> {
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let db = Connection::open(path.as_ref()).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path.as_ref(), std::fs::Permissions::from_mode(0o600))
                .map_err(|e| e.to_string())?;
        }
        db.busy_timeout(Duration::from_secs(2))
            .map_err(|e| e.to_string())?;
        db.execute_batch("PRAGMA journal_mode=WAL;
            CREATE TABLE IF NOT EXISTS runs(id TEXT PRIMARY KEY, session_id TEXT, started_ms INTEGER NOT NULL, ended_ms INTEGER, status TEXT NOT NULL, input TEXT NOT NULL DEFAULT '', output TEXT NOT NULL DEFAULT '', error TEXT NOT NULL DEFAULT '', tool_count INTEGER NOT NULL DEFAULT 0);
            CREATE INDEX IF NOT EXISTS runs_started ON runs(started_ms DESC);
            CREATE TABLE IF NOT EXISTS steps(id INTEGER PRIMARY KEY, run_id TEXT NOT NULL REFERENCES runs(id), started_ms INTEGER NOT NULL, kind TEXT NOT NULL, name TEXT NOT NULL DEFAULT '', detail TEXT NOT NULL DEFAULT '', duration_ms INTEGER);
            CREATE INDEX IF NOT EXISTS steps_run ON steps(run_id,id);").map_err(|e| e.to_string())?;
        Ok(Self(db))
    }

    pub fn data_version(&self) -> Result<i64, String> {
        self.0
            .query_row("PRAGMA data_version", [], |row| row.get(0))
            .map_err(|e| e.to_string())
    }

    pub fn runs(&self) -> Result<Vec<Run>, String> {
        let mut stmt = self.0.prepare("SELECT id,started_ms,ended_ms-started_ms,status,input,output,error,tool_count FROM runs ORDER BY started_ms DESC LIMIT 100").map_err(|e| e.to_string())?;
        stmt.query_map([], |row| {
            Ok(Run {
                id: row.get(0)?,
                started_ms: row.get(1)?,
                duration_ms: row.get(2)?,
                status: row.get(3)?,
                input: row.get(4)?,
                output: row.get(5)?,
                error: row.get(6)?,
                tool_count: row.get(7)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
    }

    pub fn steps(&self, run_id: &str) -> Result<Vec<Step>, String> {
        let mut stmt = self.0.prepare("SELECT started_ms,kind,name,detail,duration_ms FROM steps WHERE run_id=?1 ORDER BY id LIMIT 500").map_err(|e| e.to_string())?;
        stmt.query_map([run_id], |row| {
            Ok(Step {
                started_ms: row.get(0)?,
                kind: row.get(1)?,
                name: row.get(2)?,
                detail: row.get(3)?,
                duration_ms: row.get(4)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
    }
}

pub struct Recorder {
    store: Store,
    active: Option<String>,
    tools: HashMap<String, (i64, i64)>,
}

impl Recorder {
    pub fn new(store: Store) -> Self {
        let _ = store.0.execute(
            "UPDATE runs SET status='abandoned',ended_ms=?1 WHERE status='running'",
            [now_ms()],
        );
        Self {
            store,
            active: None,
            tools: HashMap::new(),
        }
    }

    fn begin(&mut self, session_id: Option<Uuid>) -> Result<String, String> {
        if let Some(id) = &self.active {
            return Ok(id.clone());
        }
        let id = Uuid::new_v4().to_string();
        self.store
            .0
            .execute(
                "INSERT INTO runs(id,session_id,started_ms,status) VALUES(?1,?2,?3,'running')",
                params![id, session_id.map(|v| v.to_string()), now_ms()],
            )
            .map_err(|e| e.to_string())?;
        self.active = Some(id.clone());
        Ok(id)
    }

    fn step(&self, id: &str, kind: &str, name: &str, detail: &str) -> Result<i64, String> {
        self.store
            .0
            .execute(
                "INSERT INTO steps(run_id,started_ms,kind,name,detail) VALUES(?1,?2,?3,?4,?5)",
                params![id, now_ms(), kind, name, bounded(detail)],
            )
            .map_err(|e| e.to_string())?;
        Ok(self.store.0.last_insert_rowid())
    }

    pub fn record(&mut self, event: &ServerEvent, session_id: Option<Uuid>) -> Result<(), String> {
        match event {
            ServerEvent::InputText { text } => {
                let id = self.begin(session_id)?;
                self.store
                    .0
                    .execute(
                        "UPDATE runs SET input=input||?1 WHERE id=?2",
                        params![bounded(text), id],
                    )
                    .map_err(|e| e.to_string())?;
                self.step(&id, "input", "User", text)?;
            }
            ServerEvent::ToolCallRequested { call_id, name } => {
                let id = self.begin(session_id)?;
                let started = now_ms();
                let row = self.step(&id, "tool_running", name, "")?;
                self.tools.insert(call_id.clone(), (row, started));
                self.store
                    .0
                    .execute("UPDATE runs SET tool_count=tool_count+1 WHERE id=?1", [&id])
                    .map_err(|e| e.to_string())?;
            }
            ServerEvent::ToolCallFinished {
                call_id, success, ..
            } => {
                if let Some((row, started)) = self.tools.remove(call_id) {
                    self.store
                        .0
                        .execute(
                            "UPDATE steps SET kind=?1,duration_ms=?2 WHERE id=?3",
                            params![
                                if *success { "tool_ok" } else { "tool_error" },
                                now_ms() - started,
                                row
                            ],
                        )
                        .map_err(|e| e.to_string())?;
                    if !success && let Some(id) = &self.active {
                        self.store
                            .0
                            .execute("UPDATE runs SET status='error' WHERE id=?1", [id])
                            .map_err(|e| e.to_string())?;
                    }
                }
            }
            ServerEvent::AssistantTextFinal {
                text, interrupted, ..
            } => {
                let id = self.begin(session_id)?;
                self.store
                    .0
                    .execute(
                        "UPDATE runs SET output=?1 WHERE id=?2",
                        params![bounded(text), id],
                    )
                    .map_err(|e| e.to_string())?;
                if *interrupted {
                    self.store
                        .0
                        .execute("UPDATE runs SET status='interrupted' WHERE id=?1", [&id])
                        .map_err(|e| e.to_string())?;
                }
                self.step(
                    &id,
                    if *interrupted {
                        "interrupted"
                    } else {
                        "output"
                    },
                    "Sofia",
                    text,
                )?;
            }
            ServerEvent::Error { message } => {
                let had_active = self.active.is_some();
                let id = self.begin(session_id)?;
                self.store
                    .0
                    .execute(
                        "UPDATE runs SET status='error',error=?1 WHERE id=?2",
                        params![bounded(message), id],
                    )
                    .map_err(|e| e.to_string())?;
                self.step(&id, "error", "Error", message)?;
                if !had_active {
                    self.finish()?;
                }
            }
            ServerEvent::TurnStateChanged { state } => {
                if matches!(
                    state,
                    TurnState::Ready
                        | TurnState::Listening
                        | TurnState::Disconnected
                        | TurnState::Reconnecting
                        | TurnState::Error
                ) {
                    self.finish()?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    pub fn finish(&mut self) -> Result<(), String> {
        if let Some(id) = self.active.take() {
            self.store.0.execute("UPDATE runs SET ended_ms=?1,status=CASE WHEN status='running' THEN 'ok' ELSE status END WHERE id=?2", params![now_ms(), id]).map_err(|e| e.to_string())?;
            for (_, (row, started)) in self.tools.drain() {
                self.store.0.execute("UPDATE steps SET kind='tool_cancelled',duration_ms=?1 WHERE id=?2 AND kind='tool_running'", params![now_ms()-started,row]).map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }
}
