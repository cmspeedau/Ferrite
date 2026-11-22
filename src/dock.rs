//! The dock: pinned applications, magnification, and the application grid.
//!
//! The dock is drawn by the compositor with egui (`docs/dock-mockup.html` is the design
//! reference, `docs/theme.md` the palette). It owns three things: the pinned app list, the "all
//! applications" grid behind the launcher tile, and the drag that moves an app between them.
//! What the user asked for is reported back as a [`DockRequest`]; starting and raising programs
//! stays in [`crate::state`], where the windows live. The drawing itself is in [`mod@ui`].

use std::{collections::HashMap, fs, path::PathBuf};

use egui::{Color32, Rect, Rounding, Stroke, TextureId, Vec2};
#[allow(deprecated)]
use egui_extras::RetainedImage;
use serde::{Deserialize, Serialize};
use smithay::{
    backend::renderer::{
        element::texture::TextureRenderElement,
        gles::{GlesError, GlesTexture},
        glow::GlowRenderer,
    },
    input::keyboard::{KeysymHandle, ModifiersState},
    utils::{Logical, Point, Rectangle},
};
use smithay_egui::EguiState;
use tracing::{debug, warn};

use crate::{
    apps::{AppEntry, IconFile},
    state::CONFIG,
};

mod ui;

/// Something the user asked the dock to do with an application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DockRequest {
    /// Raise one of the application's windows, or start it when none is open.
    Activate(String),
}

/// Palette from `docs/theme.md`. One accent (FICSIT orange) carries every "this is live" signal:
/// the running dot, the focus ring and the drop marker. Everything else stays neutral so the
/// accent means something.
pub const ACCENT: Color32 = Color32::from_rgb(0xff, 0x6b, 0x00);
pub const SURFACE: Color32 = Color32::from_rgb(0x17, 0x18, 0x1d);
pub const SURFACE_HI: Color32 = Color32::from_rgb(0x28, 0x2c, 0x34);
pub const BASE: Color32 = Color32::from_rgb(0x08, 0x09, 0x0b);
pub const TEXT: Color32 = Color32::from_rgb(0xec, 0xef, 0xf3);
pub const TEXT_DIM: Color32 = Color32::from_rgb(0x9a, 0xa1, 0xad);
pub const HAIRLINE: Color32 = Color32::from_rgba_premultiplied(255, 255, 255, 20);
pub const HOVER_BG: Color32 = Color32::from_rgba_premultiplied(255, 255, 255, 16);

/// Panel corner radius from the mockup, clamped to half the panel height.
pub const PANEL_RADIUS: f32 = 28.0;
/// Tile radius from the mockup's `.app-tile-icon`.
pub const TILE_RADIUS: f32 = 10.0;
/// Hover magnification from the mockup (`scale(1.35)`).
pub const HOVER_BOOST: f32 = 1.35;
/// Falloff width of the magnification, in icon widths. Two widths keeps the neighbour of a
/// hovered tile near its real size instead of inflating half the dock.
pub const MAGNET_WIDTH: f32 = 2.0;
/// Gap between the dock and the screen edge, from the mockup's `margin-bottom`.
pub const EDGE_MARGIN: f32 = 16.0;
/// Space between dock tiles and grid tiles.
pub const TILE_GAP: f32 = 8.0;
/// Inner padding of panels.
pub const PANEL_PAD: f32 = 12.0;
/// Hairline between the pinned apps and the launcher tile, from the mockup's `.dock-divider`.
pub const DIVIDER_W: f32 = 13.0;
/// Height of the running-app indicator (dot plus its gap to the icon).
pub const INDICATOR_H: f32 = 10.0;
/// Height of an application name under a grid tile.
pub const LABEL_H: f32 = 16.0;
/// Grid columns, from the mockup's `.app-grid`.
pub const GRID_COLUMNS: usize = 4;
/// Reference size used when picking which icon-theme bitmap to load. The dock magnifies up to
/// about this edge, so this is the smallest file that stays sharp at full boost.
pub const ICON_LOOKUP_PX: u16 = 128;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct DockPins {
    pub apps: Vec<String>,
}

/// An application being dragged, and where it came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Drag {
    /// From the grid: dropping it on the dock pins the application.
    Grid(usize),
    /// A pinned slot: dropping it back on the dock reorders, dropping it outside unpins.
    Dock(usize),
}

