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
| **Your shop buildings** (House, Archery, Barracks, Monastery) | `R` hires one unit of that building's kind (1.2); `Tab` opens the shop |
| **Idle sellers** | A lone unit standing in the wild. `R` hires one of its kind for the same price; `Tab` opens the shop |
| **Enemy castle** | Starts a fight against its garrison. Win = the whole base is yours (1.5) |
| **Tower** you do not own | Starts a fight against the units guarding it. Win = the tower pays you (1.3) |
| **AI army** | Starts a fight against that army |
| **Bandit squad** (red circle) | Walking into the circle starts a fight. Win = you take all their gold |

Water blocks the hero. Mountains are plateaus up to layer 5, climbed only by ramps (`R`) beside them (1.6).

**World map**: `M` opens a full-screen map (and closes it). It shows terrain, ramps, every base in
its owner's colour with the castle marked, towers in their holder's colour, idle sellers and the
hero. **Roaming armies and bandits are not shown**, so scouting still means walking there. The map is
rendered from `WorldState` when opened (`overworld/map.rs`).

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
plays, then the army walks on to the next tower. **While an army is taking a tower (countdown and
animation) the hero cannot enter it**; the HUD says who is taking it. They also take towers you own. AI armies never attack
castles or the hero.

AI armies and bandits **walk by the same rules as the hero**: A* (the `pathfinding` crate) over
`TerrainMap::can_step`, so they go around water and climb a plateau only by its ramps. An army
picks the nearest tower **by walking distance**; a tower nobody can reach (island, plateau without
ramps) is skipped and stays as it is. After a capture an army with **fewer than 5 units** walks
back to its castle ("returning home"), rests 60 s ("resting... Ns") and sets out again with a fresh
army; the garrison is untouched.

**Building sprites**: towers and every base building use `buildings/<colour>_buildings/<kind>.png`
in the holder's colour (towers start Black). A change of owner plays explosion + dust, then dust
alone, over each building, then the sprites switch colour (`capture_visuals` in `overworld/mod.rs`,
about 1 s). This also plays when you come back from winning a tower or castle fight.

### 1.6 World generation, layers and tiles

The map is 64×48 tiles of 64 px. It is **built as text first** (`overworld/worldgen.rs`): every
stage is a grid of letters, one per tile, so you can print it, tweak the constants at the top of
that file and look again in seconds, without starting the game.

```
cargo run -- map 11            prints the three stages of seed 11 and a stats line
cargo run -- map 11 --save     also writes assets/seeds/11.txt
WORLD_SEED=11 cargo run        plays that file if it exists, otherwise builds the same map in memory
```

Letters: `W` water (layer 0), `G` grass (1), `M` mountain before layering, `2` `3` `4` `5` plateau
layers, `R` a ramp. A seed file is 48 lines of 64 letters, top row first, optionally with `#`
comment lines; you can edit it by hand, ramps included. `cargo run -- map <seed>` prints every
stage, the last one with its `R`s, and `--save` writes that one as the seed file.

