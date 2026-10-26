//! Real-time exploration. See agents.md §1 and §4.
//!
//! The map lives in `WorldState` (a resource) so it survives trips into combat. Every time the
//! overworld state is entered, `spawn_overworld` rebuilds the entities from it.

pub mod armies;
pub mod buildings;
pub mod economy;
pub mod interact;
pub mod terrain;
pub mod world;

use bevy::prelude::*;

pub use economy::Treasuries;
pub use interact::Nearby;
pub use world::{PendingEncounter, WorldState};

use crate::assets::{Animated, GameAssets};
use crate::faction::Faction;
use crate::states::GameState;
use crate::units::{HERO_SLOTS, PlayerArmy, Team};
use buildings::BuildingKind;
use economy::{INCOME_INTERVAL_SECS, TOWER_INCOME};
use interact::{Touch, Touchable};
use terrain::{TILE, Terrain, TerrainMap};
use world::DecorationKind;

const HERO_SPEED: f32 = 260.0;
const HERO_SIZE: f32 = 40.0;
/// Sheet index (0-based) of the base-ground tiles: `tilemap_color5`.
const BASE_GROUND_SHEET: usize = 4;

#[derive(Component)]
pub struct Hero;

#[derive(Component)]
pub struct MoveSpeed(pub f32);

/// Elevation layer of a thing on the map: water 0, grass 1, mountain 2. The hero's layer follows
/// the tile it stands on and only changes on a staircase.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layer(pub u8);

#[derive(Component)]
struct OverworldHud;

#[derive(Component)]
struct LandUnitedBanner;

/// A building, tower or seller whose border colour and label follow `WorldState`.
#[derive(Component)]
struct BuildingVisual(Touch);

#[derive(Component)]
struct OwnerBorder;

#[derive(Component)]
struct BuildingLabel;

pub struct OverworldPlugin;

impl Plugin for OverworldPlugin {
    fn build(&self, app: &mut App) {
        // `WORLD_SEED=123 cargo run` reproduces a map; otherwise every start is a new one.
        let seed: u64 = std::env::var("WORLD_SEED").ok().and_then(|v| v.parse().ok()).unwrap_or_else(rand::random);
        info!("world seed {seed}");
        app.insert_resource(WorldState::generate(seed))
            .init_resource::<PlayerArmy>()
            .init_resource::<Nearby>()
            .add_plugins(economy::EconomyPlugin)
            .add_systems(OnEnter(GameState::Overworld), spawn_overworld)
            .add_systems(
                Update,
                (
                    hero_input,
                    camera_follow,
                    remember_hero_position,
                    interact::find_touch,
                    interact::tick_messages,
                    interact::castle_transfer,
                    interact::buy_units,
                    armies::ai_armies_act,
                    armies::bandits_wander,
                    armies::respawn_roamers,
                    armies::refresh_army_labels,
                    refresh_building_visuals,
                    interact::trigger_fights,
                    armies::roamer_encounters,
                    refresh_overworld_hud,
                )
                    .chain()
                    .run_if(in_state(GameState::Overworld)),
            );
    }
}

/// A `Text2d` floating at `offset` from its parent.
pub(super) fn label(text: impl Into<String>, color: Color, offset: Vec2) -> impl Bundle {
    (
        Text2d::new(text),
        TextFont { font_size: FontSize::Px(13.0), ..default() },
        TextColor(color),
        TextLayout::justify(Justify::Center),
        Transform::from_translation(offset.extend(0.2)),
        Pickable::IGNORE,
    )
}

// ---------- ground tiles ----------

/// Which cell of a 3×3 blob (plus its strips and single tile) to draw, given which
/// 4-neighbours are the same kind. Returns (col, row) inside the flat blob; add 5 to the
/// column for the raised blob.
fn blob_cell(n: bool, e: bool, s: bool, w: bool) -> (u32, u32) {
    match (n, e, s, w) {
        (true, true, true, true) => (1, 1),
        (false, true, true, true) => (1, 0),
        (true, true, false, true) => (1, 2),
        (true, true, true, false) => (0, 1),
        (true, false, true, true) => (2, 1),
        (false, true, true, false) => (0, 0),
        (false, false, true, true) => (2, 0),
        (true, true, false, false) => (0, 2),
        (true, false, false, true) => (2, 2),
        (true, false, true, false) => (3, 1),
        (false, false, true, false) => (3, 0),
        (true, false, false, false) => (3, 2),
        (false, true, false, true) => (1, 3),
        (false, true, false, false) => (0, 3),
        (false, false, false, true) => (2, 3),
        (false, false, false, false) => (3, 3),
    }
}

