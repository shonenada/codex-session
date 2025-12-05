use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use codex_protocol::models::{ContentItem, ResponseItem};
use codex_protocol::protocol::{
    EventMsg, RolloutItem, RolloutLine, SessionMetaLine, SessionSource,
};
use owo_colors::OwoColorize;
use printpdf::{BuiltinFont, Mm, PdfDocument};
use serde::Serialize;
use serde_json::Value;
use std::cmp::Reverse;
use std::fs;
use std::fs::File;
use std::io::BufWriter;
use std::io::Write;
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};
use thiserror::Error;
use time::format_description::FormatItem;
use time::macros::format_description;
use time::{OffsetDateTime, PrimitiveDateTime};
use uuid::Uuid;
use walkdir::WalkDir;

const SESSIONS_SUBDIR: &str = "sessions";
const MAX_SCAN_FILES: usize = 10_000;
const HEAD_RECORD_LIMIT: usize = 10;
const INTERACTIVE_SOURCES: &[SessionSource] = &[SessionSource::Cli, SessionSource::VSCode];

#[derive(Debug, Clone, Serialize)]
pub struct SessionSummary {
    pub id: String,
    pub path: PathBuf,
    pub preview: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
    pub cwd: Option<PathBuf>,
    pub git_branch: Option<String>,
    pub provider: Option<String>,
}

