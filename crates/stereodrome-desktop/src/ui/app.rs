use std::{cell::RefCell, rc::Rc, sync::Arc};

use cargo_packager_updater::Update;
use gpui::{
    App, AppContext as _, Bounds, Context, Entity, IntoElement, Menu, MenuItem, QuitMode, Render,
    Role, Task, TitlebarOptions, Window, WindowBounds, WindowOptions, div, prelude::*, px, size,
};
use log::warn;
use souvlaki::{MediaControlEvent, SeekDirection};
use stereodrome_desktop::{DesktopBackend, DesktopPaths, operations::cover_art};

use super::{
    actions::{
        self, CheckForUpdates, InstallUpdate, OpenMiniPlayer, OpenNanoPlayer, OpenSettings, Quit,
        ShowMainWindow,
    },
    model::DesktopModel,
    native::{
        media::MediaService,
        notifications::NotificationService,
        single_instance::{AcquireResult, SingleInstanceService},
        tray::{TrayAction, TrayService},
        updater,
    },
    theme, windows,
};

struct NativeServices {
    single_instance: Option<SingleInstanceService>,
    media: Option<MediaService>,
    tray: Option<TrayService>,
    notifications: NotificationService,
    pending_update: Option<Update>,
    native_tasks: Vec<Task<()>>,
    relaunch_on_quit: bool,
}

impl NativeServices {
    fn shutdown(&mut self) {
        self.native_tasks.clear();
        if let Some(service) = &mut self.tray {
            service.shutdown();
        }
        self.tray.take();
        self.media.take();
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
        media: None,
        tray: None,
        notifications: NotificationService::default(),
        native_tasks: Vec::new(),
        pending_update: None,
        relaunch_on_quit: false,
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
            MenuItem::action("Settings", OpenSettings),
            MenuItem::action("Mini Player", OpenMiniPlayer),
            MenuItem::action("Nano Player", OpenNanoPlayer),
            MenuItem::separator(),
            MenuItem::action("Quit Stereodrome", Quit),
        ])]);
        let model = cx.new(|_| DesktopModel::new(Arc::clone(&backend)));
        *reopen_model.borrow_mut() = Some(model.clone());
        actions::install_model_handlers(&model, cx);
        windows::observe_main_window(&model, cx);
        install_shell_handlers(&model, Arc::clone(&backend), Rc::clone(&services), cx);
        install_updater_handlers(&model, Arc::clone(&backend), Rc::clone(&services), cx);
        forward_second_instance(show_receiver, &model, cx);
        model.update(cx, DesktopModel::start);

        if let Err(error) = windows::open_main_window(model.clone(), cx) {
            model.update(cx, |model, cx| model.set_action_error(error, cx));
        }
        install_media_service(&model, &backend, Rc::clone(&services), cx);
        install_tray_service(&model, Rc::clone(&services), cx);
        cx.activate(true);
    });
}

fn install_tray_service(
    model: &Entity<DesktopModel>,
    services: Rc<RefCell<NativeServices>>,
    cx: &mut App,
) {
    let (tray, receiver) = match TrayService::new() {
        Ok(service) => service,
        Err(error) => {
            model.update(cx, |model, cx| model.set_action_error(error, cx));
            return;
        }
    };
    let state = model.read(cx);
    tray.update(
        &state.playback,
        !state.queue.items.is_empty(),
        state.updater.status,
    );
    model.update(cx, |model, cx| {
        model.tray_available = true;
        cx.notify();
    });
    services.borrow_mut().tray = Some(tray);

    let event_model = model.clone();
    let task = cx.spawn(async move |cx| {
        while let Ok(action) = receiver.recv().await {
            cx.update(|cx| match action {
                TrayAction::Show => cx.dispatch_action(&ShowMainWindow),
                TrayAction::Settings => cx.dispatch_action(&OpenSettings),
                TrayAction::TogglePlayback => {
                    event_model.update(cx, DesktopModel::toggle_playback);
                }
                TrayAction::Next => {
                    event_model.update(cx, DesktopModel::play_next);
                }
                TrayAction::Previous => {
                    event_model.update(cx, DesktopModel::play_previous);
                }
                TrayAction::Quit => cx.dispatch_action(&Quit),
            });
        }
    });
    services.borrow_mut().native_tasks.push(task);
}

