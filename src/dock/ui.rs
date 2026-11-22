//! Drawing and hit-testing for the dock and the application grid.
//!
//! Everything here runs inside one egui frame, in this order: the dimmed backdrop, the grid, the
//! dock bar, and the dragged icon on top. Layout is computed by hand rather than left to egui's
//! layouts because magnification and the drop marker need the exact rect of every slot in the
//! same frame the pointer moves.

use egui::{
    vec2, Align2, Area, Color32, Context, FontId, Frame, Id, Key, Margin, Order, Pos2, Rect,
    Rounding, ScrollArea, Sense, Shape, Stroke, TextEdit, Ui, Vec2,
};

use crate::{
    apps::AppEntry,
    dock::{
        DockRequest, DockUi, Drag, Layout, ACCENT, BASE, DIVIDER_W, EDGE_MARGIN, GRID_COLUMNS,
        HAIRLINE, HOVER_BG, INDICATOR_H, LABEL_H, PANEL_PAD, SURFACE, SURFACE_HI, TEXT, TEXT_DIM,
        TILE_GAP, TILE_RADIUS,
    },
};

fn dock_id() -> Id {
    Id::new("ferrite-dock")
}
fn grid_id() -> Id {
    Id::new("ferrite-dock-grid")
}
fn backdrop_id() -> Id {
    Id::new("ferrite-dock-backdrop")
}
fn preview_id() -> Id {
    Id::new("ferrite-dock-preview")
}
fn search_id() -> Id {
    Id::new("ferrite-dock-search")
}

/// Draw one frame. `running` holds the app ids reported by the open windows.
pub(crate) fn draw(ctx: &Context, dock: &mut DockUi, running: &[String]) {
    // Deferred `.desktop` scan; the frame before this one drew the loading state.
    dock.run_scan();

    let (screen, pointer) = ctx.input(|i| (i.screen_rect(), i.pointer.interact_pos()));

    let layout = compute_layout(dock, screen);
    dock.set_layout(layout.clone());

    if dock.grid_open() {
        draw_backdrop(ctx, dock, screen);
        draw_grid(ctx, dock, &screen, pointer);
    }
    draw_dock(ctx, dock, &layout, pointer, running);
    finish_drag(ctx, dock, &layout, pointer);
    draw_preview(ctx, dock, pointer);
    handle_keys(ctx, dock);
}

/// Panel, one slot per pin, and the launcher tile, centred on the bottom edge.
fn compute_layout(dock: &DockUi, screen: Rect) -> Layout {
    let slot = dock.slot_size();
    let pins = dock.pins().len();
    // With nothing pinned the panel keeps one labelled empty slot, so the bar does not collapse
    // to a single button the user cannot read anything from.
    let empty_w = if pins == 0 { slot + TILE_GAP } else { 0.0 };
    let width = PANEL_PAD * 2.0 + pins as f32 * slot + empty_w + DIVIDER_W + slot + TILE_GAP * 0.0;
    let height = dock.panel_height();
    let left = (screen.center().x - width * 0.5).max(screen.left() + EDGE_MARGIN);
    let top = screen.bottom() - EDGE_MARGIN - height;
    let panel = Rect::from_min_size(Pos2::new(left, top), vec2(width, height));

    let mut slots = Vec::with_capacity(pins);
    let mut x = panel.left() + PANEL_PAD + empty_w;
    for _ in 0..pins {
        slots.push(Rect::from_min_size(
            Pos2::new(x, panel.top()),
            vec2(slot, height),
        ));
        x += slot;
    }
    let launcher = Rect::from_min_size(
        Pos2::new(panel.right() - PANEL_PAD - slot, panel.top()),
        vec2(slot, height),
    );
    Layout {
        panel,
        slots,
        launcher,
    }
}

