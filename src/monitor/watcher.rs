use anyhow::Result;
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::sync::mpsc;
use std::time::Duration;

pub enum WatchEvent {
    SessionsChanged,
}

pub fn start_watcher(
    tx: mpsc::Sender<WatchEvent>,
) -> Result<RecommendedWatcher> {
    let claude_dir = dirs::home_dir()
        .expect("no home dir")
        .join(".claude");

    let sessions_dir = claude_dir.join("sessions");
    let projects_dir = claude_dir.join("projects");

    let tx2 = tx.clone();
    let mut watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
        if let Ok(_event) = res {
            let _ = tx2.send(WatchEvent::SessionsChanged);
        }
    })?;

    if sessions_dir.exists() {
        watcher.watch(&sessions_dir, RecursiveMode::NonRecursive)?;
    }

    // Watch project directories for JSONL changes
    if projects_dir.exists() {
        watcher.watch(&projects_dir, RecursiveMode::Recursive)?;
    }

    // Also start a periodic ticker for status refreshes (process liveness, etc.)
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(3));
            if tx.send(WatchEvent::SessionsChanged).is_err() {
                break;
            }
        }
    });

    Ok(watcher)
}

