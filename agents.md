# parkour_game2 – design & architecture

A top-down game in two layers: you explore an **overworld in real time**, and when your hero
touches an enemy squad the game switches to a **turn-based tactical grid**. Art comes from the
Tiny Swords free pack. Engine: **Bevy 0.19** (Rust).

```
 OVERWORLD (real-time exploration)
   - Hero walks on the map.
   - Touches an enemy squad  ->  transition to the combat scene.
         |
         v
 COMBAT SCENE (turn-based tactical grid)
   - Your stacks spawn on the left side.
   - Enemy stacks spawn on the right side.
   - Turn order is rolled randomly.
   - Win or lose  ->  back to the overworld.
```

---

## 1. Overworld (real-time exploration)

### 1.1 The map

The hero walks in real time (WASD / arrows) over a generated map (see 1.6). On it you find:

| Thing | Touching it does |
|---|---|
| **Your castle** | Opens the garrison: move units between the castle and the hero (1.4) |
| **Your shop buildings** (House, Archery, Barracks, Monastery) | `E` hires one unit of that building's kind (1.2) |
| **Idle sellers** | A lone unit standing in the wild. `E` hires one of its kind for the same price |
| **Enemy castle** | Starts a fight against its garrison. Win = the whole base is yours (1.5) |
| **Tower** you do not own | Starts a fight against the units guarding it. Win = the tower pays you (1.3) |
| **AI army** | Starts a fight against that army |
| **Bandit squad** (red circle) | Walking into the circle starts a fight. Win = you take all their gold |

Water blocks the hero. Mountains are plateaus one layer up, reachable only by their staircase (1.6).

### 1.2 Kingdom bases

Each of the four kingdoms (Blue = you, Red, Purple, Yellow) owns one base, **5×4 tiles**:

```
y=3:  .  C  C  .  M        C = Castle (2×2)   M = Monastery
y=2:  .  C  C  .  .
y=1:  .  .  *  .  .        * = spawn tile for the hero / AI armies
y=0:  H  .  A  .  B        H = House   A = Archery   B = Barracks
```

| Building | Does |
|---|---|
| **Castle** | Holds the **garrison**: up to 30 units. Starts with 10 Warriors (knights), 5 Lancers (spear guards), 10 Archers. The garrison is also what defends the castle when attacked |
| **House** | Sells Pawns |
| **Archery** | Sells Archers |
| **Barracks** | Sells Warriors |
| **Monastery** | Sells Monks |

Only the owner can buy from a base's buildings. When you take a castle, every building of that base
becomes yours. The pack has no Lancer building, so Lancers come from the garrison or idle sellers.

**Towers** are single tiles scattered over the map. They start **Black** (neutral) with a small Black
guard. Whoever holds a tower gets its income.

### 1.3 Economy

| Rule | Value |
|---|---|
| Castle income | 10 gold every 30 s to its owner |
| Tower income | 5 gold every 30 s to its owner |
| Starting gold | 50 per kingdom, 20 for Black |
| Prices | Pawn 5, Archer 10, Warrior 15, Monk 20, Lancer 25 |
| Winning a fight | You take **half** of the loser's gold (bandits: all of their purse, 10–40) |
| Losing a fight | **Half of your gold** goes to the faction that beat you |

AI kingdoms and Black earn income too, so their loot grows while you wait.

### 1.4 Armies and slots

- Every unit takes **one slot**. The hero carries at most **20** units, a castle garrison at most **30**.
- The hero starts with **no army**: go to your castle and take units.
- At your own castle: `1` Warrior, `2` Archer, `3` Lancer, `4` Monk, `5` Pawn takes **one** unit out;
  hold `Shift` to **store** one instead; hold `Ctrl` to move **all** of that kind.
- Units lost in a fight are **gone for good**. Survivors go back to the hero after every fight.

### 1.5 Fights and conquest

- Fights happen on the combat grid (§3). The enemy army is whatever defends the thing you touched:
  the castle garrison, the tower guard, the AI army itself, or the bandit squad.
- **Win against a castle**: the base and its buildings become yours; its garrison is gone; you loot
  half of that kingdom's gold. A kingdom without its castle raises no new armies.
