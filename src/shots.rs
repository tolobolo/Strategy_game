//! Dev tool: `SHOTS_DIR=<dir> WORLD_SEED=<seed> cargo run` photographs the overworld and quits.
//! One overview of the whole map, then a close-up of every ramp (up to `MAX_CLOSE_UPS`), saved
//! as `<dir>/seed<seed>_*.png`. Used to check the terrain by eye over many seeds.

use bevy::prelude::*;
use bevy::render::view::screenshot::{Capturing, Screenshot, save_to_disk};
use bevy::transform::TransformSystems;

use crate::overworld::world::WorldState;
use crate::states::GameState;

/// Frames to wait before the first shot, so every texture has loaded.
const WARM_UP_FRAMES: u32 = 90;
/// Frames to wait after moving the camera before taking the shot.
const SETTLE_FRAMES: u32 = 4;
const MAX_CLOSE_UPS: usize = 8;
/// Camera zoom of a close-up (below 1 is closer than in the game).
const CLOSE_UP_SCALE: f32 = 0.6;

pub struct ShotsPlugin;

impl Plugin for ShotsPlugin {
    fn build(&self, app: &mut App) {
        let Ok(dir) = std::env::var("SHOTS_DIR") else { return };
        app.insert_resource(Shots { dir, list: Vec::new(), next: 0, wait: WARM_UP_FRAMES })
            .add_systems(OnEnter(GameState::Overworld), plan_shots)
            .add_systems(
                PostUpdate,
                take_shots
                    .run_if(in_state(GameState::Overworld))
                    .before(TransformSystems::Propagate),
            );
    }
}

#[derive(Resource)]
struct Shots {
    dir: String,
    /// File name, camera centre, camera scale.
    list: Vec<(String, Vec2, f32)>,
    next: usize,
    wait: u32,
}

fn plan_shots(world: Res<WorldState>, windows: Query<&Window>, mut shots: ResMut<Shots>) {
    if !shots.list.is_empty() {
        return;
    }
    let seed = std::env::var("WORLD_SEED").unwrap_or_else(|_| "random".into());
    let map = &world.terrain;
    let window = windows.iter().next().map_or(Vec2::new(1600.0, 900.0), |w| w.size());
    let world_size = Vec2::new(map.width as f32, map.height as f32) * crate::overworld::terrain::TILE;
    let fit = (world_size / window).max_element();
    shots.list.push((format!("seed{seed}_0_overview.png"), Vec2::ZERO, fit));
    let mut ramps: Vec<IVec2> = map.ramps().map(|(r, _)| r).collect();
    ramps.sort_by_key(|t| (-t.y, t.x));
    for (i, ramp) in ramps.into_iter().take(MAX_CLOSE_UPS).enumerate() {
        let name = format!("seed{seed}_ramp{}_{}x{}.png", i + 1, ramp.x, ramp.y);
        shots.list.push((name, map.world_pos(ramp), CLOSE_UP_SCALE));
    }
    if let Err(e) = std::fs::create_dir_all(&shots.dir) {
        error!("cannot create {}: {e}", shots.dir);
    }
}

fn take_shots(
    mut commands: Commands,
    mut shots: ResMut<Shots>,
    capturing: Query<(), With<Capturing>>,
    mut camera: Single<(&mut Transform, &mut Projection), With<Camera2d>>,
    mut exit: MessageWriter<AppExit>,
) {
    if shots.list.is_empty() || !capturing.is_empty() {
        return;
    }
    if shots.next >= shots.list.len() {
        exit.write(AppExit::Success);
        return;
    }
    let (_, centre, scale) = shots.list[shots.next].clone();
    let (transform, projection) = &mut *camera;
    transform.translation.x = centre.x;
    transform.translation.y = centre.y;
    if let Projection::Orthographic(ortho) = &mut **projection {
        ortho.scale = scale;
    }
    if shots.wait > 0 {
        shots.wait -= 1;
        return;
    }
    let path = format!("{}/{}", shots.dir, shots.list[shots.next].0);
    commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
    shots.next += 1;
    shots.wait = SETTLE_FRAMES;
}
