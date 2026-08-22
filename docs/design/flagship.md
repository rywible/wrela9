# Flagship Game

Status: accepted through product role, gameplay and completion spine, content envelope, melee combat scope, persistence boundary, visual acceptance set, and UI surface. Theme remains intentionally open and does not gate the architecture.

## Product role

The flagship is a deliberately small but genuinely playable silent third-person field-world exploration adventure. It proves that the language, compiler, graphical editor, runtime, Facilities, field renderer, and Event Store compose into a complete product. It is neither a disposable vertical slice nor expected to carry commercial-scale content.

## Content envelope

One complete playthrough lasts approximately ten to twenty minutes. It includes one continuous 3D overworld, one separate encounter or puzzle World, Screen-space menus, an explicit goal and ending, and `New`, `Continue`, and deliberate `Reset` flows. The player controls one articulated character, encounters a small set of creatures or mechanisms, collects and uses persistent objects, and causes meaningful Event-sourced changes that survive restart.

The game is silent because Audio is outside the current version. Its interactions and presentation must remain understandable without temporary host sound effects or an undeclared audio escape hatch.

## Gameplay and persistence

The flagship uses a small conventional melee combat suite as its primary demanding interaction rather than reducing the complete game to a walking demo or inventing one Wrela-specific universal verb. It includes movement and camera control, target lock, one light attack sequence, one dodge, health and damage, one enemy family with two bounded behaviors, defeat, victory, and checkpoint respawn. Ranged attacks, weapon switching, and a second combat model are outside the current flagship.

`Continue` reconstructs gameplay from checkpointed authoritative progression: collected objects, solved mechanisms, encounter outcomes, completion state, and the latest explicit checkpoint. It does not persist Mailboxes, Actor frames, animation phase, renderer caches, or an arbitrary mid-step player position. `Reset` commits a new Campaign Epoch through the Event Store instead of deleting host storage; projections select the current epoch while preserving the immutable history until ordinary Snapshot compaction reclaims it.

Combat health, attack phase, dodge state, and moment-to-moment enemy state are transient. Defeat reconstructs the last checkpoint at full health. Encounter victory becomes authoritative and visibly final only after its Event Transaction is durably acknowledged; a victory that has not crossed that boundary cannot survive `Continue` or be presented as saved.

## Visual acceptance set

The flagship contains elevated outdoor terrain, vegetation, water, architecture, an articulated player, one enemy family, collectible objects, interactive mechanisms, a separate combat arena, HUD, menus, and a visible ending. Theme, fiction, and art direction may be selected after these semantic subjects work and do not gate architectural completion by themselves.

## Completion sequence

`New` begins at a sanctuary checkpoint. The player explores the overworld, collects three objective objects, activates at least one persistent mechanism, unlocks the separate arena, defeats its enemy, and returns to a visible ending. `Continue` must demonstrate correct persistence after collection, mechanism activation, arena unlock, and durable victory.

The UI is limited to a `New`, `Continue`, and deliberate `Reset` title menu; health and objective HUD; target indicator; pause menu with fixed controls; durable-save status; bounded Facility-error presentation; and ending screen. Settings, runtime rebinding, text entry, localization, and a general inventory interface are outside the current version.
