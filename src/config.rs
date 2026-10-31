use crate::app::Message;
use crate::services::upower::PeripheralDeviceKind;
use crate::theme::Space;
use ashell_i18n::{TemperatureUnit, UnitSystem, unit_system};
use hex_color::HexColor;
use iced::futures::StreamExt;
use iced::{Color, Subscription, futures::SinkExt, stream::channel, theme::palette};
use inotify::EventMask;
use inotify::Inotify;
use inotify::WatchMask;
use log::{debug, error, info, warn};
use regex::Regex;
use serde::{
    Deserialize, Deserializer,
    de::{self, MapAccess, Visitor},
};
use serde_with::DisplayFromStr;
use serde_with::serde_as;
use std::path::PathBuf;
use std::time::Duration;
use std::{collections::HashMap, error::Error, ops::Deref, path::Path};
use tokio::time::sleep;

pub const DEFAULT_CONFIG_FILE_PATH: &str = "~/.config/ashell/config.toml";

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum LogTarget {
    #[default]
    File,
    Stdout,
    Stderr,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(default)]
pub struct LoggingConfig {
    pub level: String,
    pub target: LogTarget,
    pub directory: Option<PathBuf>,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "warn".to_owned(),
            target: LogTarget::default(),
            directory: None,
        }
    }
}

impl LoggingConfig {
    pub fn log_directory(&self) -> anyhow::Result<PathBuf> {
        let default_directory = || crate::xdg::private_dir(true);

        match &self.directory {
            Some(directory) => expand_path(directory.clone()).or_else(|e| {
                eprintln!(
                    "ashell: warning: cannot expand logging.directory {directory:?}: {e}, using the default"
                );
                default_directory()
            }),
            None => default_directory(),
        }
    }
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Config {
    pub logging: LoggingConfig,
    pub language: Option<String>,
    pub region: Option<String>,
    pub position: Position,
    pub layer: Layer,
    pub outputs: Outputs,
    pub modules: Modules,
    #[serde(rename = "CustomModule")]
    pub custom_modules: Vec<CustomModuleDef>,
    pub updates: Option<UpdatesModuleConfig>,
    pub workspaces: WorkspacesModuleConfig,
    pub window_title: WindowTitleConfig,
    pub system_info: SystemInfoModuleConfig,
    pub notifications: NotificationsModuleConfig,
    pub tray: TrayModuleConfig,
    pub tempo: TempoModuleConfig,
    pub settings: SettingsModuleConfig,
    pub appearance: Appearance,
    pub media_player: MediaPlayerModuleConfig,
    pub keyboard_layout: KeyboardLayoutModuleConfig,
    pub animations: AnimationsConfig,
    pub enable_esc_key: bool,
    pub osd: OsdConfig,
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct AnimationsConfig {
    pub enabled: bool,
}

impl Config {
    fn validate(&mut self) {
        if let Some(ref mut updates) = self.updates {
            updates.validate();
        }
        self.appearance.bar.margin.validate("appearance.bar.margin");
        self.appearance
            .bar
            .padding
            .validate("appearance.bar.padding");
        self.appearance.bar.border.validate("appearance.bar.border");
        self.system_info.validate();
        self.tempo.validate();
        self.settings.validate();
        self.media_player.validate();
        self.notifications.validate();
    }
}

#[derive(Deserialize, Clone, Debug)]
pub struct UpdatesModuleConfig {
    pub check_cmd: String,
    pub update_cmd: String,
    #[serde(default = "UpdatesModuleConfig::default_interval")]
    pub interval: u64,
}

impl UpdatesModuleConfig {
    const fn default_interval() -> u64 {
        3600
    }

    fn validate(&mut self) {
        if self.interval == 0 {
            warn!("UpdatesModuleConfig.interval is 0, setting to 1");
            self.interval = 1;
        }
    }
}

#[derive(Deserialize, Copy, Clone, Default, PartialEq, Eq, Debug)]
pub enum WorkspaceVisibilityMode {
    #[default]
    All,
    MonitorSpecific,
    MonitorSpecificExclusive,
}

#[derive(Deserialize, Copy, Clone, Default, PartialEq, Eq, Debug)]
pub enum WorkspaceIndicatorFormat {
    #[default]
    Name,
    NameAndIcons,
}

#[derive(Deserialize, Clone, Default, Debug)]
#[serde(default)]
pub struct WorkspacesModuleConfig {
    pub visibility_mode: WorkspaceVisibilityMode,
    pub indicator_format: WorkspaceIndicatorFormat,
    pub group_by_monitor: bool,
    pub enable_workspace_filling: bool,
    pub disable_special_workspaces: bool,
    pub max_workspaces: Option<u32>,
    pub workspace_names: Vec<String>,
    pub enable_virtual_desktops: bool,
    pub invert_scroll_direction: Option<InvertScrollDirection>,
}

#[derive(Deserialize, Copy, Clone, Default, PartialEq, Eq, Debug)]
pub enum InvertScrollDirection {
    #[default]
    All,
    Mouse,
    Trackpad,
}

#[derive(Deserialize, Copy, Clone, Default, PartialEq, Eq, Debug)]
pub enum WindowTitleMode {
    #[default]
    Title,
    Class,
    InitialTitle,
    InitialClass,
}

#[derive(Deserialize, Copy, Clone, Debug)]
#[serde(default)]
pub struct WindowTitleConfig {
    pub mode: WindowTitleMode,
    pub truncate_title_after_length: u32,
}

impl Default for WindowTitleConfig {
    fn default() -> Self {
        Self {
            mode: Default::default(),
            truncate_title_after_length: 150,
        }
    }
}

#[derive(Deserialize, Clone, Default, Debug)]
#[serde(default)]
pub struct KeyboardLayoutModuleConfig {
    pub labels: HashMap<String, String>,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(default)]
pub struct SystemInfoCpu {
    pub warn_threshold: u32,
    pub alert_threshold: u32,

    pub format: CpuFormat,
}

fn validate_thresholds<T: PartialOrd + Copy + std::fmt::Display>(
    warn: &mut T,
    alert: &mut T,
    name: &str,
) {
    if *warn >= *alert {
        warn!(
            "{name} warn_threshold ({warn}) >= alert_threshold ({alert}), setting both to {alert}"
        );
        *warn = *alert;
    }
}

impl SystemInfoCpu {
    fn validate(&mut self) {
        validate_thresholds(&mut self.warn_threshold, &mut self.alert_threshold, "CPU");
    }
}

impl Default for SystemInfoCpu {
    fn default() -> Self {
        Self {
            warn_threshold: 60,
            alert_threshold: 80,
            format: CpuFormat::Percentage,
        }
    }
}

#[derive(Deserialize, Clone, Debug)]
#[serde(default)]
pub struct SystemInfoMemory {
    pub warn_threshold: u32,
    pub alert_threshold: u32,
    pub format: MemoryFormat,
}

impl SystemInfoMemory {
    fn validate(&mut self) {
        validate_thresholds(
            &mut self.warn_threshold,
            &mut self.alert_threshold,
            "Memory",
        );
    }
}

impl Default for SystemInfoMemory {
    fn default() -> Self {
        Self {
            warn_threshold: 70,
            alert_threshold: 85,
            format: MemoryFormat::Percentage,
        }
    }
}

const DEFAULT_TEMP_WARN_CELSIUS: i32 = 60;
const DEFAULT_TEMP_ALERT_CELSIUS: i32 = 80;

#[derive(Deserialize, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum TemperatureSensorType {
    #[default]
    Cpu,
    Gpu,
    Acpi,
    Nvme,
}

/// A type keyword (auto-detected) or an exact sensor label.
#[derive(Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
#[serde(untagged)]
pub enum TemperatureSensor {
    Type(TemperatureSensorType),
    Label(String),
}

impl Default for TemperatureSensor {
    fn default() -> Self {
        Self::Type(TemperatureSensorType::default())
    }
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct SystemInfoTemperature {
    warn_threshold: Option<i32>,
    alert_threshold: Option<i32>,
    pub sensor: TemperatureSensor,
    units: Option<TemperatureUnit>,
}

impl SystemInfoTemperature {
    pub fn resolved_units(&self) -> TemperatureUnit {
        self.units
            .unwrap_or_else(|| unit_system().temperature_unit())
    }