impl SessionSummary {
    pub fn resume_hint(&self) -> String {
        format!("codex resume {}", self.id.cyan())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionList {
    pub sessions: Vec<SessionSummary>,
    pub next_cursor: Option<String>,
    pub scanned_files: usize,
    pub reached_scan_cap: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionDetail {
    pub summary: SessionSummary,
    pub instructions: Option<String>,
    pub source: Option<SessionSource>,
    pub git_branch: Option<String>,
    pub meta: Option<SessionMetaLine>,
}

#[derive(Debug, Clone)]
pub struct ListOptions {
    pub limit: usize,
    pub cursor: Option<String>,
    pub providers: Vec<String>,
    pub show_all: bool,
    pub cwd_filter: Option<PathBuf>,
}

impl Default for ListOptions {
    fn default() -> Self {
        Self {
            limit: 20,
            cursor: None,
            providers: Vec::new(),
            show_all: false,
            cwd_filter: None,
        }
    }
}

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("No sessions found")]
    NotFound,
    #[error(transparent)]
    Io(#[from] io::Error),
}

pub fn list_sessions(codex_home: &Path, opts: &ListOptions) -> Result<SessionList> {
    let root = codex_home.join(SESSIONS_SUBDIR);
    if !root.exists() {
        return Ok(SessionList {
            sessions: Vec::new(),
            next_cursor: None,
            scanned_files: 0,
            reached_scan_cap: false,
        });
    }

    let anchor = opts.cursor.as_deref().and_then(|token| parse_cursor(token));
    let (mut anchor_passed, anchor_ts, anchor_id) = match anchor {
        Some(cursor) => (false, cursor.ts, cursor.id),
        None => (true, OffsetDateTime::UNIX_EPOCH, Uuid::nil()),
    };

    let mut collected: Vec<SessionSummary> = Vec::new();
    let mut scanned_files = 0usize;
    let mut reached_scan_cap = false;
    let mut more_matches_available = false;

    let year_dirs = collect_dirs_desc(&root, |s| s.parse::<u16>().ok())?;

    'outer: for (_, year_path) in year_dirs.iter() {
        let month_dirs = collect_dirs_desc(year_path, |s| s.parse::<u8>().ok())?;
        for (_, month_path) in month_dirs.iter() {
            let day_dirs = collect_dirs_desc(month_path, |s| s.parse::<u8>().ok())?;
            for (_, day_path) in day_dirs.iter() {
                let mut day_files = collect_rollout_files(day_path)?;
                day_files.sort_by_key(|(ts, sid, _)| (Reverse(*ts), Reverse(*sid)));
                for (ts, sid, path) in day_files.into_iter() {
                    scanned_files += 1;
                    if scanned_files >= MAX_SCAN_FILES && collected.len() >= opts.limit {
                        reached_scan_cap = true;
                        more_matches_available = true;
                        break 'outer;
                    }

                    if !anchor_passed {
                        if ts < anchor_ts || (ts == anchor_ts && sid < anchor_id) {
                            anchor_passed = true;
                        } else {
                            continue;
                        }
                    }

                    match summarize_session(&path)? {
                        Some(summary) => {
                            if !opts.show_all {
                                if let Some(filter) = opts.cwd_filter.as_ref() {
                                    if let Some(row_cwd) = summary.cwd.as_ref() {
                                        if !paths_match(row_cwd, filter) {
                                            continue;
                                        }
                                    } else {
                                        continue;
                                    }
                                }
                            }

                            if !opts.providers.is_empty() {
                                let provider = summary.provider.as_deref().unwrap_or("");
                                if !opts
                                    .providers
                                    .iter()
                                    .any(|candidate| candidate.eq_ignore_ascii_case(provider))
                                {
                                    continue;
                                }
                            }

                            collected.push(summary);
                            if collected.len() == opts.limit {
                                more_matches_available = true;
                                break 'outer;
                            }
                        }
                        None => continue,
                    }
                }
            }
        }
    }

    let next_cursor = if more_matches_available {
        collected
            .last()
            .and_then(|summary| build_cursor_from_path(&summary.path))
    } else {
        None
    };

    Ok(SessionList {
        sessions: collected,
        next_cursor,
        scanned_files,
        reached_scan_cap,
    })
}

pub fn load_session_detail(_codex_home: &Path, path: &Path) -> Result<SessionDetail> {
    let summary = summarize_session(path)?.ok_or(SessionError::NotFound)?;
    let head = read_head_summary(path, HEAD_RECORD_LIMIT)?;
    let meta = extract_session_meta(&head.head);
    Ok(SessionDetail {
        git_branch: summary.git_branch.clone(),
        instructions: meta
            .as_ref()
            .and_then(|line| line.meta.instructions.clone()),
        source: meta.as_ref().map(|line| line.meta.source.clone()),
        summary,
        meta,
    })
}

pub fn resolve_session_path(codex_home: &Path, query: &str) -> Result<PathBuf> {
    let path = PathBuf::from(query);
    if path.exists() {
        return Ok(path);
    }

    let uuid = Uuid::parse_str(query)
        .with_context(|| format!("{query} is not a valid UUID or file path"))?;
    let sessions_root = codex_home.join(SESSIONS_SUBDIR);
    if !sessions_root.exists() {
        return Err(SessionError::NotFound.into());
    }

    for entry in WalkDir::new(&sessions_root).into_iter().flatten() {
        if !entry.file_type().is_file() {
            continue;
        }
        if let Some(file_name) = entry.file_name().to_str() {
            if let Some((_, file_uuid)) = parse_timestamp_uuid_from_filename(file_name) {
                if file_uuid == uuid {
                    return Ok(entry.into_path());
                }
            }
        }
    }

    Err(SessionError::NotFound.into())
}

pub fn export_session_chat(source: &Path, target: &Path) -> Result<()> {
    let is_jsonl = target
        .extension()
        .map(|ext| ext.eq_ignore_ascii_case("jsonl"))
        .unwrap_or(false);
    let is_json = target
        .extension()
        .map(|ext| ext.eq_ignore_ascii_case("json"))
        .unwrap_or(false);
    let is_pdf = target
        .extension()
        .map(|ext| ext.eq_ignore_ascii_case("pdf"))
        .unwrap_or(false);
    if let Some(parent) = target.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("unable to create export directory {parent:?}"))?;
    }

    if is_jsonl {
        fs::copy(source, target)
            .with_context(|| format!("failed to copy session to {target:?}"))?;
        return Ok(());
    }

    let (meta_line, entries) = read_session_entries(source)?;

    if is_json {
        let writer = BufWriter::new(
            File::create(target)
                .with_context(|| format!("failed to create export file {target:?}"))?,
        );
        serde_json::to_writer_pretty(writer, &entries)?;
        return Ok(());
    }

    if is_pdf {
        let markdown = render_markdown(meta_line.as_ref(), &entries);
        export_markdown_pdf(&markdown, target)?;
        return Ok(());
    }

    let markdown = render_markdown(meta_line.as_ref(), &entries);
    let mut writer = BufWriter::new(
        File::create(target).with_context(|| format!("failed to create export file {target:?}"))?,
    );
    writer.write_all(markdown.as_bytes())?;
    writer.flush()?;
    Ok(())
}

