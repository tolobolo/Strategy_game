//! The work panel: `Tab` next to a gold mine, forest or sheep pasture. Shows what is left and who
//! works there, and SEND / RECALL move Pawns between your castle and the site. See agents.md §6.7.

use bevy::prelude::*;

use super::interact::{Nearby, Touch};
use super::resources::{ResourceKind, SiteKind};
use super::world::WorldState;
use crate::assets::GameAssets;
use crate::cursor::ClickCursor;
use crate::states::GameState;
use crate::ui::{panel, resource_stat, sliced};
use crate::units::UnitKind;

/// The site the panel is open for. `None` = closed.
#[derive(Resource, Default)]
pub struct OpenWork(pub Option<usize>);

#[derive(Component)]
pub struct WorkRoot;
#[derive(Component)]
pub struct WorkTitle;
#[derive(Component)]
pub struct WorkInfo;
#[derive(Component)]
pub struct WorkMessage;
#[derive(Component)]
pub struct WorkStock;
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub enum WorkButton {
    Send,
    Recall,
}
#[derive(Component)]
pub struct WorkClose;

fn button_regular_mode() -> NodeImageMode {
    sliced(45.0, 45.0, 47.0, 47.0)
}
fn button_pressed_mode() -> NodeImageMode {
    sliced(50.0, 50.0, 36.0, 49.0)
}

/// Tab next to a site opens the panel and closes it; Esc, the X and walking away close it.
/// `1` sends a worker and `2` recalls one while it is open.
#[allow(clippy::too_many_arguments)]
pub fn toggle_work(
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    assets: Res<GameAssets>,
    mut nearby: ResMut<Nearby>,
    mut world: ResMut<WorldState>,
    mut open: ResMut<OpenWork>,
    roots: Query<Entity, With<WorkRoot>>,
) {
    let mut closed_now = false;
    if let Some(site) = open.0 {
        let close = keys.any_just_pressed([KeyCode::Tab, KeyCode::Escape])
            || nearby.touch != Some(Touch::Site(site))
            || roots.is_empty();
        if close {
            open.0 = None;
            closed_now = true;
        } else if keys.just_pressed(KeyCode::Digit1) {
            act(&mut world, &mut nearby, site, WorkButton::Send);
        } else if keys.just_pressed(KeyCode::Digit2) {
            act(&mut world, &mut nearby, site, WorkButton::Recall);
        }
    }
    if open.0.is_none() {
        for root in &roots {
            commands.entity(root).despawn();
        }
        if !closed_now
            && keys.just_pressed(KeyCode::Tab)
            && let Some(Touch::Site(site)) = nearby.touch
        {
            open.0 = Some(site);
            spawn_panel(&mut commands, &assets);
        }
    }
}

fn act(world: &mut WorldState, nearby: &mut Nearby, site: usize, button: WorkButton) {
    let result = match button {
        WorkButton::Send => world.send_worker(site),
        WorkButton::Recall => world.recall_worker(site),
    };
    match result {
        Ok(text) | Err(text) => nearby.say(text),
    }
}

