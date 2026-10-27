//! Everything that tells you *what is what* in combat:
//! green = yours, red = enemy; labels on every stack; turn order strip; info panel; log.

use std::collections::VecDeque;

use bevy::prelude::*;

use super::actions::{ActionLock, CombatAction, Dying};
use super::grid::{BattleGrid, GridPos, Tile};
use super::turn::{AP_PER_TURN, ActionPoints, ActiveStack, CombatResult, TurnOrder};
use crate::assets::GameAssets;
use crate::faction::Faction;
use crate::states::GameState;
use crate::ui::{avatar_handle, finish_banner, panel, sliced};
use crate::units::{ShieldUp, Stack, Team, UnitKind};

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

/// Marker on the three action buttons of the bottom panel.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub enum ActionButton {
    Unstack,
    ShieldUp,
    EndTurn,
}

/// Avatar of the active stack in the bottom panel.
#[derive(Component)]
pub struct PanelAvatar;
/// The small ribbon behind the "<Kind>'s turn" header: blue on your turn, the enemy's kingdom
/// colour on theirs.
#[derive(Component)]
pub struct PanelHeaderRibbon;
/// `AP 2/3` next to the header.
#[derive(Component)]
pub struct PanelAp;
/// The hovered stack line.
#[derive(Component)]
pub struct HoverText;

fn wood() -> NodeImageMode {
    sliced(84.0, 84.0, 85.0, 103.0)
}
fn paper_mode() -> NodeImageMode {
    sliced(52.0, 52.0, 44.0, 45.0)
}
fn button_regular_mode() -> NodeImageMode {
    sliced(45.0, 45.0, 47.0, 47.0)
}
fn button_pressed_mode() -> NodeImageMode {
    sliced(50.0, 50.0, 36.0, 49.0)
}
fn small_ribbon_mode() -> NodeImageMode {
    sliced(62.0, 62.0, 0.0, 0.0)
}

