//! Everything that tells you *what is what* in combat:
//! green = yours, red = enemy; labels on every stack; turn order strip; info panel; log.

use std::collections::VecDeque;

use bevy::prelude::*;

use super::actions::Dying;
use super::grid::{BattleGrid, GridPos, Tile};
use super::turn::{AP_PER_TURN, ActionPoints, ActiveStack, CombatResult, TurnOrder};
use crate::faction::Faction;
use crate::states::GameState;
use crate::units::{ShieldUp, Stack, Team};

// ---------- resources ----------

/// The stack under the mouse, shown in the info panel.
#[derive(Resource, Default)]
pub struct Hovered(pub Option<Entity>);

/// Last few things that happened.
#[derive(Resource, Default)]
pub struct CombatLog {
    pub lines: VecDeque<String>,
}

impl CombatLog {
    const MAX_LINES: usize = 7;

    pub fn push(&mut self, line: impl Into<String>) {
        self.lines.push_back(line.into());
        while self.lines.len() > Self::MAX_LINES {
            self.lines.pop_front();
        }
    }
}

// ---------- markers ----------

/// Child `Text2d` under a stack showing count and HP.
#[derive(Component)]
pub struct CountLabel;

/// Yellow halo that follows the active stack.
#[derive(Component)]
pub struct ActiveMarker;

#[derive(Component)]
pub struct TurnStrip;

#[derive(Component)]
pub struct PanelHeader;

#[derive(Component)]
pub struct PanelText;

#[derive(Component)]
pub struct LogText;

pub fn count_text(stack: &Stack) -> String {
    format!("x{}  HP {}/{}", stack.count, stack.lead_hp, stack.stats().health)
}

