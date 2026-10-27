//! The shop panel: `Tab` next to one of your shop buildings or an idle seller. Wood table,
//! avatar, animated preview of the unit, its stats on paper and a red BUY button. See agents.md §6.

use bevy::prelude::*;

use super::economy::Treasuries;
use super::interact::{Nearby, Touch, try_buy};
use super::world::WorldState;
use crate::assets::{AttackDir, GameAssets, UnitAnim, UnitArt};
use crate::faction::Faction;
use crate::states::GameState;
use crate::ui::{army_stat, avatar_index, gold_stat, panel, sliced};
use crate::units::{HERO_SLOTS, PlayerArmy, UnitKind};

/// What the open panel sells and where it was opened. `None` = closed.
#[derive(Resource, Default)]
pub struct OpenShop(pub Option<ShopTarget>);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ShopTarget {
    pub touch: Touch,
    pub unit: UnitKind,
    /// Colour of the avatar and the preview: the shop's owner, or Blue for idle sellers.
    pub faction: Faction,
    pub title: &'static str,
}

#[derive(Component)]
pub struct ShopRoot;
/// The gold number, the army number and the last message at the bottom of the panel.
#[derive(Component)]
pub struct ShopGold;
#[derive(Component)]
pub struct ShopArmy;
#[derive(Component)]
pub struct ShopMessage;
#[derive(Component)]
pub struct BuyButton;
#[derive(Component)]
pub struct CloseButton;
/// Steps an `ImageNode` atlas through its frames (the unit preview).
#[derive(Component)]
pub struct UiAnimated {
    frames: usize,
    timer: Timer,
}

impl UiAnimated {
    pub fn new(frames: usize) -> Self {
        Self { frames, timer: Timer::from_seconds(0.1, TimerMode::Repeating) }
    }
}

