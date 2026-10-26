//! parkour_game2: real-time overworld + turn-based tactical combat.
//! The design and the module map live in `agents.md`.

mod assets;
mod combat;
mod faction;
mod overworld;
mod states;
mod units;

use bevy::prelude::*;
use bevy::sprite::{SpritePickingMode, SpritePickingSettings};
use bevy::window::WindowResolution;

pub const WINDOW_WIDTH: u32 = 1600;
pub const WINDOW_HEIGHT: u32 = 900;

fn main() {
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "parkour_game2".to_string(),
                        resolution: WindowResolution::new(WINDOW_WIDTH, WINDOW_HEIGHT),
                        ..default()
                    }),
                    ..default()
                })
                // Pixel art: no smoothing when sprites are scaled. Assets load from `assets/`.
                .set(ImagePlugin::default_nearest()),
        )
        // Placeholder units are plain coloured squares (no image), so pick by bounding box.
        .insert_resource(SpritePickingSettings {
            picking_mode: SpritePickingMode::BoundingBox,
            ..default()
        })
        .add_plugins((
            states::StatesPlugin,
            assets::GameAssetsPlugin,
            overworld::OverworldPlugin,
            combat::CombatPlugin,
        ))
        .add_systems(Startup, spawn_camera)
        .run();
}

/// One camera for both states. The overworld moves it, combat resets it to the grid.
fn spawn_camera(mut commands: Commands) {
    commands.spawn(Camera2d);
}