```
1. RAW       patches of 6×5 tiles (one more than a base, so a base always fits in a flat patch),
             walked row by row from a random corner. Water grows along the walk: base chance 18 %,
             every water patch adds 45 % to the next patch in its row and 60 % to the patch above
             it in the next row; after 5 in a row (or a 100 % chance) a 2-patch breather, then the
             chance recovers over 3 patches; never more than 30 % of the patches; the start corner
             is never water. Mountains: a few seeds (2 % next to water, 8 % elsewhere, ×3 once
             the first exists, far less once two exist; never right next to a mountain, so they
             stay apart) grow into clusters of 14-24 patches by taking random grass patches on
             their rim (85 % of picks taken), never a patch touching another mountain; there is
             always at least one, and a map with only one walks again until a second seeds.
             Then water is balanced to 20 % ± 3 % of the water + grass tiles (an 80 / 20 split,
             mountains not counted): a random shore patch of the smallest lake with a shore turns
             to grass (ponds go first, big lakes stay whole), or a grass patch on a shore to
             water, one at a time. Over seeds 1-100: 2.07 big mountains per map, average top
             layer 3.8 (layer 5 on about 1 in 10), water 18-27 % of water + grass.
             Output: W / G / M per tile.
2. WARP      every tile takes the letter of a spot up to 5 tiles away, shifted by smooth noise
             (one noise cell per 7 tiles), so the straight 6×5 patch edges become wavy coasts and
             mountain sides. The noise is smooth, so it makes no specks. Output: still W / G / M.
3. LAYERS    the same seed-and-grow, nested: inside every mountain, the tiles at least 3 from any
             lower tile are eligible; a random one seeds a blob that grows over random eligible
             rim tiles until 80 % of them are taken and becomes the next layer up; again for each
             layer above, up to layer 5. The 3-tile margin keeps every cliff exactly one step tall.
             Output: W / G / 2 / 3 / 4 / 5.
4. SMOOTH    (after the layers, so it sees every layer) 3 passes over the tiles in random
             order. Pushes are **off** (`PUSH_TRIES = 0`): a push runs along the grid as a band 3
             lanes wide (each lane 2-5 tiles), so it draws straight strips; over seeds 1-20 every
             setting gave more straight edges than the warp alone. Turned on, a tile tries that
             many directions per pass: grass takes water 10 %, water takes grass 4 %, a plateau
             layer takes the layer just below it 12 %, never the reverse. Then a lone tile whose
             four neighbours all share another layer joins them (a plateau never becomes water;
             at 3 of 4 it would also cut every step of a curve and straighten it again). Nothing
             may make a cliff two layers tall, let water touch layer 3 or higher, or leave a
             terrace narrower than 2 tiles (`fits`), so a ramp always has room.
             Output: still W / G / 2..5.
5. RAMPS     (`TerrainMap::ensure_ramps`, run on every grid, generated or loaded) a ramp `R`
             sits on the lower layer, directly **east or west** of the one tile it climbs to,
             which is exactly one layer higher. In `1 2 3 / 4 5 6` with 5 higher, a ramp can be
             on 4 or 6, never on 2. It fits only where exactly one of its east/west neighbours
             is higher, the tile on its other side (its open side) and the tile below it are
             open ground of its own layer, the tile above it is not higher (else it would be a
             cliff wall) and no other ramp touches it (`ramp_fits`). The check: `R`s that do
             not fit are dropped (with a warning); then a search from all layer-1 ground finds
             what can be walked to, and every plateau region (same layer, joined) that cannot
             be reached gets a ramp linking it to ground that can (up from below or down from
             above), lowest layer first, until nothing changes. Spots beside a plateau's bottom
             corner (a cliff under the tile it climbs to) are preferred, so the rock half of the
             slope leans on the cliff. When no unreachable region has a spot at all, the lowest
             one is reshaped (`reshape`) and the check goes on: a pocket (only walls and higher
             ground around it) is filled up one layer; anything else (e.g. a mountain with grass
             only north and south of it) is lowered one layer together with all the higher
             ground standing on it, so cliffs stay one layer tall. Then one extra ramp per 40
             tiles of region, 6 tiles from the others. So every plateau can always be reached;
             over seeds 1-100 this reshapes 31 regions: 25 crumbs of 1-4 tiles the warp
             broke off a mountain, and 6 mountains with only water and the map edge around them
             (no shore for a ramp), which become grass islands.
```

`cargo test terrain_stats -- --ignored --nocapture` prints the stats line for twelve seeds.
`cargo test straightness -- --ignored --nocapture` counts straight edges (5+ tiles) before and
after smoothing on seeds 1-20 and the water : grass split on seeds 1-100.
`cargo test terrain_100_seeds -- --ignored` checks the ramp rules on seeds 1-100 (every ramp fits,
every plateau reachable, ramps survive the text). `SHOTS_DIR=<dir> WORLD_SEED=<seed> cargo run`
(`src/shots.rs`) opens the game, saves an overview of the whole map and a close-up of up to 8
ramps to `<dir>`, and quits; loop it over seeds to check the terrain by eye.

**Layers.** Every tile has an elevation layer: **water 0, grass 1, plateau 2–5**. The hero's `Layer`
follows the tile under them (shown in the HUD). The tile at the foot of a cliff shows the stone
wall and is **solid**, like water. Moving between layers is only possible on a **ramp**, always by
exactly one layer, between the ramp and the tile it climbs to (east–west). The ramp tile itself is
ground on the lower layer, walked onto and off from every side: the north, the south and its open
side (`TerrainMap::is_walkable`, `can_step`). The ramp, the tile below it and the tile
on its open side stay clear of decorations, towers and sellers (`is_ramp_entry`).
The hero's `Layer` is simply read from the tile under them after every step.

