#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod window_state;

use echocord_core::{
    controller::{Controller, ControllerConfig},
    model::{AppError, BotStatus, Connection, ControlCommand, ControlSnapshot, Discovery},
};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use tauri::{
    Emitter, Listener, Manager, RunEvent, State, WindowEvent,
    menu::{CheckMenuItem, Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

struct ShellState {
    controller: Controller,
    exiting: AtomicBool,
    snapshot_requested: AtomicBool,
    visibility_requested: AtomicBool,
    presentation_visible: AtomicBool,
    expect_authenticated: bool,
}

#[tauri::command]
fn get_snapshot(state: State<'_, ShellState>) -> ControlSnapshot {
    state.snapshot_requested.store(true, Ordering::SeqCst);
    state.controller.snapshot()
}

fn window_visible(app: &tauri::AppHandle) -> bool {
    app.get_webview_window("main").is_some_and(|window| {
        window.is_visible().unwrap_or(false) && !window.is_minimized().unwrap_or(true)
    })
}

fn publish_visibility(app: &tauri::AppHandle) {
    let Some(state) = app.try_state::<ShellState>() else {
        return;
    };
    let visible = window_visible(app);
    if state.presentation_visible.swap(visible, Ordering::SeqCst) != visible {
        let _ = app.emit("echocord://visibility", visible);
    }
}

#[tauri::command]
fn get_visibility(app: tauri::AppHandle, state: State<'_, ShellState>) -> bool {
    state.visibility_requested.store(true, Ordering::SeqCst);
    window_visible(&app)
}

#[tauri::command]
async fn dispatch(command: ControlCommand, state: State<'_, ShellState>) -> Result<(), AppError> {
    state.controller.dispatch(command).await
}

#[tauri::command]
async fn configure_bot(token: String, state: State<'_, ShellState>) -> Result<(), AppError> {
    state.controller.configure_token(token).await
}

fn show_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        publish_visibility(app);
    }
}

fn request_quit(app: &tauri::AppHandle) {
    window_state::persist(app);
    let state = app.state::<ShellState>();
    if state.exiting.swap(true, Ordering::SeqCst) {
        return;
    }
    let controller = state.controller.clone();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        // Process exit also closes the parent-owned kill-on-close job.
        let _ = tokio::time::timeout(Duration::from_secs(3), controller.shutdown()).await;
        app.exit(0);
    });
}

fn tray_action(app: &tauri::AppHandle, action: &str) {
    match action {
        "open" => show_window(app),
        "quit" => request_quit(app),
        "mute" | "disconnect" => {
            let controller = app.state::<ShellState>().controller.clone();
            let command = if action == "mute" {
                let snapshot = controller.snapshot();
                ControlCommand::SetAllDestinationsMuted {
                    muted: !(!snapshot.destinations.is_empty()
                        && snapshot.destinations.iter().all(|d| d.muted)),
                }
            } else {
                ControlCommand::DisconnectAllDestinations
            };
            tauri::async_runtime::spawn(async move {
                let _ = controller.dispatch(command).await;
            });
        }
        _ => {}
    }
}

fn parse_options() -> Result<(ControllerConfig, Option<PathBuf>), &'static str> {
    let mut data_dir = None;
    let mut token_file = None;
    let mut settings_file = None;
    let mut smoke_report = None;
    let mut args = std::env::args_os().skip(1);
    while let Some(argument) = args.next() {
        let destination = match argument.to_str() {
            Some("--data-dir") => &mut data_dir,
            Some("--token-file") => &mut token_file,
            Some("--settings-file") => &mut settings_file,
            Some("--smoke-report") => &mut smoke_report,
            _ => return Err("Unknown application option"),
        };
        *destination = Some(PathBuf::from(args.next().ok_or("Missing option value")?));
    }
    let data_dir = match data_dir {
        Some(path) => path,
        None => echocord_core::storage::default_data_dir()
            .map_err(|_| "Cannot locate application data directory")?,
    };
    Ok((
        ControllerConfig {
            executable: std::env::current_exe()
                .map_err(|_| "Cannot locate application executable")?,
            token_file: Some(token_file.unwrap_or_else(|| data_dir.join("discord-token"))),
            settings_file: Some(settings_file.unwrap_or_else(|| data_dir.join("preferences.json"))),
        },
        smoke_report,
    ))
}

