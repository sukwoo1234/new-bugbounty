use std::fs;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::common::AppPaths;
use crate::dashboard_data::collect_dashboard_snapshot;
use crate::json_utils::{html_escape, json_escape, url_encode};

/// A4: a client that opens the socket and then goes quiet must not be able to hold a slot
/// forever, and a slow reader must not wedge a writer.
const READ_TIMEOUT_SECS: u64 = 15;
const WRITE_TIMEOUT_SECS: u64 = 30;
/// Connections are handled on their own threads, so the number of them in flight is capped
/// rather than left to the peer.
const MAX_IN_FLIGHT_CONNECTIONS: usize = 64;

/// Serialises every handler that reads or writes the UI state files or spawns a job. Only the
/// read-only views (dashboard, file, entity) run concurrently, which is what A4 needs: one
/// stalled or slow request no longer blocks the rest of the dashboard.
static STATE_LOCK: Mutex<()> = Mutex::new(());

fn lock_state() -> MutexGuard<'static, ()> {
    STATE_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// Releases its slot even if the connection thread unwinds.
struct InFlightGuard(Arc<AtomicUsize>);

impl Drop for InFlightGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

pub(crate) fn run_ui_server(app_paths: &AppPaths, bind: &str) -> Result<(), String> {
    let listener = TcpListener::bind(bind).map_err(|e| format!("failed to bind '{bind}': {e}"))?;
    println!("[ui] listening on http://{bind}");
    println!(
        "[ui] endpoints: /healthz, /dashboard.json, /dashboard.html, /file?path=..., /control/status, /replay/status, /target/status, /target/build/status"
    );

    let paths = Arc::new(app_paths.clone());
    let in_flight = Arc::new(AtomicUsize::new(0));

    for stream in listener.incoming() {
        let mut stream = match stream {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[ui] accept error: {e}");
                continue;
            }
        };
        let _ = stream.set_read_timeout(Some(Duration::from_secs(READ_TIMEOUT_SECS)));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(WRITE_TIMEOUT_SECS)));

        if in_flight.load(Ordering::SeqCst) >= MAX_IN_FLIGHT_CONNECTIONS {
            let _ = write_response(
                &mut stream,
                "503 Service Unavailable",
                "text/plain; charset=utf-8",
                "too many connections\n",
            );
            continue;
        }
        in_flight.fetch_add(1, Ordering::SeqCst);
        let guard = InFlightGuard(Arc::clone(&in_flight));
        let paths = Arc::clone(&paths);
        if let Err(e) = thread::Builder::new()
            .name("ui-conn".to_string())
            .spawn(move || {
                let _guard = guard;
                if let Err(e) = handle_connection(&paths, &mut stream) {
                    eprintln!("[ui] request error: {e}");
                }
            })
        {
            eprintln!("[ui] failed to spawn connection thread: {e}");
        }
    }
    Ok(())
}

/// A4: the request head has to be read in full before anything is decided from it, so that a
/// head split across TCP segments does not lose its headers, and it has to be bounded so that a
/// client that never sends the blank line cannot make the server buffer without limit.
const MAX_REQUEST_HEAD_BYTES: usize = 32 * 1024;

struct RequestHead {
    method: String,
    path: String,
}

/// `Ok(None)` means the peer closed without sending a byte - a browser preconnect, not an
/// error worth logging.
fn read_request_head<R: Read>(reader: &mut R, max_bytes: usize) -> Result<Option<String>, String> {
    let mut head = Vec::new();
    let mut buf = [0u8; 1024];
    loop {
        if head_terminator(&head).is_some() {
            break;
        }
        if head.len() >= max_bytes {
            return Err(format!("request head exceeds {max_bytes} bytes"));
        }
        let read = reader
            .read(&mut buf)
            .map_err(|e| format!("failed to read request: {e}"))?;
        if read == 0 {
            if head.is_empty() {
                return Ok(None);
            }
            return Err("request head ended before the blank line".to_string());
        }
        head.extend_from_slice(&buf[..read]);
    }
    let end = head_terminator(&head).unwrap_or(head.len());
    Ok(Some(String::from_utf8_lossy(&head[..end]).into_owned()))
}

