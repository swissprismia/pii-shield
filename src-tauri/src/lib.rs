mod clipboard;
mod sidecar;
mod window;

use clipboard::ClipboardWatcher;
use parking_lot::Mutex;
use rdev::{listen, Button, Event, EventType, Key};
use sidecar::PresidioSidecar;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{
    image::Image,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, RunEvent, State,
};

/// Check if the active window is a browser
fn is_browser_window(app_name: &str) -> bool {
    let app_name_lower = app_name.to_lowercase();
    app_name_lower.contains("chrome")
        || app_name_lower.contains("firefox")
        || app_name_lower.contains("edge")
        || app_name_lower.contains("safari")
        || app_name_lower.contains("brave")
        || app_name_lower.contains("opera")
        || app_name_lower.contains("vivaldi")
}

/// Shared state for keyboard hook (needs to be 'static for rdev callback)
static CTRL_PRESSED: AtomicBool = AtomicBool::new(false);

/// Application state shared across the Tauri app
pub struct AppState {
    clipboard_watcher: Mutex<Option<ClipboardWatcher>>,
    sidecar: Arc<tokio::sync::Mutex<PresidioSidecar>>,
    last_clipboard_hash: Mutex<u64>,
    clipboard_handled: Mutex<bool>,
    pending_anonymization: Arc<Mutex<Option<String>>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            clipboard_watcher: Mutex::new(None),
            sidecar: Arc::new(tokio::sync::Mutex::new(PresidioSidecar::new())),
            last_clipboard_hash: Mutex::new(0),
            clipboard_handled: Mutex::new(false),
            pending_anonymization: Arc::new(Mutex::new(None)),
        }
    }
}

/// Helper function to anonymize clipboard if in browser with pending anonymization
fn try_anonymize_for_browser(
    pending_anonymization: &Arc<Mutex<Option<String>>>,
    app_handle: &AppHandle,
    trigger: &str,
) {
    // Check if we're in a browser
    if let Some(window_info) = window::get_active_window() {
        let is_browser = window_info
            .app_name
            .as_ref()
            .map(|name| is_browser_window(name))
            .unwrap_or(false);

        if is_browser {
            // Check if we have pending anonymization
            let mut pending = pending_anonymization.lock();
            if let Some(anonymized_text) = pending.take() {
                let app_name = window_info.app_name.as_deref().unwrap_or("browser");
                log::info!(
                    "{} in browser detected! Auto-anonymizing for: {}",
                    trigger,
                    app_name
                );

                // Replace clipboard with anonymized text BEFORE paste completes
                if let Err(e) = clipboard::set_clipboard_text(&anonymized_text) {
                    log::error!("Failed to auto-anonymize clipboard: {}", e);
                    // Put it back for retry
                    *pending = Some(anonymized_text);
                } else {
                    log::info!("Clipboard replaced successfully before paste!");

                    // Notify frontend
                    let _ = app_handle.emit(
                        "auto-anonymized",
                        serde_json::json!({
                            "app_name": app_name,
                            "trigger": trigger
                        }),
                    );
                }
            }
        }
    }
}

/// Start the global input listener to intercept paste actions (Ctrl+V, Ctrl+X, right-click)
fn start_input_listener(pending_anonymization: Arc<Mutex<Option<String>>>, app_handle: AppHandle) {
    std::thread::spawn(move || {
        log::info!("Starting global input listener for paste interception...");

        let callback = move |event: Event| {
            match event.event_type {
                // Track Ctrl key state
                EventType::KeyPress(Key::ControlLeft) | EventType::KeyPress(Key::ControlRight) => {
                    CTRL_PRESSED.store(true, Ordering::SeqCst);
                }
                EventType::KeyRelease(Key::ControlLeft)
                | EventType::KeyRelease(Key::ControlRight) => {
                    CTRL_PRESSED.store(false, Ordering::SeqCst);
                }

                // Ctrl+V - Paste operation
                EventType::KeyPress(Key::KeyV) => {
                    if CTRL_PRESSED.load(Ordering::SeqCst) {
                        log::debug!("Ctrl+V detected!");
                        try_anonymize_for_browser(&pending_anonymization, &app_handle, "Ctrl+V");
                    }
                }

                // Ctrl+X - Cut operation (anonymize in case they paste later)
                EventType::KeyPress(Key::KeyX) => {
                    if CTRL_PRESSED.load(Ordering::SeqCst) {
                        log::debug!("Ctrl+X detected - clipboard will be re-analyzed on change");
                        // Note: The clipboard polling will detect the new content and analyze it
                        // We don't need to do anything special here, but we log for debugging
                    }
                }

                // Right-click - User might be opening context menu for paste
                // Pre-emptively replace clipboard when right-clicking in a browser
                EventType::ButtonPress(Button::Right) => {
                    log::debug!("Right-click detected!");
                    try_anonymize_for_browser(
                        &pending_anonymization,
                        &app_handle,
                        "Right-click menu",
                    );
                }

                _ => {}
            }
        };

        if let Err(error) = listen(callback) {
            log::error!("Input listener error: {:?}", error);
        }
    });
}

