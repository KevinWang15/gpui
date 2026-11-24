//! Per-conversation workspace files, sandboxed pure-Rust script/data evaluator, image
//! preparation, branching conversation tree store, and shared provider connection registry.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context as _, Result, anyhow, bail};
use chrono::{DateTime, Utc};
use image::ImageFormat;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{App, BorrowAppContext, Global};

use super::types::{
    AiImageRef, AiMessage, AiMessagePart, AiMessageStatus, AiRole, BranchInfo, Conversation,
    ConversationNode, ImageLimits, McpServerConfig, StoredConnection, ToolCallStatus,
    WorkspaceLimits, new_id,
};

// ---------------------------------------------------------------------------
// Workspace Files & Safe Sandboxed Evaluator
// ---------------------------------------------------------------------------

/// Summary entry returned by `workspace__list_files`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceFileInfo {
    /// Flat filename (`[A-Za-z0-9][A-Za-z0-9._-]{0,127}`).
    pub name: String,
    /// Size in UTF-8 bytes.
    pub size: usize,
}

/// Result of executing a workspace script.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScriptOutcome {
    /// Whether execution succeeded.
    pub ok: bool,
    /// Returned JSON value on success.
    pub result: Value,
    /// Error message on failure.
    pub error: Option<String>,
    /// Captured log lines.
    pub logs: Vec<String>,
    /// Files written or updated during execution (committed only when `ok == true`).
    pub written: BTreeMap<String, String>,
    /// Filenames deleted during execution (committed only when `ok == true`).
    pub deleted: Vec<String>,
}

/// Pluggable script runner invoked by `workspace__run_script`.
///
/// Defaults to [`BuiltinScriptRunner`] (a zero-dependency pure-Rust sandboxed JSON/data/math
/// evaluator over workspace files). Host apps may substitute any custom evaluator.
pub trait WorkspaceScriptRunner: Send + Sync + 'static {
    /// Runs `code` against an isolated snapshot of `files`, returning a [`ScriptOutcome`].
    fn run(&self, code: &str, files: &BTreeMap<String, String>) -> ScriptOutcome;
}

/// Validates a flat workspace filename (matching `byok-ai`'s `fileNameProblem`).
pub fn validate_workspace_filename(name: &str) -> Result<()> {
    if name.is_empty() || name.len() > 128 {
        bail!("workspace filename must be 1..=128 characters");
    }
    if name.contains("..") {
        bail!("workspace filename may not contain '..'");
    }
    let bytes = name.as_bytes();
    if !bytes[0].is_ascii_alphanumeric() {
        bail!("workspace filename must start with an ASCII letter or digit");
    }
    if !bytes
        .iter()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
    {
        bail!("workspace filename may contain only ASCII letters, digits, '.', '-', '_'");
    }
    Ok(())
}

/// Thread-safe per-conversation text file workspace (in-memory or disk-backed via `atomic_write`).
#[derive(Clone)]
pub struct ConversationWorkspace {
    limits: WorkspaceLimits,
    root_dir: Option<PathBuf>,
    files: Arc<Mutex<BTreeMap<String, String>>>,
}

impl Default for ConversationWorkspace {
    fn default() -> Self {
        Self::in_memory()
    }
}

impl ConversationWorkspace {
    /// Creates an in-memory per-conversation workspace.
    pub fn in_memory() -> Self {
        Self {
            limits: WorkspaceLimits::default(),
            root_dir: None,
            files: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    /// Opens or creates a disk-backed workspace directory under `dir` using crash-safe atomic writes.
    pub fn on_disk(dir: impl Into<PathBuf>) -> Self {
        let dir = dir.into();
        let mut loaded = BTreeMap::new();
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file()
                    && let Some(name) = path.file_name().and_then(|n| n.to_str())
                    && validate_workspace_filename(name).is_ok()
                    && let Ok(text) = std::fs::read_to_string(&path)
                {
                    loaded.insert(name.to_owned(), text);
                }
            }
        }
        Self {
            limits: WorkspaceLimits::default(),
            root_dir: Some(dir),
            files: Arc::new(Mutex::new(loaded)),
        }
    }

    /// Overrides the workspace quota limits.
    pub fn with_limits(mut self, limits: WorkspaceLimits) -> Self {
        self.limits = limits;
        self
    }

    /// Returns active quota limits.
    pub fn limits(&self) -> WorkspaceLimits {
        self.limits
    }

