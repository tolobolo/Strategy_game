//! The army page: `E` anywhere on the overworld. A wood table listing the troops the hero
//! carries, one row per kind going down, each with its black avatar; the selected row shows the
//! blue avatar. The selected kind idles top right with its stats underneath. See agents.md §6.4.

use bevy::prelude::*;

use super::interact::Nearby;
use super::shop::UiAnimated;
use crate::assets::{GameAssets, UnitAnim, UnitArt};
use crate::faction::Faction;
use crate::states::GameState;
use crate::ui::{army_stat, avatar_index, panel, sliced};
use crate::units::{HERO_SLOTS, PlayerArmy, UnitKind};

/// Open state: the selected kind. `None` = closed.
#[derive(Resource, Default)]
pub struct OpenArmy(pub Option<UnitKind>);

/// What the panel was last built from; a change rebuilds it.
#[derive(Resource, Default)]
pub struct ArmyPanelShown(Option<(UnitKind, Vec<(UnitKind, u32)>)>);

#[derive(Component)]
pub struct ArmyRoot;
/// One row of the list; click to select that kind.
#[derive(Component)]
pub struct ArmyRow(pub UnitKind);
#[derive(Component)]
pub struct ArmyClose;
#[derive(Component)]
pub struct ArmySlots;

const AVATAR_PX: f32 = 64.0;

fn avatar(server: &AssetServer, colour: Faction, unit: UnitKind) -> Handle<Image> {
    server.load(format!("ui_elements/ui_elements/human_avatars/avatars_{:02}.png", avatar_index(colour, unit)))
}

/// The kinds the hero carries, with counts, in the fixed unit order.
fn roster(army: &PlayerArmy) -> Vec<(UnitKind, u32)> {
    UnitKind::ALL
        .into_iter()
        .map(|k| (k, army.0.count_of(k)))
        .filter(|(_, n)| *n > 0)
        .collect()
}

/// `E` opens and closes the page; Esc and the X close it.
pub fn toggle_army(keys: Res<ButtonInput<KeyCode>>, army: Res<PlayerArmy>, mut open: ResMut<OpenArmy>) {
    if open.0.is_some() {
        if keys.any_just_pressed([KeyCode::KeyE, KeyCode::Escape]) {
            open.0 = None;
        }
    } else if keys.just_pressed(KeyCode::KeyE) {
        open.0 = Some(roster(&army).first().map_or(UnitKind::Warrior, |(k, _)| *k));
    }
}

/// Builds the page from the current state and rebuilds it whenever the selection or the army
/// changes (a bought or lost unit shows at once).
#[allow(clippy::too_many_arguments)]
pub fn refresh_army_panel(
    mut commands: Commands,
    assets: Res<GameAssets>,
    server: Res<AssetServer>,
    mut art: UnitArt,
    army: Res<PlayerArmy>,
    mut open: ResMut<OpenArmy>,
    mut shown: ResMut<ArmyPanelShown>,
    roots: Query<Entity, With<ArmyRoot>>,
) {
    let Some(mut selected) = open.0 else {
        if shown.0.is_some() || !roots.is_empty() {
            for root in &roots {
                commands.entity(root).despawn();
            }
            shown.0 = None;
        }
        return;
    };
    let troops = roster(&army);
    // Keep the selection on a kind you still have.
    if !troops.iter().any(|(k, _)| *k == selected)
        && let Some((first, _)) = troops.first()
    {
        selected = *first;
        open.0 = Some(selected);
    }
    let signature = (selected, troops.clone());
    if shown.0.as_ref() == Some(&signature) && !roots.is_empty() {
        return;
    }
    for root in &roots {
        commands.entity(root).despawn();
    }
    shown.0 = Some(signature);
    spawn_panel(&mut commands, &assets, &server, &mut art, &army, selected, &troops);
}

