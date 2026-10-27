//! Handles to the Tiny Swords art. Paths are relative to `assets/` (agents.md §5).
//! Terrain, decorations and units use real sprites; buildings and UI are still coloured squares.

use std::collections::HashMap;

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::sprite::Anchor;

use crate::faction::Faction;
use crate::overworld::buildings::BuildingKind;
use crate::units::UnitKind;

/// Every tile sheet is 9 columns × 6 rows of 64×64 tiles.
pub const TILESHEET_COLS: u32 = 9;
pub const TILESHEET_ROWS: u32 = 6;

/// An animated strip: one image and its frame layout.
#[derive(Clone, Default)]
pub struct Strip {
    pub image: Handle<Image>,
    pub layout: Handle<TextureAtlasLayout>,
    pub frames: usize,
}

impl Strip {
    /// A sprite showing frame 0 of the strip.
    pub fn sprite(&self) -> Sprite {
        Sprite::from_atlas_image(self.image.clone(), TextureAtlas { layout: self.layout.clone(), index: 0 })
    }
}

#[derive(Resource, Default)]
pub struct GameAssets {
    /// `tilemap_color1..5`: four greens/greys for the four quarters of the map, 5 = base ground.
    pub tilemaps: [Handle<Image>; 5],
    pub tilemap_layout: Handle<TextureAtlasLayout>,
    pub water: Handle<Image>,
    pub trees: [Strip; 4],
    pub bushes: [Strip; 4],
    pub rocks: [Handle<Image>; 4],
    pub water_rocks: [Strip; 4],
    pub duck: Strip,
    /// One-shot particle effects from `particle_fx/`, indexed by `Fx`.
    pub fx: HashMap<Fx, Strip>,
    /// `buildings/<colour>_buildings/<kind>.png` per faction and kind.
    pub buildings: HashMap<(Faction, BuildingKind), Handle<Image>>,
    pub ui: UiAssets,
}

/// Nine-slice panels and buttons stitched from the pack's UI sheets (`ui_elements/generated/`).
#[derive(Default)]
pub struct UiAssets {
    pub wood_table: Handle<Image>,
    pub paper: Handle<Image>,
    pub button_regular: Handle<Image>,
    pub button_pressed: Handle<Image>,
    /// `icons/icon_09.png`, the red X.
    pub icon_close: Handle<Image>,
    /// `icons/icon_03.png`, the coin: shown instead of the word "gold".
    pub icon_gold: Handle<Image>,
    /// `icons/icon_05.png`, the sword: shown instead of the word "army".
    pub icon_army: Handle<Image>,
    /// `ui_elements/generated/ribbon_<colour>_3.png`, one big ribbon per kingdom colour.
    pub ribbons: HashMap<Faction, Handle<Image>>,
    /// `ui_elements/generated/ribbon_small_<colour>_3.png` (188×60, caps 62 px).
    pub small_ribbons: HashMap<Faction, Handle<Image>>,
}

/// Scale that fits a building sprite onto its footprint (measured sizes in agents.md §5).
pub fn building_scale(kind: BuildingKind) -> f32 {
    match kind {
        BuildingKind::Castle => 0.42,
        BuildingKind::House => 0.6,
        BuildingKind::Archery | BuildingKind::Barracks => 0.4,
        BuildingKind::Monastery => 0.36,
        BuildingKind::Tower => 0.55,
    }
}

/// Anchor at the building's base (bottom of the visible pixels), so it stands on its tile.
pub fn building_anchor(kind: BuildingKind) -> Anchor {
    let (frame_h, base_row) = match kind {
        BuildingKind::Castle => (256.0, 249.0),
        BuildingKind::House => (192.0, 173.0),
        BuildingKind::Archery => (256.0, 240.0),
        BuildingKind::Barracks => (256.0, 245.0),
        BuildingKind::Monastery => (320.0, 310.0),
        BuildingKind::Tower => (256.0, 230.0),
    };
    Anchor(Vec2::new(0.0, 0.5 - base_row / frame_h))
}

/// Height of the drawn building in world units, for placing its label above it.
pub fn building_height(kind: BuildingKind) -> f32 {
    let visible = match kind {
        BuildingKind::Castle => 208.0,
        BuildingKind::House => 157.0,
        BuildingKind::Archery => 179.0,
        BuildingKind::Barracks => 187.0,
        BuildingKind::Monastery => 265.0,
        BuildingKind::Tower => 184.0,
    };
    visible * building_scale(kind)
}

