//! XDR Terminal User Interface
//! 
//! A developer-focused "control plane" that shows real-time backend internals:
//! - Active agents with balance/spend tracking
//! - Live request trace log with payment and chaos events
//! - System status (chaos mode, network, metrics)

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{prelude::*, widgets::*};
use std::{error::Error, io, sync::{Arc, Mutex}, collections::VecDeque, time::Duration};
use xdr_ledger::Ledger;
use xdr_chaos::ChaosEngine;
use xdr_trace::{Trace, EventCategory};

/// Shared application state passed from main.rs
pub struct App {
    pub ledger: Ledger,
    pub chaos: ChaosEngine,
    pub traces: Arc<Mutex<VecDeque<Trace>>>,
    pub network: String,
}

/// Entry point for the TUI - sets up terminal and runs the main loop
pub async fn run_tui(app_state: App) -> Result<(), Box<dyn Error>> {
    // Setup terminal for raw mode (direct key capture)
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Run the main loop
    let res = run_app(&mut terminal, app_state).await;

    // Restore terminal to normal mode on exit
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        println!("{:?}", err)
    }

    Ok(())
}

/// Main render loop - polls for input and redraws UI at ~10 FPS
async fn run_app<B: Backend>(terminal: &mut Terminal<B>, app: App) -> io::Result<()> {
    loop {
        terminal.draw(|f| ui(f, &app))?;

        // Poll for keyboard input with 100ms timeout (allows UI updates)
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') => return Ok(()),
                    _ => {}
                }
            }
        }
    }
}

/// Renders the entire UI frame
fn ui(f: &mut Frame, app: &App) {
    // Main layout: Header | Content | Footer
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Min(0),    // Content (flexible)
            Constraint::Length(3), // Footer
        ])
        .split(f.size());

    render_header(f, app, chunks[0]);
    render_content(f, app, chunks[1]);
    render_footer(f, chunks[2]);
}

/// Header: System status bar with chaos state, network, and metrics
fn render_header(f: &mut Frame, app: &App, area: Rect) {
    let chaos_config = app.chaos.get_config();
    let agent_count = app.ledger.list_all_agents().len();
    
    // Count recent requests (from traces)
    let trace_count = app.traces.lock().map(|t| t.len()).unwrap_or(0);
    
    let status_text = if chaos_config.enabled {
        format!(
            " CHAOS: ON (Rate: {:.0}%) | Network: {} | Agents: {} | Requests: {} ",
            chaos_config.global_failure_rate * 100.0,
            app.network,
            agent_count,
            trace_count
        )
    } else {
        format!(
            " CHAOS: OFF | Network: {} | Agents: {} | Requests: {} ",
            app.network,
            agent_count,
            trace_count
        )
    };
    
    let header = Paragraph::new(status_text)
        .block(Block::default().borders(Borders::ALL).title(" XDR Control Plane "))
        .style(Style::default().fg(if chaos_config.enabled { Color::Red } else { Color::Green }));
    
    f.render_widget(header, area);
}

/// Content: Split view with Agents on left, Trace log on right
fn render_content(f: &mut Frame, app: &App, area: Rect) {
    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(area);

    render_agents_panel(f, app, main_chunks[0]);
    render_traces_panel(f, app, main_chunks[1]);
}

/// Left panel: Active agents with balance, spend, and budget usage
fn render_agents_panel(f: &mut Frame, app: &App, area: Rect) {
    let agents = app.ledger.list_all_agents();
    
    let mut items: Vec<ListItem> = Vec::new();
    
    for agent in agents {
        // Calculate budget usage percentage
        let budget_pct = if agent.budget_limit > 0.0 {
            (agent.total_spend / agent.budget_limit * 100.0).min(100.0)
        } else {
            0.0
        };
        
        // Build visual budget bar: format uses filled/empty blocks
        let bar_width = 10;
        let filled = ((budget_pct / 100.0) * bar_width as f64) as usize;
        let empty = bar_width - filled;
        let bar = format!("{}{}", "#".repeat(filled), "-".repeat(empty));
        
        // Color based on budget consumption
        let style = if budget_pct > 80.0 {
            Style::default().fg(Color::Red)
        } else if budget_pct > 50.0 {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::Green)
        };
        
        // Format agent card as multi-line text block
        let content = Text::from(vec![
            Line::from(vec![
                Span::styled(format!("> {}", agent.id), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            ]),
            Line::from(format!("  Balance: ${:.2} USDC", agent.balance_usdc)),
            Line::from(format!("  Spend: ${:.2} / ${:.0}", agent.total_spend, agent.budget_limit)),
            Line::from(format!("  Payments: {}", agent.payment_count)),
            Line::from(vec![
                Span::raw("  "),
                Span::styled(bar, style),
                Span::raw(format!(" {:.1}%", budget_pct)),
            ]),
            Line::from(""),
        ]);
        
        items.push(ListItem::new(content));
    }
    
    // Show placeholder if no agents registered yet
    if items.is_empty() {
        items.push(ListItem::new(Text::from(vec![
            Line::from(Span::styled("  Waiting for agents...", Style::default().fg(Color::DarkGray))),
            Line::from(""),
            Line::from(Span::raw("  Run an agent to see it here:")),
            Line::from(Span::styled("  cd examples/agents", Style::default().fg(Color::Cyan))),
            Line::from(Span::styled("  npx ts-node index.ts", Style::default().fg(Color::Cyan))),
        ])));
    }
    
    let agents_list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(" Agents "));
    
    f.render_widget(agents_list, area);
}

