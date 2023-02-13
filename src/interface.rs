use std::{sync::Arc, time::Duration};

use nohash_hasher::IntSet;
use parking_lot::RwLock;
use tokio::{join, process::Command, time::sleep};
use tui::{
    style::{Color, Modifier, Style},
    text::{Span, Spans},
};

pub async fn list() -> Vec<String> {
    let mut cmd = Command::new("pacman");
    cmd.arg("-Slq");

    let mut out = Vec::new();

    let pacman_out = async move { cmd.output().await.map(|o| o.stdout) };
    let aur_out = tokio::task::spawn_blocking(move || {
        let mut buf = Vec::new();

        // This is a HTTP URL so that we don't have to bring in the tls crate, which increases
        // compile times by up to 10 seconds on my very decent machine.
        //
        // This may not sound like a big deal, but I'd like to reduce compile times for people
        // installing my software on old hardware.
        //
        // I believe this traadeoff is okay, as a MITM can not exploit this to perform anything
        // malicious other than report false information about packages to parui, or crash it.
        let Ok(req) = ureq::get("http://aur.archlinux.org/packages.gz").call() else {
            return buf;
        };
        _ = req.into_reader().read_to_end(&mut buf);
        buf
    });

    let (pacman_out, aur_out) = join!(pacman_out, aur_out);

    let Ok(pacman_out) = pacman_out else {
        return out;
    };

    let Ok(aur_out) = aur_out else {
        return out;
    };

    let mut buf = Vec::new();
    for byte in pacman_out.into_iter().chain(aur_out.into_iter()) {
        if byte != b'\n' {
            buf.push(byte);
            continue;
        }

        if let Ok(s) = std::str::from_utf8(&buf) {
            out.push(s.to_owned());
        }

        buf.clear();
    }

    out
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
pub async fn format_results<'a>(
    packages: Arc<RwLock<Vec<String>>>,
    shown: Arc<RwLock<Vec<usize>>>,
    current: usize,
    selected: &IntSet<usize>,
    height: usize,
    pad_to: usize,
    skip: usize,
    installed: Arc<RwLock<IntSet<usize>>>,
) -> Vec<Spans<'a>> {
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
        .into_iter()
        .enumerate()
        .map(|(i, line)| {
            let real_index = shown.read()[skip + i];

            let index = i + skip + 1;
            let mut spans = vec![
                Span::styled(
                    index.to_string()
                        + &" ".repeat(pad_to - (index as f32 + 1f32).log10().ceil() as usize + 1),
                    index_style,
                ),
                Span::styled(
                    line,
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
                    "!",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ));
            }
            Spans::from(spans)
        })
        .collect()
}

pub async fn get_info<'a>(
    query: &String,
    index: usize,
    installed_cache: Arc<RwLock<IntSet<usize>>>,
    command: &str,
) -> Vec<Spans<'a>> {
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
    for mut line in lines {
        if !line.starts_with(' ') {
            if let Some(idx) = line.find(':') {
                let value = line.split_off(idx + 1);
                info.push(Spans::from(vec![
                    Span::styled(line, Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(value),
                ]));
            }
        } else {
            info.push(Spans::from(line));
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

    let output = cmd_output(cmd).await;
    let mut lines = output.lines().collect::<Vec<_>>();
    let mut installed = installed.write();

    for (idx, package) in packages.read().iter().enumerate() {
        if let Some(line_idx) = lines.iter().position(|l| l == package) {
            lines.remove(line_idx);
            installed.insert(idx);
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
