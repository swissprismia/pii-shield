mod clipboard;
mod config;
mod sidecar;
mod window;

use clipboard::ClipboardWatcher;
use parking_lot::Mutex;
use rdev::{listen, Button, Event, EventType, Key};
use sidecar::{PresidioSidecar, TokenizationResult};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{
    image::Image,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, RunEvent, State,
};

/// Check if we should auto-anonymize for this window based on configured keywords
fn should_auto_anonymize(window_info: &window::WindowInfo, keywords: &[String]) -> bool {
    // Check app_name first if available
    if let Some(ref app_name) = window_info.app_name {
        let app_name_lower = app_name.to_lowercase();
        if keywords.iter().any(|keyword| app_name_lower.contains(keyword)) {
            return true;
        }
    }

    // Fallback to checking window title (important for Windows)
    let title_lower = window_info.title.to_lowercase();
    keywords.iter().any(|keyword| title_lower.contains(keyword))
}

/// Shared state for keyboard hook (needs to be 'static for rdev callback)
static CTRL_PRESSED: AtomicBool = AtomicBool::new(false);

/// Token vault for storing PII token mappings
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct TokenVault {
    /// Maps token IDs to original values (e.g., "FirstName1" -> "John")
    pub token_map: HashMap<String, String>,
    /// The original text before tokenization
    pub original_text: String,
    /// The tokenized text
    pub tokenized_text: String,
    /// Timestamp when this vault was created
    pub created_at: u64,
}

impl TokenVault {
    pub fn new() -> Self {
        Self {
            token_map: HashMap::new(),
            original_text: String::new(),
            tokenized_text: String::new(),
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
    }

    pub fn from_tokenization(result: &TokenizationResult) -> Self {
        Self {
            token_map: result.token_map.clone(),
            original_text: result.original_text.clone(),
            tokenized_text: result.tokenized_text.clone(),
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.token_map.is_empty()
    }

    pub fn clear(&mut self) {
        self.token_map.clear();
        self.original_text.clear();
        self.tokenized_text.clear();
    }
}

/// Application state shared across the Tauri app
pub struct AppState {
    clipboard_watcher: Mutex<Option<ClipboardWatcher>>,
    sidecar: Arc<tokio::sync::Mutex<PresidioSidecar>>,
    last_clipboard_hash: Mutex<u64>,
    clipboard_handled: Mutex<bool>,
    pending_anonymization: Arc<Mutex<Option<String>>>,
    config: config::Config,
    /// Token vault for storing PII token mappings for de-tokenization
    token_vault: Arc<Mutex<TokenVault>>,
    /// Pending tokenization result (used when auto-tokenizing in browsers)
    pending_tokenization: Arc<Mutex<Option<TokenizationResult>>>,
}

impl AppState {
    pub fn new() -> Self {
        let config = config::Config::load();
        log::info!("Loaded config with {} keywords", config.get_all_keywords().len());

        Self {
            clipboard_watcher: Mutex::new(None),
            sidecar: Arc::new(tokio::sync::Mutex::new(PresidioSidecar::new())),
            last_clipboard_hash: Mutex::new(0),
            clipboard_handled: Mutex::new(false),
            pending_anonymization: Arc::new(Mutex::new(None)),
            config,
            token_vault: Arc::new(Mutex::new(TokenVault::new())),
            pending_tokenization: Arc::new(Mutex::new(None)),
        }
    }
}

/// Helper function to tokenize clipboard if in browser with pending tokenization
fn try_tokenize_for_browser(
    pending_tokenization: &Arc<Mutex<Option<TokenizationResult>>>,
    token_vault: &Arc<Mutex<TokenVault>>,
    app_handle: &AppHandle,
    trigger: &str,
) {
    // Check if we're in a browser or AI assistant app
    if let Some(window_info) = window::get_active_window() {
        let state = app_handle.state::<AppState>();
        let keywords = state.config.get_all_keywords();
        let should_anonymize = should_auto_anonymize(&window_info, &keywords);

        if should_anonymize {
            // Check if we have pending tokenization
            let mut pending = pending_tokenization.lock();
            if let Some(tokenization_result) = pending.take() {
                let app_name = window_info.app_name.as_deref().unwrap_or("app");
                log::info!(
                    "{} in browser detected! Auto-tokenizing for: {}",
                    trigger,
                    app_name
                );

                // Store the token mapping in the vault for later de-tokenization
                {
                    let mut vault = token_vault.lock();
                    *vault = TokenVault::from_tokenization(&tokenization_result);
                    log::info!("Token vault updated with {} tokens", vault.token_map.len());
                }

                // Replace clipboard with tokenized text BEFORE paste completes
                if let Err(e) = clipboard::set_clipboard_text(&tokenization_result.tokenized_text) {
                    log::error!("Failed to auto-tokenize clipboard: {}", e);
                    // Put it back for retry
                    *pending = Some(tokenization_result);
                } else {
                    log::info!("Clipboard replaced with tokenized text successfully!");

                    // Notify frontend
                    let _ = app_handle.emit(
                        "auto-tokenized",
                        serde_json::json!({
                            "app_name": app_name,
                            "trigger": trigger,
                            "tokenized_text": tokenization_result.tokenized_text,
                            "token_map": tokenization_result.token_map,
                        }),
                    );
                }
            }
        }
    }
}

/// Check if text contains tokens that we can de-tokenize
fn contains_known_tokens(text: &str, token_vault: &TokenVault) -> bool {
    if token_vault.is_empty() {
        return false;
    }

    // Check if any of our token IDs appear in the text
    for token_id in token_vault.token_map.keys() {
        let token_pattern = format!("[{}]", token_id);
        if text.contains(&token_pattern) {
            return true;
        }
    }
    false
}

/// De-tokenize text using the token vault
fn detokenize_with_vault(text: &str, token_vault: &TokenVault) -> String {
    if token_vault.is_empty() {
        return text.to_string();
    }

    let mut result = text.to_string();
    for (token_id, original_value) in &token_vault.token_map {
        let token_pattern = format!("[{}]", token_id);
        result = result.replace(&token_pattern, original_value);
    }
    result
}

/// Start the global input listener to intercept paste actions (Ctrl+V, Ctrl+X, right-click)
fn start_input_listener(
    pending_tokenization: Arc<Mutex<Option<TokenizationResult>>>,
    token_vault: Arc<Mutex<TokenVault>>,
    app_handle: AppHandle,
) {
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
                        try_tokenize_for_browser(
                            &pending_tokenization,
                            &token_vault,
                            &app_handle,
                            "Ctrl+V",
                        );
                    }
                }

