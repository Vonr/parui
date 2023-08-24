use std::{
    borrow::Cow,
    collections::HashSet,
    io::{BufRead, BufReader},
    sync::Arc,
    time::Duration,
};

use compact_strings::CompactStrings;
use nohash_hasher::IntSet;
use parking_lot::RwLock;
use tokio::{join, process::Command, time::sleep};
use tui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use crate::shown::Shown;

pub async fn list(all_packages: Arc<RwLock<CompactStrings>>, show_aur: bool) {
    let mut cmd = Command::new("pacman");
    cmd.arg("-Slq");

    let pacman_out = cmd.output();
    let aur_out = tokio::task::spawn_blocking(move || {
        if show_aur {
            ureq::get("https://aur.archlinux.org/packages.gz")
                .call()
                .ok()
        } else {
            None
        }
    });

    let (pacman_out, aur_out) = join!(pacman_out, aur_out);

    let Ok(pacman_out) = pacman_out else {
        return;
    };

    let Ok(aur_out) = aur_out else {
        return;
    };

    let mut out = all_packages.write();
    out.clear();

    let mut buf = Vec::with_capacity(128);
    let mut push_byte = |byte| {
        if byte != b'\n' {
            buf.push(byte);
            return;
        }

        if let Ok(s) = std::str::from_utf8(&buf) {
            out.push(s);
        }

        buf.clear();
    };

    pacman_out.stdout.into_iter().for_each(&mut push_byte);
    aur_out.map(|o| {
        o.into_string()
            .map(|s| s.into_bytes().into_iter().for_each(push_byte))
    });

    out.shrink_to_fit();
    out.shrink_meta_to_fit();
}

pub fn search(shown: Arc<RwLock<Shown>>, query: &str, packages: &CompactStrings) {
    let mut shown = shown.write();
    shown.clear();
    *shown = if query.is_empty() {
        Shown::All
    } else {
        Shown::Few(
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
                .collect(),
        )
    }
}

#[allow(clippy::too_many_arguments)]
pub fn format_results<'a>(
    packages: Arc<RwLock<CompactStrings>>,
    shown: Arc<RwLock<Shown>>,
    current: usize,
    selected: &IntSet<usize>,
    height: usize,
    pad_to: usize,
    skip: usize,
    installed: Arc<RwLock<IntSet<usize>>>,
) -> Vec<Line<'a>> {
    use crate::{raws, style};

    const INDEX_STYLE: Style = style!(Color::Gray);
    const INSTALLED_STYLE: Style = style! {
        fg: Color::Green,
        mod: Modifier::BOLD,
    };
    const INSTALLED_SELECTED_STYLE: Style = style! {
        fg: Color::Yellow,
        bg: Color::Red,
        mod: Modifier::BOLD,
    };
    const UNINSTALLED_STYLE: Style = style! {
        fg: Color::LightBlue,
        mod: Modifier::BOLD,
    };
    const UNINSTALLED_SELECTED_STYLE: Style = style! {
        fg: Color::Blue,
        bg: Color::Red,
        mod: Modifier::BOLD,
    };
    const DEFAULT_STYLE: Style = style!();

    const PADDINGS: [Span; 16] = raws!(
        " ",
        "  ",
        "   ",
        "    ",
        "     ",
        "      ",
        "       ",
        "        ",
        "         ",
        "          ",
        "           ",
        "            ",
        "             ",
        "              ",
        "               ",
        "                "
    );

    const SELECTED: Span = Span {
        content: Cow::Borrowed("!"),
        style: style! { fg: Color::Yellow, mod: Modifier::BOLD, },
    };

    let packages = packages.read();
    match shown.read().get_vec() {
        Some(shown) => shown
            .iter()
            .skip(skip)
            .take(height - 5)
            .copied()
            .map(|idx| packages[idx].to_string())
            .enumerate()
            .map(|(i, line)| {
                let real_index = shown[skip + i];
                let index = i + skip + 1;

                let index_span = Span::styled(index.to_string(), INDEX_STYLE);
                let padding_span = PADDINGS[pad_to - index.ilog10() as usize].clone();
                let line_span = Span::styled(
                    line,
                    match (installed.read().contains(&real_index), current == index - 1) {
                        (true, true) => INSTALLED_SELECTED_STYLE,
                        (true, false) => INSTALLED_STYLE,
                        (false, true) => UNINSTALLED_SELECTED_STYLE,
                        (false, false) => UNINSTALLED_STYLE,
                    },
                );

                let spans = if selected.contains(&real_index) {
                    vec![index_span, padding_span, line_span, SELECTED]
                } else {
                    vec![index_span, padding_span, line_span]
                };
                Line::from(spans)
            })
            .collect(),
        None => packages
            .iter()
            .enumerate()
            .skip(skip)
            .take(height - 5)
            .map(|(i, s)| (i, s.to_string()))
            .map(|(i, line)| {
                let index_span = Span::styled((i + 1).to_string(), INDEX_STYLE);
                let padding_span = PADDINGS[pad_to - (i + 1).ilog10() as usize].clone();
                let line_span = Span::styled(
                    line,
                    match (installed.read().contains(&i), current == i) {
                        (true, true) => INSTALLED_SELECTED_STYLE,
                        (true, false) => INSTALLED_STYLE,
                        (false, true) => UNINSTALLED_SELECTED_STYLE,
                        (false, false) => UNINSTALLED_STYLE,
                    },
                );

                let spans = if selected.contains(&i) {
                    vec![index_span, padding_span, line_span, SELECTED]
                } else {
                    vec![index_span, padding_span, line_span]
                };
                Line::from(spans)
            })
            .collect(),
    }
}