/// Top strip (turn order on a wood bar), bottom left (wood table: whose turn, active stack on
/// paper, hovered stack, action buttons), bottom right (the log on paper).
pub fn spawn_hud(mut commands: Commands, assets: Res<GameAssets>) {
    let button = |label: &str, which: ActionButton| {
        (
            which,
            panel(assets.ui.button_regular.clone(), button_regular_mode()),
            Node {
                width: Val::Px(150.0),
                height: Val::Px(58.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            children![(
                Text::new(label.to_string()),
                TextFont { font_size: FontSize::Px(18.0), ..default() },
                TextColor(Color::WHITE),
                Pickable::IGNORE,
            )],
        )
    };

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
            // Top: the turn order on a wood bar (chips are rebuilt by refresh_turn_strip).
            (
                TurnStrip,
                panel(assets.ui.wood_table.clone(), wood()),
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(10.0),
                    padding: UiRect::new(Val::Px(60.0), Val::Px(60.0), Val::Px(16.0), Val::Px(30.0)),
                    align_self: AlignSelf::Center,
                    ..default()
                },
                Pickable::IGNORE,
            ),
            // Bottom row.
            (
                Node {
                    flex_direction: FlexDirection::Row,
                    justify_content: JustifyContent::SpaceBetween,
                    align_items: AlignItems::End,
                    column_gap: Val::Px(10.0),
                    ..default()
                },
                Pickable::IGNORE,
                children![
                    // Info panel.
                    (
                        panel(assets.ui.wood_table.clone(), wood()),
                        Node {
                            width: Val::Px(760.0),
                            padding: UiRect::new(Val::Px(60.0), Val::Px(60.0), Val::Px(40.0), Val::Px(52.0)),
                            flex_direction: FlexDirection::Column,
                            row_gap: Val::Px(8.0),
                            margin: UiRect::new(Val::Px(6.0), Val::Px(0.0), Val::Px(0.0), Val::Px(6.0)),
                            ..default()
                        },
                        Pickable::IGNORE,
                        children![
                            // Header: avatar, whose turn, AP.
                            (
                                Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, column_gap: Val::Px(14.0), ..default() },
                                Pickable::IGNORE,
                                children![
                                    (
                                        PanelAvatar,
                                        ImageNode::new(Handle::default()).with_mode(NodeImageMode::Stretch),
                                        Node { width: Val::Px(64.0), height: Val::Px(64.0), ..default() },
                                        Pickable::IGNORE,
                                    ),
                                    (
                                        PanelHeaderRibbon,
                                        panel(assets.ui.small_ribbons[&Faction::Reclaimers].clone(), small_ribbon_mode()),
                                        Node {
                                            height: Val::Px(60.0),
                                            min_width: Val::Px(260.0),
                                            padding: UiRect::axes(Val::Px(70.0), Val::Px(0.0)),
                                            justify_content: JustifyContent::Center,
                                            align_items: AlignItems::Center,
                                            ..default()
                                        },
                                        Pickable::IGNORE,
                                        children![(
                                            PanelHeader,
                                            Text::new(""),
                                            TextFont { font_size: FontSize::Px(22.0), ..default() },
                                            TextColor(Color::WHITE),
                                            Node { margin: UiRect::top(Val::Px(-6.0)), ..default() },
                                            Pickable::IGNORE,
                                        )],
                                    ),
                                    (
                                        PanelAp,
                                        Text::new(""),
                                        TextFont { font_size: FontSize::Px(22.0), ..default() },
                                        TextColor(Color::srgb(1.0, 0.9, 0.3)),
                                        Pickable::IGNORE,
                                    ),
                                ],
                            ),
                            // Active stack stats on paper.
                            (
                                panel(assets.ui.paper.clone(), paper_mode()),
                                Node { padding: UiRect::axes(Val::Px(30.0), Val::Px(18.0)), ..default() },
                                Pickable::IGNORE,
                                children![(
                                    PanelText,
                                    Text::new(""),
                                    TextFont { font_size: FontSize::Px(16.0), ..default() },
                                    TextColor(Color::srgb(0.22, 0.16, 0.10)),
                                    Pickable::IGNORE,
                                )],
                            ),
                            (
                                HoverText,
                                Text::new(""),
                                TextFont { font_size: FontSize::Px(15.0), ..default() },
                                TextColor(Color::srgb(0.92, 0.9, 0.82)),
                                Pickable::IGNORE,
                            ),
                            // Buttons.
                            (
                                Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(12.0), align_items: AlignItems::Center, ..default() },
                                Pickable::IGNORE,
                                children![
                                    button("UNSTACK  1", ActionButton::Unstack),
                                    button("SHIELD  3", ActionButton::ShieldUp),
                                    button("END TURN", ActionButton::EndTurn),
                                    (
                                        Text::new("click a lit tile: move   click an enemy: strike   click your own touching stack: merge"),
                                        TextFont { font_size: FontSize::Px(13.0), ..default() },
                                        TextColor(Color::srgb(0.85, 0.82, 0.75)),
                                        Node { max_width: Val::Px(220.0), ..default() },
                                        Pickable::IGNORE,
                                    ),
                                ],
                            ),
                        ],
                    ),
                    // Log on paper.
                    (
                        panel(assets.ui.paper.clone(), paper_mode()),
                        Node {
                            width: Val::Px(520.0),
                            padding: UiRect::axes(Val::Px(36.0), Val::Px(26.0)),
                            margin: UiRect::new(Val::Px(0.0), Val::Px(8.0), Val::Px(0.0), Val::Px(8.0)),
                            ..default()
                        },
                        Pickable::IGNORE,
                        children![(
                            LogText,
                            Text::new(""),
                            TextFont { font_size: FontSize::Px(14.0), ..default() },
                            TextColor(Color::srgb(0.22, 0.16, 0.10)),
                            Pickable::IGNORE,
                        )],
                    ),
                ],
            ),
        ],
    ));
}

/// Attach the click behaviour to the action buttons once they exist.
pub fn wire_combat_buttons(mut commands: Commands, buttons: Query<Entity, Added<ActionButton>>) {
    for entity in &buttons {
        commands.entity(entity).observe(on_button_press).observe(on_button_release).observe(on_button_click);
    }
}