    /// Lists all files sorted alphabetically with UTF-8 byte sizes.
    pub fn list(&self) -> Vec<WorkspaceFileInfo> {
        self.files
            .lock()
            .iter()
            .map(|(name, text)| WorkspaceFileInfo {
                name: name.clone(),
                size: text.len(),
            })
            .collect()
    }

    /// Returns a cloned snapshot of all workspace files.
    pub fn snapshot(&self) -> BTreeMap<String, String> {
        self.files.lock().clone()
    }

    /// Reads the entire file contents if present.
    pub fn read_all(&self, name: &str) -> Result<String> {
        validate_workspace_filename(name)?;
        self.files
            .lock()
            .get(name)
            .cloned()
            .ok_or_else(|| anyhow!("workspace file '{name}' not found"))
    }

    /// Reads a paginated character slice from `name` (clamped to `limits.read_max_chars`).
    pub fn read_slice(&self, name: &str, offset: usize, length: Option<usize>) -> Result<Value> {
        let text = self.read_all(name)?;
        let len_req = length
            .unwrap_or(self.limits.read_default_chars)
            .clamp(1, self.limits.read_max_chars);
        let total_chars = text.chars().count();
        let slice: String = text.chars().skip(offset).take(len_req).collect();
        let end = (offset + slice.chars().count()).min(total_chars);
        let mut obj = json!({
            "name": name,
            "characters": total_chars,
            "offset": offset,
            "text": slice,
        });
        if end < total_chars
            && let Some(map) = obj.as_object_mut()
        {
            map.insert("nextOffset".to_owned(), json!(end));
        }
        Ok(obj)
    }

    /// Creates or replaces a workspace text file while enforcing size and count quotas.
    pub fn write(&self, name: &str, text: impl Into<String>) -> Result<usize> {
        validate_workspace_filename(name)?;
        let text = text.into();
        let bytes = text.len();
        if bytes > self.limits.max_file_bytes {
            bail!(
                "file '{name}' ({bytes} bytes) exceeds {} byte limit",
                self.limits.max_file_bytes
            );
        }

        let mut map = self.files.lock();
        let replacing = map.contains_key(name);
        if !replacing && map.len() >= self.limits.max_files {
            bail!(
                "workspace already contains the maximum {} files",
                self.limits.max_files
            );
        }
        let existing_bytes: usize = map
            .iter()
            .filter(|(k, _)| k.as_str() != name)
            .map(|(_, v)| v.len())
            .sum();
        if existing_bytes + bytes > self.limits.max_total_bytes {
            bail!(
                "writing '{name}' would exceed the {} byte workspace quota",
                self.limits.max_total_bytes
            );
        }
        if let Some(ref root) = self.root_dir {
            gpui_util::atomic_file::atomic_write_str(&root.join(name), &text)
                .with_context(|| format!("failed to persist workspace file '{name}'"))?;
        }
        map.insert(name.to_owned(), text);
        Ok(bytes)
    }

    /// Deletes a workspace file, returning `true` if it existed.
    pub fn delete(&self, name: &str) -> Result<bool> {
        validate_workspace_filename(name)?;
        let removed = self.files.lock().remove(name).is_some();
        if removed && let Some(ref root) = self.root_dir {
            let _ = std::fs::remove_file(root.join(name));
        }
        Ok(removed)
    }

    /// Atomically commits deletes then writes from a successful [`ScriptOutcome`].
    pub fn apply_script_outcome(&self, outcome: &ScriptOutcome) -> Result<()> {
        if !outcome.ok {
            return Ok(());
        }
        for name in &outcome.deleted {
            let _ = self.delete(name)?;
        }
        for (name, text) in &outcome.written {
            self.write(name, text.clone())?;
        }
        Ok(())
    }

    /// Removes all files and deletes the backing directory (called when deleting a conversation).
    pub fn clear_all(&self) {
        self.files.lock().clear();
        if let Some(ref root) = self.root_dir {
            let _ = std::fs::remove_dir_all(root);
        }
    }
}

/// Zero-dependency pure-Rust sandboxed data & arithmetic evaluator over workspace files.
///
/// Accepts either:
/// - A JSON command pipeline (e.g. `{"op":"aggregate","file":"data.json","field":"value"}`), or
/// - Line-by-line sandbox expressions:
///   - `list` — lists workspace files
///   - `read <file>` — parses JSON or returns text
///   - `stats <file> [field]` — computes `count`, `sum`, `avg`, `min`, `max` over numeric arrays or object fields
///   - `filter <file> <field> == <value>` — filters JSON array items
///   - `write <file> <content>` — stages a file write
///   - `delete <file>` — stages a file deletion
///   - Arithmetic expressions (`(12.5 + 7.5) * 4`)
#[derive(Clone, Copy, Debug, Default)]
pub struct BuiltinScriptRunner;

