use std::{path::PathBuf, sync::Arc, time::Duration};

use gpui::{
    AnyWindowHandle, App, AppContext as _, Bounds, Context, Entity, FocusHandle, Focusable as _,
    KeyBinding, Menu, MenuItem, PathPromptOptions, Role, SharedString, TitlebarOptions, Window,
    WindowBounds, WindowKind, WindowOptions, actions, div, img, prelude::*, px, rgb, size,
    uniform_list,
};
use gpui_component::{
    Disableable as _, Icon, IconName, Root, Selectable as _,
    button::Button,
    input::{Input, InputState},
    menu::ContextMenuExt as _,
};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use raw_window_handle::RawWindowHandle;
use tray_icon::{
    Icon as TrayIcon, TrayIcon as TrayHandle, TrayIconBuilder,
    menu::{Menu as TrayMenu, MenuEvent, MenuItem as TrayMenuItem},
};

const SONG_COUNT: usize = 10_000;
const ALBUM_COUNT: usize = 2_000;
const ALBUMS_PER_ROW: usize = 4;

actions!(
    feasibility,
    [
        ChooseFolder,
        CloseSecondary,
        ContextProbe,
        MenuProbe,
        OpenCover,
        OpenMini,
        ProbeNative,
        RunTokio,
        Tab,
        TabPrev,
        TrayProbe,
    ]
);