/// Returns the offset just past the blank line that ends the head, accepting both CRLF and the
/// bare-LF form some clients send.
fn head_terminator(head: &[u8]) -> Option<usize> {
    let crlf = head.windows(4).position(|w| w == b"\r\n\r\n").map(|i| i + 4);
    let lf = head.windows(2).position(|w| w == b"\n\n").map(|i| i + 2);
    match (crlf, lf) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

fn parse_request_head(head: &str) -> Option<RequestHead> {
    let first_line = head.lines().next()?;
    let mut parts = first_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts.next().unwrap_or("/").to_string();
    Some(RequestHead { method, path })
}

fn handle_connection(app_paths: &AppPaths, stream: &mut TcpStream) -> Result<(), String> {
    let Some(head) = read_request_head(stream, MAX_REQUEST_HEAD_BYTES)? else {
        return Ok(());
    };
    let Some(request) = parse_request_head(&head) else {
        return Ok(());
    };
    let method = request.method.as_str();
    let raw_path = request.path.as_str();

    if raw_path.starts_with("/control/") {
        return handle_control_route(app_paths, stream, method, raw_path);
    }
    if raw_path.starts_with("/replay/") {
        return handle_replay_route(app_paths, stream, method, raw_path);
    }
    if raw_path.starts_with("/target/") {
        return handle_target_route(app_paths, stream, method, raw_path);
    }

    if method != "GET" {
        return write_response(
            stream,
            "405 Method Not Allowed",
            "text/plain; charset=utf-8",
            "method not allowed\n",
        );
    }

    if raw_path.starts_with("/file?") {
        return handle_file_view(app_paths, stream, raw_path);
    }
    if let Some(asset_name) = raw_path.strip_prefix("/assets/") {
        return handle_asset_view(stream, asset_name);
    }
    if let Some(id) = raw_path.strip_prefix("/run/") {
        return handle_entity_view(app_paths, stream, "run", id);
    }
    if let Some(id) = raw_path.strip_prefix("/triage/") {
        return handle_entity_view(app_paths, stream, "triage", id);
    }
    if let Some(id) = raw_path.strip_prefix("/report/") {
        return handle_entity_view(app_paths, stream, "report", id);
    }
    if let Some(id) = raw_path.strip_prefix("/coverage/") {
        return handle_entity_view(app_paths, stream, "coverage", id);
    }

    match raw_path {
        "/healthz" => write_response(stream, "200 OK", "text/plain; charset=utf-8", "ok\n"),
        "/dashboard.json" => {
            let snap = collect_dashboard_snapshot(app_paths)?;
            let body = super::dashboard::render_dashboard_json(&snap);
            write_response(stream, "200 OK", "application/json; charset=utf-8", &body)
        }
        "/dashboard.html" | "/" => {
            let snap = collect_dashboard_snapshot(app_paths)?;
            let body = super::dashboard::render_dashboard_html(&snap);
            write_response(stream, "200 OK", "text/html; charset=utf-8", &body)
        }
        _ => write_response(
            stream,
            "404 Not Found",
            "text/plain; charset=utf-8",
            "not found\n",
        ),
    }
}

fn handle_control_route(
    app_paths: &AppPaths,
    stream: &mut TcpStream,
    method: &str,
    raw_path: &str,
) -> Result<(), String> {
    let _state = lock_state();
    match (method, raw_path.split('?').next().unwrap_or(raw_path)) {
        ("GET", "/control/status") => handle_control_status(app_paths, stream),
        ("POST", "/control/start") => handle_control_start(app_paths, stream, raw_path),
        ("POST", "/control/stop") => handle_control_stop(app_paths, stream),
        _ => write_response(
            stream,
            "404 Not Found",
            "text/plain; charset=utf-8",
            "not found\n",
        ),
    }
}

fn handle_replay_route(
    app_paths: &AppPaths,
    stream: &mut TcpStream,
    method: &str,
    raw_path: &str,
) -> Result<(), String> {
    let _state = lock_state();
    match (method, raw_path.split('?').next().unwrap_or(raw_path)) {
        ("GET", "/replay/status") => handle_replay_status(app_paths, stream),
        ("POST", "/replay/start") => handle_replay_start(app_paths, stream, raw_path),
        ("POST", "/replay/stop") => handle_replay_stop(app_paths, stream),
        _ => write_response(
            stream,
            "404 Not Found",
            "text/plain; charset=utf-8",
            "not found\n",
        ),
    }
}

fn handle_target_route(
    app_paths: &AppPaths,
    stream: &mut TcpStream,
    method: &str,
    raw_path: &str,
) -> Result<(), String> {
    let _state = lock_state();
    match (method, raw_path.split('?').next().unwrap_or(raw_path)) {
        ("GET", "/target/build/status") => handle_target_build_status(app_paths, stream),
        ("POST", "/target/build/start") => handle_target_build_start(app_paths, stream, raw_path),
        ("POST", "/target/build/stop") => handle_target_build_stop(app_paths, stream),
        ("GET", "/target/status") => handle_target_status(app_paths, stream),
        ("POST", "/target/prepare") => handle_target_prepare(app_paths, stream, raw_path),
        ("POST", "/target/stop") => handle_target_stop(app_paths, stream),
        _ => write_response(
            stream,
            "404 Not Found",
            "text/plain; charset=utf-8",
            "not found\n",
        ),
    }
}

fn handle_control_status(app_paths: &AppPaths, stream: &mut TcpStream) -> Result<(), String> {
    let state = read_control_state(app_paths)?;
    let running = state.pid.map(is_process_alive).unwrap_or(false);
    let mut body = String::new();
    body.push_str("{\"schema_version\":\"1.0\"");
    body.push_str(&format!(
        ",\"running\":{}",
        if running { "true" } else { "false" }
    ));
    if let Some(pid) = state.pid {
        body.push_str(&format!(",\"pid\":{pid}"));
    } else {
        body.push_str(",\"pid\":null");
    }
    if let Some(started_at) = state.started_at {
        body.push_str(&format!(",\"started_at\":{started_at}"));
    } else {
        body.push_str(",\"started_at\":null");
    }
    body.push_str(&format!(",\"duration_seconds\":{}", state.duration_seconds));
    body.push_str(&format!(",\"target\":\"{}\"", json_escape(&state.target)));
    body.push_str(&format!(",\"backend\":\"{}\"", json_escape(&state.backend)));
    body.push_str(&format!(
        ",\"log_file\":\"{}\"",
        json_escape(&state.log_file)
    ));
    body.push('}');
    write_response(stream, "200 OK", "application/json; charset=utf-8", &body)
}

fn handle_replay_status(app_paths: &AppPaths, stream: &mut TcpStream) -> Result<(), String> {
    let state = read_replay_state(app_paths)?;
    let running = state.pid.map(is_process_alive).unwrap_or(false);
    let summary_path = if running {
        String::new()
    } else {
        extract_replay_summary_path(&state.log_file).unwrap_or_default()
    };
    let verdict = if running {
        String::new()
    } else {
        extract_replay_verdict(&state.log_file).unwrap_or_default()
    };
    let mut body = String::new();
    body.push_str("{\"schema_version\":\"1.0\"");
    body.push_str(&format!(
        ",\"running\":{}",
        if running { "true" } else { "false" }
    ));
    if let Some(pid) = state.pid {
        body.push_str(&format!(",\"pid\":{pid}"));
    } else {
        body.push_str(",\"pid\":null");
    }
    if let Some(started_at) = state.started_at {
        body.push_str(&format!(",\"started_at\":{started_at}"));
    } else {
        body.push_str(",\"started_at\":null");
    }
    body.push_str(&format!(",\"target\":\"{}\"", json_escape(&state.target)));
    body.push_str(&format!(",\"input\":\"{}\"", json_escape(&state.input)));
    body.push_str(&format!(
        ",\"log_file\":\"{}\"",
        json_escape(&state.log_file)
    ));
    body.push_str(&format!(
        ",\"summary_path\":\"{}\"",
        json_escape(&summary_path)
    ));
    body.push_str(&format!(",\"verdict\":\"{}\"", json_escape(&verdict)));
    body.push('}');
    write_response(stream, "200 OK", "application/json; charset=utf-8", &body)
}

fn handle_target_status(app_paths: &AppPaths, stream: &mut TcpStream) -> Result<(), String> {
    let mut state = read_target_prepare_state(app_paths)?;
    let running = state.pid.map(is_process_alive).unwrap_or(false);
    if !running
        && state.pid.is_some()
        && matches!(state.last_result.as_str(), "running" | "starting")
    {
        state.pid = None;
        state.started_at = None;
        if log_contains(&state.log_file, "[prepare-target] done") {
            state.last_result = "success".to_string();
            state.last_message = "prepare target completed".to_string();
        } else {
            state.last_result = "error".to_string();
            state.last_message = extract_target_error_message(&state.log_file)
                .unwrap_or_else(|| "target prepare failed".to_string());
        }
        write_target_prepare_state(app_paths, &state)?;
    }
    let meta_path = if running {
        String::new()
    } else {
        extract_log_value(&state.log_file, "meta: ").unwrap_or_default()
    };
    let downloaded_file = if running {
        String::new()
    } else {
        extract_log_value(&state.log_file, "file: ").unwrap_or_default()
    };
    let sha256 = if running {
        String::new()
    } else {
        extract_log_value(&state.log_file, "sha256: ").unwrap_or_default()
    };
    let mut body = String::new();
    body.push_str("{\"schema_version\":\"1.0\"");
    body.push_str(&format!(
        ",\"running\":{}",
        if running { "true" } else { "false" }
    ));
    if let Some(pid) = state.pid {
        body.push_str(&format!(",\"pid\":{pid}"));
    } else {
        body.push_str(",\"pid\":null");
    }
    if let Some(started_at) = state.started_at {
        body.push_str(&format!(",\"started_at\":{started_at}"));
    } else {
        body.push_str(",\"started_at\":null");
    }
    body.push_str(&format!(",\"target\":\"{}\"", json_escape(&state.target)));
    body.push_str(&format!(",\"version\":\"{}\"", json_escape(&state.version)));
    body.push_str(&format!(
        ",\"source_url\":\"{}\"",
        json_escape(&state.source_url)
    ));
    body.push_str(&format!(
        ",\"log_file\":\"{}\"",
        json_escape(&state.log_file)
    ));
    body.push_str(&format!(",\"meta_path\":\"{}\"", json_escape(&meta_path)));
    body.push_str(&format!(
        ",\"downloaded_file\":\"{}\"",
        json_escape(&downloaded_file)
    ));
    body.push_str(&format!(",\"sha256\":\"{}\"", json_escape(&sha256)));
    body.push_str(&format!(
        ",\"result\":\"{}\"",
        json_escape(if running {
            "running"
        } else {
            &state.last_result
        })
    ));
    body.push_str(&format!(
        ",\"message\":\"{}\"",
        json_escape(if running {
            "target prepare running"
        } else {
            &state.last_message
        })
    ));
    body.push('}');
    write_response(stream, "200 OK", "application/json; charset=utf-8", &body)
}

fn handle_target_build_status(app_paths: &AppPaths, stream: &mut TcpStream) -> Result<(), String> {
    let mut state = read_target_build_state(app_paths)?;
    let running = state.pid.map(is_process_alive).unwrap_or(false);
    if !running
        && state.pid.is_some()
        && matches!(state.last_result.as_str(), "running" | "starting")
    {
        state.pid = None;
        state.started_at = None;
        if log_contains(&state.log_file, "[target-build] done") {
            state.last_result = "success".to_string();
            state.last_message = "target build completed".to_string();
        } else {
            state.last_result = "error".to_string();
            state.last_message = extract_target_error_message(&state.log_file)
                .unwrap_or_else(|| "target build failed".to_string());
        }
        write_target_build_state(app_paths, &state)?;
    }
    let build_dir = if running {
        String::new()
    } else {
        extract_log_value(&state.log_file, "build_dir: ").unwrap_or_default()
    };
    let artifact = if running {
        String::new()
    } else {
        extract_log_value(&state.log_file, "artifact: ").unwrap_or_default()
    };
    let mut body = String::new();
    body.push_str("{\"schema_version\":\"1.0\"");
    body.push_str(&format!(
        ",\"running\":{}",
        if running { "true" } else { "false" }
    ));
    if let Some(pid) = state.pid {
        body.push_str(&format!(",\"pid\":{pid}"));
    } else {
        body.push_str(",\"pid\":null");
    }
    if let Some(started_at) = state.started_at {
        body.push_str(&format!(",\"started_at\":{started_at}"));
    } else {
        body.push_str(",\"started_at\":null");
    }
    body.push_str(&format!(",\"target\":\"{}\"", json_escape(&state.target)));
    body.push_str(&format!(",\"version\":\"{}\"", json_escape(&state.version)));
    body.push_str(&format!(
        ",\"log_file\":\"{}\"",
        json_escape(&state.log_file)
    ));
    body.push_str(&format!(",\"build_dir\":\"{}\"", json_escape(&build_dir)));
    body.push_str(&format!(",\"artifact\":\"{}\"", json_escape(&artifact)));
    body.push_str(&format!(
        ",\"result\":\"{}\"",
        json_escape(if running {
            "running"
        } else {
            &state.last_result
        })
    ));
    body.push_str(&format!(
        ",\"message\":\"{}\"",
        json_escape(if running {
            "target build running"
        } else {
            &state.last_message
        })
    ));
    body.push('}');
    write_response(stream, "200 OK", "application/json; charset=utf-8", &body)
}

fn handle_replay_start(
    app_paths: &AppPaths,
    stream: &mut TcpStream,
    raw_path: &str,
) -> Result<(), String> {
    let current = read_replay_state(app_paths)?;
    if let Some(pid) = current.pid {
        if is_process_alive(pid) {
            return write_response(
                stream,
                "409 Conflict",
                "application/json; charset=utf-8",
                "{\"error\":\"replay_already_running\"}",
            );
        }
    }

    let input_value = extract_query_param(raw_path, "input").unwrap_or("");
    if input_value.is_empty() {
        return write_response(
            stream,
            "400 Bad Request",
            "application/json; charset=utf-8",
            "{\"error\":\"missing_input\"}",
        );
    }
    let decoded_input =
        url_decode(input_value).ok_or_else(|| "invalid url encoding".to_string())?;
    let input_path = PathBuf::from(&decoded_input);
    if !input_path.exists() || !input_path.is_file() {
        return write_response(
            stream,
            "400 Bad Request",
            "application/json; charset=utf-8",
            "{\"error\":\"invalid_input\"}",
        );
    }

    let target = match extract_query_param(raw_path, "target").unwrap_or("onnx") {
        "onnx" | "gguf" | "safetensors" => {
            extract_query_param(raw_path, "target").unwrap_or("onnx")
        }
        _ => {
            return write_response(
                stream,
                "400 Bad Request",
                "application/json; charset=utf-8",
                "{\"error\":\"invalid_target\"}",
            )
        }
    };
    let repro_retries = extract_query_param(raw_path, "repro_retries").unwrap_or("3");
    let timeout_sec = positive_timeout_sec_or_default(extract_query_param(raw_path, "timeout_sec"));

    let replay_dir = app_paths.data_dir.join("ui-replay");
    fs::create_dir_all(&replay_dir).map_err(|e| {
        format!(
            "failed to create replay dir '{}': {e}",
            replay_dir.display()
        )
    })?;
    let log_path = replay_dir.join("replay.log");
    let log_file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&log_path)
        .map_err(|e| format!("failed to open replay log '{}': {e}", log_path.display()))?;
    let log_file_err = log_file
        .try_clone()
        .map_err(|e| format!("failed to clone replay log handle: {e}"))?;

    let cwd = std::env::current_dir().map_err(|e| format!("failed to get current dir: {e}"))?;
    let mut cmd = Command::new(tool_binary_path()?);
    cmd.current_dir(&cwd)
        .arg("triage")
        .arg("--target")
        .arg(target)
        .arg("--input")
        .arg(&input_path)
        .arg("--repro-retries")
        .arg(repro_retries)
        .arg("--timeout-sec")
        .arg(timeout_sec)
        .stdout(Stdio::from(log_file))
        .stderr(Stdio::from(log_file_err));
    let child = cmd
        .spawn()
        .map_err(|e| format!("failed to start replay triage: {e}"))?;
    let pid = child.id();

    let state = ReplayState {
        pid: Some(pid),
        started_at: Some(now_unix()),
        target: target.to_string(),
        input: input_path.display().to_string(),
        log_file: log_path.display().to_string(),
    };
    write_replay_state(app_paths, &state)?;

    let body = format!(
        "{{\"ok\":true,\"running\":true,\"pid\":{},\"target\":\"{}\",\"input\":\"{}\"}}",
        pid,
        json_escape(target),
        json_escape(&state.input),
    );
    write_response(stream, "200 OK", "application/json; charset=utf-8", &body)
}

