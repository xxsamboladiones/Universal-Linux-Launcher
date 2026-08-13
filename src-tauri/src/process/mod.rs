use std::{
    collections::HashMap,
    path::PathBuf,
    process::Child,
    sync::{Arc, Mutex},
    time::Instant,
};

use crate::database::Database;

#[derive(Clone)]
pub struct ProcessManager {
    running: Arc<Mutex<HashMap<String, u32>>>,
    database_path: PathBuf,
}

impl ProcessManager {
    pub fn new(database_path: PathBuf) -> Self {
        Self {
            running: Arc::new(Mutex::new(HashMap::new())),
            database_path,
        }
    }

    pub fn track(&self, item_id: String, session_id: i64, mut child: Child) {
        let pid = child.id();
        self.running
            .lock()
            .expect("process lock poisoned")
            .insert(item_id.clone(), pid);
        let running = Arc::clone(&self.running);
        let database_path = self.database_path.clone();
        std::thread::spawn(move || {
            let started = Instant::now();
            let status = child.wait().ok();
            let duration = started.elapsed().as_secs();
            running
                .lock()
                .expect("process lock poisoned")
                .remove(&item_id);
            match Database::open(&database_path) {
                Ok(database) => {
                    if let Err(error) = database.finish_session(
                        session_id,
                        duration,
                        status.and_then(|value| value.code()),
                    ) {
                        tracing::error!(%error, "Falha ao finalizar sessão");
                    }
                }
                Err(error) => tracing::error!(%error, "Falha ao abrir banco para finalizar sessão"),
            }
        });
    }

    pub fn running(&self) -> HashMap<String, u32> {
        self.running.lock().expect("process lock poisoned").clone()
    }
}
