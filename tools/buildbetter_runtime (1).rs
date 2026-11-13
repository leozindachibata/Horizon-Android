// Build Better's rules for Horizon, with nothing of the game in them: the placement
// mode, the shapes and the fill requests. Bundled in the Android build as
// `android_bundled::buildbetter`; buildbetter_native.rs applies them to the game.
//
// A port of Build Better by Malteas (MIT): two-corner shapes, a held preview and
// its keys. The code is this port's own.
use std::sync::Mutex;

pub const ID: &str = "buildbetter";
pub const VERSION: &str = "0.1.0";
pub const DESCRIPTION: &str = "Build Better for Horizon: mark two corners and fill a shape \
     (box, sphere, cylinder, cone, pyramid, dome, wedge) with the block in your hand. \
     Singleplayer, creative mode. After Build Better by Malteas.";

/// The mod's keys: (setting name, label in Options > Mods, default key).
pub const KEY_MODE: &str = "mode";
pub const KEY_SHAPE: &str = "shape";
pub const KEY_HOLD: &str = "hold";
pub const KEY_SET: &str = "set";
pub const KEY_CANCEL: &str = "cancel";
pub const DEFAULT_MODE: &str = "key.keyboard.b";
pub const DEFAULT_SHAPE: &str = "key.keyboard.k";
pub const DEFAULT_HOLD: &str = "key.keyboard.h";
pub const DEFAULT_SET: &str = "key.keyboard.v";
pub const DEFAULT_CANCEL: &str = "key.keyboard.x";

/// The most blocks one fill may hold (its box), and how many the preview outlines.
pub const MAX_VOLUME: i64 = 32768;
pub const MAX_PREVIEW: usize = 150;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Vanilla,
    Shapes,
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Mode::Vanilla => "Vanilla",
            Mode::Shapes => "Shapes",
        }
    }
    pub fn next(self) -> Mode {
        match self {
            Mode::Vanilla => Mode::Shapes,
            Mode::Shapes => Mode::Vanilla,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Cuboid,
    Hollow,
    Sphere,
    Cylinder,
    Cone,
    Pyramid,
    Dome,
    Wedge,
}

impl Shape {
    pub const ALL: [Shape; 8] = [
        Shape::Cuboid,
        Shape::Hollow,
        Shape::Sphere,
        Shape::Cylinder,
        Shape::Cone,
        Shape::Pyramid,
        Shape::Dome,
        Shape::Wedge,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Shape::Cuboid => "Box",
            Shape::Hollow => "Hollow box",
            Shape::Sphere => "Sphere",
            Shape::Cylinder => "Cylinder",
            Shape::Cone => "Cone",
            Shape::Pyramid => "Pyramid",
            Shape::Dome => "Dome",
            Shape::Wedge => "Wedge",
        }
    }
    pub fn next(self) -> Shape {
        let at = Shape::ALL.iter().position(|s| *s == self).unwrap_or(0);
        Shape::ALL[(at + 1) % Shape::ALL.len()]
    }
}

/// What the player has set up. Shared by the window's game and the server's.
#[derive(Clone, Copy, Debug)]
pub struct State {
    pub mode: Mode,
    pub shape: Shape,
    /// The first corner, once set.
    pub first: Option<[i32; 3]>,
    /// The preview is held in place: the second corner stays at `frozen`.
    pub hold: bool,
    pub frozen: Option<[i32; 3]>,
}

static STATE: Mutex<State> = Mutex::new(State {
    mode: Mode::Vanilla,
    shape: Shape::Cuboid,
    first: None,
    hold: false,
    frozen: None,
});

/// A copy of the state.
pub fn snapshot() -> State {
    match STATE.lock() {
        Ok(guard) => *guard,
        Err(poisoned) => *poisoned.into_inner(),
    }
}

/// Changes the state; None when the lock is gone.
pub fn with<R>(change: impl FnOnce(&mut State) -> R) -> Option<R> {
    STATE.lock().ok().map(|mut guard| change(&mut guard))
}

/// A fill asked for by the window, done by the server's level.
pub struct Request {
    pub dimension: String,
    pub item: String,
    pub blocks: Vec<[i32; 3]>,
}

static QUEUE: Mutex<Vec<Request>> = Mutex::new(Vec::new());

pub fn push(request: Request) {
    if let Ok(mut queue) = QUEUE.lock() {
        queue.push(request);
    }
}

/// Takes the requests for `dimension` out of the queue.
pub fn take(dimension: &str) -> Vec<Request> {
    let Ok(mut queue) = QUEUE.lock() else {
        return Vec::new();
    };
    if queue.is_empty() {
        return Vec::new();
    }
    let all = std::mem::take(&mut *queue);
    let mut mine = Vec::new();
    for request in all {
        if request.dimension == dimension {
            mine.push(request);
        } else {
            queue.push(request);
        }
    }
    mine
}