#[derive(Debug, Clone)]
pub(crate) struct Layout {
    /// The dock panel, in the egui screen space of the frame that computed it.
    pub panel: Rect,
    /// One slot per pinned application.
    pub slots: Vec<Rect>,
    pub launcher: Rect,
}

impl Default for Layout {
    fn default() -> Self {
        // Nothing is visible until the first frame places the panel.
        let empty = Rect::NOTHING;
        Self {
            panel: empty,
            slots: Vec::new(),
            launcher: empty,
        }
    }
}

pub struct DockUi {
    egui: EguiState,
    /// Desktop entry ids, in dock order.
    pins: Vec<String>,
    /// Running applications that are not pinned: they get a temporary tile that vanishes when
    /// the application closes, like the running (unpinned) apps on a macOS dock.
    transient: Vec<String>,
    pins_path: PathBuf,
    /// `None` until the scan has run: reading every `.desktop` entry costs a few hundred file
    /// reads, so it happens one frame after the compositor starts instead of on the hot path.
    apps: Option<Result<Vec<AppEntry>, String>>,
    /// Set at construction and when the grid opens; consumed by the next frame, so the loading
    /// state is drawn before the scan blocks.
    scan_pending: bool,
    grid_open: bool,
    query: String,
    /// Keyboard cursor into `visible`.
    cursor: usize,
    columns: usize,
    /// Indexes into `apps`, in grid order, of the applications the last frame showed.
    visible: Vec<usize>,
    drag: Option<Drag>,
    insert_at: Option<usize>,
    layout: Layout,
    requests: Vec<DockRequest>,
    /// Focus the search field on the frame the grid opens.
    focus_search: bool,
    /// Resolved icon file per icon name (`None` = looked up, nothing there).
    icon_files: HashMap<String, Option<IconFile>>,
    /// Rasterized icons, keyed by file and pixel size.
    #[allow(deprecated)]
    icons: HashMap<String, RetainedImage>,
}

impl Default for DockUi {
    fn default() -> Self {
        Self::new()
    }
}

impl DockUi {
    pub fn new() -> Self {
        let pins_path = pins_path();
        let mut pins = read_pins(&pins_path);
        if pins.is_empty() && !pins_path.exists() {
            // First run: seed from `dock_apps` so the bar is not empty out of the box. From here
            // on the pin file owns the list, and dragging is what edits it.
            pins = CONFIG
                .dock_apps
                .iter()
                .map(|entry| seed_to_id(entry))
                .collect::<Vec<_>>();
            write_pins(&pins_path, &pins);
        }

        let egui = EguiState::new(Rectangle::from_loc_and_size((0, 0), (1, 1)));
        egui.context().set_visuals(visuals());

        Self {
            egui,
            pins,
            transient: Vec::new(),
            pins_path,
            apps: None,
            scan_pending: true,
            grid_open: false,
            query: String::new(),
            cursor: 0,
            columns: GRID_COLUMNS,
            visible: Vec::new(),
            drag: None,
            insert_at: None,
            layout: Layout::default(),
            requests: Vec::new(),
            focus_search: false,
            icon_files: HashMap::new(),
            icons: HashMap::new(),
        }
    }

    /// Draw one frame of the dock and, when open, the application grid.
    ///
    /// `area` is the output in logical coordinates; `running` holds the app ids reported by the
    /// open windows and drives the running indicators.
    pub fn render(
        &mut self,
        renderer: &mut GlowRenderer,
        area: Rectangle<i32, Logical>,
        running: &[String],
    ) -> Result<TextureRenderElement<GlesTexture>, GlesError> {
        self.egui.set_focused(self.grid_open);
        // `EguiState` is an `Arc` handle, so this clone lets the draw closure take `&mut self`.
        let egui = self.egui.clone();
        egui.render(
            |ctx| self.draw_frame(ctx, running),
            renderer,
            area,
            1.0,
            1.0,
        )
    }

    /// One frame of dock and grid: layout, hit-testing, state changes.
    ///
    /// Split out from [`DockUi::render`] so the interaction logic can be driven with synthetic
    /// egui input, without a GL context.
    pub fn draw_frame(&mut self, ctx: &egui::Context, running: &[String]) {
        ui::draw(ctx, self, running)
    }