#[derive(Serialize)]
struct ChatEntry {
    role: String,
    content: String,
}

fn read_session_entries(source: &Path) -> Result<(Option<SessionMetaLine>, Vec<ChatEntry>)> {
    let file =
        File::open(source).with_context(|| format!("failed to open session file {source:?}"))?;
    let reader = BufReader::new(file);
    let mut meta_line: Option<SessionMetaLine> = None;
    let mut entries = Vec::new();
    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(rollout_line) = serde_json::from_str::<RolloutLine>(trimmed) else {
            continue;
        };
        match rollout_line.item {
            RolloutItem::SessionMeta(meta) => {
                if meta_line.is_none() {
                    meta_line = Some(meta);
                }
            }
            RolloutItem::ResponseItem(item) => {
                if let ResponseItem::Message { role, content, .. } = item {
                    let text = flatten_content(&content);
                    if !text.trim().is_empty() {
                        entries.push(ChatEntry {
                            role,
                            content: text,
                        });
                    }
                }
            }
            _ => {}
        }
    }
    Ok((meta_line, entries))
}

fn render_markdown(meta_line: Option<&SessionMetaLine>, entries: &[ChatEntry]) -> String {
    let mut buf = String::new();
    if let Some(meta) = meta_line {
        buf.push_str(&format!("# Session {}\n\n", meta.meta.id));
        buf.push_str(&format!("- started: {}\n", meta.meta.timestamp));
        buf.push_str(&format!("- cwd: {}\n", meta.meta.cwd.display()));
        if let Some(provider) = meta.meta.model_provider.as_deref() {
            buf.push_str(&format!("- provider: {}\n", provider));
        }
        buf.push_str("\n");
    }
    for entry in entries {
        if entry.content.trim().is_empty() {
            continue;
        }
        buf.push_str(&format!(
            "**{}**\n{}\n\n",
            entry.role.to_uppercase(),
            entry.content.trim()
        ));
    }
    buf
}

fn summarize_session(path: &Path) -> Result<Option<SessionSummary>> {
    let summary = read_head_summary(path, HEAD_RECORD_LIMIT)?;
    if !summary.saw_session_meta || !summary.saw_user_event {
        return Ok(None);
    }

    let meta_line = extract_session_meta(&summary.head);
    let Some(meta_line) = meta_line else {
        return Ok(None);
    };
    let SessionMetaLine { meta, git } = meta_line;

    if !INTERACTIVE_SOURCES
        .iter()
        .any(|source| source == &meta.source)
    {
        return Ok(None);
    }

    let preview = preview_from_head(&summary.head);
    let created_at = summary.created_at.as_deref().and_then(parse_timestamp_str);
    let updated_at = summary
        .updated_at
        .as_deref()
        .and_then(parse_timestamp_str)
        .or_else(|| file_modified_time(path).ok().flatten())
        .or(created_at);

    Ok(Some(SessionSummary {
        id: meta.id.to_string(),
        path: path.to_path_buf(),
        preview,
        created_at,
        updated_at,
        cwd: Some(meta.cwd.clone()),
        git_branch: git.and_then(|info| info.branch),
        provider: meta.model_provider.clone(),
    }))
}