fn install_media_service(
    model: &Entity<DesktopModel>,
    backend: &Arc<DesktopBackend>,
    services: Rc<RefCell<NativeServices>>,
    cx: &mut App,
) {
    #[cfg(target_os = "windows")]
    let hwnd = windows::main_hwnd(model, cx);
    #[cfg(not(target_os = "windows"))]
    let hwnd = None;

    let (mut media, receiver) = match MediaService::new(hwnd) {
        Ok(service) => service,
        Err(error) => {
            model.update(cx, |model, cx| model.set_action_error(error, cx));
            return;
        }
    };
    let mut playback = backend.subscribe_playback();
    media.update(&playback.borrow().clone());
    services.borrow_mut().media = Some(media);

    let event_model = model.clone();
    let event_task = cx.spawn(async move |cx| {
        while let Ok(event) = receiver.recv().await {
            cx.update(|cx| handle_media_event(event, &event_model, cx));
        }
    });

    let notification_model = model.clone();
    let playback_services = Rc::clone(&services);
    let notification_backend = Arc::clone(backend);
    let playback_task = cx.spawn(async move |cx| {
        while playback.changed().await.is_ok() {
            let state = playback.borrow_and_update().clone();
            cx.update(|cx| {
                let (settings, main_window, auxiliary_windows, has_queue, updater_status) = {
                    let model = notification_model.read(cx);
                    (
                        model.notification_settings.clone(),
                        model.windows.main,
                        model.windows.auxiliary_count(),
                        !model.queue.items.is_empty(),
                        model.updater.status,
                    )
                };
                let main_focused = main_window
                    .and_then(|handle| {
                        handle
                            .update(cx, |_, window, _| window.is_window_active())
                            .ok()
                    })
                    .unwrap_or(false);
                let notification = {
                    let mut services = playback_services.borrow_mut();
                    if let Some(media) = &mut services.media {
                        media.update(&state);
                    }
                    if let Some(tray) = &services.tray {
                        tray.update(&state, has_queue, updater_status);
                    }
                    match state.song.as_ref() {
                        Some(song) => {
                            let is_new = services.notifications.begin_song(&song.id);
                            if is_new
                                && state.is_playing
                                && settings.enabled
                                && (settings.notify_when_focused || !main_focused)
                                && (settings.notify_when_miniplayer_open || auxiliary_windows == 0)
                            {
                                Some((
                                    song.id.clone(),
                                    song.title.clone(),
                                    song.artist.clone(),
                                    song.cover_art_id.clone(),
                                ))
                            } else {
                                None
                            }
                        }
                        None => {
                            services.notifications.clear_song();
                            None
                        }
                    }
                };
                if let Some((song_id, title, artist, cover_art_id)) = notification {
                    let state = notification_backend.state();
                    let task = notification_backend.runtime_handle().spawn(async move {
                        let cover_art_id = cover_art_id?;
                        cover_art::get_cover_art_path(&state, cover_art_id, Some(128))
                            .await
                            .ok()
                    });
                    let services = Rc::clone(&playback_services);
                    let model = notification_model.clone();
                    cx.spawn(async move |cx| {
                        let cover_art_path = task.await.ok().flatten();
                        let is_current = model.update(cx, |model, _| {
                            model.playback.song.as_ref().map(|song| &song.id) == Some(&song_id)
                        });
                        if !is_current {
                            return;
                        }
                        let error = services.borrow().notifications.send_now_playing(
                            &title,
                            Some(&artist),
                            cover_art_path.as_deref().map(std::path::Path::new),
                        );
                        if let Err(error) = error {
                            model.update(cx, |model, cx| model.set_action_error(error, cx));
                        }
                    })
                    .detach();
                }
            });
        }
    });
    services
        .borrow_mut()
        .native_tasks
        .extend([event_task, playback_task]);
}

