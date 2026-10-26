//! Builds the grid, places both armies and rolls the turn order.

use bevy::prelude::*;
use rand::seq::SliceRandom;

use super::actions::{ActionLock, CombatAction};
use super::grid::{BattleGrid, GRID_COLS, GRID_ROWS, GridPos, TILE_SIZE, Tile};
use super::hud::{ActiveMarker, CombatLog, CountLabel, Hovered, count_text};
use super::turn::{ActiveStack, TurnOrder};
use crate::assets::{UnitArt, UnitVisual, unit_scale};
use crate::faction::Faction;
use crate::overworld::PendingEncounter;
use crate::states::{CombatPhase, GameState};
use crate::units::{Army, PlayerArmy, Stack, Team};

pub fn spawn_battle(
    mut commands: Commands,
    mut art: UnitArt,
    encounter: Res<PendingEncounter>,
    player_army: Res<PlayerArmy>,
    mut camera: Single<&mut Transform, With<Camera2d>>,
    mut next_phase: ResMut<NextState<CombatPhase>>,
) {
    camera.translation.x = 0.0;
    camera.translation.y = -20.0;

    let grid = BattleGrid { cols: GRID_COLS, rows: GRID_ROWS, tile_size: TILE_SIZE };
    commands.insert_resource(grid);
    commands.insert_resource(ActionLock::default());

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
    place_army(&mut commands, &mut art, &grid, Team::Player, &player_army.0, &mut order);
    place_army(&mut commands, &mut art, &grid, Team::Enemy, &encounter.enemy, &mut order);
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
    art: &mut UnitArt,
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
        let entity = spawn_stack(commands, art, grid, team, army.faction, *stack, GridPos::new(col, row));
        order.push(entity);
    }
}

/// One stack = a translucent team-coloured plate (this is what you click), the animated unit
/// sprite of its kind in the faction's colour on top, a count/HP label below and a YOU/ENEMY tag.
pub fn spawn_stack(
    commands: &mut Commands,
    art: &mut UnitArt,
    grid: &BattleGrid,
    team: Team,
    faction: Faction,
    stack: Stack,
    pos: GridPos,
) -> Entity {
    let size = grid.tile_size * 0.9;
    let plate = team.color().to_srgba();
    let visual = UnitVisual::new(faction, stack.kind, team == Team::Enemy);
    commands
        .spawn((
            stack,
            team,
            faction,
            pos,
            Sprite::from_color(Color::srgba(plate.red, plate.green, plate.blue, 0.35), Vec2::splat(size)),
            Transform::from_translation(grid.to_world(pos).extend(1.0)),
            Pickable::default(),
            DespawnOnExit(GameState::Combat),
            children![
                art.bundle(visual, unit_scale(stack.kind), Vec2::new(0.0, -size * 0.3), 0.1),
                // Count and lead-unit HP, refreshed by hud::refresh_stack_labels.
                (
                    CountLabel,
                    Text2d::new(count_text(&stack)),
                    TextFont { font_size: FontSize::Px(11.0), ..default() },
                    TextColor(Color::WHITE),
                    Transform::from_xyz(0.0, -size / 2.0 + 7.0, 0.2),
                    Pickable::IGNORE,
                ),
                // YOU / ENEMY tag.
                (
                    Text2d::new(team.label()),
                    TextFont { font_size: FontSize::Px(12.0), ..default() },
                    TextColor(team.color()),
                    Transform::from_xyz(0.0, size / 2.0 + 8.0, 0.2),
                    Pickable::IGNORE,
                ),
            ],
        ))
        .observe(on_stack_click)
        .observe(on_stack_over)
        .observe(on_stack_out)
        .id()
}

fn player_may_act(active: &Query<&Team, With<ActiveStack>>, lock: &ActionLock) -> bool {
    !lock.is_locked() && matches!(active.single(), Ok(Team::Player))
}

fn on_tile_click(
    click: On<Pointer<Click>>,
    tiles: Query<&GridPos, With<Tile>>,
    active: Query<&Team, With<ActiveStack>>,
    lock: Res<ActionLock>,
    mut actions: MessageWriter<CombatAction>,
) {
    if click.event.button != PointerButton::Primary || !player_may_act(&active, &lock) {
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
    lock: Res<ActionLock>,
    mut actions: MessageWriter<CombatAction>,
) {
    if click.event.button != PointerButton::Primary || !player_may_act(&active, &lock) {
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