/// Full-screen click-catcher behind the grid. Dismissing a modal panel by clicking away from it
/// is the expected behaviour, so the backdrop is a control, not decoration.
fn draw_backdrop(ctx: &Context, dock: &mut DockUi, screen: Rect) {
    Area::new(backdrop_id())
        .order(Order::Background)
        .interactable(true)
        .fixed_pos(Pos2::ZERO)
        .show(ctx, |ui| {
            ui.painter()
                .rect_filled(screen, Rounding::ZERO, Color32::from_black_alpha(110));
            if ui
                .interact(screen, backdrop_id().with("click"), Sense::click())
                .clicked()
            {
                dock.set_grid_open(false);
            }
        });
}

fn draw_grid(ctx: &Context, dock: &mut DockUi, screen: &Rect, pointer: Option<Pos2>) {
    let panel = dock.layout().panel;
    // Hang the grid above the dock, growing upward as the list gets longer.
    let offset = vec2(
        0.0,
        -(screen.bottom() - panel.top() + TILE_GAP * 2.0).max(EDGE_MARGIN),
    );

    Area::new(grid_id())
        .anchor(Align2::CENTER_BOTTOM, offset)
        .movable(false)
        .show(ctx, |ui| {
            Frame::none()
                .fill(SURFACE.gamma_multiply(0.97))
                .rounding(Rounding::same(16.0))
                .stroke(Stroke::new(1.0_f32, HAIRLINE))
                .inner_margin(Margin::same(20.0))
                .show(ui, |ui| {
                    ui.set_width(grid_width(dock));
                    header(ui, dock);
                    search(ui, dock);
                    tiles(ui, dock, pointer);
                    footer(ui);
                });
        });
}

fn grid_width(dock: &DockUi) -> f32 {
    let tile = dock.tile_size().x;
    PANEL_PAD * 2.0 + tile * GRID_COLUMNS as f32 + TILE_GAP * (GRID_COLUMNS as f32 - 1.0)
}

/// The mockup captions this panel with a tracked uppercase label. It is the theme's voice and the
/// only text at that size, which keeps the hierarchy in one place.
fn header(ui: &mut Ui, dock: &DockUi) {
    let shown = dock.visible().len();
    let total = dock.app_count();
    let title = match (total, shown) {
        (0, _) => "APPLICATIONS".to_string(),
        (_, 0) => "NO MATCHES".to_string(),
        (total, shown) => format!("APPLICATIONS  {shown}/{total}"),
    };
    ui.label(egui::RichText::new(title).color(ACCENT).size(11.0));
    ui.add_space(10.0);
}

fn search(ui: &mut Ui, dock: &mut DockUi) {
    let width = ui.available_width();
    let response = ui.add_sized(
        [width, 30.0],
        TextEdit::singleline(&mut dock.query)
            .id(search_id())
            .hint_text("Search applications")
            .font(FontId::proportional(13.0))
            .desired_width(width - 24.0),
    );
    if dock.take_focus_search() {
        response.request_focus();
    }
    if response.changed() {
        // Editing the filter restarts the keyboard cursor at the first result.
        dock.set_cursor(0);
    }
    ui.add_space(12.0);
}

fn tiles(ui: &mut Ui, dock: &mut DockUi, pointer: Option<Pos2>) {
    if dock.scanning() {
        ui.label(
            egui::RichText::new("Loading applications...")
                .color(TEXT_DIM)
                .size(13.0),
        );
        ui.add_space(48.0);
        return;
    }
    if let Some(Err(err)) = dock.apps_state() {
        ui.label(egui::RichText::new(err).color(TEXT_DIM).size(13.0));
        ui.add_space(48.0);
        return;
    }

    let query = dock.query.to_lowercase();
    let visible: Vec<usize> = (0..dock.app_count())
        .filter(|i| {
            query.is_empty()
                || dock
                    .app_at(*i)
                    .map(|app| app.name.to_lowercase().contains(&query))
                    .unwrap_or(false)
        })
        .collect();
    dock.set_visible(visible, GRID_COLUMNS);

    if dock.visible().is_empty() {
        ui.label(
            egui::RichText::new(if query.is_empty() {
                "No applications are installed."
            } else {
                "No application matches that name."
            })
            .color(TEXT_DIM)
            .size(13.0),
        );
        ui.add_space(48.0);
        return;
    }

    let tile = dock.tile_size();
    ScrollArea::vertical()
        .auto_shrink([true, true])
        .max_height(320.0)
        .show(ui, |ui| {
            let visible = dock.visible().to_vec();
            for row in visible.chunks(GRID_COLUMNS) {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(TILE_GAP, TILE_GAP);
                    for index in row.iter().copied() {
                        grid_tile(ui, dock, index, tile, pointer);
                    }
                });
                ui.add_space(TILE_GAP);
            }
        });
}