    pub fn warn_threshold(&self) -> i32 {
        self.warn_threshold.unwrap_or_else(|| {
            self.resolved_units()
                .convert_celsius(DEFAULT_TEMP_WARN_CELSIUS)
        })
    }

    pub fn alert_threshold(&self) -> i32 {
        self.alert_threshold.unwrap_or_else(|| {
            self.resolved_units()
                .convert_celsius(DEFAULT_TEMP_ALERT_CELSIUS)
        })
    }

    fn validate(&mut self) {
        if let (Some(warn), Some(alert)) = (&mut self.warn_threshold, &mut self.alert_threshold) {
            validate_thresholds(warn, alert, "Temperature");
        }
    }
}

#[derive(Clone, Debug, Deserialize, Default)]
pub enum DiskFormat {
    #[default]
    Percentage,
    Fraction,
}

#[derive(Clone, Debug, Deserialize, Default)]
pub enum MemoryFormat {
    #[default]
    Percentage,
    Amount,
    Fraction,
}

#[derive(Clone, Debug, Deserialize, Default)]
pub enum CpuFormat {
    #[default]
    Percentage,
    Frequency,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(default)]
pub struct SystemInfoDisk {
    pub warn_threshold: u32,
    pub alert_threshold: u32,
    pub format: DiskFormat,
    pub mounts: Option<Vec<String>>,
}

impl SystemInfoDisk {
    fn validate(&mut self) {
        validate_thresholds(&mut self.warn_threshold, &mut self.alert_threshold, "Disk");
    }
}

impl Default for SystemInfoDisk {
    fn default() -> Self {
        Self {
            warn_threshold: 80,
            alert_threshold: 90,
            format: DiskFormat::Percentage,
            mounts: None,
        }
    }
}

#[derive(Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SystemInfoDiskIndicatorConfig {
    #[serde(rename = "Disk")]
    pub path: String,
    #[serde(rename = "Name")]
    pub name: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub enum SystemInfoIndicator {
    Cpu,
    Memory,
    MemorySwap,
    Temperature,
    IpAddress,
    DownloadSpeed,
    UploadSpeed,
    #[serde(untagged)]
    Disk(SystemInfoDiskIndicatorConfig),
}

#[derive(Deserialize, Clone, Debug)]
#[serde(default)]
pub struct SystemInfoModuleConfig {
    pub indicators: Vec<SystemInfoIndicator>,
    #[serde(default = "SystemInfoModuleConfig::default_interval")]
    pub interval: u64,
    pub cpu: SystemInfoCpu,
    pub memory: SystemInfoMemory,
    pub temperature: SystemInfoTemperature,
    pub disk: SystemInfoDisk,
}

impl SystemInfoModuleConfig {
    const fn default_interval() -> u64 {
        5
    }

    fn validate(&mut self) {
        if self.interval == 0 {
            warn!("SystemInfoModuleConfig.interval is 0, setting to 1");
            self.interval = 1;
        }
        self.cpu.validate();
        self.memory.validate();
        self.temperature.validate();
        self.disk.validate();
    }
}

impl Default for SystemInfoModuleConfig {
    fn default() -> Self {
        Self {
            indicators: vec![
                SystemInfoIndicator::Cpu,
                SystemInfoIndicator::Memory,
                SystemInfoIndicator::Temperature,
            ],
            interval: Self::default_interval(),
            cpu: SystemInfoCpu::default(),
            memory: SystemInfoMemory::default(),
            temperature: SystemInfoTemperature::default(),
            disk: SystemInfoDisk::default(),
        }
    }
}

#[derive(Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ToastPosition {
    TopLeft,
    #[default]
    TopRight,
    TopCenter,
    BottomLeft,
    BottomRight,
    BottomCenter,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(default)]
pub struct NotificationsModuleConfig {
    pub format: String,
    pub show_timestamps: bool,
    pub show_bodies: bool,
    pub grouped: bool,
    pub toast: bool,
    pub toast_position: ToastPosition,
    pub toast_timeout: u64,
    pub toast_limit: usize,
    pub toast_max_height: u32,
    pub history_limit: usize,
    pub toast_timeout_bar: bool,
    pub blocklist: Vec<RegexCfg>,
}
impl Default for NotificationsModuleConfig {
    fn default() -> Self {
        Self {
            format: "%H:%M".to_string(),
            show_timestamps: true,
            show_bodies: true,
            grouped: false,
            toast: true,
            toast_position: ToastPosition::default(),
            toast_timeout: 5000,
            toast_limit: 5,
            toast_max_height: 150,
            history_limit: 200,
            toast_timeout_bar: false,
            blocklist: vec![],
        }
    }
}

impl NotificationsModuleConfig {
    fn validate(&mut self) {
        let clamped = self.history_limit.clamp(1, 1000);
        if self.history_limit != clamped {
            warn!(
                "notifications.history_limit ({}) out of range 1..=1000, clamping to {clamped}",
                self.history_limit
            );
            self.history_limit = clamped;
        }
    }
}

#[derive(Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum TrayClickAction {
    Open,
    Menu,
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct TrayModuleConfig {
    pub blocklist: Vec<RegexCfg>,
    pub right_click: Option<TrayClickAction>,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(default)]
pub struct TempoModuleConfig {
    pub clock_format: String,
    #[serde(default)]
    pub formats: Vec<String>,
    #[serde(default)]
    pub timezones: Vec<String>,
    #[serde(default)]
    pub weather_location: Option<WeatherLocation>,
    pub weather_indicator: WeatherIndicator,
    #[serde(default)]
    pub wind_speed_unit: Option<WindSpeedUnit>,
}

impl TempoModuleConfig {
    pub fn resolved_wind_speed_unit(&self) -> WindSpeedUnit {
        match self.wind_speed_unit {
            Some(unit) => unit,
            None => match unit_system() {
                UnitSystem::Imperial => WindSpeedUnit::Mph,
                UnitSystem::Metric => WindSpeedUnit::Kmh,
            },
        }
    }

    fn validate(&mut self) {
        if let Some(WeatherLocation::Coordinates(lat, lon)) = &mut self.weather_location {
            let clamped_lat = lat.clamp(-90.0, 90.0);
            let clamped_lon = lon.clamp(-180.0, 180.0);
            if *lat != clamped_lat || *lon != clamped_lon {
                warn!(
                    "tempo.weather_location latitude ({lat}) must be in -90..=90 and longitude ({lon}) in -180..=180, setting to ({clamped_lat}, {clamped_lon})"
                );
                *lat = clamped_lat;
                *lon = clamped_lon;
            }
        }
    }
}

#[derive(Deserialize, Default, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WindSpeedUnit {
    #[default]
    Kmh,
    Mph,
    Ms,
}

impl WindSpeedUnit {
    pub fn symbol(self) -> &'static str {
        match self {
            Self::Kmh => "km/h",
            Self::Mph => "mph",
            Self::Ms => "m/s",
        }
    }

    pub fn api_param(self) -> &'static str {
        match self {
            Self::Kmh => "kmh",
            Self::Mph => "mph",
            Self::Ms => "ms",
        }
    }
}

#[derive(Deserialize, Default, Clone, Debug, PartialEq, Eq)]
pub enum WeatherIndicator {
    #[default]
    IconAndTemperature,
    Icon,
    None,
}

#[derive(Deserialize, Default, Clone, Debug, PartialEq)]
pub enum WeatherLocation {
    #[default]
    Current,
    City(String),
    Coordinates(f32, f32),
}

impl std::hash::Hash for WeatherLocation {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            WeatherLocation::Current => {}
            WeatherLocation::City(city) => city.hash(state),
            WeatherLocation::Coordinates(lat, lon) => {
                lat.to_bits().hash(state);
                lon.to_bits().hash(state);
            }
        }
    }
}

impl Default for TempoModuleConfig {
    fn default() -> Self {
        Self {
            clock_format: "%a %d %b %R".to_string(),
            formats: vec![],
            timezones: vec![],
            weather_location: None,
            weather_indicator: WeatherIndicator::IconAndTemperature,
            wind_speed_unit: None,
        }
    }
}

#[derive(Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum SettingsIndicator {
    IdleInhibitor,
    PowerProfile,
    Audio,
    Microphone,
    Network,
    Vpn,
    Bluetooth,
    Battery,
    PeripheralBattery,
    Brightness,
}