/// What a tile draws as, for the autotiler. Base ground and plateau are separate blobs.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Ground {
    Water,
    Grass,
    BaseGround,
    Mountain,
}

fn ground_of(world: &WorldState, tile: IVec2) -> Option<Ground> {
    Some(match world.terrain.get(tile)? {
        Terrain::Water => Ground::Water,
        Terrain::Mountain => Ground::Mountain,
        Terrain::Grass if world.in_any_base(tile) => Ground::BaseGround,
        Terrain::Grass => Ground::Grass,
    })
}

/// Tile sprite for the ground at `tile`, following the sheet rules in agents.md §1.6.
fn ground_sprite(assets: &GameAssets, world: &WorldState, tile: IVec2, ground: Ground) -> Sprite {
    if ground == Ground::Water {
        return Sprite::from_image(assets.water.clone());
    }
    let same = |d: IVec2| ground_of(world, tile + d) == Some(ground);
    let (col, row) = blob_cell(same(IVec2::Y), same(IVec2::X), same(IVec2::NEG_Y), same(IVec2::NEG_X));
    match ground {
        Ground::Grass => assets.tile(world.terrain.quarter(tile), col, row),
        Ground::BaseGround => assets.tile(BASE_GROUND_SHEET, col, row),
        Ground::Mountain => assets.tile(world.terrain.quarter(tile), col + 5, row),
        Ground::Water => unreachable!(),
    }
}

/// Cliff face drawn on the tile below a mountain edge: left end, middle, right end or lone.
fn cliff_col(map: &TerrainMap, mountain: IVec2) -> u32 {
    let rock = |d: IVec2| map.get(mountain + d) == Some(Terrain::Mountain);
    match (rock(IVec2::NEG_X), rock(IVec2::X)) {
        (true, true) => 6,
        (false, true) => 5,
        (true, false) => 7,
        (false, false) => 8,
    }
}

fn spawn_ground(commands: &mut Commands, assets: &GameAssets, world: &WorldState) {
    let map = &world.terrain;
    for (tile, terrain) in map.tiles() {
        let pos = map.world_pos(tile);
        let ground = ground_of(world, tile).unwrap();
        commands.spawn((
            ground_sprite(assets, world, tile, ground),
            Transform::from_translation(pos.extend(-1.0)),
            Pickable::IGNORE,
            DespawnOnExit(GameState::Overworld),
        ));

        // Grass below a mountain edge gets a cliff face, or the staircase that leads up.
        let above = tile + IVec2::Y;
        if terrain == Terrain::Grass && map.get(above) == Some(Terrain::Mountain) {
            let sheet = map.quarter(above);
            if map.is_stairs(tile) {
                commands.spawn((
                    assets.tile_rect(sheet, 0, 4, 1, 2),
                    Transform::from_translation((pos + Vec2::new(0.0, TILE / 2.0)).extend(-0.5)),
                    Pickable::IGNORE,
                    DespawnOnExit(GameState::Overworld),
                ));
            } else {
                commands.spawn((
                    assets.tile(sheet, cliff_col(map, above), 4),
                    Transform::from_translation(pos.extend(-0.9)),
                    Pickable::IGNORE,
                    DespawnOnExit(GameState::Overworld),
                ));
            }
        }
    }
}