- **Win against a tower**: it becomes yours and pays you income.
- **Win against an AI army**: it is destroyed and respawns at its castle after 60 s.
- **Win against bandits**: you take their gold; a new squad appears far from the hero after 45 s.
- **Lose**: your army is gone, half your gold goes to the winner, the hero wakes up at your castle.
  The HUD shows a message about the outcome for a few seconds after every fight.
- **Undefended** castles and towers are taken just by touching them.
- **Goal**: hold all four castles. A banner tells you when the land is united.

**AI kingdoms** each have 2 roaming armies. They walk to the nearest tower they do not own, **stand at
it for 10 seconds** (the label counts down), and take it **without a fight**. Taking a tower costs the
army a random **10–30 %** of its units, which stay behind as that tower's guard. The capture animation
plays, then the army walks on to the next tower. They also take towers you own. AI armies never attack
castles or the hero.

AI armies and bandits **walk by the same rules as the hero**: A* (the `pathfinding` crate) over
`TerrainMap::can_step`, so they go around water and reach a plateau only by its staircase. An army
picks the nearest tower **by walking distance**; a tower nobody can reach (island, plateau without
stairs) is skipped and stays as it is. After a capture an army with **fewer than 5 units** walks
back to its castle ("returning home"), rests 60 s ("resting... Ns") and sets out again with a fresh
army; the garrison is untouched.

**Tower sprites** use `buildings/<colour>_buildings/tower.png` in the holder's colour: Black at the
start, then whoever takes it. A change of owner plays explosion + dust, then dust alone, then the
tower switches colour (`tower_capture_visuals` in `overworld/mod.rs`, about 1 s). This also plays when
you come back from winning a tower fight.

### 1.6 World generation, layers and tiles

The map is 64×48 tiles of 64 px, built from seeded noise (`overworld/terrain.rs`,
`overworld/world.rs`). The seed is printed in the log at start-up; `WORLD_SEED=123 cargo run`
reproduces a map.

```
NOISE LAYER 1 (elevation) ---> low = Water | mid = Grass | high = Mountain (plateau)
NOISE LAYER 2 (moisture)  ---> denser trees where it is high
NOISE LAYER 3 (features)  ---> towers and idle sellers sit on the peaks of this layer
```

**Layers.** Every tile has an elevation layer: **water 0, grass 1, mountain 2**. The hero carries a
`Layer` too (shown in the HUD) and it follows the tile under them. Moving between layers is only
possible on a **staircase**: a grass tile right below a mountain edge. Every mountain region gets
one staircase on its south side (regions with no grass to the south stay unreachable). Water can
never be entered. AI armies and bandits ignore layers (simplification).

**Tile sheets** (`assets/terrain/tileset/`, 9×6 cells of 64 px each):

| What | Sheet | Cells used |
|---|---|---|
| Water | `water_background_color.png` | the single tile |
| Grass | `tilemap_color1..4` by map quarter: 1 top-left, 2 top-right, 3 bottom-left, 4 bottom-right | flat blob top-left (3×3 + strips + single), picked by which 4-neighbours are grass |
| Base ground | `tilemap_color5` | same flat blob rules, so a base is its own island |
| Mountain | the quarter's sheet | raised blob top-right (columns 5–8), same neighbour rules |
| Cliff face | the quarter's sheet | row 4, drawn on the grass tile below a mountain edge (left end / middle / right end / lone) |
| Stairs | the quarter's sheet | the 1×2 ramp at column 0, rows 4–5, drawn over the edge tile and the grass tile below |

Bases are placed at random on grass only, spaced apart (14 tiles, relaxed if the map is cramped),
and their 5×4 footprint is flattened. Towers and sellers keep 6 tiles from bases and 5 from each
other. Bandits spawn at least 10 tiles from your base.

**Decorations** (`WorldState.decorations`, purely visual, animated where the sheet has frames):
trees (`resources/wood/trees/tree1..4`, denser with moisture), bushes and rocks on land; water rocks
and the rubber duck in the water. Never on bases, stairs, cliff faces or feature tiles.

## 2. World structure – the five kingdoms

| Kingdom | Nickname | Tiny Swords colour | Style |
|---|---|---|---|
| **Your kingdom** | The Reclaimers | Blue | Neutral knights and archers seeking to unite the land |
| **Red kingdom** | Castle Knights | Red | High defence, slow-moving heavy armour |
| **Purple kingdom** | Pawns / Goblins | Purple | Large groups of weak, fast units; swarm mechanics |
| **Yellow kingdom** | Barbarians / Raiders | Yellow | Aggressive glass cannons: high attack, low armour |
| **Black** | Towers and bandits | Black | Not a kingdom: guards the neutral towers and roams as bandits. Has no base |

