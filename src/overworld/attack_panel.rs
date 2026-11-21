//! The attack panel: `Tab` next to a tower or castle you do not own. Like your army page, it lists
//! who defends it (and any of their armies close enough to join) and what you bring; ATTACK
//! (or `Enter`) starts your own siege: `PLAYER_SIEGE_SECS` of waiting next to it, then the fight.
//! Walking away calls the siege off. See agents.md §1.5 and §6.9.

use bevy::prelude::*;

use super::autoresolve::strength;
use super::interact::{Garrison, Nearby, Touch, helpers_of, start_fight_at};
use super::world::WorldState;
use crate::assets::GameAssets;
use crate::cursor::ClickCursor;
use crate::faction::Faction;
use crate::states::GameState;
use crate::ui::{avatar_index, panel, sliced};
use crate::units::{Army, PlayerArmy, UnitKind};

/// How long you besiege a tower or castle before the fight starts.
pub const PLAYER_SIEGE_SECS: f32 = 10.0;
const AVATAR_PX: f32 = 56.0;

/// Your siege in progress: which place, and seconds until the fight.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayerSiege {
    pub place: Garrison,
    pub left: f32,
}

/// The place the panel is open for. `None` = closed.
#[derive(Resource, Default)]
pub struct OpenAttack(pub Option<Garrison>);

/// What the panel was last built from; a change rebuilds it (like the army page).
#[derive(Resource, Default)]
pub struct AttackShown(Option<(Garrison, Vec<(UnitKind, u32)>, Vec<(UnitKind, u32)>, Vec<(UnitKind, u32)>)>);

#[derive(Component)]
pub struct AttackRoot;
#[derive(Component)]
pub struct AttackButton;
#[derive(Component)]
pub struct AttackButtonLabel;
#[derive(Component)]
pub struct AttackStatus;
#[derive(Component)]
pub struct AttackClose;

fn button_regular_mode() -> NodeImageMode {
    sliced(45.0, 45.0, 47.0, 47.0)
}
fn button_pressed_mode() -> NodeImageMode {
    sliced(50.0, 50.0, 36.0, 49.0)
}

/// The tower or castle you are next to, if it is not yours.
fn enemy_place(touch: Option<Touch>, world: &WorldState) -> Option<Garrison> {
    match touch? {
        Touch::Castle(i) if !world.bases[i].owner.is_player() => Some(Garrison::Castle(i)),
        Touch::Tower(i) if !world.towers[i].owner.is_player() => Some(Garrison::Tower(i)),
        _ => None,
    }
}

/// Count per kind, in the fixed unit order.
fn roster(army: &Army) -> Vec<(UnitKind, u32)> {
    UnitKind::ALL.into_iter().map(|k| (k, army.count_of(k))).filter(|(_, n)| *n > 0).collect()
}

/// The armies of the place's kingdom close enough to join, merged.
fn helpers_army(world: &WorldState, place: Garrison) -> Army {
    let mut army = Army::empty(place.owner(world));
    for i in helpers_of(world, place) {
        for stack in &world.armies[i].army.stacks {
            army.add(stack.kind, stack.count);
        }
    }
    army
}

/// Why you cannot start a siege of `place` now, or `Ok`.
fn can_besiege(world: &WorldState, player: &PlayerArmy, place: Garrison) -> Result<(), String> {
    if let Garrison::Tower(i) = place
        && let Some(taker) = world.tower_being_taken_by(i)
    {
        return Err(format!("The {} are taking it right now.", taker.name()));
    }
    let defended = !place.army(world).is_empty() || !helpers_army(world, place).is_empty();
    if defended && player.0.is_empty() {
        return Err("You need an army! Take units from your castle first.".to_string());
    }
    Ok(())
}

/// ATTACK: start your siege of `place` (or say why not).
fn begin(world: &mut WorldState, player: &PlayerArmy, nearby: &mut Nearby, place: Garrison) {
    if world.player_siege.is_some() {
        return;
    }
    match can_besiege(world, player, place) {
        Ok(()) => {
            world.player_siege = Some(PlayerSiege { place, left: PLAYER_SIEGE_SECS });
            nearby.say(format!("You besiege the {}: stay close for {PLAYER_SIEGE_SECS:.0} s.", place.noun()));
        }
        Err(why) => nearby.say(why),
    }
}

