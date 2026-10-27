//! parkour_game2: real-time overworld + turn-based tactical combat.
//! The design and the module map live in `agents.md`.

mod assets;
mod combat;
mod faction;
mod overworld;
mod states;
mod ui;
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
        .add_systems(Update, fit_background)
        .run();
}

/// The sea-and-clouds picture behind everything, in both states. It is a child of the camera,
/// so it follows the view and never leaves the screen.
#[derive(Component)]
struct Background;

/// One camera for both states. The overworld moves it, combat resets it to the grid.
fn spawn_camera(mut commands: Commands, server: Res<AssetServer>) {
    commands.spawn((
        Camera2d,
        children![(
            Background,
            Sprite::from_image(server.load("background.png")),
            // Far behind the map (z -1) and the grid (z 0); the camera sits at z 1000.
            Transform::from_xyz(0.0, 0.0, -999.0),
            Pickable::IGNORE,
        )],
    ));
}

/// Scale the background so it always covers the window, whatever its size.
fn fit_background(
    windows: Query<&Window>,
    images: Res<Assets<Image>>,
    mut background: Single<(&mut Transform, &Sprite), With<Background>>,
) {
    let Ok(window) = windows.single() else { return };
    let (transform, sprite) = &mut *background;
    let Some(image) = images.get(&sprite.image) else { return };
    let size = image.size_f32();
    let scale = (window.width() / size.x).max(window.height() / size.y);
    if (transform.scale.x - scale).abs() > 1e-4 {
        transform.scale = Vec3::new(scale, scale, 1.0);
    }
}