The pack ships units in Blue, Red, Yellow, Purple and Black, so each side maps to one colour.
Blue is the player's default colour. Kingdom-specific stats are not implemented yet: every kingdom
uses the unit stats in §3.1.

## 3. Combat scene

### 3.1 Units and stacks

Slot limits (20 on the hero, 30 in a castle) are in §1.4.

A **stack** is a group of units of the **same kind** standing on one tile. Every unit kind has:

| Stat | Meaning |
|---|---|
| `health` | Hit points of one unit |
| `defence` | Damage each unit absorbs before losing health |
| `speed` | Movement range in tiles |
| `damage` | A `min..=max` range, rolled by every other unit in the stack (see 3.2) |
| `range` | Strike range in tiles as `min..=max`, like damage. `1..=1` = melee (adjacent only). Archers have `2..=4`: they cannot hit anything closer than 2 tiles |
| actions | The actions the kind can perform (see 3.3) |

Unit kinds available in the pack: **Warrior, Archer, Lancer, Monk, Pawn**.

### 3.2 Damage

**Adjacent** means orthogonally touching (up, down, left, right). Diagonal tiles are **not** adjacent.

**Who can strike whom**
- Melee kinds (range 1) strike an adjacent enemy stack.
- Ranged kinds (Archer) strike an enemy stack whose straight-line distance (the larger of dx, dy) lies
  within `range`, so an archer with `2..=4` cannot hit a neighbour, diagonal or not, and cannot reach
  past 4 tiles. Enemies standing next to an archer do not stop it from shooting others.

**Flanking**: if one of the attacker's stacks stands on the far side of the target (the tile straight
behind it, seen from the attacker) the target loses **1 defence** for that strike.

**Damage dealt by a stack**
`total_damage = sum of one damage roll for every other unit in the stack`
(1 unit: 1 roll, 2 units: 1 roll, 3 units: 2 rolls, 4: 2, 5: 3 ...)

**Damage taken by a stack**
Each unit has its own defence buffer. Damage hits the **lead unit** first: subtract that unit's
defence, apply the rest to its health. If the lead unit dies and damage is left over, the leftover
**spills** to the next unit in line (which again applies its own defence), and so on.

```
remaining = total_damage
while remaining > 0 and stack.count > 0:
    after_defence = remaining - defence      (min 0; stop if 0)
    if after_defence >= lead_hp:
        remaining = after_defence - lead_hp  (spill-over)
        stack.count -= 1 ; lead_hp = health
    else:
        lead_hp -= after_defence ; remaining = 0
```

### 3.3 Actions

Every stack gets **3 action points (AP)** per turn and can mix actions freely.

| Action | Cost | Effect |
|---|---|---|
| **Move** | 1 AP | Move up to `speed` steps. A straight step costs 1; diagonal steps alternate 1, 2, 1, 2 (Pathfinder rule), so two diagonals cost 3 |
| **Strike** | 1 AP | Attack an enemy stack using the rules in 3.2 (melee: adjacent; ranged: distance within its `min..=max` range) |
| **Unstack** | 1 AP | Split into two equal halves (odd count: the extra unit stays). The new half goes to the first free tile starting to the right and going clockwise. The original keeps the wounded lead unit and the remaining AP; the new half is inserted in the turn order **right before** the original, so from the next round it acts first |
| **Merge** | 1 AP | Merge with an adjacent friendly stack of the **same kind**. The counts are added into the active stack, the other stack disappears from the grid and the turn order. Its wounded lead unit is healed (a stack only tracks one wounded unit) |
| **Shield up** | 1 AP | +3 defence until this stack's next turn starts |

Only stacks of the same unit kind can be merged. A stack that has nothing to split (1 unit) or no free
tile around it cannot unstack.

### 3.4 Flow