fn handle_target_prepare(
    app_paths: &AppPaths,
    stream: &mut TcpStream,
    raw_path: &str,
) -> Result<(), String> {
    let current = read_target_prepare_state(app_paths)?;
    if let Some(pid) = current.pid {
        if is_process_alive(pid) {
            return write_response(
                stream,
                "409 Conflict",
                "application/json; charset=utf-8",
                "{\"error\":\"target_prepare_already_running\"}",
            );
        }
    }

    let target = match extract_query_param(raw_path, "target").unwrap_or("onnx") {
        "onnx" | "gguf" | "safetensors" => {
            extract_query_param(raw_path, "target").unwrap_or("onnx")
        }
        _ => {
            return write_response(
                stream,
                "400 Bad Request",
                "application/json; charset=utf-8",
                "{\"error\":\"invalid_target\"}",
            )
        }
    };
    let version = match extract_query_param(raw_path, "version") {
        Some(value) => url_decode(value).ok_or_else(|| "invalid version encoding".to_string())?,
        None => String::new(),
    };
    let source_url = match extract_query_param(raw_path, "source_url") {
        Some(value) => {
            url_decode(value).ok_or_else(|| "invalid source url encoding".to_string())?
        }
        None => String::new(),
    };

    let target_dir = app_paths.data_dir.join("ui-target");
    fs::create_dir_all(&target_dir).map_err(|e| {
        format!(
            "failed to create target dir '{}': {e}",
            target_dir.display()
        )
    })?;
    let log_path = target_dir.join("prepare-target.log");
    let log_file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&log_path)
        .map_err(|e| format!("failed to open target log '{}': {e}", log_path.display()))?;
    let log_file_err = log_file
        .try_clone()
        .map_err(|e| format!("failed to clone target log handle: {e}"))?;

    let cwd = std::env::current_dir().map_err(|e| format!("failed to get current dir: {e}"))?;
    let mut cmd = Command::new(tool_binary_path()?);
    cmd.current_dir(&cwd)
        .arg("prepare-target")
        .arg("--target")
        .arg(target);
    if !version.trim().is_empty() {
        cmd.arg("--version").arg(version.trim());
    }
    if !source_url.trim().is_empty() {
        cmd.arg("--source-url").arg(source_url.trim());
    }
    cmd.stdout(Stdio::from(log_file))
        .stderr(Stdio::from(log_file_err));

    let child = cmd
        .spawn()
        .map_err(|e| format!("failed to spawn target prepare command: {e}"))?;

    let state = TargetPrepareState {
        pid: Some(child.id()),
        started_at: Some(now_unix()),
        target: target.to_string(),
        version,
        source_url,
        log_file: log_path.display().to_string(),
        last_result: "running".to_string(),
        last_message: "target prepare running".to_string(),
    };
    write_target_prepare_state(app_paths, &state)?;

    let body = format!(
        "{{\"ok\":true,\"pid\":{},\"target\":\"{}\",\"version\":\"{}\"}}",
        child.id(),
        json_escape(target),
        json_escape(&state.version)
    );
    write_response(stream, "200 OK", "application/json; charset=utf-8", &body)
}