/// The sprite-sheet particle effects in the pack. Play one with `spawn_effect`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Fx {
    Dust1,
    /// Used for a stack's death.
    Dust2,
    Explosion1,
    Explosion2,
    Fire1,
    Fire2,
    Fire3,
    WaterSplash,
}

impl Fx {
    pub const ALL: [Fx; 8] = [Fx::Dust1, Fx::Dust2, Fx::Explosion1, Fx::Explosion2, Fx::Fire1, Fx::Fire2, Fx::Fire3, Fx::WaterSplash];

    /// File, frame size and frame count (measured).
    fn sheet(self) -> (&'static str, u32, u32) {
        match self {
            Fx::Dust1 => ("particle_fx/dust_01.png", 64, 8),
            Fx::Dust2 => ("particle_fx/dust_02.png", 64, 10),
            Fx::Explosion1 => ("particle_fx/explosion_01.png", 192, 8),
            Fx::Explosion2 => ("particle_fx/explosion_02.png", 192, 10),
            Fx::Fire1 => ("particle_fx/fire_01.png", 64, 8),
            Fx::Fire2 => ("particle_fx/fire_02.png", 64, 10),
            Fx::Fire3 => ("particle_fx/fire_03.png", 64, 12),
            Fx::WaterSplash => ("particle_fx/water_splash.png", 192, 9),
        }
    }

    pub fn fps(self) -> f32 {
        12.0
    }
}

impl GameAssets {
    /// Sprite for one tile of a tile sheet. `col`/`row` are 0-based cells of the 9×6 grid.
    pub fn tile(&self, sheet: usize, col: u32, row: u32) -> Sprite {
        Sprite::from_atlas_image(
            self.tilemaps[sheet].clone(),
            TextureAtlas { layout: self.tilemap_layout.clone(), index: (row * TILESHEET_COLS + col) as usize },
        )
    }

    /// Sprite for a rectangle of cells, used for the two-tile-tall stairs.
    pub fn tile_rect(&self, sheet: usize, col: u32, row: u32, cols: u32, rows: u32) -> Sprite {
        let mut sprite = Sprite::from_image(self.tilemaps[sheet].clone());
        sprite.rect = Some(Rect::new(
            col as f32 * 64.0,
            row as f32 * 64.0,
            (col + cols) as f32 * 64.0,
            (row + rows) as f32 * 64.0,
        ));
        sprite
    }
}

pub struct GameAssetsPlugin;

impl Plugin for GameAssetsPlugin {
    fn build(&self, app: &mut App) {
        // Built right here rather than in a Startup system: the first `OnEnter(GameState)` runs in
        // PreStartup, and the overworld needs these handles when it spawns.
        let server = app.world().resource::<AssetServer>().clone();
        let mut layouts = app.world_mut().resource_mut::<Assets<TextureAtlasLayout>>();
        let assets = load_assets(&server, &mut layouts);
        app.insert_resource(assets)
            .init_resource::<UnitSprites>()
            .add_systems(
                Update,
                (start_effects, apply_unit_visuals, animate_sprites, finish_one_shots, despawn_finished_effects).chain(),
            );
    }
}

fn strip(
    server: &AssetServer,
    layouts: &mut Assets<TextureAtlasLayout>,
    path: &str,
    frame: UVec2,
    frames: u32,
) -> Strip {
    Strip {
        image: server.load(path.to_string()),
        layout: layouts.add(TextureAtlasLayout::from_grid(frame, frames, 1, None, None)),
        frames: frames as usize,
    }
}