1. Both armies are placed: player in the left columns, enemy in the right columns.
2. Turn order is shuffled once per battle and cycles through every living stack.
3. The active stack spends its AP. Every action locks the battle while it plays out, never longer than **2 s**: a move takes 1 s for the first step and 0.5 s per further step; a strike lasts as long as the attack animation (at least 0.5 s), or the arrow's flight at 0.25 s per tile for archers; Unstack, Merge and Shield up 0.5 s; End turn 0.3 s. A destroyed stack stays visible until the lock ends. When AP reaches 0 (or End Turn is pressed) the next stack acts after that lock. Constants are at the top of `combat/actions.rs`.
4. When one side has no stacks left the battle ends and the game returns to the overworld.

---

## 4. ECS architecture (Bevy)

Everything is built from plugins, one per area. The code mirrors this list one to one.

### 4.1 States

```
GameState   : Overworld | Combat                 (src/states.rs)
CombatPhase : Deploy | Turn | Finished           (sub-state of GameState::Combat)
```

Entities that only live in one state carry `DespawnOnExit(GameState::X)`, so leaving the state
cleans them up automatically.

### 4.2 Modules / plugins

| Module | Plugin | Owns |
|---|---|---|
| `src/main.rs` | – | App setup, window, camera, asset root |
| `src/states.rs` | `StatesPlugin` | `GameState`, `CombatPhase` |
| `src/faction.rs` | – | `Faction` enum: colour, name, sprite folder |
| `src/units/mod.rs` | – | `UnitKind`, `UnitStats`, `Stack`, `Team`, `Army` |
| `src/units/rules.rs` | – | Pure combat maths (no Bevy): `stack_damage`, `apply_damage`, `merge`, `split`. Unit tests live here |
| `src/overworld/mod.rs` | `OverworldPlugin` | Spawns the map from `WorldState`, hero movement (water blocks), camera, HUD |
| `src/overworld/terrain.rs` | – | `Terrain`, `TerrainMap` (layers, stairs, `can_step`, `find_path` via the `pathfinding` crate), value noise, `generate()` |
| `src/overworld/buildings.rs` | – | `BuildingKind` (what it sells, size, colour, sprite), base layout |
| `src/overworld/world.rs` | – | `WorldState` (bases, towers, sellers, armies, bandits, hero pos), generation, `capture()` |
| `src/overworld/economy.rs` | `EconomyPlugin` | `Treasuries` (gold per faction), income timer |
| `src/overworld/interact.rs` | – | `Touch`/`Touchable`, `Nearby`, castle transfer, buying, fight triggers |
| `src/overworld/armies.rs` | – | `AiArmy` and `BanditSquad` state, movement, tower grabbing, respawns, encounters |
| `src/combat/mod.rs` | `CombatPlugin` | Wires the combat sub-modules |
| `src/combat/grid.rs` | – | `BattleGrid`, `GridPos`, `Tile` |
| `src/combat/spawn.rs` | – | Builds the grid, places both armies, rolls turn order |
| `src/combat/turn.rs` | – | `TurnOrder`, `ActionPoints`, `ActiveStack`, victory check |
| `src/combat/actions.rs` | – | `CombatAction` message, player input, enemy AI, action resolution |
| `src/combat/hud.rs` | – | Everything that tells you *what is what* on screen |
| `src/assets.rs` | `GameAssetsPlugin` | `GameAssets` handles (empty until sprites are wired in) |

### 4.3 Components

| Component | On | Meaning |
|---|---|---|
| `Hero`, `MoveSpeed` | overworld hero | The player avatar |
| `Touchable { touch, radius }` | buildings, towers, sellers | What touching it means (`Touch::Castle/Shop/Tower/Seller`) |
| `AiArmyMarker(i)`, `BanditMarker(i)` | roaming armies | Index into `WorldState.armies` / `.bandits`; transforms are synced from there |
| `TowerVisual { index, shown, swap }` | towers | Which owner colour the tower sprite shows and the pending swap after a capture |
| `ArmyLabel` | roamer children | Floating text kept in sync with the army |
| `Faction` | hero, roamers, stacks | Which side owns it; drives fill colour and sprite folder |
| `Team` | stacks | `Player` or `Enemy`. Drives the green/red "yours vs theirs" border |
| `Stack { kind, count, lead_hp }` | combat stacks | The group on a tile |
| `GridPos` | tiles, stacks | Grid coordinate; `Transform` is derived from it |
| `Tile` | grid tiles | Clickable floor |
| `ActiveStack`, `ActionPoints` | the stack whose turn it is | Removed and re-added each turn |
| `ShieldUp(bonus)` | a stack | Shield up buff; removed when that stack's next turn starts |
| `UnitVisual { faction, kind, anim, once, flip_x }` | unit sprite children | What sheet a unit shows; changing it swaps the sheet (`assets.rs`) |
| `Animated` | any atlas sprite | Frame stepping, looping or one-shot |
| `MoveTween` | a moving stack | Slides it to its new tile during the action delay |
| `Projectile` | an arrow | Flies from archer to target during the action delay |
| `Dying` | a destroyed stack | Kept on the board until the blow lands and the death dust replaces it |
| `PendingEffect`, `OneShotEffect` | effects | A queued / playing particle effect (`spawn_effect` in `assets.rs`) |
| `CountLabel`, `ActiveMarker`, … | HUD children | Labels refreshed by hud.rs |

