use std::{collections::HashMap, path::Path};

use rusqlite::{params, Connection, OptionalExtension, Transaction};

use crate::platform::Operation;
use crate::{
    core::model::{AppSettings, ArgumentPreset, ItemKind, LibraryItem, ProviderKind},
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
    "ALTER TABLE library_items ADD COLUMN owned INTEGER NOT NULL DEFAULT 1;
     CREATE INDEX idx_library_provider_owned ON library_items(provider,owned);",
    // Builds before the Epic installed-catalog split marked every entitlement
    // as installed and never persisted a local directory. Repair that legacy
    // state once so the first click offers installation instead of launch.
    "UPDATE library_items
     SET installed=0,updated_at=CURRENT_TIMESTAMP
     WHERE provider='epic' AND installed=1
       AND (working_directory IS NULL OR trim(working_directory)='');",
    "CREATE TABLE argument_presets(
        id TEXT PRIMARY KEY, name TEXT NOT NULL, arguments TEXT NOT NULL,
        created_at TEXT NOT NULL, updated_at TEXT NOT NULL
    );",
];

pub struct Database {
    conn: Connection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderAccountRecord {
    pub provider: String,
    pub state: String,
    pub display_name: Option<String>,
    pub updated_at: String,
}

impl Database {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;
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
            tx.execute_batch(
                "ALTER TABLE library_items ADD COLUMN owned INTEGER NOT NULL DEFAULT 1;
                 CREATE INDEX IF NOT EXISTS idx_library_provider_owned ON library_items(provider,owned);",
            )?;
            tx.execute_batch(
                "UPDATE library_items
                 SET installed=0,updated_at=CURRENT_TIMESTAMP
                 WHERE provider='epic' AND installed=1
                   AND (working_directory IS NULL OR trim(working_directory)='');",
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
            "UPDATE library_items SET owned=0,installed=0 WHERE provider=?1",
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

    pub fn provider_item_count(&self, provider_name: &str) -> Result<u64> {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM library_items WHERE provider=?1 AND owned=1",
                [provider_name],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    pub fn get(&self, id: &str) -> Result<Option<LibraryItem>> {
        Ok(self
            .conn
            .query_row(&format!("{} WHERE id=?1", SELECT_ITEM), [id], row)
            .optional()?)
    }

    pub fn set_installation(&self, id: &str, directory: &Path) -> Result<bool> {
        Ok(self.conn.execute(
            "UPDATE library_items
             SET installed=1,working_directory=?1,updated_at=?2
             WHERE id=?3 AND owned=1",
            params![
                directory.to_string_lossy(),
                chrono::Utc::now().to_rfc3339(),
                id
            ],
        )? == 1)
    }

    pub fn set_uninstalled(&self, id: &str) -> Result<bool> {
        Ok(self.conn.execute(
            "UPDATE library_items
             SET installed=0,working_directory=NULL,updated_at=?1
             WHERE id=?2 AND installed=1",
            params![chrono::Utc::now().to_rfc3339(), id],
        )? == 1)
    }

    /// Preenche apenas ícones ausentes de itens adicionados pelo usuário.
    /// A condição no próprio UPDATE evita sobrescrever uma imagem escolhida
    /// enquanto o backfill estava extraindo o recurso do executável.
    pub fn set_custom_icon_if_missing(&self, id: &str, icon: &Path) -> Result<bool> {
        let changed = self.conn.execute(
            "UPDATE library_items SET icon=?1,updated_at=?2
             WHERE id=?3 AND provider='custom' AND (icon IS NULL OR trim(icon)='')",
            params![icon.to_string_lossy(), chrono::Utc::now().to_rfc3339(), id],
        )?;
        Ok(changed == 1)
    }

    /// Repara IDs de tema persistidos por scans antigos sem sobrescrever uma
    /// resolução mais nova que tenha vencido a corrida.
    pub fn set_scanned_icon_if_matches(
        &self,
        id: &str,
        expected: &str,
        icon: &Path,
    ) -> Result<bool> {
        let changed = self.conn.execute(
            "UPDATE library_items SET icon=?1,updated_at=?2
             WHERE id=?3 AND provider IN ('desktop','flatpak') AND icon=?4",
            params![
                icon.to_string_lossy(),
                chrono::Utc::now().to_rfc3339(),
                id,
                expected
            ],
        )?;
        Ok(changed == 1)
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

    pub fn delete(&mut self, id: &str) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "DELETE FROM play_sessions
             WHERE item_id=?1
               AND EXISTS(SELECT 1 FROM library_items WHERE id=?1 AND provider='custom')",
            [id],
        )?;
        tx.execute(
            "DELETE FROM library_items WHERE id=?1 AND provider='custom'",
            [id],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn settings(&self) -> Result<AppSettings> {
        let value: Option<String> = self
            .conn
            .query_row("SELECT value FROM settings WHERE key='app'", [], |row| {
                row.get(0)
            })
            .optional()?;
        let mut settings: AppSettings = value
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default();
        if settings.last_manual_theme_id.is_empty() {
            settings.last_manual_theme_id = settings.active_theme_id.clone();
        }
        Ok(settings)
    }

    pub fn save_settings(&self, settings: &AppSettings) -> Result<()> {
        let value = serde_json::to_string(settings).unwrap_or_else(|_| "{}".into());
        self.conn.execute("INSERT INTO settings(key,value) VALUES('app',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [value])?;
        Ok(())
    }

    pub fn list_argument_presets(&self) -> Result<Vec<ArgumentPreset>> {
        let mut query = self.conn.prepare(
            "SELECT id,name,arguments,created_at,updated_at FROM argument_presets ORDER BY name COLLATE NOCASE"
        )?;
        let presets = query
            .query_map([], |row| {
                Ok(ArgumentPreset {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    arguments: serde_json::from_str(&row.get::<_, String>(2)?).unwrap_or_default(),
                    created_at: row.get(3)?,
                    updated_at: row.get(4)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(presets)
    }

    pub fn save_argument_preset(&self, preset: &ArgumentPreset) -> Result<()> {
        let args_json = serde_json::to_string(&preset.arguments).unwrap_or_else(|_| "[]".into());
        let now = chrono::Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO argument_presets(id,name,arguments,created_at,updated_at) VALUES(?1,?2,?3,?4,?5)
             ON CONFLICT(id) DO UPDATE SET name=excluded.name, arguments=excluded.arguments, updated_at=excluded.updated_at",
            params![&preset.id, &preset.name, &args_json, &preset.created_at, &now],
        )?;
        Ok(())
    }

    pub fn delete_argument_preset(&self, id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM argument_presets WHERE id=?1", [id])?;
        Ok(())
    }

    pub fn get_argument_preset(&self, id: &str) -> Result<Option<ArgumentPreset>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id,name,arguments,created_at,updated_at FROM argument_presets WHERE id=?1",
                [id],
                |row| {
                    Ok(ArgumentPreset {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        arguments: serde_json::from_str(&row.get::<_, String>(2)?)
                            .unwrap_or_default(),
                        created_at: row.get(3)?,
                        updated_at: row.get(4)?,
                    })
                },
            )
            .optional()?)
    }

    pub fn provider_account(&self, provider: &str) -> Result<Option<ProviderAccountRecord>> {
        let provider = provider.trim();
        if provider.is_empty() {
            return Err(crate::error::LauncherError::InvalidArguments(
                "provider da conta não pode ser vazio".into(),
            ));
        }

        Ok(self
            .conn
            .query_row(
                "SELECT provider,state,display_name,updated_at
                 FROM provider_accounts WHERE provider=?1",
                [provider],
                |row| {
                    Ok(ProviderAccountRecord {
                        provider: row.get(0)?,
                        state: row.get(1)?,
                        display_name: row.get(2)?,
                        updated_at: row.get(3)?,
                    })
                },
            )
            .optional()?)
    }

    pub fn upsert_provider_account(
        &self,
        provider: &str,
        state: &str,
        display_name: Option<&str>,
    ) -> Result<()> {
        let provider = provider.trim();
        let state = state.trim();
        if provider.is_empty() || state.is_empty() {
            return Err(crate::error::LauncherError::InvalidArguments(
                "provider e estado da conta são obrigatórios".into(),
            ));
        }
        let display_name = display_name.map(str::trim).filter(|name| !name.is_empty());

        self.conn.execute(
            "INSERT INTO provider_accounts(provider,state,display_name,updated_at)
             VALUES(?1,?2,?3,?4)
             ON CONFLICT(provider) DO UPDATE SET
                state=excluded.state,
                display_name=excluded.display_name,
                updated_at=excluded.updated_at",
            params![
                provider,
                state,
                display_name,
                chrono::Utc::now().to_rfc3339()
            ],
        )?;
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
        let mut query = self.conn.prepare(&format!(
            "SELECT {OPERATION_COLUMNS} FROM transfer_operations ORDER BY created_at DESC"
        ))?;
        let operations = query
            .query_map([], operation_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(operations)
    }

    pub fn operation(&self, id: &str) -> Result<Option<Operation>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {OPERATION_COLUMNS} FROM transfer_operations WHERE id=?1"),
                [id],
                operation_row,
            )
            .optional()?)
    }

    pub fn queue_operation(&self, operation: &Operation) -> Result<()> {
        let duplicate = self
            .conn
            .query_row(
                "SELECT 1 FROM transfer_operations
             WHERE provider=?1 AND item_id=?2 AND action=?3
               AND state IN ('queued','running','cancelling') LIMIT 1",
                params![operation.provider, operation.item_id, operation.action],
                |_| Ok(true),
            )
            .optional()?
            .unwrap_or(false);
        if duplicate {
            return Err(crate::error::LauncherError::InvalidArguments(
                "Já existe uma operação ativa para este item".into(),
            ));
        }
        self.conn.execute("INSERT INTO transfer_operations(id,provider,item_id,action,state,downloaded_bytes,total_bytes,bytes_per_second,error,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)", params![operation.id,operation.provider,operation.item_id,operation.action,operation.state,operation.downloaded_bytes,operation.total_bytes,operation.bytes_per_second,operation.error,operation.created_at,operation.updated_at])?;
        Ok(())
    }

    /// Claims a queued operation for a worker. Returning `None` means another
    /// actor removed, cancelled, or already started it before this worker.
    pub fn start_operation(&self, id: &str) -> Result<Option<Operation>> {
        Ok(self
            .conn
            .query_row(
                &format!(
                    "UPDATE transfer_operations
                     SET state='running',bytes_per_second=0,error=NULL,updated_at=?1
                     WHERE id=?2 AND state='queued'
                     RETURNING {OPERATION_COLUMNS}"
                ),
                params![chrono::Utc::now().to_rfc3339(), id],
                operation_row,
            )
            .optional()?)
    }

    /// Progress is accepted only while the worker owns a running operation.
    /// In particular, a late progress event cannot undo `cancelling`.
    pub fn update_running_progress(
        &self,
        id: &str,
        downloaded: u64,
        total: u64,
        speed: u64,
    ) -> Result<bool> {
        Ok(self.conn.execute(
            "UPDATE transfer_operations
             SET downloaded_bytes=?1,total_bytes=?2,bytes_per_second=?3,
                 error=NULL,updated_at=?4
             WHERE id=?5 AND state='running'",
            params![
                downloaded,
                total,
                speed,
                chrono::Utc::now().to_rfc3339(),
                id
            ],
        )? > 0)
    }

    /// Atomically requests cancellation for a queued or running operation.
    /// The current record is returned even when no transition was possible so
    /// callers can distinguish idempotent cancellation from a terminal state.
    pub fn request_cancel(&self, id: &str) -> Result<Option<Operation>> {
        let cancelled = self
            .conn
            .query_row(
                &format!(
                    "UPDATE transfer_operations
                     SET state='cancelling',bytes_per_second=0,updated_at=?1
                     WHERE id=?2 AND state IN ('queued','running')
                     RETURNING {OPERATION_COLUMNS}"
                ),
                params![chrono::Utc::now().to_rfc3339(), id],
                operation_row,
            )
            .optional()?;
        if cancelled.is_some() {
            return Ok(cancelled);
        }
        self.operation(id)
    }

    /// Returns a terminal operation to the queue without racing a concurrent
    /// retry/removal. A worker can only be spawned when this transition wins.
    pub fn retry_operation(&self, id: &str) -> Result<Option<Operation>> {
        Ok(self
            .conn
            .query_row(
                &format!(
                    "UPDATE transfer_operations
                     SET state='queued',bytes_per_second=0,error=NULL,updated_at=?1
                     WHERE id=?2 AND state IN ('failed','cancelled')
                     RETURNING {OPERATION_COLUMNS}"
                ),
                params![chrono::Utc::now().to_rfc3339(), id],
                operation_row,
            )
            .optional()?)
    }

    /// Commits a worker outcome without overwriting a concurrent cancellation.
    /// A running operation reaches the requested terminal outcome, while an
    /// operation in `cancelling` always reaches `cancelled`.
    pub fn finish_operation(
        &self,
        id: &str,
        outcome: &str,
        downloaded: u64,
        total: u64,
        error: Option<&str>,
    ) -> Result<Option<Operation>> {
        if !matches!(outcome, "completed" | "failed") {
            return Err(crate::error::LauncherError::InvalidArguments(
                "resultado final da operação deve ser completed ou failed".into(),
            ));
        }

        Ok(self
            .conn
            .query_row(
                &format!(
                    "UPDATE transfer_operations
                     SET state=CASE WHEN state='cancelling' THEN 'cancelled' ELSE ?1 END,
                         downloaded_bytes=?2,total_bytes=?3,bytes_per_second=0,
                         error=CASE WHEN state='cancelling' THEN NULL ELSE ?4 END,
                         updated_at=?5
                     WHERE id=?6 AND state IN ('running','cancelling')
                     RETURNING {OPERATION_COLUMNS}"
                ),
                params![
                    outcome,
                    downloaded,
                    total,
                    error,
                    chrono::Utc::now().to_rfc3339(),
                    id
                ],
                operation_row,
            )
            .optional()?)
    }

    pub fn remove_operation(&self, id: &str) -> Result<bool> {
        Ok(self.conn.execute(
            "DELETE FROM transfer_operations
             WHERE id=?1 AND state IN ('queued','completed','failed','cancelled')",
            [id],
        )? > 0)
    }

    pub fn recover_operations(&self) -> Result<()> {
        // Nenhum processo filho sobrevive ao Orbit. Um cancelamento pendente
        // pode ser concluído com segurança; os demais trabalhos ativos voltam
        // como falha recuperável, sem criar uma fila fantasma sem worker.
        self.conn.execute(
            "UPDATE transfer_operations
             SET state=CASE WHEN state='cancelling' THEN 'cancelled' ELSE 'failed' END,
                 error=CASE WHEN state='cancelling' THEN NULL
                            ELSE 'Operação interrompida; use Repetir ou Remover' END,
                 bytes_per_second=0,updated_at=?1
             WHERE state IN ('queued','running','cancelling','rolling_back')",
            [chrono::Utc::now().to_rfc3339()],
        )?;
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
environment,icon,cover,background,category,tags,favorite,hidden,owned,installed,play_count,
total_play_time,last_played_at,created_at,updated_at,terminal,compatibility FROM library_items";

const OPERATION_COLUMNS: &str = "id,provider,COALESCE(item_id,''),action,state,
downloaded_bytes,total_bytes,bytes_per_second,error,created_at,updated_at";

fn operation_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Operation> {
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
}

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
         environment,icon,cover,background,category,tags,favorite,hidden,owned,installed,play_count,
         total_play_time,last_played_at,created_at,updated_at,terminal,compatibility)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24)
         ON CONFLICT(id) DO UPDATE SET name=excluded.name,kind=excluded.kind,
         executable=excluded.executable,working_directory=excluded.working_directory,
         icon=excluded.icon,cover=excluded.cover,background=excluded.background,category=excluded.category,
         owned=excluded.owned,installed=excluded.installed,updated_at=excluded.updated_at",
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
            item.owned,
            item.installed,
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
        owned: row.get(15)?,
        installed: row.get(16)?,
        play_count: row.get(17)?,
        total_play_time_seconds: row.get(18)?,
        last_played_at: row.get(19)?,
        created_at: row.get(20)?,
        updated_at: row.get(21)?,
        terminal: row.get(22)?,
        compatibility: from_json(row.get(23)?),
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
    fn migrates_the_last_manual_theme_from_legacy_settings() {
        let db = Database::memory().unwrap();
        db.conn
            .execute(
                "INSERT INTO settings(key,value) VALUES('app',?1)",
                [r#"{"activeThemeId":"midnight","themeMode":"automatic"}"#],
            )
            .unwrap();

        let settings = db.settings().unwrap();

        assert_eq!(settings.active_theme_id, "midnight");
        assert_eq!(settings.last_manual_theme_id, "midnight");
        assert_eq!(settings.theme_mode, "automatic");
    }

    #[test]
    fn migration_repairs_legacy_epic_catalog_without_touching_local_entries() {
        let conn = Connection::open_in_memory().unwrap();
        for migration in MIGRATIONS.iter().take(5) {
            conn.execute_batch(migration).unwrap();
        }
        conn.pragma_update(None, "user_version", 5).unwrap();
        let mut db = Database { conn };

        let legacy = LibraryItem::new(
            "epic:legacy".into(),
            "Legacy entitlement".into(),
            ItemKind::Game,
            ProviderKind::Epic,
        );
        db.save_user_item(&legacy).unwrap();
        let mut local = LibraryItem::new(
            "epic:local".into(),
            "Local Epic game".into(),
            ItemKind::Game,
            ProviderKind::Epic,
        );
        local.working_directory = Some("/games/epic/local".into());
        db.save_user_item(&local).unwrap();
        let desktop = LibraryItem::new(
            "desktop:app".into(),
            "Desktop app".into(),
            ItemKind::Application,
            ProviderKind::Desktop,
        );
        db.save_user_item(&desktop).unwrap();

        db.migrate().unwrap();

        assert!(!db.get(&legacy.id).unwrap().unwrap().installed);
        assert!(db.get(&local.id).unwrap().unwrap().installed);
        assert!(db.get(&desktop.id).unwrap().unwrap().installed);
        assert_eq!(
            db.conn
                .query_row("PRAGMA user_version", [], |row| row.get::<_, usize>(0))
                .unwrap(),
            MIGRATIONS.len()
        );
    }

    #[test]
    fn deletes_custom_items_with_play_history_without_touching_provider_items() {
        let mut db = Database::memory().unwrap();
        let custom = LibraryItem::new(
            "custom:played".into(),
            "Played shortcut".into(),
            ItemKind::Application,
            ProviderKind::Custom,
        );
        let desktop = LibraryItem::new(
            "desktop:played".into(),
            "Provider shortcut".into(),
            ItemKind::Application,
            ProviderKind::Desktop,
        );
        db.save_user_item(&custom).unwrap();
        db.save_user_item(&desktop).unwrap();
        db.start_session(&custom.id, 100).unwrap();
        db.start_session(&desktop.id, 101).unwrap();

        db.delete(&custom.id).unwrap();
        db.delete(&desktop.id).unwrap();

        assert!(db.get(&custom.id).unwrap().is_none());
        assert!(db.get(&desktop.id).unwrap().is_some());
        let custom_sessions: usize = db
            .conn
            .query_row(
                "SELECT count(*) FROM play_sessions WHERE item_id=?1",
                [&custom.id],
                |row| row.get(0),
            )
            .unwrap();
        let desktop_sessions: usize = db
            .conn
            .query_row(
                "SELECT count(*) FROM play_sessions WHERE item_id=?1",
                [&desktop.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(custom_sessions, 0);
        assert_eq!(desktop_sessions, 1);
    }

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
        let removed = db.get(&item.id).unwrap().unwrap();
        assert!(!removed.owned);
        assert!(!removed.installed);
    }

    #[test]
    fn provider_scan_persists_owned_separately_from_installed() {
        let mut db = Database::memory().unwrap();
        let mut owned_only = LibraryItem::new(
            "epic:owned-only".into(),
            "Owned only".into(),
            ItemKind::Game,
            ProviderKind::Epic,
        );
        owned_only.installed = false;
        let installed = LibraryItem::new(
            "epic:installed".into(),
            "Installed".into(),
            ItemKind::Game,
            ProviderKind::Epic,
        );

        db.apply_provider_scan("epic", &[owned_only.clone(), installed.clone()])
            .unwrap();

        let owned_only = db.get(&owned_only.id).unwrap().unwrap();
        assert!(owned_only.owned);
        assert!(!owned_only.installed);
        let installed = db.get(&installed.id).unwrap().unwrap();
        assert!(installed.owned);
        assert!(installed.installed);
        assert_eq!(db.provider_item_count("epic").unwrap(), 2);
    }

    #[test]
    fn completed_download_marks_only_an_owned_item_as_installed() {
        let mut db = Database::memory().unwrap();
        let mut item = LibraryItem::new(
            "epic:downloaded".into(),
            "Downloaded".into(),
            ItemKind::Game,
            ProviderKind::Epic,
        );
        item.installed = false;
        db.apply_provider_scan("epic", std::slice::from_ref(&item))
            .unwrap();

        let directory = Path::new("/games/epic/downloaded");
        assert!(db.set_installation(&item.id, directory).unwrap());
        let installed = db.get(&item.id).unwrap().unwrap();
        assert!(installed.installed);
        assert_eq!(installed.working_directory.as_deref(), directory.to_str());

        db.apply_provider_scan("epic", &[]).unwrap();
        assert!(!db.set_installation(&item.id, directory).unwrap());
        assert!(!db.get(&item.id).unwrap().unwrap().installed);
    }

    #[test]
    fn uninstall_keeps_entitlement_and_clears_the_local_directory() {
        let mut db = Database::memory().unwrap();
        let mut item = LibraryItem::new(
            "epic:game".into(),
            "Game".into(),
            ItemKind::Game,
            ProviderKind::Epic,
        );
        item.working_directory = Some("/games/epic/game".into());
        db.apply_provider_scan("epic", std::slice::from_ref(&item))
            .unwrap();

        assert!(db.set_uninstalled(&item.id).unwrap());
        let item = db.get(&item.id).unwrap().unwrap();
        assert!(item.owned);
        assert!(!item.installed);
        assert_eq!(item.working_directory, None);
    }

    #[test]
    fn repairs_only_the_expected_scanned_icon_name() {
        let mut db = Database::memory().unwrap();
        let mut item = LibraryItem::new(
            "desktop:cachyos-hello".into(),
            "CachyOS Hello".into(),
            ItemKind::Application,
            ProviderKind::Desktop,
        );
        item.icon = Some("org.cachyos.hello".into());
        db.apply_provider_scan("desktop", std::slice::from_ref(&item))
            .unwrap();

        let resolved = Path::new("/usr/share/icons/org.cachyos.hello.svg");
        assert!(db
            .set_scanned_icon_if_matches(&item.id, "org.cachyos.hello", resolved)
            .unwrap());
        assert!(!db
            .set_scanned_icon_if_matches(&item.id, "org.cachyos.hello", Path::new("stale.svg"))
            .unwrap());
        assert_eq!(
            db.get(&item.id).unwrap().unwrap().icon.as_deref(),
            resolved.to_str()
        );
    }

    #[test]
    fn recovers_interrupted_and_cancelling_operations() {
        let db = Database::memory().unwrap();
        let mut running = crate::platform::ProviderManager::operation("epic", "game", "install");
        running.state = "running".into();
        db.queue_operation(&running).unwrap();
        let mut cancelling =
            crate::platform::ProviderManager::operation("steam", "other", "install");
        cancelling.state = "cancelling".into();
        db.queue_operation(&cancelling).unwrap();

        db.recover_operations().unwrap();
        let recovered_running = db.operation(&running.id).unwrap().unwrap();
        assert_eq!(recovered_running.state, "failed");
        assert!(recovered_running
            .error
            .as_deref()
            .unwrap()
            .contains("interrompida"));
        let recovered_cancelling = db.operation(&cancelling.id).unwrap().unwrap();
        assert_eq!(recovered_cancelling.state, "cancelled");
        assert_eq!(recovered_cancelling.error, None);
    }

    #[test]
    fn removes_terminal_or_queued_operations_but_not_running() {
        let db = Database::memory().unwrap();
        let queued = crate::platform::ProviderManager::operation("steam", "1", "install");
        db.queue_operation(&queued).unwrap();
        assert!(db.remove_operation(&queued.id).unwrap());
        assert!(db.operations().unwrap().is_empty());

        let mut running = crate::platform::ProviderManager::operation("steam", "2", "install");
        running.state = "running".into();
        db.queue_operation(&running).unwrap();
        assert!(!db.remove_operation(&running.id).unwrap());
        assert_eq!(db.operations().unwrap().len(), 1);

        let cancelling = db.request_cancel(&running.id).unwrap().unwrap();
        assert_eq!(cancelling.state, "cancelling");
        assert!(!db.remove_operation(&running.id).unwrap());
        db.finish_operation(&running.id, "failed", 0, 0, Some("cancelada"))
            .unwrap();
        assert!(db.remove_operation(&running.id).unwrap());
        assert!(db.operations().unwrap().is_empty());
    }

    #[test]
    fn operation_lifecycle_uses_compare_and_swap_transitions() {
        let db = Database::memory().unwrap();
        let queued = crate::platform::ProviderManager::operation("steam", "42", "install");
        db.queue_operation(&queued).unwrap();
        assert_eq!(db.operation(&queued.id).unwrap().unwrap().state, "queued");

        let running = db.start_operation(&queued.id).unwrap().unwrap();
        assert_eq!(running.state, "running");
        assert!(db.start_operation(&queued.id).unwrap().is_none());
        assert!(db.update_running_progress(&queued.id, 25, 100, 10).unwrap());

        let cancelling = db.request_cancel(&queued.id).unwrap().unwrap();
        assert_eq!(cancelling.state, "cancelling");
        assert!(!db.update_running_progress(&queued.id, 50, 100, 10).unwrap());

        // A conclusão tardia do worker não pode vencer o cancelamento.
        let cancelled = db
            .finish_operation(&queued.id, "completed", 25, 100, None)
            .unwrap()
            .unwrap();
        assert_eq!(cancelled.state, "cancelled");
        assert_eq!(cancelled.downloaded_bytes, 25);
        assert_eq!(cancelled.bytes_per_second, 0);
        assert_eq!(cancelled.error, None);
        assert!(db
            .finish_operation(&queued.id, "failed", 25, 100, Some("late"))
            .unwrap()
            .is_none());
    }

    #[test]
    fn finish_operation_accepts_only_terminal_worker_outcomes() {
        let db = Database::memory().unwrap();
        let completed = crate::platform::ProviderManager::operation("steam", "1", "install");
        db.queue_operation(&completed).unwrap();
        db.start_operation(&completed.id).unwrap();
        let completed = db
            .finish_operation(&completed.id, "completed", 100, 100, None)
            .unwrap()
            .unwrap();
        assert_eq!(completed.state, "completed");

        let failed = crate::platform::ProviderManager::operation("steam", "2", "install");
        db.queue_operation(&failed).unwrap();
        db.start_operation(&failed.id).unwrap();
        let failed = db
            .finish_operation(&failed.id, "failed", 10, 100, Some("erro"))
            .unwrap()
            .unwrap();
        assert_eq!(failed.state, "failed");
        assert_eq!(failed.error.as_deref(), Some("erro"));

        assert!(db
            .finish_operation(&failed.id, "cancelled", 10, 100, None)
            .is_err());
    }

    #[test]
    fn queued_cancellation_is_idempotent_and_blocks_duplicate_work() {
        let db = Database::memory().unwrap();
        let queued = crate::platform::ProviderManager::operation("steam", "99", "install");
        db.queue_operation(&queued).unwrap();
        assert_eq!(
            db.request_cancel(&queued.id).unwrap().unwrap().state,
            "cancelling"
        );
        assert_eq!(
            db.request_cancel(&queued.id).unwrap().unwrap().state,
            "cancelling"
        );
        assert!(db.start_operation(&queued.id).unwrap().is_none());

        let duplicate = crate::platform::ProviderManager::operation("steam", "99", "install");
        assert!(db.queue_operation(&duplicate).is_err());
    }

    #[test]
    fn retry_is_an_atomic_single_winner_transition() {
        let db = Database::memory().unwrap();
        let operation = crate::platform::ProviderManager::operation("steam", "7", "install");
        db.queue_operation(&operation).unwrap();
        db.start_operation(&operation.id).unwrap();
        db.finish_operation(&operation.id, "failed", 12, 100, Some("test"))
            .unwrap();

        let retried = db.retry_operation(&operation.id).unwrap().unwrap();
        assert_eq!(retried.state, "queued");
        assert_eq!(retried.downloaded_bytes, 12);
        assert_eq!(retried.error, None);
        assert!(db.retry_operation(&operation.id).unwrap().is_none());
    }

    #[test]
    fn rejects_duplicate_active_operation_for_the_same_item() {
        let db = Database::memory().unwrap();
        let first = crate::platform::ProviderManager::operation("steam", "1050280", "install");
        db.queue_operation(&first).unwrap();
        let duplicate = crate::platform::ProviderManager::operation("steam", "1050280", "install");
        assert!(db.queue_operation(&duplicate).is_err());

        db.start_operation(&first.id).unwrap();
        db.finish_operation(&first.id, "failed", 0, 0, Some("test"))
            .unwrap();
        let replacement =
            crate::platform::ProviderManager::operation("steam", "1050280", "install");
        db.queue_operation(&replacement).unwrap();
    }

    #[test]
    fn provider_account_round_trip_only_exposes_non_secret_state() {
        let db = Database::memory().unwrap();
        assert!(db.provider_account("steam").unwrap().is_none());

        db.upsert_provider_account(" steam ", " connected ", Some(" Player One "))
            .unwrap();
        let connected = db.provider_account("steam").unwrap().unwrap();
        assert_eq!(connected.provider, "steam");
        assert_eq!(connected.state, "connected");
        assert_eq!(connected.display_name.as_deref(), Some("Player One"));
        assert!(!connected.updated_at.is_empty());

        let metadata: String = db
            .conn
            .query_row(
                "SELECT metadata FROM provider_accounts WHERE provider='steam'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(metadata, "{}");

        db.upsert_provider_account("steam", "disconnected", Some("   "))
            .unwrap();
        let disconnected = db.provider_account("steam").unwrap().unwrap();
        assert_eq!(disconnected.state, "disconnected");
        assert_eq!(disconnected.display_name, None);
    }

    #[test]
    fn provider_account_rejects_empty_identity_fields() {
        let db = Database::memory().unwrap();
        assert!(db
            .upsert_provider_account("", "connected", Some("Player"))
            .is_err());
        assert!(db
            .upsert_provider_account("steam", "   ", Some("Player"))
            .is_err());
        assert!(db.provider_account("  ").is_err());
    }
}