fn spawn_decorations(commands: &mut Commands, assets: &GameAssets, world: &WorldState) {
    let map = &world.terrain;
    for deco in &world.decorations {
        let pos = map.world_pos(deco.tile);
        // Things lower on the screen are drawn on top, so a tree overlaps the one behind it.
        let z = 0.5 + (map.height - deco.tile.y) as f32 * 0.0005;
        let (sprite, animated, offset) = match deco.kind {
            DecorationKind::Tree(v) => {
                let strip = &assets.trees[v as usize];
                (strip.sprite(), Some(Animated::new(strip.frames, 6.0)), Vec2::new(0.0, 48.0))
            }
            DecorationKind::Bush(v) => {
                let strip = &assets.bushes[v as usize];
                (strip.sprite(), Some(Animated::new(strip.frames, 6.0)), Vec2::ZERO)
            }
            DecorationKind::Rock(v) => (Sprite::from_image(assets.rocks[v as usize].clone()), None, Vec2::ZERO),
            DecorationKind::WaterRock(v) => {
                let strip = &assets.water_rocks[v as usize];
                (strip.sprite(), Some(Animated::new(strip.frames, 8.0)), Vec2::ZERO)
            }
            DecorationKind::Duck => (assets.duck.sprite(), Some(Animated::new(assets.duck.frames, 4.0)), Vec2::ZERO),
        };
        let mut entity = commands.spawn((
            sprite,
            Transform::from_translation((pos + offset).extend(z)),
            Pickable::IGNORE,
            DespawnOnExit(GameState::Overworld),
        ));
        if let Some(animated) = animated {
            entity.insert(animated);
        }
    }
}

// ---------- buildings ----------

/// A coloured square with a thick border in the owner's colour and a label above it.
fn building_bundle(fill: Color, size: Vec2, pos: Vec2, touch: Touch, world: &WorldState) -> impl Bundle {
    let (owner, text) = building_text(world, touch);
    (
        Sprite::from_color(fill, size),
        Transform::from_translation(pos.extend(1.0)),
        Touchable { touch, radius: size.max_element() / 2.0 + HERO_SIZE * 0.7 },
        BuildingVisual(touch),
        DespawnOnExit(GameState::Overworld),
        children![
            (
                OwnerBorder,
                Sprite::from_color(owner.color(), size + Vec2::splat(10.0)),
                Transform::from_xyz(0.0, 0.0, -0.1),
                Pickable::IGNORE,
            ),
            (BuildingLabel, label(text, Color::WHITE, Vec2::new(0.0, size.y / 2.0 + 16.0))),
        ],
    )
}

/// Current owner and label of a touchable thing, from the world state.
fn building_text(world: &WorldState, touch: Touch) -> (Faction, String) {
    match touch {
        Touch::Castle(i) => {
            let base = &world.bases[i];
            (base.owner, format!("CASTLE of the {}\ngarrison: {}", base.owner.name(), base.garrison.summary()))
        }
        Touch::Shop { base, kind } => {
            let owner = world.bases[base].owner;
            (owner, format!("{} ({})\nsells {}", kind.name(), owner.name(), kind.sells().map_or("-", |u| u.name())))
        }
        Touch::Tower(i) => {
            let tower = &world.towers[i];
            let guard = if tower.garrison.is_empty() { "undefended".to_string() } else { tower.garrison.summary() };
            (tower.owner, format!("TOWER ({})\n{guard}", tower.owner.name()))
        }
        Touch::Seller(i) => {
            let unit = world.sellers[i].unit;
            (Faction::Black, format!("Idle {}\nE: hire for {}g", unit.name(), unit.price()))
        }
    }
}

/// Owners change while you walk around (AI armies take towers, you move units), so the
/// border colour and label of every building follow the world state.
fn refresh_building_visuals(
    world: Res<WorldState>,
    buildings: Query<(&BuildingVisual, &Children)>,
    mut borders: Query<&mut Sprite, With<OwnerBorder>>,
    mut labels: Query<&mut Text2d, (With<BuildingLabel>, Without<OwnerBorder>)>,
) {
    if !world.is_changed() {
        return;
    }
    for (visual, children) in &buildings {
        let (owner, text) = building_text(&world, visual.0);
        for child in children.iter() {
            if let Ok(mut border) = borders.get_mut(child)
                && border.color != owner.color()
            {
                border.color = owner.color();
            }
            if let Ok(mut label) = labels.get_mut(child)
                && label.0 != text
            {
                label.0 = text;
                break;
            }
        }
    }
}