fn on_button_press(press: On<Pointer<Press>>, assets: Res<GameAssets>, mut images: Query<&mut ImageNode, With<ActionButton>>) {
    if let Ok(mut image) = images.get_mut(press.entity) {
        image.image = assets.ui.button_pressed.clone();
        image.image_mode = button_pressed_mode();
    }
}

fn on_button_release(release: On<Pointer<Release>>, assets: Res<GameAssets>, mut images: Query<&mut ImageNode, With<ActionButton>>) {
    if let Ok(mut image) = images.get_mut(release.entity) {
        image.image = assets.ui.button_regular.clone();
        image.image_mode = button_regular_mode();
    }
}

/// The buttons do exactly what the keys 1 / 3 / Space do, and only on your turn.
fn on_button_click(
    click: On<Pointer<Click>>,
    buttons: Query<&ActionButton>,
    lock: Res<ActionLock>,
    active: Query<&Team, With<ActiveStack>>,
    mut actions: MessageWriter<CombatAction>,
) {
    if click.event.button != PointerButton::Primary || lock.is_locked() || !matches!(active.single(), Ok(Team::Player)) {
        return;
    }
    if let Ok(button) = buttons.get(click.entity) {
        actions.write(match button {
            ActionButton::Unstack => CombatAction::Unstack,
            ActionButton::ShieldUp => CombatAction::ShieldUp,
            ActionButton::EndTurn => CombatAction::EndTurn,
        });
    }
}

/// Keep the `x12  HP 7/10` label in sync with its stack.
pub fn refresh_stack_labels(
    changed: Query<(&Stack, &Children), Changed<Stack>>,
    mut labels: Query<&mut Text2d, With<CountLabel>>,
) {
    for (stack, children) in &changed {
        for child in children.iter() {
            if let Ok(mut label) = labels.get_mut(child) {
                label.0 = count_text(stack);
            }
        }
    }
}

/// The turn order as a queue of avatars (yours blue, the enemy's in its kingdom colour) with the
/// count under each. Whoever acts stands at the far left, bigger and on a small ribbon in its
/// team's colour; when it is done it drops to the far right and everyone shifts left.
#[allow(clippy::too_many_arguments)]
pub fn refresh_turn_strip(
    mut commands: Commands,
    assets: Res<GameAssets>,
    server: Res<AssetServer>,
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

    let n = order.order.len();
    // Rotate so the acting stack comes first.
    for step in 0..n {
        let i = (order.current + step) % n;
        let entity = order.order[i];
        let Ok((stack, team, faction)) = stacks.get(entity) else {
            continue; // destroyed
        };
        let current = step == 0;
        let size = if current { 64.0 } else { 44.0 };
        let ribbon_colour = match team {
            Team::Player => Faction::Reclaimers,
            Team::Enemy => Faction::Red,
        };
        let mut chip = commands.spawn((
            Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                padding: UiRect::axes(Val::Px(if current { 26.0 } else { 4.0 }), Val::Px(2.0)),
                ..default()
            },
            Pickable::IGNORE,
        ));
        if current {
            chip.insert(panel(assets.ui.small_ribbons[&ribbon_colour].clone(), small_ribbon_mode()));
        }
        chip.with_children(|c| {
            c.spawn((
                ImageNode::new(avatar_handle(&server, *faction, stack.kind)).with_mode(NodeImageMode::Stretch),
                Node { width: Val::Px(size), height: Val::Px(size), ..default() },
                Pickable::IGNORE,
            ));
            c.spawn((
                Text::new(format!("x{}", stack.count)),
                TextFont { font_size: FontSize::Px(if current { 16.0 } else { 13.0 }), ..default() },
                TextColor(team.color()),
                Node { margin: UiRect::top(Val::Px(-6.0)), ..default() },
                Pickable::IGNORE,
            ));
        });
        let chip = chip.id();
        commands.entity(strip).add_child(chip);
    }
}

