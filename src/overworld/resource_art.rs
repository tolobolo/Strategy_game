//! Sprites for the work sites (gold mines, forests, sheep) and the worker Pawns. The state lives
//! in `WorldState` (`overworld/resources.rs`); these systems only mirror it. See agents.md §1.7.

use bevy::prelude::*;

use super::interact::{Touch, Touchable};
use super::resources::{PendingDeliveries, SiteKind, WorkerState, FOREST_RADIUS, PASTURE_RADIUS};
use super::world::WorldState;
use super::{OVERWORLD_UNIT_SCALE, UNIT_FEET, label};
use crate::assets::{Animated, GameAssets, UnitArt, WorkerAnim, WorkerVisual};
use crate::states::GameState;

/// Trees are drawn this far above their tile (as decorations are), so the trunk stands on it.
const TREE_OFFSET: f32 = 48.0;
/// Stumps sit lower in their 256 px frame than trees 3 and 4 do in theirs: per variant, the
/// offset that puts the stump where the trunk was (measured from the sheets).
const STUMP_OFFSET: [f32; 4] = [47.0, 44.0, 78.0, 76.0];
const STONE_SCALE: f32 = 0.7;
const SHEEP_SCALE: f32 = 0.7;
/// A delivery pop-up rises this far over this long, then disappears.
const POP_RISE: f32 = 36.0;
const POP_SECS: f32 = 1.0;

#[derive(Component)]
pub struct SiteLabel(pub usize);
#[derive(Component)]
pub struct ForestTree {
    pub site: usize,
    pub slot: usize,
}
#[derive(Component)]
pub struct ForestStump {
    pub site: usize,
    pub slot: usize,
}
#[derive(Component)]
pub struct MineSparkle(pub usize);
/// One sheep; `shown` = which sheet it plays (0 move, 1 graze, 2 stand).
#[derive(Component)]
pub struct SheepSprite {
    pub site: usize,
    pub index: usize,
    shown: u8,
}
/// The entity of worker `WorldState::workers[i]`; its child carries the `WorkerVisual`.
#[derive(Component)]
pub struct WorkerMarker(pub usize);
#[derive(Component)]
pub struct DeliveryPop {
    age: f32,
    from: f32,
}

/// Things lower on the screen are drawn on top (same rule as decorations).
fn depth(world: &WorldState, tile: IVec2) -> f32 {
    0.5 + (world.terrain.height - tile.y) as f32 * 0.0005
}

/// Every site: a touchable root with its label, plus its rocks, trees or sheep.
pub fn spawn_sites(commands: &mut Commands, assets: &GameAssets, world: &WorldState) {
    for (i, site) in world.sites.iter().enumerate() {
        let pos = world.terrain.world_pos(site.tile);
        let (radius, label_y) = match site.kind {
            SiteKind::GoldMine => (40.0, 52.0),
            SiteKind::Forest => (FOREST_RADIUS as f32 * 64.0, 150.0),
            SiteKind::Pasture => (PASTURE_RADIUS as f32 * 64.0 * 0.8, 40.0),
        };
        commands.spawn((
            Transform::from_translation(pos.extend(1.0)),
            Visibility::default(),
            Touchable { touch: Touch::Site(i), radius },
            DespawnOnExit(GameState::Overworld),
            children![(SiteLabel(i), label(site_text(world, i), Color::WHITE, Vec2::new(0.0, label_y)))],
        ));
        match site.kind {
            SiteKind::GoldMine => {
                let v = site.stones as usize;
                let z = depth(world, site.tile);
                commands.spawn((
                    Sprite::from_image(assets.gold_stones[v].clone()),
                    Transform::from_translation(pos.extend(z)).with_scale(Vec3::splat(STONE_SCALE)),
                    Pickable::IGNORE,
                    DespawnOnExit(GameState::Overworld),
                ));
                let sparkle = &assets.gold_sparkles[v];
                commands.spawn((
                    MineSparkle(i),
                    sparkle.sprite(),
                    Animated::new(sparkle.frames, 6.0),
                    Transform::from_translation(pos.extend(z + 0.0001)).with_scale(Vec3::splat(STONE_SCALE)),
                    if site.is_empty() { Visibility::Hidden } else { Visibility::Inherited },
                    Pickable::IGNORE,
                    DespawnOnExit(GameState::Overworld),
                ));
            }
            SiteKind::Forest => {
                for (slot, tree) in site.trees.iter().enumerate() {
                    let at = world.terrain.world_pos(tree.tile);
                    let z = depth(world, tree.tile);
                    let v = tree.variant as usize;
                    let strip = &assets.trees[v];
                    let (tree_vis, stump_vis) =
                        if tree.standing() { (Visibility::Inherited, Visibility::Hidden) } else { (Visibility::Hidden, Visibility::Inherited) };
                    commands.spawn((
                        ForestTree { site: i, slot },
                        strip.sprite(),
                        Animated::new(strip.frames, 6.0),
                        Transform::from_translation((at + Vec2::new(0.0, TREE_OFFSET)).extend(z)),
                        tree_vis,
                        Pickable::IGNORE,
                        DespawnOnExit(GameState::Overworld),
                    ));
                    commands.spawn((
                        ForestStump { site: i, slot },
                        Sprite::from_image(assets.stumps[v].clone()),
                        Transform::from_translation((at + Vec2::new(0.0, STUMP_OFFSET[v])).extend(z)),
                        stump_vis,
                        Pickable::IGNORE,
                        DespawnOnExit(GameState::Overworld),
                    ));
                }
            }
            SiteKind::Pasture => {
                for (index, sheep) in site.sheep.iter().enumerate() {
                    let mut sprite = assets.sheep_idle.sprite();
                    sprite.flip_x = sheep.facing_left;
                    commands.spawn((
                        SheepSprite { site: i, index, shown: 2 },
                        sprite,
                        Animated::new(assets.sheep_idle.frames, 6.0),
                        Transform::from_translation(sheep.pos.extend(2.0)).with_scale(Vec3::splat(SHEEP_SCALE)),
                        if sheep.alive() { Visibility::Inherited } else { Visibility::Hidden },
                        Pickable::IGNORE,
                        DespawnOnExit(GameState::Overworld),
                    ));
                }
            }
        }
    }
}

