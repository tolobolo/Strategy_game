//! The crafting panel: `C` at your House. The upgrade tree on the left (click a card, or move
//! with the arrow keys), the selected upgrade on the right with its price and BUY (or `Enter`).
//! See agents.md §1.8 and §6.8.

use bevy::prelude::*;
use bevy::sprite::{BorderRect, SliceScaleMode, TextureSlicer};

use super::buildings::BuildingKind;
use super::economy::Treasuries;
use super::interact::{Nearby, Touch};
use super::resources::ResourceKind;
use super::world::WorldState;
use crate::assets::GameAssets;
use crate::cursor::ClickCursor;
use crate::faction::Faction;
use crate::states::GameState;
use crate::ui::{icon_stat, panel, sliced};
use crate::upgrades::{Standing, book};

/// Card size and the grid step between card positions (`pos` in `upgrades.ron`).
const CARD: Vec2 = Vec2::new(122.0, 64.0);
const STEP: Vec2 = Vec2::new(132.0, 88.0);
/// Space around the cards inside the tree paper.
const TREE_PAD: Vec2 = Vec2::new(34.0, 30.0);
/// Grid size of the tree (x 0..=4, y 0..=5).
const GRID: Vec2 = Vec2::new(4.0, 5.0);
const LINE_PX: f32 = 4.0;
const INK: Color = Color::srgb(0.22, 0.16, 0.10);
const LINE: Color = Color::srgb(0.36, 0.25, 0.15);
const SHORT: Color = Color::srgb(0.75, 0.12, 0.10);

/// The panel is open (with the selected upgrade's index into `book().upgrades`). `None` = closed.
#[derive(Resource, Default)]
pub struct OpenCraft(pub Option<usize>);

#[derive(Component)]
pub struct CraftRoot;
#[derive(Component)]
pub struct CraftCard(pub usize);
#[derive(Component)]
pub struct CardTier(pub usize);
#[derive(Component)]
pub struct CraftFrame;
#[derive(Component)]
pub struct CraftName;
#[derive(Component)]
pub struct CraftInfo;
#[derive(Component)]
pub struct CraftPrice(pub ResourceKind);
#[derive(Component)]
pub struct CraftStock(pub ResourceKind);
#[derive(Component)]
pub struct CraftBuy;
#[derive(Component)]
pub struct CraftBuyLabel;
#[derive(Component)]
pub struct CraftMessage;
#[derive(Component)]
pub struct CraftClose;

fn button_regular_mode() -> NodeImageMode {
    sliced(45.0, 45.0, 47.0, 47.0)
}
fn button_pressed_mode() -> NodeImageMode {
    sliced(50.0, 50.0, 36.0, 49.0)
}

/// Top-left corner of a card in the tree, from its grid position.
fn card_at(pos: (f32, f32)) -> Vec2 {
    TREE_PAD + Vec2::new(pos.0, pos.1) * STEP
}

fn tree_size() -> Vec2 {
    TREE_PAD * 2.0 + GRID * STEP + CARD
}

/// Standing at your own House?
fn at_house(nearby: &Nearby, world: &WorldState) -> bool {
    matches!(nearby.touch, Some(Touch::Shop { base, kind: BuildingKind::House }) if world.bases[base].owner.is_player())
}

/// `C` at your House opens the panel; `C`, Esc, the X and walking away close it. While open the
/// arrow keys move the selection to the nearest card that way and `Enter` buys.
#[allow(clippy::too_many_arguments)]
pub fn toggle_craft(
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    assets: Res<GameAssets>,
    server: Res<AssetServer>,
    mut nearby: ResMut<Nearby>,
    mut world: ResMut<WorldState>,
    mut treasuries: ResMut<Treasuries>,
    mut open: ResMut<OpenCraft>,
    roots: Query<Entity, With<CraftRoot>>,
) {
    let mut closed_now = false;
    if let Some(selected) = open.0 {
        let close = keys.any_just_pressed([KeyCode::KeyC, KeyCode::Escape]) || !at_house(&nearby, &world) || roots.is_empty();
        if close {
            open.0 = None;
            closed_now = true;
        } else {
            for (key, dir) in [
                (KeyCode::ArrowLeft, Vec2::NEG_X),
                (KeyCode::ArrowRight, Vec2::X),
                (KeyCode::ArrowUp, Vec2::NEG_Y),
                (KeyCode::ArrowDown, Vec2::Y),
            ] {
                if keys.just_pressed(key)
                    && let Some(next) = neighbour(selected, dir)
                {
                    open.0 = Some(next);
                }
            }
            if keys.any_just_pressed([KeyCode::Enter, KeyCode::NumpadEnter]) {
                buy(selected, &mut world, &mut treasuries, &mut nearby);
            }
        }
    }
    if open.0.is_none() {
        for root in &roots {
            commands.entity(root).despawn();
        }
        if !closed_now && keys.just_pressed(KeyCode::KeyC) && at_house(&nearby, &world) {
            open.0 = Some(0);
            spawn_panel(&mut commands, &assets, &server);
        }
    }
}