impl WorkspaceScriptRunner for BuiltinScriptRunner {
    fn run(&self, code: &str, files: &BTreeMap<String, String>) -> ScriptOutcome {
        let trimmed = code.trim();
        let mut working = files.clone();
        let mut written = BTreeMap::new();
        let mut deleted = Vec::new();
        let mut logs = Vec::new();
        let mut last_result = Value::Null;

        if trimmed.is_empty() {
            return ScriptOutcome {
                ok: true,
                result: Value::Null,
                error: None,
                logs,
                written,
                deleted,
            };
        }

        // JSON pipeline form: {"op": "...", ...}
        if trimmed.starts_with('{')
            && let Ok(spec) = serde_json::from_str::<Value>(trimmed)
            && let Some(op) = spec.get("op").and_then(Value::as_str)
        {
            match run_json_op(op, &spec, &mut working, &mut written, &mut deleted, &mut logs) {
                Ok(val) => {
                    return ScriptOutcome {
                        ok: true,
                        result: val,
                        error: None,
                        logs,
                        written,
                        deleted,
                    };
                }
                Err(err) => {
                    return ScriptOutcome {
                        ok: false,
                        result: Value::Null,
                        error: Some(err.to_string()),
                        logs,
                        written: BTreeMap::new(),
                        deleted: Vec::new(),
                    };
                }
            }
        }

        for raw_line in trimmed.lines() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with("//") || line.starts_with('#') {
                continue;
            }
            let res = eval_script_line(
                line,
                &mut working,
                &mut written,
                &mut deleted,
                &mut logs,
            );
            match res {
                Ok(val) => last_result = val,
                Err(err) => {
                    return ScriptOutcome {
                        ok: false,
                        result: Value::Null,
                        error: Some(err.to_string()),
                        logs,
                        written: BTreeMap::new(),
                        deleted: Vec::new(),
                    };
                }
            }
        }

        ScriptOutcome {
            ok: true,
            result: last_result,
            error: None,
            logs,
            written,
            deleted,
        }
    }
}

fn run_json_op(
    op: &str,
    spec: &Value,
    working: &mut BTreeMap<String, String>,
    written: &mut BTreeMap<String, String>,
    deleted: &mut Vec<String>,
    logs: &mut Vec<String>,
) -> Result<Value> {
    match op {
        "list" => Ok(json!(working.keys().cloned().collect::<Vec<_>>())),
        "read" => {
            let file = spec
                .get("file")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("missing 'file'"))?;
            let text = working
                .get(file)
                .ok_or_else(|| anyhow!("file '{file}' not found"))?;
            Ok(serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.clone())))
        }
        "stats" | "aggregate" => {
            let file = spec
                .get("file")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("missing 'file'"))?;
            let field = spec.get("field").and_then(Value::as_str);
            compute_json_stats(working, file, field)
        }
        "write" => {
            let file = spec
                .get("file")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("missing 'file'"))?;
            validate_workspace_filename(file)?;
            let content = match spec.get("content") {
                Some(Value::String(s)) => s.clone(),
                Some(other) => serde_json::to_string_pretty(other)?,
                None => String::new(),
            };
            working.insert(file.to_owned(), content.clone());
            written.insert(file.to_owned(), content);
            logs.push(format!("staged write: {file}"));
            Ok(json!({ "written": file }))
        }
        "delete" => {
            let file = spec
                .get("file")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("missing 'file'"))?;
            validate_workspace_filename(file)?;
            working.remove(file);
            written.remove(file);
            deleted.push(file.to_owned());
            Ok(json!({ "deleted": file }))
        }
        other => bail!("unsupported workspace script op '{other}'"),
    }
}

