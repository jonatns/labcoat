use crate::contract::Ctx;
use crate::trace_view::{self, TraceLine};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use isomer_core::{Devnet, LogEntry, SystemStatus};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, Borders, Cell, Clear, List, ListItem, ListState, Paragraph, Row, Table, Tabs, Wrap,
};
use ratatui::Frame;
use std::io::IsTerminal;
use std::time::Duration;
use tokio::sync::mpsc;

const SERVICES: &[&str] = &[
    "all",
    "bitcoind",
    "metashrew",
    "ord",
    "esplora",
    "espo",
    "jsonrpc",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Overview,
    Logs,
    Trace,
}

impl Tab {
    fn index(self) -> usize {
        match self {
            Self::Overview => 0,
            Self::Logs => 1,
            Self::Trace => 2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InputMode {
    Normal,
    Txid,
    Search,
}

enum Message {
    Status(SystemStatus),
    Logs(Vec<LogEntry>),
    Trace(Result<(String, Vec<TraceLine>), String>),
}

struct App {
    tab: Tab,
    mode: InputMode,
    input: String,
    search: String,
    status: Option<SystemStatus>,
    logs: Vec<LogEntry>,
    log_offset: usize,
    follow_logs: bool,
    service_index: usize,
    trace_txid: Option<String>,
    trace_lines: Vec<TraceLine>,
    trace_state: ListState,
    trace_loading: bool,
    trace_expanded: bool,
    help: bool,
    error: Option<String>,
    network: String,
    rpc_url: String,
}

impl App {
    fn new(ctx: &Ctx) -> Self {
        let mut trace_state = ListState::default();
        trace_state.select(Some(0));
        Self {
            tab: Tab::Overview,
            mode: InputMode::Normal,
            input: String::new(),
            search: String::new(),
            status: None,
            logs: Vec::new(),
            log_offset: 0,
            follow_logs: true,
            service_index: 0,
            trace_txid: None,
            trace_lines: Vec::new(),
            trace_state,
            trace_loading: false,
            trace_expanded: false,
            help: false,
            error: None,
            network: ctx.config.normalized_network(),
            rpc_url: ctx.config.jsonrpc_url.clone(),
        }
    }

    fn visible_logs(&self) -> Vec<&LogEntry> {
        let service = SERVICES[self.service_index];
        let needle = self.search.to_ascii_lowercase();
        self.logs
            .iter()
            .filter(|entry| service == "all" || entry.service == service)
            .filter(|entry| {
                needle.is_empty()
                    || entry.message.to_ascii_lowercase().contains(&needle)
                    || entry.service.to_ascii_lowercase().contains(&needle)
            })
            .collect()
    }

    fn visible_trace_indices(&self) -> Vec<usize> {
        let needle = self.search.to_ascii_lowercase();
        self.trace_lines
            .iter()
            .enumerate()
            .filter(|(_, line)| {
                needle.is_empty()
                    || line.summary.to_ascii_lowercase().contains(&needle)
                    || line.raw.to_ascii_lowercase().contains(&needle)
            })
            .map(|(index, _)| index)
            .collect()
    }

    fn next_tab(&mut self) {
        self.tab = match self.tab {
            Tab::Overview => Tab::Logs,
            Tab::Logs => Tab::Trace,
            Tab::Trace => Tab::Overview,
        };
    }

    fn move_selection(&mut self, down: bool) {
        match self.tab {
            Tab::Logs => {
                self.follow_logs = false;
                let len = self.visible_logs().len();
                if down {
                    self.log_offset = self.log_offset.saturating_add(1).min(len.saturating_sub(1));
                } else {
                    self.log_offset = self.log_offset.saturating_sub(1);
                }
            }
            Tab::Trace => {
                let len = self.visible_trace_indices().len();
                let current = self.trace_state.selected().unwrap_or(0);
                let next = if down {
                    current.saturating_add(1).min(len.saturating_sub(1))
                } else {
                    current.saturating_sub(1)
                };
                self.trace_state.select(Some(next));
            }
            Tab::Overview => {}
        }
    }

    fn selected_trace(&self) -> Option<&TraceLine> {
        let visible = self.visible_trace_indices();
        let visible_index = self.trace_state.selected().unwrap_or(0);
        visible
            .get(visible_index)
            .and_then(|index| self.trace_lines.get(*index))
    }
}

pub async fn run(ctx: Ctx) -> Result<(), String> {
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return Err("the terminal inspector requires an interactive stdin and stdout".into());
    }

    let (sender, mut receiver) = mpsc::unbounded_channel();
    spawn_status_source(sender.clone());
    spawn_log_source(sender.clone());
    let trace_config = ctx.config.clone();
    let mut app = App::new(&ctx);
    let mut terminal = ratatui::init();

    let result = async {
        loop {
            while let Ok(message) = receiver.try_recv() {
                apply_message(&mut app, message);
            }
            terminal
                .draw(|frame| draw(frame, &mut app))
                .map_err(|error| error.to_string())?;

            if event::poll(Duration::from_millis(100)).map_err(|error| error.to_string())? {
                if let Event::Key(key) = event::read().map_err(|error| error.to_string())? {
                    if handle_key(&mut app, key, &sender, &trace_config) {
                        break;
                    }
                }
            }
        }
        Ok(())
    }
    .await;

    ratatui::restore();
    result
}

fn spawn_status_source(sender: mpsc::UnboundedSender<Message>) {
    tokio::spawn(async move {
        let mut devnet = Devnet::new();
        loop {
            let status = devnet.status().await;
            if sender.send(Message::Status(status)).is_err() {
                break;
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    });
}

fn spawn_log_source(sender: mpsc::UnboundedSender<Message>) {
    tokio::spawn(async move {
        let devnet = Devnet::new();
        loop {
            let logs = devnet.logs(None, 5_000);
            if sender.send(Message::Logs(logs)).is_err() {
                break;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });
}

fn fetch_trace(
    sender: mpsc::UnboundedSender<Message>,
    config: labcoat_core::ToolkitConfig,
    txid: String,
) {
    // The pinned upstream provider exposes a !Send trace future. Keep it off
    // the render loop with a small current-thread runtime on a worker thread.
    std::thread::spawn(move || {
        let result = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| error.to_string())
            .and_then(|runtime| {
                runtime.block_on(async {
                    labcoat_core::toolkit::trace(&config, &txid, false)
                        .await
                        .map(|traces| {
                            let value =
                                serde_json::to_value(traces).unwrap_or(serde_json::Value::Null);
                            (txid, trace_view::normalize(&value))
                        })
                        .map_err(|error| format!("{}: {}", error.code, error.message))
                })
            });
        let _ = sender.send(Message::Trace(result));
    });
}

fn apply_message(app: &mut App, message: Message) {
    match message {
        Message::Status(status) => app.status = Some(status),
        Message::Logs(mut logs) => {
            if logs.len() > 5_000 {
                logs.drain(..logs.len() - 5_000);
            }
            app.logs = logs;
            if app.follow_logs {
                app.log_offset = app.visible_logs().len().saturating_sub(1);
            }
        }
        Message::Trace(result) => {
            app.trace_loading = false;
            match result {
                Ok((txid, lines)) => {
                    app.trace_txid = Some(txid);
                    app.trace_lines = lines;
                    app.trace_state.select(Some(0));
                    app.error = None;
                }
                Err(error) => app.error = Some(error),
            }
        }
    }
}

fn handle_key(
    app: &mut App,
    key: KeyEvent,
    sender: &mpsc::UnboundedSender<Message>,
    trace_config: &labcoat_core::ToolkitConfig,
) -> bool {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return true;
    }
    if app.help {
        if matches!(
            key.code,
            KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q')
        ) {
            app.help = false;
        }
        return false;
    }
    if app.mode != InputMode::Normal {
        match key.code {
            KeyCode::Esc => {
                app.mode = InputMode::Normal;
                app.input.clear();
            }
            KeyCode::Backspace => {
                app.input.pop();
            }
            KeyCode::Enter => {
                let input = std::mem::take(&mut app.input);
                match app.mode {
                    InputMode::Txid => {
                        if valid_txid(&input) {
                            app.trace_loading = true;
                            app.error = None;
                            fetch_trace(sender.clone(), trace_config.clone(), input);
                        } else {
                            app.error = Some(
                                "transaction id must be exactly 64 hexadecimal characters".into(),
                            );
                        }
                    }
                    InputMode::Search => {
                        app.search = input;
                        app.trace_state.select(Some(0));
                        app.log_offset = 0;
                    }
                    InputMode::Normal => {}
                }
                app.mode = InputMode::Normal;
            }
            KeyCode::Char(character) => app.input.push(character),
            _ => {}
        }
        return false;
    }

    match key.code {
        KeyCode::Char('q') => return true,
        KeyCode::Esc => {
            if !app.search.is_empty() || app.error.is_some() {
                app.search.clear();
                app.error = None;
            } else {
                return true;
            }
        }
        KeyCode::Tab => app.next_tab(),
        KeyCode::Char('1') => app.tab = Tab::Overview,
        KeyCode::Char('2') => app.tab = Tab::Logs,
        KeyCode::Char('3') => app.tab = Tab::Trace,
        KeyCode::Char('?') => app.help = true,
        KeyCode::Char('/') => {
            app.mode = InputMode::Search;
            app.input = app.search.clone();
        }
        KeyCode::Char('o') if app.tab == Tab::Trace => {
            app.mode = InputMode::Txid;
            app.input.clear();
        }
        KeyCode::Char('f') if app.tab == Tab::Logs => {
            app.service_index = (app.service_index + 1) % SERVICES.len();
            app.log_offset = 0;
        }
        KeyCode::Char(' ') if app.tab == Tab::Logs => app.follow_logs = !app.follow_logs,
        KeyCode::Enter | KeyCode::Char(' ') if app.tab == Tab::Trace => {
            app.trace_expanded = !app.trace_expanded;
        }
        KeyCode::Down | KeyCode::Char('j') => app.move_selection(true),
        KeyCode::Up | KeyCode::Char('k') => app.move_selection(false),
        KeyCode::Char('r') if app.tab == Tab::Trace => {
            if let Some(txid) = app.trace_txid.clone() {
                app.trace_loading = true;
                fetch_trace(sender.clone(), trace_config.clone(), txid);
            }
        }
        _ => {}
    }
    false
}

fn valid_txid(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    if area.width < 60 || area.height < 20 {
        frame.render_widget(
            Paragraph::new("Labcoat needs a terminal of at least 60×20. Resize to continue.")
                .alignment(ratatui::layout::Alignment::Center)
                .wrap(Wrap { trim: true })
                .block(Block::default().borders(Borders::ALL).title(" Labcoat ")),
            area,
        );
        return;
    }

    let [header, content, footer] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(1),
        Constraint::Length(if app.mode == InputMode::Normal { 2 } else { 3 }),
    ])
    .areas(area);
    draw_header(frame, app, header);
    match app.tab {
        Tab::Overview => draw_overview(frame, app, content),
        Tab::Logs => draw_logs(frame, app, content),
        Tab::Trace => draw_trace(frame, app, content),
    }
    draw_footer(frame, app, footer);

    if app.help {
        draw_help(frame, centered_rect(70, 70, area));
    }
}

fn draw_header(frame: &mut Frame, app: &App, area: Rect) {
    let tabs = Tabs::new(vec!["1 Overview", "2 Logs", "3 Trace"])
        .select(app.tab.index())
        .style(Style::default().fg(Color::Gray))
        .highlight_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .divider("  ")
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Labcoat Inspector "),
        );
    frame.render_widget(tabs, area);
}

fn draw_overview(frame: &mut Frame, app: &App, area: Rect) {
    let [summary, services] =
        Layout::vertical([Constraint::Length(5), Constraint::Min(1)]).areas(area);
    let (ready, height, mempool) = app
        .status
        .as_ref()
        .map(|status| (status.is_ready, status.block_height, status.mempool_size))
        .unwrap_or((false, 0, 0));
    let status = if app.status.is_none() {
        Span::styled("loading…", Style::default().fg(Color::Yellow))
    } else if ready {
        Span::styled("✓ ready", Style::default().fg(Color::Green))
    } else {
        Span::styled("! not ready", Style::default().fg(Color::Yellow))
    };
    let summary_text = Text::from(vec![
        Line::from(vec![
            Span::styled("Network  ", label_style()),
            Span::raw(&app.network),
            Span::raw("    "),
            status,
        ]),
        Line::from(vec![
            Span::styled("RPC      ", label_style()),
            Span::raw(&app.rpc_url),
        ]),
        Line::from(vec![
            Span::styled("Chain    ", label_style()),
            Span::raw(format!("height {height}  ·  mempool {mempool}")),
        ]),
    ]);
    frame.render_widget(
        Paragraph::new(summary_text).block(panel("Overview")),
        summary,
    );

    let rows = app
        .status
        .as_ref()
        .map(|status| {
            status
                .services
                .iter()
                .map(|service| {
                    let status_cell = if service.status == "running" {
                        Cell::from("✓ running").style(Style::default().fg(Color::Green))
                    } else {
                        Cell::from(format!("✗ {}", service.status))
                            .style(Style::default().fg(Color::Red))
                    };
                    Row::new(vec![
                        Cell::from(service.name.clone()),
                        status_cell,
                        Cell::from(service.port.to_string()),
                        Cell::from(service.version.clone().unwrap_or_default()),
                    ])
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let table = Table::new(
        rows,
        [
            Constraint::Percentage(35),
            Constraint::Length(14),
            Constraint::Length(8),
            Constraint::Min(10),
        ],
    )
    .header(Row::new(["Service", "Status", "Port", "Version"]).style(label_style()))
    .block(panel("Services"));
    frame.render_widget(table, services);
}

fn draw_logs(frame: &mut Frame, app: &App, area: Rect) {
    let visible = app.visible_logs();
    let height = usize::from(area.height.saturating_sub(2));
    let start = if app.follow_logs {
        visible.len().saturating_sub(height)
    } else {
        app.log_offset.min(visible.len().saturating_sub(1))
    };
    let lines = visible
        .iter()
        .skip(start)
        .take(height)
        .map(|entry| {
            Line::from(vec![
                Span::styled(
                    format!("[{}] ", entry.service),
                    Style::default().fg(Color::Cyan),
                ),
                Span::raw(entry.message.clone()),
            ])
        })
        .collect::<Vec<_>>();
    let title = format!(
        "Logs · {} · {}{}",
        SERVICES[app.service_index],
        if app.follow_logs {
            "following"
        } else {
            "paused"
        },
        if app.search.is_empty() {
            String::new()
        } else {
            format!(" · /{}", app.search)
        }
    );
    frame.render_widget(Paragraph::new(lines).block(panel(&title)), area);
}

fn draw_trace(frame: &mut Frame, app: &mut App, area: Rect) {
    if app.trace_loading {
        frame.render_widget(
            Paragraph::new("Fetching trace…").block(panel("Trace")),
            area,
        );
        return;
    }
    if app.trace_lines.is_empty() {
        let text = app
            .error
            .as_deref()
            .unwrap_or("Press o to open a transaction trace.");
        frame.render_widget(
            Paragraph::new(text)
                .wrap(Wrap { trim: true })
                .block(panel("Trace")),
            area,
        );
        return;
    }
    let direction = if area.width >= 80 {
        Direction::Horizontal
    } else {
        Direction::Vertical
    };
    let chunks = Layout::default()
        .direction(direction)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);
    let indices = app.visible_trace_indices();
    let items = indices
        .iter()
        .map(|index| {
            let line = &app.trace_lines[*index];
            ListItem::new(Line::from(vec![
                Span::raw("  ".repeat(line.depth)),
                Span::styled("• ", Style::default().fg(Color::Cyan)),
                Span::raw(line.summary.clone()),
            ]))
        })
        .collect::<Vec<_>>();
    let title = app
        .trace_txid
        .as_ref()
        .map(|txid| format!("Events · {txid}"))
        .unwrap_or_else(|| "Events".into());
    let list = List::new(items)
        .block(panel(&title))
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("› ");
    frame.render_stateful_widget(list, chunks[0], &mut app.trace_state);

    let selected = app.selected_trace();
    let details = selected
        .map(|line| {
            if app.trace_expanded {
                line.raw.clone()
            } else {
                format!(
                    "{}\n\nPress Enter or Space to expand raw details.",
                    line.summary
                )
            }
        })
        .unwrap_or_default();
    frame.render_widget(
        Paragraph::new(details)
            .wrap(Wrap { trim: false })
            .block(panel("Details")),
        chunks[1],
    );
}

fn draw_footer(frame: &mut Frame, app: &App, area: Rect) {
    if app.mode != InputMode::Normal {
        let prompt = if app.mode == InputMode::Txid {
            "Transaction ID"
        } else {
            "Search"
        };
        frame.render_widget(
            Paragraph::new(format!("{prompt}: {}_", app.input))
                .block(Block::default().borders(Borders::ALL)),
            area,
        );
        return;
    }
    let mut text = "Tab/1–3 switch  ·  j/k move  ·  / search  ·  ? help  ·  q quit".to_string();
    if app.tab == Tab::Logs {
        text = format!("f filter  ·  Space follow/pause  ·  {text}");
    } else if app.tab == Tab::Trace {
        text = format!("o open txid  ·  Enter expand  ·  r refresh  ·  {text}");
    }
    if let Some(error) = &app.error {
        text.push_str(&format!("\nerror: {error}"));
    }
    frame.render_widget(
        Paragraph::new(text).style(Style::default().fg(Color::Gray)),
        area,
    );
}

fn draw_help(frame: &mut Frame, area: Rect) {
    frame.render_widget(Clear, area);
    let help = Paragraph::new(
        "1–3 / Tab    Switch tabs\n\
         j/k / arrows Move selection\n\
         /            Search current view\n\
         f            Cycle log service filter\n\
         Space        Follow logs / expand trace\n\
         o            Open transaction trace\n\
         r            Refresh current trace\n\
         Esc          Clear search or close\n\
         q / Ctrl-C   Quit",
    )
    .block(panel("Help · ? to close"))
    .wrap(Wrap { trim: false });
    frame.render_widget(help, area);
}

fn panel(title: &str) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .title(format!(" {title} "))
}

fn label_style() -> Style {
    Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD)
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::vertical([
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
    .split(vertical[1])[1]
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn app() -> App {
        let ctx = Ctx::new(
            "regtest",
            "http://127.0.0.1:18888",
            "wallet.json",
            Some(2.0),
        );
        App::new(&ctx)
    }

    #[test]
    fn validates_transaction_ids() {
        assert!(valid_txid(&"a".repeat(64)));
        assert!(!valid_txid("abc"));
        assert!(!valid_txid(&"z".repeat(64)));
    }

    #[test]
    fn renders_minimum_size_message() {
        let mut app = app();
        let mut terminal = Terminal::new(TestBackend::new(40, 10)).unwrap();
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let rendered = format!("{}", terminal.backend());
        assert!(rendered.contains("60×20"));
    }

    #[test]
    fn renders_trace_summary_and_details() {
        let mut app = app();
        app.tab = Tab::Trace;
        app.trace_txid = Some("a".repeat(64));
        app.trace_lines = vec![TraceLine {
            depth: 0,
            summary: "return 3  fuel used 0".into(),
            raw: "{\"type\":\"return\"}".into(),
        }];
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let rendered = format!("{}", terminal.backend());
        assert!(rendered.contains("return 3"));
        assert!(rendered.contains("Press Enter"));
    }

    #[test]
    fn renders_overview_and_log_tabs() {
        let mut app = app();
        app.status = Some(SystemStatus {
            services: vec![isomer_core::ServiceInfo {
                id: "bitcoind".into(),
                name: "Bitcoin Core".into(),
                status: "running".into(),
                pid: None,
                port: 18443,
                uptime_secs: None,
                version: Some("28.0".into()),
            }],
            block_height: 101,
            mempool_size: 2,
            is_ready: true,
        });
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let overview = format!("{}", terminal.backend());
        assert!(overview.contains("Bitcoin Core"));
        assert!(overview.contains("height 101"));

        app.tab = Tab::Logs;
        app.logs = vec![LogEntry {
            service: "bitcoind".into(),
            timestamp: 0,
            message: "UpdateTip: new best block".into(),
            is_stderr: false,
        }];
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let logs = format!("{}", terminal.backend());
        assert!(logs.contains("UpdateTip"));
        assert!(logs.contains("following"));
    }

    #[test]
    fn help_overlay_lists_contextual_keys() {
        let mut app = app();
        app.help = true;
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let rendered = format!("{}", terminal.backend());
        assert!(rendered.contains("Open transaction trace"));
        assert!(rendered.contains("Ctrl-C"));
    }
}
