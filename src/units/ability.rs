//! Special abilities. Unlike the base stats and actions (move, attack, split, merge), an ability
//! belongs only to the unit kinds that list it in `assets/units.ron`.

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum Ability {
    /// 1 AP: +`bonus` defence until the stack's next turn.
    ShieldBlock { bonus: u32 },
}

impl Ability {
    /// Short text for stat panels, such as `Shield +3`.
    pub fn label(self) -> String {
        match self {
            Ability::ShieldBlock { bonus } => format!("Shield +{bonus}"),
        }
    }
}
