use std::{cell::RefCell, rc::Rc, sync::Arc};

use gpui::{
    App, AppContext as _, Bounds, Context, Entity, IntoElement, Menu, MenuItem, QuitMode, Render,
    Role, TitlebarOptions, Window, WindowBounds, WindowOptions, div, prelude::*, px, size,
};
use log::warn;
use stereodrome_desktop::{DesktopBackend, DesktopPaths};

use super::{
    actions::{self, Quit, ShowMainWindow},
    model::DesktopModel,
    native::single_instance::{AcquireResult, SingleInstanceService},
    theme, windows,
};

struct NativeServices {
    single_instance: Option<SingleInstanceService>,
}

impl NativeServices {
    fn shutdown(&mut self) {
        if let Some(service) = &mut self.single_instance {
            service.shutdown();
        }
        self.single_instance.take();
    }
}

pub fn run() {
    let (single_instance, show_receiver) = match SingleInstanceService::acquire() {
        Ok(AcquireResult::Primary(service, receiver)) => (service, receiver),
        Ok(AcquireResult::Secondary) => return,
        Err(error) => {
            run_startup_error(error);
            return;
        }
    };

    let paths = match DesktopPaths::detect() {
        Ok(paths) => paths,
        Err(error) => {
            run_startup_error(error.to_string());
            return;
        }
    };
    let backend = match DesktopBackend::open(paths) {
        Ok(backend) => Arc::new(backend),
        Err(error) => {
            run_startup_error(error.to_string());
            return;
        }
    };

    let services = Rc::new(RefCell::new(NativeServices {
        single_instance: Some(single_instance),
    }));
    let reopen_model = Rc::new(RefCell::new(None::<Entity<DesktopModel>>));
    let reopen_target = Rc::clone(&reopen_model);
    let application = gpui_platform::application()
        .with_assets(gpui_component_assets::Assets)
        .with_quit_mode(QuitMode::Explicit);
    application.on_reopen(move |cx| {
        if let Some(model) = reopen_target.borrow().as_ref().cloned()
            && let Err(error) = windows::open_main_window(model.clone(), cx)
        {
            model.update(cx, |model, cx| model.set_action_error(error, cx));
        }
    });

    application.run(move |cx| {
        gpui_component::init(cx);
        theme::apply(cx);
        actions::bind_keys(cx);
        cx.set_menus([Menu::new("Stereodrome").items([
            MenuItem::action("Show Stereodrome", ShowMainWindow),
            MenuItem::separator(),
            MenuItem::action("Quit Stereodrome", Quit),
        ])]);

        let model = cx.new(|_| DesktopModel::new(Arc::clone(&backend)));
        *reopen_model.borrow_mut() = Some(model.clone());
        actions::install_model_handlers(&model, cx);
        windows::observe_main_window(&model, cx);
        install_shell_handlers(&model, Arc::clone(&backend), Rc::clone(&services), cx);
        forward_second_instance(show_receiver, &model, cx);
        model.update(cx, DesktopModel::start);

        if let Err(error) = windows::open_main_window(model.clone(), cx) {
            model.update(cx, |model, cx| model.set_action_error(error, cx));
        }
        cx.activate(true);
    });
}

fn install_shell_handlers(
    model: &Entity<DesktopModel>,
    backend: Arc<DesktopBackend>,
    services: Rc<RefCell<NativeServices>>,
    cx: &mut App,
) {
    let show_model = model.downgrade();
    cx.on_action(move |_: &ShowMainWindow, cx| {
        if let Some(model) = show_model.upgrade()
            && let Err(error) = windows::open_main_window(model.clone(), cx)
        {
            model.update(cx, |model, cx| model.set_action_error(error, cx));
        }
    });

    let quit_model = model.downgrade();
    let quit_backend = Arc::clone(&backend);
    let quit_services = Rc::clone(&services);
    cx.on_action(move |_: &Quit, cx| {
        let Some(model) = quit_model.upgrade() else {
            return;
        };
        let should_quit = model.update(cx, DesktopModel::begin_quit);
        if !should_quit {
            return;
        }
        quit_services.borrow_mut().shutdown();
        if let Err(error) = quit_backend.shutdown() {
            warn!("Desktop backend shutdown failed: {error}");
        }
        cx.quit();
    });

    let quit_model = model.downgrade();
    cx.on_app_quit(move |cx| {
        if let Some(model) = quit_model.upgrade() {
            model.update(cx, |model, cx| {
                model.begin_quit(cx);
            });
        }
        services.borrow_mut().shutdown();
        if let Err(error) = backend.shutdown() {
            warn!("Desktop backend shutdown failed: {error}");
        }
        async {}
    })
    .detach();
}

fn forward_second_instance(
    receiver: async_channel::Receiver<()>,
    model: &Entity<DesktopModel>,
    cx: &mut App,
) {
    let model = model.downgrade();
    cx.spawn(async move |cx| {
        while receiver.recv().await.is_ok() {
            cx.update(|cx| {
                if let Some(model) = model.upgrade()
                    && let Err(error) = windows::open_main_window(model.clone(), cx)
                {
                    model.update(cx, |model, cx| model.set_action_error(error, cx));
                }
            });
        }
    })
    .detach();
}

struct StartupError(String);

impl Render for StartupError {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("startup-error")
            .role(Role::Alert)
            .aria_label("Stereodrome startup error")
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .p_6()
            .child(format!("Stereodrome could not start:\n{}", self.0))
    }
}

fn run_startup_error(error: String) {
    gpui_platform::application()
        .with_assets(gpui_component_assets::Assets)
        .run(move |cx| {
            gpui_component::init(cx);
            theme::apply(cx);
            let bounds = Bounds::centered(None, size(px(560.0), px(240.0)), cx);
            if let Err(open_error) = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        title: Some("Stereodrome startup error".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                |_, cx| cx.new(|_| StartupError(error)),
            ) {
                warn!("Failed to present startup error window: {open_error:#}");
            }
            cx.activate(true);
        });
}