#[derive(Deserialize, Copy, Clone, Default, PartialEq, Eq, Debug)]
pub enum SettingsFormat {
    Icon,
    #[serde(alias = "Value")]
    Percentage,
    #[default]
    #[serde(alias = "IconAndValue")]
    IconAndPercentage,
    Time,
    IconAndTime,
    Name,
    IconAndName,
    PercentageAndTime,
    IconAndPercentageAndTime,
}

#[derive(Deserialize, Clone, Default, PartialEq, Eq, Debug)]
pub enum PeripheralIndicators {
    #[default]
    All,
    Specific(Vec<PeripheralDeviceKind>),
}

#[derive(Deserialize, Clone, Debug)]
#[serde(default)]
pub struct SettingsModuleConfig {
    #[serde(default, deserialize_with = "empty_string_as_none")]
    pub lock_cmd: Option<String>,
    pub shutdown_cmd: String,
    pub suspend_cmd: String,
    #[serde(default, deserialize_with = "empty_string_as_none")]
    pub hibernate_cmd: Option<String>,
    pub reboot_cmd: String,
    pub logout_cmd: String,
    pub battery_format: SettingsFormat,
    pub battery_hide_when_full: bool,
    pub peripheral_indicators: PeripheralIndicators,
    pub peripheral_battery_format: SettingsFormat,
    pub peripheral_expanded_by_default: bool,
    pub battery_health: bool,
    pub volume_step: u8,
    pub max_volume: u8,
    pub audio_indicator_format: SettingsFormat,
    pub microphone_indicator_format: SettingsFormat,
    pub network_indicator_format: SettingsFormat,
    pub bluetooth_indicator_format: SettingsFormat,
    pub brightness_indicator_format: SettingsFormat,
    #[serde(default, deserialize_with = "empty_string_as_none")]
    pub audio_sinks_more_cmd: Option<String>,
    #[serde(default, deserialize_with = "empty_string_as_none")]
    pub audio_sources_more_cmd: Option<String>,
    #[serde(default, deserialize_with = "empty_string_as_none")]
    pub audio_sink_post_switch_cmd: Option<String>,
    #[serde(default, deserialize_with = "empty_string_as_none")]
    pub audio_source_post_switch_cmd: Option<String>,
    #[serde(default, deserialize_with = "empty_string_as_none")]
    pub wifi_more_cmd: Option<String>,
    #[serde(default, deserialize_with = "empty_string_as_none")]
    pub vpn_more_cmd: Option<String>,
    #[serde(default, deserialize_with = "empty_string_as_none")]
    pub bluetooth_more_cmd: Option<String>,
    pub remove_airplane_btn: bool,
    pub remove_idle_btn: bool,
    pub remove_dnd_btn: bool,
    pub enable_tooltips: bool,
    pub indicators: Vec<SettingsIndicator>,
    #[serde(rename = "CustomButton")]
    pub custom_buttons: Vec<SettingsCustomButton>,
    pub keyboard_backlight_slider: bool,
}

impl Default for SettingsModuleConfig {
    fn default() -> Self {
        Self {
            lock_cmd: Default::default(),
            shutdown_cmd: "shutdown now".to_string(),
            suspend_cmd: "systemctl suspend".to_string(),
            hibernate_cmd: Default::default(),
            reboot_cmd: "systemctl reboot".to_string(),
            logout_cmd: "loginctl kill-user $(whoami)".to_string(),
            battery_format: SettingsFormat::IconAndPercentage,
            battery_hide_when_full: false,
            peripheral_indicators: Default::default(),
            peripheral_battery_format: SettingsFormat::Icon,
            peripheral_expanded_by_default: false,
            battery_health: true,
            volume_step: 5,
            max_volume: 100,
            audio_indicator_format: SettingsFormat::Icon,
            microphone_indicator_format: SettingsFormat::Icon,
            network_indicator_format: SettingsFormat::Icon,
            bluetooth_indicator_format: SettingsFormat::Icon,
            brightness_indicator_format: SettingsFormat::Icon,
            audio_sinks_more_cmd: Default::default(),
            audio_sources_more_cmd: Default::default(),
            audio_sink_post_switch_cmd: Default::default(),
            audio_source_post_switch_cmd: Default::default(),
            wifi_more_cmd: Default::default(),
            vpn_more_cmd: Default::default(),
            bluetooth_more_cmd: Default::default(),
            remove_airplane_btn: Default::default(),
            remove_idle_btn: Default::default(),
            remove_dnd_btn: Default::default(),
            enable_tooltips: true,
            indicators: vec![
                SettingsIndicator::IdleInhibitor,
                SettingsIndicator::PowerProfile,
                SettingsIndicator::Audio,
                SettingsIndicator::Microphone,
                SettingsIndicator::Bluetooth,
                SettingsIndicator::Network,
                SettingsIndicator::Vpn,
                SettingsIndicator::Battery,
            ],
            custom_buttons: Default::default(),
            keyboard_backlight_slider: Default::default(),
        }
    }
}

impl SettingsModuleConfig {
    fn validate(&mut self) {
        if self.max_volume == 0 || self.max_volume > 200 {
            warn!(
                "settings.max_volume ({}) out of range 1..=200, clamping to 100",
                self.max_volume
            );
            self.max_volume = 100;
        }
        if self.volume_step == 0 || self.volume_step > 50 {
            warn!(
                "settings.volume_step ({}) out of range 1..=50, clamping to 5",
                self.volume_step
            );
            self.volume_step = 5;
        }
    }
}

#[derive(Deserialize, Clone, Debug)]
pub struct SettingsCustomButton {
    pub name: String,
    pub icon: String,
    pub command: String,
    #[serde(default, deserialize_with = "empty_string_as_none")]
    pub status_command: Option<String>,
    pub tooltip: Option<String>,
}

#[derive(Deserialize, Copy, Clone, Default, PartialEq, Eq, Debug)]
pub enum MediaPlayerFormat {
    Icon,
    #[serde(alias = "Title")]
    Text,
    #[serde(alias = "IconAndTitle")]
    #[default]
    IconAndText,
}

#[derive(Deserialize, Copy, Clone, PartialEq, Eq, Debug)]
pub enum MediaPlayerTextField {
    Artist,
    Title,
    Album,
}

#[derive(Deserialize, Copy, Clone, PartialEq, Eq, Debug)]
pub enum MediaPlayerVisualizer {
    Background,
    Before,
    After,
}

/// What a mouse button on the media player indicator does.
#[derive(Deserialize, Copy, Clone, Default, PartialEq, Eq, Debug)]
pub enum MediaPlayerButtonAction {
    /// Open the media player menu.
    Menu,
    Prev,
    PlayPause,
    Next,
    #[default]
    None,
}

/// What the scroll wheel on the media player indicator does. Separate from
/// [`MediaPlayerButtonAction`] because the click actions make no sense on a
/// wheel, and a wrong value should be rejected at parse time.
#[derive(Deserialize, Copy, Clone, Default, PartialEq, Eq, Debug)]
pub enum MediaPlayerScrollAction {
    /// Change the volume of the active player, in 5% steps.
    Volume,
    #[default]
    None,
}

#[derive(Deserialize, Copy, Clone, PartialEq, Eq, Debug)]
#[serde(default)]
pub struct MediaPlayerIndicatorControls {
    pub left: MediaPlayerButtonAction,
    pub middle: MediaPlayerButtonAction,
    pub right: MediaPlayerButtonAction,
    pub scroll: MediaPlayerScrollAction,
}

impl Default for MediaPlayerIndicatorControls {
    fn default() -> Self {
        Self {
            left: MediaPlayerButtonAction::Menu,
            middle: MediaPlayerButtonAction::None,
            right: MediaPlayerButtonAction::None,
            scroll: MediaPlayerScrollAction::None,
        }
    }
}

#[derive(Deserialize, Clone, Debug)]
#[serde(default)]
pub struct MediaPlayerModuleConfig {
    pub indicator_format: MediaPlayerFormat,
    pub indicator_fields: Vec<MediaPlayerTextField>,
    pub max_text_length: u32,
    pub indicator_visualizer: Option<MediaPlayerVisualizer>,
    pub menu_visualizer: bool,
    pub visualizer_framerate: u32,
    pub indicator_controls: MediaPlayerIndicatorControls,
}

