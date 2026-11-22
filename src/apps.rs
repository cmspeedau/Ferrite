//! Installed applications, read from freedesktop `.desktop` entries.
//!
//! The dock and the application grid need the list of programs the user can start, plus an icon
//! for each one. Both come from the desktop entry files the system and the user already install
//! (`$XDG_DATA_DIRS/applications/*.desktop`), so nothing has to be configured by hand.

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::OnceLock,
};

#[allow(deprecated)]
use egui_extras::{image::FitTo, RetainedImage};
use tracing::debug;

/// One launchable application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppEntry {
    /// Desktop entry file name, e.g. `firefox.desktop`. Stable id used in the pin file.
    pub id: String,
    /// Human readable name shown in the grid.
    pub name: String,
    /// `Exec` line with field codes removed, ready for a shell.
    pub exec: String,
    /// Icon name or path from the `Icon` key.
    pub icon: Option<String>,
    /// `StartupWMClass`, used to tell when the application is already running.
    pub wm_class: Option<String>,
}

impl AppEntry {
    /// True when `app_id` (a Wayland app id, or an X11 class) refers to this application.
    ///
    /// Clients report themselves in several shapes (`firefox`, `Firefox`, `org.mozilla.firefox`),
    /// so every name the entry is known by is compared case-insensitively.
    pub fn matches_app_id(&self, app_id: &str) -> bool {
        let app_id = app_id.to_lowercase();
        if app_id.is_empty() {
            return false;
        }
        let id = self.id.trim_end_matches(".desktop").to_lowercase();
        let name = self.name.to_lowercase();
        let class = self
            .wm_class
            .as_deref()
            .map(|class| class.to_lowercase())
            .unwrap_or_default();
        let program = self
            .exec
            .split_whitespace()
            .next()
            .map(|p| {
                Path::new(p)
                    .file_name()
                    .map(|f| f.to_string_lossy().to_lowercase())
                    .unwrap_or_default()
            })
            .unwrap_or_default();
        id == app_id || name == app_id || class == app_id || program == app_id
    }
}