**Why ramps, not stairs.** The tile sheet has no front staircase. Its two slope pieces (1 tile wide,
2 tall: column 0 runs down to the left, column 3 down to the right, rows 4–5) slope east–west, so a
ramp always climbs sideways onto a plateau; the front and back of a plateau are plain cliffs and
plateau edge.

**Tile sheets** (`assets/terrain/tileset/`, 9×6 cells of 64 px each):

| What | Sheet | Cells used |
|---|---|---|
| Water | `water_background_color.png` | the single tile |
| Grass | `tilemap_color1` (the sheet number is the layer: layer 1 → color1, layer 2 → color2, … up to color5) | flat blob top-left (3×3 + strips + single), picked by which 4-neighbours are grass |
| Plateau (any layer) | the layer's sheet (`tilemap_color2..5` for layers 2–5) | raised blob top-right (columns 5–8); "same" = same layer, so a layer-3 top is its own island on a layer-2 terrace |
| Cliff face | the sheet of the plateau above it | row 4, columns 5–8 (left end / middle / right end / lone), drawn on every cliff-foot tile |
| Ramp | the sheet of the layer it climbs to | column 0 when the higher tile is east of it (slope down to the west), column 3 when it is west, rows 4–5, drawn over the ramp tile (top half, level with the plateau) and the ground tile below it (rock half, beside the cliff) |

Bases take four patch-sized (6×5) areas, at any tile, that are entirely layer-1 grass and clear of
ramps (`flat_spots`), spaced 3 patches apart (`patch_gap`, relaxed
if needed). Towers and sellers keep 6 tiles from bases and 5 from each other and never stand on a
ramp or a cliff foot. Bandits spawn at least 10 tiles from your base.

**Decorations** (`WorldState.decorations`, purely visual, animated where the sheet has frames):
trees (`resources/wood/trees/tree1..4`, denser where a moisture noise is high), bushes and rocks on
land; water rocks and the rubber duck in the water. Never on bases, walls, ramps, plateau edges or
feature tiles; trees also keep a tile away from those (`is_edge_art`).

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
| `range` | Strike range in tiles as `min..=max`, like damage. `1..=1` = melee: the 8 surrounding tiles. Archers have `2..=4`: they cannot hit anything closer than 2 tiles |
| actions | The actions the kind can perform (see 3.3) |

Unit kinds available in the pack: **Warrior, Archer, Lancer, Monk, Pawn**.

### 3.2 Damage

**Distance** between tiles is the larger of the horizontal and vertical difference, so all 8
surrounding tiles are at distance 1. (Merging still needs the two stacks to touch orthogonally.)

