use smithay::{
    output::Output,
    utils::{Logical, Point, Rectangle},
};

use crate::state::CONFIG;

use super::process;

/// Geometry of the dock on the given output, or `None` when the dock is disabled.
pub fn dock_geometry(output: &Output) -> Option<Rectangle<i32, Logical>> {
    if !CONFIG.dock {
        return None;
    }
    let mode = output.current_mode()?;
    let size = mode.size.to_logical(1);
    Some(Rectangle::from_loc_and_size(
        (0, size.h - CONFIG.dock_height),
        (size.w, CONFIG.dock_height),
    ))
}

/// Height the dock reserves at the bottom of the output, so tiled windows stay clear of it.
pub fn reserved_height(output: &Output) -> i32 {
    dock_geometry(output).map(|geo| geo.size.h).unwrap_or(0)
}

/// Index of the dock slot under `point`, if the point is inside the dock.
pub fn slot_at(output: &Output, point: Point<f64, Logical>) -> Option<usize> {
    let geo = dock_geometry(output)?.to_f64();
    if !geo.contains(point) {
        return None;
    }
    let count = CONFIG.dock_apps.len();
    if count == 0 {
        return None;
    }
    let slot_width = geo.size.w / count as f64;
    let index = ((point.x - geo.loc.x) / slot_width) as usize;
    Some(index.min(count - 1))
}

/// Launch the application bound to the dock slot at `index`.
pub fn launch(index: usize) {
    if let Some(app) = CONFIG.dock_apps.get(index) {
        process::spawn(app);
    }
}
