//! Shared UI pieces built from the Tiny Swords UI sheets: nine-slice panels, icon + number
//! stats and ribbon banners. See agents.md §6.

use bevy::prelude::*;
use bevy::sprite::{BorderRect, SliceScaleMode, TextureSlicer};

use crate::assets::GameAssets;
use crate::faction::Faction;
use crate::units::UnitKind;

/// Size of the coin / sword icons next to a number.
pub const STAT_ICON_PX: f32 = 26.0;

/// Nine-slice mode for one of the generated `ui_elements/generated/*_9.png` images
/// (borders measured when the images were stitched, see agents.md §6).
pub fn sliced(left: f32, right: f32, top: f32, bottom: f32) -> NodeImageMode {
    NodeImageMode::Sliced(TextureSlicer {
        border: BorderRect { min_inset: Vec2::new(left, top), max_inset: Vec2::new(right, bottom) },
        center_scale_mode: SliceScaleMode::Stretch,
        sides_scale_mode: SliceScaleMode::Stretch,
        max_corner_scale: 1.0,
    })
}

/// A nine-slice image that covers its whole node. `ImageNode` draws in the content box by
/// default, which would leave the padding bare, so the visual box is the border box.
pub fn panel(image: Handle<Image>, mode: NodeImageMode) -> ImageNode {
    let mut node = ImageNode::new(image).with_mode(mode);
    node.visual_box = VisualBox::BorderBox;
    node
}

/// A square icon at `px`.
pub fn icon(image: Handle<Image>, px: f32) -> impl Bundle {
    (
        ImageNode::new(image).with_mode(NodeImageMode::Stretch),
        Node { width: Val::Px(px), height: Val::Px(px), flex_shrink: 0.0, ..default() },
        Pickable::IGNORE,
    )
}

/// `[icon] text` on one line. `marker` tags the `Text` so a refresh system can find it.
pub fn icon_stat<M: Component>(image: Handle<Image>, marker: M, font_px: f32) -> impl Bundle {
    (
        Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, column_gap: Val::Px(6.0), ..default() },
        Pickable::IGNORE,
        children![
            icon(image, STAT_ICON_PX),
            (
                marker,
                Text::new(""),
                TextFont { font_size: FontSize::Px(font_px), ..default() },
                TextColor(Color::WHITE),
                Pickable::IGNORE,
            ),
        ],
    )
}

/// Your gold: coin icon + amount. Tag with a marker and write the number into its `Text`.
pub fn gold_stat<M: Component>(assets: &GameAssets, marker: M, font_px: f32) -> impl Bundle {
    icon_stat(assets.ui.icon_gold.clone(), marker, font_px)
}

/// Your army: sword icon + `used/max`. Tag with a marker and write the text into its `Text`.
pub fn army_stat<M: Component>(assets: &GameAssets, marker: M, font_px: f32) -> impl Bundle {
    icon_stat(assets.ui.icon_army.clone(), marker, font_px)
}

/// A big ribbon in a kingdom colour with `title` across it (the finish texts).
/// The ribbon sheet's caps are 98 / 97 px wide and 103 px tall; the middle stretches.
pub fn ribbon(assets: &GameAssets, colour: Faction, title: impl Into<String>, font_px: f32) -> impl Bundle {
    (
        panel(assets.ui.ribbons[&colour].clone(), sliced(98.0, 97.0, 0.0, 0.0)),
        Node {
            height: Val::Px(103.0),
            min_width: Val::Px(420.0),
            padding: UiRect::axes(Val::Px(110.0), Val::Px(0.0)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        Pickable::IGNORE,
        children![(
            Text::new(title),
            TextFont { font_size: FontSize::Px(font_px), ..default() },
            TextColor(Color::WHITE),
            TextLayout::justify(Justify::Center),
            // The ribbon's bow sits a little above its middle; nudge the text up onto it.
            Node { margin: UiRect::top(Val::Px(-10.0)), ..default() },
            Pickable::IGNORE,
        )],
    )
}

/// The final win: a big ribbon, drawn at `scale`, as the background of the whole message.
/// Two lines fit on the ribbon's band; the text is laid out at native size and scaled with it.
pub fn big_ribbon(assets: &GameAssets, colour: Faction, text: impl Into<String>, scale: f32) -> impl Bundle {
    (
        panel(assets.ui.ribbons[&colour].clone(), sliced(98.0, 97.0, 0.0, 0.0)),
        UiTransform::from_scale(Vec2::splat(scale)),
        Node {
            height: Val::Px(103.0),
            min_width: Val::Px(360.0),
            padding: UiRect::axes(Val::Px(104.0), Val::Px(0.0)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        Pickable::IGNORE,
        children![(
            Text::new(text),
            TextFont { font_size: FontSize::Px(21.0), ..default() },
            TextColor(Color::WHITE),
            TextLayout::justify(Justify::Center),
            Node { margin: UiRect::top(Val::Px(-8.0)), ..default() },
            Pickable::IGNORE,
        )],
    )
}

/// A finish screen: a ribbon with the title, and the details on a dark card beneath it.
pub fn finish_banner(assets: &GameAssets, colour: Faction, title: &str, details: String, accent: Color) -> impl Bundle {
    (
        Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: Val::Px(6.0),
            ..default()
        },
        Pickable::IGNORE,
        children![
            ribbon(assets, colour, title, 40.0),
            (
                Node { padding: UiRect::axes(Val::Px(28.0), Val::Px(16.0)), border: UiRect::all(Val::Px(3.0)), ..default() },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.8)),
                BorderColor::all(accent),
                Pickable::IGNORE,
                children![(
                    Text::new(details),
                    TextFont { font_size: FontSize::Px(22.0), ..default() },
                    TextColor(Color::WHITE),
                    TextLayout::justify(Justify::Center),
                    Pickable::IGNORE,
                )],
            ),
        ],
    )
}

/// `human_avatars/avatars_NN.png`: five faces per kingdom colour, in this unit order.
pub fn avatar_index(faction: Faction, unit: UnitKind) -> u32 {
    let colour = match faction {
        Faction::Reclaimers => 0,
        Faction::Red => 5,
        Faction::Yellow => 10,
        Faction::Purple => 15,
        Faction::Black => 20,
    };
    let face = match unit {
        UnitKind::Warrior => 1,
        UnitKind::Lancer => 2,
        UnitKind::Archer => 3,
        UnitKind::Monk => 4,
        UnitKind::Pawn => 5,
    };
    colour + face
}

/// The avatar of `unit` in `faction`'s colour.
pub fn avatar_handle(server: &AssetServer, faction: Faction, unit: UnitKind) -> Handle<Image> {
    server.load(format!("ui_elements/ui_elements/human_avatars/avatars_{:02}.png", avatar_index(faction, unit)))
}
