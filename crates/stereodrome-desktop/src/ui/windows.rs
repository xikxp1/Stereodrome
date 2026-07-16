use gpui::{
    AnyWindowHandle, App, AppContext as _, Bounds, Context, Entity, FocusHandle, Render, Role,
    TitlebarOptions, Window, WindowBounds, WindowOptions, div, prelude::*, px, size,
};
use gpui_component::Root;

use super::{actions::ClearFocus, model::DesktopModel, views::auth};

pub struct MainView {
    model: Entity<DesktopModel>,
    inputs: auth::AuthInputs,
    focus: FocusHandle,
}

impl MainView {
    fn new(model: Entity<DesktopModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        Self {
            model,
            inputs: auth::AuthInputs::new(window, cx),
            focus,
        }
    }
}

impl Render for MainView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("stereodrome-root")
            .key_context("Stereodrome")
            .role(Role::Application)
            .aria_label("Stereodrome")
            .track_focus(&self.focus)
            .on_action(cx.listener(|_, _: &ClearFocus, window, _| window.blur()))
            .size_full()
            .child(auth::render(&self.inputs, &self.model, cx))
    }
}

pub fn open_main_window(model: Entity<DesktopModel>, cx: &mut App) -> Result<(), String> {
    if model.read(cx).quitting {
        return Ok(());
    }
    cx.activate(true);

    if let Some(handle) = model.read(cx).windows.main {
        if handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
        {
            cx.activate(true);
            return Ok(());
        }
        model.update(cx, |model, cx| {
            model.windows.main = None;
            cx.notify();
        });
    }

    let bounds = Bounds::centered(None, size(px(800.0), px(600.0)), cx);
    let view_model = model.clone();
    let handle = cx
        .open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(800.0), px(600.0))),
                titlebar: Some(TitlebarOptions {
                    title: Some("Stereodrome".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            move |window, cx| {
                let view = cx.new(|cx| MainView::new(view_model, window, cx));
                cx.new(|cx| Root::new(view, window, cx))
            },
        )
        .map_err(|error| format!("Failed to open main window: {error:#}"))?;
    let handle = AnyWindowHandle::from(handle);
    handle
        .update(cx, |_, window, _| window.activate_window())
        .map_err(|error| format!("Failed to activate main window: {error}"))?;
    cx.activate(true);
    model.update(cx, |model, cx| {
        model.windows.main = Some(handle);
        cx.notify();
    });
    Ok(())
}

pub fn observe_main_window(model: &Entity<DesktopModel>, cx: &mut App) {
    let weak = model.downgrade();
    cx.on_window_closed(move |cx, closed_id| {
        weak.update(cx, |model, cx| {
            if model
                .windows
                .main
                .is_some_and(|handle| handle.window_id() == closed_id)
            {
                model.windows.main = None;
                cx.notify();
            }
        })
        .ok();
    })
    .detach();
}