fn handle_target_build_start(
    app_paths: &AppPaths,
    stream: &mut TcpStream,
    raw_path: &str,
) -> Result<(), String> {
    let current = read_target_build_state(app_paths)?;
    if let Some(pid) = current.pid {
        if is_process_alive(pid) {
            return write_response(
                stream,
                "409 Conflict",
                "application/json; charset=utf-8",
                "{\"error\":\"target_build_already_running\"}",
            );
        }
    }

    let target = match extract_query_param(raw_path, "target").unwrap_or("gguf") {
        "gguf" | "onnx" | "safetensors" => {
            extract_query_param(raw_path, "target").unwrap_or("gguf")
        }
        _ => {
            return write_response(
                stream,
                "400 Bad Request",
                "application/json; charset=utf-8",
                "{\"error\":\"invalid_target\"}",
            )
        }
    };
    let version = match extract_query_param(raw_path, "version") {
        Some(value) => url_decode(value).ok_or_else(|| "invalid version encoding".to_string())?,
        None => latest_prepared_version(app_paths, target).unwrap_or_default(),
    };

    let build_dir = app_paths.data_dir.join("ui-target-build");
    fs::create_dir_all(&build_dir).map_err(|e| {
        format!(
            "failed to create target build dir '{}': {e}",
            build_dir.display()
        )
    })?;
    let log_path = build_dir.join("target-build.log");
    let log_file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&log_path)
        .map_err(|e| {
            format!(
                "failed to open target build log '{}': {e}",
                log_path.display()
            )
        })?;
    let log_file_err = log_file
        .try_clone()
        .map_err(|e| format!("failed to clone target build log handle: {e}"))?;

    let cwd = std::env::current_dir().map_err(|e| format!("failed to get current dir: {e}"))?;
    let mut cmd = Command::new("bash");
    cmd.current_dir(&cwd)
        .arg("scripts/build_prepared_target.sh")
        .arg(&app_paths.data_dir)
        .arg(target)
        .arg(&version)
        .stdout(Stdio::from(log_file))
        .stderr(Stdio::from(log_file_err));

    let child = cmd
        .spawn()
        .map_err(|e| format!("failed to spawn target build command: {e}"))?;

    let state = TargetBuildState {
        pid: Some(child.id()),
        started_at: Some(now_unix()),
        target: target.to_string(),
        version,
        log_file: log_path.display().to_string(),
        last_result: "running".to_string(),
        last_message: "target build running".to_string(),
    };
    write_target_build_state(app_paths, &state)?;

    let body = format!(
        "{{\"ok\":true,\"pid\":{},\"target\":\"{}\",\"version\":\"{}\"}}",
        child.id(),
        json_escape(target),
        json_escape(&state.version)
    );
    write_response(stream, "200 OK", "application/json; charset=utf-8", &body)
}

fn handle_replay_stop(app_paths: &AppPaths, stream: &mut TcpStream) -> Result<(), String> {
    let mut state = read_replay_state(app_paths)?;
    let mut was_running = false;
    if let Some(pid) = state.pid {
        if is_process_alive(pid) {
            was_running = true;
            let status = Command::new("kill")
                .arg("-TERM")
                .arg(pid.to_string())
                .status()
                .map_err(|e| format!("failed to stop replay pid {pid}: {e}"))?;
            if !status.success() {
                return write_response(
                    stream,
                    "500 Internal Server Error",
                    "application/json; charset=utf-8",
                    "{\"error\":\"failed_to_stop_replay\"}",
                );
            }
        }
    }
    state.pid = None;
    write_replay_state(app_paths, &state)?;
    let body = format!(
        "{{\"ok\":true,\"running\":false,\"stopped\":{}}}",
        if was_running { "true" } else { "false" }
    );
    write_response(stream, "200 OK", "application/json; charset=utf-8", &body)
}

fn handle_target_stop(app_paths: &AppPaths, stream: &mut TcpStream) -> Result<(), String> {
    let mut state = read_target_prepare_state(app_paths)?;
    if let Some(pid) = state.pid {
        if is_process_alive(pid) {
            let status = Command::new("kill")
                .arg(pid.to_string())
                .status()
                .map_err(|e| format!("failed to signal target prepare pid {pid}: {e}"))?;
            if !status.success() {
                return write_response(
                    stream,
                    "500 Internal Server Error",
                    "application/json; charset=utf-8",
                    "{\"error\":\"failed_to_stop_target_prepare\"}",
                );
            }
        }
    }
    state.pid = None;
    state.started_at = None;
    state.last_result = "stopped".to_string();
    state.last_message = "target prepare stopped".to_string();
    write_target_prepare_state(app_paths, &state)?;
    let body = format!(
        "{{\"ok\":true,\"target\":\"{}\"}}",
        json_escape(&state.target)
    );
    write_response(stream, "200 OK", "application/json; charset=utf-8", &body)
}

