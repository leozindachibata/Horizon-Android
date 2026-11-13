// Build Better's adapter to the game: what the rules in buildbetter_runtime.rs decide,
// done in the game. The window's game reads the keys, shows the HUD and the preview,
// and queues a fill; the server's level makes the blocks (single player runs on the
// server thread). Bundled in the Android build, wired in by android_bundled.rs.
use super::buildbetter::{self as bb, Mode};
use crate::game::Game;
use crate::game::native_hooks::NativeMod;
use horizon_client::command::args::GameMode;
use horizon_client::extras::Corner;
use horizon_client::extras_hud::Corners;
use horizon_client::gizmos::Line;
use horizon_client::hud::HudBatch;
use horizon_client::text::Text;
use horizon_world::level::flags;

const FIRST: u32 = 0xFFFF_FF55;
const SECOND: u32 = 0xFF55_FF55;
const SHAPE: u32 = 0xFF55_FFFF;
const FRAME: u32 = 0xFFE5_E5E5;

impl Game<'_> {
    /// A line in the action bar.
    fn bb_say(&mut self, text: &str) {
        let line = Text::literal(text.to_owned());
        self.titles.set_overlay(line, false);
    }

    /// The air block next to the one aimed at: where a corner goes.
    fn bb_target(&self) -> Option<[i32; 3]> {
        let hit = self.pick()?;
        Some([
            hit.pos[0] + hit.face[0],
            hit.pos[1] + hit.face[1],
            hit.pos[2] + hit.face[2],
        ])
    }

    /// A key went down or up (`native_key`): Build Better's keys, while playing.
    fn bb_key(&mut self, key: &str, pressed: bool) {
        if !pressed || !self.native_window() || !self.native_on(bb::ID) || !self.native_playing() {
            return;
        }
        let mode = self.native_mod_key(bb::ID, bb::KEY_MODE, bb::DEFAULT_MODE);
        let shape = self.native_mod_key(bb::ID, bb::KEY_SHAPE, bb::DEFAULT_SHAPE);
        let hold = self.native_mod_key(bb::ID, bb::KEY_HOLD, bb::DEFAULT_HOLD);
        let set = self.native_mod_key(bb::ID, bb::KEY_SET, bb::DEFAULT_SET);
        let cancel = self.native_mod_key(bb::ID, bb::KEY_CANCEL, bb::DEFAULT_CANCEL);
        if key == mode {
            self.bb_cycle_mode();
        } else if key == shape {
            self.bb_cycle_shape();
        } else if key == hold {
            self.bb_hold();
        } else if key == set {
            self.bb_set();
        } else if key == cancel {
            self.bb_cancel();
        }
    }

    fn bb_cycle_mode(&mut self) {
        let now = bb::with(|s| {
            s.mode = s.mode.next();
            s.first = None;
            s.hold = false;
            s.frozen = None;
            s.mode
        })
        .unwrap_or(Mode::Vanilla);
        self.bb_say(&format!("Build Better: {} mode", now.name()));
    }

    fn bb_cycle_shape(&mut self) {
        let shape = bb::with(|s| {
            s.shape = s.shape.next();
            s.shape
        });
        if let Some(shape) = shape {
            self.bb_say(&format!("Shape: {}", shape.name()));
        }
    }

    fn bb_cancel(&mut self) {
        let _ = bb::with(|s| {
            s.first = None;
            s.hold = false;
            s.frozen = None;
        });
        self.bb_say("Corners cleared");
    }

    /// Holds the preview where it is, or lets it follow the crosshair again.
    fn bb_hold(&mut self) {
        let target = self.bb_target();
        let said = bb::with(|s| {
            if s.mode != Mode::Shapes || s.first.is_none() {
                "Set the first corner before holding the preview"
            } else if s.hold {
                s.hold = false;
                s.frozen = None;
                "Preview released"
            } else if let Some(at) = target {
                s.hold = true;
                s.frozen = Some(at);
                "Preview held in place"
            } else {
                "Aim at a block"
            }
        });
        if let Some(said) = said {
            self.bb_say(said);
        }
    }

    /// The set key: the first corner, then the second, which fills.
    fn bb_set(&mut self) {
        let state = bb::snapshot();
        if state.mode != Mode::Shapes {
            self.bb_say("Press the mode key to switch to Shapes mode");
            return;
        }
        let Some(target) = self.bb_target() else {
            self.bb_say("Aim at a block");
            return;
        };
        let Some(first) = state.first else {
            let _ = bb::with(|s| s.first = Some(target));
            self.bb_say("First corner set - aim at the second and press set again");
            return;
        };
        let second = if state.hold {
            state.frozen.unwrap_or(target)
        } else {
            target
        };
        if !(self.creative || self.mode == GameMode::Creative) {
            self.bb_say("Build Better 0.1 fills in creative mode only");
            return;
        }
        let Some((item, _, _)) = self.native_held() else {
            self.bb_say("Hold a block to fill");
            return;
        };
        if self.pack.assets.registry.default_state(&item).is_none() {
            self.bb_say("Hold a block to fill");
            return;
        }
        let blocks = bb::shape_blocks(state.shape, first, second);
        if blocks.is_empty() {
            self.bb_say("Volume too large");
            return;
        }
        let Some(player) = self.native_player() else {
            return;
        };
        let count = blocks.len();
        bb::push(bb::Request {
            dimension: player.dimension.to_string(),
            item,
            blocks,
        });
        let _ = bb::with(|s| {
            s.first = None;
            s.hold = false;
            s.frozen = None;
        });
        self.bb_say(&format!("Filling up to {count} blocks"));
    }

    /// After a level's tick (`native_level_tick_post`): makes the fills asked for in
    /// this level, in its air only.
    fn bb_level_tick(&mut self, dimension: &str) {
        if self.remote.is_some() {
            return;
        }
        for request in bb::take(dimension) {
            let Some(state) = self.pack.assets.registry.default_state(&request.item) else {
                continue;
            };
            let (min_y, max_y) = {
                let layout = self.world.layout();
                (layout.min_y(), layout.max_y())
            };
            for pos in &request.blocks {
                if pos[1] < min_y || pos[1] >= max_y || !self.world.loaded(pos[0], pos[2]) {
                    continue;
                }
                let here = self.world.block(pos[0], pos[1], pos[2]);
                if !self.world.is_air(here) {
                    continue;
                }
                let _ = self.native_set_block(dimension, *pos, state, flags::UPDATE_ALL);
            }
        }
    }

    /// The HUD (`native_hud`): the mode, the shape and what to do next.
    fn bb_hud(&mut self, batch: &mut HudBatch, corners: &mut Corners) {
        if !self.native_window() || !self.native_on(bb::ID) || !self.native_corner_shown(Corner::TopLeft) {
            return;
        }
        let state = bb::snapshot();
        if state.mode == Mode::Vanilla {
            return;
        }
        let set = bb::label(&self.native_mod_key(bb::ID, bb::KEY_SET, bb::DEFAULT_SET));
        let mode_key = bb::label(&self.native_mod_key(bb::ID, bb::KEY_MODE, bb::DEFAULT_MODE));
        let shape_key = bb::label(&self.native_mod_key(bb::ID, bb::KEY_SHAPE, bb::DEFAULT_SHAPE));
        let hold_key = bb::label(&self.native_mod_key(bb::ID, bb::KEY_HOLD, bb::DEFAULT_HOLD));
        let mut lines = vec![
            format!("[{mode_key}] Build Better: {}", state.mode.name()),
            format!("[{shape_key}] Shape: {}", state.shape.name()),
        ];
        match state.first {
            None => lines.push(format!("[{set}] Set first corner")),
            Some(first) => {
                if let Some(second) = self.bb_second(&state) {
                    let volume = bb::volume(first, second);
                    if volume > bb::MAX_VOLUME {
                        lines.push("Volume too large".to_owned());
                    } else {
                        let count = bb::shape_blocks(state.shape, first, second).len();
                        lines.push(format!("{count} blocks"));
                    }
                }
                lines.push(format!("[{set}] Fill"));
                if state.hold {
                    lines.push(format!("[{hold_key}] Release preview"));
                } else {
                    lines.push(format!("[{hold_key}] Hold preview"));
                }
            }
        }
        let widest = lines
            .iter()
            .map(|l| self.native_text_width(l))
            .fold(0.0_f32, f32::max);
        let w = widest + 8.0;
        let h = lines.len() as f32 * 10.0 + 4.0;
        let (x, y) = corners.place(Corner::TopLeft, w, h);
        self.native_fill(batch, [x, y, w, h], 0x7800_0000);
        for (n, line) in lines.iter().enumerate() {
            self.native_text(batch, line, [x + 4.0, y + 3.0 + n as f32 * 10.0], 0x00FF_FFFF, true);
        }
    }

    /// The second corner now: the held one, or the block aimed at.
    fn bb_second(&self, state: &bb::State) -> Option<[i32; 3]> {
        if state.hold {
            if let Some(frozen) = state.frozen {
                return Some(frozen);
            }
        }
        self.bb_target()
    }

    /// The preview, as lines in the world (`native_frame_visuals`).
    fn bb_lines(&self, lines: &mut Vec<Line>) {
        if !self.native_window() || !self.native_on(bb::ID) {
            return;
        }
        let state = bb::snapshot();
        if state.mode != Mode::Shapes {
            return;
        }
        let Some(first) = state.first else {
            if let Some(target) = self.bb_target() {
                bb_cube(lines, target, FIRST, 2.5);
            }
            return;
        };
        let Some(second) = self.bb_second(&state) else {
            bb_cube(lines, first, FIRST, 2.5);
            return;
        };
        let blocks = bb::shape_blocks(state.shape, first, second);
        for block in bb::sample(&blocks) {
            bb_cube(lines, block, SHAPE, 1.5);
        }
        let (lo, hi) = bb::bounds(first, second);
        bb_box(
            lines,
            lo.map(f64::from),
            hi.map(|v| f64::from(v) + 1.0),
            FRAME,
            2.0,
        );
        bb_cube(lines, first, FIRST, 3.0);
        bb_cube(lines, second, SECOND, 3.0);
    }
}