    /// Take what the user asked for during the last frame.
    pub fn take_requests(&mut self) -> Vec<DockRequest> {
        std::mem::take(&mut self.requests)
    }

    pub(crate) fn push_request(&mut self, request: DockRequest) {
        self.requests.push(request);
    }

    pub fn grid_open(&self) -> bool {
        self.grid_open
    }

    pub fn toggle_grid(&mut self) {
        self.set_grid_open(!self.grid_open);
    }

    pub fn set_grid_open(&mut self, open: bool) {
        if open == self.grid_open {
            return;
        }
        self.grid_open = open;
        if open {
            self.scan_pending |= self.apps.is_none();
            self.query.clear();
            self.cursor = 0;
            self.focus_search = true;
        } else {
            self.drag = None;
            self.insert_at = None;
        }
    }

    /// Should keyboard input go to the dock instead of the focused client?
    pub fn wants_keyboard(&self) -> bool {
        self.grid_open
    }

    /// Should pointer input go to the dock instead of the focused client?
    pub fn wants_pointer(&self) -> bool {
        self.grid_open || self.drag.is_some() || self.egui.wants_pointer()
    }

    pub fn handle_pointer_motion(&mut self, position: Point<i32, Logical>) {
        self.egui.handle_pointer_motion(position);
    }

    pub fn handle_pointer_button(
        &mut self,
        button: smithay::backend::input::MouseButton,
        pressed: bool,
    ) {
        self.egui.handle_pointer_button(button, pressed);
    }

    pub fn handle_pointer_axis(&mut self, x: f64, y: f64) {
        self.egui.handle_pointer_axis(x, y);
    }

    /// Feed a key to the search field. Every key is taken while the grid is open: it is modal,
    /// and `Escape` (handled while drawing) is the way out, along with a click outside.
    pub fn handle_keyboard(
        &mut self,
        handle: &KeysymHandle,
        pressed: bool,
        modifiers: ModifiersState,
    ) {
        self.egui.handle_keyboard(handle, pressed, modifiers);
    }

    /// State of the application list: `None` while the scan is still pending.
    pub(crate) fn apps_state(&self) -> Option<&Result<Vec<AppEntry>, String>> {
        self.apps.as_ref()
    }

    /// True until the scan has filled the list in.
    pub(crate) fn scanning(&self) -> bool {
        self.apps.is_none()
    }

    pub(crate) fn app_count(&self) -> usize {
        self.apps
            .as_ref()
            .and_then(|ok| ok.as_ref().ok())
            .map(|apps| apps.len())
            .unwrap_or(0)
    }

    pub(crate) fn app_at(&self, index: usize) -> Option<&AppEntry> {
        self.apps.as_ref()?.as_ref().ok()?.get(index)
    }

    /// The application pinned at dock slot `index`.
    pub(crate) fn pin_at_index(&self, index: usize) -> Option<&AppEntry> {
        self.entry(&self.pins.get(index)?.clone())
    }

    /// Rasterized icon for an application, decoded once per file and pixel size.
    pub(crate) fn icon_for(
        &mut self,
        ctx: &egui::Context,
        app: &AppEntry,
        px: usize,
    ) -> Option<TextureId> {
        let name = app.icon.as_ref()?;
        let file = self.icon_file(name)?;
        self.icon_texture(ctx, name, &file, px)
    }

    /// Run the deferred scan, then drop pins whose application is gone: a tile that cannot start
    /// anything is a dead control, so it does not get drawn.
    pub(crate) fn run_scan(&mut self) {
        if !self.scan_pending {
            return;
        }
        self.scan_pending = false;
        self.apps = Some(crate::apps::applications());
        let before = self.pins.len();
        let kept: Vec<String> = self
            .pins
            .iter()
            .filter(|id| self.entry(id).is_some())
            .cloned()
            .collect();
        self.pins = kept;
        if self.pins.len() != before {
            warn!(
                "Dropped {} pinned application(s) that are no longer installed",
                before - self.pins.len()
            );
            self.save_pins();
        }
    }

    pub(crate) fn entry(&self, id: &str) -> Option<&AppEntry> {
        self.apps
            .as_ref()?
            .as_ref()
            .ok()
            .and_then(|apps| apps.iter().find(|app| app.id == id))
    }