fn handle_target_build_stop(app_paths: &AppPaths, stream: &mut TcpStream) -> Result<(), String> {
    let mut state = read_target_build_state(app_paths)?;
    if let Some(pid) = state.pid {
        if is_process_alive(pid) {
            let status = Command::new("kill")
                .arg(pid.to_string())
                .status()
                .map_err(|e| format!("failed to signal target build pid {pid}: {e}"))?;
            if !status.success() {
                return write_response(
                    stream,
                    "500 Internal Server Error",
                    "application/json; charset=utf-8",
                    "{\"error\":\"failed_to_stop_target_build\"}",
                );
            }
        }
    }
    state.pid = None;
    state.started_at = None;
    state.last_result = "stopped".to_string();
    state.last_message = "target build stopped".to_string();
    write_target_build_state(app_paths, &state)?;
    let body = format!(
        "{{\"ok\":true,\"target\":\"{}\"}}",
        json_escape(&state.target)
    );
    write_response(stream, "200 OK", "application/json; charset=utf-8", &body)
}

fn handle_control_start(
    app_paths: &AppPaths,
    stream: &mut TcpStream,
    raw_path: &str,
) -> Result<(), String> {
    let current = read_control_state(app_paths)?;
    if let Some(pid) = current.pid {
        if is_process_alive(pid) {
            return write_response(
                stream,
                "409 Conflict",
                "application/json; charset=utf-8",
                "{\"error\":\"already_running\"}",
            );
        }
    }

    let target = extract_query_param(raw_path, "target").unwrap_or("onnx");
    let backend = extract_query_param(raw_path, "backend").unwrap_or("local-harness");
    let duration_seconds = extract_query_param(raw_path, "duration_seconds").unwrap_or("3600");
    let workers = extract_query_param(raw_path, "workers").unwrap_or("2");
    let timeout_sec = positive_timeout_sec_or_default(extract_query_param(raw_path, "timeout_sec"));
    let restart_limit = extract_query_param(raw_path, "restart_limit").unwrap_or("1");

    if !matches!(target, "onnx" | "gguf" | "safetensors") {
        return write_response(
            stream,
            "400 Bad Request",
            "application/json; charset=utf-8",
            "{\"error\":\"invalid_target\"}",
        );
    }
    if !matches!(backend, "local-harness" | "aflpp" | "libfuzzer") {
        return write_response(
            stream,
            "400 Bad Request",
            "application/json; charset=utf-8",
            "{\"error\":\"invalid_backend\"}",
        );
    }

    let control_dir = app_paths.data_dir.join("ui-control");
    fs::create_dir_all(&control_dir).map_err(|e| {
        format!(
            "failed to create control dir '{}': {e}",
            control_dir.display()
        )
    })?;
    let log_path = control_dir.join("run-backend-loop.log");
    let log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .map_err(|e| format!("failed to open control log '{}': {e}", log_path.display()))?;
    let log_file_err = log_file
        .try_clone()
        .map_err(|e| format!("failed to clone control log handle: {e}"))?;

    let cwd = std::env::current_dir().map_err(|e| format!("failed to get current dir: {e}"))?;
    let tool_bin =
        std::env::current_exe().map_err(|e| format!("failed to resolve current exe: {e}"))?;
    let corpus_dir = app_paths.seeds_dir.join(target);
    let mut cmd = Command::new("bash");
    cmd.arg("scripts/run_backend_loop.sh")
        .current_dir(&cwd)
        .env("WORKDIR", cwd.as_os_str())
        .env("DATA_DIR", app_paths.data_dir.as_os_str())
        .env("TOOL_BIN", tool_bin.as_os_str())
        .env("TARGET", target)
        .env("BACKEND", backend)
        .env("CORPUS_DIR", corpus_dir.as_os_str())
        .env("DURATION_SECONDS", duration_seconds)
        .env("WORKERS", workers)
        .env("TIMEOUT_SEC", timeout_sec)
        .env("RESTART_LIMIT", restart_limit)
        .env("LOG_DIR", control_dir.as_os_str())
        .stdout(Stdio::from(log_file))
        .stderr(Stdio::from(log_file_err));
    if let Some(max_jobs) = extract_query_param(raw_path, "max_jobs") {
        cmd.env("MAX_JOBS", max_jobs);
    }
    let child = cmd
        .spawn()
        .map_err(|e| format!("failed to start run loop script: {e}"))?;
    let pid = child.id();

    let state = ControlState {
        pid: Some(pid),
        started_at: Some(now_unix()),
        duration_seconds: duration_seconds.parse::<u64>().unwrap_or(3600),
        target: target.to_string(),
        backend: backend.to_string(),
        log_file: log_path.display().to_string(),
    };
    write_control_state(app_paths, &state)?;

    let body = format!(
        "{{\"ok\":true,\"running\":true,\"pid\":{},\"target\":\"{}\",\"backend\":\"{}\"}}",
        pid,
        json_escape(target),
        json_escape(backend),
    );
    write_response(stream, "200 OK", "application/json; charset=utf-8", &body)
}

fn handle_control_stop(app_paths: &AppPaths, stream: &mut TcpStream) -> Result<(), String> {
    let mut state = read_control_state(app_paths)?;
    let mut was_running = false;
    if let Some(pid) = state.pid {
        if is_process_alive(pid) {
            was_running = true;
            let status = Command::new("kill")
                .arg("-TERM")
                .arg(pid.to_string())
                .status()
                .map_err(|e| format!("failed to stop pid {pid}: {e}"))?;
            if !status.success() {
                return write_response(
                    stream,
                    "500 Internal Server Error",
                    "application/json; charset=utf-8",
                    "{\"error\":\"failed_to_stop\"}",
                );
            }
        }
    }
    state.pid = None;
    write_control_state(app_paths, &state)?;
    let body = format!(
        "{{\"ok\":true,\"running\":false,\"stopped\":{}}}",
        if was_running { "true" } else { "false" }
    );
    write_response(stream, "200 OK", "application/json; charset=utf-8", &body)
}

struct ControlState {
    pid: Option<u32>,
    started_at: Option<u64>,
    duration_seconds: u64,
    target: String,
    backend: String,
    log_file: String,
}

struct ReplayState {
    pid: Option<u32>,
    started_at: Option<u64>,
    target: String,
    input: String,
    log_file: String,
}

struct TargetPrepareState {
    pid: Option<u32>,
    started_at: Option<u64>,
    target: String,
    version: String,
    source_url: String,
    log_file: String,
    last_result: String,
    last_message: String,
}

struct TargetBuildState {
    pid: Option<u32>,
    started_at: Option<u64>,
    target: String,
    version: String,
    log_file: String,
    last_result: String,
    last_message: String,
}

fn read_control_state(app_paths: &AppPaths) -> Result<ControlState, String> {
    let state_path = app_paths.data_dir.join("ui-control").join("control.state");
    if !state_path.exists() {
        return Ok(ControlState {
            pid: None,
            started_at: None,
            duration_seconds: 3600,
            target: "onnx".to_string(),
            backend: "local-harness".to_string(),
            log_file: app_paths
                .data_dir
                .join("ui-control")
                .join("run-backend-loop.log")
                .display()
                .to_string(),
        });
    }
    let content = fs::read_to_string(&state_path).map_err(|e| {
        format!(
            "failed to read control state '{}': {e}",
            state_path.display()
        )
    })?;
    let mut out = ControlState {
        pid: None,
        started_at: None,
        duration_seconds: 3600,
        target: "onnx".to_string(),
        backend: "local-harness".to_string(),
        log_file: app_paths
            .data_dir
            .join("ui-control")
            .join("run-backend-loop.log")
            .display()
            .to_string(),
    };
    for line in content.lines() {
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        match k {
            "pid" => out.pid = v.parse::<u32>().ok(),
            "started_at" => out.started_at = v.parse::<u64>().ok(),
            "duration_seconds" => out.duration_seconds = v.parse::<u64>().unwrap_or(3600),
            "target" => out.target = v.to_string(),
            "backend" => out.backend = v.to_string(),
            "log_file" => out.log_file = v.to_string(),
            _ => {}
        }
    }
    Ok(out)
}

