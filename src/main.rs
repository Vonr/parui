use std::collections::HashSet;
use std::os::unix::prelude::CommandExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::{env, io};

use config::Config;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use interface::{format_results, get_info, search};
use mode::Mode;
use tui::style::{Color, Modifier, Style};
use tui::widgets::{BorderType, Wrap};
use tui::{
    backend::CrosstermBackend,
    layout::{Alignment, Rect},
    text::{Span, Spans},
    widgets::{Block, Borders, Clear, Paragraph},
    Terminal,
};

mod config;
mod interface;
mod mode;

#[tokio::main]
async fn main() -> Result<(), io::Error> {
    let args = Config::new(env::args());
    let command = args.command;

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let terminal = Arc::new(Mutex::new(Terminal::new(backend)?));
    let mut query = String::new();
    let results = Arc::new(Mutex::new(Vec::new()));
    let mode = Arc::new(Mutex::new(Mode::Insert));
    let mut current: usize = 0;
    let mut selected = HashSet::new();
    let mut info_scroll = 0;
    let info = Arc::new(Mutex::new(Vec::new()));
    let redraw = Arc::new(AtomicBool::new(true));
    let mut insert_pos = 0;

    let installed_cache = Arc::new(Mutex::new(HashSet::new()));
    let mut cached_pages = HashSet::new();
    let error_msg = Arc::new(Mutex::new("Try searching for something"));

    let mut _search_thread = None;

    if let Some(argquery) = args.query {
        query = argquery.clone();
        insert_pos = query.len() as u16;
        let mode = mode.clone();
        let results = results.clone();
        let command = command.clone();
        let error_msg = error_msg.clone();
        let redraw = redraw.clone();
        _search_thread = Some(tokio::spawn(async move {
            {
                *error_msg.lock().unwrap() = "Searching for packages...";
                redraw.store(true, Ordering::SeqCst);
            }
            let packages = search(&argquery, &command).await;

            for line in packages.lines() {
                results.lock().unwrap().push(line.to_owned());
            }

            if !packages.is_empty() {
                *mode.lock().unwrap() = Mode::Select;
            } else {
                *error_msg.lock().unwrap() =
                    "No or too many results, try searching for something else";
            }
            redraw.store(true, Ordering::SeqCst);
        }));
    }

    {
        terminal.lock().unwrap().clear()?;
    }

    loop {
        let mut line = current;
        let size;
        {
            size = terminal.lock().unwrap().size();
        }
        if let Ok(size) = size {
            if size.height < 10 || size.width < 10 {
                continue;
            }

            let per_page = (size.height - 5) as usize;
            let page = current / per_page;
            let skipped = page * per_page;
            line -= skipped;

            if redraw.load(Ordering::SeqCst) {
                redraw.store(false, Ordering::SeqCst);
                let mut terminal = terminal.lock().unwrap();

                let formatted_results = {
                    let results = results.lock().unwrap();
                    format_results(
                        &results,
                        current,
                        &selected,
                        size.height as usize,
                        (results.len() as f32 + 1f32).log10().ceil() as usize,
                        skipped,
                        &mut installed_cache.lock().unwrap(),
                        &mut cached_pages,
                        &command,
                    )
                    .await
                };

                if info.lock().unwrap().is_empty() && !results.lock().unwrap().is_empty() {
                    let results = results.clone();
                    let command = command.clone();
                    let redraw = redraw.clone();
                    let info = info.clone();
                    let installed_cache = installed_cache.clone();
                    if let Some(search_thread) = _search_thread {
                        search_thread.abort();
                    }
                    _search_thread = Some(tokio::spawn(async move {
                        let query = results.lock().unwrap()[current].clone();
                        let query = &query;

                        let newinfo = get_info(query, current, installed_cache, &command).await;
                        *info.lock().unwrap() = newinfo;
                        redraw.store(true, Ordering::SeqCst);
                    }))
                }

                terminal.draw(|s| {
                    let search_color;
                    let results_color;
                    let bold_search_style;
                    if let Mode::Insert = *mode.lock().unwrap() {
                        search_color = Color::White;
                        results_color = Color::Gray;
                        bold_search_style = Style::default()
                            .add_modifier(Modifier::BOLD)
                            .fg(search_color)
                    } else {
                        search_color = Color::Gray;
                        results_color = Color::White;
                        bold_search_style = Style::default().fg(search_color);
                    };
                    let para = Paragraph::new(Spans::from(vec![
                        Span::styled(" Search: ", bold_search_style),
                        Span::styled(&*query, Style::default().fg(search_color)),
                    ]))
                    .block(
                        Block::default()
                            .title(Span::styled(" parui ", bold_search_style))
                            .title_alignment(Alignment::Center)
                            .border_style(Style::default().fg(search_color))
                            .borders(Borders::ALL)
                            .border_type(BorderType::Rounded),
                    )
                    .alignment(Alignment::Left);
                    let area = Rect {
                        x: 0,
                        y: 0,
                        width: size.width,
                        height: 3,
                    };
                    s.render_widget(para, area);

                    let para = Paragraph::new(formatted_results)
                        .block(
                            Block::default()
                                .borders(Borders::ALL)
                                .border_style(Style::default().fg(results_color))
                                .border_type(BorderType::Rounded),
                        )
                        .alignment(Alignment::Left);
                    let area = Rect {
                        x: 0,
                        y: 3,
                        width: size.width,
                        height: size.height - 3,
                    };
                    s.render_widget(para, area);

                    if results.lock().unwrap().is_empty() {
                        let area = Rect {
                            x: size.width / 4 + 1,
                            y: size.height / 2 - 2,
                            width: size.width / 2,
                            height: 4,
                        };
                        let no_results = Paragraph::new(*error_msg.lock().unwrap())
                            .block(
                                Block::default()
                                    .title(Span::styled(
                                        " No Results ",
                                        Style::default().add_modifier(Modifier::BOLD),
                                    ))
                                    .title_alignment(Alignment::Center)
                                    .borders(Borders::ALL)
                                    .border_type(BorderType::Rounded),
                            )
                            .wrap(Wrap { trim: true })
                            .alignment(Alignment::Center);
                        s.render_widget(Clear, area);
                        s.render_widget(no_results, area);
                    } else {
                        let area = Rect {
                            x: size.width / 2,
                            y: 4,
                            width: size.width / 2 - 1,
                            height: size.height - 5,
                        };
                        let border = Block::default()
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(results_color))
                            .border_type(BorderType::Rounded);
                        s.render_widget(Clear, area);
                        s.render_widget(border, area);

                        let info = info.lock().unwrap();
                        let no_info = info.is_empty();
                        let area = Rect {
                            x: size.width / 2 + 2,
                            y: 5,
                            width: size.width / 2 - 5,
                            height: 2 + no_info as u16 * 2,
                        };
                        let actions = Paragraph::new({
                            let mut actions = vec![
                                Spans::from(Span::styled(
                                    "Press ENTER to (re)install selected packages",
                                    Style::default()
                                        .fg(Color::Green)
                                        .add_modifier(Modifier::BOLD),
                                )),
                                Spans::from(Span::styled(
                                    "Press Shift-R to uninstall selected packages".to_owned(),
                                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                                )),
                            ];
                            if no_info {
                                actions.push(Spans::default());
                                actions.push(Spans::from(Span::styled(
                                    "Finding info...".to_owned(),
                                    Style::default().fg(Color::Gray),
                                )));
                            }
                            actions
                        })
                        .alignment(Alignment::Left);
                        s.render_widget(actions, area);

                        let area = Rect {
                            x: size.width / 2 + 2,
                            y: 8 - no_info as u16,
                            width: size.width / 2 - 5,
                            height: size.height - 10 - no_info as u16,
                        };

                        let info = Paragraph::new(info.to_vec())
                            .wrap(Wrap { trim: false })
                            .scroll((info_scroll as u16, 0));
                        s.render_widget(info, area);
                    }
                })?;
            }

            let modemutex = mode.clone();
            let mut mode = mode.lock().unwrap();
            match *mode {
                Mode::Insert => {
                    let mut terminal = terminal.lock().unwrap();
                    terminal.set_cursor(insert_pos + 10, 1)?;
                    terminal.show_cursor()?;
                }
                Mode::Select => {
                    let mut terminal = terminal.lock().unwrap();
                    terminal.set_cursor(2, line as u16 + 4)?;
                    terminal.hide_cursor()?;
                }
            }

            if event::poll(Duration::from_millis(50))? {
                match event::read()? {
                    Event::Key(k) => match *mode {
                        Mode::Insert => match k.code {
                            KeyCode::Esc => {
                                if !results.lock().unwrap().is_empty() {
                                    current = 0;
                                    redraw.store(true, Ordering::SeqCst);
                                    *mode = Mode::Select;
                                }
                            }
                            KeyCode::Left => {
                                if k.modifiers.contains(KeyModifiers::CONTROL) {
                                    let boundary = last_word_end(query.as_bytes(), insert_pos);
                                    insert_pos = boundary as u16;
                                } else if insert_pos > 0 {
                                    insert_pos -= 1;
                                } else {
                                    insert_pos = query.len() as u16;
                                }
                                redraw.store(true, Ordering::SeqCst);
                            }
                            KeyCode::Right => {
                                if k.modifiers.contains(KeyModifiers::CONTROL) {
                                    let boundary = next_word_start(query.as_bytes(), insert_pos);
                                    insert_pos = boundary as u16;
                                } else if (insert_pos as usize) < query.len() {
                                    insert_pos += 1;
                                } else {
                                    insert_pos = 0;
                                }
                                redraw.store(true, Ordering::SeqCst);
                            }
                            KeyCode::Backspace => {
                                if insert_pos != 0 {
                                    query.remove(insert_pos as usize - 1);
                                    insert_pos -= 1;
                                    redraw.store(true, Ordering::SeqCst);
                                }
                            }
                            KeyCode::Char(c) => match c {
                                'c' => {
                                    if k.modifiers == KeyModifiers::CONTROL {
                                        disable_raw_mode()?;
                                        let mut terminal = terminal.lock().unwrap();
                                        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
                                        terminal.clear()?;
                                        terminal.set_cursor(0, 0)?;

                                        return Ok(());
                                    }
                                    query.insert(insert_pos as usize, c);
                                    insert_pos += 1;
                                    redraw.store(true, Ordering::SeqCst);
                                }
                                'w' => {
                                    if k.modifiers == KeyModifiers::CONTROL {
                                        let boundary = last_word_end(query.as_bytes(), insert_pos);
                                        query = query[..boundary].to_string()
                                            + &query[insert_pos as usize..];
                                        insert_pos = boundary as u16;
                                    } else {
                                        query.insert(insert_pos as usize, c);
                                        insert_pos += 1;
                                    }
                                    redraw.store(true, Ordering::SeqCst);
                                }
                                _ => {
                                    query.insert(insert_pos as usize, c);
                                    insert_pos += 1;
                                    redraw.store(true, Ordering::SeqCst);
                                }
                            },
                            KeyCode::Enter => {
                                installed_cache.lock().unwrap().clear();
                                cached_pages.clear();
                                info.lock().unwrap().clear();
                                current = 0;
                                redraw.store(true, Ordering::SeqCst);
                                results.lock().unwrap().clear();
                                if query.as_bytes().len() > 1 {
                                    let mode = modemutex.clone();
                                    let results = results.clone();
                                    let command = command.clone();
                                    let error_msg = error_msg.clone();
                                    let redraw = redraw.clone();
                                    let query = query.clone();
                                    if let Some(search_thread) = _search_thread {
                                        search_thread.abort();
                                    }
                                    _search_thread = Some(tokio::spawn(async move {
                                        {
                                            *error_msg.lock().unwrap() =
                                                "Searching for packages...";
                                        }
                                        let packages = search(&query, &command).await;

                                        let mut results = results.lock().unwrap();
                                        packages.lines().for_each(|s| {
                                            results.push(s.to_owned());
                                        });

                                        if !results.is_empty() {
                                            *mode.lock().unwrap() = Mode::Select;
                                        } else {
                                            *error_msg.lock().unwrap() =
                                        "No or too many results, try searching for something else";
                                        }
                                        redraw.store(true, Ordering::SeqCst);
                                    }));
                                } else {
                                    let mut results = results.lock().unwrap();
                                    results.clear();
                                    *error_msg.lock().unwrap() =
                                        "Query should be at least 2 characters long";
                                }
                            }
                            _ => redraw.store(true, Ordering::SeqCst),
                        },
                        Mode::Select => match k.code {
                            KeyCode::Up => {
                                if k.modifiers == KeyModifiers::CONTROL {
                                    if info_scroll > 0 {
                                        info_scroll -= 1;
                                        redraw.store(true, Ordering::SeqCst);
                                    }
                                } else {
                                    if current > 0 {
                                        current -= 1;
                                        info.lock().unwrap().clear();
                                    } else {
                                        current = results.lock().unwrap().len() - 1;
                                        info.lock().unwrap().clear();
                                    }
                                    redraw.store(true, Ordering::SeqCst);
                                }
                            }
                            KeyCode::Down => {
                                if k.modifiers == KeyModifiers::CONTROL {
                                    if !info.lock().unwrap().is_empty() {
                                        info_scroll += 1;
                                        redraw.store(true, Ordering::SeqCst);
                                    }
                                } else {
                                    let result_count = results.lock().unwrap().len();
                                    if result_count > 1 && current < result_count - 1 {
                                        current += 1;
                                        info.lock().unwrap().clear();
                                    } else {
                                        current = 0;
                                        info.lock().unwrap().clear();
                                    }
                                    redraw.store(true, Ordering::SeqCst);
                                }
                            }
                            KeyCode::Left => {
                                let results_count = results.lock().unwrap().len();
                                if results_count > per_page {
                                    if current >= per_page {
                                        current -= per_page;
                                    } else {
                                        current = results_count - 1;
                                    }
                                    info.lock().unwrap().clear();
                                    redraw.store(true, Ordering::SeqCst);
                                }
                            }
                            KeyCode::Right => {
                                let results = results.lock().unwrap();
                                if results.len() > per_page {
                                    if current == results.len() - 1 {
                                        current = 0;
                                    } else if current + per_page > results.len() - 1 {
                                        current = results.len() - 1;
                                    } else {
                                        current += per_page;
                                    }
                                    info.lock().unwrap().clear();
                                    redraw.store(true, Ordering::SeqCst);
                                }
                            }
                            KeyCode::Esc => {
                                insert_pos = query.len() as u16;
                                redraw.store(true, Ordering::SeqCst);
                                *mode = Mode::Insert;
                            }
                            KeyCode::Char(c) => match c {
                                'j' => {
                                    if k.modifiers == KeyModifiers::CONTROL {
                                        if !info.lock().unwrap().is_empty() {
                                            info_scroll += 1;
                                            redraw.store(true, Ordering::SeqCst);
                                        }
                                    } else {
                                        let result_count = results.lock().unwrap().len();
                                        if result_count > 1 && current < result_count - 1 {
                                            current += 1;
                                            info.lock().unwrap().clear();
                                        } else {
                                            current = 0;
                                            info.lock().unwrap().clear();
                                        }
                                        redraw.store(true, Ordering::SeqCst);
                                    }
                                }
                                'k' => {
                                    if k.modifiers == KeyModifiers::CONTROL {
                                        if info_scroll > 0 {
                                            info_scroll -= 1;
                                            redraw.store(true, Ordering::SeqCst);
                                        }
                                    } else {
                                        if current > 0 {
                                            current -= 1;
                                            info.lock().unwrap().clear();
                                        } else {
                                            current = results.lock().unwrap().len() - 1;
                                            info.lock().unwrap().clear();
                                        }
                                        redraw.store(true, Ordering::SeqCst);
                                    }
                                }
                                'h' => {
                                    let results_count = results.lock().unwrap().len();
                                    if results_count > per_page {
                                        if current >= per_page {
                                            current -= per_page;
                                        } else {
                                            current = results_count - 1;
                                        }
                                        info.lock().unwrap().clear();
                                        redraw.store(true, Ordering::SeqCst);
                                    }
                                }
                                'l' => {
                                    let results = results.lock().unwrap();
                                    if results.len() > per_page {
                                        if current == results.len() - 1 {
                                            current = 0;
                                        } else if current + per_page > results.len() - 1 {
                                            current = results.len() - 1;
                                        } else {
                                            current += per_page;
                                        }
                                        info.lock().unwrap().clear();
                                        redraw.store(true, Ordering::SeqCst);
                                    }
                                }
                                ' ' => {
                                    if selected.contains(&current) {
                                        selected.remove(&current);
                                    } else {
                                        selected.insert(current);
                                    }
                                    redraw.store(true, Ordering::SeqCst);
                                }
                                'q' => {
                                    disable_raw_mode()?;
                                    let mut terminal = terminal.lock().unwrap();
                                    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
                                    terminal.clear()?;
                                    terminal.set_cursor(0, 0)?;

                                    return Ok(());
                                }
                                'i' => {
                                    insert_pos = query.len() as u16;
                                    redraw.store(true, Ordering::SeqCst);
                                    *mode = Mode::Insert;
                                }
                                'c' => {
                                    if k.modifiers == KeyModifiers::CONTROL {
                                        disable_raw_mode()?;
                                        let mut terminal = terminal.lock().unwrap();
                                        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
                                        terminal.clear()?;
                                        terminal.set_cursor(0, 0)?;

                                        return Ok(());
                                    }
                                    selected.clear();
                                    redraw.store(true, Ordering::SeqCst);
                                }
                                'g' => {
                                    info.lock().unwrap().clear();
                                    current = 0;
                                    redraw.store(true, Ordering::SeqCst);
                                }
                                'G' => {
                                    info.lock().unwrap().clear();
                                    current = results.lock().unwrap().len() - 1;
                                    redraw.store(true, Ordering::SeqCst);
                                }
                                'R' => {
                                    if installed_cache.lock().unwrap().contains(&current) {
                                        let mut has_any = false;
                                        let mut cmd = std::process::Command::new(&command);
                                        cmd.arg("-R");
                                        if selected.is_empty() {
                                            cmd.arg(&(results.lock().unwrap()[current]));
                                            has_any = true;
                                        } else {
                                            for i in selected.iter().filter(|i| {
                                                installed_cache.lock().unwrap().contains(i)
                                            }) {
                                                cmd.arg(&(results.lock().unwrap()[*i]));
                                                has_any = true;
                                            }
                                        }

                                        if !has_any {
                                            continue;
                                        }

                                        disable_raw_mode()?;
                                        let mut terminal = terminal.lock().unwrap();
                                        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

                                        terminal.clear()?;
                                        terminal.set_cursor(0, 0)?;
                                        terminal.show_cursor()?;
                                        cmd.exec();

                                        return Ok(());
                                    }
                                }

                                _ => redraw.store(true, Ordering::SeqCst),
                            },
                            KeyCode::Enter => {
                                disable_raw_mode()?;
                                let mut terminal = terminal.lock().unwrap();
                                execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

                                terminal.clear()?;
                                terminal.set_cursor(0, 0)?;
                                terminal.show_cursor()?;
                                let mut cmd = std::process::Command::new(command);
                                cmd.args(["--rebuild", "-S"]);
                                if selected.is_empty() {
                                    cmd.arg(&(results.lock().unwrap()[current]));
                                } else {
                                    for i in selected.iter() {
                                        cmd.arg(&(results.lock().unwrap()[*i]));
                                    }
                                }
                                cmd.exec();

                                return Ok(());
                            }
                            _ => redraw.store(true, Ordering::SeqCst),
                        },
                    },
                    _ => redraw.store(true, Ordering::SeqCst),
                }
            }
        }
    }
}

fn last_word_end(bytes: &[u8], pos: u16) -> usize {
    let mut boundary = 0;
    for (i, c) in bytes.iter().take(pos as usize).enumerate() {
        match *c as char {
            ' ' | '-' | '_' => {
                boundary = i;
            }
            _ => (),
        }
    }
    boundary
}

fn next_word_start(bytes: &[u8], pos: u16) -> usize {
    let mut boundary = pos as usize;
    for (i, c) in bytes.iter().skip(boundary).enumerate() {
        match *c as char {
            ' ' | '-' | '_' => {
                boundary = i + 1;
                break;
            }
            _ => (),
        }
    }
    if boundary == pos as usize {
        bytes.len()
    } else {
        boundary
    }
}
