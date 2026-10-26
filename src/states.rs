//! Top-level game states. See agents.md §4.1.

use bevy::prelude::*;

/// Which "scene" is running.
#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum GameState {
    #[default]
    Overworld,
    Combat,
}

/// Phases inside a battle. Only exists while `GameState::Combat` is active.
#[derive(SubStates, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[source(GameState = GameState::Combat)]
pub enum CombatPhase {
    /// Grid and armies are being placed.
    #[default]
    Deploy,
    /// Stacks take turns.
    Turn,
    /// One side is wiped out; waiting for the player to continue.
    Finished,
}

pub struct StatesPlugin;

impl Plugin for StatesPlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<GameState>().add_sub_state::<CombatPhase>();
    }
}