fn panel_box() -> impl Bundle {
    (
        Node {
            padding: UiRect::all(Val::Px(10.0)),
            flex_direction: FlexDirection::Column,
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
        Pickable::IGNORE,
    )
}

pub fn spawn_hud(mut commands: Commands) {
    commands.spawn((
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::SpaceBetween,
            ..default()
        },
        Pickable::IGNORE,
        DespawnOnExit(GameState::Combat),
        children![
            // Top: turn order strip (chips are rebuilt by refresh_turn_strip).
            (
                TurnStrip,
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(6.0),
                    padding: UiRect::all(Val::Px(8.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
                Pickable::IGNORE,
            ),
            // Bottom: info panel on the left, log on the right.
            (
                Node {
                    flex_direction: FlexDirection::Row,
                    justify_content: JustifyContent::SpaceBetween,
                    align_items: AlignItems::End,
                    ..default()
                },
                Pickable::IGNORE,
                children![
                    (
                        panel_box(),
                        children![
                            (
                                PanelHeader,
                                Text::new(""),
                                TextFont { font_size: FontSize::Px(26.0), ..default() },
                                Pickable::IGNORE,
                            ),
                            (
                                PanelText,
                                Text::new(""),
                                TextFont { font_size: FontSize::Px(16.0), ..default() },
                                Pickable::IGNORE,
                            ),
                        ],
                    ),
                    (
                        panel_box(),
                        children![(
                            LogText,
                            Text::new(""),
                            TextFont { font_size: FontSize::Px(15.0), ..default() },
                            TextColor(Color::srgb(0.9, 0.9, 0.8)),
                            Pickable::IGNORE,
                        )],
                    ),
                ],
            ),
        ],
    ));
}

/// Keep the `x12  HP 7/10` label in sync with its stack.
pub fn refresh_stack_labels(
    changed: Query<(&Stack, &Children), Changed<Stack>>,
    mut labels: Query<&mut Text2d, With<CountLabel>>,
) {
    for (stack, children) in &changed {
        for child in children.iter() {
            if let Ok(mut text) = labels.get_mut(child) {
                text.0 = count_text(stack);
            }
        }
    }
}

/// Rebuild the strip of chips whenever the order, a stack or the active stack changes.
pub fn refresh_turn_strip(
    mut commands: Commands,
    order: Res<TurnOrder>,
    strip: Single<Entity, With<TurnStrip>>,
    stacks: Query<(&Stack, &Team, &Faction), Without<Dying>>,
    changed_stacks: Query<(), Changed<Stack>>,
    newly_active: Query<(), Added<ActiveStack>>,
    mut removed: RemovedComponents<Stack>,
) {
    let dirty = order.is_changed()
        || !changed_stacks.is_empty()
        || !newly_active.is_empty()
        || removed.read().count() > 0;
    if !dirty {
        return;
    }

    let strip = *strip;
    commands.entity(strip).despawn_related::<Children>();

    let title = commands
        .spawn((
            Text::new("Turn order:"),
            TextFont { font_size: FontSize::Px(16.0), ..default() },
            Pickable::IGNORE,
        ))
        .id();
    commands.entity(strip).add_child(title);

    for (i, entity) in order.order.iter().enumerate() {
        let Ok((stack, team, faction)) = stacks.get(*entity) else {
            continue; // destroyed
        };
        let current = i == order.current;
        let chip = commands
            .spawn((
                Node {
                    padding: UiRect::axes(Val::Px(10.0), Val::Px(5.0)),
                    border: UiRect::all(Val::Px(if current { 4.0 } else { 2.0 })),
                    ..default()
                },
                BackgroundColor(faction.color()),
                BorderColor::all(team.color()),
                Pickable::IGNORE,
                children![(
                    Text::new(format!(
                        "{}{}",
                        if current { "> " } else { "" },
                        stack.short_label()
                    )),
                    TextFont { font_size: FontSize::Px(if current { 22.0 } else { 15.0 }), ..default() },
                    TextColor(faction.text_color()),
                    Pickable::IGNORE,
                )],
            ))
            .id();
        commands.entity(strip).add_child(chip);
    }
}

fn describe(stack: &Stack, team: Team, faction: Faction, shield: Option<&ShieldUp>) -> String {
    let s = stack.stats();
    let shield = shield.map_or(String::new(), |sh| format!("  SHIELD +{}", sh.0));
    let range = if stack.is_ranged() { format!("  range {}-{}", s.range.0, s.range.1) } else { String::new() };
    format!(
        "{} {} x{}  ({})  HP {}/{}  def {}  speed {}  dmg {}-{} ({} roll){}{}",
        team.label(),
        stack.kind.name(),
        stack.count,
        faction.name(),
        stack.lead_hp,
        s.health,
        s.defence,
        s.speed,
        s.damage.0,
        s.damage.1,
        stack.rolling_units(),
        range,
        shield
    )
}

/// Bottom-left panel: whose turn, active stack stats, hovered stack, controls. Bottom-right: log.
#[allow(clippy::type_complexity)]
pub fn refresh_panel(
    active: Query<(&Stack, &Team, &Faction, &ActionPoints, Option<&ShieldUp>), With<ActiveStack>>,
    stacks: Query<(&Stack, &Team, &Faction, Option<&ShieldUp>)>,
    hovered: Res<Hovered>,
    log: Res<CombatLog>,
    result: Option<Res<CombatResult>>,
    mut header: Single<(&mut Text, &mut TextColor), With<PanelHeader>>,
    mut panel: Single<&mut Text, (With<PanelText>, Without<PanelHeader>, Without<LogText>)>,
    mut log_text: Single<&mut Text, (With<LogText>, Without<PanelHeader>, Without<PanelText>)>,
) {
    let (header_text, header_color) = &mut *header;
    let mut lines = Vec::new();

    match (result.as_deref(), active.single()) {
        (Some(CombatResult::Victory { .. }), _) => {
            header_text.0 = "VICTORY".to_string();
            header_color.0 = Team::Player.color();
        }
        (Some(CombatResult::Defeat { .. }), _) => {
            header_text.0 = "DEFEAT".to_string();
            header_color.0 = Team::Enemy.color();
        }
        (None, Ok((stack, team, faction, ap, shield))) => {
            header_text.0 = match team {
                Team::Player => "YOUR TURN".to_string(),
                Team::Enemy => "ENEMY TURN".to_string(),
            };
            header_color.0 = team.color();
            lines.push(format!("AP {}/{}   {}", ap.0, AP_PER_TURN, describe(stack, *team, *faction, shield)));
        }
        (None, Err(_)) => {
            header_text.0 = "...".to_string();
            header_color.0 = Color::WHITE;
        }
    }

    match hovered.0.and_then(|e| stacks.get(e).ok()) {
        Some((stack, team, faction, shield)) => {
            lines.push(format!("Hover: {}", describe(stack, *team, *faction, shield)))
        }
        None => lines.push("Hover a stack to inspect it.".to_string()),
    }
    lines.push(String::new());
    lines.push("Green border = YOUR stacks     Red border = ENEMY stacks".to_string());
    lines.push("Click a lit tile: Move   Click an enemy: Strike (melee: any of the 8 tiles around you; archers: 2-4 tiles away)".to_string());
    lines.push("Click an own touching stack of the same kind: Merge   1: Unstack   3: Shield up   Space: End turn".to_string());
    panel.0 = lines.join("\n");

    log_text.0 = log.lines.iter().cloned().collect::<Vec<_>>().join("\n");
}

/// Park the yellow halo behind the active stack and pulse it.
pub fn follow_active_marker(
    time: Res<Time>,
    active: Query<&Transform, (With<ActiveStack>, Without<ActiveMarker>)>,
    mut marker: Single<(&mut Transform, &mut Visibility), With<ActiveMarker>>,
) {
    let (transform, visibility) = &mut *marker;
    match active.single() {
        Ok(active) => {
            **visibility = Visibility::Visible;
            transform.translation = active.translation.truncate().extend(0.5);
            let pulse = 1.0 + 0.08 * (time.elapsed_secs() * 6.0).sin();
            transform.scale = Vec3::splat(pulse);
        }
        Err(_) => **visibility = Visibility::Hidden,
    }
}

/// Light up the tiles the player's active stack can move to.
pub fn highlight_reachable_tiles(
    grid: Res<BattleGrid>,
    active: Query<(&GridPos, &Team, &Stack, &ActionPoints), With<ActiveStack>>,
    stacks: Query<&GridPos, With<Stack>>,
    mut tiles: Query<(&GridPos, &mut Sprite), With<Tile>>,
) {
    let reach = match active.single() {
        Ok((pos, Team::Player, stack, ap)) if ap.0 > 0 => Some((*pos, stack.stats().speed as i32)),
        _ => None,
    };
    for (pos, mut sprite) in &mut tiles {
        let base = grid.base_color(*pos);
        let lit = reach.is_some_and(|(from, speed)| {
            let steps = from.steps_to(*pos);
            steps >= 1 && steps <= speed && !stacks.iter().any(|p| p == pos)
        });
        let color = if lit { lighten(base, 0.35) } else { base };
        if sprite.color != color {
            sprite.color = color;
        }
    }
}

fn lighten(color: Color, amount: f32) -> Color {
    let c = color.to_srgba();
    Color::srgb(
        c.red + (1.0 - c.red) * amount,
        c.green + (1.0 - c.green) * amount,
        c.blue + (1.0 - c.blue) * amount,
    )
}

/// Big centred banner when the battle is over.
pub fn show_result(mut commands: Commands, result: Res<CombatResult>) {
    let (text, color) = match *result {
        CombatResult::Victory { loot } => (
            format!("VICTORY\n+{loot} gold looted\n\nSpace: back to the overworld"),
            Team::Player.color(),
        ),
        CombatResult::Defeat { gold_lost } => (
            format!("DEFEAT\nyour army is lost and {gold_lost} gold taken\n\nSpace: back to the overworld"),
            Team::Enemy.color(),
        ),
    };
    commands.spawn((
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        Pickable::IGNORE,
        DespawnOnExit(GameState::Combat),
        children![(
            Node {
                padding: UiRect::all(Val::Px(30.0)),
                border: UiRect::all(Val::Px(4.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.8)),
            BorderColor::all(color),
            Pickable::IGNORE,
            children![(
                Text::new(text),
                TextFont { font_size: FontSize::Px(36.0), ..default() },
                TextColor(color),
                TextLayout::justify(Justify::Center),
                Pickable::IGNORE,
            )],
        )],
    ));
}
