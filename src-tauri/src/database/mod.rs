use std::{collections::HashMap, path::Path};

use rusqlite::{params, Connection, OptionalExtension, Transaction};

use crate::platform::Operation;
use crate::{
    core::model::{AppSettings, ItemKind, LibraryItem, ProviderKind},
    error::Result,
};

const MIGRATIONS: &[&str] = &[
    "CREATE TABLE library_items(
        id TEXT PRIMARY KEY, name TEXT NOT NULL, kind TEXT NOT NULL,
        provider TEXT NOT NULL, executable TEXT, args TEXT NOT NULL,
        working_directory TEXT, environment TEXT NOT NULL, icon TEXT,
        cover TEXT, background TEXT, category TEXT, tags TEXT NOT NULL,
        favorite INTEGER NOT NULL DEFAULT 0, hidden INTEGER NOT NULL DEFAULT 0,
        installed INTEGER NOT NULL DEFAULT 1, play_count INTEGER NOT NULL DEFAULT 0,
        total_play_time INTEGER NOT NULL DEFAULT 0, last_played_at TEXT,
        created_at TEXT NOT NULL, updated_at TEXT NOT NULL
    );
    CREATE TABLE settings(key TEXT PRIMARY KEY,value TEXT NOT NULL);
    CREATE TABLE play_sessions(
        id INTEGER PRIMARY KEY, item_id TEXT NOT NULL, pid INTEGER,
        started_at TEXT NOT NULL, ended_at TEXT, duration_seconds INTEGER,
        exit_code INTEGER, FOREIGN KEY(item_id) REFERENCES library_items(id)
    );",
    "CREATE TABLE provider_accounts(provider TEXT PRIMARY KEY,state TEXT NOT NULL,display_name TEXT,metadata TEXT NOT NULL DEFAULT '{}',updated_at TEXT NOT NULL);
    CREATE TABLE managed_dependencies(id TEXT PRIMARY KEY,provider TEXT NOT NULL,state TEXT NOT NULL,version TEXT,executable TEXT,metadata TEXT NOT NULL DEFAULT '{}',updated_at TEXT NOT NULL);
    CREATE TABLE transfer_operations(id TEXT PRIMARY KEY,provider TEXT NOT NULL,item_id TEXT,action TEXT NOT NULL,state TEXT NOT NULL,downloaded_bytes INTEGER NOT NULL DEFAULT 0,total_bytes INTEGER NOT NULL DEFAULT 0,bytes_per_second INTEGER NOT NULL DEFAULT 0,error TEXT,created_at TEXT NOT NULL,updated_at TEXT NOT NULL);
    CREATE INDEX idx_transfer_state ON transfer_operations(state,updated_at);
    CREATE INDEX idx_library_provider ON library_items(provider,installed);
    CREATE INDEX idx_library_hidden ON library_items(hidden);",
    "ALTER TABLE library_items ADD COLUMN terminal INTEGER NOT NULL DEFAULT 0;",
    "ALTER TABLE library_items ADD COLUMN compatibility TEXT NOT NULL DEFAULT '{}';",
];

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        let mut db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    #[cfg(test)]
    pub fn memory() -> Result<Self> {
        let mut db = Self {
            conn: Connection::open_in_memory()?,
        };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&mut self) -> Result<()> {
        self.conn.execute_batch("PRAGMA foreign_keys=ON;")?;
        let mut current: usize = self
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))?;
        let legacy_schema = current == 0
            && self
                .conn
                .query_row(
                    "SELECT 1 FROM sqlite_master WHERE type='table' AND name='library_items'",
                    [],
                    |_| Ok(true),
                )
                .optional()?
                .unwrap_or(false);
        if legacy_schema {
            let tx = self.conn.transaction()?;
            tx.execute_batch(
                "ALTER TABLE play_sessions ADD COLUMN pid INTEGER;
                 ALTER TABLE play_sessions ADD COLUMN duration_seconds INTEGER;
                 CREATE INDEX IF NOT EXISTS idx_library_provider ON library_items(provider,installed);
                 CREATE INDEX IF NOT EXISTS idx_library_hidden ON library_items(hidden);",
            )?;
            tx.execute_batch(
                "ALTER TABLE library_items ADD COLUMN terminal INTEGER NOT NULL DEFAULT 0;",
            )?;
            tx.execute_batch(
                "ALTER TABLE library_items ADD COLUMN compatibility TEXT NOT NULL DEFAULT '{}';",
            )?;
            tx.pragma_update(None, "user_version", MIGRATIONS.len())?;
            tx.commit()?;
            current = MIGRATIONS.len();
        }
        for (index, sql) in MIGRATIONS.iter().enumerate().skip(current) {
            let tx = self.conn.transaction()?;
            tx.execute_batch(sql)?;
            tx.pragma_update(None, "user_version", index + 1)?;
            tx.commit()?;
        }
        Ok(())
    }

    pub fn apply_provider_scan(
        &mut self,
        provider_name: &str,
        items: &[LibraryItem],
    ) -> Result<(usize, usize)> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "UPDATE library_items SET installed=0 WHERE provider=?1",
            [provider_name],
        )?;
        let mut added = 0;
        let mut updated = 0;
        for item in items {
            if upsert_scanned(&tx, item)? {
                added += 1;
            } else {
                updated += 1;
            }
        }
        tx.commit()?;
        Ok((added, updated))
    }

    pub fn save_user_item(&mut self, item: &LibraryItem) -> Result<()> {
        let tx = self.conn.transaction()?;
        upsert_scanned(&tx, item)?;
        tx.execute(
            "UPDATE library_items SET name=?1,kind=?2,provider=?3,executable=?4,args=?5,
             working_directory=?6,environment=?7,icon=?8,category=?9,terminal=?10,compatibility=?11,updated_at=?12 WHERE id=?13",
            params![item.name, kind(&item.kind), provider(&item.provider), item.executable,
                json(&item.arguments), item.working_directory, json(&item.environment),
                item.icon, item.category, item.terminal, json(&item.compatibility), item.updated_at, item.id],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn list(&self) -> Result<Vec<LibraryItem>> {
        let mut query = self.conn.prepare(&format!(
            "{} ORDER BY favorite DESC,name COLLATE NOCASE",
            SELECT_ITEM
        ))?;
        let items = query
            .query_map([], row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(items)
    }

    pub fn get(&self, id: &str) -> Result<Option<LibraryItem>> {
        Ok(self
            .conn
            .query_row(&format!("{} WHERE id=?1", SELECT_ITEM), [id], row)
            .optional()?)
    }

    pub fn flag(&self, id: &str, column: &str, value: bool) -> Result<()> {
        let sql = match column {
            "favorite" => "UPDATE library_items SET favorite=?1 WHERE id=?2",
            "hidden" => "UPDATE library_items SET hidden=?1 WHERE id=?2",
            _ => return Ok(()),
        };
        self.conn.execute(sql, params![value, id])?;
        Ok(())
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM library_items WHERE id=?1 AND provider='custom'",
            [id],
        )?;
        Ok(())
    }

    pub fn settings(&self) -> Result<AppSettings> {
        let value: Option<String> = self
            .conn
            .query_row("SELECT value FROM settings WHERE key='app'", [], |row| {
                row.get(0)
            })
            .optional()?;
        Ok(value
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default())
    }

    pub fn save_settings(&self, settings: &AppSettings) -> Result<()> {
        let value = serde_json::to_string(settings).unwrap_or_else(|_| "{}".into());
        self.conn.execute("INSERT INTO settings(key,value) VALUES('app',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [value])?;
        Ok(())
    }

    pub fn start_session(&self, id: &str, pid: u32) -> Result<i64> {
        let now = chrono::Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE library_items SET play_count=play_count+1,last_played_at=?1 WHERE id=?2",
            params![now, id],
        )?;
        self.conn.execute(
            "INSERT INTO play_sessions(item_id,pid,started_at) VALUES(?1,?2,?3)",
            params![id, pid, now],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn finish_session(
        &self,
        session_id: i64,
        duration: u64,
        exit_code: Option<i32>,
    ) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE play_sessions SET ended_at=?1,duration_seconds=?2,exit_code=?3 WHERE id=?4",
            params![now, duration, exit_code, session_id],
        )?;
        self.conn.execute(
            "UPDATE library_items SET total_play_time=total_play_time+?1 WHERE id=(SELECT item_id FROM play_sessions WHERE id=?2)",
            params![duration, session_id],
        )?;
        Ok(())
    }

    pub fn operations(&self) -> Result<Vec<Operation>> {
        let mut query = self.conn.prepare("SELECT id,provider,COALESCE(item_id,''),action,state,downloaded_bytes,total_bytes,bytes_per_second,error,created_at,updated_at FROM transfer_operations ORDER BY created_at DESC")?;
        let operations = query
            .query_map([], |row| {
                Ok(Operation {
                    id: row.get(0)?,
                    provider: row.get(1)?,
                    item_id: row.get(2)?,
                    action: row.get(3)?,
                    state: row.get(4)?,
                    downloaded_bytes: row.get(5)?,
                    total_bytes: row.get(6)?,
                    bytes_per_second: row.get(7)?,
                    error: row.get(8)?,
                    created_at: row.get(9)?,
                    updated_at: row.get(10)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(operations)
    }

    pub fn queue_operation(&self, operation: &Operation) -> Result<()> {
        self.conn.execute("INSERT INTO transfer_operations(id,provider,item_id,action,state,downloaded_bytes,total_bytes,bytes_per_second,error,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)", params![operation.id,operation.provider,operation.item_id,operation.action,operation.state,operation.downloaded_bytes,operation.total_bytes,operation.bytes_per_second,operation.error,operation.created_at,operation.updated_at])?;
        Ok(())
    }

    pub fn update_operation(
        &self,
        id: &str,
        state: &str,
        downloaded: u64,
        total: u64,
        speed: u64,
        error: Option<&str>,
    ) -> Result<()> {
        self.conn.execute("UPDATE transfer_operations SET state=?1,downloaded_bytes=?2,total_bytes=?3,bytes_per_second=?4,error=?5,updated_at=?6 WHERE id=?7", params![state,downloaded,total,speed,error,chrono::Utc::now().to_rfc3339(),id])?;
        Ok(())
    }

    pub fn recover_operations(&self) -> Result<()> {
        self.conn.execute("UPDATE transfer_operations SET state='queued',error='Recuperada após interrupção',updated_at=?1 WHERE state IN ('running','rolling_back')", [chrono::Utc::now().to_rfc3339()])?;
        Ok(())
    }

    pub fn backup_to(&self, path: &Path) -> Result<()> {
        let mut destination = Connection::open(path)?;
        let backup = rusqlite::backup::Backup::new(&self.conn, &mut destination)?;
        backup.run_to_completion(64, std::time::Duration::from_millis(10), None)?;
        Ok(())
    }

    pub fn restore_from(&mut self, path: &Path) -> Result<()> {
        let source = Connection::open(path)?;
        let integrity: String = source.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
        if integrity != "ok" {
            return Err(crate::error::LauncherError::InvalidArguments(
                "integridade do banco de backup falhou".into(),
            ));
        }
        let backup = rusqlite::backup::Backup::new(&source, &mut self.conn)?;
        backup.run_to_completion(64, std::time::Duration::from_millis(10), None)?;
        drop(backup);
        self.migrate()?;
        Ok(())
    }
}

const SELECT_ITEM: &str = "SELECT id,name,kind,provider,executable,args,working_directory,
environment,icon,cover,background,category,tags,favorite,hidden,installed,play_count,
total_play_time,last_played_at,created_at,updated_at,terminal,compatibility FROM library_items";

fn upsert_scanned(tx: &Transaction<'_>, item: &LibraryItem) -> Result<bool> {
    let exists = tx
        .query_row(
            "SELECT 1 FROM library_items WHERE id=?1",
            [&item.id],
            |_| Ok(true),
        )
        .optional()?
        .unwrap_or(false);
    tx.execute(
        "INSERT INTO library_items(id,name,kind,provider,executable,args,working_directory,
         environment,icon,cover,background,category,tags,favorite,hidden,installed,play_count,
         total_play_time,last_played_at,created_at,updated_at,terminal,compatibility)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,1,?16,?17,?18,?19,?20,?21,?22)
         ON CONFLICT(id) DO UPDATE SET name=excluded.name,kind=excluded.kind,
         executable=excluded.executable,working_directory=excluded.working_directory,
         icon=excluded.icon,cover=excluded.cover,background=excluded.background,category=excluded.category,
         installed=1,updated_at=excluded.updated_at",
        params![
            item.id,
            item.name,
            kind(&item.kind),
            provider(&item.provider),
            item.executable,
            json(&item.arguments),
            item.working_directory,
            json(&item.environment),
            item.icon,
            item.cover,
            item.background,
            item.category,
            json(&item.tags),
            item.favorite,
            item.hidden,
            item.play_count,
            item.total_play_time_seconds,
            item.last_played_at,
            item.created_at,
            item.updated_at,
            item.terminal,
            json(&item.compatibility)
        ],
    )?;
    Ok(!exists)
}

fn row(row: &rusqlite::Row<'_>) -> rusqlite::Result<LibraryItem> {
    let kind_value: String = row.get(2)?;
    let provider_value: String = row.get(3)?;
    Ok(LibraryItem {
        id: row.get(0)?,
        name: row.get(1)?,
        kind: match kind_value.as_str() {
            "game" => ItemKind::Game,
            "script" => ItemKind::Script,
            "custom" => ItemKind::Custom,
            _ => ItemKind::Application,
        },
        provider: ProviderKind::from_str(&provider_value),
        executable: row.get(4)?,
        arguments: from_json(row.get(5)?),
        working_directory: row.get(6)?,
        environment: from_json::<HashMap<String, String>>(row.get(7)?),
        icon: row.get(8)?,
        cover: row.get(9)?,
        background: row.get(10)?,
        category: row.get(11)?,
        tags: from_json(row.get(12)?),
        favorite: row.get(13)?,
        hidden: row.get(14)?,
        installed: row.get(15)?,
        play_count: row.get(16)?,
        total_play_time_seconds: row.get(17)?,
        last_played_at: row.get(18)?,
        created_at: row.get(19)?,
        updated_at: row.get(20)?,
        terminal: row.get(21)?,
        compatibility: from_json(row.get(22)?),
    })
}

fn kind(value: &ItemKind) -> &'static str {
    value.as_str()
}
fn provider(value: &ProviderKind) -> &'static str {
    value.as_str()
}
fn json<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".into())
}
fn from_json<T: serde::de::DeserializeOwned + Default>(value: String) -> T {
    serde_json::from_str(&value).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::model::*;

    #[test]
    fn preserves_user_flags_on_rescan_and_marks_removed() {
        let mut db = Database::memory().unwrap();
        let item = LibraryItem::new(
            "steam:1".into(),
            "Old".into(),
            ItemKind::Game,
            ProviderKind::Steam,
        );
        db.apply_provider_scan("steam", std::slice::from_ref(&item))
            .unwrap();
        db.flag(&item.id, "favorite", true).unwrap();
        let mut changed = item.clone();
        changed.name = "New".into();
        db.apply_provider_scan("steam", &[changed]).unwrap();
        let got = db.get(&item.id).unwrap().unwrap();
        assert!(got.favorite);
        assert_eq!(got.name, "New");
        db.apply_provider_scan("steam", &[]).unwrap();
        assert!(!db.get(&item.id).unwrap().unwrap().installed);
    }

    #[test]
    fn recovers_interrupted_operations() {
        let db = Database::memory().unwrap();
        let mut operation = crate::platform::ProviderManager::operation("epic", "game", "install");
        operation.state = "running".into();
        db.queue_operation(&operation).unwrap();
        db.recover_operations().unwrap();
        let recovered = db.operations().unwrap();
        assert_eq!(recovered[0].state, "queued");
        assert!(recovered[0]
            .error
            .as_deref()
            .unwrap()
            .contains("interrupção"));
    }
}
