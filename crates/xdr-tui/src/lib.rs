//! XDR Terminal User Interface - Clean, Developer-Focused Control Plane

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{prelude::*, widgets::*};
use std::{error::Error, io, sync::{Arc, Mutex}, collections::VecDeque, time::Duration};
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
                        if new_cfg.enabled && new_cfg.global_failure_rate == 0.0 {
                            new_cfg.global_failure_rate = 0.2;
                            new_cfg.min_latency_ms = 200;
                        }
                        app.chaos.set_config(new_cfg);
                    },
                    KeyCode::Char('f') => {
                        let agent_id = "agent-007";
                        if let Some(state) = app.ledger.get_state(agent_id) {
                            app.ledger.set_balance(agent_id, state.balance_usdc + 50.0);
                        } else {
                            app.ledger.set_balance(agent_id, 100.0);
                        }
                    },
                    _ => {}
                }
            }
        }
    }
}

fn ui(f: &mut Frame, app: &App) {
    let area = f.size();
    
    // Clear with dark background
    f.render_widget(Block::default().style(Style::default().bg(Color::Black)), area);
    
    let main_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Header
            Constraint::Min(10),    // Content
            Constraint::Length(3),  // Footer
        ])
        .split(area);

    render_header(f, app, main_layout[0]);
    
    let content_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(35), // Left: Agent Details (fixed width)
            Constraint::Min(40),    // Right: Traffic Log
        ])
        .split(main_layout[1]);

    render_agent_panel(f, app, content_layout[0]);
    render_traffic_panel(f, app, content_layout[1]);
    render_footer(f, main_layout[2]);
}

fn render_header(f: &mut Frame, app: &App, area: Rect) {
    let chaos = app.chaos.get_config();
    let chaos_text = if chaos.enabled {
        Span::styled(" CHAOS ON ", Style::default().bg(Color::Red).fg(Color::White).add_modifier(Modifier::BOLD))
    } else {
        Span::styled(" CHAOS OFF ", Style::default().bg(Color::Green).fg(Color::Black).add_modifier(Modifier::BOLD))
    };
    
    let agents = app.ledger.list_agents();
    let trace_count = app.traces.lock().map(|t| t.len()).unwrap_or(0);
    
    let header = Paragraph::new(Line::from(vec![
        Span::styled(" XDR ", Style::default().bg(Color::Cyan).fg(Color::Black).add_modifier(Modifier::BOLD)),
        Span::raw(" Control Plane | "),
        chaos_text,
        Span::raw(format!(" | Agents: {} | Requests: {} ", agents.len(), trace_count)),
    ]))
    .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Cyan)));
    
    f.render_widget(header, area);
}

