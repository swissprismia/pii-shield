use serde::{Deserialize, Serialize};
use std::process::Stdio;
use tauri::AppHandle;
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
    pub async fn start(&mut self, _app_handle: &AppHandle) -> Result<(), SidecarError> {
        if self.child.is_some() {
            log::info!("Sidecar already running");
            return Ok(());
        }

        log::info!("Starting Presidio sidecar...");

        // Try to find the sidecar binary in various locations
        let sidecar_paths = [
            // Development path - Python script (from src-tauri, go up one level to project root)
            std::env::current_dir()
                .unwrap_or_default()
                .parent()
                .map(|p| p.join("sidecar").join("presidio_sidecar.py"))
                .unwrap_or_default(),
            // Bundled binary (PyInstaller)
            std::env::current_exe()
                .unwrap_or_default()
                .parent()
                .map(|p| p.join("presidio-sidecar"))
                .unwrap_or_default(),
            // macOS app bundle
            std::env::current_exe()
                .unwrap_or_default()
                .parent()
                .and_then(|p| p.parent())
                .map(|p| p.join("Resources").join("presidio-sidecar"))
                .unwrap_or_default(),
        ];

        // Check for Python script first (development mode)
        let python_script = &sidecar_paths[0];
        log::info!("Checking for Python script at: {:?}", python_script);
        if python_script.exists() {
            return self.start_python_sidecar(python_script).await;
        }

        // Try bundled binaries
        for path in &sidecar_paths[1..] {
            log::info!("Checking for binary at: {:?}", path);
            if path.exists() {
                return self.start_binary_sidecar(path).await;
            }
        }

        // If no sidecar found, return error
        log::error!(
            "No sidecar found. Current dir: {:?}",
            std::env::current_dir().ok()
        );
        Err(SidecarError::StartError(
            "Presidio sidecar not found. Make sure presidio_sidecar.py exists in the sidecar directory.".to_string()
        ))
    }

    /// Start Python sidecar (development mode)
    async fn start_python_sidecar(
        &mut self,
        script_path: &std::path::Path,
    ) -> Result<(), SidecarError> {
        log::info!("Starting Python sidecar from: {:?}", script_path);

        // Try to use venv Python first
        let venv_python = script_path
            .parent()
            .map(|p| p.join(".venv").join("Scripts").join("python.exe"));

        let python_cmd = if let Some(ref venv_path) = venv_python {
            if venv_path.exists() {
                log::info!("Using venv Python: {:?}", venv_path);
                venv_path.to_str().unwrap_or("python")
            } else {
                log::warn!("venv not found, trying system Python");
                "python"
            }
        } else {
            "python"
        };

        let mut child = Command::new(python_cmd)
            .arg(script_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| {
                SidecarError::StartError(format!("Failed to start Python ({}): {}", python_cmd, e))
            })?;

        self.setup_io_channels(&mut child).await?;
        self.child = Some(child);

        // Wait for ready signal
        self.wait_for_ready().await
    }

    /// Start binary sidecar (production mode)
    async fn start_binary_sidecar(
        &mut self,
        binary_path: &std::path::Path,
    ) -> Result<(), SidecarError> {
        log::info!("Starting binary sidecar from: {:?}", binary_path);

        let mut child = Command::new(binary_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| SidecarError::StartError(e.to_string()))?;

        self.setup_io_channels(&mut child).await?;
        self.child = Some(child);

        self.wait_for_ready().await
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

        self.stdin_tx = Some(stdin_tx);
        self.response_rx = Some(Arc::new(Mutex::new(response_rx)));

        Ok(())
    }

    /// Wait for the sidecar to signal it's ready
    async fn wait_for_ready(&mut self) -> Result<(), SidecarError> {
        if let Some(ref rx) = self.response_rx {
            let mut rx_guard = rx.lock().await;
            match tokio::time::timeout(std::time::Duration::from_secs(30), rx_guard.recv()).await {
                Ok(Some(line)) => {
                    if line.contains("ready") {
                        log::info!("Sidecar is ready: {}", line);
                        Ok(())
                    } else {
                        log::warn!("Unexpected ready message: {}", line);
                        Ok(())
                    }
                }
                Ok(None) => Err(SidecarError::StartError(
                    "Sidecar closed unexpectedly".to_string(),
                )),
                Err(_) => Err(SidecarError::StartError(
                    "Timeout waiting for sidecar ready".to_string(),
                )),
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
            match tokio::time::timeout(std::time::Duration::from_secs(5), rx_guard.recv()).await {
                Ok(Some(line)) => {
                    log::debug!("Received response from sidecar: {}", line);
                    let response: SidecarResponse = serde_json::from_str(&line).map_err(|e| {
                        SidecarError::ParseError(format!("Failed to parse response: {}", e))
                    })?;

                    if !response.success {
                        if let Some(error) = response.error {
                            return Err(SidecarError::AnalysisError(error));
                        }
                    }

                    return Ok(response);
                }
                Ok(None) => {
                    log::error!("Sidecar closed unexpectedly");
                    return Err(SidecarError::CommunicationError(
                        "Sidecar closed".to_string(),
                    ));
                }
                Err(_) => {
                    log::error!("Timeout waiting for sidecar response");
                    return Err(SidecarError::CommunicationError("Timeout".to_string()));
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
        matches.sort_by(|a, b| b.0.cmp(&a.0));

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
}
