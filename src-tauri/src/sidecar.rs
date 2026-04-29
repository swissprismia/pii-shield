use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::process::Stdio;
use tauri::AppHandle;
use tauri::Manager;
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;

#[derive(Error, Debug)]
pub enum SidecarError {
    #[error("Failed to start sidecar: {0}")]
    StartError(String),
    #[error("Sidecar not running")]
    NotRunning,
    #[error("Failed to communicate with sidecar: {0}")]
    CommunicationError(String),
    #[error("Failed to parse response: {0}")]
    ParseError(String),
    #[error("Analysis failed: {0}")]
    AnalysisError(String),
}

/// Entity detected by Presidio
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PiiEntity {
    pub entity_type: String,
    pub text: String,
    pub start: usize,
    pub end: usize,
    pub score: f64,
}

/// Analysis result from Presidio sidecar
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisResult {
    pub original_text: String,
    pub anonymized_text: String,
    pub entities: Vec<PiiEntity>,
}

/// Tokenization result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenizationResult {
    pub original_text: String,
    pub tokenized_text: String,
    pub token_map: std::collections::HashMap<String, String>,
    pub entities: Vec<PiiEntity>,
}

/// De-tokenization result
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetokenizationResult {
    pub tokenized_text: String,
    pub detokenized_text: String,
}

/// Request to the sidecar
#[derive(Debug, Serialize, Deserialize)]
struct SidecarRequest {
    action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    entities: Option<Vec<PiiEntity>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    token_map: Option<std::collections::HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    score_threshold: Option<f64>,
}

/// Response from the sidecar
#[derive(Debug, Serialize, Deserialize)]
struct SidecarResponse {
    success: bool,
    #[serde(default)]
    anonymized_text: String,
    #[serde(default)]
    entities: Vec<PiiEntity>,
    #[serde(default)]
    error: Option<String>,
    // Tokenization fields
    #[serde(default)]
    tokenized_text: String,
    #[serde(default)]
    token_map: std::collections::HashMap<String, String>,
    #[serde(default)]
    original_text: String,
    // De-tokenization fields
    #[serde(default)]
    detokenized_text: String,
    // Token detection fields
    #[serde(default)]
    tokens: Vec<String>,
    #[serde(default)]
    has_tokens: bool,
}

use std::sync::Arc;
use tokio::sync::Mutex;

const SIDECAR_READY_TIMEOUT_SECS: u64 = 120;
const SIDECAR_RESPONSE_TIMEOUT_SECS: u64 = 20;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

/// Manages the Presidio Python sidecar process
pub struct PresidioSidecar {
    child: Option<Child>,
    stdin_tx: Option<mpsc::Sender<String>>,
    response_rx: Option<Arc<Mutex<mpsc::Receiver<String>>>>,
}

impl PresidioSidecar {
    pub fn new() -> Self {
        Self {
            child: None,
            stdin_tx: None,
            response_rx: None,
        }
    }

    /// Check if sidecar is running
    pub fn is_running(&self) -> bool {
        self.child.is_some()
    }

    /// Start the sidecar process
    pub async fn start(&mut self, app_handle: &AppHandle) -> Result<(), SidecarError> {
        if self.child.is_some() {
            log::info!("Sidecar already running");
            return Ok(());
        }

        log::info!("Starting Presidio sidecar...");

        // Check for Python script first (development mode)
        let python_script = development_sidecar_script();
        log::info!("Checking for Python script at: {:?}", python_script);
        if python_script.exists() {
            return self.start_python_sidecar(&python_script).await;
        }

        // Try bundled binaries
        let bundled_candidates = bundled_sidecar_candidates(app_handle);
        for path in &bundled_candidates {
            log::info!("Checking for binary at: {:?}", path);
            if path.exists() {
                return self.start_binary_sidecar(path).await;
            }
        }

        // If no sidecar found, return error
        log::error!(
            "No sidecar found. Current dir: {:?}. Tried: {:?}",
            std::env::current_dir().ok(),
            bundled_candidates
        );
        Err(SidecarError::StartError(format!(
            "Presidio sidecar not found. Tried development script {:?} and bundled paths {:?}.",
            python_script, bundled_candidates
        )))
    }