impl Default for MediaPlayerModuleConfig {
    fn default() -> Self {
        MediaPlayerModuleConfig {
            indicator_format: MediaPlayerFormat::default(),
            indicator_fields: vec![MediaPlayerTextField::Artist, MediaPlayerTextField::Title],
            max_text_length: 100,
            indicator_visualizer: None,
            menu_visualizer: false,
            visualizer_framerate: Self::DEFAULT_VISUALIZER_FRAMERATE,
            indicator_controls: MediaPlayerIndicatorControls::default(),
        }
    }
}

impl MediaPlayerModuleConfig {
    const DEFAULT_VISUALIZER_FRAMERATE: u32 = 30;
    const MAX_VISUALIZER_FRAMERATE: u32 = 144;

    fn validate(&mut self) {
        let clamped = self
            .visualizer_framerate
            .clamp(1, Self::MAX_VISUALIZER_FRAMERATE);
        if clamped != self.visualizer_framerate {
            warn!(
                "MediaPlayerModuleConfig.visualizer_framerate is {}, setting to {clamped}",
                self.visualizer_framerate
            );
            self.visualizer_framerate = clamped;
        }
    }
}

fn hex_to_color(hex: HexColor) -> Color {
    Color::from_rgb8(hex.r, hex.g, hex.b)
}

fn hex_to_pair(hex: HexColor, text: Option<HexColor>, text_fallback: Color) -> palette::Pair {
    palette::Pair::new(
        hex_to_color(hex),
        text.map(hex_to_color).unwrap_or(text_fallback),
    )
}

#[derive(Deserialize, Clone, Copy, Debug)]
#[serde(untagged)]
pub enum AppearanceColor {
    Simple(HexColor),
    Complete {
        base: HexColor,
        strong: Option<HexColor>,
        weak: Option<HexColor>,
        text: Option<HexColor>,
    },
}

impl AppearanceColor {
    pub fn get_base(&self) -> Color {
        match self {
            AppearanceColor::Simple(color) => hex_to_color(*color),
            AppearanceColor::Complete { base, .. } => hex_to_color(*base),
        }
    }

    pub fn get_text(&self) -> Option<Color> {
        match self {
            AppearanceColor::Simple(_) => None,
            AppearanceColor::Complete { text, .. } => text.map(hex_to_color),
        }
    }

    pub fn get_weak_pair(&self, text_fallback: Color) -> Option<palette::Pair> {
        match self {
            AppearanceColor::Simple(_) => None,
            AppearanceColor::Complete { weak, text, .. } => {
                weak.map(|color| hex_to_pair(color, *text, text_fallback))
            }
        }
    }

    pub fn get_strong_pair(&self, text_fallback: Color) -> Option<palette::Pair> {
        match self {
            AppearanceColor::Simple(_) => None,
            AppearanceColor::Complete { strong, text, .. } => {
                strong.map(|color| hex_to_pair(color, *text, text_fallback))
            }
        }
    }
}

#[derive(Deserialize, Clone, Copy, Debug)]
#[serde(untagged)]
pub enum BackgroundAppearanceColor {
    Simple(HexColor),
    Complete {
        base: HexColor,
        weakest: Option<HexColor>,
        weaker: Option<HexColor>,
        weak: Option<HexColor>,
        neutral: Option<HexColor>,
        strong: Option<HexColor>,
        stronger: Option<HexColor>,
        strongest: Option<HexColor>,
        text: Option<HexColor>,
    },
}

impl BackgroundAppearanceColor {
    pub fn get_base(&self) -> Color {
        match self {
            BackgroundAppearanceColor::Simple(color) => hex_to_color(*color),
            BackgroundAppearanceColor::Complete { base, .. } => hex_to_color(*base),
        }
    }

    pub fn get_text(&self) -> Option<Color> {
        match self {
            BackgroundAppearanceColor::Simple(_) => None,
            BackgroundAppearanceColor::Complete { text, .. } => text.map(hex_to_color),
        }
    }

    pub fn get_pair(&self, level: BackgroundLevel, text_fallback: Color) -> Option<palette::Pair> {
        match self {
            BackgroundAppearanceColor::Simple(_) => None,
            BackgroundAppearanceColor::Complete {
                weakest,
                weaker,
                weak,
                neutral,
                strong,
                stronger,
                strongest,
                text,
                ..
            } => {
                let hex = match level {
                    BackgroundLevel::Weakest => *weakest,
                    BackgroundLevel::Weaker => *weaker,
                    BackgroundLevel::Weak => *weak,
                    BackgroundLevel::Neutral => *neutral,
                    BackgroundLevel::Strong => *strong,
                    BackgroundLevel::Stronger => *stronger,
                    BackgroundLevel::Strongest => *strongest,
                };
                hex.map(|h| hex_to_pair(h, *text, text_fallback))
            }
        }
    }
}

#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum BackgroundLevel {
    Weakest,
    Weaker,
    Weak,
    Neutral,
    Strong,
    Stronger,
    Strongest,
}

#[derive(Deserialize, Default, Copy, Clone, Eq, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum BarSurface {
    #[default]
    Transparent,
    Solid,
}

#[derive(Deserialize, Default, Copy, Clone, Eq, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum RadiusSize {
    #[default]
    None,
    Sm,
    Md,
    Lg,
    Xl,
}

#[derive(Deserialize, Default, Copy, Clone, Eq, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum SpaceSize {
    #[default]
    None,
    Xxs,
    Xs,
    Sm,
    Md,
    Lg,
    Xl,
    Xxl,
}

#[derive(Deserialize, Copy, Clone, Eq, PartialEq, Debug)]
#[serde(untagged)]
pub enum MarginSize {
    SpaceSize(SpaceSize),
    Pixels(Pixels),
}

impl Default for MarginSize {
    fn default() -> Self {
        Self::SpaceSize(SpaceSize::default())
    }
}

/// Layer-shell accepts negative margins, which silently push the bar off
/// screen; a negative or non-finite length here is always a mistake.
fn is_valid_length(length: f32) -> bool {
    length.is_finite() && length >= 0.0
}

/// Lengths stored in resolved pixels rather than a step of the spacing scale,
/// so they carry no dependency on `[appearance.scale]`. `custom`, `resolve`
/// and `deserialize` are the only door into it, which keeps fixed pixels out
/// of the size tokens.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Pixels(i32);

impl Pixels {
    /// Rounds to whole pixels: a bar sits on a device grid, and a fractional
    /// margin is rounded by the compositor anyway.
    pub fn custom(value: f32) -> Self {
        Self(value.round() as i32)
    }
}

impl<'de> serde::Deserialize<'de> for Pixels {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        f32::deserialize(deserializer).map(Pixels::custom)
    }
}

impl From<Pixels> for f32 {
    fn from(value: Pixels) -> Self {
        value.0 as f32
    }
}

impl MarginSize {
    fn validate(&mut self, field: &str, edge: &str) {
        if let MarginSize::Pixels(pixels) = *self
            && !is_valid_length(f32::from(pixels))
        {
            warn!(
                "{field} {edge} ({}) is not a valid length, using 0",
                f32::from(pixels)
            );
            *self = MarginSize::Pixels(Pixels::custom(0.0));
        }
    }
}

impl MarginSize {
    /// The edge in logical pixels, with a spacing step resolved against the
    /// configured scale.
    pub fn resolve(self, space: crate::theme::Space) -> f32 {
        match self {
            MarginSize::SpaceSize(size) => space.resolve(size),
            MarginSize::Pixels(pixels) => pixels.into(),
        }
    }
}

impl From<MarginSize> for f32 {
    fn from(value: MarginSize) -> Self {
        match value {
            MarginSize::SpaceSize(space_size) => Space::default().resolve(space_size),
            MarginSize::Pixels(pixels) => pixels.into(),
        }
    }
}