fn read_replay_state(app_paths: &AppPaths) -> Result<ReplayState, String> {
    let state_path = app_paths.data_dir.join("ui-replay").join("replay.state");
    if !state_path.exists() {
        return Ok(ReplayState {
            pid: None,
            started_at: None,
            target: "onnx".to_string(),
            input: String::new(),
            log_file: app_paths
                .data_dir
                .join("ui-replay")
                .join("replay.log")
                .display()
                .to_string(),
        });
    }
    let content = fs::read_to_string(&state_path).map_err(|e| {
        format!(
            "failed to read replay state '{}': {e}",
            state_path.display()
        )
    })?;
    let mut out = ReplayState {
        pid: None,
        started_at: None,
        target: "onnx".to_string(),
        input: String::new(),
        log_file: app_paths
            .data_dir
            .join("ui-replay")
            .join("replay.log")
            .display()
            .to_string(),
    };
    for line in content.lines() {
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        match k {
            "pid" => out.pid = v.parse::<u32>().ok(),
            "started_at" => out.started_at = v.parse::<u64>().ok(),
            "target" => out.target = v.to_string(),
            "input" => out.input = v.to_string(),
            "log_file" => out.log_file = v.to_string(),
            _ => {}
        }
    }
    Ok(out)
}

fn read_target_prepare_state(app_paths: &AppPaths) -> Result<TargetPrepareState, String> {
    let state_path = app_paths
        .data_dir
        .join("ui-target")
        .join("prepare-target.state");
    if !state_path.exists() {
        return Ok(TargetPrepareState {
            pid: None,
            started_at: None,
            target: "onnx".to_string(),
            version: String::new(),
            source_url: String::new(),
            log_file: app_paths
                .data_dir
                .join("ui-target")
                .join("prepare-target.log")
                .display()
                .to_string(),
            last_result: "idle".to_string(),
            last_message: "ready".to_string(),
        });
    }
    let content = fs::read_to_string(&state_path).map_err(|e| {
        format!(
            "failed to read target prepare state '{}': {e}",
            state_path.display()
        )
    })?;
    let mut out = TargetPrepareState {
        pid: None,
        started_at: None,
        target: "onnx".to_string(),
        version: String::new(),
        source_url: String::new(),
        log_file: app_paths
            .data_dir
            .join("ui-target")
            .join("prepare-target.log")
            .display()
            .to_string(),
        last_result: "idle".to_string(),
        last_message: "ready".to_string(),
    };
    for line in content.lines() {
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        match k {
            "pid" => out.pid = v.parse::<u32>().ok(),
            "started_at" => out.started_at = v.parse::<u64>().ok(),
            "target" => out.target = v.to_string(),
            "version" => out.version = v.to_string(),
            "source_url" => out.source_url = v.to_string(),
            "log_file" => out.log_file = v.to_string(),
            "last_result" => out.last_result = v.to_string(),
            "last_message" => out.last_message = v.to_string(),
            _ => {}
        }
    }
    Ok(out)
}

fn read_target_build_state(app_paths: &AppPaths) -> Result<TargetBuildState, String> {
    let state_path = app_paths
        .data_dir
        .join("ui-target-build")
        .join("target-build.state");
    if !state_path.exists() {
        return Ok(TargetBuildState {
            pid: None,
            started_at: None,
            target: "gguf".to_string(),
            version: String::new(),
            log_file: app_paths
                .data_dir
                .join("ui-target-build")
                .join("target-build.log")
                .display()
                .to_string(),
            last_result: "idle".to_string(),
            last_message: "ready".to_string(),
        });
    }
    let content = fs::read_to_string(&state_path).map_err(|e| {
        format!(
            "failed to read target build state '{}': {e}",
            state_path.display()
        )
    })?;
    let mut out = TargetBuildState {
        pid: None,
        started_at: None,
        target: "gguf".to_string(),
        version: String::new(),
        log_file: app_paths
            .data_dir
            .join("ui-target-build")
            .join("target-build.log")
            .display()
            .to_string(),
        last_result: "idle".to_string(),
        last_message: "ready".to_string(),
    };
    for line in content.lines() {
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        match k {
            "pid" => out.pid = v.parse::<u32>().ok(),
            "started_at" => out.started_at = v.parse::<u64>().ok(),
            "target" => out.target = v.to_string(),
            "version" => out.version = v.to_string(),
            "log_file" => out.log_file = v.to_string(),
            "last_result" => out.last_result = v.to_string(),
            "last_message" => out.last_message = v.to_string(),
            _ => {}
        }
    }
    Ok(out)
}

fn write_control_state(app_paths: &AppPaths, state: &ControlState) -> Result<(), String> {
    let control_dir = app_paths.data_dir.join("ui-control");
    fs::create_dir_all(&control_dir).map_err(|e| {
        format!(
            "failed to create control dir '{}': {e}",
            control_dir.display()
        )
    })?;
    let state_path = control_dir.join("control.state");
    let pid_text = state.pid.map(|v| v.to_string()).unwrap_or_default();
    let started_at_text = state.started_at.map(|v| v.to_string()).unwrap_or_default();
    let body = format!(
        "pid={}\nstarted_at={}\nduration_seconds={}\ntarget={}\nbackend={}\nlog_file={}\n",
        pid_text,
        started_at_text,
        state.duration_seconds,
        state.target,
        state.backend,
        state.log_file
    );
    fs::write(&state_path, body).map_err(|e| {
        format!(
            "failed to write control state '{}': {e}",
            state_path.display()
        )
    })
}

fn write_replay_state(app_paths: &AppPaths, state: &ReplayState) -> Result<(), String> {
    let replay_dir = app_paths.data_dir.join("ui-replay");
    fs::create_dir_all(&replay_dir).map_err(|e| {
        format!(
            "failed to create replay dir '{}': {e}",
            replay_dir.display()
        )
    })?;
    let state_path = replay_dir.join("replay.state");
    let pid_text = state.pid.map(|v| v.to_string()).unwrap_or_default();
    let started_at_text = state.started_at.map(|v| v.to_string()).unwrap_or_default();
    let body = format!(
        "pid={}\nstarted_at={}\ntarget={}\ninput={}\nlog_file={}\n",
        pid_text, started_at_text, state.target, state.input, state.log_file
    );
    fs::write(&state_path, body).map_err(|e| {
        format!(
            "failed to write replay state '{}': {e}",
            state_path.display()
        )
    })
}