pub fn run() {
    gpui_platform::application()
        .with_assets(gpui_component_assets::Assets)
        .run(|cx| {
            gpui_component::init(cx);
            cx.bind_keys([
                KeyBinding::new("tab", Tab, None),
                KeyBinding::new("shift-tab", TabPrev, None),
            ]);

            let runtime = tokio::runtime::Runtime::new().expect("failed to create Tokio runtime");
            let model = cx.new(|_| FeasibilityModel::new(runtime));
            install_actions(&model, cx);
            install_menus(cx);
            install_tray(&model, cx);

            let bounds = Bounds::centered(None, size(px(800.), px(600.)), cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    window_min_size: Some(size(px(800.), px(600.))),
                    titlebar: Some(TitlebarOptions {
                        title: Some("Stereodrome GPUI feasibility".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                |window, cx| {
                    let view = cx.new(|cx| Feasibility::new(model.clone(), window, cx));
                    cx.new(|cx| Root::new(view, window, cx))
                },
            )
            .expect("failed to open feasibility window");
            cx.activate(true);
        });
}

struct FeasibilityModel {
    status: SharedString,
    selected_path: Option<PathBuf>,
    selected_song: usize,
    selected_album: usize,
    shared_count: usize,
    mini_window: Option<AnyWindowHandle>,
    cover_window: Option<AnyWindowHandle>,
    runtime: tokio::runtime::Runtime,
    #[cfg(not(target_os = "linux"))]
    _tray: Option<TrayHandle>,
    #[cfg(target_os = "windows")]
    _media_controls: Option<souvlaki::MediaControls>,
}

impl FeasibilityModel {
    fn new(runtime: tokio::runtime::Runtime) -> Self {
        Self {
            status: "Ready: complete every probe and record the result".into(),
            selected_path: None,
            selected_song: 0,
            selected_album: 0,
            shared_count: 0,
            mini_window: None,
            cover_window: None,
            runtime,
            #[cfg(not(target_os = "linux"))]
            _tray: None,
            #[cfg(target_os = "windows")]
            _media_controls: None,
        }
    }

    fn set_status(&mut self, status: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.status = status.into();
        cx.notify();
    }
}

struct Feasibility {
    model: Entity<FeasibilityModel>,
    input: Entity<InputState>,
    focus: FocusHandle,
    cover_path: Arc<std::path::Path>,
}

impl Feasibility {
    fn new(model: Entity<FeasibilityModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Latin / IME / selection / copy and paste")
                .default_value("Hello, 世界")
        });
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        Self {
            model,
            input,
            focus,
            cover_path: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../src-tauri/icons/icon.png")
                .into(),
        }
    }

    fn choose_folder(&self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Select one directory".into()),
        });
        let model = self.model.clone();
        cx.spawn(async move |_, cx| match receiver.await {
            Ok(Ok(Some(paths))) => {
                let selected = paths.into_iter().next();
                model.update(cx, |model, cx| {
                    model.selected_path = selected.clone();
                    model.set_status(
                        format!("Folder selected: {}", selected.unwrap().display()),
                        cx,
                    );
                });
            }
            Ok(Ok(None)) => {}
            Ok(Err(error)) => {
                model.update(cx, |model, cx| {
                    model.set_status(format!("Folder picker error: {error:#}"), cx);
                });
            }
            Err(error) => {
                model.update(cx, |model, cx| {
                    model.set_status(format!("Folder picker channel error: {error}"), cx);
                });
            }
        })
        .detach();
    }

    fn run_tokio_probe(&self, cx: &mut Context<Self>) {
        let task = self.model.read(cx).runtime.spawn(async {
            tokio::time::sleep(Duration::from_millis(100)).await;
            42
        });
        let model = self.model.clone();
        cx.spawn(async move |_, cx| {
            let status = match task.await {
                Ok(42) => "Tokio returned 42 on the GPUI foreground executor".to_string(),
                Ok(value) => format!("Unexpected Tokio result: {value}"),
                Err(error) => format!("Tokio task failed: {error}"),
            };
            model.update(cx, |model, cx| model.set_status(status, cx));
        })
        .detach();
    }

    fn open_mini(&self, cx: &mut Context<Self>) {
        if self.model.read(cx).mini_window.is_some() {
            return;
        }
        let model = self.model.clone();
        let bounds = Bounds::centered(None, size(px(320.), px(72.)), cx);
        match cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                kind: WindowKind::Floating,
                is_resizable: false,
                titlebar: None,
                ..Default::default()
            },
            |window, cx| {
                let view = cx.new(|_| SharedWindow::new("Floating mini", model.clone()));
                cx.new(|cx| Root::new(view, window, cx))
            },
        ) {
            Ok(handle) => {
                let any = AnyWindowHandle::from(handle);
                let panel_result = any.update(cx, |_, window, _| apply_panel_flags(window));
                self.model.update(cx, |model, cx| {
                    model.mini_window = Some(any);
                    let status = match panel_result {
                        Ok(Ok(message)) => message,
                        Ok(Err(error)) => format!("Mini opened; panel probe failed: {error}"),
                        Err(error) => format!("Mini opened; native handle unavailable: {error}"),
                    };
                    model.set_status(status, cx);
                });
            }
            Err(error) => self.model.update(cx, |model, cx| {
                model.set_status(format!("Mini window error: {error:#}"), cx);
            }),
        }
    }

    fn open_cover(&self, cx: &mut Context<Self>) {
        if self.model.read(cx).cover_window.is_some() {
            return;
        }
        let model = self.model.clone();
        let bounds = Bounds::centered(None, size(px(420.), px(420.)), cx);
        match cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Shared cover-art window".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |window, cx| {
                let view = cx.new(|_| SharedWindow::new("Cover art", model.clone()));
                cx.new(|cx| Root::new(view, window, cx))
            },
        ) {
            Ok(handle) => self.model.update(cx, |model, cx| {
                model.cover_window = Some(handle.into());
                model.set_status("Cover-art window opened with the shared entity", cx);
            }),
            Err(error) => self.model.update(cx, |model, cx| {
                model.set_status(format!("Cover window error: {error:#}"), cx);
            }),
        }
    }

    fn close_secondary(&self, cx: &mut Context<Self>) {
        let handles = self.model.update(cx, |model, cx| {
            let handles = [model.mini_window.take(), model.cover_window.take()];
            model.set_status("Secondary windows closed; shared state retained", cx);
            handles
        });
        for handle in handles.into_iter().flatten() {
            handle
                .update(cx, |_, window, _| window.remove_window())
                .ok();
        }
    }

    fn probe_native(&self, window: &mut Window, cx: &mut Context<Self>) {
        let result = probe_native_handle(window, &self.model, cx);
        if let Err(error) = result {
            self.model.update(cx, |model, cx| {
                model.set_status(format!("Native handle probe failed: {error}"), cx);
            });
        }
    }
}