fn spawn_panel(commands: &mut Commands, assets: &GameAssets) {
    let ink = TextColor(Color::srgb(0.22, 0.16, 0.10));
    commands
        .spawn((
            WorkRoot,
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
                    width: Val::Px(760.0),
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
                            WorkTitle,
                            Text::new(""),
                            TextFont { font_size: FontSize::Px(28.0), ..default() },
                            TextColor(Color::WHITE),
                            Node { flex_grow: 1.0, ..default() },
                            Pickable::IGNORE,
                        ));
                        row.spawn((
                            WorkClose,
                            ImageNode::new(assets.ui.icon_close.clone()).with_mode(NodeImageMode::Stretch),
                            Node { width: Val::Px(52.0), height: Val::Px(52.0), ..default() },
                        ));
                    });
                // What is left, who works here, where they come from.
                table
                    .spawn((
                        panel(assets.ui.paper.clone(), sliced(52.0, 52.0, 44.0, 45.0)),
                        Node { padding: UiRect::axes(Val::Px(40.0), Val::Px(30.0)), ..default() },
                        Pickable::IGNORE,
                    ))
                    .with_children(|paper| {
                        paper.spawn((WorkInfo, Text::new(""), TextFont { font_size: FontSize::Px(19.0), ..default() }, ink, Pickable::IGNORE));
                    });
                // Footer: your stock of this resource, the last message, SEND and RECALL.
                table
                    .spawn((
                        Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, column_gap: Val::Px(16.0), ..default() },
                        Pickable::IGNORE,
                    ))
                    .with_children(|footer| {
                        footer.spawn(resource_stat(assets.ui.icon_gold.clone(), WorkStock, 18.0));
                        footer.spawn((
                            WorkMessage,
                            Text::new(""),
                            TextFont { font_size: FontSize::Px(16.0), ..default() },
                            TextColor(Color::WHITE),
                            Node { flex_grow: 1.0, ..default() },
                            Pickable::IGNORE,
                        ));
                        for (text, button) in [("SEND  1", WorkButton::Send), ("RECALL  2", WorkButton::Recall)] {
                            footer
                                .spawn((
                                    button,
                                    panel(assets.ui.button_regular.clone(), button_regular_mode()),
                                    Node {
                                        width: Val::Px(170.0),
                                        height: Val::Px(70.0),
                                        justify_content: JustifyContent::Center,
                                        align_items: AlignItems::Center,
                                        ..default()
                                    },
                                ))
                                .with_children(|b| {
                                    b.spawn((
                                        Text::new(text),
                                        TextFont { font_size: FontSize::Px(22.0), ..default() },
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
pub fn wire_work_buttons(
    mut commands: Commands,
    buttons: Query<Entity, Added<WorkButton>>,
    close: Query<Entity, Added<WorkClose>>,
) {
    for entity in &buttons {
        commands.entity(entity).insert(ClickCursor).observe(on_press).observe(on_release).observe(on_click);
    }
    for entity in &close {
        commands.entity(entity).insert(ClickCursor).observe(on_close_click);
    }
}

fn on_press(press: On<Pointer<Press>>, assets: Res<GameAssets>, mut images: Query<&mut ImageNode, With<WorkButton>>) {
    if let Ok(mut image) = images.get_mut(press.entity) {
        image.image = assets.ui.button_pressed.clone();
        image.image_mode = button_pressed_mode();
    }
}

fn on_release(release: On<Pointer<Release>>, assets: Res<GameAssets>, mut images: Query<&mut ImageNode, With<WorkButton>>) {
    if let Ok(mut image) = images.get_mut(release.entity) {
        image.image = assets.ui.button_regular.clone();
        image.image_mode = button_regular_mode();
    }
}

fn on_click(
    click: On<Pointer<Click>>,
    buttons: Query<&WorkButton>,
    open: Res<OpenWork>,
    mut world: ResMut<WorldState>,
    mut nearby: ResMut<Nearby>,
) {
    if click.event.button != PointerButton::Primary {
        return;
    }
    if let (Some(site), Ok(button)) = (open.0, buttons.get(click.entity)) {
        act(&mut world, &mut nearby, site, *button);
    }
}

fn on_close_click(click: On<Pointer<Click>>, mut open: ResMut<OpenWork>) {
    if click.event.button == PointerButton::Primary {
        open.0 = None;
    }
}

/// Keep the title, the paper, your stock and the message in step with the world.
#[allow(clippy::type_complexity)]
pub fn refresh_work_panel(
    open: Res<OpenWork>,
    world: Res<WorldState>,
    treasuries: Res<super::economy::Treasuries>,
    assets: Res<GameAssets>,
    nearby: Res<Nearby>,
    mut texts: ParamSet<(
        Query<&mut Text, With<WorkTitle>>,
        Query<&mut Text, With<WorkInfo>>,
        Query<&mut Text, With<WorkStock>>,
        Query<&mut Text, With<WorkMessage>>,
    )>,
    stock_icon: Query<&ChildOf, With<WorkStock>>,
    mut icons: Query<&mut ImageNode>,
    children: Query<&Children>,
) {
    let Some(i) = open.0 else { return };
    let site = &world.sites[i];
    let kind = site.kind.resource();
    let title = format!("{}  -  {}", site.kind.name(), site.status());

    let castle = world.supplying_castle(i);
    let from = match castle {
        Some(c) => {
            let garrison = &world.bases[c].garrison;
            format!(
                "From your castle: {} Pawns ready, {} workers out, {}/{} slots used",
                garrison.count_of(UnitKind::Pawn),
                world.workers_out(c),
                garrison.slots() + world.workers_out(c),
                world.upgrades.castle_slots()
            )
        }
        None => "None of your castles can reach it.".to_string(),
    };
    let job = match site.kind {
        SiteKind::GoldMine => "Workers mine with a pickaxe",
        SiteKind::Forest => "Workers chop trees with an axe (stumps regrow)",
        SiteKind::Pasture => "Workers take sheep with a knife (new ones come)",
    };
    let info = format!(
        "Workers here: {}/{}\n{job}: {} {} per trip, {:.0} s of work.\n{from}\n\nSEND takes a Pawn from that castle; RECALL sends one home.",
        world.workers_at(i),
        world.max_workers(i),
        world.trip_load(site.kind),
        kind.name(),
        site.kind.work_secs(),
    );
    let stock = treasuries.amount(crate::faction::Faction::Reclaimers, kind).to_string();
    let message = nearby.message.clone().unwrap_or_default();
    for (n, value) in [title, info, stock, message].into_iter().enumerate() {
        let mut set = |mut text: Mut<Text>| {
            if text.0 != value {
                text.0 = value.clone();
            }
        };
        match n {
            0 => texts.p0().iter_mut().for_each(&mut set),
            1 => texts.p1().iter_mut().for_each(&mut set),
            2 => texts.p2().iter_mut().for_each(&mut set),
            _ => texts.p3().iter_mut().for_each(&mut set),
        }
    }

    // The stock icon matches the resource: coin, log or meat.
    let image = match kind {
        ResourceKind::Gold => &assets.ui.icon_gold,
        ResourceKind::Wood => &assets.ui.icon_wood,
        ResourceKind::Meat => &assets.ui.icon_meat,
    };
    for parent in &stock_icon {
        for child in children.get(parent.parent()).into_iter().flatten() {
            if let Ok(mut icon) = icons.get_mut(*child)
                && icon.image != *image
            {
                icon.image = image.clone();
            }
        }
    }
}