### 4.4 Resources

| Resource | Lives | Meaning |
|---|---|---|
| `WorldState` | always | The whole map: terrain, bases, towers, sellers, AI armies, bandits, hero position. Overworld entities are rebuilt from it on every `OnEnter(Overworld)` |
| `Treasuries` | always | Gold per faction (`player_gold()`, `spend`, `take_half`) |
| `IncomeTimer` | always | 30 s tick for castle and tower income |
| `PlayerArmy` | always | The stacks the hero carries (max 20 slots). Starts empty |
| `Nearby` | always | What the hero is touching + a short HUD message |
| `PendingEncounter { enemy, source }` | Overworld → Combat | The enemy army and what it defends (`EncounterSource::Base/Tower/AiArmy/Bandits`) |
| `BattleGrid` | Combat | Grid size and tile size |
| `TurnOrder` | Combat | Shuffled list of stack entities + current index |
| `CombatLog` | Combat | Last few lines of what happened |
| `Hovered` | Combat | The stack under the mouse (for the info panel) |
| `CombatResult` | end of Combat | `Victory` or `Defeat` |
| `ActionLock` | Combat | Pause after every action (length depends on the action, max 2 s); nothing acts while it runs, and the turn passes when it ends if AP is spent |
| `EnemyThinkTimer` | always | Extra pause before each enemy AI action |
| `UnitSprites` | always | Cache of loaded unit strips per faction, kind and animation |

### 4.5 Messages

`CombatAction` (`Move(tile)`, `Strike(entity)`, `Unstack`, `Merge(entity)`, `ShieldUp`, `EndTurn`) is
written by player clicks/keys **and** by the enemy AI, and resolved by one system (`resolve_actions`
in `combat/actions.rs`). That keeps the rules in one place.

Controls: click a lit tile = Move, click an enemy stack = Strike, click an own stack = Merge,
`1` = Unstack, `3` = Shield up, `Space` = End turn. There is no way to leave a battle early.

### 4.6 Schedule

```
Startup                     spawn_camera
OnEnter(Overworld)          spawn_overworld (from WorldState)
Update [Overworld]          hero_input -> camera_follow -> remember_hero_position
                            -> find_touch -> tick_messages -> castle_transfer -> buy_units
                            -> ai_armies_act -> bandits_wander -> respawn_roamers -> refresh_army_labels
                            -> trigger_fights -> roamer_encounters -> refresh_overworld_hud
                            collect_income (EconomyPlugin)

OnEnter(Combat)             spawn_battle (grid, stacks, turn order) ; spawn_hud
OnEnter(CombatPhase::Turn)  activate_current_stack
Update [CombatPhase::Turn]  player_input -> enemy_ai -> resolve_actions -> check_victory
                            (clicks on tiles/stacks are observers in spawn.rs that write CombatAction)
Update [Combat]             hud refresh systems (labels, turn strip, panel, active marker, reachable tiles)
OnEnter(CombatPhase::Finished) show_result ; Update: wait_for_continue -> Overworld
```

### 4.7 Where do I add…

