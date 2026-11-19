//! The mouse cursor: the pack's cursor images, and a corner frame drawn around what a click would
//! act on. See agents.md §6.6.
//!
//! Every frame the combat code says what it wants in `CursorWanted` (during `Update`);
//! `apply_cursor` (in `PostUpdate`) shows it and resets it to the plain arrow, so a screen that
//! says nothing (the overworld) gets the arrow.

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::sprite::{BorderRect, SliceScaleMode, SpriteImageMode, TextureSlicer};
use bevy::window::{CursorIcon, CustomCursor, CustomCursorImage, PrimaryWindow};

const CURSOR_DIR: &str = "ui_elements/ui_elements/cursors";
/// Corner pieces of `cursor_04.png` (128 px), kept unstretched when the frame is resized.
const FRAME_CORNER: f32 = 40.0;

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum CursorKind {
    /// `cursor_01`: nothing to click here.
    #[default]
    Arrow,
    /// `cursor_02`: a click does something (a button, a valid move, strike or merge).
    Hand,
    /// `cursor_03`: a click here would not work (too far, out of range, blocked).
    No,
}

impl CursorKind {
    /// The image, the part of it that is drawn (the 64 px sheets have a wide empty border) and
    /// the hotspot inside that part.
    fn image(self) -> (&'static str, URect, (u16, u16)) {
        match self {
            CursorKind::Arrow => ("cursor_01.png", URect::new(22, 17, 44, 47), (0, 0)),
            CursorKind::Hand => ("cursor_02.png", URect::new(20, 17, 47, 49), (3, 0)),
            CursorKind::No => ("cursor_03.png", URect::new(16, 13, 48, 49), (16, 18)),
        }
    }
}

/// What the cursor should look like this frame. `frame` is a world-space rectangle to draw the
/// corner frame around (the target a click would act on).
#[derive(Resource, Default)]
pub struct CursorWanted {
    pub kind: CursorKind,
    pub frame: Option<Rect>,
}

impl CursorWanted {
    pub fn set(&mut self, kind: CursorKind, frame: Option<Rect>) {
        self.kind = kind;
        self.frame = frame;
    }
}

/// A UI element that shows the hand while the mouse is over it (the combat buttons, added by
/// `wire_combat_buttons`); it brings `Hovered`, which also counts the
/// node's children (its text and icons).
#[derive(Component, Default)]
#[require(Hovered)]
pub struct ClickCursor;

#[derive(Component)]
struct TargetFrame;

#[derive(Resource)]
struct CursorImages([Handle<Image>; 3]);

pub struct CursorPlugin;

impl Plugin for CursorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CursorWanted>()
            .add_systems(Startup, setup)
            .add_systems(PostUpdate, (hand_over_buttons, apply_cursor).chain());
    }
}

fn setup(mut commands: Commands, server: Res<AssetServer>) {
    let load = |kind: CursorKind| server.load(format!("{CURSOR_DIR}/{}", kind.image().0));
    commands.insert_resource(CursorImages([load(CursorKind::Arrow), load(CursorKind::Hand), load(CursorKind::No)]));
    commands.spawn((
        TargetFrame,
        Sprite {
            image: server.load(format!("{CURSOR_DIR}/cursor_04.png")),
            image_mode: SpriteImageMode::Sliced(TextureSlicer {
                border: BorderRect::all(FRAME_CORNER),
                center_scale_mode: SliceScaleMode::Stretch,
                sides_scale_mode: SliceScaleMode::Stretch,
                max_corner_scale: 1.0,
            }),
            ..default()
        },
        Transform::from_xyz(0.0, 0.0, 50.0),
        Visibility::Hidden,
        Pickable::IGNORE,
    ));
}

fn hand_over_buttons(buttons: Query<&Hovered, With<ClickCursor>>, mut wanted: ResMut<CursorWanted>) {
    if buttons.iter().any(|h| h.get()) {
        wanted.set(CursorKind::Hand, None);
    }
}

fn apply_cursor(
    mut commands: Commands,
    mut wanted: ResMut<CursorWanted>,
    images: Res<CursorImages>,
    window: Single<Entity, With<PrimaryWindow>>,
    mut frame: Single<(&mut Sprite, &mut Transform, &mut Visibility), With<TargetFrame>>,
    mut shown: Local<Option<CursorKind>>,
) {
    let kind = wanted.kind;
    if *shown != Some(kind) {
        *shown = Some(kind);
        let (_, rect, hotspot) = kind.image();
        let index = match kind {
            CursorKind::Arrow => 0,
            CursorKind::Hand => 1,
            CursorKind::No => 2,
        };
        commands.entity(*window).insert(CursorIcon::Custom(CustomCursor::Image(CustomCursorImage {
            handle: images.0[index].clone(),
            rect: Some(rect),
            hotspot,
            ..default()
        })));
    }

    let (sprite, transform, visibility) = &mut *frame;
    match wanted.frame {
        Some(rect) => {
            // A little bigger than the target, and never smaller than two corners.
            let size = (rect.size() + Vec2::splat(12.0)).max(Vec2::splat(FRAME_CORNER * 2.0));
            if sprite.custom_size != Some(size) {
                sprite.custom_size = Some(size);
            }
            let centre = rect.center();
            if transform.translation.truncate() != centre {
                transform.translation.x = centre.x;
                transform.translation.y = centre.y;
            }
            if **visibility != Visibility::Visible {
                **visibility = Visibility::Visible;
            }
        }
        None => {
            if **visibility != Visibility::Hidden {
                **visibility = Visibility::Hidden;
            }
        }
    }
    *wanted = CursorWanted::default();
}