    pub(crate) fn pin_index(&self, id: &str) -> Option<usize> {
        self.pins.iter().position(|pin| pin == id)
    }

    pub(crate) fn is_pinned(&self, id: &str) -> bool {
        self.pin_index(id).is_some()
    }

    pub(crate) fn toggle_pin(&mut self, id: &str) {
        match self.pin_index(id) {
            Some(index) => {
                self.pins.remove(index);
            }
            None => self.pins.push(id.to_string()),
        }
        self.save_pins();
    }

    pub(crate) fn pin_at(&mut self, id: &str, index: usize) {
        if let Some(existing) = self.pin_index(id) {
            self.pins.remove(existing);
        }
        let index = index.min(self.pins.len());
        self.pins.insert(index, id.to_string());
        self.save_pins();
    }

    pub(crate) fn unpin(&mut self, index: usize) {
        if index < self.pins.len() {
            self.pins.remove(index);
            self.save_pins();
        }
    }

    pub(crate) fn move_pin(&mut self, from: usize, to: usize) {
        if from >= self.pins.len() {
            return;
        }
        let id = self.pins.remove(from);
        let to = to.min(self.pins.len());
        self.pins.insert(to, id);
        self.save_pins();
    }

    fn save_pins(&mut self) {
        write_pins(&self.pins_path, &self.pins);
    }

    pub(crate) fn pins(&self) -> &[String] {
        &self.pins
    }

    /// Total tiles on the bar: pinned first, then the running applications that are not pinned.
    pub(crate) fn tile_count(&self) -> usize {
        self.pins.len() + self.transient.len()
    }

    /// Desktop entry id (or app id) held by tile `index`.
    pub(crate) fn tile_id(&self, index: usize) -> Option<String> {
        if index < self.pins.len() {
            self.pins.get(index).cloned()
        } else {
            self.transient.get(index - self.pins.len()).cloned()
        }
    }

    pub(crate) fn tile_pinned(&self, index: usize) -> bool {
        index < self.pins.len()
    }

    pub(crate) fn set_transient(&mut self, transient: Vec<String>) {
        self.transient = transient;
    }

    pub(crate) fn transient(&self) -> &[String] {
        &self.transient
    }

    pub(crate) fn set_visible(&mut self, visible: Vec<usize>, columns: usize) {
        self.columns = columns.max(1);
        if visible != self.visible {
            self.visible = visible;
        }
        self.cursor = self.cursor.min(self.visible.len().saturating_sub(1));
    }

    pub(crate) fn visible(&self) -> &[usize] {
        &self.visible
    }

    pub(crate) fn columns(&self) -> usize {
        self.columns.max(1)
    }

    pub(crate) fn cursor(&self) -> usize {
        self.cursor
    }

    pub(crate) fn set_cursor(&mut self, cursor: usize) {
        self.cursor = cursor.min(self.visible.len().saturating_sub(1));
    }

    pub(crate) fn move_cursor(&mut self, delta: isize) {
        let len = self.visible.len() as isize;
        if len == 0 {
            self.cursor = 0;
            return;
        }
        self.cursor = (self.cursor as isize + delta).rem_euclid(len) as usize;
    }

    pub(crate) fn drag(&self) -> Option<Drag> {
        self.drag
    }

    pub(crate) fn set_drag(&mut self, drag: Option<Drag>) {
        self.drag = drag;
        if drag.is_none() {
            self.insert_at = None;
        }
    }

    pub(crate) fn insert_at(&self) -> Option<usize> {
        self.insert_at
    }

    pub(crate) fn set_insert_at(&mut self, at: Option<usize>) {
        self.insert_at = at;
    }

    pub(crate) fn layout(&self) -> &Layout {
        &self.layout
    }

    pub(crate) fn set_layout(&mut self, layout: Layout) {
        self.layout = layout;
    }

    pub(crate) fn take_focus_search(&mut self) -> bool {
        std::mem::take(&mut self.focus_search)
    }

    /// Where an icon name lives on disk. Looked up once per name at a fixed reference size: the
    /// theme directory decides the file, the tile size only decides how it is rasterized.
    pub(crate) fn icon_file(&mut self, icon: &str) -> Option<IconFile> {
        if let Some(found) = self.icon_files.get(icon) {
            return found.clone();
        }
        let found = crate::apps::find_icon(icon, ICON_LOOKUP_PX);
        if found.is_none() {
            debug!("No icon theme entry for \"{icon}\"");
        }
        self.icon_files.insert(icon.to_string(), found.clone());
        found
    }

