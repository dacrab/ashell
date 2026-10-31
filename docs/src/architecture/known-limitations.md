# Known Limitations and Design Debt

This chapter documents known architectural limitations and design tradeoffs. Understanding these helps contributors make informed decisions and avoid re-discovering known issues.

## Menu Surface Architecture

**Issue**: Context menus use a fullscreen transparent overlay surface (created while a menu is open). This has several drawbacks:

- **Memory waste**: ~140 MB VRAM per 4K monitor for a transparent surface
- **Layering bugs**: The overlay doesn't correctly layer popups on top of other windows in all cases

**Correct approach**: Use `zwlr_layer_surface_v1::get_popup` to create proper `xdg_popup` surfaces. However, iced_layershell does not currently expose this.

**Workaround**: The current fullscreen overlay approach was also chosen because iced has a HiDPI scaling regression where newly created surfaces initially render blurry.

**Related**: [GitHub Issue #491](https://github.com/MalpenZibo/ashell/issues/491)

## Memory Usage

**Issue**: ashell's process RSS (total memory) is 100–300 MB, despite the application heap being only ~3.5 MB.

**Breakdown** (from DHAT profiling):
- Font system (fontdb + cosmic_text): ~59% of peak heap
- Shader compilation (naga/wgpu): ~16%
- A bare wgpu application uses >50 MB RSS

**Factors that increase usage**:
- High-refresh-rate monitors (reported: 300 MB on a 240 Hz 49" ultra-wide)
- Multiple monitors
- Complex bar configurations with many modules

**Possible improvements**:
- The `tiny-skia` CPU renderer is built in (used as a fallback when wgpu fails); forcing it with `ICED_BACKEND=tiny-skia` reduces RAM usage at the cost of CPU usage and battery

**Related**: [GitHub Issue #529](https://github.com/MalpenZibo/ashell/issues/529)

## Services Refactoring

**Issue**: The service layer has inconsistencies across different services. Some use broadcast channels, others use mpsc. Error handling patterns vary.

**Status**: This is an ongoing refactoring effort tracked in [GitHub Issue #445](https://github.com/MalpenZibo/ashell/issues/445). The network service is the most problematic, with unreliable WiFi scanning and incompatible architectural patterns between the NetworkManager and IWD backends.

## Minimal Test Suite

**Issue**: CI only runs the `ashell-services` tests (`cargo test -p ashell-services --all-features`), even though the root crate has ~90 unit tests of its own across `config.rs`, `theme.rs`, `outputs.rs`, the tray and notification services, and others. Those tests run locally via `cargo test --workspace` but never gate a pull request. The UI is tested manually, and services are verified by running ashell on real hardware.

**Impact**: Regressions can slip through, especially for compositor-specific behavior (Hyprland vs. Niri) or hardware-specific features (brightness, Bluetooth). Root-crate tests can rot without anyone noticing, since nothing runs them in CI.

**Fix**: add `cargo test --workspace` to CI (and to the `check` target in the `Makefile`, which mirrors it).

## NVIDIA Compatibility

**Issue**: ashell can crash or fail to render on NVIDIA GPUs with the Niri compositor.

**Workaround**: Set the `WGPU_BACKEND=gl` environment variable to use the OpenGL backend instead of Vulkan.

**Related**: [GitHub Issue #471](https://github.com/MalpenZibo/ashell/issues/471)

## Icon Index Staleness

**Issue**: The XDG icon indexes in `crates/ashell-services/src/xdg_icons.rs` (`SYSTEM_ICON_ENTRIES`, `DESKTOP_ICON_INDEX`) are built once via `LazyLock` and never rebuilt. Resolution results, including misses, are cached for the process lifetime.

**Impact**: Newly installed applications and runtime icon-theme changes are not picked up until ashell is restarted. This affects both the tray and the workspace window icons.

**Why not fixed**: Because the indexes are static within a session, invalidating the result cache alone changes nothing (re-resolution reads the same frozen data), and dropping negative cache entries would only re-run the expensive fuzzy scans on every event for the same result. A proper fix means making the indexes rebuildable (a periodic or event-driven refresh), which is disproportionate to the payoff for a status bar.

## Menu Overlay Edge Cases

**Issue**: `MenuWrapper` positions menus by centring them on their button and
clamping to the screen edge. Two problems hid here: the width was never bounded
by the output, and the clamp itself inverted once content exceeded the viewport
(selecting the negative right-edge bound and pushing the menu off the left).

**Fixed**: the layout limits are now capped to `viewport - 2 * SCREEN_INSET`, so
no menu or tooltip can be laid out wider than the surface, and placement only
has to centre.

**Still open**: tooltips are capped and clipped at `TOOLTIP_MAX_WIDTH` rather
than wrapped, so an over-long SSID or device name is truncated instead of
reflowed. They also still share menus' card styling and differ only in
animation duration, so a tooltip and a popover look alike.

## Notification Group Disclosure

**Issue**: A notification group's header reads `expanded_groups` and renders
the right items for each state, but shows no chevron or other indicator. The
item count is identical collapsed and expanded, so the two states are visually
indistinguishable and click-to-expand is undiscoverable. Every other expandable
surface in the bar (sub-menus, tray sub-menus) uses a chevron.

**Why not fixed**: it needs a decision on where the chevron goes relative to
the clear button, and it belongs with the quick-settings menu work tracked in
[Quick-settings menu](settings-menu-design.md).

## Power Panel Padding

**Issue**: The power panel applies its own `padding(space.xs)` inside a menu
that already insets by `space.md`. Every other list relies on the shell's inset,
so the power panel's rows sit 8px from the menu edge where the battery panel's
sit 16px.

**Why not fixed**: purely visual, and the correct inset depends on whether
nested cards are kept (see [Quick-settings menu](settings-menu-design.md)).
