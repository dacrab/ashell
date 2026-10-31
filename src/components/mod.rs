mod animated_size;
pub mod button;
mod centerbox;
pub mod collapsible;
pub mod countdown_bar;
mod fade;
mod format_indicator;
pub mod icons;
pub mod menu;
mod menu_wrapper;
mod module_group;
mod module_item;
pub mod motion;
pub mod password_dialog;
mod position_button;
mod quick_setting_button;
pub mod share_wifi_dialog;
pub mod slide;
mod slider_control;
pub mod spinning_icon;
mod sub_menu_wrapper;

pub use animated_size::animated_size;
pub use button::*;
pub use centerbox::*;
pub use collapsible::collapsible;
pub use fade::panel;
pub use format_indicator::*;
pub use menu::MenuSize;
pub use menu_wrapper::*;
pub use module_group::*;
pub use module_item::*;
pub use position_button::*;
pub use quick_setting_button::*;
pub use slider_control::*;
pub use sub_menu_wrapper::*;

use iced::{
    Element,
    widget::{Scrollable, Toggler, rule},
};

pub fn divider<'a, Msg: 'static>() -> Element<'a, Msg> {
    rule::horizontal(1).style(crate::theme::rule_style).into()
}

/// Height of a dialog's confirm/cancel row, in layout pixels before
/// `scale_factor`. Scaled, so the row keeps its weight next to the tile grid.
pub fn dialog_action_height() -> iced::Length {
    iced::Length::Fixed(crate::theme::use_theme(|theme| theme.px(50.)))
}

pub fn scrollable<'a, Message>(
    content: impl Into<Element<'a, Message>>,
) -> Scrollable<'a, Message> {
    iced::widget::scrollable(content).style(crate::theme::scrollable_style)
}

pub fn toggler<'a, Message>(is_checked: bool) -> Toggler<'a, Message> {
    iced::widget::toggler(is_checked).style(crate::theme::toggler_style)
}
