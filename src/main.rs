use std::collections::HashSet;
use std::env::Args;
use std::os::unix::prelude::CommandExt;
use std::process::{exit, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::{env, io};

use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use naive_opt::Search;
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

struct Config {
    query: Option<String>,
    command: String,
}

fn main() -> Result<(), io::Error> {
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
    let mut selected: usize = 0;
    let mut info_scroll = 0;
    let mut info = Vec::new();
    let redraw = Arc::new(AtomicBool::new(true));
    let mut insert_pos = 0;

    let mut installed_cache = HashSet::new();
    let mut cached_pages = HashSet::new();
    let error_msg = Arc::new(Mutex::new("Try searching for something"));

    let mut search_thread = None;

    if let Some(argquery) = args.query {
        query = argquery.clone();
        insert_pos = query.len() as u16;
        let terminal = terminal.clone();
        let mode = mode.clone();
        let results = results.clone();
        let command = command.clone();
        let error_msg = error_msg.clone();
        let redraw = redraw.clone();
        search_thread = Some(thread::spawn(move || {
            {
                *mode.lock().unwrap() = Mode::Select;
            }
            terminal.lock().unwrap().set_cursor(2, 4).unwrap();
            let packages = search(&argquery, &command);

            for line in packages.lines() {
                results.lock().unwrap().push(line.to_owned());
            }

            if packages.is_empty() {
                *mode.lock().unwrap() = Mode::Insert;
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
        let mut line = selected;
        let size;
        {
            size = terminal.lock().unwrap().size();
        }
        if let Ok(size) = size {
            if size.height < 10 || size.width < 10 {
                continue;
            }

            let per_page = (size.height - 5) as usize;
            let page = selected / per_page;
            let skipped = page * per_page;
            line -= skipped;

            if redraw.load(Ordering::SeqCst) {
                redraw.store(false, Ordering::SeqCst);
                let mut terminal = terminal.lock().unwrap();
                terminal.draw(|s| {
                    let chunks = Layout::default()
                        .constraints([Constraint::Min(3), Constraint::Percentage(100)].as_ref())
                        .split(size);

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
                    s.render_widget(para, chunks[0]);

                    let results = results.lock().unwrap();
                    let para = Paragraph::new(format_results(
                        &results,
                        selected,
                        size.height as usize,
                        (results.len() as f32 + 1f32).log10().ceil() as usize,
                        skipped,
                        &mut installed_cache,
                        &mut cached_pages,
                        &command,
                    ))
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(results_color))
                            .border_type(BorderType::Rounded),
                    )
                    .alignment(Alignment::Left);
                    s.render_widget(para, chunks[1]);

                    if results.is_empty() {
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

                        let no_info = info.is_empty();
                        let is_installed = installed_cache.contains(&selected);
                        let area = Rect {
                            x: size.width / 2 + 2,
                            y: 5,
                            width: size.width / 2 - 5,
                            height: 1 + (no_info || is_installed) as u16,
                        };
                        let actions = Paragraph::new({
                            let mut actions = Vec::new();
                            if no_info {
                                actions.push(Spans::from(Span::styled(
                                    "Press ENTER to show package information".to_owned(),
                                    Style::default()
                                        .fg(Color::Green)
                                        .add_modifier(Modifier::BOLD),
                                )));
                                if installed_cache.contains(&selected) {
                                    actions.push(Spans::from(Span::styled(
                                        "Press Shift-R to uninstall this package".to_owned(),
                                        Style::default()
                                            .fg(Color::Red)
                                            .add_modifier(Modifier::BOLD),
                                    )));
                                }
                            } else if is_installed {
                                actions.push(Spans::from(Span::styled(
                                    "Press ENTER again to reinstall this package",
                                    Style::default()
                                        .fg(Color::Green)
                                        .add_modifier(Modifier::BOLD),
                                )));
                                actions.push(Spans::from(Span::styled(
                                    "Press Shift-R to uninstall this package".to_owned(),
                                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                                )));
                            } else {
                                actions.push(Spans::from(Span::styled(
                                    "Press ENTER again to install this package",
                                    Style::default()
                                        .fg(Color::Green)
                                        .add_modifier(Modifier::BOLD),
                                )));
                            }
                            actions
                        })
                        .alignment(Alignment::Left);
                        s.render_widget(actions, area);

                        let area = Rect {
                            x: size.width / 2 + 2,
                            y: 7 + (no_info || is_installed) as u16,
                            width: size.width / 2 - 5,
                            height: size.height - 9 - (no_info || is_installed) as u16,
                        };

                        let info = Paragraph::new(info.to_vec())
                            .wrap(Wrap { trim: false })
                            .scroll((info_scroll as u16, 0));
                        s.render_widget(info, area);
                    }
                })?;
            }

            match *mode.lock().unwrap() {
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

            let modemutex = mode.clone();
            let mut mode = mode.lock().unwrap();
            match event::read()? {
                Event::Key(k) => match *mode {
                    Mode::Insert => match k.code {
                        KeyCode::Esc => {
                            if !results.lock().unwrap().is_empty() {
                                selected = 0;
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
                                    if let Some(search_thread) = search_thread {
                                        search_thread.join().unwrap();
                                    }
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
                            installed_cache.clear();
                            cached_pages.clear();
                            info.clear();
                            selected = 0;
                            if query.as_bytes().len() > 1 {
                                let terminal = terminal.clone();
                                let mode = modemutex.clone();
                                let results = results.clone();
                                let command = command.clone();
                                let error_msg = error_msg.clone();
                                let redraw = redraw.clone();
                                let query = query.clone();
                                search_thread = Some(thread::spawn(move || {
                                    terminal.lock().unwrap().set_cursor(2, 4).unwrap();
                                    let packages = search(&query, &command);

                                    let mut results = results.lock().unwrap();
                                    results.clear();
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
                                if selected > 0 {
                                    selected -= 1;
                                    info.clear();
                                } else {
                                    selected = results.lock().unwrap().len() - 1;
                                    info.clear();
                                }
                                redraw.store(true, Ordering::SeqCst);
                            }
                        }
                        KeyCode::Down => {
                            if k.modifiers == KeyModifiers::CONTROL {
                                if !info.is_empty() {
                                    info_scroll += 1;
                                    redraw.store(true, Ordering::SeqCst);
                                }
                            } else {
                                let result_count = results.lock().unwrap().len();
                                if result_count > 1 && selected < result_count - 1 {
                                    selected += 1;
                                    info.clear();
                                } else {
                                    selected = 0;
                                    info.clear();
                                }
                                redraw.store(true, Ordering::SeqCst);
                            }
                        }
                        KeyCode::Left => {
                            let results_count = results.lock().unwrap().len();
                            if results_count > per_page {
                                if selected >= per_page {
                                    selected -= per_page;
                                } else {
                                    selected = results_count - 1;
                                }
                                info.clear();
                                redraw.store(true, Ordering::SeqCst);
                            }
                        }
                        KeyCode::Right => {
                            let results = results.lock().unwrap();
                            if results.len() > per_page {
                                if selected == results.len() - 1 {
                                    selected = 0;
                                } else if selected + per_page > results.len() - 1 {
                                    selected = results.len() - 1;
                                } else {
                                    selected += per_page;
                                }
                                info.clear();
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
                                    if !info.is_empty() {
                                        info_scroll += 1;
                                        redraw.store(true, Ordering::SeqCst);
                                    }
                                } else {
                                    let result_count = results.lock().unwrap().len();
                                    if result_count > 1 && selected < result_count - 1 {
                                        selected += 1;
                                        info.clear();
                                    } else {
                                        selected = 0;
                                        info.clear();
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
                                    if selected > 0 {
                                        selected -= 1;
                                        info.clear();
                                    } else {
                                        selected = results.lock().unwrap().len() - 1;
                                        info.clear();
                                    }
                                    redraw.store(true, Ordering::SeqCst);
                                }
                            }
                            'h' => {
                                let results_count = results.lock().unwrap().len();
                                if results_count > per_page {
                                    if selected >= per_page {
                                        selected -= per_page;
                                    } else {
                                        selected = results_count - 1;
                                    }
                                    info.clear();
                                    redraw.store(true, Ordering::SeqCst);
                                }
                            }
                            'l' => {
                                let results = results.lock().unwrap();
                                if results.len() > per_page {
                                    if selected == results.len() - 1 {
                                        selected = 0;
                                    } else if selected + per_page > results.len() - 1 {
                                        selected = results.len() - 1;
                                    } else {
                                        selected += per_page;
                                    }
                                    info.clear();
                                    redraw.store(true, Ordering::SeqCst);
                                }
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
                                query.push(c);
                                redraw.store(true, Ordering::SeqCst);
                            }
                            'g' => {
                                selected = 0;
                                redraw.store(true, Ordering::SeqCst);
                            }
                            'G' => {
                                selected = results.lock().unwrap().len() - 1;
                                redraw.store(true, Ordering::SeqCst);
                            }
                            'R' => {
                                if installed_cache.contains(&selected) {
                                    disable_raw_mode()?;
                                    let mut terminal = terminal.lock().unwrap();
                                    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

                                    terminal.clear()?;
                                    terminal.set_cursor(0, 0)?;
                                    terminal.show_cursor()?;
                                    let mut cmd = Command::new(command);
                                    cmd.arg("-R").arg(&(results.lock().unwrap()[selected]));
                                    cmd.exec();

                                    return Ok(());
                                }
                            }

                            _ => redraw.store(true, Ordering::SeqCst),
                        },
                        KeyCode::Enter => {
                            if info.is_empty() {
                                info = get_info(
                                    &(results.lock().unwrap()[selected]),
                                    selected,
                                    &installed_cache,
                                    &command,
                                );
                                redraw.store(true, Ordering::SeqCst);
                            } else {
                                disable_raw_mode()?;
                                let mut terminal = terminal.lock().unwrap();
                                execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

                                terminal.clear()?;
                                terminal.set_cursor(0, 0)?;
                                terminal.show_cursor()?;
                                let mut cmd = Command::new(command);
                                cmd.args(["--rebuild", "-S", &(results.lock().unwrap()[selected])]);
                                cmd.exec();

                                return Ok(());
                            }
                        }
                        _ => redraw.store(true, Ordering::SeqCst),
                    },
                },
                _ => redraw.store(true, Ordering::SeqCst),
            }
        }
    }
}

fn search(query: &str, command: &str) -> String {
    let mut cmd = Command::new(command);
    cmd.args(["--topdown", "-Ssq", query]);
    cmd_output(cmd)
}

#[allow(clippy::too_many_arguments)]
fn format_results(
    lines: &[String],
    selected: usize,
    height: usize,
    pad_to: usize,
    skip: usize,
    installed_cache: &mut HashSet<usize>,
    cached_pages: &mut HashSet<usize>,
    command: &str,
) -> Vec<Spans<'static>> {
    let index_style = Style::default().fg(Color::Gray);
    let installed_style = Style::default()
        .fg(Color::Green)
        .add_modifier(Modifier::BOLD);
    let installed_selected_style = Style::default()
        .bg(Color::Red)
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let uninstalled_style = Style::default()
        .fg(Color::LightBlue)
        .add_modifier(Modifier::BOLD);
    let uninstalled_selected_style = Style::default()
        .bg(Color::Red)
        .fg(Color::Blue)
        .add_modifier(Modifier::BOLD);

    let lines: Vec<String> = lines.iter().skip(skip).take(height - 5).cloned().collect();
    if lines.is_empty() {
        return vec![Spans::default()];
    }

    is_installed(&lines, skip, installed_cache, cached_pages, command);

    lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let index = i + skip + 1;
            let index_string = " ".to_string() + &index.to_string();
            Spans::from(vec![
                Span::styled(index_string, index_style),
                Span::raw(" ".repeat(pad_to - (index as f32 + 1f32).log10().ceil() as usize + 1)),
                Span::styled(
                    line.clone(),
                    if installed_cache.contains(&(i + skip + 1)) {
                        if selected == index - 1 {
                            installed_selected_style
                        } else {
                            installed_style
                        }
                    } else if selected == index - 1 {
                        uninstalled_selected_style
                    } else {
                        uninstalled_style
                    },
                ),
            ])
        })
        .collect()
}

fn get_info(
    query: &String,
    index: usize,
    installed_cache: &HashSet<usize>,
    command: &str,
) -> Vec<Spans<'static>> {
    let mut cmd = Command::new(command);

    if installed_cache.contains(&index) {
        cmd.arg("-Qi");
    } else {
        cmd.arg("-Si");
    };
    cmd.arg(query);

    let output = cmd_output(cmd);

    let mut info = Vec::with_capacity(output.lines().count());
    for line in output.lines().map(|c| c.to_owned()) {
        if !line.starts_with(' ') {
            if let Some((key, value)) = line.split_once(':') {
                info.push(Spans::from(vec![
                    Span::styled(
                        key.to_owned() + ":",
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(value.to_owned()),
                ]));
            }
        } else {
            info.push(Spans::from(Span::raw(line.to_owned())));
        }
    }

    info
}

fn is_installed(
    queries: &[String],
    skip: usize,
    installed_cache: &mut HashSet<usize>,
    cached_pages: &mut HashSet<usize>,
    command: &str,
) {
    if cached_pages.contains(&skip) {
        return;
    }

    let mut cmd = Command::new(command);
    cmd.arg("-Qq");
    cmd.args(queries);

    let output = cmd_output(cmd);

    let mut index;
    for (i, query) in queries.iter().enumerate() {
        index = i + skip;
        let is_installed = output.includes(&(query.to_owned() + "\n"));
        if is_installed {
            installed_cache.insert(index);
        }
    }
    cached_pages.insert(skip);
}

fn cmd_output(mut cmd: Command) -> String {
    if let Ok(output) = cmd.output() {
        if let Ok(output) = String::from_utf8(output.stdout) {
            return output;
        }
    }
    String::new()
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

fn print_help() {
    println!(
        "{}",
        [
            "Usage: parui [OPTION]... QUERY",
            "Search for QUERY in the Arch User Repository.",
            "Example:",
            "    parui -p=yay rustup\n",
            "Options:",
            "    -p=<PROGRAM>",
            "        Selects program used to search AUR",
            "        Not guaranteed to work well",
            "        Default: paru",
            "    -h",
            "        Print this help and exit",
            "Keybinds:",
            "    Both:",
            "       <Escape>",
            "           Switch Modes",
            "       <C-c>",
            "           Exit parui",
            "   Insert:",
            "       <Return>",
            "           Search for query",
            "       <C-w>",
            "           Remove previous word",
            "   Select:",
            "       i:",
            "           Enter insert mode",
            "       <Return>:",
            "           Find info or install",
            "       <C-j>, <C-Down>:",
            "           Move info one row down",
            "       <C-k>, <C-Up>:",
            "           Move info one row up",
            "       h, <Left>:",
            "           Move one page back",
            "       j, <Down>:",
            "           Move one row down",
            "       k, <Up>:",
            "           Move one row up",
            "       l, <Right>:",
            "           Move one page forwards",
            "       <S-R>:",
            "           Remove installed package",
            "       q:",
            "           Exit parui",
        ]
        .join("\n")
    );
    exit(0);
}

impl Config {
    pub fn new(args: Args) -> Self {
        let mut query: Option<String> = None;
        let mut command = String::from("paru");

        for arg in args.skip(1) {
            match arg.as_str() {
                "-h" | "--help" => print_help(),
                _ => {
                    if arg.starts_with("-p=") {
                        command = arg.clone().chars().skip(3).collect::<String>();
                    } else if let Some(q) = query {
                        query = Some(q + " " + &arg);
                    } else {
                        query = Some(arg.to_owned());
                    }
                }
            }
        }

        if let Err(err) = Command::new(command.as_str()).arg("-h").output() {
            match err.kind() {
                std::io::ErrorKind::NotFound => {
                    eprintln!("parui: {}: command not found", command);
                }
                _ => {
                    eprintln!("parui: {}: {}", command, err);
                }
            }
            exit(1);
        }

        Self { query, command }
    }
}