fn load_assets(server: &AssetServer, layouts: &mut Assets<TextureAtlasLayout>) -> GameAssets {
    let mut assets = GameAssets::default();
    assets.tilemaps = std::array::from_fn(|i| server.load(format!("terrain/tileset/tilemap_color{}.png", i + 1)));
    assets.tilemap_layout =
        layouts.add(TextureAtlasLayout::from_grid(UVec2::splat(64), TILESHEET_COLS, TILESHEET_ROWS, None, None));
    assets.water = server.load("terrain/tileset/water_background_color.png");

    // Trees 1 and 2 are 192×256 frames, 3 and 4 are 192×192. All have 8 frames.
    assets.trees = std::array::from_fn(|i| {
        let height = if i < 2 { 256 } else { 192 };
        strip(server, layouts, &format!("terrain/resources/wood/trees/tree{}.png", i + 1), UVec2::new(192, height), 8)
    });
    assets.bushes = std::array::from_fn(|i| {
        strip(server, layouts, &format!("terrain/decorations/bushes/bushe{}.png", i + 1), UVec2::splat(128), 8)
    });
    assets.rocks = std::array::from_fn(|i| server.load(format!("terrain/decorations/rocks/rock{}.png", i + 1)));
    assets.water_rocks = std::array::from_fn(|i| {
        strip(
            server,
            layouts,
            &format!("terrain/decorations/rocks_in_the_water/water_rocks_0{}.png", i + 1),
            UVec2::splat(64),
            16,
        )
    });
    assets.duck = strip(server, layouts, "terrain/decorations/rubber_duck/rubber_duck.png", UVec2::splat(32), 3);
    for fx in Fx::ALL {
        let (file, frame, frames) = fx.sheet();
        assets.fx.insert(fx, strip(server, layouts, file, UVec2::splat(frame), frames));
    }
    assets.ui = UiAssets {
        wood_table: server.load("ui_elements/generated/wood_table_9.png"),
        paper: server.load("ui_elements/generated/regular_paper_9.png"),
        button_regular: server.load("ui_elements/generated/big_red_button_regular_9.png"),
        button_pressed: server.load("ui_elements/generated/big_red_button_pressed_9.png"),
        icon_close: server.load("ui_elements/ui_elements/icons/icon_09.png"),
        icon_gold: server.load("ui_elements/ui_elements/icons/icon_03.png"),
        icon_army: server.load("ui_elements/ui_elements/icons/icon_05.png"),
        ribbons: HashMap::new(),
        small_ribbons: HashMap::new(),
    };
    for (faction, colour) in [
        (Faction::Reclaimers, "blue"),
        (Faction::Red, "red"),
        (Faction::Yellow, "yellow"),
        (Faction::Purple, "purple"),
        (Faction::Black, "black"),
    ] {
        assets.ui.ribbons.insert(faction, server.load(format!("ui_elements/generated/ribbon_{colour}_3.png")));
        assets.ui.small_ribbons.insert(faction, server.load(format!("ui_elements/generated/ribbon_small_{colour}_3.png")));
    }
    for faction in [Faction::Reclaimers, Faction::Red, Faction::Purple, Faction::Yellow, Faction::Black] {
        for kind in BuildingKind::ALL {
            let path = format!("{}/{}", faction.building_dir(), kind.sprite_file());
            assets.buildings.insert((faction, kind), server.load(path));
        }
    }
    assets
}

// ---------- one-shot effects ----------

/// A particle effect waiting for its cue (a sword landing, an arrow arriving).
#[derive(Component)]
pub struct PendingEffect {
    fx: Fx,
    scale: f32,
    delay: Timer,
    /// Entity to despawn when the effect starts (the thing the effect replaces).
    replaces: Option<Entity>,
}

/// Marker for a playing effect; despawned when its animation finishes.
#[derive(Component)]
pub struct OneShotEffect;

/// Play `fx` at `pos` after `delay` seconds, optionally despawning `replaces` at that moment.
pub fn spawn_effect(commands: &mut Commands, fx: Fx, pos: Vec2, z: f32, scale: f32, delay: f32, replaces: Option<Entity>) {
    commands.spawn((
        PendingEffect { fx, scale, delay: Timer::from_seconds(delay.max(0.0), TimerMode::Once), replaces },
        Transform::from_translation(pos.extend(z)),
        Visibility::Hidden,
        Pickable::IGNORE,
    ));
}

