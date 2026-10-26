//! The battle grid and grid coordinates.

use bevy::prelude::*;

pub const GRID_COLS: i32 = 12;
pub const GRID_ROWS: i32 = 8;
pub const TILE_SIZE: f32 = 72.0;
/// How many columns on each edge belong to a side for deployment and tinting.
const SIDE_WIDTH: i32 = 2;

#[derive(Resource, Clone, Copy, Debug)]
pub struct BattleGrid {
    pub cols: i32,
    pub rows: i32,
    pub tile_size: f32,
}

impl BattleGrid {
    /// World position of a tile centre. Row 0 is the top row, column 0 the left column.
    pub fn to_world(&self, pos: GridPos) -> Vec2 {
        let width = self.cols as f32 * self.tile_size;
        let height = self.rows as f32 * self.tile_size;
        Vec2::new(
            pos.0.x as f32 * self.tile_size - width / 2.0 + self.tile_size / 2.0,
            height / 2.0 - pos.0.y as f32 * self.tile_size - self.tile_size / 2.0,
        )
    }

    pub fn contains(&self, pos: GridPos) -> bool {
        (0..self.cols).contains(&pos.0.x) && (0..self.rows).contains(&pos.0.y)
    }

    pub fn is_player_side(&self, pos: GridPos) -> bool {
        pos.0.x < SIDE_WIDTH
    }

    pub fn is_enemy_side(&self, pos: GridPos) -> bool {
        pos.0.x >= self.cols - SIDE_WIDTH
    }

    /// Columns a side deploys in, nearest to its edge first.
    pub fn deploy_columns(&self, player: bool) -> Vec<i32> {
        if player {
            (0..SIDE_WIDTH).collect()
        } else {
            (self.cols - SIDE_WIDTH..self.cols).rev().collect()
        }
    }

    /// Checkerboard floor, tinted green on the player's side and red on the enemy's.
    pub fn base_color(&self, pos: GridPos) -> Color {
        let (r, g, b) = if self.is_player_side(pos) {
            (0.30, 0.56, 0.32)
        } else if self.is_enemy_side(pos) {
            (0.58, 0.32, 0.30)
        } else {
            (0.42, 0.50, 0.36)
        };
        let k = if (pos.0.x + pos.0.y) % 2 == 0 { 1.0 } else { 0.86 };
        Color::srgb(r * k, g * k, b * k)
    }

    pub fn all_positions(&self) -> impl Iterator<Item = GridPos> + '_ {
        (0..self.rows).flat_map(move |y| (0..self.cols).map(move |x| GridPos::new(x, y)))
    }
}

/// Grid coordinate of a tile or a stack. The `Transform` is derived from it.
#[derive(Component, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct GridPos(pub IVec2);

impl GridPos {
    pub fn new(x: i32, y: i32) -> Self {
        Self(IVec2::new(x, y))
    }

    /// Movement cost between two tiles, Pathfinder style: a straight step costs 1, and
    /// diagonal steps alternate 1, 2, 1, 2... (so two diagonals cost 3).
    pub fn steps_to(self, other: GridPos) -> i32 {
        let d = (self.0 - other.0).abs();
        let diagonals = d.x.min(d.y);
        let straight = d.x.max(d.y) - diagonals;
        straight + diagonals + diagonals / 2
    }

    /// Orthogonally touching. Diagonal tiles are NOT adjacent (melee range, archer engagement).
    pub fn is_adjacent(self, other: GridPos) -> bool {
        let d = (self.0 - other.0).abs();
        d.x + d.y == 1
    }

    /// Range as the crow flies in tiles (the larger of dx, dy). Used for ranged strikes.
    pub fn distance_to(self, other: GridPos) -> i32 {
        let d = (self.0 - other.0).abs();
        d.x.max(d.y)
    }

    /// Unit step pointing from `self` towards `other` (each axis -1, 0 or 1).
    pub fn direction_to(self, other: GridPos) -> IVec2 {
        (other.0 - self.0).signum()
    }

    /// The 8 surrounding tiles starting to the right and going clockwise on screen.
    /// Used by Unstack to find a free tile.
    pub fn ring_clockwise_from_right(self) -> [GridPos; 8] {
        // Row 0 is the top row, so "down" on screen is +y.
        [(1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)]
            .map(|(dx, dy)| GridPos(self.0 + IVec2::new(dx, dy)))
    }
}