pub async fn get_info<'a>(
    all_packages: Arc<RwLock<CompactStrings>>,
    index: usize,
    installed_cache: Arc<RwLock<IntSet<usize>>>,
    command: &str,
) -> Vec<Line<'a>> {
    if index >= all_packages.read().len() {
        return Vec::new();
    }

    let mut cmd = Command::new(command);

    if installed_cache.read().contains(&index) {
        cmd.arg("-Qi");
    } else {
        // Debounce so that we don't spam requests
        sleep(Duration::from_millis(200)).await;

        cmd.arg("-Si");
    };

    cmd.arg(&all_packages.read()[index]);

    let output = cmd_output(cmd).await;
    let lines = output.lines().map(ToOwned::to_owned).collect::<Vec<_>>();

    const KEY_STYLE: Style = Style {
        fg: None,
        bg: None,
        underline_color: None,
        add_modifier: Modifier::BOLD,
        sub_modifier: Modifier::empty(),
    };

    let mut info = Vec::with_capacity(lines.len());
    for mut line in lines {
        if !line.starts_with(' ') {
            if let Some(idx) = line.find(':') {
                let value = line.split_off(idx + 1);
                info.push(Line::from(vec![
                    Span::styled(line, KEY_STYLE),
                    Span::raw(value),
                ]));
            }
        } else {
            info.push(Line::from(line));
        }
    }

    info
}

pub async fn check_installed(
    packages: Arc<RwLock<CompactStrings>>,
    installed: Arc<RwLock<IntSet<usize>>>,
) {
    const PATH: &str = "/var/lib/pacman/local";
    const DESC: &str = "desc";

    let Ok(dir) = std::fs::read_dir(PATH) else {
        return;
    };

    let mut set = HashSet::new();
    for entry in dir.filter_map(Result::ok) {
        let Ok(ft) = entry.file_type() else {
            continue;
        };

        if !ft.is_dir() {
            continue;
        }

        let path = entry.path().join(DESC);
        let Ok(file) = std::fs::File::open(path) else {
            continue;
        };

        let Some(Ok(name)) = BufReader::new(file).lines().nth(1) else {
            continue;
        };

        set.insert(name);
    }

    let mut installed = installed.write();
    for (pos, _) in packages
        .read()
        .iter()
        .enumerate()
        .filter(|(_, p)| set.contains(*p))
    {
        installed.insert(pos);
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