fn main() {
    #[cfg(windows)]
    if std::env::args().nth(1).as_deref() == Some("--audio-worker") {
        let result = std::env::args()
            .nth(2)
            .and_then(|s| s.parse::<usize>().ok())
            .ok_or_else(|| {
                AppError::new(
                    echocord_core::model::ErrorCode::Internal,
                    "Invalid audio worker handle.",
                )
            })
            .and_then(echocord_core::mixing::run_worker);
        std::process::exit(if result.is_ok() { 0 } else { 1 });
    }

    #[cfg(windows)]
    if std::env::args().nth(1).as_deref() == Some("--playback-worker") {
        let result = std::env::args()
            .nth(2)
            .and_then(|raw| raw.parse::<usize>().ok())
            .ok_or_else(|| {
                AppError::new(
                    echocord_core::model::ErrorCode::Internal,
                    "Invalid playback worker handle.",
                )
            })
            .and_then(echocord_core::playback::run_worker);
        std::process::exit(if result.is_ok() { 0 } else { 1 });
    }

    // The engine must never initialize a second WebView or tray icon.
    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("--engine-worker")) {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("Cannot initialize engine runtime");
        if runtime
            .block_on(echocord_core::engine::run_worker())
            .is_err()
        {
            eprintln!("Echocord engine stopped with an error");
            std::process::exit(1);
        }
        return;
    }
    let (configuration, smoke_report) = parse_options().unwrap_or_else(|message| {
        eprintln!("{message}");
        std::process::exit(2);
    });
    let window_preferences = window_state::preference_path(
        configuration.settings_file.as_deref(),
        configuration.token_file.as_deref(),
    );
    let expect_authenticated = configuration
        .token_file
        .as_ref()
        .is_some_and(|p| p.is_file());
    let controller = tauri::async_runtime::block_on(async { Controller::prepare(configuration) });
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(
            |app, _arguments, _directory| {
                show_window(app);
            },
        ))
        .manage(ShellState {
            controller,
            exiting: AtomicBool::new(false),
            snapshot_requested: AtomicBool::new(false),
            visibility_requested: AtomicBool::new(false),
            presentation_visible: AtomicBool::new(true),
            expect_authenticated,
        })
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            get_visibility,
            dispatch,
            configure_bot
        ])
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                window_state::remember(window);
                window_state::persist(window.app_handle());
                if !window
                    .app_handle()
                    .state::<ShellState>()
                    .controller
                    .snapshot()
                    .settings
                    .keep_in_tray
                {
                    request_quit(window.app_handle());
                    return;
                }
                let _ = window.hide();
                publish_visibility(window.app_handle());
            } else if matches!(event, WindowEvent::Resized(_) | WindowEvent::Focused(_)) {
                // Losing focus alone does not hide a visible window. Native
                // minimize/restore changes are observed through window state.
                if matches!(event, WindowEvent::Resized(_)) {
                    window_state::remember(window);
                }
                publish_visibility(window.app_handle());
            }
        })
        .setup(move |app| {
            window_state::install(app, window_preferences.clone());
            app.state::<ShellState>().controller.activate();
            let open = MenuItem::with_id(app, "open", "Open Echocord", true, None::<&str>)?;
            let mute = CheckMenuItem::with_id(
                app,
                "mute",
                "Mute all outputs",
                false,
                false,
                None::<&str>,
            )?;
            let disconnect =
                MenuItem::with_id(app, "disconnect", "Stop all outputs", false, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &mute, &disconnect, &quit])?;
            let tray = TrayIconBuilder::with_id("echocord")
                .icon(
                    app.default_window_icon()
                        .ok_or("Missing application icon")?
                        .clone(),
                )
                .tooltip("Echocord — disconnected")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| tray_action(app, event.id.as_ref()))
                .on_tray_icon_event(|tray, event| {
                    if matches!(
                        event,
                        TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        }
                    ) {
                        show_window(tray.app_handle());
                    }
                })
                .build(app)?;
            let mut snapshots = app.state::<ShellState>().controller.subscribe();
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let mut last_tray = None;
                let mut last_control: Option<ControlSnapshot> = None;
                loop {
                    let snapshot = snapshots.borrow_and_update().clone();
                    let has_outputs = !snapshot.destinations.is_empty();
                    let all_muted = has_outputs && snapshot.destinations.iter().all(|d| d.muted);
                    let active = snapshot
                        .destinations
                        .iter()
                        .any(|d| d.connection.locks_route());
                    let playing = snapshot
                        .destinations
                        .iter()
                        .any(|d| d.connection == Connection::Connected);
                    let fault = snapshot.destinations.iter().any(|d| {
                        matches!(
                            d.connection,
                            Connection::Error { .. } | Connection::CleanupIncomplete { .. }
                        )
                    });
                    let label = if playing && fault {
                        "playing, needs attention"
                    } else if fault {
                        "needs attention"
                    } else if playing && all_muted {
                        "playing, muted"
                    } else if playing {
                        "playing"
                    } else if active {
                        "changing outputs"
                    } else {
                        "idle"
                    };
                    let tray_state = (all_muted, active, has_outputs, label);
                    if last_tray != Some(tray_state) {
                        let _ = mute.set_checked(tray_state.0);
                        let _ = disconnect.set_enabled(tray_state.1);
                        let _ = mute.set_enabled(tray_state.2);
                        let _ = tray.set_tooltip(Some(format!("Echocord — {label}")));
                        last_tray = Some(tray_state);
                    }
                    let _ = handle.emit("echocord://telemetry", &snapshot.telemetry);
                    if last_control
                        .as_ref()
                        .is_none_or(|last| !snapshot.same_control_state(last))
                    {
                        let _ = handle.emit("echocord://snapshot", &snapshot);
                        last_control = Some(snapshot);
                    }
                    if snapshots.changed().await.is_err() {
                        break;
                    }
                }
            });
            if let Some(report) = smoke_report.clone() {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let result = native_smoke(&handle).await;
                    let text = if result {
                        "native-ui-ipc-tray-lifecycle: passed\n"
                    } else {
                        "native-ui-ipc-tray-lifecycle: failed\n"
                    };
                    let _ = std::fs::write(report, text);
                    request_quit(&handle);
                });
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("Cannot initialize Echocord window");
    app.run(|app, event| {
        if let RunEvent::ExitRequested { api, .. } = event
            && !app.state::<ShellState>().exiting.load(Ordering::SeqCst)
        {
            api.prevent_exit();
            request_quit(app);
        }
    });
}