/// Tab next to a tower or castle you do not own opens the panel and closes it; Esc, the X and
/// walking away close it. `Enter` attacks.
#[allow(clippy::too_many_arguments)]
pub fn toggle_attack(
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    mut nearby: ResMut<Nearby>,
    mut world: ResMut<WorldState>,
    player: Res<PlayerArmy>,
    mut open: ResMut<OpenAttack>,
    mut shown: ResMut<AttackShown>,
    roots: Query<Entity, With<AttackRoot>>,
) {
    let here = enemy_place(nearby.touch, &world);
    let mut closed_now = false;
    if let Some(place) = open.0 {
        let close = keys.any_just_pressed([KeyCode::Tab, KeyCode::Escape]) || here != Some(place);
        if close {
            open.0 = None;
            closed_now = true;
        } else if keys.any_just_pressed([KeyCode::Enter, KeyCode::NumpadEnter]) {
            begin(&mut world, &player, &mut nearby, place);
        }
    }
    if open.0.is_none() {
        if !roots.is_empty() {
            for root in &roots {
                commands.entity(root).despawn();
            }
            shown.0 = None;
        }
        if !closed_now
            && keys.just_pressed(KeyCode::Tab)
            && let Some(place) = here
        {
            open.0 = Some(place);
        }
    }
}

/// Your siege ticks down while you stay next to the place; walking away calls it off. At zero the
/// fight starts: its garrison plus any of its kingdom's armies within reach (`helpers_of`), or the
/// place is simply taken if nobody defends it.
pub fn player_siege_tick(
    time: Res<Time>,
    mut commands: Commands,
    mut world: ResMut<WorldState>,
    mut nearby: ResMut<Nearby>,
    mut treasuries: ResMut<super::economy::Treasuries>,
    player: Res<PlayerArmy>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    let Some(siege) = world.player_siege else { return };
    if enemy_place(nearby.touch, &world) != Some(siege.place) {
        world.player_siege = None;
        nearby.say("You walked away: the siege is off.");
        return;
    }
    let left = siege.left - time.delta_secs();
    if left > 0.0 {
        world.player_siege = Some(PlayerSiege { left, ..siege });
        return;
    }
    world.player_siege = None;
    start_fight_at(&mut commands, &mut next_state, &mut nearby, &mut world, &mut treasuries, &player, siege.place);
}

fn avatar(server: &AssetServer, colour: Faction, unit: UnitKind) -> Handle<Image> {
    server.load(format!("ui_elements/ui_elements/human_avatars/avatars_{:02}.png", avatar_index(colour, unit)))
}

/// One column of rows like the army page: avatar + "Kind  xN".
fn rows(column: &mut ChildSpawnerCommands, server: &AssetServer, colour: Faction, troops: &[(UnitKind, u32)], empty: &str) {
    if troops.is_empty() {
        column.spawn((
            Text::new(empty),
            TextFont { font_size: FontSize::Px(19.0), ..default() },
            TextColor(Color::srgb(0.8, 0.78, 0.72)),
            Pickable::IGNORE,
        ));
    }
    for &(kind, count) in troops {
        column
            .spawn((
                Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, column_gap: Val::Px(12.0), ..default() },
                Pickable::IGNORE,
            ))
            .with_children(|row| {
                row.spawn((
                    ImageNode::new(avatar(server, colour, kind)).with_mode(NodeImageMode::Stretch),
                    Node { width: Val::Px(AVATAR_PX), height: Val::Px(AVATAR_PX), ..default() },
                    Pickable::IGNORE,
                ));
                row.spawn((
                    Text::new(format!("{}  x{count}", kind.name())),
                    TextFont { font_size: FontSize::Px(21.0), ..default() },
                    TextColor(Color::WHITE),
                    Pickable::IGNORE,
                ));
            });
    }
}

fn heading(column: &mut ChildSpawnerCommands, text: String) {
    column.spawn((
        Text::new(text),
        TextFont { font_size: FontSize::Px(22.0), ..default() },
        TextColor(Color::srgb(1.0, 0.9, 0.6)),
        Pickable::IGNORE,
    ));
}

