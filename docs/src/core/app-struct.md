# The App Struct

The `App` struct in `src/app.rs` is the central state container for the entire application. It owns all module instances, the configuration, the theme, and the output/surface management.

## Fields

```rust
pub struct App {
    config_path: PathBuf,               // Path to the TOML config file
    pub theme: AshellTheme,             // Current theme (colors, spacing, fonts)
    pub general_config: GeneralConfig,  // Extracted config subset (outputs, modules, layer)
    pub outputs: Outputs,               // Multi-monitor surface management

    // Module instances
    pub custom: HashMap<String, Custom>,     // User-defined custom modules
    pub updates: Option<Updates>,            // Package update checker (optional)
    pub workspaces: Workspaces,              // Workspace indicators
    pub window_title: WindowTitle,           // Active window display
    pub system_info: SystemInfo,             // CPU/RAM/disk/network stats
    pub keyboard_layout: KeyboardLayout,     // Keyboard layout indicator
    pub keyboard_submap: KeyboardSubmap,     // Hyprland submap display
    pub tray: TrayModule,                    // System tray
    pub clock: Clock,                        // Time display (deprecated)
    pub tempo: Tempo,                        // Advanced clock/calendar/weather
    pub privacy: Privacy,                    // Mic/camera/screenshare indicators
    pub settings: Settings,                  // Settings panel
    pub media_player: MediaPlayer,           // MPRIS media control

    pub visible: bool,                       // Bar visibility (toggled via SIGUSR1)
}
```

## GeneralConfig

A subset of the config used at the App level:

```rust
pub struct GeneralConfig {
    outputs: config::Outputs,     // Which monitors to show the bar on
    pub modules: Modules,         // Left/center/right module layout
    pub layer: config::Layer,     // Wayland layer (Top/Bottom/Overlay)
    enable_esc_key: bool,         // Whether ESC closes menus
}
```

## Initialization

`App::new()` returns a closure that produces the initial state and a startup task:

```rust
pub fn new(
    (config, config_path): (Config, PathBuf),
) -> impl FnOnce() -> (Self, Task<Message>) {
    move || {
        let (outputs, task) = Outputs::new(/* style, position, layer, scale_factor */);

        // Initialize all modules from config
        let custom = config.custom_modules.into_iter()
            .map(|o| (o.name.clone(), Custom::new(o)))
            .collect();

        (App { /* all fields */ }, task)
    }
}
```

The startup task creates the initial layer surfaces.

## Config Hot-Reload

When the config file changes, `App::refesh_config()` propagates changes to all modules:

```rust
fn refesh_config(&mut self, config: Box<Config>) {
    // Update general config
    self.general_config = GeneralConfig { /* ... */ };

    // Update theme
    self.theme = AshellTheme::new(config.position, &config.appearance);

    // Update log level via tracing reload handle
    if let Some(setter) = crate::SET_LOG_LEVEL.get() {
        setter(&config.log_level);
    }

    // Sync outputs (may create/destroy surfaces)
    let task = self.outputs.sync(/* ... */);

    // Propagate to each module via ConfigReloaded messages
    self.workspaces.update(workspaces::Message::ConfigReloaded(config.workspaces));
    self.settings.update(settings::Message::ConfigReloaded(config.settings));
    // ... and so on for each module
}
```

This enables live editing of the config file without restarting ashell.