/// Turn pending effects into playing sprites when their cue arrives.
fn start_effects(
    time: Res<Time>,
    mut commands: Commands,
    assets: Res<GameAssets>,
    mut pending: Query<(Entity, &mut PendingEffect, &mut Transform, &mut Visibility)>,
) {
    for (entity, mut effect, mut transform, mut visibility) in &mut pending {
        if !effect.delay.tick(time.delta()).is_finished() {
            continue;
        }
        if let Some(victim) = effect.replaces
            && let Ok(mut victim) = commands.get_entity(victim)
        {
            victim.despawn();
        }
        let strip = &assets.fx[&effect.fx];
        transform.scale = Vec3::splat(effect.scale);
        *visibility = Visibility::Visible;
        commands
            .entity(entity)
            .remove::<PendingEffect>()
            .insert((strip.sprite(), Animated::once(strip.frames, effect.fx.fps()), OneShotEffect));
    }
}

fn despawn_finished_effects(mut commands: Commands, effects: Query<(Entity, &Animated), With<OneShotEffect>>) {
    for (entity, animated) in &effects {
        if animated.finished {
            commands.entity(entity).despawn();
        }
    }
}

// ---------- generic sprite animation ----------

/// Steps an atlas sprite through its frames.
#[derive(Component)]
pub struct Animated {
    pub frames: usize,
    pub timer: Timer,
    /// Stop on the last frame instead of looping.
    pub once: bool,
    /// Set when a one-shot animation has reached its last frame.
    pub finished: bool,
}

impl Animated {
    pub fn new(frames: usize, fps: f32) -> Self {
        Self { frames, timer: Timer::from_seconds(1.0 / fps, TimerMode::Repeating), once: false, finished: false }
    }

    pub fn once(frames: usize, fps: f32) -> Self {
        Self { once: true, ..Self::new(frames, fps) }
    }
}

pub fn animate_sprites(time: Res<Time>, mut query: Query<(&mut Animated, &mut Sprite)>) {
    for (mut anim, mut sprite) in &mut query {
        if anim.frames == 0 || anim.finished || !anim.timer.tick(time.delta()).just_finished() {
            continue;
        }
        let Some(atlas) = sprite.texture_atlas.as_mut() else { continue };
        if anim.once && atlas.index + 1 >= anim.frames {
            anim.finished = true;
        } else {
            atlas.index = (atlas.index + 1) % anim.frames;
        }
    }
}

// ---------- unit sprites ----------

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum UnitAnim {
    Idle,
    Run,
    /// Striking towards a direction (see `AttackDir::towards`).
    Attack(AttackDir),
    Guard,
}

/// Vertical component of an attack. Sheets face right; the left side is the same sheet flipped.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum AttackDir {
    Up,
    UpRight,
    Right,
    DownRight,
    Down,
}

impl AttackDir {
    /// Direction and horizontal flip for an attacker at `from` hitting `to` (world coords, y up):
    /// the signs of `to - from` pick the sheet, a negative x flips it.
    pub fn towards(from: Vec2, to: Vec2) -> (AttackDir, Option<bool>) {
        let d = to - from;
        let dx = if d.x.abs() < 1.0 { 0 } else { d.x.signum() as i32 };
        let dy = if d.y.abs() < 1.0 { 0 } else { d.y.signum() as i32 };
        let dir = match (dx, dy) {
            (0, 1) => AttackDir::Up,
            (0, -1) => AttackDir::Down,
            (_, 1) => AttackDir::UpRight,
            (_, -1) => AttackDir::DownRight,
            _ => AttackDir::Right,
        };
        // Straight up or down keeps whatever way the unit was facing.
        let flip = (dx != 0).then_some(dx < 0);
        (dir, flip)
    }

    /// Tilt (radians) for kinds that only have a right-facing attack sheet: the sprite is turned
    /// towards the target instead. The Lancer has a sheet per direction and is never tilted.
    pub fn tilt(self) -> f32 {
        match self {
            AttackDir::Up => std::f32::consts::FRAC_PI_2,
            AttackDir::UpRight => std::f32::consts::FRAC_PI_4,
            AttackDir::Right => 0.0,
            AttackDir::DownRight => -std::f32::consts::FRAC_PI_4,
            AttackDir::Down => -std::f32::consts::FRAC_PI_2,
        }
    }
}

impl UnitAnim {
    fn fps(self) -> f32 {
        match self {
            UnitAnim::Idle => 8.0,
            UnitAnim::Run => 10.0,
            UnitAnim::Attack(_) => 10.0,
            UnitAnim::Guard => 8.0,
        }
    }
}