fn spawn_overworld(
    mut commands: Commands,
    world: Res<WorldState>,
    assets: Res<GameAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    spawn_ground(&mut commands, &assets, &world);
    spawn_decorations(&mut commands, &assets, &world);

    // Kingdom bases.
    for (bi, base) in world.bases.iter().enumerate() {
        for (kind, tile) in base.buildings() {
            let size_tiles = kind.size().as_vec2();
            let size = size_tiles * TILE - Vec2::splat(6.0);
            let pos = world.terrain.world_pos(tile) + (size_tiles - Vec2::ONE) * TILE / 2.0;
            let touch = match kind {
                BuildingKind::Castle => Touch::Castle(bi),
                _ => Touch::Shop { base: bi, kind },
            };
            commands.spawn(building_bundle(kind.color(), size, pos, touch, &world));
        }
    }

    // Towers.
    for (ti, tower) in world.towers.iter().enumerate() {
        commands.spawn(building_bundle(
            BuildingKind::Tower.color(),
            Vec2::splat(TILE - 10.0),
            world.terrain.world_pos(tower.tile),
            Touch::Tower(ti),
            &world,
        ));
    }

    // Idle sellers.
    for (si, seller) in world.sellers.iter().enumerate() {
        let text = format!("Idle {}\nE: hire for {}g", seller.unit.name(), seller.unit.price());
        commands.spawn((
            Sprite::from_color(Color::srgb(0.75, 0.75, 0.70), Vec2::splat(HERO_SIZE)),
            Transform::from_translation(world.terrain.world_pos(seller.tile).extend(1.0)),
            Touchable { touch: Touch::Seller(si), radius: HERO_SIZE * 1.2 },
            DespawnOnExit(GameState::Overworld),
            children![
                (
                    Text2d::new(seller.unit.initial().to_string()),
                    TextFont { font_size: FontSize::Px(24.0), ..default() },
                    TextColor(Color::BLACK),
                    Transform::from_xyz(0.0, 0.0, 0.1),
                    Pickable::IGNORE,
                ),
                label(text, Color::WHITE, Vec2::new(0.0, HERO_SIZE * 0.9)),
            ],
        ));
    }

    // Roaming armies and bandits.
    for (i, army) in world.armies.iter().enumerate().filter(|(_, a)| a.alive) {
        armies::spawn_ai_army_entity(&mut commands, army, i);
    }
    for (i, squad) in world.bandits.iter().enumerate().filter(|(_, b)| b.alive) {
        armies::spawn_bandit_entity(&mut commands, &mut meshes, &mut materials, squad, i);
    }

    // Hero.
    let hero_layer = world.terrain.layer(world.terrain.tile_of(world.hero_pos)).unwrap_or(1);
    commands.spawn((
        Hero,
        MoveSpeed(HERO_SPEED),
        Layer(hero_layer),
        Faction::Reclaimers,
        Sprite::from_color(Faction::Reclaimers.color(), Vec2::splat(HERO_SIZE)),
        Transform::from_translation(world.hero_pos.extend(5.0)),
        DespawnOnExit(GameState::Overworld),
        children![label("HERO", Team::Player.color(), Vec2::new(0.0, HERO_SIZE * 0.85))],
    ));

    // HUD.
    commands.spawn((
        OverworldHud,
        Text::new(""),
        TextFont { font_size: FontSize::Px(18.0), ..default() },
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(12.0),
            top: Val::Px(10.0),
            padding: UiRect::all(Val::Px(8.0)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
        Pickable::IGNORE,
        DespawnOnExit(GameState::Overworld),
    ));
    commands.spawn((
        LandUnitedBanner,
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        Visibility::Hidden,
        Pickable::IGNORE,
        DespawnOnExit(GameState::Overworld),
        children![(
            Text::new("YOU UNITED THE LAND!\nEvery castle is yours."),
            TextFont { font_size: FontSize::Px(44.0), ..default() },
            TextColor(Team::Player.color()),
            TextLayout::justify(Justify::Center),
            Node { padding: UiRect::all(Val::Px(30.0)), ..default() },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.8)),
            Pickable::IGNORE,
        )],
    ));
}

