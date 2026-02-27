mod clipboard;
mod config;
mod sidecar;
mod window;

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

// ── Tray Icon Rendering ───────────────────────────────────────────────────────

/// Render a 32×32 shield icon as raw RGBA bytes with the given fill colour.
fn render_shield_icon(r: u8, g: u8, b: u8) -> Vec<u8> {
    const SIZE: usize = 32;
    let mut pixels = vec![0u8; SIZE * SIZE * 4];
    for y in 0..SIZE {
        for x in 0..SIZE {
            if inside_shield(x, y, SIZE) {
                let idx = (y * SIZE + x) * 4;
                pixels[idx] = r;
                pixels[idx + 1] = g;
                pixels[idx + 2] = b;
                pixels[idx + 3] = 255;
            }
        }
    }
    pixels
}

/// Point-in-shield test (normalised coordinates).
fn inside_shield(x: usize, y: usize, size: usize) -> bool {
    let s = size as f32;
    let nx = x as f32 / s;
    let ny = y as f32 / s;
    // Reject top/bottom margin
    if !(0.06..=0.95).contains(&ny) {
        return false;
    }
    if ny <= 0.62 {
        // Rectangular body
        nx > 0.15 && nx < 0.85
    } else {
        // Tapered point
        let t = (ny - 0.62) / 0.33;
        let half = 0.35 * (1.0 - t);
        (nx - 0.5).abs() < half
    }
}

/// Update the system-tray icon. States:
/// - idle    → green   (monitoring, no PII)
/// - warning → orange  (PII detected)
/// - danger  → red     (secrets detected)
fn set_tray_icon(app: &AppHandle, r: u8, g: u8, b: u8) {
    if let Some(tray) = app.tray_by_id("pii-tray") {
        let data = render_shield_icon(r, g, b);
        let icon = Image::new_owned(data, 32, 32);
        let _ = tray.set_icon(Some(icon));
    }
}

const TRAY_IDLE: (u8, u8, u8) = (34, 197, 94); // green  #22c55e
const TRAY_WARNING: (u8, u8, u8) = (245, 158, 11); // orange #f59e0b
const TRAY_DANGER: (u8, u8, u8) = (239, 68, 68); // red    #ef4444

/// Returns true if the entity type is a secret/credential (vs personal PII).
fn is_secret_entity(entity_type: &str) -> bool {
    matches!(
        entity_type,
        "API_KEY"
            | "OPENAI_API_KEY"
            | "ANTHROPIC_API_KEY"
            | "AWS_ACCESS_KEY"
            | "GITHUB_TOKEN"
            | "JWT_TOKEN"
            | "PRIVATE_KEY"
    )
}

// ─────────────────────────────────────────────────────────────────────────────

