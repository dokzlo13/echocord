//! Small, non-sensitive window-size preference, independent of audio settings.
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
use tauri::Manager;

const MIN_SIZE: u32 = 420;
const MAX_SIZE: u32 = 8192;

struct WindowMemory {
    path: Option<PathBuf>,
    size: Mutex<Option<(u32, u32)>>,
}

pub fn preference_path(settings: Option<&Path>, token: Option<&Path>) -> Option<PathBuf> {
    let path = settings?.with_extension("window-size");
    if token.is_some_and(|token| {
        path == token
            || matches!((path.canonicalize(), token.canonicalize()), (Ok(a), Ok(b)) if a == b)
    }) {
        None
    } else {
        Some(path)
    }
}

fn read_size(path: &Path) -> Option<(u32, u32)> {
    let mut bytes = Vec::new();
    fs::File::open(path)
        .ok()?
        .take(65)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > 64 {
        return None;
    }
    let text = std::str::from_utf8(&bytes).ok()?;
    let mut fields = text.split_whitespace();
    let width = fields.next()?.parse::<u32>().ok()?;
    let height = fields.next()?.parse::<u32>().ok()?;
    if fields.next().is_some()
        || !(MIN_SIZE..=MAX_SIZE).contains(&width)
        || !(MIN_SIZE..=MAX_SIZE).contains(&height)
    {
        return None;
    }
    Some((width, height))
}

pub fn install(app: &tauri::App, path: Option<PathBuf>) {
    let saved = path.as_deref().and_then(read_size);
    app.manage(WindowMemory {
        path,
        size: Mutex::new(saved),
    });
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    if let Some((mut width, mut height)) = saved {
        if let Ok(Some(monitor)) = window.current_monitor() {
            let available = monitor.size().to_logical::<u32>(monitor.scale_factor());
            width = width.min(available.width.saturating_sub(24).max(MIN_SIZE));
            height = height.min(available.height.saturating_sub(64).max(MIN_SIZE));
        }
        let _ = window.set_size(tauri::LogicalSize::new(width, height));
        let _ = window.center();
    }
}

pub fn remember(window: &tauri::Window) {
    if window.label() != "main" || window.is_minimized().unwrap_or(true) {
        return;
    }
    let Some(memory) = window.app_handle().try_state::<WindowMemory>() else {
        return;
    };
    let (Ok(physical), Ok(scale)) = (window.inner_size(), window.scale_factor()) else {
        return;
    };
    let size = physical.to_logical::<u32>(scale);
    if (MIN_SIZE..=MAX_SIZE).contains(&size.width) && (MIN_SIZE..=MAX_SIZE).contains(&size.height) {
        *memory.size.lock().unwrap_or_else(|e| e.into_inner()) = Some((size.width, size.height));
    }
}

/// Persist only at close/quit, not on every resize or meter update.
pub fn persist(app: &tauri::AppHandle) {
    let Some(memory) = app.try_state::<WindowMemory>() else {
        return;
    };
    let Some(path) = memory.path.as_ref() else {
        return;
    };
    let Some((width, height)) = *memory.size.lock().unwrap_or_else(|e| e.into_inner()) else {
        return;
    };
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let temporary = path.with_extension(format!(
        "window-{}-{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)?;
        writeln!(file, "{width} {height}")?;
        file.sync_all()?;
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
}