fn read_head_summary(path: &Path, head_limit: usize) -> io::Result<HeadSummary> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut summary = HeadSummary::default();

    for line in reader.lines().flatten() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let parsed: RolloutLine = match serde_json::from_str(trimmed) {
            Ok(value) => value,
            Err(_) => continue,
        };

        match parsed.item {
            RolloutItem::SessionMeta(meta_line) => {
                summary.source = Some(meta_line.meta.source.clone());
                summary.model_provider = meta_line.meta.model_provider.clone();
                summary.created_at = summary
                    .created_at
                    .clone()
                    .or_else(|| Some(parsed.timestamp.clone()));
                if let Ok(val) = serde_json::to_value(meta_line) {
                    summary.head.push(val);
                    summary.saw_session_meta = true;
                }
            }
            RolloutItem::ResponseItem(item) => {
                summary.created_at = summary
                    .created_at
                    .clone()
                    .or_else(|| Some(parsed.timestamp.clone()));
                if let Ok(val) = serde_json::to_value(item) {
                    summary.head.push(val);
                }
            }
            RolloutItem::EventMsg(EventMsg::UserMessage(_)) => {
                summary.saw_user_event = true;
            }
            _ => {}
        }

        if summary.head.len() >= head_limit || summary.saw_session_meta && summary.saw_user_event {
            break;
        }
    }

    if summary.updated_at.is_none() {
        summary.updated_at = summary.created_at.clone();
    }
    Ok(summary)
}

#[derive(Default)]
struct HeadSummary {
    head: Vec<Value>,
    source: Option<SessionSource>,
    model_provider: Option<String>,
    created_at: Option<String>,
    updated_at: Option<String>,
    saw_session_meta: bool,
    saw_user_event: bool,
}

fn extract_session_meta(head: &[Value]) -> Option<SessionMetaLine> {
    head.iter()
        .find_map(|val| serde_json::from_value::<SessionMetaLine>(val.clone()).ok())
}

fn preview_from_head(head: &[Value]) -> Option<String> {
    head.iter()
        .filter_map(|val| serde_json::from_value::<ResponseItem>(val.clone()).ok())
        .find_map(preview_from_response_item)
}

fn preview_from_response_item(item: ResponseItem) -> Option<String> {
    match item {
        ResponseItem::Message { role, content, .. } if role == "user" => {
            let mut pieces: Vec<String> = Vec::new();
            for entry in content {
                match entry {
                    ContentItem::InputText { text } => {
                        if is_session_prefix(&text) {
                            return None;
                        }
                        let trimmed = text.trim();
                        if trimmed.is_empty() {
                            continue;
                        }
                        if looks_like_instructions(trimmed) {
                            continue;
                        }
                        pieces.push(trimmed.to_string());
                    }
                    _ => {}
                }
            }
            if pieces.is_empty() {
                None
            } else {
                Some(pieces.join(" "))
            }
        }
        _ => None,
    }
}

fn flatten_content(content: &[ContentItem]) -> String {
    let mut buf = String::new();
    for entry in content {
        match entry {
            ContentItem::InputText { text } | ContentItem::OutputText { text } => {
                if !buf.is_empty() {
                    buf.push_str("\n");
                }
                buf.push_str(text);
            }
            ContentItem::InputImage { image_url } => {
                if !buf.is_empty() {
                    buf.push_str("\n");
                }
                buf.push_str(&format!("[image: {image_url}]"));
            }
        }
    }
    buf
}

fn is_session_prefix(text: &str) -> bool {
    let trimmed = text.trim_start();
    let lowered = trimmed.to_ascii_lowercase();
    lowered.starts_with("<environment_context>") || lowered.starts_with("<user_instructions>")
}

fn looks_like_instructions(text: &str) -> bool {
    text.starts_with("# AGENTS") || text.contains("<INSTRUCTIONS>")
}

fn file_modified_time(path: &Path) -> io::Result<Option<DateTime<Utc>>> {
    let meta = fs::metadata(path)?;
    let modified = meta.modified().ok();
    if let Some(modified) = modified {
        let chrono_time: DateTime<Utc> = modified.into();
        Ok(Some(chrono_time))
    } else {
        Ok(None)
    }
}