fn site_text(world: &WorldState, i: usize) -> String {
    let site = &world.sites[i];
    let workers = world.workers_at(i);
    format!("{}\n{}  workers {}/{}", site.kind.name().to_uppercase(), site.status(), workers, world.max_workers(i))
}

fn set_visibility(visibility: &mut Visibility, show: bool) {
    let wanted = if show { Visibility::Inherited } else { Visibility::Hidden };
    if *visibility != wanted {
        *visibility = wanted;
    }
}

/// Labels, trees ↔ stumps, and the sparkle of mines that still hold gold.
#[allow(clippy::type_complexity)]
pub fn refresh_site_visuals(
    world: Res<WorldState>,
    mut labels: Query<(&SiteLabel, &mut Text2d)>,
    mut trees: Query<(&ForestTree, &mut Visibility), Without<ForestStump>>,
    mut stumps: Query<(&ForestStump, &mut Visibility), (Without<ForestTree>, Without<MineSparkle>)>,
    mut sparkles: Query<(&MineSparkle, &mut Visibility), (Without<ForestTree>, Without<ForestStump>)>,
) {
    if !world.is_changed() {
        return;
    }
    for (label, mut text) in &mut labels {
        let value = site_text(&world, label.0);
        if text.0 != value {
            text.0 = value;
        }
    }
    for (tree, mut visibility) in &mut trees {
        set_visibility(&mut visibility, world.sites[tree.site].trees[tree.slot].standing());
    }
    for (stump, mut visibility) in &mut stumps {
        set_visibility(&mut visibility, !world.sites[stump.site].trees[stump.slot].standing());
    }
    for (sparkle, mut visibility) in &mut sparkles {
        set_visibility(&mut visibility, !world.sites[sparkle.0].is_empty());
    }
}

/// Sheep follow their state: walking (`sheep_move`, facing the way they go), grazing
/// (`sheep_grass`) or standing still while a worker is busy with them (`sheep_idle`); hidden while gone.
pub fn sync_sheep(
    world: Res<WorldState>,
    assets: Res<GameAssets>,
    mut sheep: Query<(&mut SheepSprite, &mut Transform, &mut Sprite, &mut Animated, &mut Visibility)>,
) {
    for (mut marker, mut transform, mut sprite, mut animated, mut visibility) in &mut sheep {
        let state = &world.sites[marker.site].sheep[marker.index];
        set_visibility(&mut visibility, state.alive());
        if !state.alive() {
            continue;
        }
        transform.translation.x = state.pos.x;
        transform.translation.y = state.pos.y;
        if sprite.flip_x != state.facing_left {
            sprite.flip_x = state.facing_left;
        }
        let wanted = if state.claimed {
            2
        } else if state.path.is_empty() {
            1
        } else {
            0
        };
        if marker.shown != wanted {
            marker.shown = wanted;
            let (strip, fps) = match wanted {
                0 => (&assets.sheep_move, 8.0),
                1 => (&assets.sheep_grass, 8.0),
                _ => (&assets.sheep_idle, 6.0),
            };
            sprite.image = strip.image.clone();
            sprite.texture_atlas = Some(TextureAtlas { layout: strip.layout.clone(), index: 0 });
            *animated = Animated::new(strip.frames, fps);
        }
    }
}