/// Every launchable application, sorted by name.
///
/// `Err` carries a human readable reason (no readable application directory), which the grid
/// shows instead of an empty list, so a broken setup is visible rather than silent.
pub fn applications() -> Result<Vec<AppEntry>, String> {
    let dirs = application_dirs();
    let readable = dirs.iter().filter(|d| d.is_dir()).count();
    if readable == 0 {
        return Err(format!(
            "no application directory could be read (looked in {})",
            dirs.iter()
                .map(|d| d.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    // Later directories hold the user's own entries, which win over the system ones, so the
    // scan runs from the least to the most specific and keeps the last entry per id.
    let mut by_id: HashMap<String, AppEntry> = HashMap::new();
    for dir in dirs.iter().rev() {
        let Ok(read_dir) = fs::read_dir(dir) else {
            continue;
        };
        for entry in read_dir.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
                continue;
            }
            match parse_desktop(&path) {
                Some(app) => {
                    by_id.insert(app.id.clone(), app);
                }
                None => debug!("Skipping {}", path.display()),
            }
        }
    }

    let mut apps: Vec<AppEntry> = by_id.into_values().collect();
    apps.sort_by_cached_key(|app| app.name.to_lowercase());
    debug!("Found {} applications", apps.len());
    Ok(apps)
}

fn application_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(data_home) = std::env::var("XDG_DATA_HOME") {
        if !data_home.is_empty() {
            dirs.push(PathBuf::from(data_home).join("applications"));
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        dirs.push(PathBuf::from(home).join(".local/share/applications"));
    }
    let data_dirs = std::env::var("XDG_DATA_DIRS")
        .unwrap_or_else(|_| "/usr/local/share:/usr/share".to_string());
    for dir in data_dirs.split(':') {
        if !dir.is_empty() {
            dirs.push(PathBuf::from(dir).join("applications"));
        }
    }
    dirs
}

fn parse_desktop(path: &Path) -> Option<AppEntry> {
    let content = fs::read_to_string(path)
        .map_err(|err| debug!("{}: {err}", path.display()))
        .ok()?;

    let mut fields = HashMap::new();
    let mut in_main = false;
    for line in content.lines() {
        let line = line.trim();
        if let Some(section) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            // Only the main section describes the entry itself; `Desktop Action`s do not.
            in_main = section == "Desktop Entry";
            continue;
        }
        if !in_main || line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        // Localized and variant keys (`Name[de]`, `Keywords[en]`) are not needed here.
        if key.contains('[') {
            continue;
        }
        fields.insert(key.trim().to_string(), unescape(value.trim()));
    }

    let flag = |key: &str| {
        fields
            .get(key)
            .map(|v| v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
    };

    if let Some(kind) = fields.get("Type") {
        if kind != "Application" {
            return None;
        }
    }
    if flag("NoDisplay") || flag("Hidden") {
        return None;
    }
    // A terminal application launched without a terminal would start and instantly look broken.
    if flag("Terminal") {
        return None;
    }
    if let Some(try_exec) = fields.get("TryExec") {
        if !on_path(try_exec) {
            return None;
        }
    }
    let exec = fields.get("Exec")?.clone();
    if exec.is_empty() {
        return None;
    }

    let id = path.file_name()?.to_string_lossy().to_string();
    let name = fields
        .get("Name")
        .cloned()
        .or_else(|| path.file_stem().map(|s| s.to_string_lossy().to_string()))?;

    Some(AppEntry {
        id,
        name,
        exec: strip_field_codes(&exec),
        icon: fields.get("Icon").cloned().filter(|icon| !icon.is_empty()),
        wm_class: fields
            .get("StartupWMClass")
            .cloned()
            .filter(|class| !class.is_empty()),
    })
}

/// `Exec` values quote the program and pass field codes (`%f`, `%u`, ...) for the launcher to
/// fill in. Neither survives into a plain shell command, so both are removed here.
fn strip_field_codes(exec: &str) -> String {
    exec.split_whitespace()
        .filter(|arg| {
            !(arg.len() == 2
                && arg.starts_with('%')
                && matches!(
                    arg.chars().nth(1).unwrap_or(' '),
                    'f' | 'F' | 'u' | 'U' | 'd' | 'D' | 'n' | 'N' | 'i' | 'c' | 'k' | 'v' | 'm'
                ))
        })
        .collect::<Vec<_>>()
        .join(" ")
        .replace('"', "'")
        .trim()
        .to_string()
}

fn unescape(value: &str) -> String {
    value.replace("\\s", " ").replace("\\%", "%")
}

fn on_path(program: &str) -> bool {
    if program.contains('/') {
        return Path::new(program).exists();
    }
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .any(|dir| dir.join(program).exists())
}

/// Where an icon file was found, and in which format it has to be decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IconFile {
    Png(PathBuf),
    Svg(PathBuf),
}

/// Resolve a desktop entry `Icon` value to a file on disk.
///
/// Sizes are tried from the largest down so magnification stays sharp, and vector icons win
/// over bitmaps because they are rasterized at exactly the size the tile needs.
pub fn find_icon(icon: &str, px: u16) -> Option<IconFile> {
    if icon.is_empty() {
        return None;
    }
    if icon.contains('/') {
        let path = PathBuf::from(icon);
        if !path.exists() {
            return None;
        }
        return Some(
            if path.extension().and_then(|e| e.to_str()) == Some("svg") {
                IconFile::Svg(path)
            } else {
                IconFile::Png(path)
            },
        );
    }

    let dirs = icon_dirs();
    for dir in dirs {
        for category in ICON_CATEGORIES {
            let svg = dir
                .join("scalable")
                .join(category)
                .join(format!("{icon}.svg"));
            if svg.exists() {
                return Some(IconFile::Svg(svg));
            }
        }
    }
    for size in ICON_SIZES.iter().rev() {
        // A bitmap smaller than half the tile would be upscaled into mush; keep looking.
        if (*size as u32) * 2 < px as u32 {
            continue;
        }
        for dir in dirs {
            for category in ICON_CATEGORIES {
                let candidate = dir
                    .join(format!("{size}x{size}"))
                    .join(category)
                    .join(format!("{icon}.png"));
                if candidate.exists() {
                    return Some(IconFile::Png(candidate));
                }
            }
        }
    }
    // Last resort: any bitmap at all, however small it has to be stretched.
    for size in ICON_SIZES.iter().rev() {
        for dir in dirs {
            for category in ICON_CATEGORIES {
                let candidate = dir
                    .join(format!("{size}x{size}"))
                    .join(category)
                    .join(format!("{icon}.png"));
                if candidate.exists() {
                    return Some(IconFile::Png(candidate));
                }
            }
        }
    }
    for dir in ["/usr/share/pixmaps", "/usr/local/share/pixmaps"] {
        for ext in ["png", "xpm"] {
            let candidate = Path::new(dir).join(format!("{icon}.{ext}"));
            if candidate.exists() {
                return Some(IconFile::Png(candidate));
            }
        }
    }
    None
}

const ICON_SIZES: [u16; 8] = [16, 22, 24, 32, 48, 64, 128, 256];
const ICON_CATEGORIES: [&str; 7] = [
    "apps",
    "sites",
    "places",
    "status",
    "mimetypes",
    "categories",
    "",
];

/// Theme directories to search, most generic last. Cached because the dock looks icons up on
/// every frame it redraws a tile.
fn icon_dirs() -> &'static [PathBuf] {
    static DIRS: OnceLock<Vec<PathBuf>> = OnceLock::new();
    DIRS.get_or_init(|| {
        let mut roots = Vec::new();
        if let Some(home) = std::env::var_os("XDG_DATA_HOME") {
            roots.push(PathBuf::from(home).join("icons"));
        }
        if let Some(home) = std::env::var_os("HOME") {
            roots.push(PathBuf::from(home).join(".local/share/icons"));
        }
        let data_dirs = std::env::var("XDG_DATA_DIRS")
            .unwrap_or_else(|_| "/usr/local/share:/usr/share".to_string());
        for dir in data_dirs.split(':') {
            if !dir.is_empty() {
                roots.push(PathBuf::from(dir).join("icons"));
            }
        }

        let mut dirs = Vec::new();
        for root in roots {
            let Ok(read_dir) = fs::read_dir(root) else {
                continue;
            };
            let mut themes: Vec<PathBuf> = read_dir
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.is_dir())
                // Cursor themes carry no app icons and only slow the search down.
                .filter(|path| path.join("index.theme").exists() || has_app_dir(path))
                .collect();
            themes.sort();
            // hicolor is the fallback theme every other one inherits from, so its icons count
            // as much as the branded ones.
            themes.sort_by_key(|path| {
                if path.file_name().and_then(|n| n.to_str()) == Some("hicolor") {
                    0
                } else {
                    1
                }
            });
            dirs.extend(themes);
        }
        debug!("Indexed {} icon theme directories", dirs.len());
        dirs
    })
}