/// The outline of block `at`.
fn bb_cube(lines: &mut Vec<Line>, at: [i32; 3], color: u32, width: f32) {
    bb_box(
        lines,
        at.map(f64::from),
        at.map(|v| f64::from(v) + 1.0),
        color,
        width,
    );
}

/// The twelve edges of the box from `lo` to `hi`, a little outside so they show.
fn bb_box(lines: &mut Vec<Line>, lo: [f64; 3], hi: [f64; 3], color: u32, width: f32) {
    let grow = 0.002;
    let corner = |n: usize| -> [f64; 3] {
        std::array::from_fn(|axis| {
            if n >> axis & 1 == 0 {
                lo[axis] - grow
            } else {
                hi[axis] + grow
            }
        })
    };
    for n in 0..8_usize {
        for axis in 0..3 {
            if n >> axis & 1 == 0 {
                lines.push(Line {
                    start: corner(n),
                    end: corner(n | 1 << axis),
                    color,
                    width,
                });
            }
        }
    }
}

/// A key went down or up.
pub(super) fn on_key(game: &mut Game<'_>, key: &str, pressed: bool) {
    game.bb_key(key, pressed);
}

/// The row in Options > Mods, with its keys.
pub(super) fn list(mods: &mut Vec<NativeMod>) {
    mods.push(NativeMod {
        id: bb::ID.into(),
        title: format!("Build Better {}", bb::VERSION),
        description: bb::DESCRIPTION.into(),
        keys: vec![
            [bb::KEY_MODE.into(), "Cycle placement mode".into(), bb::DEFAULT_MODE.into()],
            [bb::KEY_SHAPE.into(), "Cycle shape".into(), bb::DEFAULT_SHAPE.into()],
            [bb::KEY_HOLD.into(), "Hold preview in place".into(), bb::DEFAULT_HOLD.into()],
            [bb::KEY_SET.into(), "Set corner / fill".into(), bb::DEFAULT_SET.into()],
            [bb::KEY_CANCEL.into(), "Clear corners".into(), bb::DEFAULT_CANCEL.into()],
        ],
    });
}

/// The box with the mode, shape and next step.
pub(super) fn hud(game: &mut Game<'_>, batch: &mut HudBatch, corners: &mut Corners) {
    game.bb_hud(batch, corners);
}

/// The preview's lines in the world.
pub(super) fn preview(game: &Game<'_>, lines: &mut Vec<Line>) {
    game.bb_lines(lines);
}

/// After a level's tick: the server's level makes the fills asked for.
pub(super) fn level_tick(game: &mut Game<'_>, dimension: &str) {
    game.bb_level_tick(dimension);
}
