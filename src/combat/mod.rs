//! Turn-based tactical combat. See agents.md §3 and §4.

pub mod actions;
pub mod grid;
pub mod hud;
pub mod spawn;
pub mod turn;

use bevy::prelude::*;

use crate::states::{CombatPhase, GameState};

pub struct CombatPlugin;

impl Plugin for CombatPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<actions::CombatAction>()
            .init_resource::<hud::Hovered>()
            .init_resource::<actions::EnemyThinkTimer>()
            .add_systems(
                OnEnter(GameState::Combat),
                (spawn::spawn_battle, hud::spawn_hud).chain(),
            )
            .add_systems(OnEnter(CombatPhase::Turn), turn::activate_current_stack)
            .add_systems(
                Update,
                (
                    actions::tick_action_lock,
                    actions::player_input,
                    actions::enemy_ai,
                    actions::resolve_actions,
                    turn::check_victory,
                )
                    .chain()
                    .run_if(in_state(CombatPhase::Turn)),
            )
            .add_systems(
                Update,
                (
                    hud::refresh_stack_labels,
                    hud::refresh_turn_strip,
                    hud::refresh_panel,
                    hud::follow_active_marker,
                    hud::highlight_reachable_tiles,
                    actions::tween_stack_moves,
                    actions::fly_projectiles,
                    actions::shield_visuals,
                )
                    .run_if(in_state(GameState::Combat)),
            )
            .add_systems(OnEnter(CombatPhase::Finished), hud::show_result)
            .add_systems(
                Update,
                turn::wait_for_continue.run_if(in_state(CombatPhase::Finished)),
            )
            .add_systems(OnExit(GameState::Combat), turn::cleanup);
    }
}
