use std::io;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use crossterm::event::{self, Event as CEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Row, Table};

use crate::session_store::SessionSummary;

const DELETE_SEQUENCE_TIMEOUT: Duration = Duration::from_millis(600);

pub fn run(sessions: Vec<SessionSummary>) -> Result<Option<SessionSummary>> {
    if sessions.is_empty() {
        println!("No Codex sessions recorded yet. Start a session to manage history.");
        return Ok(None);
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let mut app = App::new(sessions);
    let mut outcome = None;
    loop {
        terminal.draw(|f| app.draw(f))?;

        if crossterm::event::poll(Duration::from_millis(200))? {
            match event::read()? {
                CEvent::Key(key) if key.kind == KeyEventKind::Press => match app.handle_key(key)? {
                    AppAction::None => {}
                    AppAction::Quit => break,
                    AppAction::Resume(summary) => {
                        outcome = Some(summary);
                        break;
                    }
                },
                _ => {}
            }
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(outcome)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Normal,
    Search,
    ConfirmDelete,
}

struct App {
    sessions: Vec<SessionSummary>,
    filtered: Vec<usize>,
    selected: usize,
    query: String,
    mode: Mode,
    delete_primed_at: Option<Instant>,
    status: Option<String>,
}

enum AppAction {
    None,
    Quit,
    Resume(SessionSummary),
}

impl App {
    fn new(sessions: Vec<SessionSummary>) -> Self {
        let mut app = Self {
            sessions,
            filtered: Vec::new(),
            selected: 0,
            query: String::new(),
            mode: Mode::Normal,
            delete_primed_at: None,
            status: None,
        };
        app.apply_filter();
        app
    }

    fn apply_filter(&mut self) {
        self.filtered = self
            .sessions
            .iter()
            .enumerate()
            .filter(|(_, session)| self.matches_query(session))
            .map(|(idx, _)| idx)
            .collect();
        if self.filtered.is_empty() {
            self.selected = 0;
        } else if self.selected >= self.filtered.len() {
            self.selected = self.filtered.len() - 1;
        }
    }

    fn matches_query(&self, summary: &SessionSummary) -> bool {
        if self.query.is_empty() {
            true
        } else {
            let needle = self.query.to_ascii_lowercase();
            summary.id.to_ascii_lowercase().contains(&needle)
                || summary
                    .preview
                    .as_deref()
                    .map(|p| p.to_ascii_lowercase().contains(&needle))
                    .unwrap_or(false)
                || summary
                    .cwd
                    .as_ref()
                    .map(|p| {
                        p.display()
                            .to_string()
                            .to_ascii_lowercase()
                            .contains(&needle)
                    })
                    .unwrap_or(false)
        }
    }

    fn draw(&self, frame: &mut ratatui::Frame) {
        let layout = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(2),
        ])
        .split(frame.area());

        let title = Line::from(vec![
            Span::styled("Codex Sessions", Style::default().fg(Color::Cyan)),
            Span::raw("  (enter=resume, /=search, dd=delete, q=quit)"),
        ]);
        frame.render_widget(title, layout[0]);

        let search_prompt = match self.mode {
            Mode::Search => format!("/{}", self.query),
            _ => format!("{} sessions", self.filtered.len()),
        };
        frame.render_widget(Line::from(search_prompt), layout[1]);

        let rows: Vec<Row> = self
            .filtered
            .iter()
            .enumerate()
            .map(|(visible_idx, &orig_idx)| {
                let summary = &self.sessions[orig_idx];
                let cwd = summary
                    .cwd
                    .as_ref()
                    .map(|p| crate::shorten_path(p, 28))
                    .unwrap_or_else(|| "(unknown)".into());
                let preview = summary
                    .preview
                    .as_deref()
                    .map(crate::truncate_preview)
                    .unwrap_or_else(|| String::from("(no user input)"));
                let updated = summary
                    .updated_at
                    .map(crate::format_relative)
                    .unwrap_or_else(|| "unknown".into());
                let mut row = Row::new(vec![
                    updated,
                    summary.git_branch.as_deref().unwrap_or("-").to_string(),
                    cwd,
                    preview,
                ]);
                if visible_idx == self.selected {
                    row = row.style(Style::default().fg(Color::Black).bg(Color::Cyan));
                }
                row
            })
            .collect();

        let header = Row::new(vec!["Updated", "Branch", "CWD", "Conversation"])
            .style(Style::default().add_modifier(Modifier::BOLD));
        let table = Table::new(
            rows,
            [
                Constraint::Length(20),
                Constraint::Length(12),
                Constraint::Length(30),
                Constraint::Min(10),
            ],
        )
        .header(header)
        .column_spacing(2)
        .block(Block::default().borders(Borders::ALL));
        frame.render_widget(table, layout[2]);

        if let Some(status) = self.status.as_deref() {
            frame.render_widget(Line::from(status.to_string()), layout[3]);
        }

        if self.mode == Mode::ConfirmDelete {
            let area = centered_rect(60, 20, frame.area());
            let session = self.current_session();
            let text = format!(
                "Delete session {}?\nThis cannot be undone.\nPress y to confirm or n to cancel.",
                session.map(|s| s.id.clone()).unwrap_or_default()
            );
            let block = Paragraph::new(text)
                .style(Style::default().fg(Color::Red))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Confirm delete"),
                );
            frame.render_widget(Clear, area);
            frame.render_widget(block, area);
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> Result<AppAction> {
        self.status = None;
        match self.mode {
            Mode::Normal => self.handle_normal_mode(key),
            Mode::Search => self.handle_search_mode(key),
            Mode::ConfirmDelete => self.handle_confirm_mode(key),
        }
    }

    fn handle_normal_mode(&mut self, key: KeyEvent) -> Result<AppAction> {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => Ok(AppAction::Quit),
            KeyCode::Up | KeyCode::Char('k') => {
                self.move_selection_up();
                Ok(AppAction::None)
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.move_selection_down();
                Ok(AppAction::None)
            }
            KeyCode::Char('/') => {
                self.mode = Mode::Search;
                self.query.clear();
                self.apply_filter();
                Ok(AppAction::None)
            }
            KeyCode::Enter => {
                if let Some(session) = self.current_session() {
                    return Ok(AppAction::Resume(session.clone()));
                }
                Ok(AppAction::None)
            }
            KeyCode::Char('d') => {
                let now = Instant::now();
                if let Some(prime) = self.delete_primed_at {
                    if now.duration_since(prime) <= DELETE_SEQUENCE_TIMEOUT {
                        if self.current_session().is_some() {
                            self.mode = Mode::ConfirmDelete;
                        }
                        self.delete_primed_at = None;
                        return Ok(AppAction::None);
                    }
                }
                self.delete_primed_at = Some(now);
                self.status = Some(String::from("Press d again to delete the selected session"));
                Ok(AppAction::None)
            }
            _ => {
                self.delete_primed_at = None;
                Ok(AppAction::None)
            }
        }
    }

    fn handle_search_mode(&mut self, key: KeyEvent) -> Result<AppAction> {
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                if self.query.is_empty() {
                    self.apply_filter();
                }
            }
            KeyCode::Enter => {
                self.mode = Mode::Normal;
            }
            KeyCode::Backspace => {
                self.query.pop();
                self.apply_filter();
            }
            KeyCode::Char('j') if key.modifiers.is_empty() => self.move_selection_down(),
            KeyCode::Char('k') if key.modifiers.is_empty() => self.move_selection_up(),
            KeyCode::Char(c) => {
                if !key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.query.push(c);
                    self.apply_filter();
                }
            }
            KeyCode::Down => self.move_selection_down(),
            KeyCode::Up => self.move_selection_up(),
            _ => {}
        }
        Ok(AppAction::None)
    }

    fn handle_confirm_mode(&mut self, key: KeyEvent) -> Result<AppAction> {
        match key.code {
            KeyCode::Char('y') => {
                if let Some(session) = self.current_session().cloned() {
                    std::fs::remove_file(&session.path)
                        .with_context(|| format!("failed to delete {:?}", session.path))?;
                    self.sessions.retain(|s| s.path != session.path);
                    self.apply_filter();
                    self.status = Some(format!("Deleted session {}", session.id));
                }
                self.mode = Mode::Normal;
            }
            KeyCode::Char('n') | KeyCode::Esc => {
                self.mode = Mode::Normal;
            }
            _ => {}
        }
        Ok(AppAction::None)
    }

    fn move_selection_up(&mut self) {
        if self.filtered.is_empty() {
            return;
        }
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    fn move_selection_down(&mut self) {
        if self.filtered.is_empty() {
            return;
        }
        if self.selected + 1 < self.filtered.len() {
            self.selected += 1;
        }
    }

    fn current_session(&self) -> Option<&SessionSummary> {
        self.filtered
            .get(self.selected)
            .and_then(|&idx| self.sessions.get(idx))
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup_layout = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .split(area);

    Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(popup_layout[1])[1]
}