impl From<MarginSize> for i32 {
    fn from(value: MarginSize) -> Self {
        match value {
            MarginSize::SpaceSize(space_size) => Space::default().resolve(space_size) as i32,
            MarginSize::Pixels(pixels) => pixels.0,
        }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum CssShorthand<T> {
    One(T),
    Many(Vec<T>),
}

impl<T: Copy> CssShorthand<T> {
    fn expand<E: serde::de::Error>(self) -> Result<[T; 4], E> {
        let values = match self {
            CssShorthand::One(v) => return Ok([v, v, v, v]),
            CssShorthand::Many(v) => v,
        };
        match values.as_slice() {
            [a] => Ok([*a, *a, *a, *a]),
            [a, b] => Ok([*a, *b, *a, *b]),
            [a, b, c, d] => Ok([*a, *b, *c, *d]),
            _ => Err(E::custom("expected 1, 2 or 4 values (CSS shorthand)")),
        }
    }
}

/// Per-corner radius selection, deserialized with CSS `border-radius` shorthand:
/// 1 value = all corners, 2 = `[top-left+bottom-right, top-right+bottom-left]`,
/// 4 = `[top-left, top-right, bottom-right, bottom-left]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct BarRadius {
    pub top_left: RadiusSize,
    pub top_right: RadiusSize,
    pub bottom_right: RadiusSize,
    pub bottom_left: RadiusSize,
}

impl<'de> Deserialize<'de> for BarRadius {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let [top_left, top_right, bottom_right, bottom_left] =
            CssShorthand::<RadiusSize>::deserialize(deserializer)?.expand()?;
        Ok(Self {
            top_left,
            top_right,
            bottom_right,
            bottom_left,
        })
    }
}

/// Per-edge margin selection, deserialized with CSS `margin` shorthand:
/// 1 value = all edges, 2 = `[vertical, horizontal]`, 4 = `[top, right, bottom, left]`.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct BarMargin {
    pub top: MarginSize,
    pub right: MarginSize,
    pub bottom: MarginSize,
    pub left: MarginSize,
}

impl<'de> Deserialize<'de> for BarMargin {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let [top, right, bottom, left] =
            CssShorthand::<MarginSize>::deserialize(deserializer)?.expand()?;
        Ok(Self {
            top,
            right,
            bottom,
            left,
        })
    }
}

impl BarMargin {
    fn validate(&mut self, field: &str) {
        self.top.validate(field, "top");
        self.right.validate(field, "right");
        self.bottom.validate(field, "bottom");
        self.left.validate(field, "left");
    }

    pub fn resolve(self, space: crate::theme::Space) -> ResolvedEdges {
        ResolvedEdges {
            top: self.top.resolve(space),
            right: self.right.resolve(space),
            bottom: self.bottom.resolve(space),
            left: self.left.resolve(space),
        }
    }
}

impl From<BarMargin> for (i32, i32, i32, i32) {
    fn from(value: BarMargin) -> Self {
        (
            value.top.into(),
            value.right.into(),
            value.bottom.into(),
            value.left.into(),
        )
    }
}

/// Per-edge inner spacing, deserialized with the CSS `padding` shorthand:
/// 1 value = all edges, 2 = `[vertical, horizontal]`, 4 = `[top, right, bottom, left]`.
///
/// Unlike `margin` this stays inside the layer surface, so a solid bar keeps its
/// full-width background and the bar still receives input at the screen edge.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct BarPadding {
    pub top: MarginSize,
    pub right: MarginSize,
    pub bottom: MarginSize,
    pub left: MarginSize,
}

impl<'de> Deserialize<'de> for BarPadding {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let [top, right, bottom, left] =
            CssShorthand::<MarginSize>::deserialize(deserializer)?.expand()?;
        Ok(Self {
            top,
            right,
            bottom,
            left,
        })
    }
}

impl BarPadding {
    fn validate(&mut self, field: &str) {
        self.top.validate(field, "top");
        self.right.validate(field, "right");
        self.bottom.validate(field, "bottom");
        self.left.validate(field, "left");
    }

    pub fn resolve(self, space: crate::theme::Space) -> ResolvedEdges {
        ResolvedEdges {
            top: self.top.resolve(space),
            right: self.right.resolve(space),
            bottom: self.bottom.resolve(space),
            left: self.left.resolve(space),
        }
    }
}

/// Per-edge lengths already resolved to logical pixels. A margin is a
/// layer-shell unit the compositor needs as an integer, so it converts through
/// `i32`; a padding stays inside the surface and stays fractional.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct ResolvedEdges {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl From<ResolvedEdges> for (i32, i32, i32, i32) {
    fn from(value: ResolvedEdges) -> Self {
        (
            value.top.round() as i32,
            value.right.round() as i32,
            value.bottom.round() as i32,
            value.left.round() as i32,
        )
    }
}

/// A palette entry, resolved from the surface's theme when drawing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaletteColor {
    Background(Option<BackgroundLevel>),
    Primary(Shade),
    Success(Shade),
    Warning(Shade),
    Danger(Shade),
    Text,
}

#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Shade {
    #[serde(skip)]
    Base,
    Weak,
    Strong,
}

impl PaletteColor {
    fn parse<E: de::Error>(value: &str) -> Result<Self, E> {
        fn variant<'de, T: Deserialize<'de>, E: de::Error>(value: &'de str) -> Result<T, E> {
            T::deserialize(de::value::StrDeserializer::new(value))
        }
        let shade = |shade: Option<&str>| shade.map_or(Ok(Shade::Base), variant);

        let (name, variant_name) = match value.split_once('.') {
            Some((name, variant_name)) => (name, Some(variant_name)),
            None => (value, None),
        };
        Ok(match (name, variant_name) {
            ("background", level) => PaletteColor::Background(level.map(variant).transpose()?),
            ("primary", v) => PaletteColor::Primary(shade(v)?),
            ("success", v) => PaletteColor::Success(shade(v)?),
            ("warning", v) => PaletteColor::Warning(shade(v)?),
            ("danger", v) => PaletteColor::Danger(shade(v)?),
            ("text", None) => PaletteColor::Text,
            _ => return Err(E::custom(format!("unknown palette color `{value}`"))),
        })
    }
}

/// A hex color (`#RRGGBB`) or a reference to the palette (`primary`, `background.strong`, ...).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorRef {
    Hex(HexColor),
    Palette(PaletteColor),
}

impl<'de> Deserialize<'de> for ColorRef {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        if value.starts_with('#') {
            HexColor::parse(&value)
                .map(ColorRef::Hex)
                .map_err(de::Error::custom)
        } else {
            PaletteColor::parse(&value).map(ColorRef::Palette)
        }
    }
}

#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(default)]
pub struct Border {
    pub width: f32,
    pub color: ColorRef,
}

impl Default for Border {
    fn default() -> Self {
        Self {
            width: 0.0,
            color: ColorRef::Palette(PaletteColor::Background(Some(BackgroundLevel::Strong))),
        }
    }
}

impl Border {
    fn validate(&mut self, field: &str) {
        if !is_valid_length(self.width) {
            warn!(
                "{field}.width ({}) is not a valid length, using 0",
                self.width
            );
            self.width = 0.0;
        }
    }
}

#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(default)]
pub struct BarAppearance {
    pub surface: BarSurface,
    pub radius: BarRadius,
    pub margin: BarMargin,
    pub padding: BarPadding,
    pub border: Border,
}

impl Default for BarAppearance {
    fn default() -> Self {
        let xxs = MarginSize::SpaceSize(SpaceSize::Xxs);
        Self {
            surface: BarSurface::default(),
            radius: BarRadius::default(),
            margin: BarMargin::default(),
            padding: BarPadding {
                top: xxs,
                right: xxs,
                bottom: xxs,
                left: xxs,
            },
            border: Border::default(),
        }
    }
}

#[derive(Deserialize, Default, Clone, Copy, Debug)]
#[serde(default)]
pub struct MenuAppearance {
    pub backdrop: f32,
}

/// `[appearance.scale]`: named steps for the two size scales every module draws
/// from. Named rather than raw numbers because the value that matters is the
/// whole rhythm, not one gap: a config that hand-tunes a single padding is how
/// a bar starts to feel assembled from parts.
#[derive(Deserialize, Default, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(default)]
pub struct ScaleAppearance {
    pub spacing: SpacingScale,
    pub radius: RadiusScale,
}

/// How tightly elements sit together. Steps the whole `Space` scale.
#[derive(Deserialize, Default, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SpacingScale {
    /// 3/6/9/12/18/24/36: fits many modules on a narrow bar.
    Tight,
    /// 3.5/7/10.5/14/21/28/42: a quarter below the default.
    Compact,
    #[default]
    /// 4/8/12/16/24/32/48.
    Default,
    /// 5/10/15/20/30/40/60: a quarter above the default.
    Comfortable,
    /// 6/12/18/24/36/48/72: a touch-friendly bar.
    Spacious,
}