    /// Start Python sidecar (development mode)
    async fn start_python_sidecar(
        &mut self,
        script_path: &std::path::Path,
    ) -> Result<(), SidecarError> {
        log::info!("Starting Python sidecar from: {:?}", script_path);

        let python_candidates = python_command_candidates(script_path);
        let mut attempted = Vec::new();
        let mut last_error = None;

        for python_cmd in python_candidates {
            let python_label = python_cmd.to_string_lossy().to_string();
            attempted.push(python_label.clone());
            log::info!("Trying Python runtime: {}", python_label);

            let mut command = Command::new(&python_cmd);
            command
                .arg(script_path)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true);
            configure_sidecar_command(&mut command);

            match command.spawn() {
                Ok(child) => match self.initialize_child(child).await {
                    Ok(child) => {
                        self.child = Some(child);
                        return Ok(());
                    }
                    Err(err) => {
                        log::warn!(
                            "Failed to initialize Python runtime {}: {}",
                            python_label,
                            err
                        );
                        last_error = Some(err.to_string());
                    }
                },
                Err(err) => {
                    log::warn!("Failed to start Python runtime {}: {}", python_label, err);
                    last_error = Some(err.to_string());
                }
            }
        }

        Err(SidecarError::StartError(format!(
            "Failed to start Python sidecar. Tried: {}. Last error: {}",
            attempted.join(", "),
            last_error.unwrap_or_else(|| "unknown error".to_string())
        )))
    }

    /// Start binary sidecar (production mode)
    async fn start_binary_sidecar(
        &mut self,
        binary_path: &std::path::Path,
    ) -> Result<(), SidecarError> {
        log::info!("Starting binary sidecar from: {:?}", binary_path);

        let mut command = Command::new(binary_path);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        configure_sidecar_command(&mut command);

        let child = command
            .spawn()
            .map_err(|e| SidecarError::StartError(e.to_string()))?;

        let child = self.initialize_child(child).await?;
        self.child = Some(child);
        Ok(())
    }

    /// Start mock sidecar for development when Presidio isn't available
    #[allow(dead_code)]
    async fn start_mock_sidecar(&mut self) -> Result<(), SidecarError> {
        log::warn!("Using mock sidecar - PII detection will be simulated");

        // Don't set up channels for mock mode - we'll detect this and use mock_analyze directly
        self.stdin_tx = None;
        self.response_rx = None;

        Ok(())
    }

    /// Set up IO channels for the sidecar process
    async fn setup_io_channels(&mut self, child: &mut Child) -> Result<(), SidecarError> {
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| SidecarError::StartError("Failed to get stdin".to_string()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| SidecarError::StartError("Failed to get stdout".to_string()))?;
        let stderr = child.stderr.take();

        // Create channels for communication
        let (stdin_tx, mut stdin_rx) = mpsc::channel::<String>(100);
        let (response_tx, response_rx) = mpsc::channel::<String>(100);

        // Spawn stdin writer task
        let mut stdin_writer = stdin;
        tokio::spawn(async move {
            while let Some(msg) = stdin_rx.recv().await {
                if let Err(e) = stdin_writer.write_all(msg.as_bytes()).await {
                    log::error!("Failed to write to sidecar stdin: {}", e);
                    break;
                }
                if let Err(e) = stdin_writer.write_all(b"\n").await {
                    log::error!("Failed to write newline to sidecar stdin: {}", e);
                    break;
                }
                if let Err(e) = stdin_writer.flush().await {
                    log::error!("Failed to flush sidecar stdin: {}", e);
                    break;
                }
            }
        });

        // Spawn stdout reader task
        let mut reader = BufReader::new(stdout).lines();
        tokio::spawn(async move {
            while let Ok(Some(line)) = reader.next_line().await {
                if response_tx.send(line).await.is_err() {
                    break;
                }
            }
        });

        if let Some(stderr) = stderr {
            let mut stderr_reader = BufReader::new(stderr).lines();
            tokio::spawn(async move {
                while let Ok(Some(line)) = stderr_reader.next_line().await {
                    log::warn!("Sidecar stderr: {}", line);
                }
            });
        }

        self.stdin_tx = Some(stdin_tx);
        self.response_rx = Some(Arc::new(Mutex::new(response_rx)));

        Ok(())
    }

    async fn initialize_child(&mut self, mut child: Child) -> Result<Child, SidecarError> {
        self.stdin_tx = None;
        self.response_rx = None;

        if let Err(err) = self.setup_io_channels(&mut child).await {
            self.stdin_tx = None;
            self.response_rx = None;
            return Err(err);
        }

        if let Err(err) = self.wait_for_ready().await {
            self.stdin_tx = None;
            self.response_rx = None;
            return Err(err);
        }

        Ok(child)
    }

    /// Wait for the sidecar to signal it's ready.
    ///
    /// `rx_guard` is held for the full timeout duration. This is safe because
    /// `start()` takes `&mut self`, so the outer `Mutex<PresidioSidecar>` held by
    /// the caller prevents any concurrent `send_request` from running while startup
    /// is in progress. If that outer lock is ever removed, this reasoning must be
    /// revisited.
    async fn wait_for_ready(&mut self) -> Result<(), SidecarError> {
        if let Some(ref rx) = self.response_rx {
            let mut rx_guard = rx.lock().await;
            let deadline = tokio::time::Instant::now()
                + std::time::Duration::from_secs(SIDECAR_READY_TIMEOUT_SECS);
            let mut recent_output = Vec::new();

            loop {
                let now = tokio::time::Instant::now();
                if now >= deadline {
                    return Err(SidecarError::StartError(format!(
                        "Timeout waiting for sidecar ready{}",
                        format_recent_output(&recent_output)
                    )));
                }

                match tokio::time::timeout(deadline - now, rx_guard.recv()).await {
                    Ok(Some(line)) => {
                        let trimmed = line.trim();
                        if trimmed.is_empty() {
                            continue;
                        }

                        match serde_json::from_str::<serde_json::Value>(trimmed) {
                            Ok(value) if is_ready_payload(&value) => {
                                log::info!("Sidecar is ready: {}", trimmed);
                                return Ok(());
                            }
                            Ok(value) => {
                                remember_output(
                                    &mut recent_output,
                                    serde_json::to_string(&value)
                                        .unwrap_or_else(|_| trimmed.to_string()),
                                );
                                log::warn!("Ignoring unexpected startup message: {}", trimmed);
                            }
                            Err(err) => {
                                remember_output(&mut recent_output, trimmed.to_string());
                                log::warn!(
                                    "Ignoring non-JSON startup output from sidecar: {} ({})",
                                    trimmed,
                                    err
                                );
                            }
                        }
                    }
                    Ok(None) => {
                        return Err(SidecarError::StartError(format!(
                            "Sidecar closed unexpectedly before signaling ready{}",
                            format_recent_output(&recent_output)
                        )));
                    }
                    Err(_) => {
                        return Err(SidecarError::StartError(format!(
                            "Timeout waiting for sidecar ready{}",
                            format_recent_output(&recent_output)
                        )));
                    }
                }
            }
        } else {
            Ok(())
        }
    }

    /// Send a request to the sidecar and get a response
    async fn send_request(&self, request: SidecarRequest) -> Result<SidecarResponse, SidecarError> {
        // Check if sidecar is running
        if self.stdin_tx.is_none() || self.response_rx.is_none() {
            return Err(SidecarError::NotRunning);
        }

        let request_json =
            serde_json::to_string(&request).map_err(|e| SidecarError::ParseError(e.to_string()))?;

        // Send request
        if let Some(ref tx) = self.stdin_tx {
            tx.send(request_json).await.map_err(|e| {
                SidecarError::CommunicationError(format!("Failed to send to sidecar: {}", e))
            })?;
        }

        // Wait for response with timeout
        if let Some(ref rx) = self.response_rx {
            let mut rx_guard = rx.lock().await;
            let deadline = tokio::time::Instant::now()
                + std::time::Duration::from_secs(SIDECAR_RESPONSE_TIMEOUT_SECS);
            let mut recent_output = Vec::new();

            loop {
                let now = tokio::time::Instant::now();
                if now >= deadline {
                    log::error!(
                        "Timeout waiting for sidecar response{}",
                        format_recent_output(&recent_output)
                    );
                    return Err(SidecarError::CommunicationError(format!(
                        "Timeout waiting for sidecar response{}",
                        format_recent_output(&recent_output)
                    )));
                }

                match tokio::time::timeout(deadline - now, rx_guard.recv()).await {
                    Ok(Some(line)) => {
                        let trimmed = line.trim();
                        if trimmed.is_empty() {
                            continue;
                        }

                        log::debug!("Received response candidate from sidecar: {}", trimmed);
                        match serde_json::from_str::<SidecarResponse>(trimmed) {
                            Ok(response) => {
                                if !response.success {
                                    if let Some(error) = response.error.clone() {
                                        return Err(SidecarError::AnalysisError(error));
                                    }
                                }

                                return Ok(response);
                            }
                            Err(err) => {
                                remember_output(&mut recent_output, trimmed.to_string());
                                log::warn!(
                                    "Ignoring non-protocol sidecar output while awaiting response: {} ({})",
                                    trimmed,
                                    err
                                );
                            }
                        }
                    }
                    Ok(None) => {
                        log::error!(
                            "Sidecar closed unexpectedly{}",
                            format_recent_output(&recent_output)
                        );
                        return Err(SidecarError::CommunicationError(format!(
                            "Sidecar closed{}",
                            format_recent_output(&recent_output)
                        )));
                    }
                    Err(_) => {
                        log::error!(
                            "Timeout waiting for sidecar response{}",
                            format_recent_output(&recent_output)
                        );
                        return Err(SidecarError::CommunicationError(format!(
                            "Timeout waiting for sidecar response{}",
                            format_recent_output(&recent_output)
                        )));
                    }
                }
            }
        }

        Err(SidecarError::NotRunning)
    }

    /// Analyze text for PII
    #[allow(dead_code)]
    pub async fn analyze(
        &self,
        text: &str,
        language: Option<&str>,
        score_threshold: Option<f64>,
    ) -> Result<AnalysisResult, SidecarError> {
        let request = SidecarRequest {
            action: "analyze".to_string(),
            text: Some(text.to_string()),
            entities: None,
            token_map: None,
            language: language.map(|s| s.to_string()),
            score_threshold,
        };

        let response = self.send_request(request).await?;

        Ok(AnalysisResult {
            original_text: text.to_string(),
            anonymized_text: response.anonymized_text,
            entities: response.entities,
        })
    }

    /// Analyze text for PII and tokenize it in one step
    pub async fn analyze_and_tokenize(
        &self,
        text: &str,
        language: Option<&str>,
        score_threshold: Option<f64>,
    ) -> Result<TokenizationResult, SidecarError> {
        let request = SidecarRequest {
            action: "analyze_and_tokenize".to_string(),
            text: Some(text.to_string()),
            entities: None,
            token_map: None,
            language: language.map(|s| s.to_string()),
            score_threshold,
        };

        let response = self.send_request(request).await?;

        Ok(TokenizationResult {
            original_text: text.to_string(),
            tokenized_text: response.tokenized_text,
            token_map: response.token_map,
            entities: response.entities,
        })
    }

    /// De-tokenize text by replacing tokens with original values
    #[allow(dead_code)]
    pub async fn detokenize(
        &self,
        text: &str,
        token_map: std::collections::HashMap<String, String>,
    ) -> Result<DetokenizationResult, SidecarError> {
        let request = SidecarRequest {
            action: "detokenize".to_string(),
            text: Some(text.to_string()),
            entities: None,
            token_map: Some(token_map),
            language: None,
            score_threshold: None,
        };

        let response = self.send_request(request).await?;

        Ok(DetokenizationResult {
            tokenized_text: text.to_string(),
            detokenized_text: response.detokenized_text,
        })
    }

    /// Detect if text contains tokens
    #[allow(dead_code)]
    pub async fn detect_tokens(&self, text: &str) -> Result<(bool, Vec<String>), SidecarError> {
        let request = SidecarRequest {
            action: "detect_tokens".to_string(),
            text: Some(text.to_string()),
            entities: None,
            token_map: None,
            language: None,
            score_threshold: None,
        };

        let response = self.send_request(request).await?;

        Ok((response.has_tokens, response.tokens))
    }

    /// Mock analysis using simple pattern matching
    #[allow(dead_code)]
    fn mock_analyze(&self, text: &str) -> Result<AnalysisResult, SidecarError> {
        let mut entities = Vec::new();
        let mut anonymized = text.to_string();

        // Simple regex-based detection for common PII patterns
        let patterns: Vec<(&str, &str, regex::Regex)> = vec![
            (
                "EMAIL_ADDRESS",
                "[EMAIL]",
                regex::Regex::new(r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}").unwrap(),
            ),
            (
                "PHONE_NUMBER",
                "[PHONE]",
                regex::Regex::new(r"\b(\+?1[-.\s]?)?\(?\d{3}\)?[-.\s]?\d{3}[-.\s]?\d{4}\b")
                    .unwrap(),
            ),
            (
                "CREDIT_CARD",
                "[CREDIT_CARD]",
                regex::Regex::new(r"\b\d{4}[-\s]?\d{4}[-\s]?\d{4}[-\s]?\d{4}\b").unwrap(),
            ),
            (
                "US_SSN",
                "[SSN]",
                regex::Regex::new(r"\b\d{3}[-\s]?\d{2}[-\s]?\d{4}\b").unwrap(),
            ),
            (
                "IP_ADDRESS",
                "[IP_ADDRESS]",
                regex::Regex::new(r"\b\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}\b").unwrap(),
            ),
        ];

        // Collect all matches first
        let mut matches: Vec<(usize, usize, String, String, String)> = Vec::new();

        for (entity_type, replacement, pattern) in &patterns {
            for mat in pattern.find_iter(text) {
                matches.push((
                    mat.start(),
                    mat.end(),
                    entity_type.to_string(),
                    replacement.to_string(),
                    mat.as_str().to_string(),
                ));
            }
        }

        // Sort by position (reverse) so we can replace from end to start
        matches.sort_by_key(|b| std::cmp::Reverse(b.0));

        // Build entities and anonymized text
        for (start, end, entity_type, replacement, matched_text) in matches {
            entities.push(PiiEntity {
                entity_type: entity_type.clone(),
                text: matched_text,
                start,
                end,
                score: 0.85,
            });

            anonymized.replace_range(start..end, &replacement);
        }

        // Reverse entities to match original order
        entities.reverse();

        Ok(AnalysisResult {
            original_text: text.to_string(),
            anonymized_text: anonymized,
            entities,
        })
    }

    /// Stop the sidecar process
    pub fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            log::info!("Stopping sidecar...");
            let _ = child.start_kill();
        }
        self.stdin_tx = None;
        self.response_rx = None;
    }
}