fn grid_tile(ui: &mut Ui, dock: &mut DockUi, index: usize, tile: Vec2, pointer: Option<Pos2>) {
    let (rect, response) = ui.allocate_exact_size(tile, Sense::click_and_drag());
    let Some(app) = dock.app_at(index).cloned() else {
        return;
    };
    let selected = dock.cursor() == index;
    if response.hovered() && dock.drag().is_none() {
        dock.set_cursor(index);
    }
    if selected {
        ui.painter()
            .rect_filled(rect, Rounding::same(TILE_RADIUS), HOVER_BG);
        // One accent for every "this is the live one" signal: focus ring and running dot match.
        ui.painter().rect_stroke(
            rect,
            Rounding::same(TILE_RADIUS),
            Stroke::new(1.5_f32, ACCENT),
        );
    } else if response.hovered() {
        ui.painter()
            .rect_filled(rect, Rounding::same(TILE_RADIUS), HOVER_BG);
    }

    let icon_size = dock.icon_size();
    let icon = Rect::from_center_size(
        Pos2::new(rect.center().x, rect.top() + icon_size * 0.5 + 4.0),
        vec2(icon_size, icon_size),
    );
    icon_or_placeholder(ui, dock, &app, icon);

    ui.painter().text(
        Pos2::new(rect.center().x, rect.bottom() - LABEL_H + 2.0),
        Align2::CENTER_TOP,
        shorten(&app.name, 18),
        FontId::proportional(11.5),
        TEXT,
    );

    if dock.is_pinned(&app.id) {
        ui.painter()
            .circle_filled(Pos2::new(rect.right() - 8.0, rect.top() + 8.0), 3.5, ACCENT);
    }

    if response.clicked() {
        dock.push_request(DockRequest::Activate(app.id.clone()));
        dock.set_grid_open(false);
    }
    if response.drag_started() {
        dock.set_drag(Some(Drag::Grid(index)));
    }
    let _ = pointer;
}

fn footer(ui: &mut Ui) {
    ui.add_space(12.0);
    ui.label(
        egui::RichText::new(
            "type to filter   arrows to move   enter to launch   p to pin   esc to close",
        )
        .color(TEXT_DIM)
        .size(10.5),
    );
}

