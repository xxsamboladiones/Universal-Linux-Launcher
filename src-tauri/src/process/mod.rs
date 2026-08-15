use std::{
    collections::HashMap,
    path::PathBuf,
    process::Child,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use crate::database::Database;

#[derive(Debug, Clone, PartialEq, Eq)]
enum TrackingStrategy {
    ProcessTree,
    SteamApp(String),
}

#[derive(Debug, Clone, Copy)]
struct TrackedSession {
    session_id: i64,
    display_pid: u32,
}

type RunningSessions = Arc<Mutex<HashMap<String, TrackedSession>>>;

#[derive(Clone)]
pub struct ProcessManager {
    running: RunningSessions,
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
        let root_pid = child.id();
        self.running
            .lock()
            .expect("process lock poisoned")
            .insert(
                item_id.clone(),
                TrackedSession {
                    session_id,
                    display_pid: root_pid,
                },
            );

        let running = Arc::clone(&self.running);
        let database_path = self.database_path.clone();
        std::thread::spawn(move || {
            let started = Instant::now();
            let strategy = tracking_strategy(&item_id);
            let exit_code = monitor_lifetime(
                &item_id,
                session_id,
                root_pid,
                &strategy,
                &mut child,
                &running,
            );
            let duration = started.elapsed().as_secs();

            remove_if_current(&running, &item_id, session_id);
            match Database::open(&database_path) {
                Ok(database) => {
                    if let Err(error) = database.finish_session(session_id, duration, exit_code) {
                        tracing::error!(%error, "Falha ao finalizar sessão");
                    }
                }
                Err(error) => tracing::error!(%error, "Falha ao abrir banco para finalizar sessão"),
            }
        });
    }

    pub fn running(&self) -> HashMap<String, u32> {
        self.running
            .lock()
            .expect("process lock poisoned")
            .iter()
            .map(|(item_id, session)| (item_id.clone(), session.display_pid))
            .collect()
    }
}

fn tracking_strategy(item_id: &str) -> TrackingStrategy {
    let Some(app_id) = item_id.strip_prefix("steam:") else {
        return TrackingStrategy::ProcessTree;
    };
    if !app_id.is_empty() && app_id.chars().all(|character| character.is_ascii_digit()) {
        TrackingStrategy::SteamApp(app_id.to_string())
    } else {
        TrackingStrategy::ProcessTree
    }
}

fn update_display_pid(
    running: &RunningSessions,
    item_id: &str,
    session_id: i64,
    display_pid: u32,
) {
    let mut running = running.lock().expect("process lock poisoned");
    if let Some(session) = running.get_mut(item_id) {
        if session.session_id == session_id {
            session.display_pid = display_pid;
        }
    }
}

fn remove_if_current(running: &RunningSessions, item_id: &str, session_id: i64) {
    let mut running = running.lock().expect("process lock poisoned");
    if running
        .get(item_id)
        .is_some_and(|session| session.session_id == session_id)
    {
        running.remove(item_id);
    }
}

#[cfg(target_os = "linux")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ProcessIdentity {
    pid: u32,
    start_ticks: u64,
}

#[cfg(target_os = "linux")]
fn process_identity(pid: u32) -> Option<ProcessIdentity> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let end = stat.rfind(") ")?;
    let fields = stat[end + 2..].split_whitespace().collect::<Vec<_>>();
    let start_ticks = fields.get(19)?.parse().ok()?;
    Some(ProcessIdentity { pid, start_ticks })
}

#[cfg(target_os = "linux")]
fn process_identity_alive(identity: ProcessIdentity) -> bool {
    process_identity(identity.pid).is_some_and(|current| current.start_ticks == identity.start_ticks)
}

