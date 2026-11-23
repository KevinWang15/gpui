//! Cross-platform single-instance application guard with local IPC forwarding and Windows
//! `AllowSetForegroundWindow` handoff.
//!
//! When an application starts, call [`SingleInstance::acquire`]:
//! - The **first** process receives [`SingleInstanceOutcome::Primary`], containing a
//!   [`SingleInstanceGuard`] and an async receiver of payloads forwarded by later launches.
//! - Any **subsequent** process connects to the running primary, grants it foreground focus
//!   permission on Windows (`AllowSetForegroundWindow`), forwards its payload (for example
//!   `"show"`, `"toggle"`, or a file path to open), and receives
//!   [`SingleInstanceOutcome::SecondaryForwarded`] so it can exit immediately.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Result;
use futures::channel::mpsc;

/// Maximum IPC payload size accepted from a secondary launch (64 KiB).
const MAX_IPC_LINE_BYTES: usize = 64 * 1024;

/// Result of attempting to acquire a single-instance lock for an application.
pub enum SingleInstanceOutcome {
    /// This process is the primary instance. Keep [`SingleInstanceGuard`] alive for the
    /// lifetime of the app and drain `messages` on GPUI's foreground executor.
    Primary {
        /// RAII guard that stops the listener and cleans up the socket/pipe on drop.
        guard: SingleInstanceGuard,
        /// Payloads forwarded by secondary launches.
        messages: mpsc::UnboundedReceiver<String>,
    },
    /// Another primary instance is already running and has received this process's payload
    /// (and foreground focus permission on Windows).
    SecondaryForwarded,
}

/// Active single-instance IPC listener guard; dropping it tears down the listener cleanly.
pub struct SingleInstanceGuard {
    stop: Arc<AtomicBool>,
    #[cfg(unix)]
    socket_path: std::path::PathBuf,
    #[cfg(target_os = "windows")]
    pipe_name: String,
}

impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        #[cfg(unix)]
        {
            // Wake the blocking accept loop and remove the socket file.
            let _ = std::os::unix::net::UnixStream::connect(&self.socket_path);
            let _ = std::fs::remove_file(&self.socket_path);
        }
        #[cfg(target_os = "windows")]
        {
            // Wake the blocking ConnectNamedPipe loop.
            let _ = windows_impl::wake_pipe(&self.pipe_name);
        }
    }
}

/// Cross-platform single-instance coordinator.
pub struct SingleInstance;

impl SingleInstance {
    /// Attempts to become the primary instance for `app_id`.
    ///
    /// If another instance is already running for the same `(user, app_id)`, forwards `payload`
    /// (newlines are replaced with spaces to keep the framing single-line) and returns
    /// [`SingleInstanceOutcome::SecondaryForwarded`].
    pub fn acquire(app_id: &str, payload: &str) -> Result<SingleInstanceOutcome> {
        let sanitized_payload = payload.replace(['\r', '\n'], " ");
        #[cfg(unix)]
        {
            unix_impl::acquire(app_id, &sanitized_payload)
        }
        #[cfg(target_os = "windows")]
        {
            windows_impl::acquire(app_id, &sanitized_payload)
        }
        #[cfg(not(any(unix, target_os = "windows")))]
        {
            let _ = (app_id, sanitized_payload);
            let (_tx, rx) = mpsc::unbounded();
            Ok(SingleInstanceOutcome::Primary {
                guard: SingleInstanceGuard {
                    stop: Arc::new(AtomicBool::new(false)),
                },
                messages: rx,
            })
        }
    }
}

fn fnv1a_32(input: &str) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for byte in input.as_bytes() {
        hash ^= *byte as u32;
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

fn sanitize_name(input: &str) -> String {
    let mut out: String = input
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') {
                c
            } else {
                '-'
            }
        })
        .take(32)
        .collect();
    if out.is_empty() {
        out.push_str("app");
    }
    out
}

#[cfg(unix)]
mod unix_impl {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::PathBuf;
    use std::time::Duration;

    fn socket_path_for(app_id: &str) -> PathBuf {
        let base = std::env::var_os("XDG_RUNTIME_DIR")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let user = std::env::var("USER").unwrap_or_else(|_| "user".to_owned());
        let safe_app = sanitize_name(app_id);
        let safe_user = sanitize_name(&user);
        let hash = fnv1a_32(&format!("{user}:{app_id}"));
        base.join(format!("gpui-{safe_app}-{safe_user}-{hash:08x}.sock"))
    }

    fn try_forward(path: &PathBuf, payload: &str) -> bool {
        let Ok(mut stream) = UnixStream::connect(path) else {
            return false;
        };
        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
        if writeln!(stream, "{payload}").is_err() {
            return false;
        }
        let _ = stream.flush();
        let mut ack = String::new();
        let _ = BufReader::new(stream.take(128)).read_line(&mut ack);
        true
    }