fn draw_dock(
    ctx: &Context,
    dock: &mut DockUi,
    layout: &Layout,
    pointer: Option<Pos2>,
    running: &[String],
) {
    Area::new(dock_id())
        .order(Order::Middle)
        .fixed_pos(Pos2::ZERO)
        .movable(false)
        .show(ctx, |ui| {
            ui.painter()
                .rect_filled(layout.panel, dock.rounding(), SURFACE.gamma_multiply(0.94));
            ui.painter().rect_stroke(
                layout.panel,
                dock.rounding(),
                Stroke::new(1.0_f32, HAIRLINE),
            );
            // Top highlight from the mockup's `inset 0 1px 0 rgba(255,255,255,0.12)`: it is what
            // makes the panel read as raised rather than as a flat dark bar.
            let inset = dock.rounding().nw * 0.6 + 6.0;
            ui.painter().line_segment(
                [
                    Pos2::new(layout.panel.left() + inset, layout.panel.top() + 1.0),
                    Pos2::new(layout.panel.right() - inset, layout.panel.top() + 1.0),
                ],
                Stroke::new(1.0_f32, Color32::from_white_alpha(28)),
            );

            // Divider between the pinned applications and the launcher.
            let divider_x = layout.launcher.left() - DIVIDER_W * 0.5;
            ui.painter().line_segment(
                [
                    Pos2::new(divider_x, layout.panel.center().y - 20.0),
                    Pos2::new(divider_x, layout.panel.center().y + 20.0),
                ],
                Stroke::new(1.0_f32, HAIRLINE),
            );

            if dock.pins().is_empty() {
                empty_slot(ui, dock, layout);
            }

            for (index, slot) in layout.slots.clone().iter().enumerate() {
                dock_slot(ui, dock, layout, *slot, index, pointer, running);
            }

            if let Some(at) = dock.insert_at() {
                // Insertion marker: where the dragged application lands if it is dropped now.
                let x = match (at, layout.slots.first()) {
                    (0, Some(first)) => first.left() - TILE_GAP * 0.5,
                    _ => layout
                        .slots
                        .get(at.saturating_sub(1))
                        .map(|prev| prev.right() + TILE_GAP * 0.5)
                        .unwrap_or(layout.launcher.left() - TILE_GAP * 0.5),
                };
                ui.painter().line_segment(
                    [
                        Pos2::new(x, layout.panel.top() + PANEL_PAD),
                        Pos2::new(x, layout.panel.bottom() - PANEL_PAD),
                    ],
                    Stroke::new(2.0_f32, ACCENT),
                );
            }

            launcher_tile(ui, dock, layout.launcher, pointer);
        });
}

/// The dock before anything is pinned: a labelled slot that says what belongs there.
fn empty_slot(ui: &mut Ui, dock: &DockUi, layout: &Layout) {
    let slot = dock.slot_size();
    let rect = Rect::from_min_size(
        Pos2::new(layout.panel.left() + PANEL_PAD, layout.panel.top()),
        vec2(slot, layout.panel.height()),
    );
    let inner = Rect::from_center_size(rect.center(), vec2(slot - 10.0, dock.icon_size()));
    ui.painter().rect_stroke(
        inner,
        Rounding::same(TILE_RADIUS),
        Stroke::new(1.0_f32, HAIRLINE),
    );
    ui.painter().text(
        Pos2::new(rect.center().x, rect.bottom() - 2.0),
        Align2::CENTER_BOTTOM,
        "drop here",
        FontId::proportional(9.5),
        TEXT_DIM,
    );
}

fn dock_slot(
    ui: &mut Ui,
    dock: &mut DockUi,
    layout: &Layout,
    slot: Rect,
    index: usize,
    pointer: Option<Pos2>,
    running: &[String],
) {
    let scale = dock.magnification(slot.center().x, pointer);
    let size = dock.icon_size() * scale;
    let floor = layout.panel.bottom() - PANEL_PAD - INDICATOR_H;
    // Icons sit on the floor of the panel and grow upwards, as on macOS. Centring them would lift
    // the row off the bar while a neighbour is hovered.
    let icon = Rect::from_min_max(
        Pos2::new(slot.center().x - size * 0.5, floor - size),
        Pos2::new(slot.center().x + size * 0.5, floor),
    );

    let Some(app) = dock.pin_at_index(index).cloned() else {
        return;
    };
    let response = ui.interact(slot, dock_id().with(index), Sense::click_and_drag());
    if response.hovered() && dock.drag().is_none() {
        ui.painter()
            .rect_filled(slot, Rounding::same(TILE_RADIUS), HOVER_BG);
    }
    icon_or_placeholder(ui, dock, &app, icon);

    if crate::dock::is_running(&app, running) {
        ui.painter().circle_filled(
            Pos2::new(slot.center().x, layout.panel.bottom() - INDICATOR_H * 0.5),
            3.0,
            ACCENT,
        );
    }

    if response.clicked() {
        dock.push_request(DockRequest::Activate(app.id.clone()));
    }
    if response.drag_started() {
        dock.set_drag(Some(Drag::Dock(index)));
    }
}

