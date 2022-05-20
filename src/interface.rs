use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
    time::Duration,
};

use naive_opt::Search;
use tokio::{process::Command, time::sleep};
use tui::{
    style::{Color, Modifier, Style},
    text::{Span, Spans},
};

pub async fn search(query: &str, command: &str) -> String {
    let mut cmd = Command::new(command);
    cmd.args(["--topdown", "-Ssq", query]);
    cmd_output(cmd).await
}

#[allow(clippy::too_many_arguments)]
pub async fn format_results(
    lines: &[String],
    current: usize,
    selected: &HashSet<usize>,
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

    is_installed(&lines, skip, installed_cache, cached_pages, command).await;

    lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let index = i + skip + 1;
            let index_string = " ".to_string() + &index.to_string();
            let mut spans = vec![
                Span::styled(index_string, index_style),
                Span::raw(" ".repeat(pad_to - (index as f32 + 1f32).log10().ceil() as usize + 1)),
                Span::styled(
                    line.clone(),
                    if installed_cache.contains(&(i + skip)) {
                        if current == index - 1 {
                            installed_selected_style
                        } else {
                            installed_style
                        }
                    } else if current == index - 1 {
                        uninstalled_selected_style
                    } else {
                        uninstalled_style
                    },
                ),
            ];
            if selected.contains(&(i + skip)) {
                spans.push(Span::styled(
                    " !",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ));
            }
            Spans::from(spans)
        })
        .collect()
}

pub async fn get_info(
    query: &String,
    index: usize,
    installed_cache: Arc<Mutex<HashSet<usize>>>,
    command: &str,
) -> Vec<Spans<'static>> {
    let mut cmd = Command::new(command);

    if installed_cache.lock().unwrap().contains(&index) {
        cmd.arg("-Qi");
    } else {
        cmd.arg("-Si");
        sleep(Duration::from_millis(200)).await;
    };
    cmd.arg(query);

    let output = cmd_output(cmd).await;

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

async fn is_installed(
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

    let output = cmd_output(cmd).await;

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

async fn cmd_output(mut cmd: Command) -> String {
    if let Ok(output) = cmd.output().await {
        if let Ok(output) = String::from_utf8(output.stdout) {
            return output;
        }
    }
    String::new()
}
