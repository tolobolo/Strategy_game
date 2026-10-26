//! Builds the grid, places both armies and rolls the turn order.

use bevy::prelude::*;
use rand::seq::SliceRandom;

use super::actions::CombatAction;
use super::grid::{BattleGrid, GRID_COLS, GRID_ROWS, GridPos, TILE_SIZE, Tile};
use super::hud::{ActiveMarker, CombatLog, CountLabel, Hovered, count_text};
use super::turn::{ActiveStack, TurnOrder};
use crate::faction::Faction;
use crate::overworld::PendingEncounter;
use crate::states::{CombatPhase, GameState};
use crate::units::{Army, PlayerArmy, Stack, Team};

pub fn spawn_battle(
    mut commands: Commands,
    encounter: Res<PendingEncounter>,
    player_army: Res<PlayerArmy>,
    mut camera: Single<&mut Transform, With<Camera2d>>,
    mut next_phase: ResMut<NextState<CombatPhase>>,
) {
    camera.translation.x = 0.0;
    camera.translation.y = -20.0;

    let grid = BattleGrid { cols: GRID_COLS, rows: GRID_ROWS, tile_size: TILE_SIZE };
    commands.insert_resource(grid);

    let mut log = CombatLog::default();
    log.push(format!(
        "Battle: {} vs {}",
        player_army.0.faction.name(),
        encounter.enemy.faction.name()
    ));
    commands.insert_resource(log);

    for pos in grid.all_positions() {
        commands
            .spawn((
                Tile,
                pos,
                Sprite::from_color(grid.base_color(pos), Vec2::splat(grid.tile_size - 2.0)),
                Transform::from_translation(grid.to_world(pos).extend(0.0)),
                // Sprites are only hit-tested when they carry a Pickable component.
                Pickable::default(),
                DespawnOnExit(GameState::Combat),
            ))
            .observe(on_tile_click);
    }

    let mut order = Vec::new();
    place_army(&mut commands, &grid, Team::Player, &player_army.0, &mut order);
    place_army(&mut commands, &grid, Team::Enemy, &encounter.enemy, &mut order);
    order.shuffle(&mut rand::rng());
    commands.insert_resource(TurnOrder { order, current: 0 });

    // Yellow halo that sits behind whichever stack is active.
    commands.spawn((
        ActiveMarker,
        Sprite::from_color(Color::srgb(1.0, 0.9, 0.2), Vec2::splat(grid.tile_size)),
        Transform::from_xyz(0.0, 0.0, 0.5),
        Visibility::Hidden,
        Pickable::IGNORE,
        DespawnOnExit(GameState::Combat),
    ));

    next_phase.set(CombatPhase::Turn);
}

/// Player stacks fill the left columns, enemy stacks the right ones, centred vertically.
fn place_army(
    commands: &mut Commands,
    grid: &BattleGrid,
    team: Team,
    army: &Army,
    order: &mut Vec<Entity>,
) {
    let columns = grid.deploy_columns(team == Team::Player);
    let stacks: Vec<&Stack> = army.stacks.iter().filter(|s| !s.is_dead()).collect();
    let per_column = grid.rows as usize;
    let first_row = (grid.rows - (stacks.len().min(per_column) as i32)) / 2;

    for (i, stack) in stacks.into_iter().enumerate() {
        let col = columns[(i / per_column) % columns.len()];
        let row = first_row + (i % per_column) as i32;
        let entity = spawn_stack(commands, grid, team, army.faction, *stack, GridPos::new(col, row));
        order.push(entity);
    }
}

/// One stack = a faction-coloured square with a team-coloured border and three labels.
/// To use real art, replace the `Sprite::from_color` on the parent with `Sprite::from_atlas_image`.
pub fn spawn_stack(
    commands: &mut Commands,
    grid: &BattleGrid,
    team: Team,
    faction: Faction,
    stack: Stack,
    pos: GridPos,
) -> Entity {
    let size = grid.tile_size * 0.68;
    commands
        .spawn((
            stack,
            team,
            faction,
            pos,
            Sprite::from_color(faction.color(), Vec2::splat(size)),
            Transform::from_translation(grid.to_world(pos).extend(1.0)),
            Pickable::default(),
            DespawnOnExit(GameState::Combat),
            children![
                // Team border: a slightly bigger square behind the fill.
                (
                    Sprite::from_color(team.color(), Vec2::splat(size + 10.0)),
                    Transform::from_xyz(0.0, 0.0, -0.1),
                    Pickable::IGNORE,
                ),
                // Kind initial.
                (
                    Text2d::new(stack.kind.initial().to_string()),
                    TextFont { font_size: FontSize::Px(30.0), ..default() },
                    TextColor(faction.text_color()),
                    Transform::from_xyz(0.0, 1.0, 0.1),
                    Pickable::IGNORE,
                ),
                // Count and lead-unit HP, refreshed by hud::refresh_stack_labels.
                (
                    CountLabel,
                    Text2d::new(count_text(&stack)),
                    TextFont { font_size: FontSize::Px(12.0), ..default() },
                    TextColor(Color::WHITE),
                    Transform::from_xyz(0.0, -size / 2.0 - 12.0, 0.1),
                    Pickable::IGNORE,
                ),
                // YOU / ENEMY tag.
                (
                    Text2d::new(team.label()),
                    TextFont { font_size: FontSize::Px(12.0), ..default() },
                    TextColor(team.color()),
                    Transform::from_xyz(0.0, size / 2.0 + 13.0, 0.1),
                    Pickable::IGNORE,
                ),
            ],
        ))
        .observe(on_stack_click)
        .observe(on_stack_over)
        .observe(on_stack_out)
        .id()
}

fn player_is_active(active: &Query<&Team, With<ActiveStack>>) -> bool {
    matches!(active.single(), Ok(Team::Player))
}

fn on_tile_click(
    click: On<Pointer<Click>>,
    tiles: Query<&GridPos, With<Tile>>,
    active: Query<&Team, With<ActiveStack>>,
    mut actions: MessageWriter<CombatAction>,
) {
    if click.event.button != PointerButton::Primary || !player_is_active(&active) {
        return;
    }
    if let Ok(pos) = tiles.get(click.entity) {
        actions.write(CombatAction::Move(*pos));
    }
}

fn on_stack_click(
    click: On<Pointer<Click>>,
    active: Query<&Team, With<ActiveStack>>,
    teams: Query<&Team>,
    mut actions: MessageWriter<CombatAction>,
) {
    if click.event.button != PointerButton::Primary || !player_is_active(&active) {
        return;
    }
    match teams.get(click.entity) {
        Ok(Team::Player) => actions.write(CombatAction::Merge(click.entity)),
        _ => actions.write(CombatAction::Strike(click.entity)),
    };
}

fn on_stack_over(over: On<Pointer<Over>>, mut hovered: ResMut<Hovered>) {
    hovered.0 = Some(over.entity);
}

fn on_stack_out(out: On<Pointer<Out>>, mut hovered: ResMut<Hovered>) {
    if hovered.0 == Some(out.entity) {
        hovered.0 = None;
    }
}