fn spawn_panel(commands: &mut Commands, assets: &GameAssets, server: &AssetServer, world: &WorldState, place: Garrison, snapshot: &(Vec<(UnitKind, u32)>, Vec<(UnitKind, u32)>, Vec<(UnitKind, u32)>)) {
    let owner = place.owner(world);
    let (garrison, helpers, mine) = snapshot;
    let title = match place {
        Garrison::Castle(_) => format!("Castle of the {}", owner.name()),
        Garrison::Tower(_) => format!("Tower of the {}", owner.name()),
    };
    commands
        .spawn((
            AttackRoot,
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            Pickable::IGNORE,
            DespawnOnExit(GameState::Overworld),
        ))
        .with_children(|root| {
            root.spawn((
                panel(assets.ui.wood_table.clone(), sliced(84.0, 84.0, 85.0, 103.0)),
                Node {
                    width: Val::Px(940.0),
                    padding: UiRect::new(Val::Px(70.0), Val::Px(70.0), Val::Px(56.0), Val::Px(80.0)),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(14.0),
                    ..default()
                },
            ))
            .with_children(|table| {
                table
                    .spawn((Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, ..default() }, Pickable::IGNORE))
                    .with_children(|row| {
                        row.spawn((
                            Text::new(title),
                            TextFont { font_size: FontSize::Px(28.0), ..default() },
                            TextColor(Color::WHITE),
                            Node { flex_grow: 1.0, ..default() },
                            Pickable::IGNORE,
                        ));
                        row.spawn((
                            AttackClose,
                            ImageNode::new(assets.ui.icon_close.clone()).with_mode(NodeImageMode::Stretch),
                            Node { width: Val::Px(52.0), height: Val::Px(52.0), ..default() },
                        ));
                    });
                // Two columns: who defends it (and who will come to help), and what you bring.
                table
                    .spawn((Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(40.0), ..default() }, Pickable::IGNORE))
                    .with_children(|body| {
                        body.spawn((Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(6.0), width: Val::Px(380.0), ..default() }, Pickable::IGNORE))
                            .with_children(|col| {
                                heading(col, format!("Defending the {}", place.noun()));
                                rows(col, server, owner, garrison, "Nobody: it is undefended.");
                                if !helpers.is_empty() {
                                    heading(col, format!("+ {} army close by (joins)", owner.name()));
                                    rows(col, server, owner, helpers, "");
                                }
                            });
                        body.spawn((Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(6.0), flex_grow: 1.0, ..default() }, Pickable::IGNORE))
                            .with_children(|col| {
                                heading(col, "Your army".to_string());
                                rows(col, server, Faction::Reclaimers, mine, "No troops with you.");
                            });
                    });
                // Footer: the odds and the siege countdown, and ATTACK.
                table
                    .spawn((Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, column_gap: Val::Px(20.0), ..default() }, Pickable::IGNORE))
                    .with_children(|footer| {
                        footer.spawn((
                            AttackStatus,
                            Text::new(""),
                            TextFont { font_size: FontSize::Px(18.0), ..default() },
                            TextColor(Color::WHITE),
                            Node { flex_grow: 1.0, ..default() },
                            Pickable::IGNORE,
                        ));
                        footer
                            .spawn((
                                AttackButton,
                                panel(assets.ui.button_regular.clone(), button_regular_mode()),
                                Node {
                                    width: Val::Px(260.0),
                                    height: Val::Px(70.0),
                                    justify_content: JustifyContent::Center,
                                    align_items: AlignItems::Center,
                                    ..default()
                                },
                            ))
                            .with_children(|b| {
                                b.spawn((AttackButtonLabel, Text::new("ATTACK  Enter"), TextFont { font_size: FontSize::Px(24.0), ..default() }, TextColor(Color::WHITE), Pickable::IGNORE));
                            });
                    });
            });
        });
}