**Who can strike whom**
- Melee kinds (range 1) strike any enemy stack on one of the 8 surrounding tiles, diagonals included.
- Ranged kinds (Archer) strike an enemy stack whose straight-line distance (the larger of dx, dy) lies
  within `range`, so an archer with `2..=4` cannot hit any of its 8 neighbours and cannot reach
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
| **Strike** | 1 AP | Attack an enemy stack using the rules in 3.2 (melee: one of the 8 surrounding tiles; ranged: distance within its `min..=max` range). The attack animation is chosen from the direction attacker → target: the signs of the difference pick the Lancer's up / up-right / right / down-right / down sheet, a negative horizontal difference flips the sprite. Kinds with only one attack sheet are tilted towards the target instead (45° for diagonals, 90° straight up or down) |
| **Unstack** | 1 AP | Split into two equal halves (odd count: the extra unit stays). The new half goes to the first free tile starting to the right and going clockwise. The original keeps the wounded lead unit and the remaining AP; the new half is inserted in the turn order **right before** the original, so from the next round it acts first |
| **Merge** | 1 AP | Merge with an orthogonally adjacent friendly stack of the **same kind**. The counts are added into the active stack, the other stack disappears from the grid and the turn order. Its wounded lead unit is healed (a stack only tracks one wounded unit) |
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
| `src/overworld/worldgen.rs` | – | World building as text: `Grid` of letters, `raw` → `warp` → `layers` → `smooth`, stats, `to_text` / `from_text` |
| `src/overworld/terrain.rs` | – | `TerrainMap` (layers 0–5, ramps, `can_step`, `find_path` via the `pathfinding` crate), `from_grid` / `to_grid` (with `R`), seed-file loading, `ensure_ramps` (ramp check, placement and reshaping), value noise |
| `src/shots.rs` | – | dev tool: with `SHOTS_DIR` set, screenshots the overworld (overview + ramp close-ups) and quits |
| `src/overworld/buildings.rs` | – | `BuildingKind` (what it sells, size, colour, sprite), base layout |
| `src/overworld/world.rs` | – | `WorldState` (bases, towers, sellers, armies, bandits, hero pos), generation, `capture()` |
| `src/overworld/economy.rs` | `EconomyPlugin` | `Treasuries` (gold per faction), income timer |
| `src/overworld/interact.rs` | – | `Touch`/`Touchable`, `Nearby`, castle transfer, buying, fight triggers |
| `src/overworld/armies.rs` | – | `AiArmy` and `BanditSquad` state, movement, tower grabbing, respawns, encounters |
| `src/overworld/map.rs` | – | The `M` world map overlay, rendered as one pixel per tile from `WorldState` |
| `src/overworld/shop.rs` | – | The `Tab` shop panel (§6.1): wood table, avatar, animated preview, stats paper, buy button |
| `src/ui.rs` | – | Shared UI pieces (§6.2): nine-slice panels, coin / sword stats, ribbons, finish banner |
| `src/overworld/castle_panel.rs` | – | The `Tab` castle panel (§6.3): avatar ribbon, garrison / hero counts, TAKE / STORE |
| `src/overworld/army_panel.rs` | – | The `E` army page (§6.4): troop list with avatars, preview and stats |
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
| `BuildingSprite { kind, shown, swap }` | buildings, towers | Which owner colour the sprite shows and the pending swap after a capture |
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
| `WorldMapOpen` | always | Whether the `M` map overlay is showing |
| `OpenShop` | always | The shop panel's target while it is open |
| `OpenCastle` | always | The castle panel's base and selected unit kind while it is open |
| `OpenArmy`, `ArmyPanelShown` | always | The army page's selected kind, and what it was last built from |
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
Overworld keys: `Tab` shop / castle panel, `E` army page, `R` quick buy, `M` map, `1`–`5` castle transfer.

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
- **a new UI panel** → copy the shape of `overworld/shop.rs`: a resource that says whether it is open, a spawn function built from `children![..]`, marker components on the buttons and a `wire_*_buttons` system that attaches picking observers to them.
- **a new roamer behaviour** → `overworld/armies.rs` (`ai_armies_act` state machine, `bandits_wander`, `follow_path`).
- **movement rules** → `TerrainMap::can_step` in `overworld/terrain.rs` (same layer, or a ramp and the tile it climbs to); the hero and `find_path` (used by all roamers) both go through it.
- **anything clickable in combat** → give the sprite `Pickable::default()`. Bevy 0.19 only hit-tests sprites that carry that component; labels and decorations get `Pickable::IGNORE`.
- **tuning the map** → the constants at the top of `overworld/worldgen.rs`; look at the result with `cargo run -- map <seed>`, keep a map you like with `--save`.
- **tuning the rest** → the constants at the top of `economy.rs`, `armies.rs`, `world.rs`.
- **a new particle effect** → an `Fx` variant with its file in `assets.rs`, then `spawn_effect(.., fx, pos, z, scale, delay, replaces)` wherever it should play.
- **a new decoration** → `DecorationKind` + `place_decorations` in `world.rs`, a `Strip` in `assets.rs`, a match arm in `spawn_decorations`.
- **which sheet draws what** → `ground_sprite` and `blob_cell` in `overworld/mod.rs`; the sheet number is the layer (`layer_sheet`).
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
| Lancer | Attack | `lancer/lancer_<up, up_right, right, down_right, down>_attack.png` | 3 each | Strike, picked by the direction to the target (`AttackDir::towards`); left side = flipped |
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
| Screen background | `background.png` (user-made, 2496×800) | sea and clouds behind everything in both states: a child sprite of the camera at z −999, scaled each frame to cover the window (`fit_background` in `main.rs`) |
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

All wired in (`building_scale` / `building_anchor` in `src/assets.rs`), one file per kingdom colour:

| Building | File | Size | Base row |
|---|---|---|---|
| Castle | `castle.png` | 320×256 | 249 |
| House | `house1.png` (`house2`, `house3` unused) | 128×192 | 173 |
| Archery | `archery.png` | 192×256 | 240 |
| Barracks | `barracks.png` | 192×256 | 245 |
| Monastery | `monastery.png` | 192×320 | 310 |
| Tower | `tower.png` | 128×256 | 230 |

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

