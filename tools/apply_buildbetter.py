#!/usr/bin/env python3
"""Puts Build Better into the Horizon Android source: copies the two Rust files and
makes the small edits that wire them in. Run from the repository root. Safe to run twice."""
import pathlib
import shutil
import sys

ROOT = pathlib.Path(".")
GAME = ROOT / "crates/horizon-client/src/game"
BUNDLED = GAME / "android_bundled"


def edit(path, old, new, marker):
    text = path.read_text(encoding="utf-8")
    if marker in text:
        print(f"already done: {path.name} ({marker})")
        return
    if text.count(old) != 1:
        sys.exit(f"ERROR: in {path} the anchor was found {text.count(old)} times, expected 1:\n{old}")
    path.write_text(text.replace(old, new, 1), encoding="utf-8")
    print(f"edited: {path.name} ({marker})")


# 1. The two new files.
for name in ("buildbetter_runtime.rs", "buildbetter_native.rs"):
    source = ROOT / "tools" / name
    if not source.exists():
        sys.exit(f"ERROR: tools/{name} is missing: upload it to the tools folder first")
    shutil.copyfile(source, BUNDLED / name)
    print(f"copied: {name}")

ab = GAME / "android_bundled.rs"

# 2a. The modules.
edit(
    ab,
    "use std::time::{Duration, Instant};\n",
    "use std::time::{Duration, Instant};\n\n"
    '#[cfg(target_os = "android")]\n'
    '#[path = "android_bundled/buildbetter_runtime.rs"]\n'
    "pub(super) mod buildbetter;\n\n"
    '#[cfg(target_os = "android")]\n'
    '#[path = "android_bundled/buildbetter_native.rs"]\n'
    "mod buildbetter_native;\n",
    "mod buildbetter_native;",
)

# 2b. The keys.
edit(
    ab,
    "        zoom_key::run(&mut *game, key, pressed);\n",
    "        zoom_key::run(&mut *game, key, pressed);\n"
    "        buildbetter_native::on_key(&mut *game, key, pressed);\n",
    "buildbetter_native::on_key",
)

# 2c. The row in Options > Mods.
edit(
    ab,
    "        smartf3_list::run(&mut *mods);\n",
    "        smartf3_list::run(&mut *mods);\n        buildbetter_native::list(&mut *mods);\n",
    "buildbetter_native::list",
)

# 2d. The HUD.
edit(
    ab,
    "    corners: &mut horizon_client::extras_hud::Corners,\n) {\n    ensure_dynamic_mods_loaded(game, false);\n",
    "    corners: &mut horizon_client::extras_hud::Corners,\n) {\n"
    '    #[cfg(target_os = "android")]\n'
    "    {\n"
    "        buildbetter_native::hud(&mut *game, &mut *batch, &mut *corners);\n"
    "    }\n"
    "    ensure_dynamic_mods_loaded(game, false);\n",
    "buildbetter_native::hud",
)

# 2e. Two functions at the end.
text = ab.read_text(encoding="utf-8")
if "fn native_frame_lines" not in text:
    text = text.rstrip("\n") + '''

pub(super) fn native_frame_lines(
    game: &Game<'_>,
    lines: &mut Vec<horizon_client::gizmos::Line>,
) {
    let _ = (&game, &lines);
    #[cfg(target_os = "android")]
    {
        buildbetter_native::preview(game, lines);
    }
}

pub(super) fn native_level_tick_post(game: &mut Game<'_>, dimension: &str) {
    let _ = (&game, &dimension);
    #[cfg(target_os = "android")]
    {
        buildbetter_native::level_tick(game, dimension);
    }
}
'''
    ab.write_text(text, encoding="utf-8")
    print("edited: android_bundled.rs (two functions at the end)")

# 3. The level tick.
edit(
    GAME / "native_hooks.rs",
    "    pub(super) fn native_level_tick_post(&mut self, dimension: &str) {\n"
    "        mod_api::dispatch_level_tick_post(self, mod_api::mods(), dimension);\n",
    "    pub(super) fn native_level_tick_post(&mut self, dimension: &str) {\n"
    "        super::android_bundled::native_level_tick_post(self, dimension);\n"
    "        mod_api::dispatch_level_tick_post(self, mod_api::mods(), dimension);\n",
    "android_bundled::native_level_tick_post",
)

# 4. The preview.
edit(
    GAME / "native_visuals.rs",
    "    ) -> Vec<dynamic_light::Colored> {\n        let Some(player) = self.native_player() else {\n",
    "    ) -> Vec<dynamic_light::Colored> {\n"
    "        super::android_bundled::native_frame_lines(self, lines);\n"
    "        let Some(player) = self.native_player() else {\n",
    "android_bundled::native_frame_lines",
)
print("Build Better is in.")
