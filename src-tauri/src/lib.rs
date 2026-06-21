use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

// ─── State ───────────────────────────────────────────────────────────────────

struct BackendProcess {
    child:  Child,
    port:   u16,
}

// Holds the port once the backend is confirmed healthy — readable by React via command.
struct ReadyPort(Option<u16>);

// ─── Commands ────────────────────────────────────────────────────────────────

/// React calls this on mount to check if the backend is already ready.
/// Returns the port if healthy, null otherwise — handles the race where the
/// backend-ready event fires before React has registered its listener.
#[tauri::command]
fn get_backend_port(state: tauri::State<Arc<Mutex<ReadyPort>>>) -> Option<u16> {
    state.lock().ok()?.0
}

// ─── Native OS commands ───────────────────────────────────────────────────────

#[tauri::command]
async fn pick_file(
    app: AppHandle,
    title: Option<String>,
) -> Result<Option<String>, String> {
    let mut builder = app.dialog().file();
    if let Some(t) = title {
        builder = builder.set_title(t);
    }
    match builder.blocking_pick_file() {
        Some(p) => Ok(Some(p.to_string())),
        None    => Ok(None),
    }
}

#[tauri::command]
async fn pick_save_path(
    app: AppHandle,
    default_name: Option<String>,
) -> Result<Option<String>, String> {
    let mut builder = app.dialog().file();
    if let Some(name) = default_name {
        builder = builder.set_file_name(name);
    }
    match builder.blocking_save_file() {
        Some(p) => Ok(Some(p.to_string())),
        None    => Ok(None),
    }
}

#[tauri::command]
fn read_text_file(path: String) -> Result<String, String> {
    std::fs::read_to_string(&path).map_err(|e| e.to_string())
}

#[tauri::command]
fn write_text_file(path: String, content: String) -> Result<(), String> {
    std::fs::write(&path, content).map_err(|e| e.to_string())
}

#[tauri::command]
async fn open_path(app: AppHandle, path: String) -> Result<(), String> {
    app.opener().open_path(path, None::<&str>).map_err(|e| e.to_string())
}

// ─── Sidecar lifecycle ────────────────────────────────────────────────────────

fn find_free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("could not find a free port")
        .local_addr()
        .unwrap()
        .port()
}

fn backend_exe_path(app: &tauri::App) -> PathBuf {
    // Production: backend/ is bundled as a Tauri resource next to the executable.
    if let Ok(res_dir) = app.path().resource_dir() {
        let candidate = res_dir.join("backend").join("backend.exe");
        if candidate.exists() {
            return candidate;
        }
    }
    // Also check next to the executable (some Tauri installer layouts)
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            let candidate = exe_dir.join("backend").join("backend.exe");
            if candidate.exists() {
                return candidate;
            }
        }
    }
    // Dev mode: use venv Python if it exists, else system Python
    PathBuf::from("backend/main.py")
}

fn spawn_backend(exe: PathBuf, port: u16, db_path: &str) -> Result<Child, String> {
    let port_str = port.to_string();

    let child = if exe.extension().map(|e| e == "py").unwrap_or(false) {
        // Dev mode: use venv Python if it exists, else fall back to system Python
        let venv_python = PathBuf::from("backend/.venv/Scripts/python.exe");
        let python = if venv_python.exists() { venv_python } else { PathBuf::from("python") };
        Command::new(python)
            .arg(exe)
            .arg("--port").arg(&port_str)
            .arg("--db-path").arg(db_path)
            .spawn()
    } else {
        Command::new(exe)
            .arg("--port").arg(&port_str)
            .arg("--db-path").arg(db_path)
            .spawn()
    };

    child.map_err(|e| format!("Failed to start backend: {e}"))
}

fn poll_health(port: u16, timeout_secs: u64) -> bool {
    let url      = format!("http://127.0.0.1:{}/health", port);
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);

    while Instant::now() < deadline {
        if let Ok(resp) = reqwest::blocking::get(&url) {
            if resp.status().is_success() {
                return true;
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    false
}

// ─── App entry point ──────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .setup(|app| {
            let app_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&app_dir)?;
            let db_path = app_dir.to_string_lossy().to_string();

            // If ARTHDESK_BACKEND_PORT is set, skip spawning and use that port directly.
            // Useful for development: run Python manually in one terminal, Tauri in another.
            let manual_port = std::env::var("ARTHDESK_BACKEND_PORT")
                .ok()
                .and_then(|v| v.parse::<u16>().ok());

            let port   = manual_port.unwrap_or_else(find_free_port);
            let exe    = backend_exe_path(app);
            let handle = app.handle().clone();
            let process_state: Arc<Mutex<Option<BackendProcess>>> = Arc::new(Mutex::new(None));
            let state_for_thread = Arc::clone(&process_state);
            let state_for_exit   = Arc::clone(&process_state);

            app.manage(process_state);

            let ready_port: Arc<Mutex<ReadyPort>> = Arc::new(Mutex::new(ReadyPort(None)));
            app.manage(Arc::clone(&ready_port));

            std::thread::spawn(move || {
                if manual_port.is_none() {
                    // Spawn backend
                    let child = match spawn_backend(exe, port, &db_path) {
                        Ok(c)  => c,
                        Err(e) => {
                            eprintln!("[backend] spawn failed: {e}");
                            let _ = handle.emit("backend-crashed", e);
                            return;
                        }
                    };
                    let mut guard = state_for_thread.lock().unwrap();
                    *guard = Some(BackendProcess { child, port });
                } else {
                    eprintln!("[backend] using manually started backend on port {port}");
                }

                // Poll /health until ready (works for both spawned and manual)
                if !poll_health(port, 30) {
                    eprintln!("[backend] health check timed out after 30s");
                    let _ = handle.emit("backend-crashed", "Backend health check timed out");
                    return;
                }

                // Store port so React can query it if it missed the event
                if let Ok(mut rp) = ready_port.lock() {
                    rp.0 = Some(port);
                }
                let _ = handle.emit("backend-ready", port);

                // Only watch for exit if we spawned the process
                loop {
                    std::thread::sleep(Duration::from_millis(500));
                    let mut guard = state_for_exit.lock().unwrap();
                    if let Some(proc) = guard.as_mut() {
                        match proc.child.try_wait() {
                            Ok(Some(_)) => {
                                *guard = None;
                                drop(guard);
                                eprintln!("[backend] process exited unexpectedly");
                                let _ = handle.emit("backend-crashed", "Backend process exited");
                                break;
                            }
                            Ok(None) => {}
                            Err(e) => {
                                eprintln!("[backend] wait error: {e}");
                                break;
                            }
                        }
                    } else {
                        break;
                    }
                }
            });

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                // Kill the backend process on window close
                if let Some(state) = window.app_handle().try_state::<Arc<Mutex<Option<BackendProcess>>>>() {
                    let mut guard = state.lock().unwrap();
                    if let Some(mut proc) = guard.take() {
                        let _ = proc.child.kill();
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_backend_port,
            pick_file,
            pick_save_path,
            read_text_file,
            write_text_file,
            open_path,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