fn install_updater_handlers(
    model: &Entity<DesktopModel>,
    backend: Arc<DesktopBackend>,
    services: Rc<RefCell<NativeServices>>,
    cx: &mut App,
) {
    let check_model = model.clone();
    let check_backend = Arc::clone(&backend);
    let check_services = Rc::clone(&services);
    cx.on_action(move |_: &CheckForUpdates, cx| {
        if check_model.read(cx).updater.busy {
            return;
        }
        if check_model.read(cx).connectivity.manual_offline_enabled {
            check_model.update(cx, |model, cx| {
                model.set_action_error("Update checks are unavailable in manual offline mode", cx);
            });
            return;
        }
        check_model.update(cx, |model, cx| {
            model.updater.status = "checking";
            model.updater.busy = true;
            model.updater.version = None;
            model.updater.notes = None;
            cx.notify();
        });
        refresh_tray(&check_model, &check_services, cx);

        let check = check_backend
            .runtime_handle()
            .spawn_blocking(updater::check);
        let result_model = check_model.clone();
        let result_services = Rc::clone(&check_services);
        cx.spawn(async move |cx| {
            let result = check
                .await
                .map_err(|error| format!("Update task failed: {error}"))
                .and_then(|result| result);
            cx.update(|cx| match result {
                Ok(Some(update)) => {
                    let version = update.version.clone();
                    let notes = update.body.clone();
                    let notification_error = {
                        let mut services = result_services.borrow_mut();
                        services.pending_update = Some(update);
                        services.notifications.send_update_available(&version).err()
                    };
                    result_model.update(cx, |model, cx| {
                        model.updater.status = "update available";
                        model.updater.version = Some(version);
                        model.updater.notes = notes;
                        model.updater.busy = false;
                        if let Some(error) = notification_error {
                            model.set_action_error(error, cx);
                        } else {
                            cx.notify();
                        }
                    });
                    refresh_tray(&result_model, &result_services, cx);
                }
                Ok(None) => {
                    result_services.borrow_mut().pending_update = None;
                    result_model.update(cx, |model, cx| {
                        model.updater.status = "up to date";
                        model.updater.busy = false;
                        cx.notify();
                    });
                    refresh_tray(&result_model, &result_services, cx);
                }
                Err(error) => {
                    result_model.update(cx, |model, cx| {
                        model.updater.status = "error";
                        model.updater.busy = false;
                        model.set_action_error(error, cx);
                    });
                    refresh_tray(&result_model, &result_services, cx);
                }
            });
        })
        .detach();
    });

    let install_model = model.clone();
    let install_services = Rc::clone(&services);
    cx.on_action(move |_: &InstallUpdate, cx| {
        if install_model.read(cx).updater.busy {
            return;
        }
        let Some(update) = install_services.borrow_mut().pending_update.take() else {
            install_model.update(cx, |model, cx| {
                model.set_action_error("No verified update is ready to install", cx);
            });
            return;
        };
        let retry_update = update.clone();
        install_model.update(cx, |model, cx| {
            model.updater.status = "downloading and installing";
            model.updater.busy = true;
            cx.notify();
        });
        refresh_tray(&install_model, &install_services, cx);

        let install = backend
            .runtime_handle()
            .spawn_blocking(move || updater::install(update));
        let result_model = install_model.clone();
        let result_services = Rc::clone(&install_services);
        cx.spawn(async move |cx| {
            let result = install
                .await
                .map_err(|error| format!("Update task failed: {error}"))
                .and_then(|result| result);
            cx.update(|cx| match result {
                Ok(()) => {
                    result_model.update(cx, |model, cx| {
                        model.updater.status = "installed";
                        model.updater.busy = false;
                        cx.notify();
                    });
                    result_services.borrow_mut().relaunch_on_quit = true;
                    cx.dispatch_action(&Quit);
                }
                Err(error) => {
                    result_services.borrow_mut().pending_update = Some(retry_update);
                    result_model.update(cx, |model, cx| {
                        model.updater.status = "error";
                        model.updater.busy = false;
                        model.set_action_error(error, cx);
                    });
                    refresh_tray(&result_model, &result_services, cx);
                }
            });
        })
        .detach();
    });
}