/// Start clipboard monitoring
#[tauri::command]
async fn start_monitoring(app_handle: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    log::info!("Starting clipboard monitoring...");

    // Start the sidecar process
    {
        let mut sidecar = state.sidecar.lock().await;
        sidecar
            .start(&app_handle)
            .await
            .map_err(|e| e.to_string())?;
    }

    // Start the global input listener for paste interception (Ctrl+V, right-click, etc.)
    let pending_for_input = state.pending_anonymization.clone();
    start_input_listener(pending_for_input, app_handle.clone());

    // Start clipboard watcher in a background task
    let sidecar = state.sidecar.clone();
    let app_handle_clone = app_handle.clone();

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(500));

        loop {
            interval.tick().await;

            // Get current clipboard content
            if let Some(text) = clipboard::get_clipboard_text() {
                let hash = clipboard::hash_text(&text);

                // Check if we should process this clipboard content
                let should_process = {
                    let state = app_handle_clone.state::<AppState>();
                    let mut last_hash = state.last_clipboard_hash.lock();
                    let mut handled = state.clipboard_handled.lock();

                    if hash != *last_hash {
                        *last_hash = hash;
                        *handled = false;
                        true
                    } else if *handled {
                        false
                    } else {
                        false // Already saw this, waiting for user action
                    }
                };

                if should_process && !text.trim().is_empty() {
                    log::debug!("New clipboard content detected, analyzing...");

                    // Analyze with Presidio
                    let result = {
                        let sidecar = sidecar.lock().await;
                        sidecar.analyze(&text).await
                    };

                    match result {
                        Ok(analysis) => {
                            if !analysis.entities.is_empty() {
                                log::info!("Detected {} PII entities", analysis.entities.len());

                                // Store anonymized text for auto-replacement
                                {
                                    let state = app_handle_clone.state::<AppState>();
                                    let mut pending = state.pending_anonymization.lock();
                                    *pending = Some(analysis.anonymized_text.clone());
                                }

                                // Emit event to frontend
                                let _ = app_handle_clone.emit("pii-detected", &analysis);
                            } else {
                                // No PII found, just update stats
                                let _ = app_handle_clone.emit("clipboard-scanned", ());
                            }
                        }
                        Err(e) => {
                            log::error!("Failed to analyze clipboard: {}", e);
                            let _ = app_handle_clone.emit(
                                "sidecar-status",
                                serde_json::json!({
                                    "status": "error",
                                    "message": e.to_string()
                                }),
                            );
                        }
                    }
                }
            }

            // Update active window info periodically and auto-anonymize in browsers
            if let Some(window_info) = window::get_active_window() {
                // Check if we're in a browser and have pending anonymization
                let is_browser = if let Some(ref app_name) = window_info.app_name {
                    let result = is_browser_window(app_name);
                    if result {
                        log::debug!("Browser detected: {}", app_name);
                    }
                    result
                } else {
                    false
                };

                if is_browser {
                    let state = app_handle_clone.state::<AppState>();
                    let mut pending = state.pending_anonymization.lock();

                    if let Some(anonymized_text) = pending.take() {
                        let app_name = window_info.app_name.as_deref().unwrap_or("browser");
                        log::info!("🔄 Auto-anonymizing clipboard for browser: {}", app_name);
                        log::info!("📋 Replacing clipboard with: {}", anonymized_text);

                        // Replace clipboard with anonymized text
                        if let Err(e) = clipboard::set_clipboard_text(&anonymized_text) {
                            log::error!("Failed to auto-anonymize clipboard: {}", e);
                        } else {
                            log::info!("✅ Clipboard replaced successfully");

                            // Mark as handled
                            let mut handled = state.clipboard_handled.lock();
                            *handled = true;

                            // Notify frontend
                            let _ = app_handle_clone.emit(
                                "auto-anonymized",
                                serde_json::json!({
                                    "app_name": app_name
                                }),
                            );
                        }
                    } else {
                        log::debug!("No pending anonymization for browser");
                    }
                }

                let _ = app_handle_clone.emit("active-window-changed", &window_info);
            }
        }
    });

    Ok(())
}

/// Mark current clipboard content as handled (user clicked anonymize or ignore)
#[tauri::command]
async fn mark_clipboard_handled(state: State<'_, AppState>) -> Result<(), String> {
    let mut handled = state.clipboard_handled.lock();
    *handled = true;
    Ok(())
}

/// Get sidecar status
#[tauri::command]
async fn get_sidecar_status(state: State<'_, AppState>) -> Result<bool, String> {
    let sidecar = state.sidecar.lock().await;
    Ok(sidecar.is_running())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_shell::init())
        .manage(AppState::new())
        .setup(|app| {
            // Build system tray
            let quit = MenuItem::with_id(app, "quit", "Quit PII Shield", true, None::<&str>)?;
            let show = MenuItem::with_id(app, "show", "Show Window", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;

            // Create a simple 32x32 RGBA icon (shield icon approximation)
            let icon_data: Vec<u8> = vec![0x63, 0x66, 0xf1, 0xff].repeat(32 * 32);

            let _tray = TrayIconBuilder::new()
                .icon(Image::new_owned(icon_data, 32, 32))
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "quit" => {
                        app.exit(0);
                    }
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                })
                .build(app)?;

            log::info!("PII Shield initialized");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            start_monitoring,
            mark_clipboard_handled,
            get_sidecar_status,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            if let RunEvent::ExitRequested { .. } = event {
                // Clean up sidecar on exit
                let state = app_handle.state::<AppState>();
                if let Ok(mut sidecar) = state.sidecar.try_lock() {
                    sidecar.stop();
                };
            }
        });
}
