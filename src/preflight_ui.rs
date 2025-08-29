use color_eyre::Result;
use ratatui::{
    DefaultTerminal, Frame,
    crossterm::event::{self, Event, KeyCode, KeyEventKind, poll},
    layout::{Constraint, Layout, Alignment},
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Gauge},
};
use std::time::{Duration, Instant};
use tokio::time::sleep;

use crate::ollama::{self, OllamaError};

pub enum CheckStatus {
    Pending,
    Running,
    Success,
    Failed(String),
}

pub struct PreflightUI {
    ollama_status: CheckStatus,
    spinner_state: usize,
    start_time: Instant,
}

impl PreflightUI {
    pub fn new() -> Self {
        Self {
            ollama_status: CheckStatus::Pending,
            spinner_state: 0,
            start_time: Instant::now(),
        }
    }

    pub async fn run_checks(mut self, mut terminal: DefaultTerminal) -> Result<Result<(), OllamaError>, color_eyre::Report> {
        let mut last_spinner_update = Instant::now();
        
        self.ollama_status = CheckStatus::Running;
        
        let ollama_check = tokio::spawn(async {
            ollama::check_available().await
        });

        loop {
            terminal.draw(|frame| self.draw(frame))?;

            if last_spinner_update.elapsed() >= Duration::from_millis(100) {
                self.spinner_state = (self.spinner_state + 1) % 4;
                last_spinner_update = Instant::now();
            }

            if poll(Duration::from_millis(50))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press && key.code == KeyCode::Char('q') {
                        std::process::exit(1);
                    }
                }
            }

            if ollama_check.is_finished() {
                match ollama_check.await.unwrap() {
                    Ok(_) => {
                        self.ollama_status = CheckStatus::Success;
                        terminal.draw(|frame| self.draw(frame))?;
                        sleep(Duration::from_millis(1000)).await;
                        return Ok(Ok(()));
                    },
                    Err(e) => {
                        self.ollama_status = CheckStatus::Failed(e.to_string());
                        terminal.draw(|frame| self.draw(frame))?;
                        sleep(Duration::from_millis(2000)).await;
                        return Ok(Err(e));
                    }
                }
            }

            sleep(Duration::from_millis(50)).await;
        }
    }

    fn draw(&self, frame: &mut Frame) {
        let chunks = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(5),
            Constraint::Length(3),
            Constraint::Min(0),
        ])
        .split(frame.area());

        let title = Paragraph::new("🦙 Ollama Code - System Check")
            .style(Style::default().fg(Color::Cyan).bold())
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::ALL));
        frame.render_widget(title, chunks[0]);

        let ollama_text = match &self.ollama_status {
            CheckStatus::Pending => {
                vec![
                    Span::styled("●", Style::default().fg(Color::Gray)),
                    Span::raw(" Ollama Service: "),
                    Span::styled("Waiting...", Style::default().fg(Color::Gray)),
                ]
            },
            CheckStatus::Running => {
                let spinner_chars = ['◐', '◓', '◑', '◒'];
                let spinner = spinner_chars[self.spinner_state];
                vec![
                    Span::styled(spinner.to_string(), Style::default().fg(Color::Yellow)),
                    Span::raw(" Ollama Service: "),
                    Span::styled("Checking availability...", Style::default().fg(Color::Yellow)),
                ]
            },
            CheckStatus::Success => {
                vec![
                    Span::styled("✓", Style::default().fg(Color::Green).bold()),
                    Span::raw(" Ollama Service: "),
                    Span::styled("Available", Style::default().fg(Color::Green).bold()),
                ]
            },
            CheckStatus::Failed(err) => {
                vec![
                    Span::styled("✗", Style::default().fg(Color::Red).bold()),
                    Span::raw(" Ollama Service: "),
                    Span::styled(format!("Failed - {}", err), Style::default().fg(Color::Red)),
                ]
            },
        };

        let check_paragraph = Paragraph::new(Line::from(ollama_text))
            .block(Block::default().borders(Borders::ALL).title("Preflight Checks"))
            .alignment(Alignment::Left);
        frame.render_widget(check_paragraph, chunks[1]);

        let progress = match &self.ollama_status {
            CheckStatus::Pending => 0,
            CheckStatus::Running => {
                let elapsed = self.start_time.elapsed().as_millis();
                ((elapsed % 1000) * 100 / 1000) as u16
            },
            CheckStatus::Success => 100,
            CheckStatus::Failed(_) => 100,
        };

        let gauge = Gauge::default()
            .block(Block::default().borders(Borders::ALL).title("Progress"))
            .gauge_style(match &self.ollama_status {
                CheckStatus::Success => Style::default().fg(Color::Green),
                CheckStatus::Failed(_) => Style::default().fg(Color::Red),
                _ => Style::default().fg(Color::Blue),
            })
            .ratio(progress as f64 / 100.0);
        frame.render_widget(gauge, chunks[2]);

        let help_text = match &self.ollama_status {
            CheckStatus::Failed(_) => "Press 'q' to quit or wait to continue...",
            _ => "Press 'q' to quit",
        };

        let help = Paragraph::new(help_text)
            .style(Style::default().fg(Color::Gray))
            .alignment(Alignment::Center);
        frame.render_widget(help, chunks[3]);
    }
}