/// The launcher: the mockup's orange hexagon with a Launchpad-style 3x3 of dots, which reads as
/// "opens many things" without needing an icon file.
fn launcher_tile(ui: &mut Ui, dock: &mut DockUi, slot: Rect, pointer: Option<Pos2>) {
    let response = ui.interact(slot, dock_id().with("launcher"), Sense::click());
    let size = dock.icon_size() * dock.magnification(slot.center().x, pointer);
    let center = Pos2::new(
        slot.center().x,
        slot.bottom() - PANEL_PAD - INDICATOR_H - size * 0.5,
    );
    let radius = size * 0.5;
    if response.hovered() && dock.drag().is_none() {
        ui.painter()
            .circle_filled(center, radius * 1.12, Color32::from_white_alpha(14));
    }
    // Flat-top-to-pointy-top hexagon matching the mockup's clip-path polygon.
    let points: Vec<Pos2> = (0..6)
        .map(|i| {
            let angle = std::f32::consts::PI * (0.5 + i as f32 / 3.0);
            Pos2::new(
                center.x + radius * angle.cos(),
                center.y + radius * angle.sin(),
            )
        })
        .collect();
    ui.painter().add(Shape::convex_polygon(
        points,
        ACCENT,
        Stroke::new(1.0_f32, Color32::from_black_alpha(60)),
    ));
    let step = radius * 0.42;
    for dx in [-1.0, 0.0, 1.0] {
        for dy in [-1.0, 0.0, 1.0] {
            ui.painter().circle_filled(
                Pos2::new(center.x + dx * step, center.y + dy * step),
                radius * 0.11,
                BASE,
            );
        }
    }
    if dock.grid_open() {
        // Open state: a ring around the tile, so the panel it belongs to is obvious.
        ui.painter().rect_stroke(
            slot.expand(2.0),
            Rounding::same(TILE_RADIUS),
            Stroke::new(1.5_f32, TEXT),
        );
    }

    if response.clicked() {
        dock.toggle_grid();
    }
}

/// The icon following the pointer while an application is dragged.
fn draw_preview(ctx: &Context, dock: &mut DockUi, pointer: Option<Pos2>) {
    let Some(drag) = dock.drag() else {
        return;
    };
    let Some(pointer) = pointer else {
        return;
    };
    let app = match drag {
        Drag::Grid(index) => dock.app_at(index).cloned(),
        Drag::Dock(index) => dock.pin_at_index(index).cloned(),
    };
    let Some(app) = app else { return };

    let size = dock.icon_size() * 1.1;
    Area::new(preview_id())
        .order(Order::Foreground)
        .interactable(false)
        .fixed_pos(Pos2::ZERO)
        .show(ctx, |ui| {
            let rect = Rect::from_center_size(pointer, vec2(size, size));
            icon_or_placeholder(ui, dock, &app, rect);
            if matches!(drag, Drag::Dock(_)) && !dock.layout().panel.expand(24.0).contains(pointer)
            {
                ui.painter().text(
                    Pos2::new(rect.center().x, rect.top() - 6.0),
                    Align2::CENTER_BOTTOM,
                    "release to unpin",
                    FontId::proportional(10.5),
                    ACCENT,
                );
            }
        });
}