- **a new unit kind** → `UnitKind` + `UnitKind::stats()` in `units/mod.rs`, then a row in the sprite map.
- **a new action** → variant in `CombatAction`, a match arm in `resolve_actions`, a key or click in `player_input`.
- **a new kingdom** → `Faction` variant + colour + sprite folder in `faction.rs`, and add it to `Faction::KINGDOMS`.
- **a new building** → `BuildingKind` in `overworld/buildings.rs` (what it sells, size) and a slot in `base_layout()`.
- **a new thing to touch** → `Touch` variant in `overworld/interact.rs`, spawn it with a `Touchable`, handle it in the systems there and in the HUD prompt.
- **a new roamer behaviour** → `overworld/armies.rs` (`ai_armies_act` state machine, `bandits_wander`, `follow_path`).
- **movement rules** → `TerrainMap::can_step` in `overworld/terrain.rs`; the hero and `find_path` (used by all roamers) both go through it.
- **anything clickable in combat** → give the sprite `Pickable::default()`. Bevy 0.19 only hit-tests sprites that carry that component; labels and decorations get `Pickable::IGNORE`.
- **tuning** → the constants at the top of `economy.rs`, `armies.rs`, `world.rs`, `terrain.rs` (water / mountain thresholds).
- **a new particle effect** → an `Fx` variant with its file in `assets.rs`, then `spawn_effect(.., fx, pos, z, scale, delay, replaces)` wherever it should play.
- **a new decoration** → `DecorationKind` + `place_decorations` in `world.rs`, a `Strip` in `assets.rs`, a match arm in `spawn_decorations`.
- **which sheet draws what** → `ground_sprite` and `blob_cell` in `overworld/mod.rs`, `BASE_GROUND_SHEET` for the base ground.
- **what defends something / what a win does** → `WorldState::enemy_army_for` and `WorldState::capture` in `overworld/world.rs`.
- **real sprites** → load the sheet in `assets.rs`, replace `Sprite::from_color(..)` with `Sprite::from_atlas_image(..)` where the placeholder is built, add an animation timer system.

---

## 5. Sprite map (to fill in when swapping placeholders for art)

The pack was moved to `assets/` with **snake_case** folder and file names (the original zip is kept),
so paths load with `asset_server.load("terrain/tileset/tilemap_color1.png")`. Frame sizes were
measured from the PNGs. Terrain, decorations and units are wired in (`src/assets.rs`); buildings and
UI still use coloured squares.

### Units – `units/<colour>_units/<kind>/…`

All five colours have the same files. Frames are 192×192 (Lancer: 320×320), one row per sheet.
The character stands in the middle of the frame with its feet at row 137 (Lancer: 198); sprites
face right and are flipped for enemies. Wired in through `UnitArt` in `src/assets.rs`: combat uses
Idle, Run (while sliding to a new tile), Attack (one shot on Strike) and Guard (while Shield up
lasts); the overworld uses Idle and Run.

| Kind | Animation | File | Frames | Used for |
|---|---|---|---|---|
| Warrior | Idle | `warrior/warrior_idle.png` | 8 | standing |
| Warrior | Run | `warrior/warrior_run.png` | 6 | Move, hero walking |
| Warrior | Attack | `warrior/warrior_attack1.png` | 4 | Strike |
| Warrior | Attack 2 | `warrior/warrior_attack2.png` | 4 | not used yet |
| Warrior | Guard | `warrior/warrior_guard.png` | 6 | Shield up |
| Archer | Idle | `archer/archer_idle.png` | 6 | standing, Shield up (no guard sheet) |
| Archer | Run | `archer/archer_run.png` | 4 | Move |
| Archer | Shoot | `archer/archer_shoot.png` | 8 | Strike |
| Archer | Arrow | `archer/arrow.png` | 1 (64×64) | projectile flying to the target on Strike |
| Lancer | Idle | `lancer/lancer_idle.png` | 12 | standing |
| Lancer | Run | `lancer/lancer_run.png` | 6 | Move |
| Lancer | Attack | `lancer/lancer_right_attack.png` | 3 | Strike (also `_up_`, `_down_`, `_up_right_`, `_down_right_` variants, 3 frames each) |
| Lancer | Defence | `lancer/lancer_right_defence.png` | 6 | Shield up (same directional variants) |
| Monk | Idle | `monk/idle.png` | 6 | standing, Shield up |
| Monk | Run | `monk/run.png` | 4 | Move |
| Monk | Heal | `monk/heal.png` + `monk/heal_effect.png` | 11 | Strike for now; the future heal action |
| Pawn | Idle | `pawn/pawn_idle.png` | 8 | standing, Shield up |
| Pawn | Run | `pawn/pawn_run.png` | 6 | Move |
| Pawn | Interact | `pawn/pawn_interact_knife.png` | 4 | Strike (`_axe` 6, `_hammer` 3, `_pickaxe` 6 frames) |
| Pawn | Carrying | `pawn/pawn_idle_<axe,gold,hammer,knife,meat,pickaxe,wood>.png`, `pawn_run_<…>.png` | 8 / 6 | not used yet |