impl Drop for PresidioSidecar {
    fn drop(&mut self) {
        self.stop();
    }
}

fn python_command_candidates(script_path: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut candidates = Vec::new();

    if let Some(parent) = script_path.parent() {
        let venv_dirs = [".venv", "venv"];

        #[cfg(target_os = "windows")]
        {
            for venv_dir in venv_dirs {
                candidates.push(parent.join(venv_dir).join("Scripts").join("python.exe"));
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            for venv_dir in venv_dirs {
                candidates.push(parent.join(venv_dir).join("bin").join("python"));
                candidates.push(parent.join(venv_dir).join("bin").join("python3"));
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        candidates.push(std::path::PathBuf::from("python"));
        candidates.push(std::path::PathBuf::from("py"));
    }

    #[cfg(not(target_os = "windows"))]
    {
        candidates.push(std::path::PathBuf::from("python3"));
        candidates.push(std::path::PathBuf::from("python"));
    }

    candidates
}

fn remember_output(lines: &mut Vec<String>, line: String) {
    const MAX_LINES: usize = 5;

    if lines.len() == MAX_LINES {
        lines.remove(0);
    }
    lines.push(line);
}

fn format_recent_output(lines: &[String]) -> String {
    if lines.is_empty() {
        String::new()
    } else {
        format!("; recent sidecar output: {}", lines.join(" | "))
    }
}

fn is_ready_payload(value: &serde_json::Value) -> bool {
    value
        .get("status")
        .and_then(|status| status.as_str())
        .map(|status| status.eq_ignore_ascii_case("ready"))
        .unwrap_or(false)
}

fn configure_sidecar_command(command: &mut Command) {
    #[cfg(target_os = "windows")]
    {
        command.creation_flags(CREATE_NO_WINDOW);
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = command;
    }
}

fn development_sidecar_script() -> std::path::PathBuf {
    std::env::current_dir()
        .unwrap_or_default()
        .parent()
        .map(|p| p.join("sidecar").join("presidio_sidecar.py"))
        .unwrap_or_default()
}

fn packaged_sidecar_names() -> &'static [&'static str] {
    #[cfg(target_os = "windows")]
    {
        &[
            "presidio-sidecar-x86_64-pc-windows-msvc.exe",
            "presidio-sidecar.exe",
            "presidio-sidecar",
        ]
    }

    #[cfg(target_os = "macos")]
    {
        &[
            "presidio-sidecar-universal-apple-darwin",
            "presidio-sidecar-aarch64-apple-darwin",
            "presidio-sidecar-x86_64-apple-darwin",
            "presidio-sidecar",
        ]
    }

    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        &[
            "presidio-sidecar-x86_64-unknown-linux-gnu",
            "presidio-sidecar-aarch64-unknown-linux-gnu",
            "presidio-sidecar",
        ]
    }
}

fn bundled_sidecar_candidates(app_handle: &AppHandle) -> Vec<std::path::PathBuf> {
    let mut dirs = Vec::new();

    if let Ok(resource_dir) = app_handle.path().resource_dir() {
        dirs.push(resource_dir);
    }

    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(exe_dir) = current_exe.parent() {
            dirs.push(exe_dir.to_path_buf());
            if let Some(parent) = exe_dir.parent() {
                dirs.push(parent.join("Resources"));
            }
        }
    }

    let mut seen = HashSet::new();
    let mut candidates = Vec::new();
    for dir in dirs {
        for name in packaged_sidecar_names() {
            let path = dir.join(name);
            if seen.insert(path.clone()) {
                candidates.push(path);
            }
        }
    }

    candidates
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_email_detection() {
        let sidecar = PresidioSidecar::new();
        let result = sidecar
            .mock_analyze("Contact me at john.doe@example.com")
            .unwrap();

        assert_eq!(result.entities.len(), 1);
        assert_eq!(result.entities[0].entity_type, "EMAIL_ADDRESS");
        assert!(result.anonymized_text.contains("[EMAIL]"));
    }

    #[test]
    fn test_mock_phone_detection() {
        let sidecar = PresidioSidecar::new();
        let result = sidecar.mock_analyze("Call me at 555-123-4567").unwrap();

        assert_eq!(result.entities.len(), 1);
        assert_eq!(result.entities[0].entity_type, "PHONE_NUMBER");
        assert!(result.anonymized_text.contains("[PHONE]"));
    }

    #[test]
    fn test_mock_multiple_pii() {
        let sidecar = PresidioSidecar::new();
        let result = sidecar
            .mock_analyze("Email: test@test.com, Phone: 123-456-7890")
            .unwrap();

        assert_eq!(result.entities.len(), 2);
    }

    #[test]
    fn test_python_candidates_include_virtualenv_and_system_fallback() {
        let script_path = std::path::Path::new("sidecar/presidio_sidecar.py");
        let candidates = python_command_candidates(script_path);
        let candidate_strings: Vec<String> = candidates
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect();

        #[cfg(target_os = "windows")]
        {
            assert!(candidate_strings.iter().any(|path| {
                path.ends_with(".venv\\Scripts\\python.exe")
                    || path.ends_with("venv\\Scripts\\python.exe")
            }));
            assert!(candidate_strings.iter().any(|path| path == "python"));
        }

        #[cfg(not(target_os = "windows"))]
        {
            assert!(candidate_strings.iter().any(|path| {
                path.ends_with(".venv/bin/python") || path.ends_with("venv/bin/python")
            }));
            assert!(candidate_strings.iter().any(|path| path == "python3"));
        }
    }

    #[test]
    fn test_packaged_sidecar_names_include_platform_binary() {
        let names = packaged_sidecar_names();

        #[cfg(target_os = "windows")]
        assert!(names.contains(&"presidio-sidecar-x86_64-pc-windows-msvc.exe"));

        #[cfg(target_os = "macos")]
        assert!(names.contains(&"presidio-sidecar-universal-apple-darwin"));

        #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
        assert!(names.contains(&"presidio-sidecar-x86_64-unknown-linux-gnu"));
    }

    #[test]
    fn test_is_ready_payload_detects_ready_status() {
        let ready = serde_json::json!({ "status": "ready", "presidio": true });
        let not_ready = serde_json::json!({ "success": true });

        assert!(is_ready_payload(&ready));
        assert!(!is_ready_payload(&not_ready));
    }

    #[test]
    fn test_remember_output_keeps_recent_lines() {
        let mut lines = Vec::new();

        for idx in 0..7 {
            remember_output(&mut lines, format!("line-{idx}"));
        }

        assert_eq!(
            lines,
            vec!["line-2", "line-3", "line-4", "line-5", "line-6"]
        );
    }
}