fn write_target_prepare_state(
    app_paths: &AppPaths,
    state: &TargetPrepareState,
) -> Result<(), String> {
    let target_dir = app_paths.data_dir.join("ui-target");
    fs::create_dir_all(&target_dir).map_err(|e| {
        format!(
            "failed to create target dir '{}': {e}",
            target_dir.display()
        )
    })?;
    let state_path = target_dir.join("prepare-target.state");
    let pid_text = state.pid.map(|v| v.to_string()).unwrap_or_default();
    let started_at_text = state.started_at.map(|v| v.to_string()).unwrap_or_default();
    let body = format!(
        "pid={}\nstarted_at={}\ntarget={}\nversion={}\nsource_url={}\nlog_file={}\nlast_result={}\nlast_message={}\n",
        pid_text,
        started_at_text,
        state.target,
        state.version,
        state.source_url,
        state.log_file,
        state.last_result,
        state.last_message
    );
    fs::write(&state_path, body).map_err(|e| {
        format!(
            "failed to write target prepare state '{}': {e}",
            state_path.display()
        )
    })
}

fn write_target_build_state(app_paths: &AppPaths, state: &TargetBuildState) -> Result<(), String> {
    let target_dir = app_paths.data_dir.join("ui-target-build");
    fs::create_dir_all(&target_dir).map_err(|e| {
        format!(
            "failed to create target build dir '{}': {e}",
            target_dir.display()
        )
    })?;
    let state_path = target_dir.join("target-build.state");
    let pid_text = state.pid.map(|v| v.to_string()).unwrap_or_default();
    let started_at_text = state.started_at.map(|v| v.to_string()).unwrap_or_default();
    let body = format!(
        "pid={}\nstarted_at={}\ntarget={}\nversion={}\nlog_file={}\nlast_result={}\nlast_message={}\n",
        pid_text,
        started_at_text,
        state.target,
        state.version,
        state.log_file,
        state.last_result,
        state.last_message
    );
    fs::write(&state_path, body).map_err(|e| {
        format!(
            "failed to write target build state '{}': {e}",
            state_path.display()
        )
    })
}

fn extract_replay_summary_path(log_file: &str) -> Option<String> {
    let body = fs::read_to_string(log_file).ok()?;
    for line in body.lines().rev() {
        if let Some(path) = line.strip_prefix("summary: ") {
            return Some(path.trim().to_string());
        }
    }
    None
}

fn extract_replay_verdict(log_file: &str) -> Option<String> {
    let body = fs::read_to_string(log_file).ok()?;
    for line in body.lines().rev() {
        if let Some(v) = line.strip_prefix("verdict: ") {
            return Some(v.trim().to_string());
        }
    }
    None
}

fn extract_log_value(log_file: &str, prefix: &str) -> Option<String> {
    let body = fs::read_to_string(log_file).ok()?;
    for line in body.lines().rev() {
        if let Some(v) = line.strip_prefix(prefix) {
            return Some(v.trim().to_string());
        }
    }
    None
}

fn log_contains(log_file: &str, needle: &str) -> bool {
    fs::read_to_string(log_file)
        .map(|body| body.contains(needle))
        .unwrap_or(false)
}

fn extract_target_error_message(log_file: &str) -> Option<String> {
    let body = fs::read_to_string(log_file).ok()?;
    for line in body.lines().rev() {
        if let Some(msg) = line
            .split_once("prepare-target error: ")
            .map(|(_, v)| v.trim())
        {
            return Some(msg.to_string());
        }
        if line.contains("source URL") || line.contains("download failed") {
            return Some(line.trim().to_string());
        }
    }
    body.lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .map(|line| line.trim().to_string())
}

fn latest_prepared_version(app_paths: &AppPaths, target: &str) -> Option<String> {
    let target_name = target_storage_name(target)?;
    let target_root = app_paths.data_dir.join("targets").join(target_name);
    let mut versions = fs::read_dir(target_root)
        .ok()?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect::<Vec<_>>();
    versions.sort();
    versions.pop()
}

fn target_storage_name(target: &str) -> Option<&'static str> {
    match target {
        "gguf" => Some("llama.cpp"),
        "onnx" => Some("onnxruntime"),
        "safetensors" => Some("safetensors"),
        _ => None,
    }
}

fn tool_binary_path() -> Result<PathBuf, String> {
    if let Some(value) = std::env::var_os("TOOL_BIN") {
        let path = PathBuf::from(value);
        if !path.as_os_str().is_empty() {
            return Ok(path);
        }
    }
    std::env::current_exe().map_err(|e| format!("failed to resolve current executable: {e}"))
}

fn is_process_alive(pid: u32) -> bool {
    Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn handle_entity_view(
    app_paths: &AppPaths,
    stream: &mut TcpStream,
    kind: &str,
    id: &str,
) -> Result<(), String> {
    let (title, main_rel_path, extra_rel_paths) = match kind {
        "run" => (
            format!("Run Detail: {id}"),
            format!("./data/runs/{id}/status.json"),
            vec![format!("./data/runs/{id}/logs/backend-engine-w1.log")],
        ),
        "triage" => (
            format!("Triage Detail: {id}"),
            format!("./data/triage/{id}/summary.json"),
            vec![format!("./data/triage/{id}/attempt-1.log")],
        ),
        "report" => (
            format!("Report Detail: {id}"),
            format!("./data/reports/{id}/report.md"),
            vec![format!("./data/reports/{id}/meta.json")],
        ),
        "coverage" => (
            format!("Coverage Detail: {id}"),
            format!("./data/coverage/{id}/summary.json"),
            vec![],
        ),
        _ => {
            return write_response(
                stream,
                "404 Not Found",
                "text/plain; charset=utf-8",
                "not found\n",
            )
        }
    };

    let main_file = resolve_safe_data_path(&app_paths.data_dir, &main_rel_path)?;
    let main_content = fs::read_to_string(&main_file)
        .map_err(|e| format!("failed to read '{}': {e}", main_file.display()))?;

    let mut extras_html = String::new();
    for extra in extra_rel_paths {
        if let Ok(extra_path) = resolve_safe_data_path(&app_paths.data_dir, &extra) {
            if extra_path.exists() {
                let href = format!("/file?path={}", url_encode(&extra));
                extras_html.push_str(&format!(
                    "<li><a href=\"{}\">{}</a></li>",
                    html_escape(&href),
                    html_escape(&extra)
                ));
            }
        }
    }
    let extras_section = if extras_html.is_empty() {
        String::new()
    } else {
        format!("<h3>Related Files</h3><ul>{extras_html}</ul>")
    };

    let html = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{}</title><style>body{{font-family:Segoe UI,system-ui,sans-serif;margin:0;background:#eef2f7;color:#111827}}.wrap{{max-width:1100px;margin:0 auto;padding:14px}}.top{{position:sticky;top:0;background:#eef2f7cc;backdrop-filter:blur(4px);padding-bottom:10px}}.card{{background:#fff;border:1px solid #d4dee8;border-radius:12px;padding:14px;box-shadow:0 4px 12px rgba(16,24,40,.04)}}pre{{white-space:pre-wrap;word-break:break-word;background:#f8fafc;border:1px solid #e2e8f0;padding:12px;border-radius:10px;max-height:68vh;overflow:auto}}a{{color:#0b5563;text-decoration:none}}ul{{margin:8px 0 12px 16px;padding:0}}li{{margin:4px 0}}.meta{{font-size:12px;color:#4b5563}}@media (max-width:640px){{.wrap{{padding:10px}}h2{{font-size:18px}}pre{{font-size:11px}}}}</style></head><body><div class=\"wrap\"><div class=\"top\"><p><a href=\"/dashboard.html\">← dashboard</a></p><h2>{}</h2><p class=\"meta\">{}</p></div><div class=\"card\">{}<pre>{}</pre></div></div></body></html>",
        html_escape(&title),
        html_escape(&title),
        html_escape(&main_rel_path),
        extras_section,
        html_escape(&main_content)
    );

    write_response(stream, "200 OK", "text/html; charset=utf-8", &html)
}

fn handle_file_view(
    app_paths: &AppPaths,
    stream: &mut TcpStream,
    raw_path: &str,
) -> Result<(), String> {
    let Some(path_value) = extract_query_param(raw_path, "path") else {
        return write_response(
            stream,
            "400 Bad Request",
            "text/plain; charset=utf-8",
            "missing query parameter: path\n",
        );
    };
    let decoded = url_decode(path_value).ok_or_else(|| "invalid url encoding".to_string())?;
    let resolved = resolve_safe_data_path(&app_paths.data_dir, &decoded)?;
    let body = fs::read_to_string(&resolved)
        .map_err(|e| format!("failed to read '{}': {e}", resolved.display()))?;
    write_response(stream, "200 OK", "text/plain; charset=utf-8", &body)
}

fn handle_asset_view(stream: &mut TcpStream, asset_name: &str) -> Result<(), String> {
    let (rel_path, content_type) = match asset_name {
        "dashboard.css" => ("templates/assets/dashboard.css", "text/css; charset=utf-8"),
        _ => {
            return write_response(
                stream,
                "404 Not Found",
                "text/plain; charset=utf-8",
                "not found\n",
            )
        }
    };
    let body = fs::read_to_string(rel_path)
        .map_err(|e| format!("failed to read asset '{rel_path}': {e}"))?;
    write_response(stream, "200 OK", content_type, &body)
}

// A29: `timeout_sec=0` means "no time limit" to the shell wrapper and is rejected by the
// run/triage pipelines, so a query value that is not a positive integer falls back to the
// documented default instead of spawning a job that can only fail.
fn positive_timeout_sec_or_default(raw: Option<&str>) -> &str {
    match raw {
        Some(value) if value.parse::<u64>().is_ok_and(|seconds| seconds >= 1) => value,
        _ => "30",
    }
}

fn extract_query_param<'a>(path: &'a str, key: &str) -> Option<&'a str> {
    let query = path.split_once('?')?.1;
    for part in query.split('&') {
        let (k, v) = part.split_once('=')?;
        if k == key {
            return Some(v);
        }
    }
    None
}