    #[allow(deprecated)]
    pub(crate) fn icon_texture(
        &mut self,
        ctx: &egui::Context,
        name: &str,
        file: &IconFile,
        px: usize,
    ) -> Option<TextureId> {
        let key = format!(
            "{}@{px}",
            match file {
                IconFile::Png(path) | IconFile::Svg(path) => path.display().to_string(),
            }
        );
        if !self.icons.contains_key(&key) {
            match crate::apps::load_icon(file, px) {
                Ok(image) => {
                    self.icons.insert(key.clone(), image);
                }
                Err(err) => {
                    debug!("Ignoring icon {key}: {err}");
                    // Do not retry a file that failed to decode on every frame.
                    self.icon_files.insert(name.to_string(), None);
                    return None;
                }
            }
        }
        self.icons.get(&key).map(|image| image.texture_id(ctx))
    }

    /// Base icon edge, from the configured dock height.
    pub(crate) fn icon_size(&self) -> f32 {
        icon_size()
    }

    /// Edge of the space a tile always occupies, magnified or not. Keeping slots fixed stops the
    /// panel from sliding the next icon into the cursor while a tile grows.
    pub(crate) fn slot_size(&self) -> f32 {
        slot_size()
    }

    pub(crate) fn panel_height(&self) -> f32 {
        panel_height()
    }

    pub(crate) fn rounding(&self) -> Rounding {
        Rounding::same(PANEL_RADIUS.min(self.panel_height() * 0.5))
    }

    /// Grid tile: icon above the application name.
    pub(crate) fn tile_size(&self) -> Vec2 {
        Vec2::new(
            (self.icon_size() + 24.0).max(96.0),
            self.icon_size() + 24.0 + LABEL_H,
        )
    }

    /// How much the tile centred on `center_x` is magnified, given the pointer.
    pub(crate) fn magnification(&self, center_x: f32, pointer: Option<egui::Pos2>) -> f32 {
        // While dragging, the bar holds still: a moving insertion marker over moving icons is
        // impossible to aim at.
        if self.drag.is_some() {
            return 1.0;
        }
        let Some(pointer) = pointer else {
            return 1.0;
        };
        let influence = self.icon_size() * MAGNET_WIDTH;
        let falloff = (1.0 - (pointer.x - center_x).abs() / influence).max(0.0);
        1.0 + (HOVER_BOOST - 1.0) * falloff * falloff
    }

    /// Slot index the dragged icon would be inserted at, given the pointer.
    pub(crate) fn insert_index(&self, pointer_x: f32) -> usize {
        self.layout
            .slots
            .iter()
            .position(|slot| pointer_x < slot.center().x)
            .unwrap_or(self.pins.len())
    }
}

/// Accept either a desktop entry id (`firefox.desktop`) or a bare program name (`firefox`) in
/// the config, so the shipped default works without anyone checking `/usr/share/applications`.
fn seed_to_id(entry: &str) -> String {
    if entry.ends_with(".desktop") {
        entry.to_string()
    } else {
        format!("{entry}.desktop")
    }
}

/// Where the pin list lives. Kept out of `config.ron` so pinning an app can never reformat or
/// clobber a hand-edited configuration file.
pub fn pins_path() -> PathBuf {
    xdg::BaseDirectories::new()
        .ok()
        .and_then(|base| base.place_config_file("ferrite/dock.ron").ok())
        .unwrap_or_else(|| PathBuf::from("ferrite-dock.ron"))
}

fn read_pins(path: &PathBuf) -> Vec<String> {
    match fs::read_to_string(path) {
        Ok(contents) => ron::from_str::<DockPins>(&contents)
            .map(|pins| pins.apps)
            .map_err(|err| warn!("{}: ignoring bad dock pin file: {err}", path.display()))
            .unwrap_or_default(),
        Err(err) if path.exists() => {
            warn!("Failed to read {}: {err}", path.display());
            Vec::new()
        }
        Err(_) => Vec::new(),
    }
}