## 6. UI

### 6.1 Shop panel

Opened with **Tab** while standing next to one of your shop buildings (House, Archery, Barracks,
Monastery) or an idle seller. **R** hires one unit directly without opening it (quick buy). Closed
with Tab, Esc, the red X, or by walking away. The world keeps running while it is open.

```
+-----------------------------------------------------------------------------------+
|  TOP-DOWN GAME WORLD                                                              |
|                                                                                   |
|                                     (Mouse Click)                                 |
|                                          │                                        |
|                                          ▼                                        |
|                                  [Unit / Building]                                |
|                                          │                                        |
|  ========================================│======================================  |
|  FULLSCREEN UI OVERLAY (Shop Screen)     │ (Raycast Hit triggers UI)              |
|                                          ▼                                        |
|  +-----------------------------------------------------------------------------+  |
|  | SHOP INSPECTION PANEL / texture: avator of the unit                 icon_9  |  |
|  |                                                                             |  |
|  |  +-----------------------------+    +------------------------------------+  |  |
|  |  | 2D PREVIEW / RENDER TEXTURE |    | STATS OVERLAY                      |  |  |
|  |  |                             |    | Name: Heavy Archer                 |  |  |
|  |  |        (o/  <- Unit         |    | ATK : 85                           |  |  |
|  |  |       /|    Attacking       |    | DEF : 40                           |  |  |
|  |  |       / \   Animation       |    | SPD : 12                           |  |  |
|  |  +-----------------------------+    +------------------------------------+  |  |
|  |                                                                             |  |
|  |                                             +----------------------------+  |  |
|  |                                             | [ BUY UNIT - $150 ]        |  |  |
|  |                                             +----------------------------+  |  |
|  +-----------------------------------------------------------------------------+  |
|                                                                                   |
+-----------------------------------------------------------------------------------+
```

| Part | Asset | Notes |
|---|---|---|
| Panel | `wood_table.png` | nine-slice; the metal corners come from the sheet's corner tiles |
| Avatar | `human_avatars/avatars_NN.png` | five faces per colour (blue 01–05, red 06–10, yellow 11–15, purple 16–20, black 21–25) in the order Warrior, Lancer, Archer, Monk, Pawn; the shop owner's colour, Blue for idle sellers (`avatar_index` in `src/ui.rs`) |
| 2D preview | the unit's Attack strip (`units/<colour>_units/…`) | looping, no background |
| Stats | `papers/regular_paper.png` | nine-slice; name, HP, DEF, ATK range, SPD, RANGE, price, slot cost |
| Buy button | `buttons/big_red_button_regular.png` / `_pressed.png` | nine-slice; the big button because "BUY - 10g" needs the width. Buys one unit per click through the same `try_buy` as `E` |
| Close | `icons/icon_09.png` | the red X |
| Status line | – | gold, army slots and the last message (full army, not enough gold, hired) |

The pack's panels and buttons are sheets of nine separate tiles with gaps, which Bevy's
`NodeImageMode::Sliced` cannot read directly. The tiles were stitched into contiguous nine-slice
images in `assets/ui_elements/generated/*_9.png`; the border sizes measured while stitching are the
`sliced(..)` values in `overworld/shop.rs`:

| Generated file | Size | Borders left / right / top / bottom |
|---|---|---|
| `wood_table_9.png` | 232×252 | 84 / 84 / 85 / 103 |
| `regular_paper_9.png` | 168×153 | 52 / 52 / 44 / 45 |
| `big_red_button_regular_9.png` | 154×158 | 45 / 45 / 47 / 47 |
| `big_red_button_pressed_9.png` | 164×149 | 50 / 50 / 36 / 49 |

`ImageNode` draws in its **content box** by default (`VisualBox::ContentBox`), which leaves the
node's padding bare; the panels use `VisualBox::BorderBox` (`panel()` in `overworld/shop.rs`) so the
wood and paper cover the whole node including the padding.

### 6.3 Castle panel (`overworld/castle_panel.rs`)