/// Apply the drop once the drag ends.
fn finish_drag(ctx: &Context, dock: &mut DockUi, layout: &Layout, pointer: Option<Pos2>) {
    let Some(drag) = dock.drag() else {
        return;
    };
    let over_dock = pointer
        .map(|p| layout.panel.expand(24.0).contains(p))
        .unwrap_or(false);
    dock.set_insert_at(over_dock.then(|| dock.insert_index(pointer.unwrap().x)));
    if !ctx.input(|i| i.pointer.any_released()) {
        return;
    }
    match drag {
        Drag::Grid(index) => {
            if over_dock {
                if let Some(app) = dock.app_at(index).cloned() {
                    let at = dock.insert_index(pointer.map(|p| p.x).unwrap_or(0.0));
                    dock.pin_at(&app.id, at);
                }
            }
        }
        Drag::Dock(from) => {
            if over_dock {
                let to = dock.insert_index(pointer.map(|p| p.x).unwrap_or(0.0));
                // The dragged slot is removed first, so slots after it shift one to the left.
                dock.move_pin(from, if to > from { to - 1 } else { to });
            } else {
                dock.unpin(from);
            }
        }
    }
    dock.set_drag(None);
}

/// Keyboard control of the grid. Arrows take focus away from the search field so the caret does
/// not jump while the cursor moves; typing again brings it back.
fn handle_keys(ctx: &Context, dock: &mut DockUi) {
    if !dock.grid_open() {
        return;
    }
    let columns = dock.columns() as isize;
    let (escape, arrow, enter, tab, pin) = ctx.input(|i| {
        (
            i.key_pressed(Key::Escape),
            [
                i.key_pressed(Key::ArrowLeft).then_some(-1),
                i.key_pressed(Key::ArrowRight).then_some(1),
                i.key_pressed(Key::ArrowUp).then_some(-columns),
                i.key_pressed(Key::ArrowDown).then_some(columns),
                i.key_pressed(Key::Tab).then_some(1),
            ]
            .into_iter()
            .flatten()
            .next(),
            i.key_pressed(Key::Enter),
            false,
            i.key_pressed(Key::P),
        )
    });
    let _ = tab;

    if escape {
        if !dock.query.is_empty() {
            dock.query.clear();
            dock.set_cursor(0);
        } else {
            dock.set_grid_open(false);
        }
        return;
    }
    if let Some(delta) = arrow {
        if ctx.memory(|m| m.has_focus(search_id())) {
            ctx.memory_mut(|m| m.surrender_focus(search_id()));
        }
        dock.move_cursor(delta);
        return;
    }
    let search_focused = ctx.memory(|m| m.has_focus(search_id()));
    if (enter || (pin && !search_focused)) && !dock.visible().is_empty() {
        let Some(app) = dock.app_at(dock.visible()[dock.cursor()]).cloned() else {
            return;
        };
        if pin {
            dock.toggle_pin(&app.id);
        } else {
            dock.push_request(DockRequest::Activate(app.id.clone()));
            dock.set_grid_open(false);
        }
    }
}

/// The icon for an application, or a lettered tile when no theme icon exists for it. A letter is
/// an honest stand-in; an invented glyph would claim to be the application's art.
fn icon_or_placeholder(ui: &mut Ui, dock: &mut DockUi, app: &AppEntry, rect: Rect) {
    if let Some(texture) = dock.icon_for(ui.ctx(), app, rect.height() as usize) {
        ui.painter().add(Shape::image(
            texture,
            rect,
            Rect::from_min_max(Pos2::ZERO, vec2(1.0, 1.0).to_pos2()),
            Color32::WHITE,
        ));
        return;
    }
    let letter = app
        .name
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "?".to_string());
    ui.painter()
        .rect_filled(rect, Rounding::same(TILE_RADIUS), SURFACE_HI);
    ui.painter().rect_stroke(
        rect,
        Rounding::same(TILE_RADIUS),
        Stroke::new(1.0_f32, HAIRLINE),
    );
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        letter,
        FontId::proportional(rect.height() * 0.45),
        TEXT,
    );
}

fn shorten(text: &str, max: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max {
        return text.to_string();
    }
    let mut shortened: String = chars[..max].iter().collect();
    shortened.push('…');
    shortened
}