async fn native_smoke(app: &tauri::AppHandle) -> bool {
    // Explicit test mode: no Connect command or voice transmission. Saved recording
    // inputs may restore capture through the normal startup policy.
    for _ in 0..100 {
        let state = app.state::<ShellState>();
        if state.snapshot_requested.load(Ordering::SeqCst)
            && state.visibility_requested.load(Ordering::SeqCst)
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    if !app
        .state::<ShellState>()
        .snapshot_requested
        .load(Ordering::SeqCst)
        || !app
            .state::<ShellState>()
            .visibility_requested
            .load(Ordering::SeqCst)
    {
        return false;
    }
    let mut local_sources_ready = false;
    for _ in 0..70 {
        let snapshot = app.state::<ShellState>().controller.snapshot();
        match (&snapshot.source_discovery, &snapshot.playback_discovery) {
            (Discovery::Ready, Discovery::Ready) => {
                local_sources_ready = true;
                break;
            }
            (Discovery::Error { .. }, _) | (_, Discovery::Error { .. }) => return false,
            _ => tokio::time::sleep(Duration::from_millis(100)).await,
        }
    }
    if !local_sources_ready {
        return false;
    }
    if app.state::<ShellState>().expect_authenticated {
        let mut authenticated = false;
        for _ in 0..150 {
            let snapshot = app.state::<ShellState>().controller.snapshot();
            if snapshot.bot.status == BotStatus::Ready && snapshot.discovery == Discovery::Ready {
                authenticated = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        if !authenticated {
            return false;
        }
    }
    let Some(window) = app.get_webview_window("main") else {
        return false;
    };
    if app.tray_by_id("echocord").is_none() || window.is_visible().ok() != Some(true) {
        return false;
    }
    if window.is_decorated().ok() != Some(false) || window.is_maximizable().ok() != Some(false) {
        return false;
    }
    let (Ok(original), Ok(scale)) = (window.inner_size(), window.scale_factor()) else {
        return false;
    };
    let original = original.to_logical::<u32>(scale);
    let geometry_ok = async {
        for (width, height) in [(420, 420), (760, 440)] {
            if window
                .set_size(tauri::LogicalSize::new(width, height))
                .is_err()
                || !wait_window_size(&window, width, height).await
            {
                return false;
            }
        }
        true
    }
    .await;
    // Restore the owner's size even if a geometry assertion fails.
    let restored = window.set_size(original).is_ok()
        && wait_window_size(&window, original.width, original.height).await;
    if !restored {
        window_state::protect_saved_size(app);
    }
    if !geometry_ok || !restored {
        return false;
    }
    let hidden_event = std::sync::Arc::new(AtomicBool::new(false));
    let visible_event = std::sync::Arc::new(AtomicBool::new(false));
    let hidden = hidden_event.clone();
    let visible = visible_event.clone();
    let listener = app.listen("echocord://visibility", move |event| {
        if event.payload() == "false" {
            hidden.store(true, Ordering::SeqCst);
        } else if event.payload() == "true" {
            visible.store(true, Ordering::SeqCst);
        }
    });
    let result = async {
        if window.minimize().is_err() || !wait_visibility(app, false).await {
            return false;
        }
        if !hidden_event.swap(false, Ordering::SeqCst) {
            return false;
        }
        show_window(app);
        if !wait_visibility(app, true).await || !visible_event.swap(false, Ordering::SeqCst) {
            return false;
        }
        let keep_in_tray = app
            .state::<ShellState>()
            .controller
            .snapshot()
            .settings
            .keep_in_tray;
        let hide_result = if keep_in_tray {
            window.close()
        } else {
            // Closing would quit normally; fresh-profile smoke covers close-to-tray.
            let result = window.hide();
            publish_visibility(app);
            result
        };
        if hide_result.is_err() || !wait_visibility(app, false).await {
            return false;
        }
        if window.is_visible().ok() != Some(false) || !hidden_event.load(Ordering::SeqCst) {
            return false;
        }
        show_window(app);
        wait_visibility(app, true).await && visible_event.load(Ordering::SeqCst)
    }
    .await;
    app.unlisten(listener);
    result
}

async fn wait_window_size(window: &tauri::WebviewWindow, width: u32, height: u32) -> bool {
    for _ in 0..40 {
        if let (Ok(size), Ok(scale)) = (window.inner_size(), window.scale_factor()) {
            let logical = size.to_logical::<u32>(scale);
            if logical.width.abs_diff(width) <= 1 && logical.height.abs_diff(height) <= 1 {
                return true;
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}

async fn wait_visibility(app: &tauri::AppHandle, expected: bool) -> bool {
    for _ in 0..40 {
        if window_visible(app) == expected
            && app
                .state::<ShellState>()
                .presentation_visible
                .load(Ordering::SeqCst)
                == expected
        {
            // Event handlers run synchronously with emit; let presentation settle.
            tokio::time::sleep(Duration::from_millis(100)).await;
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}