fn wood_table_mode() -> NodeImageMode {
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

/// What the thing the hero is touching would sell, if it is a shop of yours or an idle seller.
fn shop_for(world: &WorldState, touch: Option<Touch>) -> Option<ShopTarget> {
    match touch? {
        Touch::Shop { base, kind } if world.bases[base].owner.is_player() => Some(ShopTarget {
            touch: Touch::Shop { base, kind },
            unit: kind.sells()?,
            faction: world.bases[base].owner,
            title: kind.name(),
        }),
        Touch::Seller(i) => Some(ShopTarget {
            touch: Touch::Seller(i),
            unit: world.sellers[i].unit,
            faction: Faction::Reclaimers,
            title: "Idle seller",
        }),
        _ => None,
    }
}

/// Tab opens the panel next to a shop and closes it again; Esc, the X and walking away close it.
#[allow(clippy::too_many_arguments)]
pub fn toggle_shop(
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    assets: Res<GameAssets>,
    server: Res<AssetServer>,
    mut art: UnitArt,
    world: Res<WorldState>,
    nearby: Res<Nearby>,
    mut open: ResMut<OpenShop>,
    roots: Query<Entity, With<ShopRoot>>,
) {
    if let Some(target) = open.0 {
        let close = keys.any_just_pressed([KeyCode::Tab, KeyCode::Escape])
            || nearby.touch != Some(target.touch)
            || roots.is_empty();
        if close {
            open.0 = None;
        }
    }
    if open.0.is_none() {
        for root in &roots {
            commands.entity(root).despawn();
        }
        if keys.just_pressed(KeyCode::Tab)
            && let Some(target) = shop_for(&world, nearby.touch)
        {
            open.0 = Some(target);
            spawn_panel(&mut commands, &assets, &server, &mut art, target);
        }
    }
}

fn spawn_panel(commands: &mut Commands, assets: &GameAssets, server: &AssetServer, art: &mut UnitArt, target: ShopTarget) {
    let avatar: Handle<Image> =
        server.load(format!("ui_elements/ui_elements/human_avatars/avatars_{:02}.png", avatar_index(target.faction, target.unit)));
    let strip = art.strip(target.faction, target.unit, UnitAnim::Attack(AttackDir::Right));
    let s = target.unit.stats();
    let stats = format!(
        "{}\n\nHP     {}\nDEF    {}\nATK    {}-{}\nSPD    {}\nRANGE  {}-{}\n\nPrice  {} gold\nTakes 1 of your {} slots",
        target.unit.name(),
        s.health,
        s.defence,
        s.damage.0,
        s.damage.1,
        s.speed,
        s.range.0,
        s.range.1,
        target.unit.price(),
        HERO_SLOTS
    );
    let ink = TextColor(Color::srgb(0.22, 0.16, 0.10));

    commands.spawn((
        ShopRoot,
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        Pickable::IGNORE,
        DespawnOnExit(GameState::Overworld),
        children![(
            panel(assets.ui.wood_table.clone(), wood_table_mode()),
            // No fixed height: the table grows around its content.
            Node {
                width: Val::Px(860.0),
                // Padding clears the plank borders of the nine-slice (84 / 85 / 103 px).
                padding: UiRect::new(Val::Px(70.0), Val::Px(70.0), Val::Px(60.0), Val::Px(80.0)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(16.0),
                ..default()
            },
            children![
                // Header: avatar, title, close.
                (
                    Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, column_gap: Val::Px(16.0), ..default() },
                    Pickable::IGNORE,
                    children![
                        (
                            ImageNode::new(avatar).with_mode(NodeImageMode::Stretch),
                            Node { width: Val::Px(96.0), height: Val::Px(96.0), ..default() },
                            Pickable::IGNORE,
                        ),
                        (
                            Text::new(format!("{}  -  {}", target.title, target.unit.name())),
                            TextFont { font_size: FontSize::Px(28.0), ..default() },
                            TextColor(Color::WHITE),
                            Node { flex_grow: 1.0, ..default() },
                            Pickable::IGNORE,
                        ),
                        (
                            CloseButton,
                            ImageNode::new(assets.ui.icon_close.clone()).with_mode(NodeImageMode::Stretch),
                            Node { width: Val::Px(52.0), height: Val::Px(52.0), ..default() },
                        ),
                    ],
                ),
                // Body: animated preview (no background) and the stats paper.
                (
                    Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(24.0), align_items: AlignItems::Center, ..default() },
                    Pickable::IGNORE,
                    children![
                        (
                            UiAnimated::new(strip.frames),
                            ImageNode::from_atlas_image(strip.image.clone(), TextureAtlas { layout: strip.layout.clone(), index: 0 })
                                .with_mode(NodeImageMode::Stretch),
                            Node { width: Val::Px(260.0), height: Val::Px(260.0), flex_shrink: 0.0, ..default() },
                            Pickable::IGNORE,
                        ),
                        (
                            panel(assets.ui.paper.clone(), paper_mode()),
                            Node { flex_grow: 1.0, padding: UiRect::axes(Val::Px(40.0), Val::Px(32.0)), ..default() },
                            Pickable::IGNORE,
                            children![(
                                Text::new(stats),
                                TextFont { font_size: FontSize::Px(19.0), ..default() },
                                ink,
                                Pickable::IGNORE,
                            )],
                        ),
                    ],
                ),
                // Footer: status line and the BUY button.
                (
                    Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, column_gap: Val::Px(16.0), ..default() },
                    Pickable::IGNORE,
                    children![
                        (
                            Node {
                                flex_direction: FlexDirection::Row,
                                align_items: AlignItems::Center,
                                column_gap: Val::Px(18.0),
                                flex_grow: 1.0,
                                ..default()
                            },
                            Pickable::IGNORE,
                            children![
                                gold_stat(assets, ShopGold, 18.0),
                                army_stat(assets, ShopArmy, 18.0),
                                (
                                    ShopMessage,
                                    Text::new(""),
                                    TextFont { font_size: FontSize::Px(16.0), ..default() },
                                    TextColor(Color::WHITE),
                                    Pickable::IGNORE,
                                ),
                            ],
                        ),
                        (
                            BuyButton,
                            panel(assets.ui.button_regular.clone(), button_regular_mode()),
                            Node {
                                width: Val::Px(240.0),
                                height: Val::Px(74.0),
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                ..default()
                            },
                            children![(
                                Text::new(format!("BUY  -  {}g", target.unit.price())),
                                TextFont { font_size: FontSize::Px(24.0), ..default() },
                                TextColor(Color::WHITE),
                                Pickable::IGNORE,
                            )],
                        ),
                    ],
                ),
            ],
        )],
    ));
}

