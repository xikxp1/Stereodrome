use gpui::{
    AppContext as _, Entity, Focusable as _, FontWeight, InteractiveElement as _, IntoElement,
    Role, div, prelude::*, px,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _,
    button::{Button, ButtonVariants as _},
    input::{Input, InputContentType, InputState},
};
use stereodrome_desktop::operations::auth::ConnectParams;

use crate::ui::model::{DesktopModel, VisibleSurface};

pub struct AuthInputs {
    pub url: Entity<InputState>,
    pub username: Entity<InputState>,
    pub password: Entity<InputState>,
}

impl AuthInputs {
    pub fn new(window: &mut gpui::Window, cx: &mut gpui::App) -> Self {
        Self {
            url: cx.new(|cx| InputState::new(window, cx).placeholder("https://your-server.com")),
            username: cx.new(|cx| InputState::new(window, cx).placeholder("Username")),
            password: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("Password")
                    .masked(true)
            }),
        }
    }
}

pub fn render(
    inputs: &AuthInputs,
    model: &Entity<DesktopModel>,
    cx: &mut gpui::App,
) -> impl IntoElement {
    let state = model.read(cx);
    let surface = state.visible_surface();
    let busy = state.auth.initializing || state.auth.connecting;
    let error = state
        .auth
        .error
        .as_ref()
        .or(state.action_error.as_ref())
        .cloned();
    let configured_server = state.auth.status.server_url.clone();
    let configured_user = state.auth.status.username.clone();
    let connected = state.auth.status.connected;
    let offline = state.offline();
    let manual_offline = state.connectivity.manual_offline_enabled;
    let quitting = state.quitting;
    let updater_status = state.updater.status;
    let auxiliary_windows = state.windows.auxiliary_count();

    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(cx.theme().background)
        .text_color(cx.theme().foreground)
        .child(match surface {
            VisibleSurface::Restoring => div()
                .id("restore-status")
                .role(Role::Status)
                .aria_label("Restoring saved session")
                .text_lg()
                .child("Restoring saved session…")
                .into_any_element(),
            VisibleSurface::Login => login_form(inputs, model, busy || quitting, error, cx),
            VisibleSurface::Library => configured_session(
                model,
                configured_server,
                configured_user,
                connected,
                offline,
                manual_offline,
                busy || quitting,
                error,
                updater_status,
                auxiliary_windows,
                cx,
            ),
        })
}

fn login_form(
    inputs: &AuthInputs,
    model: &Entity<DesktopModel>,
    disabled: bool,
    error: Option<String>,
    cx: &mut gpui::App,
) -> gpui::AnyElement {
    let url_focus = inputs.url.focus_handle(cx);
    let username_focus = inputs.username.focus_handle(cx);
    let password_focus = inputs.password.focus_handle(cx);
    let model_for_connect = model.clone();
    let url = inputs.url.clone();
    let username = inputs.username.clone();
    let password = inputs.password.clone();

    div()
        .w_full()
        .max_w(px(448.0))
        .flex()
        .flex_col()
        .gap_3()
        .p_6()
        .rounded_lg()
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().background)
        .child(
            div()
                .text_2xl()
                .font_weight(FontWeight::SEMIBOLD)
                .child("Connect to Stereodrome"),
        )
        .child(
            div()
                .text_color(cx.theme().muted_foreground)
                .child("Use your existing Subsonic-compatible server account."),
        )
        .child(labeled_input(
            "Server URL",
            "server-url",
            &inputs.url,
            &url_focus,
            InputContentType::Url,
        ))
        .child(labeled_input(
            "Username",
            "username",
            &inputs.username,
            &username_focus,
            InputContentType::Username,
        ))
        .child(labeled_input(
            "Password",
            "password",
            &inputs.password,
            &password_focus,
            InputContentType::Password,
        ))
        .when_some(error, |this, error| {
            this.child(
                div()
                    .id("login-error")
                    .role(Role::Alert)
                    .text_color(cx.theme().danger)
                    .child(error),
            )
        })
        .child(
            Button::new("connect")
                .primary()
                .label(if disabled { "Connecting…" } else { "Connect" })
                .disabled(disabled)
                .on_click(move |_, _, cx| {
                    let params = ConnectParams {
                        url: url.read(cx).value().to_string(),
                        username: username.read(cx).value().to_string(),
                        password: password.read(cx).value().to_string(),
                    };
                    model_for_connect.update(cx, |model, cx| model.connect(params, cx));
                }),
        )
        .into_any_element()
}

fn labeled_input(
    label: &'static str,
    id: &'static str,
    input: &Entity<InputState>,
    focus: &gpui::FocusHandle,
    content_type: InputContentType,
) -> gpui::AnyElement {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(label))
        .child(
            div()
                .id(id)
                .role(if content_type == InputContentType::Password {
                    Role::PasswordInput
                } else {
                    Role::TextInput
                })
                .aria_label(label)
                .track_focus(focus)
                .child(Input::new(input).content_type(content_type)),
        )
        .into_any_element()
}

#[allow(clippy::too_many_arguments)]
fn configured_session(
    model: &Entity<DesktopModel>,
    server: Option<String>,
    username: Option<String>,
    connected: bool,
    offline: bool,
    manual_offline: bool,
    disabled: bool,
    error: Option<String>,
    updater_status: &'static str,
    auxiliary_windows: usize,
    cx: &mut gpui::App,
) -> gpui::AnyElement {
    let offline_model = model.clone();
    let disconnect_model = model.clone();
    div()
        .w_full()
        .max_w(px(512.0))
        .flex()
        .flex_col()
        .gap_3()
        .p_6()
        .rounded_lg()
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().background)
        .child(
            div()
                .text_2xl()
                .font_weight(FontWeight::SEMIBOLD)
                .child("Library"),
        )
        .child(
            div()
                .id("connection-status")
                .role(Role::Status)
                .aria_label(if connected {
                    "Connected"
                } else {
                    "Offline library"
                })
                .text_color(if connected {
                    cx.theme().success
                } else {
                    cx.theme().warning
                })
                .child(if connected {
                    "Connected"
                } else if manual_offline {
                    "Manual offline mode — local library remains available"
                } else {
                    "Server unavailable — local library remains available"
                }),
        )
        .child(format!(
            "{}{}",
            server.unwrap_or_default(),
            username.map_or_else(String::new, |name| format!(" · {name}"))
        ))
        .child(format!(
            "Updater: {updater_status} · Auxiliary windows: {auxiliary_windows}"
        ))
        .child(if offline {
            "Network-only actions are disabled; cached library and playback remain available."
        } else {
            "Native library views arrive in Phase 4."
        })
        .when_some(error, |this, error| {
            this.child(
                div()
                    .id("session-error")
                    .role(Role::Alert)
                    .text_color(cx.theme().danger)
                    .child(error),
            )
        })
        .child(
            div()
                .flex()
                .gap_2()
                .child(
                    Button::new("toggle-offline")
                        .label(if manual_offline {
                            "Go Online"
                        } else {
                            "Work Offline"
                        })
                        .disabled(disabled)
                        .on_click(move |_, _, cx| {
                            offline_model.update(cx, |model, cx| {
                                model.set_manual_offline(!manual_offline, cx)
                            });
                        }),
                )
                .child(
                    Button::new("disconnect")
                        .danger()
                        .label("Disconnect")
                        .disabled(disabled)
                        .on_click(move |_, _, cx| {
                            disconnect_model.update(cx, DesktopModel::disconnect);
                        }),
                ),
        )
        .into_any_element()
}
