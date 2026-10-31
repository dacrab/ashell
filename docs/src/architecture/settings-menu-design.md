# Quick-settings menu: information architecture

**Status**: Open design questions, not implemented. The tokens below were
resolved during a review pass; the hierarchy questions still need a decision.

This page records *what* the quick-settings menu does today and *why* it reads
as two different screens stacked together, so the open questions can be
answered without re-discovering the analysis.

## What the menu is today

`Settings::menu_view` (`src/modules/settings/mod.rs`) builds one column:

| Order | Row | Source | Interactive |
|---|---|---|---|
| 1 | Battery + Lock + Power | `power.battery_menu_indicator`, `icon_button` | battery opens sub-menu; lock acts |
| 2 | Battery sub-menu | `power.battery_menu` | collapsible |
| 3 | Power sub-menu | `power.menu` | collapsible |
| 4 | Sink slider (+ sink list) | `audio.sliders`, `sinks_submenu` | slider + chevron |
| 5 | Source slider (+ source list) | `audio.sliders`, `sources_submenu` | slider + chevron |
| 6 | Brightness slider | `brightness.slider` | slider |
| 7 | Keyboard backlight | `power.kbd_backlight_slider` | slider |
| 8 | Quick-setting grid | `quick_settings_section` | 2-up tiles |

The grid contains, in order: Wi-Fi, Bluetooth, VPN, Airplane, idle inhibitor,
DND, power profile, charge limit, custom buttons.

## The contradiction

Rows 1–3 and row 8 are **two mental models of the same settings**:

- The header is a **status bar**. The battery is read here; lock and power are
  icons that act immediately.
- The grid is a **control panel**. Every tile is icon + name + subtitle, toggles
  on left click and opens a detail sub-menu from its chevron.

Power appears in both: as a header icon (row 1, opens the *power actions*
sub-menu) and as a grid tile (row 8, toggles the *power profile*). They are not
the same control, but they look related and sit in one menu.

## Smaller inconsistencies

1. **Two gap sizes for the same gesture.** Sub-menus opened from rows 1–7 use
   `open_padding_top(space.md)`; sub-menus opened from the grid use
   `open_padding_top(space.xs)`.
2. **Grid and column gaps disagree** — the column uses `space.md`, the
   quick-setting grid uses `space.xs`.
3. **Sub-menu cards are nested cards.** `sub_menu_wrapper` renders a
   `card_style(radius.lg)` card inside the already-card-like menu body, so a
   tile's sub-menu reads as a panel within a panel.
4. **An odd tile count leaves a half-width tile beside dead space**
   (`row![left_button, space::horizontal()]`).

## Resolved

- **Tile height and radius.** The tile was 50px tall with a `radius.xl` (32px)
  corner — nearly a stadium, against 16px cards elsewhere. It is now
  `radius.lg`, and its height resolves through `px()` so it follows
  `scale_factor`.
- **Reaching a tile's detail view.** Previously the only affordance was the
  chevron; the tile now carries an explicit chevron button, so left click
  keeps toggling and the chevron opens the detail. This removed the need to
  choose between the two behaviours.

## Open questions

1. Should the final tile of an odd count span the full width, or stay
   half-width beside dead space?
2. Should the power *profile* stay a grid tile, or move into the header power
   menu, now that both are named "power"?
3. Should grid and column gaps be unified, and at which step?