`Tab` next to your own castle. Same wood table; under the title a **small blue ribbon**
(`ribbons/small_ribbons.png`, the forked-end row of each colour stitched into
`generated/ribbon_small_<colour>_3.png`, 188×60, caps 62 px) carries the **five avatars** in the
same order as the number keys: Warrior, Archer, Lancer, Monk, Pawn. The **selected kind shows its blue avatar, the others their
black one** (blue `avatars_01` ↔ black `avatars_21`, i.e. +20). Click an avatar or press 1–5 to
select. Below: the idle animation of the selected kind, a paper with how many are in the castle and
with you plus the stats, the sword army count, and **TAKE / STORE** buttons that move one unit (hold
Ctrl: all) through the same `transfer` function as the number keys. The number keys keep working
with the panel open.

### 6.4 Army page (`overworld/army_panel.rs`)

`E` anywhere on the overworld (E again, Esc or the X closes it). A wood table with **your troops
listed downwards**, one row per kind you carry with its count: black avatars, the **selected row in
blue**, like the castle ribbon but vertical. Click a row to select. The selected kind idles **top
right** with its stats on a paper **under** it. The page is rebuilt whenever the selection or the
army changes, so a unit bought or lost shows at once. With no troops it says where to get some.

### 6.5 Combat HUD (`combat/hud.rs`)

- **Turn queue** (top, on a wood bar): one avatar per stack in its side's colour (yours blue, the
  enemy's in its kingdom colour) with the count under it. The queue is **rotated so whoever acts is at
  the far left**, bigger and on a small ribbon in its team colour (blue / red); when it is done it
  drops to the far right and the rest shift left.
- **Info panel** (bottom left, wood table): the acting stack's avatar, a **small ribbon reading
  "<Kind>'s turn"** whose colour says whose turn it is (blue = yours, the enemy's kingdom colour =
  theirs) and the AP; the stack's stats on paper; the hovered stack; and the red buttons
  **UNSTACK 1 / SHIELD 3 / END TURN**, which do exactly what the keys do and only on your turn.
- **Log** (bottom right) on paper.

### 6.2 Shared pieces (`src/ui.rs`)

- **Gold is never written as a word**: wherever your gold total shows (overworld HUD, shop status
  line) it is `icons/icon_03.png` (the coin) followed by the number (`gold_stat`).
- **Army the same**: `icons/icon_05.png` (the sword) followed by `used/max` (`army_stat`).
- **Finish texts use the big ribbons**: VICTORY (blue ribbon) and DEFEAT (red) as a title ribbon
  with the details on a dark card beneath (`finish_banner`); the last win, YOU UNITED THE LAND, sits
  entirely on one blue ribbon drawn at 2.2× (`big_ribbon`, scaled with `UiTransform`). `ribbons/big_ribbons.png` is
  five colours × three pieces; each row was stitched into `generated/ribbon_<colour>_3.png`
  (259×103, caps 98 / 97 px) and drawn as a three-slice with a stretched middle.
- `panel`, `sliced`, `icon` are the building blocks every panel uses.

Buttons are plain UI nodes with picking observers (`On<Pointer<Press / Release / Click>>`), attached
by `wire_shop_buttons` after the panel is spawned, so there is no per-frame "is the mouse over X"
checking anywhere. Resource: `OpenShop(Option<ShopTarget>)`; `None` despawns the panel.

Not done yet: the world map as a UI texture instead of a generated image.

---

## 7. Roadmap

- [x] Placeholder build: coloured squares, labels, states, grid, turn order, move + strike
- [x] Unstack / Merge / Shield up, archer range, flanking
- [x] Generated map (noise), bases with buildings, towers, sellers, income, loot
- [x] Castle garrison with transfer keys, hero slot limit, hiring
- [x] AI armies grabbing towers, bandit squads, respawns, defeat penalty
- [ ] Heal for Monk
- [ ] Kingdom-specific unit stats (Red heavy, Purple swarm, Yellow glass cannon)
- [ ] Smarter enemy AI in combat (Unstack / Merge / Shield up)
- [x] AI armies and bandits path around water and use ramps; armies retreat and refill
- [ ] AI kingdoms that attack
- [x] Terrain tiles, cliffs, ramps and decorations from the sprite map
- [x] Patch-based world with plateaus up to layer 5, terraces and `R` ramps checked so every plateau can be reached
- [x] Unit sprites with idle / run / attack / guard
- [x] Arrows and death dust
- [x] Tower and base building sprites with the capture animation
- [x] Shop panel and castle panel with the pack's UI (wood table, paper, buttons, ribbons, avatars)
- [x] Combat HUD with the pack's UI (turn queue of avatars, ribbons, red buttons)
- [ ] Water foam; hit and heal effects