/// The nearest card from `from` in direction `dir` (within 60° of it).
fn neighbour(from: usize, dir: Vec2) -> Option<usize> {
    let ups = &book().upgrades;
    let here = Vec2::new(ups[from].pos.0, ups[from].pos.1);
    ups.iter()
        .enumerate()
        .filter(|(i, _)| *i != from)
        .map(|(i, u)| (i, Vec2::new(u.pos.0, u.pos.1) - here))
        .filter(|(_, d)| d.normalize_or_zero().dot(dir) > 0.5)
        .min_by(|a, b| a.1.length().total_cmp(&b.1.length()))
        .map(|(i, _)| i)
}

fn buy(index: usize, world: &mut WorldState, treasuries: &mut Treasuries, nearby: &mut Nearby) {
    match world.upgrades.buy(&book().upgrades[index], treasuries) {
        Ok(text) | Err(text) => nearby.say(text),
    }
}

/// The 9-sliced corner frame (`cursor_04`) around the selected card.
fn frame_mode() -> NodeImageMode {
    NodeImageMode::Sliced(TextureSlicer {
        border: BorderRect::all(40.0),
        center_scale_mode: SliceScaleMode::Stretch,
        sides_scale_mode: SliceScaleMode::Stretch,
        max_corner_scale: 0.5,
    })
}

/// A small paper card: corners scaled down to fit the card.
fn card_mode() -> NodeImageMode {
    NodeImageMode::Sliced(TextureSlicer {
        border: BorderRect { min_inset: Vec2::new(52.0, 44.0), max_inset: Vec2::new(52.0, 45.0) },
        center_scale_mode: SliceScaleMode::Stretch,
        sides_scale_mode: SliceScaleMode::Stretch,
        max_corner_scale: 0.4,
    })
}

fn absolute(at: Vec2, size: Vec2) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: Val::Px(at.x),
        top: Val::Px(at.y),
        width: Val::Px(size.x),
        height: Val::Px(size.y),
        ..default()
    }
}

/// Open the panel without the key (the screenshot tool uses it).
pub fn open_now(commands: &mut Commands, assets: &GameAssets, server: &AssetServer, open: &mut OpenCraft, selected: usize) {
    open.0 = Some(selected);
    spawn_panel(commands, assets, server);
}