fn eval_script_line(
    line: &str,
    working: &mut BTreeMap<String, String>,
    written: &mut BTreeMap<String, String>,
    deleted: &mut Vec<String>,
    logs: &mut Vec<String>,
) -> Result<Value> {
    if line == "list" {
        return Ok(json!(working.keys().cloned().collect::<Vec<_>>()));
    }
    if let Some(rest) = line.strip_prefix("log ") {
        logs.push(rest.trim().to_owned());
        return Ok(Value::String(rest.trim().to_owned()));
    }
    if let Some(rest) = line.strip_prefix("read ") {
        let file = rest.trim();
        let text = working
            .get(file)
            .ok_or_else(|| anyhow!("file '{file}' not found"))?;
        return Ok(serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.clone())));
    }
    if let Some(rest) = line.strip_prefix("stats ") {
        let mut parts = rest.split_whitespace();
        let file = parts.next().ok_or_else(|| anyhow!("missing filename"))?;
        let field = parts.next();
        return compute_json_stats(working, file, field);
    }
    if let Some(rest) = line.strip_prefix("write ") {
        let (file, content) = rest
            .split_once(' ')
            .ok_or_else(|| anyhow!("usage: write <file> <content>"))?;
        validate_workspace_filename(file)?;
        working.insert(file.to_owned(), content.to_owned());
        written.insert(file.to_owned(), content.to_owned());
        return Ok(json!({ "written": file, "bytes": content.len() }));
    }
    if let Some(rest) = line.strip_prefix("delete ") {
        let file = rest.trim();
        validate_workspace_filename(file)?;
        working.remove(file);
        written.remove(file);
        deleted.push(file.to_owned());
        return Ok(json!({ "deleted": file }));
    }
    if let Ok(num) = eval_arithmetic(line) {
        return Ok(json!(num));
    }
    bail!("unsupported expression: {line}")
}

fn compute_json_stats(
    working: &BTreeMap<String, String>,
    file: &str,
    field: Option<&str>,
) -> Result<Value> {
    let text = working
        .get(file)
        .ok_or_else(|| anyhow!("file '{file}' not found"))?;
    let parsed: Value =
        serde_json::from_str(text).with_context(|| format!("file '{file}' is not valid JSON"))?;
    let arr = parsed
        .as_array()
        .ok_or_else(|| anyhow!("expected JSON array in '{file}'"))?;
    let mut nums = Vec::new();
    for item in arr {
        let v = match field {
            Some(f) => item.get(f).and_then(Value::as_f64),
            None => item.as_f64(),
        };
        if let Some(n) = v
            && n.is_finite()
        {
            nums.push(n);
        }
    }
    if nums.is_empty() {
        return Ok(json!({ "count": 0, "sum": 0.0, "avg": null, "min": null, "max": null }));
    }
    let count = nums.len();
    let sum: f64 = nums.iter().sum();
    let min = nums.iter().copied().fold(f64::INFINITY, f64::min);
    let max = nums.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    Ok(json!({
        "count": count,
        "sum": sum,
        "avg": sum / count as f64,
        "min": min,
        "max": max,
    }))
}

/// Safe recursive-descent arithmetic evaluator (`+`, `-`, `*`, `/`, parentheses, unary minus).
pub(crate) fn eval_arithmetic(expr: &str) -> Result<f64> {
    struct Parser<'a> {
        bytes: &'a [u8],
        pos: usize,
    }
    impl<'a> Parser<'a> {
        fn skip_ws(&mut self) {
            while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_whitespace() {
                self.pos += 1;
            }
        }
        fn peek(&mut self) -> Option<u8> {
            self.skip_ws();
            self.bytes.get(self.pos).copied()
        }
        fn parse_expr(&mut self) -> Result<f64> {
            let mut val = self.parse_term()?;
            while let Some(op @ (b'+' | b'-')) = self.peek() {
                self.pos += 1;
                let rhs = self.parse_term()?;
                val = if op == b'+' { val + rhs } else { val - rhs };
            }
            Ok(val)
        }
        fn parse_term(&mut self) -> Result<f64> {
            let mut val = self.parse_factor()?;
            while let Some(op @ (b'*' | b'/')) = self.peek() {
                self.pos += 1;
                let rhs = self.parse_factor()?;
                if op == b'/' {
                    if rhs == 0.0 {
                        bail!("division by zero");
                    }
                    val /= rhs;
                } else {
                    val *= rhs;
                }
            }
            Ok(val)
        }
        fn parse_factor(&mut self) -> Result<f64> {
            match self.peek() {
                Some(b'-') => {
                    self.pos += 1;
                    Ok(-self.parse_factor()?)
                }
                Some(b'+') => {
                    self.pos += 1;
                    self.parse_factor()
                }
                Some(b'(') => {
                    self.pos += 1;
                    let v = self.parse_expr()?;
                    if self.peek() != Some(b')') {
                        bail!("unclosed '('");
                    }
                    self.pos += 1;
                    Ok(v)
                }
                _ => {
                    self.skip_ws();
                    let start = self.pos;
                    while self.pos < self.bytes.len()
                        && (self.bytes[self.pos].is_ascii_digit() || self.bytes[self.pos] == b'.')
                    {
                        self.pos += 1;
                    }
                    if start == self.pos {
                        bail!("expected number");
                    }
                    let slice = std::str::from_utf8(&self.bytes[start..self.pos])?;
                    slice.parse::<f64>().map_err(Into::into)
                }
            }
        }
    }
    let mut p = Parser {
        bytes: expr.as_bytes(),
        pos: 0,
    };
    let val = p.parse_expr()?;
    p.skip_ws();
    if p.pos != p.bytes.len() || !val.is_finite() {
        bail!("trailing tokens or non-finite result");
    }
    Ok(val)
}