/// Right panel: Live trace log with nested events (payments, chaos, etc.)
fn render_traces_panel(f: &mut Frame, app: &App, area: Rect) {
    let traces_lock = app.traces.lock().unwrap();
    
    let mut items: Vec<ListItem> = Vec::new();
    
    // Show most recent traces first (reversed), limit to what fits
    for trace in traces_lock.iter().rev().take(30) {
        // Color code by status
        let status_style = match trace.status_code.unwrap_or(0) {
            200..=299 => Style::default().fg(Color::Green),
            402 => Style::default().fg(Color::Yellow),
            400..=499 => Style::default().fg(Color::Magenta),
            500..=599 => Style::default().fg(Color::Red),
            _ => Style::default().fg(Color::DarkGray),
        };
        
        // Truncate URL for display
        let url_display = if trace.url.len() > 35 {
            format!("{}...", &trace.url[..32])
        } else {
            trace.url.clone()
        };
        
        // Duration display
        let duration = trace.duration_ms.map(|d| format!("{}ms", d)).unwrap_or_else(|| "...".to_string());
        
        // Main trace line
        let mut lines = vec![
            Line::from(vec![
                Span::styled(
                    format!("[{}]", trace.status_code.unwrap_or(0)),
                    status_style.add_modifier(Modifier::BOLD)
                ),
                Span::raw(" "),
                Span::styled(&trace.method, Style::default().fg(Color::Cyan)),
                Span::raw(" "),
                Span::raw(url_display),
                Span::styled(format!(" ({})", duration), Style::default().fg(Color::DarkGray)),
            ]),
        ];
        
        // Add nested event lines (indented) - show last 2 events per trace
        for event in trace.events.iter().rev().take(2).rev() {
            let (icon, color) = match event.category {
                EventCategory::Payment => ("$", Color::Yellow),
                EventCategory::Chaos => ("!", Color::Red),
                EventCategory::Upstream => ("^", Color::Blue),
                EventCategory::Error => ("X", Color::Red),
                EventCategory::Info => ("i", Color::Gray),
            };
            
            // Truncate message if too long
            let msg = if event.message.len() > 45 {
                format!("{}...", &event.message[..42])
            } else {
                event.message.clone()
            };
            
            lines.push(Line::from(vec![
                Span::raw("   |- "),
                Span::styled(format!("[{}] ", icon), Style::default().fg(color)),
                Span::styled(msg, Style::default().fg(color)),
            ]));
        }
        
        items.push(ListItem::new(Text::from(lines)));
    }
    
    // Placeholder when no traffic yet
    if items.is_empty() {
        items.push(ListItem::new(Text::from(vec![
            Line::from(Span::styled("  Waiting for requests...", Style::default().fg(Color::DarkGray))),
            Line::from(""),
            Line::from(Span::raw("  Traffic will appear here in real-time")),
        ])));
    }
    
    let traces_list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(" Live Traffic "));
    
    f.render_widget(traces_list, area);
}

/// Footer: Keyboard controls help bar
fn render_footer(f: &mut Frame, area: Rect) {
    let help_text = " [q] Quit | Run agents in another terminal to see live traffic ";
    
    let footer = Paragraph::new(help_text)
        .block(Block::default().borders(Borders::ALL))
        .style(Style::default().fg(Color::DarkGray));
    
    f.render_widget(footer, area);
}