fn spawn_panel(commands: &mut Commands, assets: &GameAssets, server: &AssetServer) {
    let ups = &book().upgrades;
    let tree = tree_size();
    let frame_image: Handle<Image> = server.load("ui_elements/ui_elements/cursors/cursor_04.png");
    let resource_icon = |kind: ResourceKind| match kind {
        ResourceKind::Wood => assets.ui.icon_wood.clone(),
        ResourceKind::Meat => assets.ui.icon_meat.clone(),
        ResourceKind::Gold => assets.ui.icon_gold.clone(),
    };
    let kinds = [ResourceKind::Wood, ResourceKind::Meat, ResourceKind::Gold];

    commands
        .spawn((
            CraftRoot,
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
                    padding: UiRect::new(Val::Px(64.0), Val::Px(64.0), Val::Px(44.0), Val::Px(70.0)),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(10.0),
                    ..default()
                },
            ))
            .with_children(|table| {
                // Header: title and the close X.
                table
                    .spawn((Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, ..default() }, Pickable::IGNORE))
                    .with_children(|row| {
                        row.spawn((
                            Text::new("CRAFTING  -  your House"),
                            TextFont { font_size: FontSize::Px(28.0), ..default() },
                            TextColor(Color::WHITE),
                            Node { flex_grow: 1.0, ..default() },
                            Pickable::IGNORE,
                        ));
                        row.spawn((
                            CraftClose,
                            ImageNode::new(assets.ui.icon_close.clone()).with_mode(NodeImageMode::Stretch),
                            Node { width: Val::Px(48.0), height: Val::Px(48.0), ..default() },
                        ));
                    });

                table
                    .spawn((Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(18.0), ..default() }, Pickable::IGNORE))
                    .with_children(|body| {
                        // Left: the tree on paper. Lines first, so the cards sit on top of them.
                        body.spawn((
                            panel(assets.ui.paper.clone(), sliced(52.0, 52.0, 44.0, 45.0)),
                            Node { width: Val::Px(tree.x), height: Val::Px(tree.y), ..default() },
                            Pickable::IGNORE,
                        ))
                        .with_children(|paper| {
                            for u in ups {
                                let Some(parent) = u.needs.as_ref().and_then(|p| book().get(p)) else { continue };
                                for (at, size) in elbow(card_at(parent.pos), card_at(u.pos)) {
                                    paper.spawn((absolute(at, size), BackgroundColor(LINE), Pickable::IGNORE));
                                }
                            }
                            for (i, u) in ups.iter().enumerate() {
                                paper
                                    .spawn((
                                        CraftCard(i),
                                        panel(assets.ui.paper.clone(), card_mode()),
                                        Node {
                                        flex_direction: FlexDirection::Column,
                                        justify_content: JustifyContent::Center,
                                        align_items: AlignItems::Center,
                                            ..absolute(card_at(u.pos), CARD)
                                        },
                                    ))
                                    .with_children(|card| {
                                        card.spawn((
                                            Text::new(u.name.clone()),
                                            TextFont { font_size: FontSize::Px(13.0), ..default() },
                                            TextColor(INK),
                                            TextLayout::justify(Justify::Center),
                                            Pickable::IGNORE,
                                        ));
                                        card.spawn((
                                            CardTier(i),
                                            Text::new(format!("Tier {}", u.tier)),
                                            TextFont { font_size: FontSize::Px(11.0), ..default() },
                                            TextColor(LINE),
                                            Pickable::IGNORE,
                                        ));
                                    });
                            }
                            paper.spawn((
                                CraftFrame,
                                ImageNode::new(frame_image.clone()).with_mode(frame_mode()),
                                absolute(Vec2::ZERO, CARD + Vec2::splat(16.0)),
                                Pickable::IGNORE,
                            ));
                        });

                        // Right: the selected upgrade, its price and BUY.
                        body.spawn((
                            panel(assets.ui.paper.clone(), sliced(52.0, 52.0, 44.0, 45.0)),
                            Node {
                                width: Val::Px(380.0),
                                height: Val::Px(tree.y),
                                padding: UiRect::axes(Val::Px(36.0), Val::Px(32.0)),
                                flex_direction: FlexDirection::Column,
                                row_gap: Val::Px(14.0),
                                ..default()
                            },
                            Pickable::IGNORE,
                        ))
                        .with_children(|side| {
                            side.spawn((CraftName, Text::new(""), TextFont { font_size: FontSize::Px(24.0), ..default() }, TextColor(INK), Pickable::IGNORE));
                            side.spawn((
                                CraftInfo,
                                Text::new(""),
                                TextFont { font_size: FontSize::Px(17.0), ..default() },
                                TextColor(INK),
                                Node { flex_grow: 1.0, ..default() },
                                Pickable::IGNORE,
                            ));
                            side.spawn((Text::new("Price"), TextFont { font_size: FontSize::Px(17.0), ..default() }, TextColor(INK), Pickable::IGNORE));
                            side.spawn((Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(18.0), ..default() }, Pickable::IGNORE))
                                .with_children(|row| {
                                    for kind in kinds {
                                        row.spawn(icon_stat(resource_icon(kind), CraftPrice(kind), 20.0));
                                    }
                                });
                            side.spawn((
                                CraftBuy,
                                panel(assets.ui.button_regular.clone(), button_regular_mode()),
                                Node {
                                    height: Val::Px(70.0),
                                    justify_content: JustifyContent::Center,
                                    align_items: AlignItems::Center,
                                    ..default()
                                },
                            ))
                            .with_children(|b| {
                                b.spawn((CraftBuyLabel, Text::new("BUY"), TextFont { font_size: FontSize::Px(24.0), ..default() }, TextColor(Color::WHITE), Pickable::IGNORE));
                            });
                        });
                    });

                // Footer: what you have, and the last message.
                table
                    .spawn((Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, column_gap: Val::Px(22.0), ..default() }, Pickable::IGNORE))
                    .with_children(|footer| {
                        footer.spawn((Text::new("You have"), TextFont { font_size: FontSize::Px(18.0), ..default() }, TextColor(Color::WHITE), Pickable::IGNORE));
                        for kind in kinds {
                            footer.spawn(icon_stat(resource_icon(kind), CraftStock(kind), 18.0));
                        }
                        footer.spawn((
                            CraftMessage,
                            Text::new(""),
                            TextFont { font_size: FontSize::Px(16.0), ..default() },
                            TextColor(Color::WHITE),
                            Pickable::IGNORE,
                        ));
                    });
            });
        });
}

