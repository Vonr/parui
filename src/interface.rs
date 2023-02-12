use std::{sync::Arc, time::Duration};

use nohash_hasher::IntSet;
use parking_lot::RwLock;
use tokio::{process::Command, time::sleep};
use tui::{
    style::{Color, Modifier, Style},
    text::{Span, Spans},
};

pub async fn list(command: &str) -> Vec<String> {
    let mut cmd = Command::new(command);
    cmd.arg("-Slq");
    cmd_output(cmd)
        .await
        .lines()
        .map(ToOwned::to_owned)
        .collect()
}

pub fn search(query: &str, packages: &[String]) -> Vec<usize> {
    packages
        .iter()
        .enumerate()
        .filter_map(|(i, package)| {
            if package.contains(query) {
                Some(i)
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
}

#[allow(clippy::too_many_arguments)]
pub async fn format_results(
    packages: Arc<RwLock<Vec<String>>>,
    shown: Arc<RwLock<Vec<usize>>>,
    current: usize,
    selected: &IntSet<usize>,
    height: usize,
    pad_to: usize,
    skip: usize,
    installed: Arc<RwLock<IntSet<usize>>>,
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

    let names: Vec<String> = shown
        .read()
        .iter()
        .skip(skip)
        .take(height - 5)
        .map(|idx| packages.read()[*idx].clone())
        .collect();
    if names.is_empty() {
        return vec![Spans::default()];
    }

    names
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let real_index = shown.read()[skip + i];

            let index = i + skip + 1;
            let index_string = " ".to_string() + &index.to_string();
            let mut spans = vec![
                Span::styled(index_string, index_style),
                Span::raw(" ".repeat(pad_to - (index as f32 + 1f32).log10().ceil() as usize + 1)),
                Span::styled(
                    line.clone(),
                    if installed.read().contains(&real_index) {
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
            if selected.contains(&real_index) {
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
    installed_cache: Arc<RwLock<IntSet<usize>>>,
    command: &str,
) -> Vec<Spans<'static>> {
    let mut cmd = Command::new(command);

    if installed_cache.read().contains(&index) {
        cmd.arg("-Qi");
    } else {
        // Debounce so that we don't spam requests
        sleep(Duration::from_millis(200)).await;

        cmd.arg("-Si");
    };
    cmd.arg(query);

    let output = cmd_output(cmd).await;
    let lines = output.lines().map(ToOwned::to_owned).collect::<Vec<_>>();

    let mut info = Vec::with_capacity(lines.len());
    for mut line in lines.into_iter() {
        if !line.starts_with(' ') {
            if let Some(idx) = line.find(':') {
                let value = line.split_off(idx + 1);
                info.push(Spans::from(vec![
                    Span::styled(line, Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(value),
                ]));
            }
        } else {
            info.push(Spans::from(Span::raw(line)));
        }
    }

    info
}

pub async fn is_installed(
    packages: Arc<RwLock<Vec<String>>>,
    installed: Arc<RwLock<IntSet<usize>>>,
    command: &str,
) {
    let mut cmd = Command::new(command);
    cmd.arg("-Qq");

    for package in cmd_output(cmd).await.lines() {
        if let Some(idx) = packages.read().iter().position(|o| o == package) {
            installed.write().insert(idx);
        }
    }
}

async fn cmd_output(mut cmd: Command) -> String {
    if let Ok(output) = cmd.output().await {
        if let Ok(output) = String::from_utf8(output.stdout) {
            return output;
        }
    }
    String::new()
}
