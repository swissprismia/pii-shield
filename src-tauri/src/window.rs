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
        get_active_window_linux()
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

#[cfg(target_os = "linux")]
fn get_active_window_linux() -> Option<WindowInfo> {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt};

    // Connect to the X server
    let (conn, screen_num) = x11rb::connect(None).ok()?;
    let screen = &conn.setup().roots[screen_num];
    let root = screen.root;

    // Get the _NET_ACTIVE_WINDOW atom
    let active_window_atom = conn
        .intern_atom(false, b"_NET_ACTIVE_WINDOW")
        .ok()?
        .reply()
        .ok()?
        .atom;

    // Get the active window ID
    let active_window_reply = conn
        .get_property(false, root, active_window_atom, AtomEnum::WINDOW, 0, 1)
        .ok()?
        .reply()
        .ok()?;

    if active_window_reply.value.len() < 4 {
        return None;
    }

    let window_id = u32::from_ne_bytes([
        active_window_reply.value[0],
        active_window_reply.value[1],
        active_window_reply.value[2],
        active_window_reply.value[3],
    ]);

    if window_id == 0 {
        return None;
    }

    // Get _NET_WM_NAME atom for UTF-8 window title
    let net_wm_name_atom = conn
        .intern_atom(false, b"_NET_WM_NAME")
        .ok()?
        .reply()
        .ok()?
        .atom;

    let utf8_string_atom = conn
        .intern_atom(false, b"UTF8_STRING")
        .ok()?
        .reply()
        .ok()?
        .atom;

    // Try to get _NET_WM_NAME first (UTF-8)
    let title = conn
        .get_property(
            false,
            window_id,
            net_wm_name_atom,
            utf8_string_atom,
            0,
            1024,
        )
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .and_then(|reply| {
            if !reply.value.is_empty() {
                String::from_utf8(reply.value).ok()
            } else {
                None
            }
        })
        .or_else(|| {
            // Fallback to WM_NAME
            conn.get_property(
                false,
                window_id,
                AtomEnum::WM_NAME,
                AtomEnum::STRING,
                0,
                1024,
            )
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .and_then(|reply| {
                if !reply.value.is_empty() {
                    String::from_utf8_lossy(&reply.value).into_owned().into()
                } else {
                    None
                }
            })
        })
        .unwrap_or_else(|| "Unknown".to_string());

    // Get _NET_WM_PID for process ID
    let net_wm_pid_atom = conn
        .intern_atom(false, b"_NET_WM_PID")
        .ok()?
        .reply()
        .ok()?
        .atom;

    let pid = conn
        .get_property(false, window_id, net_wm_pid_atom, AtomEnum::CARDINAL, 0, 1)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .and_then(|reply| {
            if reply.value.len() >= 4 {
                Some(u32::from_ne_bytes([
                    reply.value[0],
                    reply.value[1],
                    reply.value[2],
                    reply.value[3],
                ]))
            } else {
                None
            }
        });

    // Get application name from /proc/<pid>/comm or WM_CLASS
    let app_name = pid
        .and_then(|p| std::fs::read_to_string(format!("/proc/{}/comm", p)).ok())
        .map(|s| s.trim().to_string())
        .or_else(|| {
            // Fallback to WM_CLASS
            let wm_class_atom = conn
                .intern_atom(false, b"WM_CLASS")
                .ok()?
                .reply()
                .ok()?
                .atom;

            conn.get_property(false, window_id, wm_class_atom, AtomEnum::STRING, 0, 1024)
                .ok()
                .and_then(|cookie| cookie.reply().ok())
                .and_then(|reply| {
                    if !reply.value.is_empty() {
                        // WM_CLASS contains two null-separated strings: instance and class
                        // We want the class (second one)
                        let parts: Vec<&[u8]> = reply.value.split(|&b| b == 0).collect();
                        if parts.len() >= 2 && !parts[1].is_empty() {
                            String::from_utf8_lossy(parts[1]).into_owned().into()
                        } else if !parts.is_empty() && !parts[0].is_empty() {
                            String::from_utf8_lossy(parts[0]).into_owned().into()
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
        });

    Some(WindowInfo {
        title,
        app_name,
        process_id: pid,
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
