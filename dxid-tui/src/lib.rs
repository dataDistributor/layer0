use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::Span,
    widgets::{Block, Borders, Paragraph, Tabs},
    Terminal,
};
use std::io;
use std::time::{Duration, Instant};
use tokio::runtime::Runtime;

pub fn launch_tui() -> Result<()> {
    let rt = Runtime::new()?;
    rt.block_on(async { run_ui().await })
}

async fn run_ui() -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let tabs = vec![
        "Dashboard",
        "Wallet",
        "Identities",
        "Chains",
        "Bridge",
        "Mining",
        "AI",
    ];
    let mut active = 0usize;
    let mut input = String::new();
    let mut ai_output = String::new();
    let mut last_tick = Instant::now();

    loop {
        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(3), Constraint::Min(0)].as_ref())
                .split(f.size());
            let titles: Vec<Span> = tabs
                .iter()
                .map(|t| Span::styled(*t, Style::default().fg(Color::Cyan)))
                .collect();
            let tabs_widget = Tabs::new(titles)
                .select(active)
                .block(Block::default().borders(Borders::ALL).title("dxid"))
                .style(Style::default().fg(Color::White))
                .highlight_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
            f.render_widget(tabs_widget, chunks[0]);

            match active {
                0 => {
                    let para = Paragraph::new("Dashboard\nHeight: n/a\nPeers: n/a");
                    f.render_widget(para, chunks[1]);
                }
                6 => {
                    let area = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([Constraint::Min(5), Constraint::Length(3)].as_ref())
                        .split(chunks[1]);
                    f.render_widget(
                        Paragraph::new(ai_output.clone()).block(Block::default().title("AI")),
                        area[0],
                    );
                    f.render_widget(
                        Paragraph::new(input.clone())
                            .block(Block::default().title("Prompt").borders(Borders::ALL)),
                        area[1],
                    );
                }
                _ => {
                    let para = Paragraph::new("Use number keys 1-7 to switch tabs. q to quit.");
                    f.render_widget(para, chunks[1]);
                }
            }
        })?;

        let timeout = Duration::from_millis(250);
        let poll = event::poll(timeout)?;
        if poll {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Char('1') => active = 0,
                    KeyCode::Char('2') => active = 1,
                    KeyCode::Char('3') => active = 2,
                    KeyCode::Char('4') => active = 3,
                    KeyCode::Char('5') => active = 4,
                    KeyCode::Char('6') => active = 5,
                    KeyCode::Char('7') => active = 6,
                    KeyCode::Enter if active == 6 => {
                        // Fake AI query for now; integration happens via dxid-ai-hypervisor.
                        ai_output = format!("Hypervisor would answer: {}", input);
                        input.clear();
                    }
                    KeyCode::Char(c) if active == 6 => input.push(c),
                    KeyCode::Backspace if active == 6 => {
                        input.pop();
                    }
                    _ => {}
                }
            }
        }

        if last_tick.elapsed() > Duration::from_secs(5) {
            last_tick = Instant::now();
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn placeholder() {
        // TUI is interactive; skip runtime tests.
        assert!(true);
    }
}