/// File (inside `units/<colour>_units/<kind>/`) and frame count of one animation.
/// Kinds without a sheet for an animation fall back to idle. See the table in agents.md §5.
fn unit_sheet(kind: UnitKind, anim: UnitAnim) -> (&'static str, u32) {
    use UnitAnim::*;
    use UnitKind::*;
    match (kind, anim) {
        (Warrior, Idle) => ("warrior/warrior_idle.png", 8),
        (Warrior, Run) => ("warrior/warrior_run.png", 6),
        (Warrior, Attack(_)) => ("warrior/warrior_attack1.png", 4),
        (Warrior, Guard) => ("warrior/warrior_guard.png", 6),
        (Archer, Idle | Guard) => ("archer/archer_idle.png", 6),
        (Archer, Run) => ("archer/archer_run.png", 4),
        (Archer, Attack(_)) => ("archer/archer_shoot.png", 8),
        (Lancer, Idle) => ("lancer/lancer_idle.png", 12),
        (Lancer, Run) => ("lancer/lancer_run.png", 6),
        (Lancer, Attack(AttackDir::Up)) => ("lancer/lancer_up_attack.png", 3),
        (Lancer, Attack(AttackDir::UpRight)) => ("lancer/lancer_up_right_attack.png", 3),
        (Lancer, Attack(AttackDir::Right)) => ("lancer/lancer_right_attack.png", 3),
        (Lancer, Attack(AttackDir::DownRight)) => ("lancer/lancer_down_right_attack.png", 3),
        (Lancer, Attack(AttackDir::Down)) => ("lancer/lancer_down_attack.png", 3),
        (Lancer, Guard) => ("lancer/lancer_right_defence.png", 6),
        (Monk, Idle | Guard) => ("monk/idle.png", 6),
        (Monk, Run) => ("monk/run.png", 4),
        (Monk, Attack(_)) => ("monk/heal.png", 11),
        (Pawn, Idle | Guard) => ("pawn/pawn_idle.png", 8),
        (Pawn, Run) => ("pawn/pawn_run.png", 6),
        (Pawn, Attack(_)) => ("pawn/pawn_interact_knife.png", 4),
    }
}

/// How long one pass of an animation takes, in seconds.
pub fn anim_duration(kind: UnitKind, anim: UnitAnim) -> f32 {
    let (_, frames) = unit_sheet(kind, anim);
    frames as f32 / anim.fps()
}

/// Frame size of a kind's sheets.
pub fn unit_frame(kind: UnitKind) -> u32 {
    match kind {
        UnitKind::Lancer => 320,
        _ => 192,
    }
}

/// Scale that makes the visible character roughly one combat tile tall.
pub fn unit_scale(kind: UnitKind) -> f32 {
    match kind {
        UnitKind::Lancer => 0.45,
        _ => 0.6,
    }
}

/// Anchor at the character's feet (measured: feet at row 137 of 192, row 198 of 320).
pub fn unit_anchor(kind: UnitKind) -> Anchor {
    match kind {
        UnitKind::Lancer => Anchor(Vec2::new(0.0, -0.12)),
        _ => Anchor(Vec2::new(0.0, -0.21)),
    }
}

/// Lazily loaded unit strips, keyed by faction colour, kind and animation.
#[derive(Resource, Default)]
pub struct UnitSprites {
    cache: HashMap<(Faction, UnitKind, UnitAnim), Strip>,
}

/// What a unit sprite shows. Change it and `apply_unit_visuals` swaps the sheet.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnitVisual {
    pub faction: Faction,
    pub kind: UnitKind,
    pub anim: UnitAnim,
    /// Play once, then go back to idle.
    pub once: bool,
    /// Sheets face right; enemies in combat face left.
    pub flip_x: bool,
}

impl UnitVisual {
    pub fn new(faction: Faction, kind: UnitKind, flip_x: bool) -> Self {
        Self { faction, kind, anim: UnitAnim::Idle, once: false, flip_x }
    }

    pub fn play(&mut self, anim: UnitAnim, once: bool) {
        self.anim = anim;
        self.once = once;
    }
}

/// Everything needed to build a unit sprite, bundled so spawn helpers take one parameter.
#[derive(SystemParam)]
pub struct UnitArt<'w> {
    sprites: ResMut<'w, UnitSprites>,
    server: Res<'w, AssetServer>,
    layouts: ResMut<'w, Assets<TextureAtlasLayout>>,
}