/// Line pieces (top-left, size) joining a parent card to a child card: down (or up) from the
/// parent to half way, across, then on to the child, like the branches in the picture.
fn elbow(parent: Vec2, child: Vec2) -> Vec<(Vec2, Vec2)> {
    let half = CARD / 2.0;
    let (p, c) = (parent + half, child + half);
    let down = c.y > p.y;
    let y1 = if down { p.y + half.y } else { p.y - half.y };
    let y2 = if down { c.y - half.y } else { c.y + half.y };
    let mid = (y1 + y2) / 2.0;
    let w = LINE_PX / 2.0;
    let vertical = |x: f32, a: f32, b: f32| (Vec2::new(x - w, a.min(b)), Vec2::new(LINE_PX, (a - b).abs() + w));
    let mut pieces = vec![vertical(p.x, y1, mid), vertical(c.x, mid, y2)];
    if (p.x - c.x).abs() > 0.5 {
        pieces.push((Vec2::new(p.x.min(c.x) - w, mid - w), Vec2::new((p.x - c.x).abs() + LINE_PX, LINE_PX)));
    }
    pieces
}

/// Attach click behaviour to freshly spawned cards and buttons.
pub fn wire_craft_buttons(
    mut commands: Commands,
    cards: Query<Entity, Added<CraftCard>>,
    buy: Query<Entity, Added<CraftBuy>>,
    close: Query<Entity, Added<CraftClose>>,
) {
    for entity in &cards {
        commands.entity(entity).insert(ClickCursor).observe(on_card_click);
    }
    for entity in &buy {
        commands.entity(entity).insert(ClickCursor).observe(on_press).observe(on_release).observe(on_buy_click);
    }
    for entity in &close {
        commands.entity(entity).insert(ClickCursor).observe(on_close_click);
    }
}

fn on_card_click(click: On<Pointer<Click>>, cards: Query<&CraftCard>, mut open: ResMut<OpenCraft>) {
    if click.event.button == PointerButton::Primary
        && open.0.is_some()
        && let Ok(card) = cards.get(click.entity)
    {
        open.0 = Some(card.0);
    }
}

fn on_press(press: On<Pointer<Press>>, assets: Res<GameAssets>, mut images: Query<&mut ImageNode, With<CraftBuy>>) {
    if let Ok(mut image) = images.get_mut(press.entity) {
        image.image = assets.ui.button_pressed.clone();
        image.image_mode = button_pressed_mode();
    }
}

fn on_release(release: On<Pointer<Release>>, assets: Res<GameAssets>, mut images: Query<&mut ImageNode, With<CraftBuy>>) {
    if let Ok(mut image) = images.get_mut(release.entity) {
        image.image = assets.ui.button_regular.clone();
        image.image_mode = button_regular_mode();
    }
}

fn on_buy_click(
    click: On<Pointer<Click>>,
    open: Res<OpenCraft>,
    mut world: ResMut<WorldState>,
    mut treasuries: ResMut<Treasuries>,
    mut nearby: ResMut<Nearby>,
) {
    if click.event.button == PointerButton::Primary
        && let Some(selected) = open.0
    {
        buy(selected, &mut world, &mut treasuries, &mut nearby);
    }
}

