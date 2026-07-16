use gpui::{App, Hsla, px, rgb};
use gpui_component::{Theme, ThemeMode};

fn color(hex: u32) -> Hsla {
    rgb(hex).into()
}

pub fn apply(cx: &mut App) {
    let theme = Theme::global_mut(cx);
    theme.mode = ThemeMode::Light;
    theme.font_family = ".SystemUIFont".into();
    theme.radius = px(4.0);
    theme.radius_lg = px(6.0);

    let primary = color(0x006fea);
    let primary_hover = color(0x005fc9);
    let primary_active = color(0x0052ae);
    let accent = color(0x0095dc);
    let secondary = color(0x86909b);
    let secondary_hover = color(0x77818c);
    let neutral = color(0x2f3338);
    let background = color(0xfbfcfd);
    let muted = color(0xeceff1);
    let raised = color(0xd5d8db);
    let foreground = color(0x171b1f);
    let border = color(0xb4b8bc);
    let info = color(0x0086d8);
    let success = color(0x299236);
    let warning = color(0xd29922);
    let warning_foreground = color(0x271700);
    let danger = color(0xcc272e);
    let white = color(0xffffff);

    theme.background = background;
    theme.foreground = foreground;
    theme.border = border;
    theme.input = border;
    theme.ring = primary;
    theme.caret = primary;
    theme.selection = primary.opacity(0.22);
    theme.link = primary;
    theme.link_hover = primary_hover;
    theme.link_active = primary_active;

    theme.primary = primary;
    theme.primary_hover = primary_hover;
    theme.primary_active = primary_active;
    theme.primary_foreground = white;
    theme.button_primary = primary;
    theme.button_primary_hover = primary_hover;
    theme.button_primary_active = primary_active;
    theme.button_primary_foreground = white;

    theme.accent = accent.opacity(0.18);
    theme.accent_foreground = foreground;
    theme.secondary = secondary;
    theme.secondary_hover = secondary_hover;
    theme.secondary_active = neutral;
    theme.secondary_foreground = foreground;
    theme.button_secondary = secondary;
    theme.button_secondary_hover = secondary_hover;
    theme.button_secondary_active = neutral;
    theme.button_secondary_foreground = foreground;

    theme.muted = muted;
    theme.muted_foreground = secondary;
    theme.popover = background;
    theme.popover_foreground = foreground;
    theme.button = background;
    theme.button_hover = muted;
    theme.button_active = raised;
    theme.button_foreground = foreground;

    theme.colors.list = background;
    theme.list_even = muted.opacity(0.45);
    theme.list_head = muted;
    theme.list_hover = muted;
    theme.list_active = primary;
    theme.list_active_border = primary_active;
    theme.table = background;
    theme.table_even = muted.opacity(0.45);
    theme.table_head = muted;
    theme.table_head_foreground = foreground;
    theme.table_hover = muted;
    theme.table_active = primary;
    theme.table_active_border = primary_active;
    theme.table_row_border = border;

    theme.sidebar = muted;
    theme.sidebar_foreground = foreground;
    theme.sidebar_border = border;
    theme.sidebar_accent = raised;
    theme.sidebar_accent_foreground = foreground;
    theme.sidebar_primary = primary;
    theme.sidebar_primary_foreground = white;
    theme.title_bar = neutral;
    theme.title_bar_border = border;
    theme.status_bar = muted;
    theme.status_bar_border = border;
    theme.window_border = border;

    theme.info = info;
    theme.info_foreground = white;
    theme.button_info = info;
    theme.button_info_foreground = white;
    theme.success = success;
    theme.success_foreground = white;
    theme.button_success = success;
    theme.button_success_foreground = white;
    theme.warning = warning;
    theme.warning_foreground = warning_foreground;
    theme.button_warning = warning;
    theme.button_warning_foreground = warning_foreground;
    theme.danger = danger;
    theme.danger_foreground = white;
    theme.button_danger = danger;
    theme.button_danger_foreground = white;

    theme.scrollbar = muted;
    theme.scrollbar_thumb = secondary.opacity(0.55);
    theme.scrollbar_thumb_hover = secondary;
    theme.slider_bar = raised;
    theme.slider_thumb = primary;
    theme.switch = raised;
    theme.switch_thumb = background;
    theme.progress_bar = primary;
    theme.skeleton = raised;
    theme.overlay = neutral.opacity(0.45);
    theme.transparent = color(0x000000).opacity(0.0);
}