impl UnitArt<'_> {
    pub fn strip(&mut self, faction: Faction, kind: UnitKind, anim: UnitAnim) -> Strip {
        let key = (faction, kind, anim);
        if let Some(strip) = self.sprites.cache.get(&key) {
            return strip.clone();
        }
        let (file, frames) = unit_sheet(kind, anim);
        let path = format!("{}/{file}", faction.sprite_dir());
        let strip = strip(&self.server, &mut self.layouts, &path, UVec2::splat(unit_frame(kind)), frames);
        self.sprites.cache.insert(key, strip.clone());
        strip
    }

    /// The arrow projectile of a faction's archers (`archer/arrow.png`, 64×64, points right).
    pub fn arrow(&mut self, faction: Faction) -> Handle<Image> {
        self.server.load(format!("{}/archer/arrow.png", faction.sprite_dir()))
    }

    /// A child entity bundle showing `visual`, feet at `offset` from the parent, at `scale`.
    pub fn bundle(&mut self, visual: UnitVisual, scale: f32, offset: Vec2, z: f32) -> impl Bundle {
        let strip = self.strip(visual.faction, visual.kind, visual.anim);
        let mut sprite = strip.sprite();
        sprite.flip_x = visual.flip_x;
        (
            visual,
            sprite,
            Animated::new(strip.frames, visual.anim.fps()),
            unit_anchor(visual.kind),
            Transform::from_translation(offset.extend(z)).with_scale(Vec3::splat(scale)),
            Pickable::IGNORE,
        )
    }
}

/// Swap the sheet whenever a `UnitVisual` changes.
fn apply_unit_visuals(
    mut art: UnitArt,
    mut query: Query<(&UnitVisual, &mut Sprite, &mut Animated, &mut Transform), Changed<UnitVisual>>,
) {
    for (visual, mut sprite, mut animated, mut transform) in &mut query {
        let strip = art.strip(visual.faction, visual.kind, visual.anim);
        sprite.image = strip.image;
        sprite.texture_atlas = Some(TextureAtlas { layout: strip.layout, index: 0 });
        sprite.flip_x = visual.flip_x;
        // Kinds without directional sheets lean towards their target while striking. A flipped
        // sprite faces left, so its tilt runs the other way round.
        let tilt = match visual.anim {
            UnitAnim::Attack(dir) if visual.kind != UnitKind::Lancer => {
                if visual.flip_x { -dir.tilt() } else { dir.tilt() }
            }
            _ => 0.0,
        };
        transform.rotation = Quat::from_rotation_z(tilt);
        *animated = if visual.once {
            Animated::once(strip.frames, visual.anim.fps())
        } else {
            Animated::new(strip.frames, visual.anim.fps())
        };
    }
}

/// One-shot animations (attacks) return to idle when they end.
fn finish_one_shots(mut query: Query<(&mut UnitVisual, &Animated)>) {
    for (mut visual, animated) in &mut query {
        if visual.once && animated.finished {
            visual.play(UnitAnim::Idle, false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attack_direction_follows_the_sign_of_the_difference() {
        let me = Vec2::new(100.0, 100.0);
        assert_eq!(AttackDir::towards(me, Vec2::new(172.0, 100.0)), (AttackDir::Right, Some(false)));
        assert_eq!(AttackDir::towards(me, Vec2::new(28.0, 100.0)), (AttackDir::Right, Some(true)), "left = flipped");
        assert_eq!(AttackDir::towards(me, Vec2::new(172.0, 172.0)), (AttackDir::UpRight, Some(false)));
        assert_eq!(AttackDir::towards(me, Vec2::new(28.0, 28.0)), (AttackDir::DownRight, Some(true)), "down-left = flipped down-right");
        assert_eq!(AttackDir::towards(me, Vec2::new(100.0, 172.0)), (AttackDir::Up, None), "straight up keeps the facing");
        assert_eq!(AttackDir::towards(me, Vec2::new(100.0, 28.0)), (AttackDir::Down, None));
        assert!(AttackDir::UpRight.tilt() > 0.0 && AttackDir::DownRight.tilt() < 0.0 && AttackDir::Right.tilt() == 0.0);
    }
}
