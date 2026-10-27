//! The castle panel: `Tab` next to your own castle. A small ribbon carries the five unit
//! avatars; the selected kind shows its blue avatar, the others their black one. TAKE and STORE
//! move that kind between the garrison and the hero. See agents.md §6.3.

use bevy::prelude::*;

use super::interact::{Nearby, Touch, transfer};
use super::world::WorldState;
use crate::assets::{GameAssets, UnitAnim, UnitArt};
use crate::faction::Faction;
use crate::states::GameState;
use crate::ui::{army_stat, avatar_index, panel, sliced};
use crate::units::{CASTLE_SLOTS, HERO_SLOTS, PlayerArmy, UnitKind};

/// Which base the panel is open for and which unit kind is selected. `None` = closed.
#[derive(Resource, Default)]
pub struct OpenCastle(pub Option<CastleTarget>);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CastleTarget {
    pub base: usize,
    pub selected: UnitKind,
}

#[derive(Component)]
pub struct CastleRoot;
/// One avatar on the ribbon; click to select that kind.
#[derive(Component)]
pub struct AvatarButton(pub UnitKind);
#[derive(Component)]
pub struct TakeButton;
#[derive(Component)]
pub struct StoreButton;
#[derive(Component)]
pub struct CastleClose;
/// Texts refreshed every frame.
#[derive(Component)]
pub struct CastleTitle;
#[derive(Component)]
pub struct CastleStats;
#[derive(Component)]
pub struct CastleArmy;
#[derive(Component)]
pub struct CastleMessage;
/// The animated preview of the selected kind.
#[derive(Component)]
pub struct CastlePreview;

const AVATAR_PX: f32 = 72.0;

fn small_ribbon_mode() -> NodeImageMode {
    sliced(62.0, 62.0, 0.0, 0.0)
}
fn button_regular_mode() -> NodeImageMode {
    sliced(45.0, 45.0, 47.0, 47.0)
}
fn button_pressed_mode() -> NodeImageMode {
    sliced(50.0, 50.0, 36.0, 49.0)
}

fn avatar_path(faction: Faction, unit: UnitKind) -> String {
    format!("ui_elements/ui_elements/human_avatars/avatars_{:02}.png", avatar_index(faction, unit))
}

/// Tab next to your own castle opens the panel and closes it; Esc, the X and walking away close it.
/// Number keys still work and also move the selection.
#[allow(clippy::too_many_arguments)]
pub fn toggle_castle(
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    assets: Res<GameAssets>,
    server: Res<AssetServer>,
    mut art: UnitArt,
    world: Res<WorldState>,
    nearby: Res<Nearby>,
    mut open: ResMut<OpenCastle>,
    roots: Query<Entity, With<CastleRoot>>,
) {
    if let Some(target) = open.0 {
        let close = keys.any_just_pressed([KeyCode::Tab, KeyCode::Escape])
            || nearby.touch != Some(Touch::Castle(target.base))
            || roots.is_empty();
        if close {
            open.0 = None;
        } else {
            // 1–5 select (and, through `castle_transfer`, also move) that kind.
            for (key, kind) in [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4, KeyCode::Digit5]
                .into_iter()
                .zip(UnitKind::ALL)
            {
                if keys.just_pressed(key) {
                    open.0 = Some(CastleTarget { selected: kind, ..target });
                }
            }
        }
    }
    if open.0.is_none() {
        for root in &roots {
            commands.entity(root).despawn();
        }
        if keys.just_pressed(KeyCode::Tab)
            && let Some(Touch::Castle(base)) = nearby.touch
            && world.bases[base].owner.is_player()
        {
            let target = CastleTarget { base, selected: UnitKind::Warrior };
            open.0 = Some(target);
            spawn_panel(&mut commands, &assets, &server, &mut art, target);
        }
    }
}