/// A key as the player knows it: `key.keyboard.b` is `B`.
pub fn label(key: &str) -> String {
    key.strip_prefix("key.keyboard.")
        .unwrap_or(key)
        .to_uppercase()
}

/// The box between two corners: its lowest and highest block.
pub fn bounds(a: [i32; 3], b: [i32; 3]) -> ([i32; 3], [i32; 3]) {
    (
        std::array::from_fn(|i| a[i].min(b[i])),
        std::array::from_fn(|i| a[i].max(b[i])),
    )
}

/// How many blocks the box between two corners holds.
pub fn volume(a: [i32; 3], b: [i32; 3]) -> i64 {
    let (lo, hi) = bounds(a, b);
    (0..3)
        .map(|i| i64::from(hi[i]) - i64::from(lo[i]) + 1)
        .product()
}

/// Whether the cell (`i`, `j`, `k`) of an `sx` x `sy` x `sz` box is in `shape`.
fn inside(shape: Shape, i: i32, j: i32, k: i32, sx: i32, sy: i32, sz: i32) -> bool {
    // Distance from the middle of each axis, 1.0 at the box's edge.
    let norm = |v: i32, size: i32| (f64::from(v) - f64::from(size - 1) / 2.0) / (f64::from(size) / 2.0);
    let dx = norm(i, sx);
    let dz = norm(k, sz);
    match shape {
        Shape::Cuboid => true,
        Shape::Hollow => {
            i == 0 || i == sx - 1 || j == 0 || j == sy - 1 || k == 0 || k == sz - 1
        }
        Shape::Sphere => {
            let dy = norm(j, sy);
            dx * dx + dy * dy + dz * dz <= 1.0
        }
        Shape::Cylinder => dx * dx + dz * dz <= 1.0,
        Shape::Cone => {
            // Widest at the bottom, a point at the top.
            let scale = 1.0 - f64::from(j) / f64::from(sy);
            dx * dx + dz * dz <= scale * scale
        }
        Shape::Pyramid => {
            let inset = (i64::from(j) * i64::from(sx.min(sz))) / (2 * i64::from(sy));
            let inset = inset as i32;
            i >= inset && i <= sx - 1 - inset && k >= inset && k <= sz - 1 - inset
        }
        Shape::Dome => {
            let dy = (f64::from(j) + 0.5) / f64::from(sy);
            dx * dx + dy * dy + dz * dz <= 1.0
        }
        Shape::Wedge => (i64::from(k) + 1) * i64::from(sy) > i64::from(j) * i64::from(sz),
    }
}

/// The blocks of `shape` fitted in the box between two corners. Empty when the box
/// holds more than [`MAX_VOLUME`].
pub fn shape_blocks(shape: Shape, a: [i32; 3], b: [i32; 3]) -> Vec<[i32; 3]> {
    if volume(a, b) > MAX_VOLUME {
        return Vec::new();
    }
    let (lo, hi) = bounds(a, b);
    let [sx, sy, sz] = std::array::from_fn(|n| hi[n] - lo[n] + 1);
    let mut out = Vec::new();
    for j in 0..sy {
        for i in 0..sx {
            for k in 0..sz {
                if inside(shape, i, j, k, sx, sy, sz) {
                    out.push([lo[0] + i, lo[1] + j, lo[2] + k]);
                }
            }
        }
    }
    out
}

/// At most [`MAX_PREVIEW`] of `blocks`, spread over the whole list.
pub fn sample(blocks: &[[i32; 3]]) -> Vec<[i32; 3]> {
    if blocks.len() <= MAX_PREVIEW {
        return blocks.to_vec();
    }
    let step = blocks.len().div_ceil(MAX_PREVIEW);
    blocks.iter().step_by(step).copied().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn box_fills_its_volume() {
        assert_eq!(shape_blocks(Shape::Cuboid, [0, 0, 0], [2, 2, 2]).len(), 27);
        assert_eq!(shape_blocks(Shape::Hollow, [0, 0, 0], [2, 2, 2]).len(), 26);
    }

    #[test]
    fn too_big_is_empty() {
        assert!(shape_blocks(Shape::Cuboid, [0, 0, 0], [99, 99, 99]).is_empty());
    }

    #[test]
    fn shapes_fit_their_box() {
        for shape in Shape::ALL {
            let blocks = shape_blocks(shape, [3, 3, 3], [9, 8, 7]);
            assert!(!blocks.is_empty());
            assert!(blocks.iter().all(|p| (3..=9).contains(&p[0])
                && (3..=8).contains(&p[1])
                && (3..=7).contains(&p[2])));
        }
    }
}