impl Render for Feasibility {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (status, selected_path) = {
            let model = self.model.read(cx);
            (
                model.status.clone(),
                model.selected_path.as_ref().map_or_else(
                    || "No folder selected".to_string(),
                    |path| path.display().to_string(),
                ),
            )
        };

        let input_focus = self.input.focus_handle(cx);
        div()
            .id("feasibility-root")
            .role(Role::Application)
            .aria_label("Stereodrome GPUI feasibility")
            .track_focus(&self.focus)
            .on_action(cx.listener(|_, _: &Tab, window, cx| window.focus_next(cx)))
            .on_action(cx.listener(|_, _: &TabPrev, window, cx| window.focus_prev(cx)))
            .on_action(cx.listener(|this, _: &ChooseFolder, _, cx| this.choose_folder(cx)))
            .on_action(cx.listener(|this, _: &RunTokio, _, cx| this.run_tokio_probe(cx)))
            .on_action(cx.listener(|this, _: &OpenMini, _, cx| this.open_mini(cx)))
            .on_action(cx.listener(|this, _: &OpenCover, _, cx| this.open_cover(cx)))
            .on_action(cx.listener(|this, _: &CloseSecondary, _, cx| this.close_secondary(cx)))
            .on_action(cx.listener(|this, _: &ProbeNative, window, cx| {
                this.probe_native(window, cx)
            }))
            .on_action(cx.listener(|this, _: &ContextProbe, _, cx| {
                this.model.update(cx, |model, cx| {
                    model.shared_count += 1;
                    model.set_status("Typed gpui-component context-menu action dispatched", cx);
                });
            }))
            .size_full()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .bg(rgb(0xf6f7fb))
            .text_color(rgb(0x20232a))
            .child(
                div()
                    .text_xl()
                    .font_weight(gpui::FontWeight::BOLD)
                    .child("Phase 0 native feasibility"),
            )
            .child(
                div()
                    .id("ime-input")
                    .role(Role::TextInput)
                    .aria_label("IME and clipboard test field")
                    .track_focus(&input_focus)
                    .child(
                        Input::new(&self.input)
                            .cleanable(true),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .flex_wrap()
                    .child(
                        Button::new("selected-state")
                            .label("Selected")
                            .selected(true),
                    )
                    .child(
                        Button::new("disabled-state")
                            .label("Disabled")
                            .disabled(true),
                    )
                    .child(
                        Button::new("folder")
                            .label("Choose folder")
                            .on_click(cx.listener(|this, _, _, cx| this.choose_folder(cx))),
                    )
                    .child(
                        Button::new("tokio")
                            .label("Run Tokio probe")
                            .on_click(cx.listener(|this, _, _, cx| this.run_tokio_probe(cx))),
                    )
                    .child(
                        Button::new("mini")
                            .label("Open mini")
                            .on_click(cx.listener(|this, _, _, cx| this.open_mini(cx))),
                    )
                    .child(
                        Button::new("cover")
                            .label("Open cover")
                            .on_click(cx.listener(|this, _, _, cx| this.open_cover(cx))),
                    )
                    .child(
                        Button::new("close-secondary")
                            .label("Close secondary")
                            .on_click(cx.listener(|this, _, _, cx| this.close_secondary(cx))),
                    )
                    .child(
                        Button::new("native")
                            .label("Probe native handle")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.probe_native(window, cx)
                            })),
                    )
                    .child(
                        div()
                            .id("context-menu-target")
                            .focusable()
                            .tab_stop(true)
                            .role(Role::Button)
                            .aria_label("Context menu probe")
                            .px_2()
                            .py_1()
                            .border_1()
                            .border_color(rgb(0x687087))
                            .rounded_md()
                            .child("Right-click menu")
                            .context_menu(|menu, _, _| {
                                menu.menu("Dispatch typed action", Box::new(ContextProbe))
                            }),
                    )
                    .child(Icon::new(IconName::Search).size(px(20.))),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(img(self.cover_path.clone()).size(px(48.)))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(status)
                            .child(format!("Folder: {selected_path}")),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap_3()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .id("song-list")
                            .role(Role::List)
                            .aria_label("Virtualized songs")
                            .flex_1()
                            .h_full()
                            .border_1()
                            .border_color(rgb(0xd5d8e0))
                            .child(
                                uniform_list(
                                    "songs",
                                    SONG_COUNT,
                                    cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                                        let selected = this.model.read(cx).selected_song;
                                        range
                                            .map(|index| {
                                                let model = this.model.clone();
                                                div()
                                                    .id(("song", index))
                                                    .role(Role::ListItem)
                                                    .aria_label(format!("Song {}", index + 1))
                                                    .aria_selected(index == selected)
                                                    .focusable()
                                                    .tab_stop(true)
                                                    .h(px(28.))
                                                    .px_2()
                                                    .border_b_1()
                                                    .border_color(rgb(0xe7e9ee))
                                                    .when(index == selected, |row| {
                                                        row.bg(rgb(0xdbe8ff))
                                                    })
                                                    .focus(|row| {
                                                        row.border_2().border_color(rgb(0x2457c5))
                                                    })
                                                    .on_click(move |_, _, cx| {
                                                        model.update(cx, |model, cx| {
                                                            model.selected_song = index;
                                                            model.set_status(
                                                                format!("Selected song {}", index + 1),
                                                                cx,
                                                            );
                                                        });
                                                    })
                                                    .child(format!(
                                                        "{:05}  Feasibility song",
                                                        index + 1
                                                    ))
                                            })
                                            .collect()
                                    }),
                                )
                                .h_full(),
                            ),
                    )
                    .child(
                        div()
                            .id("album-list")
                            .role(Role::List)
                            .aria_label("Virtualized album card rows")
                            .flex_1()
                            .h_full()
                            .border_1()
                            .border_color(rgb(0xd5d8e0))
                            .child(
                                uniform_list(
                                    "album-rows",
                                    ALBUM_COUNT.div_ceil(ALBUMS_PER_ROW),
                                    cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                                        let selected = this.model.read(cx).selected_album;
                                        range
                                            .map(|row| {
                                                let first = row * ALBUMS_PER_ROW;
                                                div()
                                                    .id(("album-row", row))
                                                    .role(Role::ListItem)
                                                    .aria_label(format!("Album row {}", row + 1))
                                                    .h(px(92.))
                                                    .flex()
                                                    .gap_1()
                                                    .p_1()
                                                    .children(
                                                        (first..(first + ALBUMS_PER_ROW)
                                                            .min(ALBUM_COUNT))
                                                            .map(|index| {
                                                                let model = this.model.clone();
                                                                div()
                                                                    .id(("album", index))
                                                                    .focusable()
                                                                    .tab_stop(true)
                                                                    .role(Role::Button)
                                                                    .aria_label(format!(
                                                                        "Album {}",
                                                                        index + 1
                                                                    ))
                                                                    .aria_selected(index == selected)
                                                                    .flex_1()
                                                                    .p_1()
                                                                    .border_1()
                                                                    .rounded_md()
                                                                    .border_color(rgb(0xb8bdc9))
                                                                    .when(index == selected, |card| {
                                                                        card.bg(rgb(0xdbe8ff))
                                                                    })
                                                                    .focus(|card| {
                                                                        card.border_2().border_color(
                                                                            rgb(0x2457c5),
                                                                        )
                                                                    })
                                                                    .on_click(move |_, _, cx| {
                                                                        model.update(
                                                                            cx,
                                                                            |model, cx| {
                                                                                model.selected_album =
                                                                                    index;
                                                                                model.set_status(
                                                                                    format!(
                                                                                        "Selected album {}",
                                                                                        index + 1
                                                                                    ),
                                                                                    cx,
                                                                                );
                                                                            },
                                                                        );
                                                                    })
                                                                    .child(format!(
                                                                        "Album {}",
                                                                        index + 1
                                                                    ))
                                                            }),
                                                    )
                                            })
                                            .collect()
                                    }),
                                )
                                .h_full(),
                            ),
                    ),
            )
    }
}