fn write_pins(path: &PathBuf, apps: &[String]) {
    if let Some(parent) = path.parent() {
        if let Err(err) = fs::create_dir_all(parent) {
            warn!("Failed to create {}: {err}", parent.display());
            return;
        }
    }
    let pretty = ron::ser::PrettyConfig::new().compact_arrays(true);
    match ron::ser::to_string_pretty(
        &DockPins {
            apps: apps.to_vec(),
        },
        pretty,
    ) {
        Ok(ron) => {
            if let Err(err) = fs::write(path, ron) {
                warn!("Failed to write {}: {err}", path.display());
            } else {
                debug!("Wrote {} dock pins to {}", apps.len(), path.display());
            }
        }
        Err(err) => warn!("Failed to serialize dock pins: {err}"),
    }
}

fn visuals() -> egui::Visuals {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = SURFACE;
    visuals.window_fill = SURFACE;
    visuals.extreme_bg_color = BASE;
    visuals.override_text_color = Some(TEXT);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, HAIRLINE);
    visuals.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    visuals.widgets.inactive.bg_fill = SURFACE_HI;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, HAIRLINE);
    visuals.widgets.hovered.weak_bg_fill = HOVER_BG;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    visuals.widgets.active.weak_bg_fill = SURFACE_HI;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    visuals.selection.bg_fill = ACCENT.gamma_multiply(0.6);
    visuals
}

/// True when one of the open windows belongs to this application.
pub fn is_running(app: &AppEntry, running: &[String]) -> bool {
    running.iter().any(|id| app.matches_app_id(id))
}

/// Base icon edge, from the configured dock height.
fn icon_size() -> f32 {
    (CONFIG.dock_height.max(40) as f32 - PANEL_PAD * 2.0 - INDICATOR_H).clamp(16.0, 128.0)
}

/// Edge of the space a tile always occupies, magnified or not.
fn slot_size() -> f32 {
    icon_size() * HOVER_BOOST + TILE_GAP
}

/// Height of the dock panel.
fn panel_height() -> f32 {
    icon_size() * HOVER_BOOST + PANEL_PAD * 2.0 + INDICATOR_H
}

