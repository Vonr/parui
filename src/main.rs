use std::collections::HashMap;
use std::io;
use std::iter::{Enumerate, Skip};
use std::{borrow::Cow, os::unix::prelude::CommandExt};

use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use tui::style::{Color, Modifier, Style};
use tui::widgets::{BorderType, Wrap};
use tui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Layout, Rect},
    text::{Span, Spans},
    widgets::{Block, Borders, Clear, Paragraph},
    Terminal,
};

enum Mode {
    Insert,
    Select,
}

fn main() -> Result<(), io::Error> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    // let mut asi = termion::async_stdin();

    let mut query: Cow<String> = Cow::Owned(String::new());
    let mut results: Vec<String> = Vec::new();
    let mut mode = Mode::Insert;
    let mut selected = 0;
    let mut info: Vec<Spans> = Vec::new();

    let mut installed_cache: HashMap<usize, bool> = HashMap::new();

    terminal.clear()?;
    loop {
        let mut line = selected;
        let mut should_skip = false;
        terminal.draw(|s| {
            let size = s.size();
            if size.height <= 5 {
                should_skip = true;
                return;
            }

            let chunks = Layout::default()
                .constraints([Constraint::Min(3), Constraint::Percentage(100)].as_ref())
                .split(size);

            let para = Paragraph::new(Spans::from(vec![Span::raw(&*query)]))
                .block(
                    Block::default()
                        .title(Span::styled(
                            "parui",
                            Style::default().add_modifier(Modifier::BOLD),
                        ))
                        .title_alignment(Alignment::Center)
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded),
                )
                .alignment(Alignment::Left);
            s.render_widget(para, chunks[0]);

            let page = selected / (size.height - 5);
            let skipped = page * (size.height - 5);
            line -= skipped;
            let para = Paragraph::new(format_results(
                results.clone(),
                size.height as usize,
                results.len().to_string().len(),
                skipped as usize,
                &mut installed_cache,
            ))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded),
            )
            .alignment(Alignment::Left);
            s.render_widget(para, chunks[1]);

            if !info.is_empty() {
                let area = Rect {
                    x: size.width / 2,
                    y: 4,
                    width: size.width / 2,
                    height: size.height - 5,
                };
                let info = Paragraph::new(info.clone())
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .border_type(BorderType::Rounded),
                    )
                    .wrap(Wrap { trim: true })
                    .alignment(Alignment::Left);
                s.render_widget(Clear, area);
                s.render_widget(info, area);
            }
        })?;

        if should_skip {
            continue;
        }

        match mode {
            Mode::Insert => {
                terminal.set_cursor(query.len() as u16 + 1, 1)?;
            }
            Mode::Select => {
                terminal.set_cursor(1, line + 4)?;
            }
        }

        terminal.show_cursor()?;

        match event::read()? {
            Event::Key(k) => match mode {
                Mode::Insert => match k.code {
                    KeyCode::Esc => {
                        selected = 0;
                        mode = Mode::Select;
                    }
                    KeyCode::Backspace => {
                        if !query.is_empty() {
                            query.to_mut().pop();
                        }
                    }
                    KeyCode::Char(c) => match c {
                        'c' => {
                            if k.modifiers == KeyModifiers::CONTROL {
                                disable_raw_mode()?;
                                execute!(
                                    terminal.backend_mut(),
                                    LeaveAlternateScreen,
                                    DisableMouseCapture
                                )?;
                                terminal.clear()?;
                                terminal.set_cursor(0, 0)?;

                                return Ok(());
                            }
                            query.to_mut().push(c);
                        }
                        'w' => {
                            if k.modifiers == KeyModifiers::CONTROL {
                                let chars = query.clone();
                                let mut chars = chars.chars().rev();
                                while let Some(c) = chars.next() {
                                    if c == ' ' {
                                        break;
                                    }
                                }
                                let chars = chars.rev().collect::<String>();
                                query = Cow::Owned(chars);
                            }
                        }
                        _ => query.to_mut().push(c),
                    },
                    KeyCode::Enter => {
                        results.clear();
                        let packages = search(&query);

                        for line in packages.lines().map(|line| line.to_string()) {
                            results.push(line);
                        }
                        mode = Mode::Select;
                    }
                    _ => (),
                },
                Mode::Select => match k.code {
                    KeyCode::Up => {
                        if let Mode::Select = mode {
                            if selected > 0 {
                                selected -= 1;
                                info.clear();
                            } else {
                                selected = results.len() as u16 - 1;
                                info.clear();
                            }
                        }
                    }
                    KeyCode::Down => {
                        if let Mode::Select = mode {
                            let result_count = results.len();
                            if result_count > 1 && selected < result_count as u16 - 1 {
                                selected += 1;
                                info.clear();
                            } else {
                                selected = 0;
                                info.clear();
                            }
                        }
                    }
                    KeyCode::Left => {
                        let size = terminal.size().unwrap();
                        let per_page = size.height - 5;

                        if selected >= per_page && results.len() > per_page as usize {
                            selected -= per_page;
                            info.clear();
                        }
                    }
                    KeyCode::Right => {
                        let size = terminal.size().unwrap();
                        let per_page = size.height - 5;

                        if selected < results.len() as u16 - per_page
                            && results.len() > per_page as usize
                        {
                            selected += per_page;
                            info.clear();
                        }
                    }
                    KeyCode::Char(c) => match c {
                        'j' => {
                            if selected < results.len() as u16 - 1 {
                                selected += 1;
                                info.clear();
                            }
                        }
                        'k' => {
                            if selected > 0 {
                                selected -= 1;
                                info.clear();
                            }
                        }
                        'h' => {
                            let size = terminal.size().unwrap();
                            let per_page = size.height - 5;

                            if selected >= per_page && results.len() > per_page as usize {
                                selected -= per_page;
                                info.clear();
                            }
                        }
                        'l' => {
                            let size = terminal.size().unwrap();
                            let per_page = size.height - 5;

                            if selected < results.len() as u16 - per_page
                                && results.len() > per_page as usize
                            {
                                selected += per_page;
                                info.clear();
                            }
                        }
                        'q' => {
                            disable_raw_mode()?;
                            execute!(
                                terminal.backend_mut(),
                                LeaveAlternateScreen,
                                DisableMouseCapture
                            )?;
                            terminal.clear()?;
                            terminal.set_cursor(0, 0)?;

                            return Ok(());
                        }
                        'i' => {
                            mode = Mode::Insert;
                        }
                        'c' => {
                            if k.modifiers == KeyModifiers::CONTROL {
                                disable_raw_mode()?;
                                execute!(
                                    terminal.backend_mut(),
                                    LeaveAlternateScreen,
                                    DisableMouseCapture
                                )?;
                                terminal.clear()?;
                                terminal.set_cursor(0, 0)?;

                                return Ok(());
                            }
                            query.to_mut().push(c);
                        }
                        'g' => {
                            selected = 0;
                        }
                        'G' => {
                            selected = results.len() as u16 - 1;
                        }
                        'R' => {
                            disable_raw_mode()?;
                            execute!(
                                terminal.backend_mut(),
                                LeaveAlternateScreen,
                                DisableMouseCapture
                            )?;

                            terminal.clear()?;
                            terminal.set_cursor(0, 0)?;
                            terminal.show_cursor()?;
                            let mut cmd = std::process::Command::new("paru");
                            cmd.arg("-R").arg(results[selected as usize].clone());
                            cmd.exec();

                            return Ok(());
                        }

                        _ => (),
                    },
                    KeyCode::Enter => {
                        if info.is_empty() {
                            info = get_info(
                                results[selected as usize].clone(),
                                selected as usize,
                                &mut installed_cache,
                            );
                        } else {
                            disable_raw_mode()?;
                            execute!(
                                terminal.backend_mut(),
                                LeaveAlternateScreen,
                                DisableMouseCapture
                            )?;

                            terminal.clear()?;
                            terminal.set_cursor(0, 0)?;
                            terminal.show_cursor()?;
                            let mut cmd = std::process::Command::new("paru");
                            cmd.arg("-S").arg(results[selected as usize].clone());
                            cmd.exec();

                            return Ok(());
                        }
                    }
                    _ => (),
                },
            },
            Event::Mouse(m) => {
                if let Mode::Select = mode {
                    match m.kind {
                        MouseEventKind::ScrollDown => {
                            let result_count = results.len();
                            if result_count > 1 && selected < result_count as u16 - 1 {
                                selected += 1;
                                info.clear();
                            } else {
                                selected = 0;
                                info.clear();
                            }
                        }
                        MouseEventKind::ScrollUp => {
                            if selected > 0 {
                                selected -= 1;
                                info.clear();
                            } else {
                                selected = results.len() as u16 - 1;
                                info.clear();
                            }
                        }
                        _ => (),
                    }
                }
            }
            _ => (),
        }
    }
}