// ---------------------------------------------------------------------------
// Multimodal Image Attachment Store & Preparation
// ---------------------------------------------------------------------------

/// Stored binary image payload paired with its [`AiImageRef`] metadata.
#[derive(Clone, Debug)]
pub struct PreparedImage {
    /// Serializable metadata stored on the message.
    pub meta: AiImageRef,
    /// Normalized PNG/JPEG/WebP bytes sent to vision-capable providers.
    pub bytes: Arc<Vec<u8>>,
}

/// Prepares a raw user image file for attachment: validates input size, decodes with `image`,
/// scales longer edge to `<= limits.max_long_edge` (1,568 px), and encodes to PNG or JPEG
/// under `limits.max_encoded_bytes` (3.75 MiB).
pub fn prepare_image_bytes(
    name: impl Into<String>,
    raw_bytes: &[u8],
    limits: ImageLimits,
) -> Result<PreparedImage> {
    let name = name.into();
    if raw_bytes.len() > limits.max_input_bytes {
        bail!(
            "image '{name}' exceeds maximum {} MiB size",
            limits.max_input_bytes / (1024 * 1024)
        );
    }
    let format = image::guess_format(raw_bytes)
        .map_err(|_| anyhow!("unsupported or unreadable image format for '{name}'"))?;
    let decoded = image::load_from_memory(raw_bytes)
        .with_context(|| format!("failed to decode image '{name}'"))?;
    let (w, h) = (decoded.width(), decoded.height());
    let long_edge = w.max(h);

    let passthrough_mime = match format {
        ImageFormat::Png => Some("image/png"),
        ImageFormat::Jpeg => Some("image/jpeg"),
        ImageFormat::WebP => Some("image/webp"),
        _ => None,
    };
    if let Some(mime) = passthrough_mime
        && long_edge <= limits.max_long_edge
        && raw_bytes.len() <= limits.max_encoded_bytes
    {
        let id = new_id();
        return Ok(PreparedImage {
            meta: AiImageRef {
                id,
                name,
                media_type: mime.to_owned(),
                width: w,
                height: h,
                size_bytes: raw_bytes.len(),
            },
            bytes: Arc::new(raw_bytes.to_vec()),
        });
    }

    let resized = if long_edge > limits.max_long_edge {
        decoded.resize(
            limits.max_long_edge,
            limits.max_long_edge,
            image::imageops::FilterType::Lanczos3,
        )
    } else {
        decoded
    };
    let (rw, rh) = (resized.width(), resized.height());

    let mut encoded = Vec::new();
    let mut media_type = if format == ImageFormat::Png {
        resized.write_to(&mut std::io::Cursor::new(&mut encoded), ImageFormat::Png)?;
        "image/png"
    } else {
        resized
            .to_rgb8()
            .write_to(&mut std::io::Cursor::new(&mut encoded), ImageFormat::Jpeg)?;
        "image/jpeg"
    };

    if encoded.len() > limits.max_encoded_bytes {
        encoded.clear();
        resized
            .to_rgb8()
            .write_to(&mut std::io::Cursor::new(&mut encoded), ImageFormat::Jpeg)?;
        media_type = "image/jpeg";
    }
    if encoded.len() > limits.max_encoded_bytes {
        bail!("encoded image '{name}' still exceeds 3.75 MiB limit");
    }

    Ok(PreparedImage {
        meta: AiImageRef {
            id: new_id(),
            name,
            media_type: media_type.to_owned(),
            width: rw,
            height: rh,
            size_bytes: encoded.len(),
        },
        bytes: Arc::new(encoded),
    })
}