fn parse_timestamp_str(ts: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(ts)
        .map(|dt| dt.with_timezone(&Utc))
        .ok()
}

struct Cursor {
    ts: OffsetDateTime,
    id: Uuid,
}

fn parse_cursor(token: &str) -> Option<Cursor> {
    let (ts_str, uuid_str) = token.split_once('|')?;
    let format: &[FormatItem] =
        format_description!("[year]-[month]-[day]T[hour]-[minute]-[second]");
    let ts = PrimitiveDateTime::parse(ts_str, format).ok()?.assume_utc();
    let uuid = Uuid::parse_str(uuid_str).ok()?;
    Some(Cursor { ts, id: uuid })
}

fn build_cursor_from_path(path: &Path) -> Option<String> {
    let file_name = path.file_name()?.to_str()?;
    let (ts, uuid) = parse_timestamp_uuid_from_filename(file_name)?;
    let format: &[FormatItem] =
        format_description!("[year]-[month]-[day]T[hour]-[minute]-[second]");
    let ts_str = ts.format(format).ok()?;
    Some(format!("{ts_str}|{uuid}"))
}

fn parse_timestamp_uuid_from_filename(name: &str) -> Option<(OffsetDateTime, Uuid)> {
    let core = name.strip_prefix("rollout-")?.strip_suffix(".jsonl")?;
    let (sep_idx, uuid) = core
        .match_indices('-')
        .rev()
        .find_map(|(idx, _)| Uuid::parse_str(&core[idx + 1..]).ok().map(|u| (idx, u)))?;
    let ts_str = &core[..sep_idx];
    let format: &[FormatItem] =
        format_description!("[year]-[month]-[day]T[hour]-[minute]-[second]");
    let ts = PrimitiveDateTime::parse(ts_str, format).ok()?.assume_utc();
    Some((ts, uuid))
}

fn collect_dirs_desc<T, F>(dir: &Path, parse: F) -> io::Result<Vec<(T, PathBuf)>>
where
    T: Ord + Copy,
    F: Fn(&str) -> Option<T>,
{
    let mut entries: Vec<(T, PathBuf)> = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            if let Some(name) = entry.file_name().to_str() {
                if let Some(parsed) = parse(name) {
                    entries.push((parsed, entry.path()));
                }
            }
        }
    }
    entries.sort_by_key(|(val, _)| Reverse(*val));
    Ok(entries)
}

fn collect_rollout_files(dir: &Path) -> io::Result<Vec<(OffsetDateTime, Uuid, PathBuf)>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            if let Some(name) = entry.file_name().to_str() {
                if let Some((ts, uuid)) = parse_timestamp_uuid_from_filename(name) {
                    files.push((ts, uuid, entry.path()));
                }
            }
        }
    }
    Ok(files)
}

fn paths_match(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => a == b,
    }
}

fn export_markdown_pdf(markdown: &str, target: &Path) -> Result<()> {
    let (doc, page, layer) = PdfDocument::new("Codex Session", Mm(210.0), Mm(297.0), "Layer 1");
    let font = doc.add_builtin_font(BuiltinFont::Helvetica)?;
    let mut current_page = page;
    let mut current_layer = doc.get_page(current_page).get_layer(layer);
    let mut y_mm = 280.0;
    let left_margin = 15.0;
    let font_size = 12.0;
    let line_height_mm = font_size * 1.4 * 0.35277777;

    for line in markdown.lines() {
        if y_mm < 20.0 {
            let (new_page, new_layer) = doc.add_page(Mm(210.0), Mm(297.0), "Layer 1");
            current_page = new_page;
            current_layer = doc.get_page(current_page).get_layer(new_layer);
            y_mm = 280.0;
        }
        current_layer.use_text(line, font_size, Mm(left_margin), Mm(y_mm), &font);
        y_mm -= line_height_mm;
    }

    let mut writer = BufWriter::new(
        File::create(target).with_context(|| format!("failed to create export file {target:?}"))?,
    );
    doc.save(&mut writer)?;
    writer.flush()?;
    Ok(())
}