struct SharedWindow {
    title: SharedString,
    model: Entity<FeasibilityModel>,
}

impl SharedWindow {
    fn new(title: impl Into<SharedString>, model: Entity<FeasibilityModel>) -> Self {
        Self {
            title: title.into(),
            model,
        }
    }
}

impl Render for SharedWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.model.read(cx).shared_count;
        let model = self.model.clone();
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_2()
            .bg(rgb(0xf6f7fb))
            .child(self.title.clone())
            .child(format!("Shared counter: {count}"))
            .child(
                Button::new("shared-counter")
                    .label("Increment shared state")
                    .on_click(move |_, _, cx| {
                        model.update(cx, |model, cx| {
                            model.shared_count += 1;
                            model.set_status("Shared entity changed from a secondary window", cx);
                        });
                    }),
            )
    }
}

fn install_actions(model: &Entity<FeasibilityModel>, cx: &mut App) {
    let menu_model = model.downgrade();
    cx.on_action(move |_: &MenuProbe, cx| {
        menu_model
            .update(cx, |model, cx| {
                model.shared_count += 1;
                model.set_status("Typed application-menu action dispatched", cx);
            })
            .ok();
    });

    let tray_model = model.downgrade();
    cx.on_action(move |_: &TrayProbe, cx| {
        tray_model
            .update(cx, |model, cx| {
                model.shared_count += 1;
                model.set_status("Tray callback woke GPUI and dispatched TrayProbe", cx);
            })
            .ok();
    });
}

