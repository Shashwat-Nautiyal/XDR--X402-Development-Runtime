use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{prelude::*, widgets::*};
use std::{error::Error, io, sync::{Arc, Mutex}, collections::VecDeque, time::Duration};
use chrono::Local;
use xdr_ledger::Ledger;
use xdr_chaos::ChaosEngine;
use xdr_trace::Trace;

pub struct App {
    pub ledger: Ledger,
    pub chaos: ChaosEngine,
    pub traces: Arc<Mutex<VecDeque<Trace>>>,
}

pub async fn run_tui(app_state: App) -> Result<(), Box<dyn Error>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let res = run_app(&mut terminal, app_state).await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        println!("{:?}", err)
    }
    Ok(())
}

async fn run_app<B: Backend>(terminal: &mut Terminal<B>, app: App) -> io::Result<()> {
    loop {
        terminal.draw(|f| ui(f, &app))?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') => return Ok(()),
                    KeyCode::Char('c') => {
                        let cfg = app.chaos.get_config();
                        let mut new_cfg = cfg.clone();
                        new_cfg.enabled = !cfg.enabled;
                        // Defaults if enabling
                        if new_cfg.enabled && new_cfg.global_failure_rate == 0.0 {
                             new_cfg.global_failure_rate = 0.2;
                             new_cfg.min_latency_ms = 200;
                        }
                        app.chaos.set_config(new_cfg);
                    },
                    _ => {}
                }
            }
        }
    }
}

fn ui(f: &mut Frame, app: &App) {
    // LAYOUT DEFINITION
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // 1. Top Status Bar
            Constraint::Min(0),     // 2. Main Content (Agents | Traffic)
            Constraint::Length(3),  // 3. Bottom Controls
        ])
        .split(f.size());

    // --- 1. TOP STATUS BAR ---
    render_top_bar(f, app, main_chunks[0]);

    // --- 2. MAIN CONTENT SPLIT ---
    let content_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(30), // Agents (Left)
            Constraint::Percentage(70), // Traffic (Right)
        ])
        .split(main_chunks[1]);

    render_agents_panel(f, app, content_chunks[0]);
    render_traffic_panel(f, app, content_chunks[1]);

    // --- 3. BOTTOM BAR ---
    render_bottom_bar(f, main_chunks[2]);
}

fn render_top_bar(f: &mut Frame, app: &App, area: Rect) {
    let chaos_cfg = app.chaos.get_config();
    let (chaos_status, chaos_style) = if chaos_cfg.enabled {
        (format!("🌪️ CHAOS: ON ({:.0}%)", chaos_cfg.global_failure_rate * 100.0), Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
    } else {
        ("🛡️ CHAOS: OFF".to_string(), Style::default().fg(Color::Green))
    };

    let time = Local::now().format("%H:%M:%S").to_string();
    
    // Using a Table for the header to align columns perfectly
    let header_cells = Row::new(vec![
        Cell::from(" 🌀 XDR Control Plane v1.0 ").style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Cell::from(chaos_status).style(chaos_style),
        Cell::from(" 🌐 cronos-testnet (338) "),
        Cell::from(format!(" 🕐 {} ", time)),
    ]);

    let header_table = Table::new(vec![header_cells], [
        Constraint::Percentage(25),
        Constraint::Percentage(25),
        Constraint::Percentage(25),
        Constraint::Percentage(25),
    ])
    .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded));
    
    f.render_widget(header_table, area);
}

fn render_agents_panel(f: &mut Frame, app: &App, area: Rect) {
    // NOTE: In a real app, iterate the DashMap. For Hackathon, we mock the single agent state 
    // or you can implement a `.list()` on Ledger.
    // Assuming "agent-007" exists for demo:
    let agent = app.ledger.get_state("agent-007");
    
    let rows = if let Some(a) = agent {
        vec![
            Row::new(vec![
                Cell::from("agent-007").style(Style::default().fg(Color::Cyan)),
                Cell::from(format!("${:.2}", a.balance_usdc)).style(Style::default().fg(if a.balance_usdc < 5.0 { Color::Red } else { Color::Green })),
                Cell::from(format!("${:.2}", a.total_spend)),
                Cell::from("✅ Active"),
            ])
        ]
    } else {
        vec![Row::new(vec![Cell::from("Waiting..."), Cell::from("-"), Cell::from("-"), Cell::from("-")])]
    };

    let table = Table::new(rows, [
        Constraint::Percentage(30),
        Constraint::Percentage(25),
        Constraint::Percentage(25),
        Constraint::Percentage(20),
    ])
    .header(
        Row::new(vec!["ID", "Balance", "Spend", "Status"])
            .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::UNDERLINED))
            .bottom_margin(1)
    )
    .block(Block::default()
        .title(" 👥 Agents ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded));

    f.render_widget(table, area);
}

fn render_traffic_panel(f: &mut Frame, app: &App, area: Rect) {
    let traces = app.traces.lock().unwrap();
    
    // Build list items with nested events for developer visibility
    let items: Vec<ListItem> = traces.iter().rev().take(15).map(|t| {
        let (icon, color) = match t.status_code.unwrap_or(0) {
            200..=299 => ("✓", Color::Green),
            402 => ("$", Color::Yellow),
            429 => ("!", Color::Red),
            500..=599 => ("X", Color::Red),
            _ => ("?", Color::Gray),
        };

        // Main request line
        let mut lines = vec![
            Line::from(vec![
                Span::styled(
                    format!("[{}] ", t.status_code.unwrap_or(0)),
                    Style::default().fg(color).add_modifier(Modifier::BOLD)
                ),
                Span::styled(format!("{} ", t.method), Style::default().fg(Color::Cyan)),
                Span::raw(if t.url.len() > 40 { format!("{}...", &t.url[..37]) } else { t.url.clone() }),
                Span::styled(
                    format!(" ({}ms)", t.duration_ms.unwrap_or(0)),
                    Style::default().fg(Color::DarkGray)
                ),
            ]),
        ];
        
        // Show trace events (funding, payments, balance updates)
        for event in t.events.iter().rev().take(3).rev() {
            let (evt_icon, evt_color) = match event.category {
                xdr_trace::EventCategory::Payment => ("$", Color::Yellow),
                xdr_trace::EventCategory::Chaos => ("!", Color::Red),
                xdr_trace::EventCategory::Info => ("i", Color::Cyan),
                xdr_trace::EventCategory::Upstream => ("^", Color::Blue),
                xdr_trace::EventCategory::Error => ("X", Color::Red),
            };
            
            // Truncate long messages
            let msg = if event.message.len() > 55 {
                format!("{}...", &event.message[..52])
            } else {
                event.message.clone()
            };
            
            lines.push(Line::from(vec![
                Span::raw("   "),
                Span::styled(format!("[{}] ", evt_icon), Style::default().fg(evt_color)),
                Span::styled(msg, Style::default().fg(evt_color)),
            ]));
        }
        
        ListItem::new(Text::from(lines))
    }).collect();

    let list = List::new(items)
        .block(Block::default()
            .title(" 📡 Live Traffic (with Events) ")
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded));

    f.render_widget(list, area);
}

fn render_bottom_bar(f: &mut Frame, area: Rect) {
    let controls = Line::from(vec![
        Span::raw(" Controls: "),
        Span::styled("[Q] Quit ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
        Span::raw(" | "),
        Span::styled("[C] Toggle Chaos ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::raw(" | "),
        Span::raw("[↑/↓] Scroll Logs "), 
    ]);

    let block = Paragraph::new(controls)
        .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded));
    f.render_widget(block, area);
}