/// WASD / arrows. Water blocks. A change of layer is only allowed through a staircase
/// (`TerrainMap::can_step`); against anything else the hero slides along the edge.
fn hero_input(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    world: Res<WorldState>,
    mut hero: Single<(&mut Transform, &MoveSpeed, &mut Layer), With<Hero>>,
) {
    let mut dir = Vec2::ZERO;
    if keys.any_pressed([KeyCode::KeyW, KeyCode::ArrowUp]) {
        dir.y += 1.0;
    }
    if keys.any_pressed([KeyCode::KeyS, KeyCode::ArrowDown]) {
        dir.y -= 1.0;
    }
    if keys.any_pressed([KeyCode::KeyA, KeyCode::ArrowLeft]) {
        dir.x -= 1.0;
    }
    if keys.any_pressed([KeyCode::KeyD, KeyCode::ArrowRight]) {
        dir.x += 1.0;
    }
    let (transform, speed, layer) = &mut *hero;
    let step = dir.normalize_or_zero() * speed.0 * time.delta_secs();
    let map = &world.terrain;
    let mut pos = transform.translation.truncate();

    for axis_step in [Vec2::new(step.x, 0.0), Vec2::new(0.0, step.y)] {
        let next = pos + axis_step;
        let (from, to) = (map.tile_of(pos), map.tile_of(next));
        if from == to || map.can_step(from, to) {
            pos = next;
        }
    }
    transform.translation.x = pos.x;
    transform.translation.y = pos.y;

    if let Some(now) = map.layer(map.tile_of(pos))
        && layer.0 != now
    {
        layer.0 = now;
    }
}

fn camera_follow(
    hero: Single<&Transform, With<Hero>>,
    mut camera: Single<&mut Transform, (With<Camera2d>, Without<Hero>)>,
) {
    camera.translation.x = hero.translation.x;
    camera.translation.y = hero.translation.y;
}

/// Keeps the memory current every frame so the hero returns to the same spot after a battle.
fn remember_hero_position(hero: Single<&Transform, With<Hero>>, mut world: ResMut<WorldState>) {
    let pos = hero.translation.truncate();
    if world.hero_pos != pos {
        world.hero_pos = pos;
    }
}

fn refresh_overworld_hud(
    world: Res<WorldState>,
    treasuries: Res<Treasuries>,
    army: Res<PlayerArmy>,
    nearby: Res<Nearby>,
    hero: Single<&Layer, With<Hero>>,
    mut hud: Single<&mut Text, With<OverworldHud>>,
    mut banner: Single<&mut Visibility, With<LandUnitedBanner>>,
) {
    let status = format!(
        "Gold {}    Army {}/{}: {}    Castles {}/{}    Towers {}/{}    Layer {}",
        treasuries.player_gold(),
        army.0.slots(),
        HERO_SLOTS,
        army.0.summary(),
        world.castles_owned(),
        world.bases.len(),
        world.towers_owned(),
        world.towers.len(),
        hero.0,
    );

    let prompt = match nearby.touch {
        Some(Touch::Castle(i)) if world.bases[i].owner.is_player() => format!(
            "YOUR CASTLE  garrison {}/30: {}\n1 W  2 A  3 L  4 M  5 P: take one   Shift+key: store one   hold Ctrl: all",
            world.bases[i].garrison.slots(),
            world.bases[i].garrison.summary()
        ),
        Some(Touch::Castle(i)) => format!("Castle of the {}. Touching it attacks!", world.bases[i].owner.name()),
        Some(Touch::Shop { base, kind }) if world.bases[base].owner.is_player() => {
            let unit = kind.sells().unwrap();
            format!("{}: E = hire a {} for {}g", kind.name(), unit.name(), unit.price())
        }
        Some(Touch::Shop { base, kind }) => {
            format!("{} of the {}. Take their castle to use it.", kind.name(), world.bases[base].owner.name())
        }
        Some(Touch::Tower(i)) if world.towers[i].owner.is_player() => {
            format!("Your tower: +{TOWER_INCOME} gold every {INCOME_INTERVAL_SECS:.0} s")
        }
        Some(Touch::Tower(i)) => format!("Tower of the {}. Touching it attacks!", world.towers[i].owner.name()),
        Some(Touch::Seller(i)) => {
            let unit = world.sellers[i].unit;
            format!("Idle {}: E = hire for {}g", unit.name(), unit.price())
        }
        None => "WASD: move.  Walk into buildings, towers and armies to use or attack them.  Red circles: bandits.  Stairs lead up to the high ground.".to_string(),
    };

    let mut text = format!("{status}\n{prompt}");
    if let Some(message) = &nearby.message {
        text.push('\n');
        text.push_str(message);
    }
    if hud.0 != text {
        hud.0 = text;
    }

    let wanted = if world.land_united() { Visibility::Visible } else { Visibility::Hidden };
    if **banner != wanted {
        **banner = wanted;
    }
}
