//! Handles to the Tiny Swords art. Paths are relative to `assets/` (agents.md §5).
//! Units and buildings are still coloured squares; terrain and decorations use real sprites.

use bevy::prelude::*;

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
        app.insert_resource(assets).add_systems(Update, animate_sprites);
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
    assets
}

/// Loops an atlas sprite through its frames.
#[derive(Component)]
pub struct Animated {
    pub frames: usize,
    pub timer: Timer,
}

impl Animated {
    pub fn new(frames: usize, fps: f32) -> Self {
        Self { frames, timer: Timer::from_seconds(1.0 / fps, TimerMode::Repeating) }
    }
}

pub fn animate_sprites(time: Res<Time>, mut query: Query<(&mut Animated, &mut Sprite)>) {
    for (mut anim, mut sprite) in &mut query {
        if anim.frames > 0
            && anim.timer.tick(time.delta()).just_finished()
            && let Some(atlas) = sprite.texture_atlas.as_mut()
        {
            atlas.index = (atlas.index + 1) % anim.frames;
        }
    }
}