fn on_close_click(click: On<Pointer<Click>>, mut open: ResMut<OpenCraft>) {
    if click.event.button == PointerButton::Primary {
        open.0 = None;
    }
}

fn set_text(text: &mut Text, value: String) {
    if text.0 != value {
        text.0 = value;
    }
}

/// Cards tinted by standing (blue = yours, plain = open, dark = locked), the frame on the selected
/// card, and the right side, prices and stock kept current.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn refresh_craft_panel(
    open: Res<OpenCraft>,
    world: Res<WorldState>,
    treasuries: Res<Treasuries>,
    nearby: Res<Nearby>,
    mut cards: Query<(&CraftCard, &mut ImageNode), Without<CraftFrame>>,
    mut frame: Query<&mut Node, With<CraftFrame>>,
    mut texts: ParamSet<(
        Query<(&CardTier, &mut Text)>,
        Query<&mut Text, With<CraftName>>,
        Query<&mut Text, With<CraftInfo>>,
        Query<(&CraftPrice, &mut Text, &mut TextColor)>,
        Query<(&CraftStock, &mut Text)>,
        Query<&mut Text, With<CraftBuyLabel>>,
        Query<&mut Text, With<CraftMessage>>,
    )>,
) {
    let Some(selected) = open.0 else { return };
    let ups = &book().upgrades;
    let up = &world.upgrades;

    for (card, mut image) in &mut cards {
        let tint = match up.standing(&ups[card.0]) {
            Standing::Bought => Color::srgb(0.62, 0.78, 1.0),
            Standing::Open => Color::WHITE,
            Standing::Locked => Color::srgb(0.48, 0.46, 0.44),
        };
        if image.color != tint {
            image.color = tint;
        }
    }
    for (tier, mut text) in &mut texts.p0() {
        let u = &ups[tier.0];
        let value = match up.standing(u) {
            Standing::Bought => format!("Tier {}  -  yours", u.tier),
            Standing::Open => format!("Tier {}", u.tier),
            Standing::Locked => format!("Tier {}  -  locked", u.tier),
        };
        set_text(&mut text, value);
    }
    let at = card_at(ups[selected].pos) - Vec2::splat(8.0);
    for mut node in &mut frame {
        if node.left != Val::Px(at.x) || node.top != Val::Px(at.y) {
            node.left = Val::Px(at.x);
            node.top = Val::Px(at.y);
        }
    }

    let u = &ups[selected];
    let standing = up.standing(u);
    for mut text in &mut texts.p1() {
        set_text(&mut text, u.name.clone());
    }
    let needs = u.needs.as_deref().and_then(|p| book().get(p)).map_or("nothing".to_string(), |p| p.name.clone());
    let state = match standing {
        Standing::Bought => "You have it.".to_string(),
        Standing::Open => match up.can_buy(u, &treasuries) {
            Ok(()) => "You can craft it now.".to_string(),
            Err(why) => why,
        },
        Standing::Locked => format!("Locked: craft {needs} first."),
    };
    let info = format!("Tier {}   needs: {needs}\n\n{}\n\n{state}", u.tier, u.text);
    for mut text in &mut texts.p2() {
        set_text(&mut text, info.clone());
    }
    for (price, mut text, mut colour) in &mut texts.p3() {
        let n = u.cost.parts().into_iter().find(|(k, _)| *k == price.0).map_or(0, |(_, n)| n);
        set_text(&mut text, n.to_string());
        let short = standing != Standing::Bought && treasuries.amount(Faction::Reclaimers, price.0) < n;
        let wanted = if short { SHORT } else { INK };
        if colour.0 != wanted {
            colour.0 = wanted;
        }
    }
    for (stock, mut text) in &mut texts.p4() {
        set_text(&mut text, treasuries.amount(Faction::Reclaimers, stock.0).to_string());
    }
    let label = match standing {
        Standing::Bought => "BOUGHT",
        Standing::Locked => "LOCKED",
        Standing::Open => "BUY  Enter",
    };
    for mut text in &mut texts.p5() {
        set_text(&mut text, label.to_string());
    }
    for mut text in &mut texts.p6() {
        set_text(&mut text, nearby.message.clone().unwrap_or_default());
    }
}