/// The sheet for what a worker is doing (agents.md §1.7 has the table).
fn worker_anim(world: &WorldState, i: usize) -> WorkerAnim {
    let worker = &world.workers[i];
    let site = &world.sites[worker.site];
    let tool = site.kind.tool();
    match worker.state {
        WorkerState::ToSite | WorkerState::Approach(_) if !worker.path.is_empty() => WorkerAnim::Walk(tool),
        WorkerState::ToSite | WorkerState::Approach(_) | WorkerState::Waiting(_) => WorkerAnim::Wait(tool),
        WorkerState::Working { .. } => WorkerAnim::Work(tool),
        WorkerState::ToCastle(_) => WorkerAnim::Carry(site.kind.resource()),
        WorkerState::GoingHome => WorkerAnim::Home,
    }
}

/// Spawn an entity for every worker that has none, despawn those whose worker is gone, and move
/// and animate the rest. Workers face the way they walk, or the thing they work on.
pub fn sync_workers(
    mut commands: Commands,
    mut art: UnitArt,
    world: Res<WorldState>,
    mut entities: Query<(Entity, &WorkerMarker, &mut Transform, &Children)>,
    mut visuals: Query<&mut WorkerVisual>,
) {
    let mut seen = vec![false; world.workers.len()];
    for (entity, marker, mut transform, children) in &mut entities {
        let alive = world.workers.get(marker.0).is_some_and(|w| w.alive);
        if !alive || seen[marker.0] {
            commands.entity(entity).despawn();
            continue;
        }
        seen[marker.0] = true;
        let worker = &world.workers[marker.0];
        let before = transform.translation.truncate();
        transform.translation.x = worker.pos.x;
        transform.translation.y = worker.pos.y;
        let anim = worker_anim(&world, marker.0);
        let site = &world.sites[worker.site];
        let dx = match worker.work_target(site, &world.terrain) {
            Some(target) => target.x - worker.pos.x,
            None => worker.pos.x - before.x,
        };
        for child in children.iter() {
            if let Ok(mut visual) = visuals.get_mut(child) {
                let flip = if dx.abs() > 0.01 { dx < 0.0 } else { visual.flip_x };
                if visual.anim != anim || visual.flip_x != flip {
                    visual.anim = anim;
                    visual.flip_x = flip;
                }
            }
        }
    }
    for (i, worker) in world.workers.iter().enumerate() {
        if worker.alive && !seen[i] {
            let visual = WorkerVisual { faction: worker.faction, anim: worker_anim(&world, i), flip_x: false };
            commands.spawn((
                WorkerMarker(i),
                Transform::from_translation(worker.pos.extend(3.0)),
                Visibility::default(),
                DespawnOnExit(GameState::Overworld),
                children![art.worker_bundle(visual, OVERWORLD_UNIT_SCALE, UNIT_FEET, 0.1)],
            ));
        }
    }
}

/// A delivered load pops up at the castle gate: the wood / gold / meat picture rising and fading.
pub fn spawn_delivery_pops(mut commands: Commands, assets: Res<GameAssets>, mut pending: ResMut<PendingDeliveries>) {
    for delivery in pending.0.drain(..) {
        let size = if delivery.kind == super::resources::ResourceKind::Gold { 0.3 } else { 0.6 };
        commands.spawn((
            DeliveryPop { age: 0.0, from: delivery.pos.y + 30.0 },
            Sprite::from_image(assets.resource_pops[&delivery.kind].clone()),
            Transform::from_translation((delivery.pos + Vec2::new(0.0, 30.0)).extend(6.0)).with_scale(Vec3::splat(size)),
            Pickable::IGNORE,
            DespawnOnExit(GameState::Overworld),
        ));
    }
}

pub fn animate_delivery_pops(
    mut commands: Commands,
    time: Res<Time>,
    mut pops: Query<(Entity, &mut DeliveryPop, &mut Transform, &mut Sprite)>,
) {
    for (entity, mut pop, mut transform, mut sprite) in &mut pops {
        pop.age += time.delta_secs();
        let t = (pop.age / POP_SECS).min(1.0);
        transform.translation.y = pop.from + POP_RISE * t;
        sprite.color = Color::srgba(1.0, 1.0, 1.0, 1.0 - t * t);
        if pop.age >= POP_SECS {
            commands.entity(entity).despawn();
        }
    }
}