fn install_menus(cx: &mut App) {
    cx.set_menus([Menu::new("Stereodrome").items([
        MenuItem::action("Run typed menu probe", MenuProbe),
        MenuItem::separator(),
        MenuItem::action("Disabled probe", gpui::NoAction).disabled(true),
    ])]);
}

enum TrayMessage {
    Activated,
    #[cfg(target_os = "linux")]
    Started(Result<(), String>),
}

fn install_tray(model: &Entity<FeasibilityModel>, cx: &mut App) {
    let (sender, receiver) = async_channel::unbounded();
    #[cfg(target_os = "linux")]
    let weak = model.downgrade();
    cx.spawn(async move |cx| {
        while let Ok(message) = receiver.recv().await {
            match message {
                TrayMessage::Activated => {
                    cx.update(|cx| cx.dispatch_action(&TrayProbe));
                }
                #[cfg(target_os = "linux")]
                TrayMessage::Started(result) => {
                    weak.update(cx, |model, cx| match result {
                        Ok(()) => model.set_status(
                            "Tray ready; activate its menu to test the GPUI wake path",
                            cx,
                        ),
                        Err(error) => {
                            model.set_status(format!("Tray initialization error: {error}"), cx)
                        }
                    })
                    .ok();
                }
            }
        }
    })
    .detach();

    #[cfg(target_os = "linux")]
    std::thread::spawn(move || {
        let result = gtk::init()
            .map_err(|error| error.to_string())
            .and_then(|_| create_tray(sender.clone()).map(|tray| (tray, sender.clone())));
        match result {
            Ok((_tray, sender)) => {
                sender.try_send(TrayMessage::Started(Ok(()))).ok();
                gtk::main();
            }
            Err(error) => {
                sender.try_send(TrayMessage::Started(Err(error))).ok();
            }
        }
    });

    #[cfg(not(target_os = "linux"))]
    match create_tray(sender) {
        Ok(tray) => model.update(cx, |model, cx| {
            model._tray = Some(tray);
            model.set_status(
                "Tray ready; activate its menu to test the GPUI wake path",
                cx,
            );
        }),
        Err(error) => model.update(cx, |model, cx| {
            model.set_status(format!("Tray initialization error: {error}"), cx);
        }),
    }
}

