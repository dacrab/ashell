mod animated_size;
pub mod button;
mod centerbox;
pub mod collapsible;
mod format_indicator;
pub mod icons;
pub mod menu;
mod menu_wrapper;
mod module_group;
mod module_item;
pub mod password_dialog;
mod position_button;
mod quick_setting_button;
pub mod slide;
mod slider_control;
pub mod spinning_icon;
mod sub_menu_wrapper;

pub use animated_size::animated_size;
pub use button::*;
pub use centerbox::*;
pub use collapsible::collapsible;
pub use format_indicator::*;
pub use menu::MenuSize;
pub use menu_wrapper::*;
pub use module_group::*;
pub use module_item::*;
pub use position_button::*;
pub use quick_setting_button::*;
pub use slider_control::*;
pub use sub_menu_wrapper::*;

use crate::theme::use_theme;
use iced::{
    Element,
    widget::{Scrollable, Toggler, rule},
};
use std::time::Duration;

/// Duration shared by all UI animations (menus, slides, collapsibles,
/// size transitions). One knob to keep motion timing consistent.
pub const ANIMATION_DURATION: Duration = Duration::from_millis(100);

/// Fixed height of the settings quick-toggle rows and password-dialog
/// buttons.
pub const CONTROL_ROW_HEIGHT: f32 = 50.;

pub fn divider<'a, Msg: 'static>() -> Element<'a, Msg> {
    rule::horizontal(1).style(crate::theme::rule_style).into()
}

pub fn scrollable<'a, Message>(
    content: impl Into<Element<'a, Message>>,
) -> Scrollable<'a, Message> {
    iced::widget::scrollable(content).style(crate::theme::scrollable_style)
}

pub fn toggler<'a, Message>(is_checked: bool) -> Toggler<'a, Message> {
    iced::widget::toggler(is_checked).style(crate::theme::toggler_style)
}

/// Append the standard "More" button below a settings submenu when a
/// `*_more_cmd` is configured.
pub fn with_more_button<'a, Msg: 'static + Clone>(
    main: Element<'a, Msg>,
    more_msg: Option<Msg>,
) -> Element<'a, Msg> {
    use iced::widget::column;

    let space = use_theme(|t| t.space);
    match more_msg {
        Some(msg) => column!(
            main,
            divider(),
            crate::components::styled_button(crate::t!("settings-more"))
                .on_press(msg)
                .width(iced::Length::Fill)
        )
        .spacing(space.sm)
        .into(),
        None => main,
    }
}