fn has_app_dir(theme: &Path) -> bool {
    fs::read_dir(theme)
        .map(|entries| {
            entries.flatten().any(|entry| {
                let path = entry.path();
                path.is_dir() && path.join("apps").is_dir()
            })
        })
        .unwrap_or(false)
}

/// Rasterize an icon file into an egui texture source.
///
/// `RetainedImage` is deprecated in favour of egui's async image loaders, which need a
/// `Context` and a byte source we do not have: the dock decodes from a known file and keeps the
/// texture itself. This is the same type egui uses internally.
#[allow(deprecated)]
pub fn load_icon(file: &IconFile, px: usize) -> Result<RetainedImage, String> {
    let bytes = fs::read(match file {
        IconFile::Png(path) | IconFile::Svg(path) => path,
    })
    .map_err(|err| format!("{}: {err}", file_path(file).display()))?;

    let name = file_path(file).display().to_string();
    match file {
        IconFile::Svg(_) => {
            RetainedImage::from_svg_bytes_with_size(name, &bytes, FitTo::Size(px as u32, px as u32))
        }
        IconFile::Png(_) => RetainedImage::from_image_bytes(name, &bytes),
    }
}

fn file_path(file: &IconFile) -> &Path {
    match file {
        IconFile::Png(path) | IconFile::Svg(path) => path,
    }
}