    pub(super) fn acquire(app_id: &str, payload: &str) -> Result<SingleInstanceOutcome> {
        let path = socket_path_for(app_id);
        if try_forward(&path, payload) {
            return Ok(SingleInstanceOutcome::SecondaryForwarded);
        }

        // Stale socket file from a previous crash: remove before binding.
        if path.exists() {
            let _ = std::fs::remove_file(&path);
        }
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let listener = match UnixListener::bind(&path) {
            Ok(listener) => listener,
            Err(_) if try_forward(&path, payload) => {
                return Ok(SingleInstanceOutcome::SecondaryForwarded);
            }
            Err(err) => return Err(err.into()),
        };

        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));

        let stop = Arc::new(AtomicBool::new(false));
        let stop_bg = stop.clone();
        let (tx, rx) = mpsc::unbounded();

        std::thread::Builder::new()
            .name(format!("gpui-single-instance-{}", sanitize_name(app_id)))
            .spawn(move || {
                for incoming in listener.incoming() {
                    if stop_bg.load(Ordering::Acquire) {
                        break;
                    }
                    let Ok(mut stream) = incoming else {
                        continue;
                    };
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                    let mut line = String::new();
                    let read_res = {
                        let mut reader = BufReader::new((&stream).take((MAX_IPC_LINE_BYTES + 1) as u64));
                        reader.read_line(&mut line)
                    };
                    if let Ok(n) = read_res
                        && n > 0
                        && line.len() <= MAX_IPC_LINE_BYTES
                    {
                        let trimmed = line.trim_end_matches(['\r', '\n']).to_owned();
                        let _ = tx.unbounded_send(trimmed);
                    }
                    let _ = writeln!(stream, "ok:{}", std::process::id());
                    let _ = stream.flush();
                }
            })?;

        Ok(SingleInstanceOutcome::Primary {
            guard: SingleInstanceGuard {
                stop,
                socket_path: path,
            },
            messages: rx,
        })
    }
}