/// Can a stack with `range` standing on `from` strike something on `to`?
/// Melee `(1, 1)` reaches the 8 surrounding tiles; ranged kinds need the straight-line distance
/// (`distance_to`) to lie within `min..=max`, so nothing closer than `min` can be hit.
pub fn in_strike_range(range: (u32, u32), from: GridPos, to: GridPos) -> bool {
    let d = from.distance_to(to);
    if range.1 <= 1 {
        // Melee reaches all 8 surrounding tiles, diagonals included.
        return d == 1;
    }
    d >= range.0 as i32 && d <= range.1 as i32
}

/// Marker for a clickable floor tile.
#[derive(Component)]
pub struct Tile;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn straight_steps_cost_one_each() {
        assert_eq!(GridPos::new(0, 0).steps_to(GridPos::new(3, 0)), 3);
        assert_eq!(GridPos::new(0, 0).steps_to(GridPos::new(0, 2)), 2);
    }

    #[test]
    fn diagonals_alternate_one_and_two() {
        let from = GridPos::new(0, 0);
        assert_eq!(from.steps_to(GridPos::new(1, 1)), 1);
        assert_eq!(from.steps_to(GridPos::new(2, 2)), 3);
        assert_eq!(from.steps_to(GridPos::new(3, 3)), 4);
        assert_eq!(from.steps_to(GridPos::new(4, 4)), 6);
    }

    #[test]
    fn mixed_paths_add_straight_and_diagonal_costs() {
        // One diagonal (1) + two straight (2).
        assert_eq!(GridPos::new(0, 0).steps_to(GridPos::new(3, 1)), 3);
        // Two diagonals (3) + one straight (1).
        assert_eq!(GridPos::new(0, 0).steps_to(GridPos::new(3, 2)), 4);
    }

    #[test]
    fn adjacency_is_orthogonal_only() {
        let c = GridPos::new(5, 5);
        assert!(c.is_adjacent(GridPos::new(5, 4)));
        assert!(c.is_adjacent(GridPos::new(6, 5)));
        assert!(!c.is_adjacent(GridPos::new(6, 6)), "diagonal is not adjacent");
        assert!(!c.is_adjacent(GridPos::new(7, 5)));
        assert!(!c.is_adjacent(c));
    }

    #[test]
    fn ring_starts_right_and_goes_clockwise() {
        let ring = GridPos::new(3, 3).ring_clockwise_from_right();
        assert_eq!(ring[0], GridPos::new(4, 3), "right");
        assert_eq!(ring[1], GridPos::new(4, 4), "down-right (row grows downwards)");
        assert_eq!(ring[4], GridPos::new(2, 3), "left");
        assert_eq!(ring[6], GridPos::new(3, 2), "up");
    }

    #[test]
    fn strike_range_has_a_minimum() {
        let me = GridPos::new(5, 5);
        let melee = (1, 1);
        let bow = (2, 4);
        assert!(in_strike_range(melee, me, GridPos::new(6, 5)));
        assert!(in_strike_range(melee, me, GridPos::new(6, 6)), "melee hits diagonally too");
        assert!(!in_strike_range(melee, me, GridPos::new(7, 6)), "two tiles away is not melee");
        assert!(!in_strike_range(bow, me, GridPos::new(6, 5)), "archer: 1 tile is too close");
        assert!(!in_strike_range(bow, me, GridPos::new(6, 6)), "diagonal neighbour is still distance 1");
        assert!(in_strike_range(bow, me, GridPos::new(7, 5)));
        assert!(in_strike_range(bow, me, GridPos::new(9, 8)), "distance 4 diagonal-ish");
        assert!(!in_strike_range(bow, me, GridPos::new(10, 5)), "5 is out of range");
    }

    #[test]
    fn direction_and_distance() {
        let a = GridPos::new(2, 2);
        assert_eq!(a.direction_to(GridPos::new(6, 1)), IVec2::new(1, -1));
        assert_eq!(a.distance_to(GridPos::new(6, 1)), 4);
    }
}