fn describe(stack: &Stack, team: Team, faction: Faction, shield: Option<&ShieldUp>) -> String {
    let s = stack.stats();
    let shield = shield.map_or(String::new(), |sh| format!("   SHIELD +{}", sh.0));
    let range = if stack.is_ranged() { format!("   RANGE {}-{}", s.range.0, s.range.1) } else { String::new() };
    format!(
        "{} {} x{}  ({})\nHP {}/{}   DEF {}   SPD {}   ATK {}-{} ({} roll){}{}",
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

/// Bottom-left panel: whose turn, AP, the active stack's avatar and stats, the hovered stack.
/// Bottom-right: the log.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn refresh_panel(
    server: Res<AssetServer>,
    active: Query<(&Stack, &Team, &Faction, &ActionPoints, Option<&ShieldUp>), With<ActiveStack>>,
    stacks: Query<(&Stack, &Team, &Faction, Option<&ShieldUp>)>,
    hovered: Res<Hovered>,
    log: Res<CombatLog>,
    result: Option<Res<CombatResult>>,
    mut texts: ParamSet<(
        Single<(&mut Text, &mut TextColor), With<PanelHeader>>,
        Single<&mut Text, With<PanelAp>>,
        Single<&mut Text, With<PanelText>>,
        Single<&mut Text, With<HoverText>>,
        Single<&mut Text, With<LogText>>,
    )>,
    assets: Res<GameAssets>,
    mut avatar: Single<&mut ImageNode, (With<PanelAvatar>, Without<PanelHeaderRibbon>)>,
    mut ribbon: Single<&mut ImageNode, (With<PanelHeaderRibbon>, Without<PanelAvatar>)>,
    mut shown_avatar: Local<Option<(Faction, UnitKind)>>,
    mut shown_ribbon: Local<Option<Faction>>,
) {
    let mut header = texts.p0();
    let (header_text, header_color) = &mut *header;
    let mut ap_text = String::new();
    let mut stats = String::new();
    // Ribbon colour: blue for you, the enemy's kingdom colour for them.
    let mut ribbon_colour = Faction::Reclaimers;

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
            header_text.0 = format!("{}'s turn", stack.kind.name());
            header_color.0 = Color::WHITE;
            ribbon_colour = match team {
                Team::Player => Faction::Reclaimers,
                Team::Enemy => *faction,
            };
            ap_text = format!("AP {}/{}", ap.0, AP_PER_TURN);
            stats = describe(stack, *team, *faction, shield);
            let key = (*faction, stack.kind);
            if *shown_avatar != Some(key) {
                *shown_avatar = Some(key);
                avatar.image = avatar_handle(&server, *faction, stack.kind);
            }
        }
        (None, Err(_)) => {
            header_text.0 = "...".to_string();
            header_color.0 = Color::WHITE;
        }
    }

    drop(header);
    if *shown_ribbon != Some(ribbon_colour) {
        *shown_ribbon = Some(ribbon_colour);
        ribbon.image = assets.ui.small_ribbons[&ribbon_colour].clone();
    }

    let hover = match hovered.0.and_then(|e| stacks.get(e).ok()) {
        Some((stack, team, faction, shield)) => format!("Hover: {}", describe(stack, *team, *faction, shield).replace('\n', "   ")),
        None => "Hover a stack to inspect it.   Green = yours, red = enemy.".to_string(),
    };
    let log_lines = log.lines.iter().cloned().collect::<Vec<_>>().join("\n");

    for (i, value) in [ap_text, stats, hover, log_lines].into_iter().enumerate() {
        let mut text = match i {
            0 => texts.p1().into_inner(),
            1 => texts.p2().into_inner(),
            2 => texts.p3().into_inner(),
            _ => texts.p4().into_inner(),
        };
        if text.0 != value {
            text.0 = value;
        }
    }
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
pub fn show_result(mut commands: Commands, assets: Res<GameAssets>, result: Res<CombatResult>) {
    // Blue ribbon for a win, red for a loss.
    let (title, ribbon, text, color) = match *result {
        CombatResult::Victory { loot } => (
            "VICTORY",
            Faction::Reclaimers,
            format!("+{loot} gold looted\n\nSpace: back to the overworld"),
            Team::Player.color(),
        ),
        CombatResult::Defeat { gold_lost } => (
            "DEFEAT",
            Faction::Red,
            format!("Your army is lost and {gold_lost} gold taken\n\nSpace: back to the overworld"),
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
        children![finish_banner(&assets, ribbon, title, text, color)],
    ));
}
