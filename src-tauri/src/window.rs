use serde::{Deserialize, Serialize};

/// Information about the active window
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowInfo {
    pub title: String,
    pub app_name: Option<String>,
    pub process_id: Option<u32>,
}

/// Get information about the currently active window
pub fn get_active_window() -> Option<WindowInfo> {
    #[cfg(target_os = "windows")]
    {
        get_active_window_windows()
    }

    #[cfg(target_os = "macos")]
    {
        get_active_window_macos()
    }

    #[cfg(target_os = "linux")]
    {
        // Linux support is out of scope for PoC
        None
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        None
    }
}

#[cfg(target_os = "windows")]
fn get_active_window_windows() -> Option<WindowInfo> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW};

    unsafe {
        let hwnd: HWND = GetForegroundWindow();
        if hwnd.0.is_null() {
            return None;
        }

        // Get window title
        let mut title_buf = [0u16; 512];
        let len = GetWindowTextW(hwnd, &mut title_buf);
        if len == 0 {
            return None;
        }

        let title = String::from_utf16_lossy(&title_buf[..len as usize]);

        Some(WindowInfo {
            title,
            app_name: None, // Would need additional API calls
            process_id: None,
        })
    }
}

#[cfg(target_os = "macos")]
fn get_active_window_macos() -> Option<WindowInfo> {
    use core_graphics::display::CGDisplay;

    // Note: Full implementation would require accessibility APIs
    // For PoC, we'll use a simplified approach

    // This is a placeholder - full implementation would use:
    // - CGWindowListCopyWindowInfo to get window info
    // - NSWorkspace to get app info
    // For now, return a basic response

    Some(WindowInfo {
        title: "Active Window".to_string(),
        app_name: Some("Unknown App".to_string()),
        process_id: None,
    })
}

/// Check if the active window is an AI assistant (ChatGPT, Claude, etc.)
pub fn is_ai_assistant_window(window_info: &WindowInfo) -> bool {
    let ai_indicators = [
        "chatgpt",
        "claude",
        "gemini",
        "copilot",
        "openai",
        "anthropic",
        "bard",
        "perplexity",
    ];

    let title_lower = window_info.title.to_lowercase();
    let app_name_lower = window_info
        .app_name
        .as_ref()
        .map(|s| s.to_lowercase())
        .unwrap_or_default();

    ai_indicators
        .iter()
        .any(|indicator| title_lower.contains(indicator) || app_name_lower.contains(indicator))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ai_assistant_detection() {
        let chatgpt_window = WindowInfo {
            title: "ChatGPT - Chrome".to_string(),
            app_name: Some("Google Chrome".to_string()),
            process_id: None,
        };
        assert!(is_ai_assistant_window(&chatgpt_window));

        let claude_window = WindowInfo {
            title: "Claude".to_string(),
            app_name: Some("Safari".to_string()),
            process_id: None,
        };
        assert!(is_ai_assistant_window(&claude_window));

        let notepad_window = WindowInfo {
            title: "Untitled - Notepad".to_string(),
            app_name: Some("Notepad".to_string()),
            process_id: None,
        };
        assert!(!is_ai_assistant_window(&notepad_window));
    }
}
