use std::time::Duration;

use unicode_segmentation::UnicodeSegmentation;

pub mod launcher;
pub mod remote_value;

#[derive(Debug, Clone, Copy)]
pub enum IndicatorState {
    Normal,
    Success,
    Warning,
    Danger,
}

pub fn format_duration(duration: &Duration) -> String {
    let h = duration.as_secs() / 60 / 60;
    let m = duration.as_secs() / 60 % 60;
    if h > 0 {
        format!("{h}h {m:>2}m")
    } else {
        format!("{m:>2}m")
    }
}

/// Truncate a string to `max_length` graphemes by replacing the middle with
/// `...`, keeping both ends. Returns the input unchanged when short enough.
pub fn truncate_text(value: &str, max_length: u32) -> String {
    if value.graphemes(true).count() <= max_length as usize {
        return value.to_string();
    }

    let graphemes = value.graphemes(true).collect::<Vec<&str>>();
    let split = max_length as usize / 2;
    let last = graphemes.len() - split;
    format!(
        "{}...{}",
        graphemes[..split].concat(),
        graphemes[last..].concat()
    )
}

/// Clamp-adjust a value by `step`, bounded by `max`: adding saturates at
/// `max`, subtracting at zero. Shared by volume, microphone and brightness
/// adjustments.
pub fn stepped_value(cur: u32, up: bool, step: u32, max: u32) -> u32 {
    if up {
        (cur + step).min(max)
    } else {
        cur.saturating_sub(step)
    }
}

/// Vertical component of a scroll delta, shared by the scrollable sliders.
pub fn scroll_y(delta: iced::mouse::ScrollDelta) -> f32 {
    match delta {
        iced::mouse::ScrollDelta::Lines { y, .. } => y,
        iced::mouse::ScrollDelta::Pixels { y, .. } => y,
    }
}

pub fn floor_dp(num: f32, dp: i32) -> f32 {
    let scale = 10_f32.powi(dp);
    (num * scale).floor() / scale
}

pub fn bytes_to_gib(bytes: u64) -> f32 {
    bytes as f32 / 1_073_741_824_f32
}
pub fn bytes_to_gb(bytes: u64) -> f32 {
    bytes as f32 / 1_000_000_000_f32
}

pub fn celsius_to_fahrenheit(cel: i32) -> i32 {
    cel * 9 / 5 + 32
}