/// How soft the corners are. Steps the whole `Radius` scale.
#[derive(Deserialize, Default, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RadiusScale {
    /// Square everywhere.
    Sharp,
    /// 2/4/8/16: slightly rounded.
    Subtle,
    #[default]
    /// 4/8/16/32.
    Default,
    /// 6/12/24/48: noticeably round.
    Rounded,
}

/// A layer-shell surface. Each one is drawn with its own theme.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Surface {
    Bar,
    Menu,
    Osd,
    Notifications,
}

impl Surface {
    pub const ALL: [Surface; 4] = [
        Surface::Bar,
        Surface::Menu,
        Surface::Osd,
        Surface::Notifications,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Opacity {
    default: f32,
    bar: Option<f32>,
    menu: Option<f32>,
    osd: Option<f32>,
    notifications: Option<f32>,
}

impl Opacity {
    pub fn get(&self, surface: Surface) -> f32 {
        match surface {
            Surface::Bar => self.bar,
            Surface::Menu => self.menu,
            Surface::Osd => self.osd,
            Surface::Notifications => self.notifications,
        }
        .unwrap_or(self.default)
    }
}

impl Default for Opacity {
    fn default() -> Self {
        Self {
            default: 1.0,
            bar: None,
            menu: None,
            osd: None,
            notifications: None,
        }
    }
}

impl<'de> Deserialize<'de> for Opacity {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct OpacityVisitor;

        impl<'de> Visitor<'de> for OpacityVisitor {
            type Value = Opacity;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str(
                    "a number between 0.0 and 1.0, or a table with `default` and per-surface overrides",
                )
            }

            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Opacity, E> {
                Ok(Opacity {
                    default: check_opacity(v).map_err(E::custom)?,
                    ..Opacity::default()
                })
            }

            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Opacity, E> {
                self.visit_f64(v as f64)
            }

            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Opacity, E> {
                self.visit_f64(v as f64)
            }

            fn visit_map<M: MapAccess<'de>>(self, map: M) -> Result<Opacity, M::Error> {
                #[derive(Deserialize)]
                struct Table {
                    default: Option<f64>,
                    bar: Option<f64>,
                    menu: Option<f64>,
                    osd: Option<f64>,
                    notifications: Option<f64>,
                }

                let table = Table::deserialize(de::value::MapAccessDeserializer::new(map))?;
                let check =
                    |v: Option<f64>| v.map(check_opacity).transpose().map_err(de::Error::custom);

                Ok(Opacity {
                    default: check(table.default)?.unwrap_or(1.0),
                    bar: check(table.bar)?,
                    menu: check(table.menu)?,
                    osd: check(table.osd)?,
                    notifications: check(table.notifications)?,
                })
            }
        }

        deserializer.deserialize_any(OpacityVisitor)
    }
}

#[derive(Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Appearance {
    pub font_name: Option<String>,
    pub font_weight: FontWeight,
    #[serde(deserialize_with = "scale_factor_deserializer")]
    pub scale_factor: f64,
    pub opacity: Opacity,
    pub bar: BarAppearance,
    pub menu: MenuAppearance,
    pub scale: ScaleAppearance,
    pub background_color: BackgroundAppearanceColor,
    pub primary_color: AppearanceColor,
    pub success_color: AppearanceColor,
    pub warning_color: AppearanceColor,
    pub danger_color: AppearanceColor,
    pub text_color: AppearanceColor,
    pub workspace_colors: Vec<AppearanceColor>,
    pub special_workspace_colors: Option<Vec<AppearanceColor>>,
    /// Blur the wallpaper behind ashell's translucent surfaces via
    /// `ext-background-effect-v1`. No-op where the protocol is unsupported.
    pub blur: BlurMode,
}

/// Weight of the face picked from `font_name`.
#[derive(Deserialize, Default, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FontWeight {
    Thin,
    ExtraLight,
    Light,
    #[default]
    Normal,
    Medium,
    SemiBold,
    Bold,
    ExtraBold,
    Black,
}

impl FontWeight {
    /// The CSS / OpenType numeric weight.
    pub fn value(self) -> u16 {
        match self {
            Self::Thin => 100,
            Self::ExtraLight => 200,
            Self::Light => 300,
            Self::Normal => 400,
            Self::Medium => 500,
            Self::SemiBold => 600,
            Self::Bold => 700,
            Self::ExtraBold => 800,
            Self::Black => 900,
        }
    }
}

/// When to ask the compositor for background blur.
#[derive(Deserialize, Default, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum BlurMode {
    /// Blur when `opacity < 1`; blurring an opaque surface would not show.
    #[default]
    Auto,
    /// Always ask, even at full opacity.
    Always,
    /// Never ask.
    Never,
}

impl BlurMode {
    pub fn enabled(self, opacity: f32) -> bool {
        match self {
            Self::Auto => opacity < 1.0,
            Self::Always => true,
            Self::Never => false,
        }
    }
}

static PRIMARY: HexColor = HexColor::rgb(122, 162, 247);

fn scale_factor_deserializer<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v = f64::deserialize(deserializer)?;

    if v <= 0.0 {
        return Err(serde::de::Error::custom(
            "Scale factor must be greater than 0.0",
        ));
    }

    if v > 2.0 {
        return Err(serde::de::Error::custom(
            "Scale factor cannot be greater than 2.0",
        ));
    }

    Ok(v)
}

fn check_opacity(v: f64) -> Result<f32, &'static str> {
    if v < 0.0 {
        return Err("Opacity cannot be negative");
    }

    if v > 1.0 {
        return Err("Opacity cannot be greater than 1.0");
    }

    Ok(v as f32)
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            font_name: None,
            font_weight: FontWeight::default(),
            scale_factor: 1.0,
            opacity: Opacity::default(),
            bar: BarAppearance::default(),
            menu: MenuAppearance::default(),
            scale: ScaleAppearance::default(),
            background_color: BackgroundAppearanceColor::Complete {
                base: HexColor::rgb(26, 27, 38),
                weakest: None,
                weaker: None,
                weak: Some(HexColor::rgb(36, 39, 58)),
                neutral: None,
                strong: Some(HexColor::rgb(65, 72, 104)),
                stronger: None,
                strongest: None,
                text: None,
            },
            primary_color: AppearanceColor::Simple(PRIMARY),
            success_color: AppearanceColor::Simple(HexColor::rgb(158, 206, 106)),
            warning_color: AppearanceColor::Simple(HexColor::rgb(224, 175, 104)),
            danger_color: AppearanceColor::Simple(HexColor::rgb(247, 118, 142)),
            text_color: AppearanceColor::Simple(HexColor::rgb(169, 177, 214)),
            workspace_colors: vec![
                AppearanceColor::Simple(PRIMARY),
                AppearanceColor::Simple(HexColor::rgb(158, 206, 106)),
            ],
            special_workspace_colors: None,
            blur: BlurMode::default(),
        }
    }
}

#[derive(Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Position {
    #[default]
    Top,
    Bottom,
}

#[derive(Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Layer {
    #[default]
    Bottom,
    Top,
    Overlay,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ModuleName {
    Updates,
    Workspaces,
    WindowTitle,
    SystemInfo,
    KeyboardLayout,
    KeyboardSubmap,
    Tray,
    Tempo,
    Privacy,
    Settings,
    MediaPlayer,
    Custom(String),
    Notifications,
}

/// Each builtin module's layout name and its snake_case name, used for its
/// settings section.
const BUILTIN_MODULES: [(&str, &str, ModuleName); 12] = [
    ("Updates", "updates", ModuleName::Updates),
    ("Workspaces", "workspaces", ModuleName::Workspaces),
    ("WindowTitle", "window_title", ModuleName::WindowTitle),
    ("SystemInfo", "system_info", ModuleName::SystemInfo),
    (
        "KeyboardLayout",
        "keyboard_layout",
        ModuleName::KeyboardLayout,
    ),
    (
        "KeyboardSubmap",
        "keyboard_submap",
        ModuleName::KeyboardSubmap,
    ),
    ("Tray", "tray", ModuleName::Tray),
    ("Notifications", "notifications", ModuleName::Notifications),
    ("Tempo", "tempo", ModuleName::Tempo),
    ("Privacy", "privacy", ModuleName::Privacy),
    ("Settings", "settings", ModuleName::Settings),
    ("MediaPlayer", "media_player", ModuleName::MediaPlayer),
];