fn create_tray(sender: async_channel::Sender<TrayMessage>) -> Result<TrayHandle, String> {
    let menu = TrayMenu::new();
    let item = TrayMenuItem::new("Wake GPUI", true, None);
    let item_id = item.id().clone();
    menu.append(&item).map_err(|error| error.to_string())?;
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        if event.id == item_id {
            sender.try_send(TrayMessage::Activated).ok();
        }
    }));

    let mut rgba = vec![0_u8; 32 * 32 * 4];
    for pixel in rgba.chunks_exact_mut(4) {
        pixel.copy_from_slice(&[36, 87, 197, 255]);
    }
    let icon = TrayIcon::from_rgba(rgba, 32, 32).map_err(|error| error.to_string())?;
    TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("Stereodrome GPUI feasibility")
        .with_icon(icon)
        .build()
        .map_err(|error| error.to_string())
}

fn probe_native_handle(
    window: &mut Window,
    model: &Entity<FeasibilityModel>,
    cx: &mut Context<Feasibility>,
) -> Result<(), String> {
    let handle = raw_window_handle::HasWindowHandle::window_handle(window)
        .map_err(|error| error.to_string())?;
    let raw = handle.as_raw();

    #[cfg(target_os = "windows")]
    {
        let RawWindowHandle::Win32(handle) = raw else {
            return Err(format!("expected Win32 handle, received {raw:?}"));
        };
        let hwnd = handle.hwnd.get() as usize;
        let mut controls = souvlaki::MediaControls::new(souvlaki::PlatformConfig {
            dbus_name: "stereodrome",
            display_name: "Stereodrome",
            hwnd: Some(hwnd as *mut std::ffi::c_void),
        })
        .map_err(|error| format!("souvlaki initialization failed: {error:?}"))?;
        controls
            .attach(|_| {})
            .map_err(|error| format!("souvlaki attach failed: {error:?}"))?;
        model.update(cx, |model, cx| {
            model._media_controls = Some(controls);
            model.set_status(format!("Valid HWND {hwnd:#x}; souvlaki initialized"), cx);
        });
        return Ok(());
    }

    #[cfg(not(target_os = "windows"))]
    {
        model.update(cx, |model, cx| {
            model.set_status(format!("Native window handle: {raw:?}"), cx);
        });
        Ok(())
    }
}

#[cfg(target_os = "macos")]
fn apply_panel_flags(window: &mut Window) -> Result<String, String> {
    use objc2_app_kit::{
        NSFloatingWindowLevel, NSView, NSWindowCollectionBehavior, NSWindowStyleMask,
    };

    let handle = raw_window_handle::HasWindowHandle::window_handle(window)
        .map_err(|error| error.to_string())?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return Err("expected an AppKit native handle".into());
    };
    let view = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
    let native_window = view.window().ok_or("AppKit view has no NSWindow")?;
    native_window.setStyleMask(native_window.styleMask() | NSWindowStyleMask::NonactivatingPanel);
    native_window.setCollectionBehavior(
        native_window.collectionBehavior()
            | NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary,
    );
    native_window.setLevel(NSFloatingWindowLevel);
    Ok("Floating mini opened; nonactivating/all-spaces panel flags applied".into())
}

#[cfg(not(target_os = "macos"))]
fn apply_panel_flags(window: &mut Window) -> Result<String, String> {
    let handle = raw_window_handle::HasWindowHandle::window_handle(window)
        .map_err(|error| error.to_string())?;
    Ok(format!(
        "Floating mini opened with shared entity and native handle {:?}",
        handle.as_raw()
    ))
}