fn refresh_tray(
    model: &Entity<DesktopModel>,
    services: &Rc<RefCell<NativeServices>>,
    cx: &mut App,
) {
    let model = model.read(cx);
    if let Some(tray) = &services.borrow().tray {
        tray.update(
            &model.playback,
            !model.queue.items.is_empty(),
            model.updater.status,
        );
    }
}

fn handle_media_event(event: MediaControlEvent, model: &Entity<DesktopModel>, cx: &mut App) {
    match event {
        MediaControlEvent::Play => model.update(cx, |model, cx| {
            if !model.playback.is_playing {
                model.toggle_playback(cx);
            }
        }),
        MediaControlEvent::Pause | MediaControlEvent::Stop => model.update(cx, |model, cx| {
            if model.playback.is_playing {
                model.toggle_playback(cx);
            }
        }),
        MediaControlEvent::Toggle => {
            model.update(cx, DesktopModel::toggle_playback);
        }
        MediaControlEvent::Next => {
            model.update(cx, DesktopModel::play_next);
        }
        MediaControlEvent::Previous => {
            model.update(cx, DesktopModel::play_previous);
        }
        MediaControlEvent::Seek(direction) => {
            let delta = if direction == SeekDirection::Forward {
                10.0
            } else {
                -10.0
            };
            model.update(cx, |model, cx| model.seek_by(delta, cx));
        }
        MediaControlEvent::SeekBy(direction, duration) => {
            let delta = duration.as_secs_f64()
                * if direction == SeekDirection::Forward {
                    1.0
                } else {
                    -1.0
                };
            model.update(cx, |model, cx| model.seek_by(delta, cx));
        }
        MediaControlEvent::SetPosition(position) => {
            model.update(cx, |model, cx| model.seek_to(position.0.as_secs_f64(), cx));
        }
        MediaControlEvent::SetVolume(volume) => {
            model.update(cx, |model, cx| model.set_volume(volume as f32, cx));
        }
        MediaControlEvent::Raise => {
            if let Err(error) = windows::open_main_window(model.clone(), cx) {
                model.update(cx, |model, cx| model.set_action_error(error, cx));
            }
        }
        MediaControlEvent::Quit => cx.dispatch_action(&Quit),
        MediaControlEvent::OpenUri(_) => {}
    }
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

    let mini_model = model.downgrade();
    cx.on_action(move |_: &OpenMiniPlayer, cx| {
        if let Some(model) = mini_model.upgrade()
            && let Err(error) = windows::open_mini_player(model.clone(), cx)
        {
            model.update(cx, |model, cx| model.set_action_error(error, cx));
        }
    });

    let nano_model = model.downgrade();
    cx.on_action(move |_: &OpenNanoPlayer, cx| {
        if let Some(model) = nano_model.upgrade()
            && let Err(error) = windows::open_nano_player(model.clone(), cx)
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
        let relaunch = {
            let mut services = quit_services.borrow_mut();
            services.shutdown();
            std::mem::take(&mut services.relaunch_on_quit)
        };
        if let Err(error) = quit_backend.shutdown() {
            warn!("Desktop backend shutdown failed: {error}");
        }
        if relaunch && let Err(error) = updater::relaunch() {
            warn!("Update installed but relaunch failed: {error}");
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