fn search(query: &str) -> String {
    let mut cmd = std::process::Command::new("paru");
    cmd.arg("--topdown").arg("-Ssq").arg(query);
    let output = cmd.output().unwrap();
    String::from_utf8(output.stdout).unwrap()
}

fn format_results(
    lines: Vec<String>,
    height: usize,
    pad_to: usize,
    skip: usize,
    cache: &mut HashMap<usize, bool>,
) -> Vec<Spans<'static>> {
    let index_style = Style::default().fg(Color::Gray);
    let installed_style = Style::default().fg(Color::Green);
    let uninstalled_style = Style::default().fg(Color::White);
    let lines = lines.into_iter().enumerate().skip(skip).take(height - 5);
    is_installed(lines.clone(), cache);

    lines
        .map(|(i, line)| {
            let index_string = (i + 1).to_string();
            Spans::from(vec![
                Span::styled(index_string.clone(), index_style),
                Span::raw(" ".repeat(pad_to - index_string.len() + 1)),
                Span::styled(
                    line.clone(),
                    if let Some(true) = cache.get(&i) {
                        installed_style
                    } else {
                        uninstalled_style
                    },
                ),
            ])
        })
        .collect()
}

fn get_info(query: String, index: usize, cache: &mut HashMap<usize, bool>) -> Vec<Spans<'static>> {
    let mut cmd = std::process::Command::new("paru");
    let mut info = Vec::new();
    let stdout = if let Some(true) = cache.get(&index) {
        cmd.arg("-Qi").arg(query);
        let output = cmd.output().unwrap();

        info.push(Spans::from(Span::styled(
            "Press ENTER again to reinstall this package",
            Style::default().fg(Color::Green),
        )));
        info.push(Spans::from(Span::styled(
            "Press R to uninstall this package".to_owned(),
            Style::default().fg(Color::Red),
        )));

        String::from_utf8(output.stdout).unwrap()
    } else {
        cmd.arg("-Si").arg(query);
        let output = cmd.output().unwrap();

        info.push(Spans::from(Span::styled(
            "Press ENTER again to install this package",
            Style::default().fg(Color::Green),
        )));

        String::from_utf8(output.stdout).unwrap()
    };

    info.push(Spans::from(Span::raw("")));

    for line in stdout.lines().map(|c| c.to_owned()) {
        info.push(Spans::from(Span::raw(line)));
    }

    info
}

fn is_installed(
    queries: std::iter::Take<Skip<Enumerate<std::vec::IntoIter<String>>>>,
    cache: &mut HashMap<usize, bool>,
) {
    let mut cmd = std::process::Command::new("paru");
    cmd.arg("-Qq");
    for q in queries.clone().map(|(_, q)| q) {
        cmd.arg(q);
    }

    let output = cmd.output().unwrap().stdout;
    let output = String::from_utf8(output.clone()).unwrap();
    for query in queries {
        if cache.contains_key(&query.0) {
            continue;
        }
        let is_installed = output.contains(&(query.1 + "\n"));
        cache.insert(query.0, is_installed);
    }
}