Aseprite sources (Blue only) are in `units/units_aseprite_in_blue_only/`.

### Terrain – `terrain/…`

| Asset | Sheet | Tile / frame |
|---|---|---|
| Ground tilemap | `tileset/tilemap_color1..5.png` (576×384) | 64×64, 9 columns × 6 rows (see §1.6 for the cell map) |
| Water foam | `tileset/water_foam.png` (3072×192) | 192×192, 16 frames |
| Water background | `tileset/water_background_color.png` | 64×64 |
| Shadow | `tileset/shadow.png` | 192×192 |
| Trees | `resources/wood/trees/tree1..4.png` | 192×256 (1, 2) or 192×192 (3, 4), 8 frames; stumps separate |
| Gold stones | `resources/gold/gold_stones/gold_stone_N.png` | 128×128 (+ `_highlight`) |
| Sheep | `resources/meat/sheep/sheep_idle.png` (768×128) | 128×128, 6 frames |
| Bushes | `decorations/bushes/bushe1..4.png` | 128×128, 8 frames |
| Rocks | `decorations/rocks/rock1..4.png` | 64×64 |
| Water rocks | `decorations/rocks_in_the_water/water_rocks_01..04.png` | 64×64, 16 frames |
| Rubber duck | `decorations/rubber_duck/rubber_duck.png` | 32×32, 3 frames |
| Clouds | `decorations/clouds/clouds_01..08.png` | 576×256 |

### Buildings – `buildings/<colour>_buildings/…`

Castle 320×256, Barracks 192×256, Tower 128×256 (base at row 230, wired in), Archery, Monastery,
House1–3. The base buildings are still coloured squares.

### UI – `ui_elements/ui_elements/…`

Buttons (Big/Small, Blue/Red, Regular/Pressed), Bars (`BigBar_Base/Fill`, `SmallBar_Base/Fill` for
HP bars), Banners, Ribbons, Papers, Cursors 64×64, Human Avatars, Icons.

### Particle FX – `particle_fx/…`

Sprite-sheet effects, played once by `spawn_effect` in `src/assets.rs` (Bevy has no built-in particle
system; the pack's effects are frame strips, so none is needed).

| Effect | File | Frame | Frames | Used for |
|---|---|---|---|---|
| Dust 1 | `dust_01.png` | 64 | 8 | second stage of a tower capture |
| Dust 2 | `dust_02.png` | 64 | 10 | **death of a stack** (only when its last unit dies), timed to the blow landing or the arrow arriving; first stage of a tower capture |
| Explosion 1 / 2 | `explosion_01.png`, `explosion_02.png` | 192 | 8 / 10 | 2: first stage of a tower capture |
| Fire 1 / 2 / 3 | `fire_01..03.png` | 64 | 8 / 10 / 12 | – |
| Water splash | `water_splash.png` | 192 | 9 | – |

---

## 6. Roadmap

- [x] Placeholder build: coloured squares, labels, states, grid, turn order, move + strike
- [x] Unstack / Merge / Shield up, archer range, flanking
- [x] Generated map (noise), bases with buildings, towers, sellers, income, loot
- [x] Castle garrison with transfer keys, hero slot limit, hiring
- [x] AI armies grabbing towers, bandit squads, respawns, defeat penalty
- [ ] Heal for Monk
- [ ] Kingdom-specific unit stats (Red heavy, Purple swarm, Yellow glass cannon)
- [ ] Smarter enemy AI in combat (Unstack / Merge / Shield up)
- [x] AI armies and bandits path around water and use stairs; armies retreat and refill
- [ ] AI kingdoms that attack
- [x] Terrain tiles, cliffs, stairs and decorations from the sprite map
- [x] Unit sprites with idle / run / attack / guard
- [x] Arrows and death dust
- [x] Tower sprites and capture animation
- [ ] Base building and UI sprites; water foam; hit and heal effects
