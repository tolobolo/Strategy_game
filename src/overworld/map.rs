//! The world map overlay (`M`): terrain, bases, towers, sellers, stairs and the hero. Roaming
//! armies and bandits are deliberately left out. See agents.md §1.1.

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use super::Hero;
use super::terrain::Terrain;
use super::world::WorldState;
use crate::faction::Faction;
use crate::states::GameState;

/// Screen pixels per map tile on the overlay.
const PIXELS_PER_TILE: u32 = 10;

#[derive(Component)]
pub struct WorldMap;

#[derive(Resource, Default)]
pub struct WorldMapOpen(pub bool);

pub fn toggle_world_map(
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut open: ResMut<WorldMapOpen>,
    world: Res<WorldState>,
    hero: Single<&Transform, With<Hero>>,
    existing: Query<Entity, With<WorldMap>>,
) {
    if !keys.just_pressed(KeyCode::KeyM) {
        return;
    }
    if open.0 {
        for entity in &existing {
            commands.entity(entity).despawn();
        }
        open.0 = false;
        return;
    }
    open.0 = true;

    let (width, height) = (world.terrain.width as u32, world.terrain.height as u32);
    let image = images.add(render_map(&world, world.terrain.tile_of(hero.translation.truncate())));
    commands.spawn((
        WorldMap,
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            row_gap: Val::Px(8.0),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.75)),
        Pickable::IGNORE,
        DespawnOnExit(GameState::Overworld),
        children![
            (
                Text::new("WORLD MAP   (M to close)"),
                TextFont { font_size: FontSize::Px(22.0), ..default() },
                TextColor(Color::WHITE),
                Pickable::IGNORE,
            ),
            (
                ImageNode::new(image),
                Node {
                    width: Val::Px((width * PIXELS_PER_TILE) as f32),
                    height: Val::Px((height * PIXELS_PER_TILE) as f32),
                    ..default()
                },
                Pickable::IGNORE,
            ),
            (
                Text::new(legend(&world)),
                TextFont { font_size: FontSize::Px(14.0), ..default() },
                TextColor(Color::srgb(0.85, 0.85, 0.85)),
                Pickable::IGNORE,
            ),
        ],
    ));
}

fn legend(world: &WorldState) -> String {
    let kingdoms: Vec<String> = Faction::KINGDOMS
        .iter()
        .map(|f| format!("{} = {}", color_name(*f), f.name()))
        .collect();
    format!(
        "{}   |   black = neutral towers   grey dot = idle seller   white = stairs   bright green = you\nCastles {}/{}   Towers {}/{}",
        kingdoms.join("   "),
        world.castles_owned(),
        world.bases.len(),
        world.towers_owned(),
        world.towers.len()
    )
}

fn color_name(faction: Faction) -> &'static str {
    match faction {
        Faction::Reclaimers => "blue",
        Faction::Red => "red",
        Faction::Purple => "purple",
        Faction::Yellow => "yellow",
        Faction::Black => "black",
    }
}

fn terrain_rgb(terrain: Terrain) -> [u8; 3] {
    match terrain {
        Terrain::Water => [56, 115, 191],
        Terrain::Grass => [115, 168, 89],
        Terrain::Mountain => [140, 133, 128],
    }
}

fn faction_rgb(faction: Faction) -> [u8; 3] {
    let c = faction.color().to_srgba();
    [(c.red * 255.0) as u8, (c.green * 255.0) as u8, (c.blue * 255.0) as u8]
}

/// One pixel per map tile, drawn in the orientation of the overworld (row 0 at the top).
/// Structures are painted over the terrain; armies and bandits are not shown.
fn render_map(world: &WorldState, hero_tile: IVec2) -> Image {
    let map = &world.terrain;
    let (w, h) = (map.width, map.height);
    let mut pixels = vec![0u8; (w * h * 4) as usize];
    let mut put = |tile: IVec2, rgb: [u8; 3]| {
        if !map.in_bounds(tile) {
            return;
        }
        let i = (((h - 1 - tile.y) * w + tile.x) * 4) as usize;
        pixels[i..i + 3].copy_from_slice(&rgb);
        pixels[i + 3] = 255;
    };

    for (tile, terrain) in map.tiles() {
        put(tile, terrain_rgb(terrain));
    }
    for stairs in map.stairs() {
        put(stairs, [240, 240, 240]);
    }
    for base in &world.bases {
        for tile in base.tiles() {
            put(tile, faction_rgb(base.owner));
        }
        // The castle itself is a lighter block in the middle of the base.
        for (kind, tile) in base.buildings() {
            if kind == super::buildings::BuildingKind::Castle {
                for d in [IVec2::ZERO, IVec2::X, IVec2::Y, IVec2::ONE] {
                    put(tile + d, [230, 230, 230]);
                }
            }
        }
    }
    for tower in &world.towers {
        put(tower.tile, faction_rgb(tower.owner));
        for d in [IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y] {
            put(tower.tile + d, [20, 20, 24]);
        }
        put(tower.tile, faction_rgb(tower.owner));
    }
    for seller in &world.sellers {
        put(seller.tile, [200, 200, 195]);
    }
    // The hero: a bright cross so it stands out at one pixel per tile.
    for d in [IVec2::ZERO, IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y] {
        put(hero_tile + d, [60, 255, 90]);
    }

    let mut image = Image::new(
        Extent3d { width: w as u32, height: h as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    // Crisp pixels when the 64×48 image is scaled up.
    image.sampler = bevy::image::ImageSampler::nearest();
    image
}