                // Ctrl+X - Cut operation (tokenize in case they paste later)
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
                    try_tokenize_for_browser(
                        &pending_tokenization,
                        &token_vault,
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
    log::info!("Starting clipboard monitoring with tokenization support...");

    // Start the sidecar process
    {
        let mut sidecar = state.sidecar.lock().await;
        sidecar
            .start(&app_handle)
            .await
            .map_err(|e| e.to_string())?;
    }

    // Start the global input listener for paste interception (Ctrl+V, right-click, etc.)
    let pending_for_input = state.pending_tokenization.clone();
    let token_vault_for_input = state.token_vault.clone();
    start_input_listener(pending_for_input, token_vault_for_input, app_handle.clone());

    // Start clipboard watcher in a background task
    let sidecar = state.sidecar.clone();
    let token_vault = state.token_vault.clone();
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
                    // First, check if this text contains tokens we can de-tokenize
                    let should_detokenize = {
                        let vault = token_vault.lock();
                        contains_known_tokens(&text, &vault)
                    };

                    if should_detokenize {
                        // De-tokenize the text
                        let detokenized = {
                            let vault = token_vault.lock();
                            detokenize_with_vault(&text, &vault)
                        };

                        log::info!("Detected tokens in clipboard, de-tokenizing...");
                        log::debug!("Original: {}", text);
                        log::debug!("De-tokenized: {}", detokenized);

                        // Replace clipboard with de-tokenized text
                        if let Err(e) = clipboard::set_clipboard_text(&detokenized) {
                            log::error!("Failed to de-tokenize clipboard: {}", e);
                        } else {
                            // Mark as handled
                            {
                                let state = app_handle_clone.state::<AppState>();
                                let mut handled = state.clipboard_handled.lock();
                                *handled = true;
                            }

                            // Get the token map for the event
                            let token_map = {
                                let vault = token_vault.lock();
                                vault.token_map.clone()
                            };

                            // Notify frontend
                            let _ = app_handle_clone.emit(
                                "auto-detokenized",
                                serde_json::json!({
                                    "original_text": text,
                                    "detokenized_text": detokenized,
                                    "token_map": token_map,
                                }),
                            );
                        }
                    } else {
                        // No tokens found, analyze for PII and tokenize
                        log::debug!("New clipboard content detected, analyzing for PII...");

                        // Analyze and tokenize with Presidio
                        let result = {
                            let sidecar = sidecar.lock().await;
                            sidecar.analyze_and_tokenize(&text).await
                        };

                        match result {
                            Ok(tokenization) => {
                                if !tokenization.entities.is_empty() {
                                    log::info!(
                                        "Detected {} PII entities, tokenized with {} tokens",
                                        tokenization.entities.len(),
                                        tokenization.token_map.len()
                                    );

                                    // Store tokenization result for auto-replacement
                                    {
                                        let state = app_handle_clone.state::<AppState>();
                                        let mut pending = state.pending_tokenization.lock();
                                        *pending = Some(tokenization.clone());
                                    }

                                    // Emit event to frontend
                                    let _ = app_handle_clone.emit("pii-detected", &tokenization);
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
            }

            // Update active window info periodically and auto-tokenize in browsers/AI apps
            if let Some(window_info) = window::get_active_window() {
                // Check if we're in a browser or AI assistant app
                let keywords = {
                    let state = app_handle_clone.state::<AppState>();
                    state.config.get_all_keywords()
                };
                let should_anonymize = should_auto_anonymize(&window_info, &keywords);
                if should_anonymize {
                    log::debug!("Auto-anonymize target detected: {} ({})",
                        window_info.app_name.as_deref().unwrap_or("unknown"),
                        window_info.title);
                }

                if should_anonymize {
                    let state = app_handle_clone.state::<AppState>();
                    let mut pending = state.pending_tokenization.lock();

                    if let Some(tokenization_result) = pending.take() {
                        let app_name = window_info.app_name.as_deref().unwrap_or("browser");
                        log::info!("Auto-tokenizing clipboard for browser: {}", app_name);

                        // Store the token mapping in the vault for later de-tokenization
                        {
                            let mut vault = state.token_vault.lock();
                            *vault = TokenVault::from_tokenization(&tokenization_result);
                            log::info!("Token vault updated with {} tokens", vault.token_map.len());
                        }

                        // Replace clipboard with tokenized text
                        if let Err(e) = clipboard::set_clipboard_text(&tokenization_result.tokenized_text) {
                            log::error!("Failed to auto-tokenize clipboard: {}", e);
                        } else {
                            log::info!("Clipboard replaced with tokenized text");

                            // Mark as handled
                            let mut handled = state.clipboard_handled.lock();
                            *handled = true;

                            // Notify frontend
                            let _ = app_handle_clone.emit(
                                "auto-tokenized",
                                serde_json::json!({
                                    "app_name": app_name,
                                    "tokenized_text": tokenization_result.tokenized_text,
                                    "token_map": tokenization_result.token_map,
                                }),
                            );
                        }
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

/// Get the current token vault
#[tauri::command]
async fn get_token_vault(state: State<'_, AppState>) -> Result<TokenVault, String> {
    let vault = state.token_vault.lock();
    Ok(vault.clone())
}

/// Clear the token vault
#[tauri::command]
async fn clear_token_vault(state: State<'_, AppState>) -> Result<(), String> {
    let mut vault = state.token_vault.lock();
    vault.clear();
    log::info!("Token vault cleared");
    Ok(())
}

/// Manually tokenize text and copy to clipboard
#[tauri::command]
async fn tokenize_and_copy(
    text: String,
    state: State<'_, AppState>,
) -> Result<TokenizationResult, String> {
    let sidecar = state.sidecar.lock().await;
    let result = sidecar
        .analyze_and_tokenize(&text)
        .await
        .map_err(|e| e.to_string())?;

    // Store in vault
    {
        let mut vault = state.token_vault.lock();
        *vault = TokenVault::from_tokenization(&result);
    }

    // Copy tokenized text to clipboard
    clipboard::set_clipboard_text(&result.tokenized_text).map_err(|e| e.to_string())?;

    Ok(result)
}

/// Manually de-tokenize text using the current vault
#[tauri::command]
async fn detokenize_text(
    text: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let vault = state.token_vault.lock();
    let detokenized = detokenize_with_vault(&text, &vault);
    Ok(detokenized)
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
            get_token_vault,
            clear_token_vault,
            tokenize_and_copy,
            detokenize_text,
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
