use std::{
    borrow::Cow,
    collections::HashSet,
    ffi::OsStr,
    fs::{File, FileType},
    hash::{BuildHasherDefault, DefaultHasher},
    io::{BufRead, BufReader, Seek},
    sync::Arc,
    time::Duration,
};

use compact_strings::FixedCompactStrings;
use flate2::bufread::GzDecoder;
use nohash_hasher::IntSet;
use parking_lot::RwLock;
use regex::Regex;
use tokio::{join, process::Command, time::sleep};
use tui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use crate::{matcher::Matcher, shown::Shown};

pub async fn list(show_aur: bool) -> FixedCompactStrings {
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

    let mut out = FixedCompactStrings::with_capacity(16 * 16384, 16384);

    let Ok(pacman_out) = pacman_out else {
        return out;
    };

    let Ok(aur_out) = aur_out else {
        return out;
    };

    out.extend(
        pacman_out
            .stdout
            .split(|&b| b == b'\n')
            .filter(|line| !line.is_empty())
            .filter_map(|line| std::str::from_utf8(line).ok()),
    );

    if let Some(aur_out) = aur_out {
        let reader = BufReader::new(aur_out.into_body().into_reader());
        let decoder = GzDecoder::new(reader);

        if let Ok(s) = std::io::read_to_string(decoder) {
            out.extend(
                s.as_bytes()
                    .split(|&b| b == b'\n')
                    .filter(|line| !line.is_empty())
                    .filter_map(|line| std::str::from_utf8(line).ok()),
            );
        }
    }

    out.shrink_to_fit();
    out.shrink_meta_to_fit();

    out
}

pub fn search(query: &str, packages: &FixedCompactStrings, shown: Arc<RwLock<Shown>>) {
    let query = query.trim();
    if query.is_empty() || query == ".*" || query == "." {
        *shown.write() = Shown::All
    } else {
        fn inner<'a>(
            matcher: &(impl Matcher<&'a str> + ?Sized),
            packages: &'a FixedCompactStrings,
            shown: Arc<RwLock<Shown>>,
        ) {
            let mut handle = shown.write();
            match *handle {
                Shown::Few(_) => {
                    handle.clear();
                    handle.extend(
                        packages
                            .iter()
                            .enumerate()
                            .filter(|(_, package)| matcher.matches(*package))
                            .map(|(i, _)| i),
                    )
                }
                _ => {
                    *handle = Shown::Few(
                        packages
                            .iter()
                            .enumerate()
                            .filter(|(_, package)| matcher.matches(*package))
                            .map(|(i, _)| i)
                            .collect(),
                    )
                }
            }
        }

        if let Ok(re) = Regex::new(query) {
            inner(&re, packages, shown);
        } else {
            inner(query, packages, shown);
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn format_results<'line>(
    packages: &'line FixedCompactStrings,
    shown: Arc<RwLock<Shown>>,
    current: usize,
    selected: &IntSet<usize>,
    height: usize,
    pad_to: usize,
    skip: usize,
    installed: &IntSet<usize>,
) -> Vec<Line<'line>> {
    use crate::{cows, style};

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

    const PADDINGS: [Cow<'static, str>; 16] = cows!(
        "",
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
        "               "
    );

    const SELECTED: Span = Span {
        content: Cow::Borrowed("!"),
        style: style! { fg: Color::Yellow, mod: Modifier::BOLD, },
    };

    match shown.read().get_vec() {
        Some(shown) => shown
            .iter()
            .skip(skip)
            .take(height - 5)
            .copied()
            .enumerate()
            .map(|(i, package_idx)| {
                let real_index = shown[skip + i];
                let index = i + skip + 1;

                let index_span = Span::styled(index.to_string(), INDEX_STYLE);
                let padding_span = Span {
                    content: PADDINGS[pad_to - index.ilog10() as usize].clone(),
                    style: DEFAULT_STYLE,
                };
                let line_span = Span::styled(
                    &packages[package_idx],
                    match (installed.contains(&real_index), current == index - 1) {
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
            .map(|(i, line)| {
                let index_span = Span::styled((i + 1).to_string(), INDEX_STYLE);
                let padding_span = Span {
                    content: PADDINGS[pad_to - (i + 1).ilog10() as usize].clone(),
                    style: DEFAULT_STYLE,
                };
                let line_span = Span::styled(
                    line,
                    match (installed.contains(&i), current == i) {
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

pub async fn get_info<'line>(
    all_packages: &FixedCompactStrings,
    index: usize,
    installed_cache: &IntSet<usize>,
    command: &str,
) -> Vec<Line<'line>> {
    if index >= all_packages.len() {
        return Vec::new();
    }

    let mut cmd = Command::new(command);

    if installed_cache.contains(&index) {
        cmd.arg("-Qi");
    } else {
        // Debounce so that we don't spam requests
        sleep(Duration::from_millis(200)).await;

        cmd.arg("-Si");
    };

    cmd.arg(&all_packages[index]);

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
        if line.starts_with(' ') {
            info.push(Line::from(line));
            continue;
        }

        if let Some(idx) = line.find(':') {
            let value = line.split_off(idx + 1);
            info.push(Line::from(vec![
                Span::styled(line, KEY_STYLE),
                Span::raw(value),
            ]));
        }
    }

    info
}

pub fn check_installed(packages: &FixedCompactStrings) -> IntSet<usize> {
    const DIR: &str = "/var/lib/pacman/local/";

    let Ok(dir) = std::fs::read_dir(DIR) else {
        return IntSet::default();
    };

    let mut out = IntSet::with_capacity_and_hasher(512, Default::default());
    let mut set =
        HashSet::with_capacity_and_hasher(512, BuildHasherDefault::<DefaultHasher>::default());

    let mut path = Vec::with_capacity(256);
    let dir_os = OsStr::new(DIR);
    let dir_os_bytes = dir_os.as_encoded_bytes();
    let dir_len = dir_os_bytes.len();
    path.extend_from_slice(dir_os_bytes);

    let mut reader: Option<BufReader<File>> = None;
    for entry in dir.filter_map(Result::ok) {
        let Ok(true) = entry.file_type().as_ref().map(FileType::is_dir) else {
            continue;
        };

        path.extend_from_slice(entry.file_name().as_encoded_bytes());
        path.extend_from_slice(OsStr::new("/desc").as_encoded_bytes());

        // SAFETY: `path` only contains content that originated from `OsStr::as_encoded_bytes`
        let Ok(file) = File::open(unsafe { OsStr::from_encoded_bytes_unchecked(&path) }) else {
            continue;
        };

        unsafe {
            path.set_len(dir_len);
        }

        match reader {
            Some(ref mut reader) => {
                reader.rewind().unwrap();
                *reader.get_mut() = file;
            }
            None => {
                reader = Some(BufReader::new(file));
            }
        }

        if let Some(Ok(name)) = reader.as_mut().unwrap().lines().nth(1) {
            set.insert(name);
        };
    }

    for (pos, _) in packages
        .iter()
        .enumerate()
        .filter(|(_, p)| set.contains(*p))
    {
        out.insert(pos);
    }
    out
}

async fn cmd_output(mut cmd: Command) -> String {
    cmd.output()
        .await
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .unwrap_or_default()
}