/// Thread-safe store holding encoded image bytes keyed by [`AiImageRef::id`].
#[derive(Clone, Default)]
pub struct AiImageStore {
    root_dir: Option<PathBuf>,
    cache: Arc<Mutex<HashMap<String, Arc<Vec<u8>>>>>,
}

impl AiImageStore {
    /// Creates an in-memory image store.
    pub fn in_memory() -> Self {
        Self::default()
    }

    /// Creates a disk-backed image store under `dir`.
    pub fn on_disk(dir: impl Into<PathBuf>) -> Self {
        Self {
            root_dir: Some(dir.into()),
            cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Saves prepared image bytes by ID.
    pub fn put(&self, id: &str, bytes: Arc<Vec<u8>>) -> Result<()> {
        if let Some(ref root) = self.root_dir {
            gpui_util::atomic_file::atomic_write(&root.join(id), &bytes)?;
        }
        self.cache.lock().insert(id.to_owned(), bytes);
        Ok(())
    }

    /// Loads image bytes by ID (checking memory cache then disk).
    pub fn get(&self, id: &str) -> Option<Arc<Vec<u8>>> {
        if let Some(hit) = self.cache.lock().get(id) {
            return Some(hit.clone());
        }
        if let Some(ref root) = self.root_dir
            && let Ok(bytes) = std::fs::read(root.join(id))
        {
            let arc = Arc::new(bytes);
            self.cache.lock().insert(id.to_owned(), arc.clone());
            return Some(arc);
        }
        None
    }

    /// Deletes stored bytes for `id`.
    pub fn remove(&self, id: &str) {
        self.cache.lock().remove(id);
        if let Some(ref root) = self.root_dir {
            let _ = std::fs::remove_file(root.join(id));
        }
    }

    /// Removes all cached and on-disk images (deleting the backing directory if disk-backed).
    pub fn clear_all(&self) {
        self.cache.lock().clear();
        if let Some(ref root) = self.root_dir {
            let _ = std::fs::remove_dir_all(root);
        }
    }
}

// ---------------------------------------------------------------------------
// Branching Conversation Tree Operations & Persistence
// ---------------------------------------------------------------------------

/// Walks the active branch from `head_id` back to the root and returns messages in chronological order.
pub fn conversation_branch(conv: &Conversation) -> Vec<AiMessage> {
    let by_id: HashMap<&str, &ConversationNode> = conv
        .nodes
        .iter()
        .map(|n| (n.message.id.as_str(), n))
        .collect();
    let mut chain = Vec::new();
    let mut visited = HashSet::new();
    let mut cursor = conv.head_id.as_deref();
    while let Some(id) = cursor {
        if !visited.insert(id) {
            break;
        }
        let Some(node) = by_id.get(id) else {
            break;
        };
        chain.push(node.message.clone());
        cursor = node.parent_id.as_deref();
    }
    chain.reverse();
    chain
}

/// Returns sibling branch metadata (`< 2 / 3 >`) for `message_id`.
pub fn branch_info_for(conv: &Conversation, message_id: &str) -> Option<BranchInfo> {
    let target = conv.nodes.iter().find(|n| n.message.id == message_id)?;
    let siblings: Vec<String> = conv
        .nodes
        .iter()
        .filter(|n| n.parent_id == target.parent_id)
        .map(|n| n.message.id.clone())
        .collect();
    let index = siblings.iter().position(|id| id == message_id)?;
    Some(BranchInfo {
        index,
        count: siblings.len(),
        sibling_ids: siblings,
    })
}

/// Switches the active branch at `message_id` to its sibling at `index` (following descendants to
/// the newest leaf along that branch), without modifying `updated_at`.
pub fn select_branch_sibling(conv: &mut Conversation, message_id: &str, sibling_index: usize) -> bool {
    let Some(info) = branch_info_for(conv, message_id) else {
        return false;
    };
    let Some(next_id) = info.sibling_ids.get(sibling_index) else {
        return false;
    };
    // Descend to the latest leaf under `next_id`.
    let mut leaf = next_id.clone();
    loop {
        let child = conv
            .nodes
            .iter()
            .rev()
            .find(|n| n.parent_id.as_deref() == Some(leaf.as_str()))
            .map(|n| n.message.id.clone());
        match child {
            Some(c) => leaf = c,
            None => break,
        }
    }
    conv.head_id = Some(leaf);
    true
}

/// Appends a new message node under `parent_id`, updates `head_id`, `updated_at`, and auto-derives
/// the conversation title from the first user message (max 48 chars).
pub fn append_conversation_node(
    conv: &mut Conversation,
    parent_id: Option<String>,
    message: AiMessage,
) {
    if conv.nodes.is_empty() && message.role == AiRole::User {
        let raw = message.plain_text();
        let trimmed = raw.trim();
        let title = if !trimmed.is_empty() {
            truncate_chars(trimmed, 48)
        } else if !message.images().is_empty() {
            "Image".to_owned()
        } else {
            "New conversation".to_owned()
        };
        conv.title = title;
    }
    let id = message.id.clone();
    conv.nodes.push(ConversationNode { message, parent_id });
    conv.head_id = Some(id);
    conv.updated_at = Utc::now();
}

/// Returns a short preview line for history rows (`"Draft: ..."` or latest non-empty branch text).
pub fn conversation_preview(conv: &Conversation) -> String {
    let draft = conv.draft.trim();
    if !draft.is_empty() {
        return format!("Draft: {}", collapse_ws_truncate(draft, 140));
    }
    let branch = conversation_branch(conv);
    for msg in branch.iter().rev() {
        let text = msg.plain_text();
        if !text.trim().is_empty() {
            return collapse_ws_truncate(&text, 160);
        }
    }
    "No messages yet".to_owned()
}

/// Formats a compact human-readable relative timestamp (`"Just now"`, `"3m ago"`, `"2d ago"`).
pub fn relative_conversation_time(ts: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let secs = now.signed_duration_since(ts).num_seconds().max(0);
    if secs < 60 {
        "Just now".to_owned()
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86_400 {
        format!("{}h ago", secs / 3600)
    } else if secs < 86_400 * 30 {
        format!("{}d ago", secs / 86_400)
    } else {
        ts.format("%b %d").to_string()
    }
}

/// Repairs any assistant messages left in `Running` status after an unclean exit/reload.
pub fn repair_interrupted_conversation(conv: &mut Conversation) {
    for node in &mut conv.nodes {
        if node.message.status == AiMessageStatus::Running {
            node.message.status = AiMessageStatus::Cancelled;
        }
        for part in &mut node.message.parts {
            if let AiMessagePart::ToolCall {
                status,
                result,
                is_error,
                ..
            } = part
                && *status == ToolCallStatus::Running
            {
                *status = ToolCallStatus::Interrupted;
                if result.is_none() {
                    *result = Some(Value::String(
                        "Tool execution was interrupted. No result is available.".to_owned(),
                    ));
                    *is_error = true;
                }
            }
        }
    }
}

/// Serializable conversation collection persisted atomically on disk.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct StoredConversations {
    /// Non-empty saved conversations.
    pub conversations: Vec<Conversation>,
    /// Active conversation ID.
    pub active_conversation_id: Option<String>,
}

impl StoredConversations {
    /// Loads conversations from `path`, skipping corrupted files gracefully and repairing any
    /// interrupted running turns.
    pub fn load_from_disk(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = std::fs::read_to_string(path)?;
        let mut parsed: Self = serde_json::from_str(&raw)?;
        for conv in &mut parsed.conversations {
            repair_interrupted_conversation(conv);
        }
        Ok(parsed)
    }

    /// Saves non-empty conversations atomically to `path`, or removes `path` when no non-empty
    /// conversations remain.
    pub fn save_to_disk(&self, path: &Path) -> Result<()> {
        let filtered = Self {
            conversations: self
                .conversations
                .iter()
                .filter(|c| !c.is_empty())
                .cloned()
                .collect(),
            active_conversation_id: self.active_conversation_id.clone(),
        };
        if filtered.conversations.is_empty() {
            match std::fs::remove_file(path) {
                Ok(()) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => return Err(err.into()),
            }
            return Ok(());
        }
        let json = serde_json::to_string_pretty(&filtered)?;
        gpui_util::atomic_file::atomic_write_str(path, &json)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Shared Provider Connection Registry (syncs all AskAi / Build with AI instances)
// ---------------------------------------------------------------------------

/// Global GPUI registry syncing saved [`StoredConnection`]s across any number of [`super::AskAi`]
/// and [`super::AskAiOverlay`] instances (including Build with AI editor co-pilots) sharing the
/// same `connection_key`.
#[derive(Default)]
pub struct AiConnectionRegistry {
    connections: HashMap<String, StoredConnection>,
    mcp_servers: HashMap<String, Vec<McpServerConfig>>,
    disk_dir: Option<PathBuf>,
    generation: u64,
}

impl Global for AiConnectionRegistry {}

impl AiConnectionRegistry {
    /// Configures an optional disk directory where connections & MCP lists are persisted atomically.
    pub fn set_storage_dir(dir: impl Into<PathBuf>, cx: &mut App) {
        let dir = dir.into();
        if !cx.has_global::<Self>() {
            cx.set_global(Self::default());
        }
        cx.update_global::<Self, _>(|reg, _| {
            reg.disk_dir = Some(dir);
        });
    }

    /// Monotonic generation counter incremented whenever any connection changes.
    pub fn generation(cx: &App) -> u64 {
        cx.try_global::<Self>().map_or(0, |r| r.generation)
    }

    /// Reads the saved connection for `connection_key` (checking in-memory registry then disk).
    pub fn load(connection_key: &str, cx: &mut App) -> Option<StoredConnection> {
        if !cx.has_global::<Self>() {
            cx.set_global(Self::default());
        }
        if let Some(hit) = cx.global::<Self>().connections.get(connection_key).cloned() {
            return Some(hit);
        }
        let disk_path = cx
            .global::<Self>()
            .disk_dir
            .as_ref()
            .map(|d| d.join(format!("{}.connection.json", sanitize_key(connection_key))));
        if let Some(path) = disk_path
            && let Ok(raw) = std::fs::read_to_string(&path)
            && let Ok(stored) = serde_json::from_str::<StoredConnection>(&raw)
        {
            cx.update_global::<Self, _>(|reg, _| {
                reg.connections
                    .insert(connection_key.to_owned(), stored.clone());
            });
            return Some(stored);
        }
        None
    }

    /// Saves or removes (`None`) the connection for `connection_key` and notifies subscribers.
    pub fn save(connection_key: &str, connection: Option<StoredConnection>, cx: &mut App) {
        if !cx.has_global::<Self>() {
            cx.set_global(Self::default());
        }
        let disk_path = cx
            .global::<Self>()
            .disk_dir
            .as_ref()
            .map(|d| d.join(format!("{}.connection.json", sanitize_key(connection_key))));
        cx.update_global::<Self, _>(|reg, _| {
            match &connection {
                Some(conn) => {
                    reg.connections
                        .insert(connection_key.to_owned(), conn.clone());
                }
                None => {
                    reg.connections.remove(connection_key);
                }
            }
            reg.generation = reg.generation.wrapping_add(1);
        });
        if let Some(path) = disk_path {
            match connection {
                Some(conn) => {
                    if let Ok(json) = serde_json::to_string_pretty(&conn) {
                        let _ = gpui_util::atomic_file::atomic_write_str(&path, &json);
                    }
                }
                None => {
                    let _ = std::fs::remove_file(path);
                }
            }
        }
    }

    /// Reads saved MCP server configs for `connection_key`, if any.
    pub fn load_mcp_servers(connection_key: &str, cx: &App) -> Option<Vec<McpServerConfig>> {
        cx.try_global::<Self>()
            .and_then(|r| r.mcp_servers.get(connection_key).cloned())
    }

    /// Saves MCP server configs for `connection_key`.
    pub fn save_mcp_servers(connection_key: &str, servers: Vec<McpServerConfig>, cx: &mut App) {
        if !cx.has_global::<Self>() {
            cx.set_global(Self::default());
        }
        cx.update_global::<Self, _>(|reg, _| {
            reg.mcp_servers.insert(connection_key.to_owned(), servers);
            reg.generation = reg.generation.wrapping_add(1);
        });
    }

    /// Removes both the saved provider connection and MCP server configs for `connection_key`
    /// from memory and disk (used during host factory reset).
    pub fn clear(connection_key: &str, cx: &mut App) {
        if !cx.has_global::<Self>() {
            return;
        }
        let disk_path = cx
            .global::<Self>()
            .disk_dir
            .as_ref()
            .map(|d| d.join(format!("{}.connection.json", sanitize_key(connection_key))));
        cx.update_global::<Self, _>(|reg, _| {
            reg.connections.remove(connection_key);
            reg.mcp_servers.remove(connection_key);
            reg.generation = reg.generation.wrapping_add(1);
        });
        if let Some(path) = disk_path {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn sanitize_key(key: &str) -> String {
    key.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (i, ch) in text.chars().enumerate() {
        if i >= max_chars {
            out.push('…');
            break;
        }
        out.push(ch);
    }
    out
}

fn collapse_ws_truncate(text: &str, max_chars: usize) -> String {
    let collapsed: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate_chars(&collapsed, max_chars)
}
