use std::os::unix::prelude::CommandExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use std::{env, io};

use atomic::Atomic;
use config::Config;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use interface::{format_results, get_info, is_installed, list, search};
use mode::Mode;
use nohash_hasher::IntSet;
use parking_lot::{Mutex, RwLock};
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
    let mut query;
    let shown = Arc::new(RwLock::new(Vec::new()));
    let mode = Arc::new(Atomic::new(Mode::Insert));
    let mut current: usize = 0;
    let mut selected = IntSet::default();
    let mut info_scroll = 0;
    let info = Arc::new(Mutex::new(Vec::new()));
    let redraw = Arc::new(AtomicBool::new(true));
    let mut insert_pos;

    let all_packages = Arc::new(RwLock::new(Vec::new()));
    let installed = Arc::new(RwLock::new(IntSet::default()));
    let error_msg = Arc::new(Mutex::new("Try searching for something"));

    let mut _search_thread = None;

    {
        query = args.query.clone().unwrap_or_default();
        let query = query.clone();
        insert_pos = query.len() as u16;
        let mode = mode.clone();
        let shown = shown.clone();
        let error_msg = error_msg.clone();
        let redraw = redraw.clone();
        let command = command.clone();
        let all_packages = all_packages.clone();
        let installed = installed.clone();
        _search_thread = Some(tokio::spawn(async move {
            {
                *error_msg.lock() = "Searching for packages...";
                redraw.store(true, Ordering::SeqCst);

                if all_packages.read().len() == 0 {
                    std::mem::swap(&mut list().await, &mut all_packages.write());
                }

                if installed.read().len() == 0 {
                    is_installed(all_packages.clone(), installed.clone(), &command).await;
                }

                std::mem::swap(
                    &mut search(&query, &all_packages.read()),
                    &mut shown.write(),
                );
            }

            if !shown.read().is_empty() {
                mode.store(Mode::Select, Ordering::SeqCst);
            } else {
                *error_msg.lock() = "No or too many shown, try searching for something else";
            }
            redraw.store(true, Ordering::SeqCst);
        }));
    }

    {
        terminal.lock().clear()?;
    }

    loop {
        let mut line = current;
        let size = { terminal.lock().size() };
        let Ok(size) = size else {
            continue;
        };

        if size.height < 10 || size.width < 10 {
            continue;
        }

        let per_page = (size.height - 5) as usize;
        let page = current / per_page;
        let skipped = page * per_page;
        line -= skipped;

        if redraw.swap(false, Ordering::SeqCst) {
            let shown_len_str_len = ((shown.read().len() + 1).ilog10() + 1) as usize;

            let formatted_shown = {
                format_results(
                    all_packages.clone(),
                    shown.clone(),
                    current,
                    &selected,
                    size.height as usize,
                    shown_len_str_len,
                    skipped,
                    installed.clone(),
                )
                .await
            };

            let mut terminal = terminal.lock();
            if info.lock().is_empty() && !shown.read().is_empty() {
                let shown = shown.clone();
                let command = command.clone();
                let redraw = redraw.clone();
                let info = info.clone();
                let installed = installed.clone();
                let all_packages = all_packages.clone();
                if let Some(search_thread) = _search_thread {
                    search_thread.abort();
                }
                _search_thread = Some(tokio::spawn(async move {
                    let query = {
                        let all_packages = all_packages.read();
                        all_packages[shown.read()[current]].clone()
                    };

                    let real_index = shown.read()[current];
                    let newinfo = get_info(&query, real_index, installed, &command).await;
                    *info.lock() = newinfo;
                    redraw.store(true, Ordering::SeqCst);
                }))
            }

            terminal.draw(|s| {
                let search_color;
                let shown_color;
                let bold_search_style;
                if matches!(mode.load(Ordering::SeqCst), Mode::Insert) {
                    search_color = Color::White;
                    shown_color = Color::Gray;
                    bold_search_style = Style::default()
                        .add_modifier(Modifier::BOLD)
                        .fg(search_color)
                } else {
                    search_color = Color::Gray;
                    shown_color = Color::White;
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

                let para = Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(shown_color))
                    .border_type(BorderType::Rounded);
                let area = Rect {
                    x: 0,
                    y: 3,
                    width: size.width,
                    height: size.height - 3,
                };
                s.render_widget(para, area);

                let para = Paragraph::new(formatted_shown).alignment(Alignment::Left);
                let area = Rect {
                    x: 2,
                    y: 4,
                    width: size.width - 2,
                    height: size.height - 4,
                };
                s.render_widget(para, area);

                if shown.read().is_empty() {
                    let area = Rect {
                        x: size.width / 4 + 1,
                        y: size.height / 2 - 2,
                        width: size.width / 2,
                        height: 4,
                    };
                    let no_shown = Paragraph::new(*error_msg.lock())
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
                    s.render_widget(no_shown, area);
                } else {
                    let area = Rect {
                        x: size.width / 2,
                        y: 4,
                        width: size.width / 2 - 1,
                        height: size.height - 5,
                    };
                    let border = Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(shown_color))
                        .border_type(BorderType::Rounded);
                    s.render_widget(Clear, area);
                    s.render_widget(border, area);

                    let (info, no_info) = {
                        let info_lock = info.lock();
                        (info_lock.clone(), info_lock.is_empty())
                    };

                    let area = Rect {
                        x: size.width / 2 + 2,
                        y: 5,
                        width: size.width / 2 - 5,
                        height: 2 + no_info as u16 * 2,
                    };
                    let actions = Paragraph::new(if no_info {
                        vec![
                            Spans::from(Span::styled(
                                "Press ENTER to (re)install selected packages",
                                Style::default()
                                    .fg(Color::Green)
                                    .add_modifier(Modifier::BOLD),
                            )),
                            Spans::from(Span::styled(
                                "Press Shift-R to uninstall selected packages",
                                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                            )),
                            Spans::default(),
                            Spans::from(Span::styled(
                                "Finding info...",
                                Style::default().fg(Color::Gray),
                            )),
                        ]
                    } else {
                        vec![
                            Spans::from(Span::styled(
                                "Press ENTER to (re)install selected packages",
                                Style::default()
                                    .fg(Color::Green)
                                    .add_modifier(Modifier::BOLD),
                            )),
                            Spans::from(Span::styled(
                                "Press Shift-R to uninstall selected packages",
                                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                            )),
                        ]
                    })
                    .alignment(Alignment::Left);
                    s.render_widget(actions, area);

                    let area = Rect {
                        x: size.width / 2 + 2,
                        y: 8 - no_info as u16,
                        width: size.width / 2 - 5,
                        height: size.height - 10 - no_info as u16,
                    };

                    let info = Paragraph::new(info.clone())
                        .wrap(Wrap { trim: false })
                        .scroll((info_scroll as u16, 0));
                    s.render_widget(info, area);
                }
            })?;

            match mode.load(Ordering::SeqCst) {
                Mode::Insert => {
                    terminal.set_cursor(insert_pos + 10, 1)?;
                    terminal.show_cursor()?;
                }
                Mode::Select => {
                    terminal.set_cursor(2, line as u16 + 4)?;
                    terminal.hide_cursor()?;
                }
            }
        }

        if !event::poll(Duration::from_millis(50))? {
            continue;
        }

        let e = event::read()?;

        let Event::Key(k) = e else {
            if matches!(e, Event::Resize(..)) {
                redraw.store(true, Ordering::SeqCst);
            };
            continue
        };

        match mode.load(Ordering::SeqCst) {
            Mode::Insert => match k.code {
                KeyCode::Esc => {
                    if !shown.read().is_empty() {
                        current = 0;
                        redraw.store(true, Ordering::SeqCst);
                        mode.store(Mode::Select, Ordering::SeqCst);
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
                            let mut terminal = terminal.lock();
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
                            query = query[..boundary].to_string() + &query[insert_pos as usize..];
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
                    installed.write().clear();
                    info.lock().clear();
                    current = 0;
                    redraw.store(true, Ordering::SeqCst);
                    shown.write().clear();
                    let mode = mode.clone();
                    let shown = shown.clone();
                    let error_msg = error_msg.clone();
                    let redraw = redraw.clone();
                    let query = query.clone();
                    let command = command.clone();
                    let all_packages = all_packages.clone();
                    let installed = installed.clone();
                    if let Some(search_thread) = _search_thread {
                        search_thread.abort();
                    }
                    _search_thread = Some(tokio::spawn(async move {
                        {
                            *error_msg.lock() = "Searching for packages...";

                            if all_packages.read().len() == 0 {
                                std::mem::swap(&mut list().await, &mut all_packages.write());
                            }

                            is_installed(all_packages.clone(), installed.clone(), &command).await;

                            std::mem::swap(
                                &mut search(&query, &all_packages.read()),
                                &mut shown.write(),
                            );
                        }

                        if !shown.read().is_empty() {
                            mode.store(Mode::Select, Ordering::SeqCst);
                        } else {
                            *error_msg.lock() =
                                "No or too many shown, try searching for something else";
                        }
                        redraw.store(true, Ordering::SeqCst);
                    }));
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
                        } else {
                            current = shown.read().len() - 1;
                        }
                        info.lock().clear();
                        redraw.store(true, Ordering::SeqCst);
                    }
                }
                KeyCode::Down => {
                    if k.modifiers == KeyModifiers::CONTROL {
                        if !info.lock().is_empty() {
                            info_scroll += 1;
                            redraw.store(true, Ordering::SeqCst);
                        }
                    } else {
                        let result_count = shown.read().len();
                        if result_count > 1 && current < result_count - 1 {
                            current += 1;
                        } else {
                            current = 0;
                        }
                        info.lock().clear();
                        redraw.store(true, Ordering::SeqCst);
                    }
                }
                KeyCode::Left => {
                    let shown_count = shown.read().len();
                    if shown_count > per_page {
                        if current >= per_page {
                            current -= per_page;
                        } else {
                            current = shown_count - 1;
                        }
                        info.lock().clear();
                        redraw.store(true, Ordering::SeqCst);
                    }
                }
                KeyCode::Right => {
                    let shown = shown.read();
                    if shown.len() > per_page {
                        if current == shown.len() - 1 {
                            current = 0;
                        } else if current + per_page > shown.len() - 1 {
                            current = shown.len() - 1;
                        } else {
                            current += per_page;
                        }
                        info.lock().clear();
                        redraw.store(true, Ordering::SeqCst);
                    }
                }
                KeyCode::Esc => {
                    insert_pos = query.len() as u16;
                    redraw.store(true, Ordering::SeqCst);
                    mode.store(Mode::Insert, Ordering::SeqCst);
                }
                KeyCode::Char(c) => match c {
                    'j' => {
                        if k.modifiers == KeyModifiers::CONTROL {
                            if !info.lock().is_empty() {
                                info_scroll += 1;
                                redraw.store(true, Ordering::SeqCst);
                            }
                        } else {
                            let result_count = shown.read().len();
                            if result_count > 1 && current < result_count - 1 {
                                current += 1;
                            } else {
                                current = 0;
                            }
                            info.lock().clear();
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
                            } else {
                                current = shown.read().len() - 1;
                            }
                            info.lock().clear();
                            redraw.store(true, Ordering::SeqCst);
                        }
                    }
                    'h' => {
                        let shown_count = shown.read().len();
                        if shown_count > per_page {
                            if current >= per_page {
                                current -= per_page;
                            } else {
                                current = shown_count - 1;
                            }
                            info.lock().clear();
                            redraw.store(true, Ordering::SeqCst);
                        }
                    }
                    'l' => {
                        let shown = shown.read();
                        if shown.len() > per_page {
                            if current == shown.len() - 1 {
                                current = 0;
                            } else if current + per_page > shown.len() - 1 {
                                current = shown.len() - 1;
                            } else {
                                current += per_page;
                            }
                            info.lock().clear();
                            redraw.store(true, Ordering::SeqCst);
                        }
                    }
                    ' ' => {
                        let real_current = shown.read()[current];
                        if selected.contains(&real_current) {
                            selected.remove(&real_current);
                        } else {
                            selected.insert(real_current);
                        }
                        redraw.store(true, Ordering::SeqCst);
                    }
                    'q' => {
                        disable_raw_mode()?;
                        let mut terminal = terminal.lock();
                        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
                        terminal.clear()?;
                        terminal.set_cursor(0, 0)?;

                        return Ok(());
                    }
                    'i' | '/' => {
                        insert_pos = query.len() as u16;
                        redraw.store(true, Ordering::SeqCst);
                        mode.store(Mode::Insert, Ordering::SeqCst);
                    }
                    'c' => {
                        if k.modifiers == KeyModifiers::CONTROL {
                            disable_raw_mode()?;
                            let mut terminal = terminal.lock();
                            execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
                            terminal.clear()?;
                            terminal.set_cursor(0, 0)?;

                            return Ok(());
                        }
                        selected.clear();
                        redraw.store(true, Ordering::SeqCst);
                    }
                    'g' if current != 0 => {
                        info.lock().clear();
                        current = 0;
                        redraw.store(true, Ordering::SeqCst);
                    }
                    'G' if current != shown.read().len() - 1 => {
                        info.lock().clear();
                        current = shown.read().len() - 1;
                        redraw.store(true, Ordering::SeqCst);
                    }
                    'R' => {
                        if installed.read().contains(&current) {
                            let mut has_any = false;
                            let mut cmd = std::process::Command::new(&command);
                            cmd.arg("-R");
                            if selected.is_empty() {
                                cmd.arg(&(all_packages.read()[shown.read()[current]]));
                                has_any = true;
                            } else {
                                for i in selected.iter() {
                                    if installed.read().contains(i) {
                                        cmd.arg(&(all_packages.read()[*i]));
                                        has_any = true;
                                    }
                                }
                            }

                            if !has_any {
                                continue;
                            }

                            disable_raw_mode()?;
                            let mut terminal = terminal.lock();
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
                    let mut terminal = terminal.lock();
                    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

                    terminal.clear()?;
                    terminal.set_cursor(0, 0)?;
                    terminal.show_cursor()?;
                    let mut cmd = std::process::Command::new(command);
                    cmd.args(["--rebuild", "-S"]);
                    if selected.is_empty() {
                        cmd.arg(&(all_packages.read()[shown.read()[current]]));
                    } else {
                        for i in selected.iter() {
                            cmd.arg(&(all_packages.read()[*i]));
                        }
                    }
                    cmd.exec();

                    return Ok(());
                }
                _ => redraw.store(true, Ordering::SeqCst),
            },
        }
    }
}

fn last_word_end(bytes: &[u8], pos: u16) -> usize {
    bytes
        .iter()
        .take(pos.saturating_sub(1) as usize)
        .rposition(|c| matches!(*c, b' ' | b'-' | b'_'))
        .map(|i| i + 1)
        .unwrap_or_default()
}

fn next_word_start(bytes: &[u8], pos: u16) -> usize {
    bytes
        .iter()
        .skip(pos as usize)
        .position(|c| matches!(*c, b' ' | b'-' | b'_'))
        .map(|i| i + pos as usize + 1)
        .unwrap_or(bytes.len())
}
