//! Dev tool: `SHOTS_DIR=<dir> WORLD_SEED=<seed> cargo run` photographs the overworld and quits.
//! One overview of the whole map, a close-up of every ramp (up to `MAX_CLOSE_UPS`) and one of the
//! nearest gold mine, forest and pasture, saved as `<dir>/seed<seed>_*.png`. Used to check the
//! terrain and the resource art by eye. For the site close-ups a worker is put to work at each
//! (and one tree is cut down), so the work animations and a stump show. The last shot puts the
//! hero at your House with some upgrades bought and the crafting panel open.

use bevy::prelude::*;
use bevy::render::view::screenshot::{Capturing, Screenshot, save_to_disk};
use bevy::transform::TransformSystems;

use crate::assets::GameAssets;
use crate::faction::Faction;
use crate::overworld::Treasuries;
use crate::overworld::buildings::BuildingKind;
use crate::overworld::craft_panel::{OpenCraft, open_now};
use crate::overworld::terrain::TILE;
use crate::overworld::resources::{ResourceKind, SiteKind, Worker, WorkerState};
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
        app.insert_resource(Shots { dir, list: Vec::new(), next: 0, wait: WARM_UP_FRAMES, house: None })
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
    /// Where the hero stands for the crafting shot; taken (set to None) once it is staged.
    house: Option<Vec2>,
}

fn plan_shots(mut world: ResMut<WorldState>, windows: Query<&Window>, mut shots: ResMut<Shots>) {
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
    let home = world.home_base().spawn_tile();
    for kind in [SiteKind::GoldMine, SiteKind::Forest, SiteKind::Pasture] {
        let nearest = (0..world.sites.len())
            .filter(|i| world.sites[*i].kind == kind)
            .min_by_key(|i| (world.sites[*i].tile - home).abs().element_sum());
        let Some(i) = nearest else { continue };
        let tile = world.sites[i].tile;
        let (pos, slot) = match kind {
            SiteKind::GoldMine => (world.terrain.world_pos(tile + IVec2::NEG_X), None),
            SiteKind::Forest => {
                world.sites[i].trees[0].wood_left = 0;
                world.sites[i].trees[0].regrow_in = 999.0;
                world.sites[i].trees[1].claimed = true;
                let tree = &world.sites[i].trees[1];
                let drop = [65.0, 73.0, 26.0, 24.0][tree.variant as usize];
                (world.terrain.world_pos(tree.tile) + Vec2::new(-34.0, 12.0 - drop), Some(1))
            }
            SiteKind::Pasture => {
                let sheep = &mut world.sites[i].sheep[0];
                sheep.claimed = true;
                (sheep.pos + Vec2::new(-30.0, 0.0), Some(0))
            }
        };
        world.workers.push(Worker {
            faction: Faction::Reclaimers,
            site: i,
            castle: 0,
            pos,
            path: Vec::new(),
            state: WorkerState::Working { left: 999.0, slot },
            recalled: false,
            alive: true,
        });
        let name = format!("seed{seed}_site_{}.png", kind.name().replace(' ', "_").to_lowercase());
        shots.list.push((name, world.terrain.world_pos(tile), CLOSE_UP_SCALE));
    }
    // A beaten AI army: its ghost flies to its castle gate and waits there.
    if let Some(i) = world.armies.iter().position(|a| a.alive)
        && let Some(base) = world.base_of(world.armies[i].faction)
    {
        let gate = world.terrain.world_pos(base.spawn_tile());
        world.armies[i].pos = gate + Vec2::new(-200.0, 120.0);
        world.armies[i].die();
        shots.list.push((format!("seed{seed}_ghost.png"), gate + Vec2::new(-60.0, 40.0), CLOSE_UP_SCALE));
    }
    // Last: the crafting panel at your House (hero moved there when the shot comes up).
    if let Some((kind, tile)) = world.home_base().buildings().find(|(k, _)| *k == BuildingKind::House) {
        let size = kind.size().as_vec2();
        let centre = world.terrain.world_pos(tile) + (size - Vec2::ONE) * TILE / 2.0;
        let feet = centre + Vec2::new(0.0, -size.y * TILE / 2.0 + 8.0);
        shots.list.push((format!("seed{seed}_crafting.png"), feet, 1.0));
        shots.house = Some(feet);
    }
    if let Err(e) = std::fs::create_dir_all(&shots.dir) {
        error!("cannot create {}: {e}", shots.dir);
    }
}

#[allow(clippy::too_many_arguments)]
fn take_shots(
    mut commands: Commands,
    mut shots: ResMut<Shots>,
    capturing: Query<(), With<Capturing>>,
    mut camera: Single<(&mut Transform, &mut Projection), With<Camera2d>>,
    mut hero: Query<&mut Transform, (With<crate::overworld::Hero>, Without<Camera2d>)>,
    mut world: ResMut<WorldState>,
    mut treasuries: ResMut<Treasuries>,
    mut craft: ResMut<OpenCraft>,
    assets: Res<GameAssets>,
    server: Res<AssetServer>,
    mut exit: MessageWriter<AppExit>,
) {
    if shots.list.is_empty() || !capturing.is_empty() {
        return;
    }
    if shots.next >= shots.list.len() {
        exit.write(AppExit::Success);
        return;
    }
    let (name, centre, scale) = shots.list[shots.next].clone();
    // The crafting shot: walk the hero to the House, fill the stores, buy a few upgrades, open it.
    if name.ends_with("_crafting.png")
        && let Some(feet) = shots.house.take()
    {
        for mut t in &mut hero {
            t.translation.x = feet.x;
            t.translation.y = feet.y - 10.0;
        }
        for kind in [ResourceKind::Wood, ResourceKind::Meat, ResourceKind::Gold] {
            treasuries.add_resource(Faction::Reclaimers, kind, 150);
        }
        for id in ["basic_training", "army_slots", "gather", "castle_slots_1"] {
            let up = crate::upgrades::book().get(id).unwrap();
            let _ = world.upgrades.buy(up, &mut treasuries);
        }
        let selected = crate::upgrades::book().upgrades.iter().position(|u| u.id == "pawn_speed").unwrap_or(0);
        open_now(&mut commands, &assets, &server, &mut craft, selected);
        shots.wait = 30;
    }
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