fn spawn_panel(commands: &mut Commands, assets: &GameAssets, server: &AssetServer, art: &mut UnitArt, target: CastleTarget) {
    let strip = art.strip(Faction::Reclaimers, target.selected, UnitAnim::Idle);
    let avatars: Vec<(UnitKind, Handle<Image>)> = UnitKind::ALL
        .into_iter()
        .map(|kind| {
            let colour = if kind == target.selected { Faction::Reclaimers } else { Faction::Black };
            (kind, server.load(avatar_path(colour, kind)))
        })
        .collect();
    let ink = TextColor(Color::srgb(0.22, 0.16, 0.10));

    commands
        .spawn((
            CastleRoot,
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
                    width: Val::Px(900.0),
                    padding: UiRect::new(Val::Px(70.0), Val::Px(70.0), Val::Px(60.0), Val::Px(80.0)),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(14.0),
                    ..default()
                },
            ))
            .with_children(|table| {
                // Header: title and the close X.
                table
                    .spawn((
                        Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, column_gap: Val::Px(16.0), ..default() },
                        Pickable::IGNORE,
                    ))
                    .with_children(|row| {
                        row.spawn((
                            CastleTitle,
                            Text::new(""),
                            TextFont { font_size: FontSize::Px(28.0), ..default() },
                            TextColor(Color::WHITE),
                            Node { flex_grow: 1.0, ..default() },
                            Pickable::IGNORE,
                        ));
                        row.spawn((
                            CastleClose,
                            ImageNode::new(assets.ui.icon_close.clone()).with_mode(NodeImageMode::Stretch),
                            Node { width: Val::Px(52.0), height: Val::Px(52.0), ..default() },
                        ));
                    });

                // The ribbon of avatars: selected = blue, others = black.
                table
                    .spawn((
                        panel(assets.ui.small_ribbons[&Faction::Reclaimers].clone(), small_ribbon_mode()),
                        Node {
                            height: Val::Px(96.0),
                            padding: UiRect::axes(Val::Px(70.0), Val::Px(0.0)),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            column_gap: Val::Px(18.0),
                            align_self: AlignSelf::Center,
                            ..default()
                        },
                        Pickable::IGNORE,
                    ))
                    .with_children(|ribbon| {
                        for (kind, image) in avatars {
                            ribbon.spawn((
                                AvatarButton(kind),
                                ImageNode::new(image).with_mode(NodeImageMode::Stretch),
                                Node { width: Val::Px(AVATAR_PX), height: Val::Px(AVATAR_PX), margin: UiRect::top(Val::Px(-14.0)), ..default() },
                            ));
                        }
                    });

                // Body: preview and the paper with the counts.
                table
                    .spawn((
                        Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(24.0), align_items: AlignItems::Center, ..default() },
                        Pickable::IGNORE,
                    ))
                    .with_children(|body| {
                        body.spawn((
                            CastlePreview,
                            super::shop::UiAnimated::new(strip.frames),
                            ImageNode::from_atlas_image(strip.image.clone(), TextureAtlas { layout: strip.layout.clone(), index: 0 })
                                .with_mode(NodeImageMode::Stretch),
                            Node { width: Val::Px(220.0), height: Val::Px(220.0), flex_shrink: 0.0, ..default() },
                            Pickable::IGNORE,
                        ));
                        body.spawn((
                            panel(assets.ui.paper.clone(), sliced(52.0, 52.0, 44.0, 45.0)),
                            Node { flex_grow: 1.0, padding: UiRect::axes(Val::Px(40.0), Val::Px(30.0)), ..default() },
                            Pickable::IGNORE,
                        ))
                        .with_children(|paper| {
                            paper.spawn((
                                CastleStats,
                                Text::new(""),
                                TextFont { font_size: FontSize::Px(19.0), ..default() },
                                ink,
                                Pickable::IGNORE,
                            ));
                        });
                    });

                // Footer: army count, message, TAKE and STORE.
                table
                    .spawn((
                        Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, column_gap: Val::Px(16.0), ..default() },
                        Pickable::IGNORE,
                    ))
                    .with_children(|footer| {
                        footer.spawn(army_stat(assets, CastleArmy, 18.0));
                        footer.spawn((
                            CastleMessage,
                            Text::new(""),
                            TextFont { font_size: FontSize::Px(16.0), ..default() },
                            TextColor(Color::WHITE),
                            Node { flex_grow: 1.0, ..default() },
                            Pickable::IGNORE,
                        ));
                        for (label, marker) in [("TAKE", true), ("STORE", false)] {
                            let mut button = footer.spawn((
                                panel(assets.ui.button_regular.clone(), button_regular_mode()),
                                Node {
                                    width: Val::Px(190.0),
                                    height: Val::Px(70.0),
                                    justify_content: JustifyContent::Center,
                                    align_items: AlignItems::Center,
                                    ..default()
                                },
                            ));
                            if marker {
                                button.insert(TakeButton);
                            } else {
                                button.insert(StoreButton);
                            }
                            button.with_children(|b| {
                                b.spawn((
                                    Text::new(label),
                                    TextFont { font_size: FontSize::Px(24.0), ..default() },
                                    TextColor(Color::WHITE),
                                    Pickable::IGNORE,
                                ));
                            });
                        }
                    });
            });
        });
}

/// Attach click behaviour to freshly spawned buttons.
pub fn wire_castle_buttons(
    mut commands: Commands,
    avatars: Query<Entity, Added<AvatarButton>>,
    take: Query<Entity, Added<TakeButton>>,
    store: Query<Entity, Added<StoreButton>>,
    close: Query<Entity, Added<CastleClose>>,
) {
    for entity in &avatars {
        commands.entity(entity).observe(on_avatar_click);
    }
    for entity in take.iter().chain(store.iter()) {
        commands.entity(entity).observe(on_press).observe(on_release).observe(on_move_click);
    }
    for entity in &close {
        commands.entity(entity).observe(on_close_click);
    }
}