fn render_agent_panel(f: &mut Frame, app: &App, area: Rect) {
    let agents = app.ledger.list_agents();
    
    let mut text_lines: Vec<Line> = Vec::new();
    
    if agents.is_empty() {
        text_lines.push(Line::from(""));
        text_lines.push(Line::from(Span::styled(
            "  No agents connected",
            Style::default().fg(Color::DarkGray)
        )));
        text_lines.push(Line::from(""));
        text_lines.push(Line::from("  Run the demo agent:"));
        text_lines.push(Line::from(Span::styled(
            "  npx ts-node index.ts",
            Style::default().fg(Color::Yellow)
        )));
        text_lines.push(Line::from(""));
        text_lines.push(Line::from("  Or press [F] to pre-fund"));
    } else {
        for agent in &agents {
            // Agent ID header
            text_lines.push(Line::from(vec![
                Span::styled(
                    format!(" {} ", agent.id),
                    Style::default().bg(Color::Blue).fg(Color::White).add_modifier(Modifier::BOLD)
                ),
            ]));
            text_lines.push(Line::from(""));
            
            // Balance - prominent display
            let balance_color = if agent.balance_usdc < 5.0 { 
                Color::Red 
            } else if agent.balance_usdc < 20.0 { 
                Color::Yellow 
            } else { 
                Color::Green 
            };
            
            text_lines.push(Line::from(vec![
                Span::raw("  Balance: "),
                Span::styled(
                    format!("${:.2}", agent.balance_usdc),
                    Style::default().fg(balance_color).add_modifier(Modifier::BOLD)
                ),
                Span::styled(" USDC", Style::default().fg(Color::DarkGray)),
            ]));
            
            // Spend info
            text_lines.push(Line::from(vec![
                Span::raw("  Spent:   "),
                Span::styled(
                    format!("${:.2}", agent.total_spend),
                    Style::default().fg(Color::Yellow)
                ),
                Span::styled(
                    format!(" / ${:.0} limit", agent.budget_limit),
                    Style::default().fg(Color::DarkGray)
                ),
            ]));
            
            // Payment count
            text_lines.push(Line::from(vec![
                Span::raw("  Payments: "),
                Span::styled(
                    format!("{}", agent.payment_count),
                    Style::default().fg(Color::Cyan)
                ),
            ]));
            
            // Budget usage bar
            let pct = if agent.budget_limit > 0.0 {
                (agent.total_spend / agent.budget_limit * 100.0).min(100.0)
            } else { 0.0 };
            
            let bar_width = 20;
            let filled = (pct / 100.0 * bar_width as f64) as usize;
            let empty = bar_width - filled;
            
            let bar_color = if pct > 80.0 { Color::Red } 
                           else if pct > 50.0 { Color::Yellow } 
                           else { Color::Green };
            
            text_lines.push(Line::from(""));
            text_lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    "=".repeat(filled),
                    Style::default().fg(bar_color)
                ),
                Span::styled(
                    "-".repeat(empty),
                    Style::default().fg(Color::DarkGray)
                ),
                Span::styled(
                    format!(" {:.0}%", pct),
                    Style::default().fg(bar_color)
                ),
            ]));
            
            text_lines.push(Line::from(""));
        }
    }
    
    let panel = Paragraph::new(text_lines)
        .block(Block::default()
            .title(" Agent Wallet ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Blue)));
    
    f.render_widget(panel, area);
}

fn render_traffic_panel(f: &mut Frame, app: &App, area: Rect) {
    let traces = app.traces.lock().unwrap();
    
    // Calculate visible rows (subtract 3 for borders and header)
    let visible_rows = (area.height as usize).saturating_sub(3);
    
    let mut text_lines: Vec<Line> = Vec::new();
    
    if traces.is_empty() {
        text_lines.push(Line::from(""));
        text_lines.push(Line::from(Span::styled(
            "  Waiting for requests...",
            Style::default().fg(Color::DarkGray)
        )));
    } else {
        // Get the agent's current balance for display
        let current_balance = app.ledger.get_state("agent-007")
            .map(|a| a.balance_usdc)
            .unwrap_or(0.0);
        
        // Show most recent traces
        for trace in traces.iter().rev().take(visible_rows) {
            let status = trace.status_code.unwrap_or(0);
            let (status_style, status_label) = match status {
                200..=299 => (Style::default().fg(Color::Green), "OK "),
                402 => (Style::default().fg(Color::Yellow), "PAY"),
                429 => (Style::default().fg(Color::Magenta), "LIM"),
                500..=599 => (Style::default().fg(Color::Red), "ERR"),
                _ => (Style::default().fg(Color::Gray), "???"),
            };
            
            // Truncate path
            let path = if trace.url.len() > 30 {
                format!("...{}", &trace.url[trace.url.len()-27..])
            } else {
                trace.url.clone()
            };
            
            let latency = trace.duration_ms.unwrap_or(0);
            let latency_style = match latency {
                0..=100 => Style::default().fg(Color::Green),
                101..=300 => Style::default().fg(Color::Yellow),
                _ => Style::default().fg(Color::Red),
            };
            
            text_lines.push(Line::from(vec![
                Span::styled(format!(" {:>3} ", status), status_style.add_modifier(Modifier::BOLD)),
                Span::styled(status_label, status_style),
                Span::raw(" "),
                Span::styled(format!("{:<5}", trace.method), Style::default().fg(Color::Cyan)),
                Span::raw(" "),
                Span::raw(path),
                Span::raw(" "),
                Span::styled(format!("{:>4}ms", latency), latency_style),
            ]));
            
            // Show balance change for payment events
            if status == 200 || status == 402 {
                for event in &trace.events {
                    if matches!(event.category, xdr_trace::EventCategory::Payment) {
                        // Truncate message if needed
                        let msg = if event.message.len() > 50 {
                            format!("{}...", &event.message[..47])
                        } else {
                            event.message.clone()
                        };
                        text_lines.push(Line::from(vec![
                            Span::raw("      "),
                            Span::styled("$ ", Style::default().fg(Color::Yellow)),
                            Span::styled(msg, Style::default().fg(Color::DarkGray)),
                        ]));
                        break; // Only show first payment event
                    }
                }
            }
        }
    }
    
    let panel = Paragraph::new(text_lines)
        .block(Block::default()
            .title(" Request Log ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Magenta)));
    
    f.render_widget(panel, area);
}

fn render_footer(f: &mut Frame, area: Rect) {
    let footer = Paragraph::new(Line::from(vec![
        Span::styled(" [Q] ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
        Span::raw("Quit  "),
        Span::styled(" [C] ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::raw("Toggle Chaos  "),
        Span::styled(" [F] ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        Span::raw("Fund +$50  "),
    ]))
    .alignment(Alignment::Center)
    .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::DarkGray)));
    
    f.render_widget(footer, area);
}