/// Height the dock takes off the bottom of an output, so tiled windows stop above it.
pub fn reserved_height() -> i32 {
    if CONFIG.dock {
        (panel_height() + EDGE_MARGIN * 2.0).ceil() as i32
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apps::AppEntry;
    use egui::{Area, Id, Order, Pos2, Sense};

    fn test_dir() -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!(
            "ferrite-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::create_dir_all(&p);
        p
    }

    fn test_app(id: &str, name: &str) -> AppEntry {
        AppEntry {
            id: id.to_string(),
            name: name.to_string(),
            exec: format!("/usr/bin/{id}"),
            icon: None,
            wm_class: None,
        }
    }

    fn make_dock(apps: Vec<AppEntry>, pins: Vec<&str>) -> DockUi {
        let dir = test_dir();
        let path = dir.join("dock.ron");
        let mut dock = DockUi {
            egui: EguiState::new(Rectangle::from_loc_and_size((0, 0), (1, 1))),
            pins: pins.into_iter().map(|s| s.to_string()).collect(),
            transient: Vec::new(),
            pins_path: path,
            apps: Some(Ok(apps)),
            scan_pending: false,
            grid_open: false,
            query: String::new(),
            cursor: 0,
            columns: GRID_COLUMNS,
            visible: Vec::new(),
            drag: None,
            insert_at: None,
            layout: Layout::default(),
            requests: Vec::new(),
            focus_search: false,
            icon_files: std::collections::HashMap::new(),
            icons: std::collections::HashMap::new(),
        };
        dock
    }

    fn run_frame(dock: &mut DockUi, running: &[String], events: Vec<egui::Event>) {
        let ctx = dock.egui.context().clone();
        let mut input = egui::RawInput::default();
        input.screen_rect = Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 800.0)));
        input.events = events;
        ctx.run(input, |ctx| dock.draw_frame(ctx, running));
    }

    fn primary_press(pos: egui::Pos2) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::default(),
        }
    }
    fn primary_release(pos: egui::Pos2) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::default(),
        }
    }

    #[test]
    fn escape_closes_the_grid() {
        let mut dock = make_dock(vec![test_app("a.desktop", "Alpha")], vec![]);
        dock.set_grid_open(true);
        run_frame(
            &mut dock,
            &[],
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                pressed: true,
                modifiers: egui::Modifiers::default(),
                repeat: false,
            }],
        );
        assert!(!dock.grid_open(), "escape should close the grid");
    }

    #[test]
    fn enter_on_the_grid_requests_launch_and_closes() {
        let mut dock = make_dock(vec![test_app("a.desktop", "Alpha")], vec![]);
        dock.set_grid_open(true);
        run_frame(
            &mut dock,
            &[],
            vec![egui::Event::Key {
                key: egui::Key::Enter,
                pressed: true,
                modifiers: egui::Modifiers::default(),
                repeat: false,
            }],
        );
        assert!(!dock.grid_open());
        let reqs = dock.take_requests();
        assert_eq!(reqs, vec![DockRequest::Activate("a.desktop".to_string())]);
    }

    #[test]
    fn p_pins_the_cursor_tile() {
        let mut dock = make_dock(vec![test_app("a.desktop", "Alpha")], vec![]);
        dock.set_grid_open(true);
        // Move the cursor once so the search field gives up focus, then pin.
        run_frame(
            &mut dock,
            &[],
            vec![egui::Event::Key {
                key: egui::Key::ArrowRight,
                pressed: true,
                modifiers: egui::Modifiers::default(),
                repeat: false,
            }],
        );
        run_frame(
            &mut dock,
            &[],
            vec![egui::Event::Key {
                key: egui::Key::P,
                pressed: true,
                modifiers: egui::Modifiers::default(),
                repeat: false,
            }],
        );
        assert!(dock.is_pinned("a.desktop"));
    }

    #[test]
    fn query_filters_the_grid() {
        let mut dock = make_dock(
            vec![
                test_app("a.desktop", "Alpha"),
                test_app("b.desktop", "Beta"),
            ],
            vec![],
        );
        dock.set_grid_open(true);
        dock.query = "alp".to_string();
        run_frame(&mut dock, &[], vec![]);
        assert_eq!(dock.visible(), &[0usize]);
    }

    #[test]
    fn arrows_move_the_cursor() {
        let mut dock = make_dock(
            vec![
                test_app("a.desktop", "Alpha"),
                test_app("b.desktop", "Beta"),
            ],
            vec![],
        );
        dock.set_grid_open(true);
        run_frame(&mut dock, &[], vec![]);
        assert_eq!(dock.cursor(), 0);
        run_frame(
            &mut dock,
            &[],
            vec![egui::Event::Key {
                key: egui::Key::ArrowRight,
                pressed: true,
                modifiers: egui::Modifiers::default(),
                repeat: false,
            }],
        );
        assert_eq!(dock.cursor(), 1);
    }

    #[test]
    fn drag_from_grid_to_dock_pins() {
        let mut dock = make_dock(vec![test_app("a.desktop", "Alpha")], vec![]);
        let pins_path = dock.pins_path.clone();
        run_frame(&mut dock, &[], vec![]);
        dock.set_grid_open(true);
        // Compute the grid tile position by scanning visible apps first.
        run_frame(&mut dock, &[], vec![]);
        // Set the drag manually, as if a tile had been pressed and moved.
        dock.set_drag(Some(Drag::Grid(0)));
        let panel_c = dock.layout().panel.center();
        // A drag started on a tile: move onto the dock, then release.
        run_frame(
            &mut dock,
            &[],
            vec![egui::Event::PointerMoved(panel_c), primary_release(panel_c)],
        );
        assert!(
            dock.is_pinned("a.desktop"),
            "dragging onto the dock should pin"
        );
        assert!(dock.drag().is_none());
        // The pin file is written, so it survives a restart.
        let contents = std::fs::read_to_string(&pins_path).unwrap();
        assert!(contents.contains("a.desktop"));
    }

    #[test]
    fn drag_pin_out_of_dock_unpins() {
        let mut dock = make_dock(vec![test_app("a.desktop", "Alpha")], vec!["a.desktop"]);
        run_frame(&mut dock, &[], vec![]);
        dock.set_drag(Some(Drag::Dock(0)));
        // Release well outside the panel.
        let outside = egui::Pos2::new(0.0, 0.0);
        run_frame(
            &mut dock,
            &[],
            vec![egui::Event::PointerMoved(outside), primary_release(outside)],
        );
        assert!(!dock.is_pinned("a.desktop"));
    }

    #[test]
    fn drag_within_dock_reorders() {
        let mut dock = make_dock(
            vec![
                test_app("a.desktop", "Alpha"),
                test_app("b.desktop", "Beta"),
            ],
            vec!["a.desktop", "b.desktop"],
        );
        run_frame(&mut dock, &[], vec![]);
        dock.set_drag(Some(Drag::Dock(0)));
        // Release at the far right of the panel: the dragged slot moves to the end.
        let right = egui::Pos2::new(
            dock.layout().panel.right() - 5.0,
            dock.layout().panel.center().y,
        );
        run_frame(
            &mut dock,
            &[],
            vec![egui::Event::PointerMoved(right), primary_release(right)],
        );
        assert_eq!(dock.pins(), ["b.desktop", "a.desktop"]);
    }

    #[test]
    fn magnification_peaks_under_the_pointer() {
        let mut dock = make_dock(vec![], vec![]);
        run_frame(&mut dock, &[], vec![]);
        let slot_x = 320.0;
        let peak = dock.magnification(slot_x, Some(egui::Pos2::new(slot_x, 0.0)));
        let far = dock.magnification(slot_x, Some(egui::Pos2::new(slot_x + 1000.0, 0.0)));
        assert!(peak > far, "magnification should fall off with distance");
        assert!((peak - HOVER_BOOST).abs() < 0.01);
        assert_eq!(far, 1.0);
    }

    #[test]
    fn insert_index_tracks_the_pointer() {
        let mut dock = make_dock(
            vec![
                test_app("a.desktop", "Alpha"),
                test_app("b.desktop", "Beta"),
            ],
            vec!["a.desktop", "b.desktop"],
        );
        run_frame(&mut dock, &[], vec![]);
        // Far left lands before the first slot, far right after the last.
        let left = dock.layout().slots[0].left() - 10.0;
        let right = dock.layout().slots[1].right() + 10.0;
        assert_eq!(dock.insert_index(left), 0);
        assert_eq!(dock.insert_index(right), 2);
    }

    #[test]
    fn unresolvable_pins_are_dropped_after_the_scan() {
        // "ghost.desktop" does not exist in the fixed app list.
        let mut dock = make_dock(
            vec![test_app("a.desktop", "Alpha")],
            vec!["a.desktop", "ghost.desktop"],
        );
        // Mirror what run_scan does once the application list is known: drop pins whose
        // application is gone, so the dock never shows a tile that cannot start anything.
        let kept: Vec<String> = dock
            .pins()
            .iter()
            .filter(|id| dock.entry(id).is_some())
            .cloned()
            .collect();
        dock.pins = kept;
        dock.save_pins();
        assert_eq!(dock.pins(), ["a.desktop"]);
    }

    #[test]
    fn running_indicator_matches_app_ids() {
        let entry = test_app("firefox.desktop", "Firefox");
        assert!(crate::apps::AppEntry::matches_app_id(&entry, "firefox"));
        assert!(crate::apps::AppEntry::matches_app_id(&entry, "Firefox"));
        assert!(crate::apps::AppEntry::matches_app_id(
            &entry,
            "firefox.desktop"
        ));
        assert!(!crate::apps::AppEntry::matches_app_id(&entry, "konsole"));
    }
    #[test]
    fn running_apps_appear_as_transient_tiles() {
        let mut dock = make_dock(vec![test_app("firefox.desktop", "Firefox")], vec![]);
        assert_eq!(dock.tile_count(), 0);
        run_frame(&mut dock, &["firefox".to_string()], vec![]);
        assert_eq!(dock.tile_count(), 1, "a running unpinned app should get a tile");
        assert!(!dock.tile_pinned(0), "the running tile must not be pinned");
        assert!(crate::dock::is_running(dock.tile_id(0).as_ref().and_then(|id| dock.entry(id)).unwrap(), &["firefox".to_string()]));
    }

    #[test]
    fn running_pinned_app_does_not_duplicate() {
        let mut dock = make_dock(vec![test_app("firefox.desktop", "Firefox")], vec!["firefox.desktop"]);
        run_frame(&mut dock, &["firefox".to_string()], vec![]);
        assert_eq!(dock.tile_count(), 1, "pinned + running must not duplicate");
    }

}