impl std::fmt::Display for ModuleName {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            ModuleName::Custom(name) => f.write_str(name),
            builtin => f.write_str(
                BUILTIN_MODULES
                    .iter()
                    .find(|(_, _, module)| module == builtin)
                    .expect("every builtin module is listed")
                    .0,
            ),
        }
    }
}

impl<'de> Deserialize<'de> for ModuleName {
    fn deserialize<D>(deserializer: D) -> Result<ModuleName, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct ModuleNameVisitor;
        impl Visitor<'_> for ModuleNameVisitor {
            type Value = ModuleName;
            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a string representing a ModuleName")
            }
            fn visit_str<E>(self, value: &str) -> Result<ModuleName, E>
            where
                E: serde::de::Error,
            {
                Ok(BUILTIN_MODULES
                    .iter()
                    .find(|(name, _, _)| *name == value)
                    .map_or_else(
                        || ModuleName::Custom(value.to_string()),
                        |(_, _, module)| module.clone(),
                    ))
            }
        }
        deserializer.deserialize_str(ModuleNameVisitor)
    }
}

#[derive(Deserialize, Clone, Debug)]
#[serde(untagged)]
pub enum ModuleDef {
    Single(ModuleName),
    Group(Vec<ModuleName>),
}

#[derive(Deserialize, Clone, Debug)]
pub struct Modules {
    #[serde(default)]
    pub left: Vec<ModuleDef>,
    #[serde(default)]
    pub center: Vec<ModuleDef>,
    #[serde(default)]
    pub right: Vec<ModuleDef>,
}

impl Modules {
    pub fn contains(&self, target: &ModuleName) -> bool {
        self.left
            .iter()
            .chain(&self.center)
            .chain(&self.right)
            .any(|def| match def {
                ModuleDef::Single(m) => m == target,
                ModuleDef::Group(ms) => ms.contains(target),
            })
    }
}

impl Default for Modules {
    fn default() -> Self {
        Self {
            left: vec![ModuleDef::Single(ModuleName::Workspaces)],
            center: vec![ModuleDef::Single(ModuleName::WindowTitle)],
            right: vec![ModuleDef::Group(vec![
                ModuleName::Tempo,
                ModuleName::Privacy,
                ModuleName::Settings,
            ])],
        }
    }
}

#[derive(Deserialize, Clone, Default, Debug, PartialEq, Eq)]
pub enum Outputs {
    #[default]
    All,
    Active,
    #[serde(deserialize_with = "non_empty")]
    Targets(Vec<String>),
}

fn non_empty<'de, D, T>(d: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    let vec = <Vec<T>>::deserialize(d)?;
    if vec.is_empty() {
        use serde::de::Error;

        Err(D::Error::custom("need non-empty"))
    } else {
        Ok(vec)
    }
}

fn empty_string_as_none<'de, D>(d: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<String>::deserialize(d)?
        .and_then(|value| (!value.trim().is_empty()).then_some(value)))
}

/// Newtype wrapper around `Regex`to be deserializable and usable as a hashmap key
#[serde_as]
#[derive(Debug, Clone, Deserialize)]
#[serde(transparent)]
pub struct RegexCfg(#[serde_as(as = "DisplayFromStr")] pub Regex);

impl PartialEq for RegexCfg {
    fn eq(&self, other: &Self) -> bool {
        self.0.as_str() == other.0.as_str()
    }
}
impl Eq for RegexCfg {}

impl std::hash::Hash for RegexCfg {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // hash the raw pattern string
        self.0.as_str().hash(state);
    }
}

impl Deref for RegexCfg {
    type Target = Regex;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Deserialize, Copy, Clone, Default, PartialEq, Eq, Debug)]
pub enum CustomModuleType {
    #[default]
    Button,
    Text,
}

#[serde_as]
#[derive(Deserialize, Clone, Debug)]
pub struct CustomModuleDef {
    pub name: String,
    #[serde(default, deserialize_with = "empty_string_as_none")]
    pub command: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,

    /// yields json lines containing text, alt, (optionally tooltip)
    #[serde(default, deserialize_with = "empty_string_as_none")]
    pub listen_cmd: Option<String>,
    /// map of regex -> icon
    pub icons: Option<HashMap<RegexCfg, String>>,
    /// regex to show alert
    pub alert: Option<RegexCfg>,
    /// Display type: Button (clickable) or Text (display only)
    #[serde(default)]
    pub r#type: CustomModuleType,
    /// command to run on right-click
    #[serde(default, deserialize_with = "empty_string_as_none")]
    pub on_right_click: Option<String>,
    /// command to run on middle-click
    #[serde(default, deserialize_with = "empty_string_as_none")]
    pub on_middle_click: Option<String>,
    /// command to run on scroll up
    #[serde(default, deserialize_with = "empty_string_as_none")]
    pub on_scroll_up: Option<String>,
    /// command to run on scroll down
    #[serde(default, deserialize_with = "empty_string_as_none")]
    pub on_scroll_down: Option<String>,
    // .. appearance etc
}

#[derive(Deserialize, Clone, Debug)]
#[serde(default)]
pub struct OsdConfig {
    pub enabled: bool,
    pub timeout: u64,
    pub show_volume_percentage: bool,
    pub show_brightness_percentage: bool,
}

impl Default for OsdConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            timeout: 1500,
            show_volume_percentage: false,
            show_brightness_percentage: false,
        }
    }
}

/// Parses just the `[logging]` table, before the logger exists: no `log::*`
/// here, problems go to `stderr`.
pub fn read_logging_config(path: Option<&PathBuf>) -> LoggingConfig {
    #[derive(Deserialize)]
    struct LoggingConfigWrapper {
        #[serde(default)]
        logging: LoggingConfig,
    }

    let config_path = match resolve_config_path(path.map(PathBuf::as_path)) {
        Ok(p) => p,
        Err(e) => {
            eprintln!(
                "ashell: warning: cannot expand the config path: {e}, using the default logging setup"
            );
            return LoggingConfig::default();
        }
    };

    let content = match std::fs::read_to_string(&config_path) {
        Ok(c) => c,
        Err(e) => {
            if e.kind() != std::io::ErrorKind::NotFound {
                eprintln!(
                    "ashell: warning: cannot read {}: {e}, using the default logging setup",
                    config_path.display()
                );
            }
            return LoggingConfig::default();
        }
    };

    match toml::from_str::<LoggingConfigWrapper>(&content) {
        Ok(wrapper) => wrapper.logging,
        Err(_) => {
            // read_config reports the full parse error; this is only a hint,
            // and a syntax error in the file also lands here.
            eprintln!(
                "ashell: warning: cannot read the logging configuration from {}, using the default logging setup",
                config_path.display()
            );
            LoggingConfig::default()
        }
    }
}

fn resolve_config_path(path: Option<&Path>) -> Result<PathBuf, Box<dyn Error + Send>> {
    expand_path(match path {
        Some(p) => p.to_path_buf(),
        None => PathBuf::from(DEFAULT_CONFIG_FILE_PATH),
    })
}

/// Resolve the path in the caller's environment, so the daemon receives an
/// absolute path whatever its own working directory is.
pub fn resolve_config_arg(s: &str) -> Result<PathBuf, String> {
    let expanded = shellexpand::full(s).map_err(|e| e.to_string())?;
    std::env::current_dir()
        .map_err(|e| e.to_string())?
        .join(expanded.as_ref())
        .canonicalize()
        .map_err(|e| format!("invalid config file {s}: {e}"))
}

pub fn get_config(path: Option<&PathBuf>) -> Result<(Config, PathBuf), Box<dyn Error + Send>> {
    let explicit = path.is_some();
    let expanded = resolve_config_path(path.map(|p| p.as_path()))?;

    if explicit {
        info!("Config path provided {expanded:?}");

        if !expanded.exists() {
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Config file does not exist: {}", expanded.display()),
            )));
        }
    } else {
        // DEFAULT_CONFIG_FILE_PATH has a parent and shellexpand never strips components.
        let parent = expanded
            .parent()
            .expect("Failed to get default config parent directory");

        if !parent.exists() {
            std::fs::create_dir_all(parent)
                .expect("Failed to create default config parent directory");
        }
    }

    Ok((read_config(&expanded).unwrap_or_default(), expanded))
}

fn expand_path(path: PathBuf) -> Result<PathBuf, Box<dyn Error + Send>> {
    let str_path = path.to_string_lossy();
    let expanded =
        shellexpand::full(&str_path).map_err(|e| Box::new(e) as Box<dyn Error + Send>)?;

    Ok(PathBuf::from(expanded.to_string()))
}