/// Attach click behaviour to freshly spawned buttons (the `children!` macro gives no entities).
pub fn wire_shop_buttons(
    mut commands: Commands,
    buy: Query<Entity, Added<BuyButton>>,
    close: Query<Entity, Added<CloseButton>>,
) {
    for entity in &buy {
        commands.entity(entity).observe(on_buy_press).observe(on_buy_release).observe(on_buy_click);
    }
    for entity in &close {
        commands.entity(entity).observe(on_close_click);
    }
}

fn on_buy_press(press: On<Pointer<Press>>, assets: Res<GameAssets>, mut images: Query<&mut ImageNode, With<BuyButton>>) {
    if let Ok(mut image) = images.get_mut(press.entity) {
        image.image = assets.ui.button_pressed.clone();
        image.image_mode = button_pressed_mode();
    }
}

fn on_buy_release(release: On<Pointer<Release>>, assets: Res<GameAssets>, mut images: Query<&mut ImageNode, With<BuyButton>>) {
    if let Ok(mut image) = images.get_mut(release.entity) {
        image.image = assets.ui.button_regular.clone();
        image.image_mode = button_regular_mode();
    }
}

fn on_buy_click(
    click: On<Pointer<Click>>,
    open: Res<OpenShop>,
    mut nearby: ResMut<Nearby>,
    mut treasuries: ResMut<Treasuries>,
    mut army: ResMut<PlayerArmy>,
) {
    if click.event.button != PointerButton::Primary {
        return;
    }
    if let Some(target) = open.0 {
        try_buy(target.unit, &mut nearby, &mut treasuries, &mut army);
    }
}

fn on_close_click(click: On<Pointer<Click>>, mut open: ResMut<OpenShop>) {
    if click.event.button == PointerButton::Primary {
        open.0 = None;
    }
}

pub fn refresh_shop_status(
    treasuries: Res<Treasuries>,
    army: Res<PlayerArmy>,
    nearby: Res<Nearby>,
    mut gold: Single<&mut Text, (With<ShopGold>, Without<ShopArmy>, Without<ShopMessage>)>,
    mut slots: Single<&mut Text, (With<ShopArmy>, Without<ShopGold>, Without<ShopMessage>)>,
    mut message: Single<&mut Text, (With<ShopMessage>, Without<ShopGold>, Without<ShopArmy>)>,
) {
    set_text(&mut gold, treasuries.player_gold().to_string());
    set_text(&mut slots, format!("{}/{}", army.0.slots(), HERO_SLOTS));
    set_text(&mut message, nearby.message.clone().unwrap_or_default());
}

fn set_text(text: &mut Text, value: String) {
    if text.0 != value {
        text.0 = value;
    }
}

pub fn animate_ui_images(time: Res<Time>, mut query: Query<(&mut UiAnimated, &mut ImageNode)>) {
    for (mut anim, mut image) in &mut query {
        if anim.timer.tick(time.delta()).just_finished()
            && let Some(atlas) = image.texture_atlas.as_mut()
        {
            atlas.index = (atlas.index + 1) % anim.frames.max(1);
        }
    }
}
