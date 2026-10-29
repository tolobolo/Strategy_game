//! parkour_game2: real-time overworld + turn-based tactical combat.
//! The design and the module map live in `agents.md`.

mod assets;
mod combat;
mod faction;
mod overworld;
mod shots;
mod states;
mod ui;
mod units;

use bevy::prelude::*;
use bevy::sprite::{SpritePickingMode, SpritePickingSettings};
use bevy::window::WindowResolution;

pub const WINDOW_WIDTH: u32 = 1600;
pub const WINDOW_HEIGHT: u32 = 900;

fn main() {
    if map_mode() {
        return;
    }
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
            shots::ShotsPlugin,
        ))
        .add_systems(Startup, spawn_camera)
        .add_systems(Update, fit_background)
        .run();
}

/// `cargo run -- map <seed> [--save]`: print the text stages of a world (the last with its
/// ramps) and its stats, optionally save the final grid as a seed file, and exit without
/// opening a window.
fn map_mode() -> bool {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) != Some("map") {
        return false;
    }
    let seed: u64 = match args.get(1).and_then(|s| s.parse().ok()) {
        Some(seed) => seed,
        None => {
            eprintln!("usage: cargo run -- map <seed> [--save]");
            return true;
        }
    };
    let (w, h) = (overworld::world::MAP_WIDTH, overworld::world::MAP_HEIGHT);
    let raw = overworld::worldgen::raw(seed, w, h);
    let warped = overworld::worldgen::warp(&raw, seed);
    let layered = overworld::worldgen::layers(&warped, seed);
    let smoothed = overworld::worldgen::smooth(&layered, seed);
    let map = overworld::terrain::from_grid_with_ramps(&smoothed, seed);
    let with_ramps = map.to_grid();
    println!("== raw (patches) ==\n{}", raw.to_text());
    println!("== warped ==\n{}", warped.to_text());
    println!("== layered ==\n{}", layered.to_text());
    println!("== smoothed ==\n{}", smoothed.to_text());
    println!("== ramps (R) ==\n{}", with_ramps.to_text());
    println!("stats: {} | ramps {} | room for bases {}", smoothed.stats(), map.ramps().count(), map.base_room());
    for (layer, tile, size) in map.unreached_regions() {
        println!("unreachable: layer {layer} plateau at {tile} ({size} tiles), no room for a ramp");
    }
    if args.iter().any(|a| a == "--save") {
        let path = overworld::terrain::seed_path(seed);
        let text = format!("# seed {seed}\n{}", with_ramps.to_text());
        match std::fs::create_dir_all(overworld::terrain::SEEDS_DIR).and_then(|_| std::fs::write(&path, text)) {
            Ok(()) => println!("saved {}", path.display()),
            Err(e) => eprintln!("could not save {}: {e}", path.display()),
        }
    }
    true
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