#[cfg(target_os = "linux")]
fn direct_children(pid: u32) -> Vec<u32> {
    std::fs::read_to_string(format!("/proc/{pid}/task/{pid}/children"))
        .ok()
        .map(|children| {
            children
                .split_whitespace()
                .filter_map(|value| value.parse().ok())
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(target_os = "linux")]
fn discover_descendants(known: &mut HashMap<u32, ProcessIdentity>) {
    loop {
        let parents = known.values().copied().collect::<Vec<_>>();
        let mut added = false;
        for parent in parents {
            if !process_identity_alive(parent) {
                continue;
            }
            for child_pid in direct_children(parent.pid) {
                if known.contains_key(&child_pid) {
                    continue;
                }
                if let Some(identity) = process_identity(child_pid) {
                    known.insert(child_pid, identity);
                    added = true;
                }
            }
        }
        if !added {
            break;
        }
    }
}

#[cfg(target_os = "linux")]
fn steam_app_processes(app_id: &str) -> Vec<ProcessIdentity> {
    let app_id_key = format!("SteamAppId={app_id}");
    let game_id_key = format!("SteamGameId={app_id}");
    let current_pid = std::process::id();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };

    entries
        .flatten()
        .filter_map(|entry| entry.file_name().to_string_lossy().parse::<u32>().ok())
        .filter(|pid| *pid != current_pid)
        .filter_map(|pid| {
            let environment = std::fs::read(format!("/proc/{pid}/environ")).ok()?;
            let matches = environment.split(|byte| *byte == 0).any(|variable| {
                variable == app_id_key.as_bytes() || variable == game_id_key.as_bytes()
            });
            matches.then(|| process_identity(pid)).flatten()
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn monitor_lifetime(
    item_id: &str,
    session_id: i64,
    root_pid: u32,
    strategy: &TrackingStrategy,
    child: &mut Child,
    running: &RunningSessions,
) -> Option<i32> {
    const GENERIC_HANDOFF_GRACE: Duration = Duration::from_millis(750);
    const STEAM_HANDOFF_GRACE: Duration = Duration::from_secs(15);

    let started = Instant::now();
    let mut known = HashMap::new();
    if let Some(root) = process_identity(root_pid) {
        known.insert(root_pid, root);
    }

    let mut root_reaped = false;
    let mut root_exit_code = None;
    let mut root_exited_at = None;
    let mut steam_bound = false;

    loop {
        discover_descendants(&mut known);

        if let TrackingStrategy::SteamApp(app_id) = strategy {
            if !steam_bound {
                let matches = steam_app_processes(app_id);
                if !matches.is_empty() {
                    for identity in matches {
                        known.insert(identity.pid, identity);
                    }
                    steam_bound = true;
                }
            }
        }

        if !root_reaped {
            match child.try_wait() {
                Ok(Some(status)) => {
                    root_reaped = true;
                    root_exit_code = status.code();
                    root_exited_at = Some(Instant::now());
                }
                Ok(None) => {}
                Err(error) => {
                    tracing::warn!(%error, pid=root_pid, "Falha ao consultar processo raiz");
                    if process_identity(root_pid).is_none() {
                        root_exited_at.get_or_insert_with(Instant::now);
                    }
                }
            }
        }

        let live_descendants = known
            .values()
            .copied()
            .filter(|identity| identity.pid != root_pid && process_identity_alive(*identity))
            .collect::<Vec<_>>();

        if let Some(identity) = live_descendants.first() {
            update_display_pid(running, item_id, session_id, identity.pid);
        }

        let Some(root_exited_at) = root_exited_at else {
            std::thread::sleep(poll_interval(started.elapsed()));
            continue;
        };

        if !live_descendants.is_empty() {
            std::thread::sleep(poll_interval(started.elapsed()));
            continue;
        }

        let grace = match strategy {
            TrackingStrategy::SteamApp(_) if !steam_bound => STEAM_HANDOFF_GRACE,
            _ => GENERIC_HANDOFF_GRACE,
        };
        if root_exited_at.elapsed() < grace {
            std::thread::sleep(poll_interval(started.elapsed()));
            continue;
        }
        break;
    }

    if !root_reaped {
        match child.wait() {
            Ok(status) => root_exit_code = status.code(),
            Err(error) => tracing::warn!(%error, pid=root_pid, "Falha ao reaproveitar processo raiz"),
        }
    }
    root_exit_code
}

#[cfg(not(target_os = "linux"))]
fn monitor_lifetime(
    _item_id: &str,
    _session_id: i64,
    _root_pid: u32,
    _strategy: &TrackingStrategy,
    child: &mut Child,
    _running: &RunningSessions,
) -> Option<i32> {
    child.wait().ok().and_then(|status| status.code())
}

fn poll_interval(elapsed: Duration) -> Duration {
    if elapsed < Duration::from_secs(3) {
        Duration::from_millis(100)
    } else {
        Duration::from_millis(500)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steam_items_get_a_provider_specific_strategy() {
        assert_eq!(
            tracking_strategy("steam:730"),
            TrackingStrategy::SteamApp("730".into())
        );
        assert_eq!(
            tracking_strategy("steam:not-an-id"),
            TrackingStrategy::ProcessTree
        );
        assert_eq!(
            tracking_strategy("epic:example"),
            TrackingStrategy::ProcessTree
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn process_identity_rejects_pid_reuse() {
        let current = process_identity(std::process::id()).expect("current process identity");
        assert!(process_identity_alive(current));
        assert!(!process_identity_alive(ProcessIdentity {
            pid: current.pid,
            start_ticks: current.start_ticks.saturating_add(1),
        }));
    }

    #[test]
    fn a_stale_session_cannot_remove_a_newer_one() {
        let running = Arc::new(Mutex::new(HashMap::from([(
            "custom:test".into(),
            TrackedSession {
                session_id: 2,
                display_pid: 200,
            },
        )])));

        remove_if_current(&running, "custom:test", 1);
        assert_eq!(
            running
                .lock()
                .unwrap()
                .get("custom:test")
                .map(|session| session.display_pid),
            Some(200)
        );

        remove_if_current(&running, "custom:test", 2);
        assert!(running.lock().unwrap().is_empty());
    }
}