/// Check if we should auto-anonymize for this window based on configured keywords
fn should_auto_anonymize(window_info: &window::WindowInfo, keywords: &[String]) -> bool {
    // Check app_name first if available
    if let Some(ref app_name) = window_info.app_name {
        let app_name_lower = app_name.to_lowercase();
        if keywords
            .iter()
            .any(|keyword| app_name_lower.contains(keyword))
        {
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

/// A single history entry recording a tokenization or de-tokenization event
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HistoryEntry {
    pub timestamp: u64,
    pub action: String,
    pub entity_count: usize,
    pub app_name: String,
    pub original_preview: String,
    pub tokenized_preview: String,
}

impl HistoryEntry {
    fn new(
        action: &str,
        entity_count: usize,
        app_name: &str,
        original: &str,
        tokenized: &str,
    ) -> Self {
        let preview = |s: &str| -> String {
            if s.len() > 60 {
                format!("{}…", &s[..60])
            } else {
                s.to_string()
            }
        };
        Self {
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            action: action.to_string(),
            entity_count,
            app_name: app_name.to_string(),
            original_preview: preview(original),
            tokenized_preview: preview(tokenized),
        }
    }
}

/// Application state shared across the Tauri app
pub struct AppState {
    sidecar: Arc<tokio::sync::Mutex<PresidioSidecar>>,
    last_clipboard_hash: Mutex<u64>,
    clipboard_handled: Mutex<bool>,
    /// App configuration (language, score threshold, monitored apps)
    config: Mutex<config::Config>,
    /// Token vault for storing PII token mappings for de-tokenization
    token_vault: Arc<Mutex<TokenVault>>,
    /// Pending tokenization result (used when auto-tokenizing in browsers)
    pending_tokenization: Arc<Mutex<Option<TokenizationResult>>>,
    /// In-memory session history (never persisted to disk)
    history: Arc<Mutex<Vec<HistoryEntry>>>,
}

impl AppState {
    pub fn new() -> Self {
        let config = config::Config::load();
        log::info!(
            "Loaded config with {} keywords",
            config.get_all_keywords().len()
        );

        Self {
            sidecar: Arc::new(tokio::sync::Mutex::new(PresidioSidecar::new())),
            last_clipboard_hash: Mutex::new(0),
            clipboard_handled: Mutex::new(false),
            config: Mutex::new(config),
            token_vault: Arc::new(Mutex::new(TokenVault::new())),
            pending_tokenization: Arc::new(Mutex::new(None)),
            history: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
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
        let keywords = state.config.lock().get_all_keywords();
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

                    // Mark as handled AND update hash to prevent re-processing
                    {
                        let state = app_handle.state::<AppState>();
                        let tokenized_hash =
                            clipboard::hash_text(&tokenization_result.tokenized_text);
                        let mut last_hash = state.last_clipboard_hash.lock();
                        let mut handled = state.clipboard_handled.lock();
                        *last_hash = tokenized_hash;
                        *handled = true;
                    }

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
    let history = state.history.clone();
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
                    } else {
                        false
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
                            // Mark as handled AND update hash to prevent re-processing
                            {
                                let state = app_handle_clone.state::<AppState>();
                                let detokenized_hash = clipboard::hash_text(&detokenized);
                                let mut last_hash = state.last_clipboard_hash.lock();
                                let mut handled = state.clipboard_handled.lock();
                                *last_hash = detokenized_hash;
                                *handled = true;
                            }

                            // Get the token map for the event
                            let token_map = {
                                let vault = token_vault.lock();
                                vault.token_map.clone()
                            };

                            // Record history entry
                            {
                                let entry = HistoryEntry::new(
                                    "detokenized",
                                    token_map.len(),
                                    "clipboard",
                                    &text,
                                    &detokenized,
                                );
                                let mut hist = history.lock();
                                hist.push(entry.clone());
                                if hist.len() > 50 {
                                    hist.remove(0);
                                }
                                let _ = app_handle_clone.emit("history-updated", &*hist);
                            }

                            // Restore idle tray icon
                            let (r, g, b) = TRAY_IDLE;
                            set_tray_icon(&app_handle_clone, r, g, b);
                            if let Some(tray) = app_handle_clone.tray_by_id("pii-tray") {
                                let _ = tray.set_tooltip(Some("PII Shield — monitoring"));
                            }

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
                            let state = app_handle_clone.state::<AppState>();
                            let (lang, threshold) = {
                                let cfg = state.config.lock();
                                (cfg.language.clone(), cfg.score_threshold)
                            };
                            let sidecar = sidecar.lock().await;
                            sidecar
                                .analyze_and_tokenize(&text, Some(&lang), Some(threshold))
                                .await
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

                                    // Record history entry
                                    {
                                        let entry = HistoryEntry::new(
                                            "detected",
                                            tokenization.entities.len(),
                                            "clipboard",
                                            &tokenization.original_text,
                                            &tokenization.tokenized_text,
                                        );
                                        let mut hist = history.lock();
                                        hist.push(entry);
                                        if hist.len() > 50 {
                                            hist.remove(0);
                                        }
                                        let _ = app_handle_clone.emit("history-updated", &*hist);
                                    }

                                    // Update tray icon: red for secrets, orange for PII
                                    let has_secrets = tokenization
                                        .entities
                                        .iter()
                                        .any(|e| is_secret_entity(&e.entity_type));
                                    if has_secrets {
                                        let (r, g, b) = TRAY_DANGER;
                                        set_tray_icon(&app_handle_clone, r, g, b);
                                        if let Some(tray) = app_handle_clone.tray_by_id("pii-tray")
                                        {
                                            let _ = tray.set_tooltip(Some(
                                                "PII Shield — secrets detected!",
                                            ));
                                        }
                                    } else {
                                        let (r, g, b) = TRAY_WARNING;
                                        set_tray_icon(&app_handle_clone, r, g, b);
                                        if let Some(tray) = app_handle_clone.tray_by_id("pii-tray")
                                        {
                                            let _ = tray.set_tooltip(Some(&format!(
                                                "PII Shield — {} PII item{} detected",
                                                tokenization.entities.len(),
                                                if tokenization.entities.len() == 1 {
                                                    ""
                                                } else {
                                                    "s"
                                                }
                                            )));
                                        }
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
                    let kw = state.config.lock().get_all_keywords();
                    kw
                };
                let should_anonymize = should_auto_anonymize(&window_info, &keywords);
                if should_anonymize {
                    log::debug!(
                        "Auto-anonymize target detected: {} ({})",
                        window_info.app_name.as_deref().unwrap_or("unknown"),
                        window_info.title
                    );
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
                        if let Err(e) =
                            clipboard::set_clipboard_text(&tokenization_result.tokenized_text)
                        {
                            log::error!("Failed to auto-tokenize clipboard: {}", e);
                        } else {
                            log::info!("Clipboard replaced with tokenized text");

                            // Mark as handled AND update hash to prevent re-processing
                            let tokenized_hash =
                                clipboard::hash_text(&tokenization_result.tokenized_text);
                            let mut last_hash = state.last_clipboard_hash.lock();
                            let mut handled = state.clipboard_handled.lock();
                            *last_hash = tokenized_hash;
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

/// Mark current clipboard content as handled (user clicked tokenize or ignore)
#[tauri::command]
async fn mark_clipboard_handled(
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let mut handled = state.clipboard_handled.lock();
    *handled = true;
    // Restore idle (green) tray icon
    let (r, g, b) = TRAY_IDLE;
    set_tray_icon(&app_handle, r, g, b);
    if let Some(tray) = app_handle.tray_by_id("pii-tray") {
        let _ = tray.set_tooltip(Some("PII Shield — monitoring"));
    }
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
    let (lang, threshold) = {
        let cfg = state.config.lock();
        (cfg.language.clone(), cfg.score_threshold)
    };
    let sidecar = state.sidecar.lock().await;
    let result = sidecar
        .analyze_and_tokenize(&text, Some(&lang), Some(threshold))
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
async fn detokenize_text(text: String, state: State<'_, AppState>) -> Result<String, String> {
    let vault = state.token_vault.lock();
    let detokenized = detokenize_with_vault(&text, &vault);
    Ok(detokenized)
}

/// Get the in-memory session history (last 50 entries)
#[tauri::command]
async fn get_history(state: State<'_, AppState>) -> Result<Vec<HistoryEntry>, String> {
    let history = state.history.lock();
    Ok(history.clone())
}

/// Get the current configuration
#[tauri::command]
async fn get_config(state: State<'_, AppState>) -> Result<config::Config, String> {
    let config = state.config.lock();
    Ok(config.clone())
}

/// Save updated configuration to disk
#[tauri::command]
async fn save_config(new_config: config::Config, state: State<'_, AppState>) -> Result<(), String> {
    new_config.save().map_err(|e| e.to_string())?;
    let mut config = state.config.lock();
    *config = new_config;
    log::info!("Config updated via Settings UI");
    Ok(())
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

            // Render the initial idle (green) shield icon
            let idle_data = render_shield_icon(TRAY_IDLE.0, TRAY_IDLE.1, TRAY_IDLE.2);

            let _tray = TrayIconBuilder::with_id("pii-tray")
                .icon(Image::new_owned(idle_data, 32, 32))
                .tooltip("PII Shield — monitoring")
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
            get_config,
            save_config,
            get_history,
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