fn on_avatar_click(click: On<Pointer<Click>>, avatars: Query<&AvatarButton>, mut open: ResMut<OpenCastle>) {
    if click.event.button != PointerButton::Primary {
        return;
    }
    if let (Ok(avatar), Some(target)) = (avatars.get(click.entity), open.0) {
        open.0 = Some(CastleTarget { selected: avatar.0, ..target });
    }
}

fn on_press(press: On<Pointer<Press>>, assets: Res<GameAssets>, mut images: Query<&mut ImageNode, Or<(With<TakeButton>, With<StoreButton>)>>) {
    if let Ok(mut image) = images.get_mut(press.entity) {
        image.image = assets.ui.button_pressed.clone();
        image.image_mode = button_pressed_mode();
    }
}

fn on_release(release: On<Pointer<Release>>, assets: Res<GameAssets>, mut images: Query<&mut ImageNode, Or<(With<TakeButton>, With<StoreButton>)>>) {
    if let Ok(mut image) = images.get_mut(release.entity) {
        image.image = assets.ui.button_regular.clone();
        image.image_mode = button_regular_mode();
    }
}

/// TAKE / STORE one unit of the selected kind; with Ctrl held, all of them.
#[allow(clippy::too_many_arguments)]
fn on_move_click(
    click: On<Pointer<Click>>,
    keys: Res<ButtonInput<KeyCode>>,
    stores: Query<(), With<StoreButton>>,
    open: Res<OpenCastle>,
    mut world: ResMut<WorldState>,
    mut army: ResMut<PlayerArmy>,
    mut nearby: ResMut<Nearby>,
) {
    if click.event.button != PointerButton::Primary {
        return;
    }
    let Some(target) = open.0 else { return };
    let store = stores.contains(click.entity);
    let all = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    transfer(&mut world, &mut army, &mut nearby, target.base, target.selected, store, all);
}

fn on_close_click(click: On<Pointer<Click>>, mut open: ResMut<OpenCastle>) {
    if click.event.button == PointerButton::Primary {
        open.0 = None;
    }
}

/// Keep avatars (blue = selected), preview, counts and message in step with the state.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn refresh_castle_panel(
    open: Res<OpenCastle>,
    server: Res<AssetServer>,
    mut art: UnitArt,
    world: Res<WorldState>,
    army: Res<PlayerArmy>,
    nearby: Res<Nearby>,
    mut avatars: Query<(&AvatarButton, &mut ImageNode), Without<CastlePreview>>,
    mut preview: Query<(&mut ImageNode, &mut super::shop::UiAnimated), With<CastlePreview>>,
    mut texts: ParamSet<(
        Single<&mut Text, With<CastleTitle>>,
        Single<&mut Text, With<CastleStats>>,
        Single<&mut Text, With<CastleArmy>>,
        Single<&mut Text, With<CastleMessage>>,
    )>,
    mut shown: Local<Option<UnitKind>>,
) {
    let Some(target) = open.0 else {
        *shown = None;
        return;
    };
    let base = &world.bases[target.base];
    let kind = target.selected;

    if *shown != Some(kind) {
        *shown = Some(kind);
        for (avatar, mut image) in &mut avatars {
            let colour = if avatar.0 == kind { Faction::Reclaimers } else { Faction::Black };
            image.image = server.load(avatar_path(colour, avatar.0));
        }
        let strip = art.strip(Faction::Reclaimers, kind, UnitAnim::Idle);
        for (mut image, mut anim) in &mut preview {
            image.image = strip.image.clone();
            image.texture_atlas = Some(TextureAtlas { layout: strip.layout.clone(), index: 0 });
            *anim = super::shop::UiAnimated::new(strip.frames);
        }
    }

    let s = kind.stats();
    let stats = format!(
        "{}\n\nIn the castle  {}\nWith you       {}\n\nHP {}   DEF {}   ATK {}-{}   SPD {}\n\n1-5 or click an avatar: select\nTAKE / STORE one; Ctrl+click: all",
        kind.name(),
        base.garrison.count_of(kind),
        army.0.count_of(kind),
        s.health,
        s.defence,
        s.damage.0,
        s.damage.1,
        s.speed
    );
    let title = format!("Castle of the {}   garrison {}/{}", base.owner.name(), base.garrison.slots(), CASTLE_SLOTS);
    let slots = format!("{}/{}", army.0.slots(), HERO_SLOTS);
    let message = nearby.message.clone().unwrap_or_default();
    for (i, value) in [title, stats, slots, message].into_iter().enumerate() {
        let mut text = match i {
            0 => texts.p0().into_inner(),
            1 => texts.p1().into_inner(),
            2 => texts.p2().into_inner(),
            _ => texts.p3().into_inner(),
        };
        if text.0 != value {
            text.0 = value;
        }
    }
}