fn url_decode(input: &str) -> Option<String> {
    let mut out = Vec::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let h1 = from_hex(bytes[i + 1])?;
                let h2 = from_hex(bytes[i + 2])?;
                out.push((h1 << 4) | h2);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

fn from_hex(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

fn resolve_safe_data_path(data_dir: &Path, requested: &str) -> Result<PathBuf, String> {
    let data_canon = fs::canonicalize(data_dir).map_err(|e| {
        format!(
            "failed to canonicalize data dir '{}': {e}",
            data_dir.display()
        )
    })?;

    let req_path = Path::new(requested);
    let candidate = if req_path.is_absolute() {
        req_path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| format!("failed to get current dir: {e}"))?
            .join(req_path)
    };
    let canon = fs::canonicalize(&candidate).map_err(|e| {
        format!(
            "failed to canonicalize requested path '{}': {e}",
            candidate.display()
        )
    })?;

    if !canon.starts_with(&data_canon) {
        return Err(format!(
            "requested path is outside data dir: {}",
            candidate.display()
        ));
    }
    Ok(canon)
}

fn write_response(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    body: &str,
) -> Result<(), String> {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.as_bytes().len(),
        body
    );
    stream
        .write_all(response.as_bytes())
        .map_err(|e| format!("failed to write response: {e}"))
}

#[cfg(test)]
mod tests {
    use super::{parse_request_head, positive_timeout_sec_or_default, read_request_head};

    // A4: the old server did one 4 KiB read() and looked only at the first line, so a request
    // whose head arrived in several TCP segments lost its headers. The head has to be read
    // until the blank line before anything may be decided from it.
    #[test]
    fn request_head_is_read_until_the_blank_line_even_across_reads() {
        struct Chunked(Vec<&'static [u8]>);
        impl std::io::Read for Chunked {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                if self.0.is_empty() {
                    return Ok(0);
                }
                let chunk = self.0.remove(0);
                buf[..chunk.len()].copy_from_slice(chunk);
                Ok(chunk.len())
            }
        }
        let mut reader = Chunked(vec![
            b"POST /control/start?target=onnx HTT",
            b"P/1.1\r\nHost: 127.0.0.1:8787\r\nX-Tool-",
            b"Token: abc\r\n\r\n",
        ]);
        let head = read_request_head(&mut reader, 32 * 1024)
            .expect("head")
            .expect("head present");
        assert!(head.ends_with("\r\n\r\n"), "head was {head:?}");
        let parsed = parse_request_head(&head).expect("parsed");
        assert_eq!(parsed.method, "POST");
        assert_eq!(parsed.path, "/control/start?target=onnx");
    }

    #[test]
    fn a_head_without_a_blank_line_is_an_error_not_a_silent_truncation() {
        let mut reader: &[u8] = b"GET /healthz HTTP/1.1\r\nHost: x\r\n";
        assert!(read_request_head(&mut reader, 32 * 1024).is_err());
    }

    // A browser preconnect opens the socket and closes it again; that is not a request error.
    #[test]
    fn a_connection_that_sends_nothing_is_not_an_error() {
        let mut reader: &[u8] = b"";
        assert_eq!(read_request_head(&mut reader, 32 * 1024), Ok(None));
    }

    // A4: an oversized head must be refused rather than buffered without bound.
    #[test]
    fn an_oversized_request_head_is_refused() {
        let mut long = String::from("GET / HTTP/1.1\r\n");
        for i in 0..500 {
            long.push_str(&format!("X-Pad-{i}: {}\r\n", "a".repeat(64)));
        }
        long.push_str("\r\n");
        let mut reader = long.as_bytes();
        assert!(read_request_head(&mut reader, 4096).is_err());
    }

    #[test]
    fn a_request_line_without_a_path_falls_back_to_root() {
        let parsed = parse_request_head("GET\r\n\r\n").expect("parsed");
        assert_eq!(parsed.method, "GET");
        assert_eq!(parsed.path, "/");
    }

    // A29: 0 (and anything unparseable) means "no time limit" downstream and is now
    // rejected by the pipelines, so the UI must not hand it on.
    #[test]
    fn non_positive_timeout_query_falls_back_to_the_default() {
        assert_eq!(positive_timeout_sec_or_default(Some("45")), "45");
        assert_eq!(positive_timeout_sec_or_default(Some("1")), "1");
        assert_eq!(positive_timeout_sec_or_default(Some("0")), "30");
        assert_eq!(positive_timeout_sec_or_default(Some("-5")), "30");
        assert_eq!(positive_timeout_sec_or_default(Some("abc")), "30");
        assert_eq!(positive_timeout_sec_or_default(Some("")), "30");
        assert_eq!(positive_timeout_sec_or_default(None), "30");
    }
}