/// Rebuild the panel when what it shows changes; keep the status and the button label current.
#[allow(clippy::too_many_arguments)]
pub fn refresh_attack_panel(
    mut commands: Commands,
    assets: Res<GameAssets>,
    server: Res<AssetServer>,
    world: Res<WorldState>,
    player: Res<PlayerArmy>,
    nearby: Res<Nearby>,
    open: Res<OpenAttack>,
    mut shown: ResMut<AttackShown>,
    roots: Query<Entity, With<AttackRoot>>,
    mut status: Query<&mut Text, (With<AttackStatus>, Without<AttackButtonLabel>)>,
    mut label: Query<&mut Text, (With<AttackButtonLabel>, Without<AttackStatus>)>,
) {
    let Some(place) = open.0 else { return };
    let snapshot = (roster(place.army(&world)), roster(&helpers_army(&world, place)), roster(&player.0));
    let fresh = (place, snapshot.0.clone(), snapshot.1.clone(), snapshot.2.clone());
    if shown.0.as_ref() != Some(&fresh) {
        for root in &roots {
            commands.entity(root).despawn();
        }
        spawn_panel(&mut commands, &assets, &server, &world, place, &snapshot);
        shown.0 = Some(fresh);
        return; // the new texts exist from the next frame
    }

    let them = strength(place.army(&world)) + strength(&helpers_army(&world, place));
    let us = strength(&player.0);
    let odds = if them == 0.0 {
        "Undefended: besiege it to take it.".to_string()
    } else if us == 0.0 {
        "You have no army.".to_string()
    } else if us > them * 1.3 {
        "They look weaker than you.".to_string()
    } else if us * 1.3 < them {
        "They look stronger than you!".to_string()
    } else {
        "An even fight.".to_string()
    };
    let siege = world.player_siege.filter(|s| s.place == place);
    let text = match siege {
        Some(s) => format!("Besieging: the fight starts in {} s. Stay close!", s.left.ceil() as u32),
        None => match can_besiege(&world, &player, place) {
            Ok(()) => format!("{odds}\nATTACK: besiege it for {PLAYER_SIEGE_SECS:.0} s, then fight."),
            Err(why) => why,
        },
    };
    let text = match &nearby.message {
        Some(m) if siege.is_none() => format!("{text}\n{m}"),
        _ => text,
    };
    for mut t in &mut status {
        if t.0 != text {
            t.0 = text.clone();
        }
    }
    let button = match siege {
        Some(s) => format!("BESIEGING  {}s", s.left.ceil() as u32),
        None => "ATTACK  Enter".to_string(),
    };
    for mut t in &mut label {
        if t.0 != button {
            t.0 = button.clone();
        }
    }
}

pub fn wire_attack_buttons(mut commands: Commands, buttons: Query<Entity, Added<AttackButton>>, close: Query<Entity, Added<AttackClose>>) {
    for entity in &buttons {
        commands.entity(entity).insert(ClickCursor).observe(on_press).observe(on_release).observe(on_attack_click);
    }
    for entity in &close {
        commands.entity(entity).insert(ClickCursor).observe(on_close_click);
    }
}

fn on_press(press: On<Pointer<Press>>, assets: Res<GameAssets>, mut images: Query<&mut ImageNode, With<AttackButton>>) {
    if let Ok(mut image) = images.get_mut(press.entity) {
        image.image = assets.ui.button_pressed.clone();
        image.image_mode = button_pressed_mode();
    }
}

fn on_release(release: On<Pointer<Release>>, assets: Res<GameAssets>, mut images: Query<&mut ImageNode, With<AttackButton>>) {
    if let Ok(mut image) = images.get_mut(release.entity) {
        image.image = assets.ui.button_regular.clone();
        image.image_mode = button_regular_mode();
    }
}

fn on_attack_click(
    click: On<Pointer<Click>>,
    open: Res<OpenAttack>,
    mut world: ResMut<WorldState>,
    player: Res<PlayerArmy>,
    mut nearby: ResMut<Nearby>,
) {
    if click.event.button == PointerButton::Primary
        && let Some(place) = open.0
    {
        begin(&mut world, &player, &mut nearby, place);
    }
}

fn on_close_click(click: On<Pointer<Click>>, mut open: ResMut<OpenAttack>) {
    if click.event.button == PointerButton::Primary {
        open.0 = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn besieging_needs_an_army_unless_nobody_defends() {
        let mut world = WorldState::generate(3);
        world.armies.clear();
        let place = Garrison::Tower(0);
        let none = PlayerArmy::default();
        assert!(can_besiege(&world, &none, place).unwrap_err().contains("army"));
        let mut some = PlayerArmy::default();
        some.0.add(UnitKind::Warrior, 5);
        assert!(can_besiege(&world, &some, place).is_ok());
        world.towers[0].garrison = Army::empty(Faction::Black);
        assert!(can_besiege(&world, &none, place).is_ok(), "an undefended tower can be taken without an army");
    }

    #[test]
    fn attack_starts_a_siege_once() {
        let mut world = WorldState::generate(3);
        let mut some = PlayerArmy::default();
        some.0.add(UnitKind::Warrior, 5);
        let mut nearby = Nearby::default();
        begin(&mut world, &some, &mut nearby, Garrison::Tower(1));
        assert_eq!(world.player_siege, Some(PlayerSiege { place: Garrison::Tower(1), left: PLAYER_SIEGE_SECS }));
        begin(&mut world, &some, &mut nearby, Garrison::Tower(2));
        assert_eq!(world.player_siege.unwrap().place, Garrison::Tower(1), "one siege at a time");
    }
}