fn spawn_panel(
    commands: &mut Commands,
    assets: &GameAssets,
    server: &AssetServer,
    art: &mut UnitArt,
    army: &PlayerArmy,
    selected: UnitKind,
    troops: &[(UnitKind, u32)],
) {
    let strip = art.strip(Faction::Reclaimers, selected, UnitAnim::Idle);
    let s = selected.stats();
    let stats = if troops.is_empty() {
        "No troops.\n\nTake units from your castle (Tab there)\nor hire them at a shop or an idle seller.".to_string()
    } else {
        format!(
            "{}  x{}\n\nHP     {}\nDEF    {}\nATK    {}-{}\nSPD    {}\nRANGE  {}-{}",
            selected.name(),
            army.0.count_of(selected),
            s.health,
            s.defence,
            s.damage.0,
            s.damage.1,
            s.speed,
            s.range.0,
            s.range.1
        )
    };
    let ink = TextColor(Color::srgb(0.22, 0.16, 0.10));

    commands
        .spawn((
            ArmyRoot,
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
                    width: Val::Px(860.0),
                    min_height: Val::Px(560.0),
                    padding: UiRect::new(Val::Px(70.0), Val::Px(70.0), Val::Px(60.0), Val::Px(80.0)),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(14.0),
                    ..default()
                },
            ))
            .with_children(|table| {
                // Header: title, sword count, close X.
                table
                    .spawn((
                        Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, column_gap: Val::Px(20.0), ..default() },
                        Pickable::IGNORE,
                    ))
                    .with_children(|row| {
                        row.spawn((
                            Text::new("Your army"),
                            TextFont { font_size: FontSize::Px(28.0), ..default() },
                            TextColor(Color::WHITE),
                            Pickable::IGNORE,
                        ));
                        row.spawn(army_stat(assets, ArmySlots, 22.0));
                        row.spawn((Node { flex_grow: 1.0, ..default() }, Pickable::IGNORE));
                        row.spawn((
                            ArmyClose,
                            ImageNode::new(assets.ui.icon_close.clone()).with_mode(NodeImageMode::Stretch),
                            Node { width: Val::Px(52.0), height: Val::Px(52.0), ..default() },
                        ));
                    });

                // Body: the list on the left, preview + stats on the right.
                table
                    .spawn((
                        Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(30.0), align_items: AlignItems::FlexStart, ..default() },
                        Pickable::IGNORE,
                    ))
                    .with_children(|body| {
                        // List.
                        body.spawn((
                            Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(8.0), width: Val::Px(300.0), ..default() },
                            Pickable::IGNORE,
                        ))
                        .with_children(|list| {
                            for &(kind, count) in troops {
                                let colour = if kind == selected { Faction::Reclaimers } else { Faction::Black };
                                list.spawn((
                                    ArmyRow(kind),
                                    Node {
                                        flex_direction: FlexDirection::Row,
                                        align_items: AlignItems::Center,
                                        column_gap: Val::Px(12.0),
                                        padding: UiRect::axes(Val::Px(10.0), Val::Px(4.0)),
                                        ..default()
                                    },
                                    BackgroundColor(if kind == selected {
                                        Color::srgba(1.0, 1.0, 1.0, 0.12)
                                    } else {
                                        Color::NONE
                                    }),
                                ))
                                .with_children(|row| {
                                    row.spawn((
                                        ImageNode::new(avatar(server, colour, kind)).with_mode(NodeImageMode::Stretch),
                                        Node { width: Val::Px(AVATAR_PX), height: Val::Px(AVATAR_PX), ..default() },
                                        Pickable::IGNORE,
                                    ));
                                    row.spawn((
                                        Text::new(format!("{}  x{count}", kind.name())),
                                        TextFont { font_size: FontSize::Px(22.0), ..default() },
                                        TextColor(if kind == selected { Color::WHITE } else { Color::srgb(0.8, 0.78, 0.72) }),
                                        Pickable::IGNORE,
                                    ));
                                });
                            }
                        });

                        // Preview on top, stats paper under it.
                        body.spawn((
                            Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, row_gap: Val::Px(10.0), flex_grow: 1.0, ..default() },
                            Pickable::IGNORE,
                        ))
                        .with_children(|right| {
                            if !troops.is_empty() {
                                right.spawn((
                                    UiAnimated::new(strip.frames),
                                    ImageNode::from_atlas_image(strip.image.clone(), TextureAtlas { layout: strip.layout.clone(), index: 0 })
                                        .with_mode(NodeImageMode::Stretch),
                                    Node { width: Val::Px(200.0), height: Val::Px(200.0), ..default() },
                                    Pickable::IGNORE,
                                ));
                            }
                            right
                                .spawn((
                                    panel(assets.ui.paper.clone(), sliced(52.0, 52.0, 44.0, 45.0)),
                                    Node { width: Val::Percent(100.0), padding: UiRect::axes(Val::Px(40.0), Val::Px(30.0)), ..default() },
                                    Pickable::IGNORE,
                                ))
                                .with_children(|paper| {
                                    paper.spawn((
                                        Text::new(stats),
                                        TextFont { font_size: FontSize::Px(19.0), ..default() },
                                        ink,
                                        Pickable::IGNORE,
                                    ));
                                });
                        });
                    });
            });
        });
}

pub fn wire_army_buttons(mut commands: Commands, rows: Query<Entity, Added<ArmyRow>>, close: Query<Entity, Added<ArmyClose>>) {
    for entity in &rows {
        commands.entity(entity).observe(on_row_click);
    }
    for entity in &close {
        commands.entity(entity).observe(on_close_click);
    }
}

fn on_row_click(click: On<Pointer<Click>>, rows: Query<&ArmyRow>, mut open: ResMut<OpenArmy>) {
    if click.event.button == PointerButton::Primary
        && let Ok(row) = rows.get(click.entity)
        && open.0.is_some()
    {
        open.0 = Some(row.0);
    }
}

fn on_close_click(click: On<Pointer<Click>>, mut open: ResMut<OpenArmy>) {
    if click.event.button == PointerButton::Primary {
        open.0 = None;
    }
}

pub fn refresh_army_slots(army: Res<PlayerArmy>, _nearby: Res<Nearby>, mut slots: Single<&mut Text, With<ArmySlots>>) {
    let value = format!("{}/{}", army.0.slots(), HERO_SLOTS);
    if slots.0 != value {
        slots.0 = value;
    }
}