#[cfg(target_os = "windows")]
mod windows_impl {
    use super::*;
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Foundation::{
        CloseHandle, ERROR_PIPE_CONNECTED, HANDLE, INVALID_HANDLE_VALUE,
    };
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_GENERIC_READ,
        FILE_GENERIC_WRITE, FILE_SHARE_NONE, OPEN_EXISTING, PIPE_ACCESS_DUPLEX, ReadFile,
        WriteFile,
    };
    use windows::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE,
        PIPE_TYPE_BYTE, PIPE_WAIT,
    };
    use windows::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow;
    use windows::core::PCWSTR;

    fn wide(s: &str) -> Vec<u16> {
        OsStr::new(s).encode_wide().chain([0]).collect()
    }

    fn pipe_name_for(app_id: &str) -> String {
        let user = std::env::var("USERNAME").unwrap_or_else(|_| "user".to_owned());
        let safe_app = sanitize_name(app_id);
        let safe_user = sanitize_name(&user);
        let hash = fnv1a_32(&format!("{user}:{app_id}"));
        format!(r"\\.\pipe\gpui-{safe_app}-{safe_user}-{hash:08x}")
    }

    pub(super) fn wake_pipe(pipe_name: &str) -> Result<()> {
        let wname = wide(pipe_name);
        let handle = unsafe {
            CreateFileW(
                PCWSTR(wname.as_ptr()),
                FILE_GENERIC_READ.0 | FILE_GENERIC_WRITE.0,
                FILE_SHARE_NONE,
                None,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                None,
            )
        }?;
        unsafe {
            let _ = CloseHandle(handle);
        }
        Ok(())
    }

    fn read_line_capped(handle: HANDLE) -> Option<String> {
        let mut buf = Vec::with_capacity(256);
        let mut chunk = [0u8; 256];
        loop {
            let mut read = 0u32;
            let ok = unsafe { ReadFile(handle, Some(&mut chunk), Some(&mut read), None) };
            if ok.is_err() || read == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..read as usize]);
            if buf.contains(&b'\n') || buf.len() > MAX_IPC_LINE_BYTES {
                break;
            }
        }
        if buf.len() > MAX_IPC_LINE_BYTES {
            return None;
        }
        let s = String::from_utf8_lossy(&buf);
        Some(
            s.lines()
                .next()
                .unwrap_or_default()
                .trim_end_matches('\r')
                .to_owned(),
        )
    }

    fn write_all(handle: HANDLE, bytes: &[u8]) -> bool {
        let mut written = 0u32;
        unsafe { WriteFile(handle, Some(bytes), Some(&mut written), None) }.is_ok()
    }

    fn try_forward(pipe_name: &str, payload: &str) -> bool {
        let wname = wide(pipe_name);
        let Ok(handle) = (unsafe {
            CreateFileW(
                PCWSTR(wname.as_ptr()),
                FILE_GENERIC_READ.0 | FILE_GENERIC_WRITE.0,
                FILE_SHARE_NONE,
                None,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                None,
            )
        }) else {
            return false;
        };

        // Step 1: Read primary PID greeting (`pid:<u32>\n`) and grant foreground permission.
        if let Some(greeting) = read_line_capped(handle)
            && let Some(pid_str) = greeting.strip_prefix("pid:")
            && let Ok(pid) = pid_str.trim().parse::<u32>()
        {
            unsafe {
                let _ = AllowSetForegroundWindow(pid);
            }
        }

        // Step 2: Send payload line.
        let line = format!("{payload}\n");
        let sent = write_all(handle, line.as_bytes());
        unsafe {
            let _ = CloseHandle(handle);
        }
        sent
    }

    fn create_pipe_instance(wname: &[u16], first: bool) -> windows::core::Result<HANDLE> {
        let mut open_mode = PIPE_ACCESS_DUPLEX;
        if first {
            open_mode |= FILE_FLAG_FIRST_PIPE_INSTANCE;
        }
        let handle = unsafe {
            CreateNamedPipeW(
                PCWSTR(wname.as_ptr()),
                open_mode,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                16,
                4096,
                4096,
                0,
                None,
            )
        };
        if handle == INVALID_HANDLE_VALUE || handle.is_invalid() {
            Err(windows::core::Error::from_thread())
        } else {
            Ok(handle)
        }
    }

    pub(super) fn acquire(app_id: &str, payload: &str) -> Result<SingleInstanceOutcome> {
        let pipe_name = pipe_name_for(app_id);
        let wname = wide(&pipe_name);

        let first_pipe = match create_pipe_instance(&wname, true) {
            Ok(handle) => handle,
            Err(_) => {
                if try_forward(&pipe_name, payload) {
                    return Ok(SingleInstanceOutcome::SecondaryForwarded);
                }
                // Brief retry in case the previous instance just exited.
                create_pipe_instance(&wname, true)?
            }
        };

        let stop = Arc::new(AtomicBool::new(false));
        let stop_bg = stop.clone();
        let (tx, rx) = mpsc::unbounded();
        let pipe_raw = first_pipe.0 as usize;

        std::thread::Builder::new()
            .name(format!("gpui-single-instance-{}", sanitize_name(app_id)))
            .spawn(move || {
                let handle = HANDLE(pipe_raw as *mut _);
                let pid_line = format!("pid:{}\n", std::process::id());
                loop {
                    let connected = unsafe { ConnectNamedPipe(handle, None) };
                    let is_connected = connected.is_ok()
                        || connected
                            .as_ref()
                            .err()
                            .is_some_and(|e| e.code() == ERROR_PIPE_CONNECTED.into());
                    if stop_bg.load(Ordering::Acquire) {
                        unsafe {
                            let _ = DisconnectNamedPipe(handle);
                        }
                        break;
                    }
                    if is_connected {
                        let _ = write_all(handle, pid_line.as_bytes());
                        if let Some(msg) = read_line_capped(handle) {
                            let _ = tx.unbounded_send(msg);
                        }
                        unsafe {
                            let _ = DisconnectNamedPipe(handle);
                        }
                    }
                }
                unsafe {
                    let _ = CloseHandle(handle);
                }
            })?;

        Ok(SingleInstanceOutcome::Primary {
            guard: SingleInstanceGuard { stop, pipe_name },
            messages: rx,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_instance_primary_and_secondary_round_trip() {
        let id = format!(
            "gpui-unit-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );

        let primary = SingleInstance::acquire(&id, "initial").expect("primary acquire");
        let (guard, mut messages) = match primary {
            SingleInstanceOutcome::Primary { guard, messages } => (guard, messages),
            SingleInstanceOutcome::SecondaryForwarded => {
                panic!("expected Primary for fresh app id")
            }
        };

        let second = SingleInstance::acquire(&id, "toggle --focus").expect("secondary acquire");
        assert!(matches!(second, SingleInstanceOutcome::SecondaryForwarded));

        let received = futures::executor::block_on(async {
            use futures::StreamExt;
            messages.next().await
        });
        assert_eq!(received.as_deref(), Some("toggle --focus"));

        drop(guard);

        // After primary drops, a new acquire becomes Primary again.
        let third = SingleInstance::acquire(&id, "reopen").expect("third acquire");
        assert!(matches!(third, SingleInstanceOutcome::Primary { .. }));
    }
}