pub fn read_config(path: &Path) -> Result<Config, Box<dyn Error + Send>> {
    let content =
        std::fs::read_to_string(path).map_err(|e| Box::new(e) as Box<dyn Error + Send>)?;

    info!("Decoding config file {path:?}");

    parse_config(&content).map_err(|e| {
        // Both syntax and type/value errors end up here, so report them in one
        // place instead of only covering the serde_ignored match.
        let msg = format!("Failed to parse config file {}: {e}", path.display());
        warn!("{msg}");
        eprintln!("ashell: error: {msg}");
        Box::new(e) as Box<dyn Error + Send>
    })
}

fn parse_config(content: &str) -> Result<Config, toml::de::Error> {
    let de = toml::Deserializer::parse(content)?;
    let mut unknown_fields = Vec::new();
    let mut config: Config = serde_ignored::deserialize(de, |path| {
        unknown_fields.push(path.to_string());
    })?;

    for field in &unknown_fields {
        let msg = format!("Unknown configuration field ignored: {field}");
        warn!("{msg}");
        eprintln!("ashell: warning: {msg}");
    }

    info!("Config file loaded successfully");
    config.validate();
    Ok(config)
}

enum Event {
    Changed,
    Removed,
}

pub fn subscription(path: &Path) -> Subscription<Message> {
    let path = path.to_path_buf();

    Subscription::run_with(path, |path| {
        let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.clone());
        channel(100, async move |mut output| {
            match (path.parent(), path.file_name(), Inotify::init()) {
                (Some(folder), Some(file_name), Ok(inotify)) => {
                    debug!("Watching config file at {path:?}");

                    let res = inotify.watches().add(
                        folder,
                        WatchMask::CREATE | WatchMask::DELETE | WatchMask::MOVE | WatchMask::MODIFY,
                    );

                    if let Err(e) = res {
                        error!("Failed to add watch for {folder:?}: {e}");
                        return;
                    }

                    let buffer = [0; 1024];
                    let stream = inotify.into_event_stream(buffer);

                    if let Ok(stream) = stream {
                        let mut stream = stream.ready_chunks(10);

                        debug!("Starting config file watch loop");

                        loop {
                            let Some(events) = stream.next().await else {
                                error!("inotify event stream ended, config hot-reload disabled");
                                return;
                            };

                            debug!("Received inotify events: {events:?}");

                            let mut file_event = None;

                            for event in events {
                                debug!("Event: {event:?}");
                                match event {
                                    Ok(inotify::Event {
                                        name: Some(name),
                                        mask: EventMask::DELETE | EventMask::MOVED_FROM,
                                        ..
                                    }) if file_name == name => {
                                        debug!("File deleted or moved");
                                        file_event = Some(Event::Removed);
                                    }
                                    Ok(inotify::Event {
                                        name: Some(name),
                                        mask:
                                            EventMask::CREATE | EventMask::MODIFY | EventMask::MOVED_TO,
                                        ..
                                    }) if file_name == name => {
                                        debug!("File created or moved");

                                        file_event = Some(Event::Changed);
                                    }
                                    _ => {
                                        debug!("Ignoring event");
                                    }
                                }
                            }

                            match file_event {
                                Some(Event::Changed) => {
                                    info!("Reload config file");

                                    let path_clone = path.clone();
                                    let new_config = tokio::task::spawn_blocking(move || {
                                        read_config(&path_clone).unwrap_or_default()
                                    })
                                    .await
                                    .unwrap_or_default();

                                    let _ = output
                                        .send(Message::ConfigChanged(Box::new(new_config)))
                                        .await;
                                }
                                Some(Event::Removed) => {
                                    // wait and double check if the file is really gone
                                    sleep(Duration::from_millis(500)).await;

                                    if !path.exists() {
                                        info!("Config file removed");
                                        let _ = output
                                            .send(Message::ConfigChanged(Box::default()))
                                            .await;
                                    }
                                }
                                None => {
                                    debug!("No relevant file event detected.");
                                }
                            }
                        }
                    } else {
                        error!("Failed to create inotify event stream");
                    }
                }
                (None, _, _) => {
                    error!(
                        "Config file path does not have a parent directory, cannot watch for changes"
                    );
                }
                (_, None, _) => {
                    error!("Config file path does not have a file name, cannot watch for changes");
                }
                (_, _, Err(e)) => {
                    error!("Failed to initialize inotify: {e}");
                }
            }
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(toml: &str) -> Result<Config, toml::de::Error> {
        toml::from_str(toml).map(|mut config: Config| {
            config.validate();
            config
        })
    }

    const BACKGROUND_STRONG: ColorRef =
        ColorRef::Palette(PaletteColor::Background(Some(BackgroundLevel::Strong)));

    #[test]
    fn scale_defaults_change_nothing() {
        let appearance = parse("").unwrap().appearance;
        assert_eq!(appearance.scale, ScaleAppearance::default());
        assert_eq!(appearance.scale.spacing, SpacingScale::Default);
        assert_eq!(appearance.scale.radius, RadiusScale::Default);
    }

    #[test]
    fn parses_scale_names() {
        let scale = parse("[appearance.scale]\nspacing = \"compact\"\nradius = \"rounded\"")
            .unwrap()
            .appearance
            .scale;
        assert_eq!(scale.spacing, SpacingScale::Compact);
        assert_eq!(scale.radius, RadiusScale::Rounded);
    }

    #[test]
    fn rejects_unknown_scale_names() {
        // A typo must not silently fall back to the default rhythm.
        assert!(parse("[appearance.scale]\nspacing = \"tightest\"").is_err());
        assert!(parse("[appearance.scale]\nradius = \"pill\"").is_err());
    }

    #[test]
    fn margin_pixels_still_parse() {
        use crate::config::Pixels;
        let bar = parse("[appearance.bar]\nmargin = 8")
            .unwrap()
            .appearance
            .bar;
        assert_eq!(bar.margin.top, MarginSize::Pixels(Pixels::custom(8.0)));
    }

    #[test]
    fn style_defaults_keep_current_look() {
        let appearance = parse("").unwrap().appearance;
        assert_eq!(appearance.bar.border.width, 0.0);
        assert_eq!(appearance.bar.border.color, BACKGROUND_STRONG);
    }

    #[test]
    fn border_fields_default_individually() {
        let bar = parse("[appearance.bar]\nborder = { width = 2 }")
            .unwrap()
            .appearance
            .bar;
        assert_eq!(bar.border.width, 2.0);
        assert_eq!(bar.border.color, BACKGROUND_STRONG);
    }

    #[test]
    fn negative_border_width_falls_back_to_zero() {
        let bar = parse("[appearance.bar]\nborder = { width = -1 }")
            .unwrap()
            .appearance
            .bar;
        assert_eq!(bar.border.width, 0.0);
    }

    #[test]
    fn parses_palette_references() {
        use PaletteColor::*;
        let cases = [
            ("background", Background(None)),
            (
                "background.weakest",
                Background(Some(BackgroundLevel::Weakest)),
            ),
            (
                "background.neutral",
                Background(Some(BackgroundLevel::Neutral)),
            ),
            (
                "background.strongest",
                Background(Some(BackgroundLevel::Strongest)),
            ),
            ("primary.weak", Primary(Shade::Weak)),
            ("success", Success(Shade::Base)),
            ("warning.strong", Warning(Shade::Strong)),
            ("danger.weak", Danger(Shade::Weak)),
            ("text", Text),
        ];
        for (name, expected) in cases {
            assert_eq!(
                PaletteColor::parse::<de::value::Error>(name),
                Ok(expected),
                "{name}"
            );
        }
    }

    #[test]
    fn rejects_unknown_colors() {
        for color in [
            "accent",
            "primary.base",
            "primary.strongest",
            "background.base",
            "text.weak",
            "#zzzzzz",
        ] {
            let toml = format!("[appearance.bar]\nborder = {{ color = \"{color}\" }}");
            assert!(parse(&toml).is_err(), "{color} should be rejected");
        }
    }

    #[test]
    fn syntax_error_is_a_parse_error() {
        assert!(parse_config("position = \"unterminated").is_err());
        assert!(parse_config("[bar").is_err());
        assert!(parse_config("position = 5").is_err());
    }
}
