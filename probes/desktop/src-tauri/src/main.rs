use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use tauri::{Manager, RunEvent, State, WebviewWindow, WindowEvent};

const ACKNOWLEDGEMENT: &str = "echocord-desktop-ready";

#[derive(Default)]
struct Proof {
    ready: AtomicBool,
    round_trip: AtomicBool,
    destroyed: AtomicBool,
}

#[tauri::command]
fn frontend_ready(
    window: WebviewWindow,
    proof: State<'_, Arc<Proof>>,
) -> Result<&'static str, String> {
    let visible = window.is_visible().map_err(|error| error.to_string())?;
    let size = window.inner_size().map_err(|error| error.to_string())?;
    if window.label() != "probe" || !visible || size.width == 0 || size.height == 0 {
        return Err("Expected a visible, nonempty probe window".into());
    }
    proof.ready.store(true, Ordering::SeqCst);
    println!("EC-002 frontend_ready ipc=true native_visible=true native_size_nonzero=true");
    Ok(ACKNOWLEDGEMENT)
}

#[tauri::command]
fn frontend_complete(
    window: WebviewWindow,
    proof: State<'_, Arc<Proof>>,
    acknowledgement: String,
) -> Result<(), String> {
    if window.label() != "probe"
        || !proof.ready.load(Ordering::SeqCst)
        || acknowledgement != ACKNOWLEDGEMENT
    {
        return Err("Frontend did not complete the readiness round trip".into());
    }
    proof.round_trip.store(true, Ordering::SeqCst);
    println!("EC-002 frontend_round_trip ipc=true");
    window.close().map_err(|error| error.to_string())
}

fn main() {
    // Keep the console subsystem so WSL can collect telemetry without a plugin.
    println!("EC-002 runtime_start");
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(25));
        eprintln!("EC-002 watchdog_timeout");
        std::process::exit(3);
    });

    let proof = Arc::new(Proof::default());
    let app = tauri::Builder::default()
        .manage(Arc::clone(&proof))
        .invoke_handler(tauri::generate_handler![frontend_ready, frontend_complete])
        .setup(|app| {
            let window = app
                .get_webview_window("probe")
                .ok_or("Probe window was not created")?;
            if !window.is_visible()? {
                return Err("Probe window was not visible".into());
            }
            println!("EC-002 window_created native_visible=true");
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("Could not initialize the Windows WebView shell");

    let event_proof = Arc::clone(&proof);
    let exit_code = app.run_return(move |_handle, event| match event {
        RunEvent::WindowEvent { label, event, .. } if label == "probe" => match event {
            WindowEvent::CloseRequested { .. } => println!("EC-002 window_close_requested"),
            WindowEvent::Destroyed => {
                event_proof.destroyed.store(true, Ordering::SeqCst);
                println!("EC-002 window_destroyed");
            }
            _ => {}
        },
        RunEvent::Exit => println!("EC-002 runtime_exit"),
        _ => {}
    });

    let complete = proof.ready.load(Ordering::SeqCst)
        && proof.round_trip.load(Ordering::SeqCst)
        && proof.destroyed.load(Ordering::SeqCst);
    if exit_code == 0 && complete {
        println!("EC-002 lifecycle_complete success=true");
    } else {
        eprintln!("EC-002 lifecycle_incomplete");
        std::process::exit(if exit_code == 0 { 2 } else { exit_code });
    }
}
