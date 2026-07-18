use std::time::Duration;

#[cfg(target_os = "windows")]
use std::ffi::c_void;

#[cfg(any(target_os = "macos", target_os = "windows"))]
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

use gpui::{
    AnyWindowHandle, App, AppContext as _, Bounds, Context, Entity, FocusHandle, Focusable as _,
    MouseButton, ObjectFit, Render, Role, Subscription, Task, TitlebarOptions, Window,
    WindowBounds, WindowKind, WindowOptions, div, img, point, prelude::*, px, relative, size,
};
use gpui_component::{ActiveTheme as _, Disableable as _, Root, button::Button, input::InputEvent};

use super::{
    actions::{ClearFocus, Quit},
    model::{DesktopModel, VisibleSurface},
    views::{
        auth,
        library::{
            AddSongToPlaylist, DeletePlaylist, LibraryView, PlaySelected, PlaySelectedNext,
            QueueSelected, RemovePlaylistSong, RenamePlaylist, TogglePlaylistOffline,
        },
        player::PlayerView,
        settings::SettingsView,
    },
};

struct CoverArtView {
    model: Entity<DesktopModel>,
}

impl CoverArtView {
    fn new(model: Entity<DesktopModel>, cx: &mut Context<Self>) -> Self {
        cx.observe(&model, |_, _, cx| cx.notify()).detach();
        Self { model }
    }
}

impl Render for CoverArtView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .bg(cx.theme().background)
            .when_some(self.model.read(cx).cover_art_path.clone(), |view, path| {
                view.child(img(path).size_full().object_fit(ObjectFit::Contain))
            })
    }
}
struct AuxiliaryPlayerView {
    model: Entity<DesktopModel>,
    nano: bool,
    hovered: bool,
    bounds_save_task: Option<Task<()>>,
}

impl AuxiliaryPlayerView {
    fn new(
        model: Entity<DesktopModel>,
        nano: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&model, |_, _, cx| cx.notify()).detach();
        if !nano {
            cx.observe_window_bounds(window, |this, window, cx| {
                if this.bounds_save_task.is_some() {
                    return;
                }
                this.bounds_save_task = Some(cx.spawn_in(window, async move |this, cx| {
                    cx.background_executor()
                        .timer(Duration::from_millis(100))
                        .await;
                    this.update_in(cx, |this, window, cx| {
                        let origin = window.window_bounds().get_bounds().origin;
                        this.model.update(cx, |model, cx| {
                            model.persist_mini_player_position(
                                stereodrome_desktop::operations::settings::MiniPlayerPosition {
                                    x: f64::from(origin.x),
                                    y: f64::from(origin.y),
                                },
                                cx,
                            );
                        });
                        this.bounds_save_task.take();
                    })
                    .ok();
                }));
            })
            .detach();
        }
        Self {
            model,
            nano,
            hovered: false,
            bounds_save_task: None,
        }
    }
}

impl Render for AuxiliaryPlayerView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (playback, queue, cover_art_path, show_next_song) = {
            let state = self.model.read(cx);
            (
                state.playback.clone(),
                state.queue.clone(),
                state.current_cover_art_path.clone(),
                state.playback_settings.show_next_song_in_miniplayer,
            )
        };
        let has_song = playback.song.is_some();
        let title = playback
            .song
            .as_ref()
            .map_or_else(|| "Nothing playing".to_string(), |song| song.title.clone());
        let artist = playback
            .song
            .as_ref()
            .map_or_else(String::new, |song| song.artist.clone());
        let progress = if playback.duration > 0.0 {
            (playback.position / playback.duration).clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        let next_song = queue.prepared_next_item.clone().or_else(|| {
            queue
                .current_index
                .and_then(|index| queue.items.get(index + 1).cloned())
        });
        let previous_model = self.model.clone();
        let toggle_model = self.model.clone();
        let next_model = self.model.clone();
        let reroll_model = self.model.clone();
        let backward_model = self.model.clone();
        let forward_model = self.model.clone();
        let (primary_text, secondary_text) = if !has_song {
            (" ".to_string(), "Not Playing".to_string())
        } else if show_next_song {
            (
                format!("{artist} — {title}"),
                next_song.as_ref().map_or_else(
                    || "Next: Unknown".to_string(),
                    |song| format!("Next: {} — {}", song.artist, song.title),
                ),
            )
        } else {
            (title, artist)
        };
        let restore_model = self.model.clone();
        let nano_mode_model = self.model.clone();
        let mini_mode_model = self.model.clone();

        div()
            .id(if self.nano {
                "nano-player"
            } else {
                "mini-player"
            })
            .key_context("Stereodrome")
            .role(Role::Application)
            .aria_label(if self.nano {
                "Stereodrome nano player"
            } else {
                "Stereodrome mini player"
            })
            .size_full()
            .relative()
            .overflow_hidden()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .on_hover(cx.listener(|this, hovered, _, cx| {
                if this.hovered != *hovered {
                    this.hovered = *hovered;
                    cx.notify();
                }
            }))
            .when(self.nano, |view| {
                view.child(
                    div()
                        .id("restore-mini")
                        .role(Role::Button)
                        .aria_label("Restore mini player")
                        .focusable()
                        .tab_stop(true)
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(if playback.is_playing { "▶" } else { "Ⅱ" })
                        .on_click(move |_, window, cx| {
                            if let Err(error) = open_mini_player(nano_mode_model.clone(), cx) {
                                nano_mode_model
                                    .update(cx, |model, cx| model.set_action_error(error, cx));
                            } else {
                                window.remove_window();
                            }
                        }),
                )
            })
            .when(!self.nano, |view| {
                view.p_1()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(
                        div()
                            .id("mini-cover-art")
                            .size(px(56.0))
                            .flex_none()
                            .relative()
                            .overflow_hidden()
                            .rounded_sm()
                            .bg(cx.theme().secondary)
                            .when_some(cover_art_path, |cover, path| {
                                cover.child(img(path).size_full().object_fit(ObjectFit::Cover))
                            })
                            .when(self.hovered, |cover| {
                                cover.child(
                                    div()
                                        .absolute()
                                        .inset_0()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(
                                            Button::new("mini-toggle")
                                                .compact()
                                                .label(if playback.is_playing {
                                                    "Pause"
                                                } else {
                                                    "Play"
                                                })
                                                .disabled(!has_song)
                                                .on_click(move |_, _, cx| {
                                                    toggle_model
                                                        .update(cx, DesktopModel::toggle_playback);
                                                }),
                                        ),
                                )
                            }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .truncate()
                                    .child(primary_text),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .truncate()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(secondary_text),
                            )
                            .when(!self.hovered, |details| {
                                details.child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_1()
                                        .child(
                                            div()
                                                .w(px(34.0))
                                                .text_xs()
                                                .child(format_player_duration(playback.position)),
                                        )
                                        .child(
                                            div()
                                                .h(px(4.0))
                                                .flex_1()
                                                .rounded_full()
                                                .bg(cx.theme().secondary)
                                                .child(
                                                    div()
                                                        .h_full()
                                                        .w(relative(progress))
                                                        .rounded_full()
                                                        .bg(cx.theme().primary),
                                                ),
                                        )
                                        .child(div().w(px(34.0)).text_xs().child(format!(
                                            "-{}",
                                            format_player_duration(
                                                (playback.duration - playback.position).max(0.0)
                                            )
                                        ))),
                                )
                            })
                            .when(self.hovered, |details| {
                                details.child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .gap_1()
                                        .child(
                                            Button::new("mini-previous")
                                                .compact()
                                                .label("Prev")
                                                .disabled(queue.items.is_empty())
                                                .on_click(move |_, _, cx| {
                                                    previous_model
                                                        .update(cx, DesktopModel::play_previous);
                                                }),
                                        )
                                        .child(
                                            Button::new("mini-seek-back")
                                                .compact()
                                                .label("-10")
                                                .disabled(!has_song)
                                                .on_click(move |_, _, cx| {
                                                    backward_model.update(cx, |model, cx| {
                                                        model.seek_by(-10.0, cx)
                                                    });
                                                }),
                                        )
                                        .child(
                                            Button::new("mini-reroll")
                                                .compact()
                                                .label("Roll")
                                                .disabled(!queue.shuffle || queue.items.len() < 2)
                                                .on_click(move |_, _, cx| {
                                                    reroll_model
                                                        .update(cx, DesktopModel::reroll_next);
                                                }),
                                        )
                                        .child(
                                            Button::new("mini-seek-forward")
                                                .compact()
                                                .label("+10")
                                                .disabled(!has_song)
                                                .on_click(move |_, _, cx| {
                                                    forward_model.update(cx, |model, cx| {
                                                        model.seek_by(10.0, cx)
                                                    });
                                                }),
                                        )
                                        .child(
                                            Button::new("mini-next")
                                                .compact()
                                                .label("Next")
                                                .disabled(queue.items.is_empty())
                                                .on_click(move |_, _, cx| {
                                                    next_model.update(cx, DesktopModel::play_next);
                                                }),
                                        ),
                                )
                            }),
                    )
                    .when(self.hovered, |view| {
                        view.child(
                            div()
                                .absolute()
                                .top_1()
                                .left(px(64.0))
                                .flex()
                                .gap_1()
                                .child(
                                    Button::new("mini-drag")
                                        .compact()
                                        .label("Drag")
                                        .on_mouse_down(MouseButton::Left, |_, window, _| {
                                            window.start_window_move()
                                        }),
                                )
                                .child(Button::new("mini-main").compact().label("Main").on_click(
                                    move |_, _, cx| {
                                        if let Err(error) =
                                            open_main_window(restore_model.clone(), cx)
                                        {
                                            restore_model.update(cx, |model, cx| {
                                                model.set_action_error(error, cx)
                                            });
                                        }
                                    },
                                )),
                        )
                        .child(
                            div()
                                .absolute()
                                .top_1()
                                .right_1()
                                .flex()
                                .gap_1()
                                .child(Button::new("mini-nano").compact().label("Nano").on_click(
                                    move |_, window, cx| {
                                        if let Err(error) =
                                            open_nano_player(mini_mode_model.clone(), cx)
                                        {
                                            mini_mode_model.update(cx, |model, cx| {
                                                model.set_action_error(error, cx)
                                            });
                                        } else {
                                            window.remove_window();
                                        }
                                    },
                                ))
                                .child(
                                    Button::new("mini-close")
                                        .compact()
                                        .label("Close")
                                        .on_click(|_, window, _| window.remove_window()),
                                ),
                        )
                    })
            })
    }
}

fn format_player_duration(seconds: f64) -> String {
    let seconds = seconds.max(0.0) as u64;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

pub struct MainView {
    model: Entity<DesktopModel>,
    inputs: auth::AuthInputs,
    library: Entity<LibraryView>,
    player: Entity<PlayerView>,
    focus: FocusHandle,
    surface: VisibleSurface,
    _subscriptions: Vec<Subscription>,
}

impl MainView {
    fn new(model: Entity<DesktopModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let inputs = auth::AuthInputs::new(window, cx);
        let surface = model.read(cx).visible_surface();
        if surface == VisibleSurface::Login {
            window.focus(&inputs.url.focus_handle(cx), cx);
        } else {
            window.focus(&focus, cx);
        }
        let username_focus = inputs.username.focus_handle(cx);
        let password_focus = inputs.password.focus_handle(cx);
        let subscriptions = vec![
            cx.subscribe_in(&inputs.url, window, move |_, _, event, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    window.focus(&username_focus, cx);
                }
            }),
            cx.subscribe_in(&inputs.username, window, move |_, _, event, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    window.focus(&password_focus, cx);
                }
            }),
            cx.subscribe_in(&inputs.password, window, |this, _, event, _, cx| {
                if matches!(event, InputEvent::PressEnter { .. })
                    && this.model.read(cx).visible_surface() == VisibleSurface::Login
                    && !this.model.read(cx).auth.connecting
                {
                    auth::submit(&this.inputs, &this.model, cx);
                }
            }),
        ];
        let library_model = model.clone();
        let library = cx.new(|cx| LibraryView::new(library_model, window, cx));
        let player_model = model.clone();
        let player = cx.new(|cx| PlayerView::new(player_model, cx));
        cx.observe_in(&model, window, |this, _, window, cx| {
            let surface = this.model.read(cx).visible_surface();
            if surface != this.surface {
                this.surface = surface;
                if surface == VisibleSurface::Login {
                    window.focus(&this.inputs.url.focus_handle(cx), cx);
                } else {
                    window.focus(&this.focus, cx);
                }
            }
            cx.notify();
        })
        .detach();
        Self {
            model,
            inputs,
            player,
            library,
            focus,
            surface,
            _subscriptions: subscriptions,
        }
    }
}

impl Render for MainView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match self.model.read(cx).visible_surface() {
            VisibleSurface::Library => div()
                .size_full()
                .flex()
                .flex_col()
                .child(div().flex_1().min_h_0().child(self.library.clone()))
                .child(self.player.clone())
                .into_any_element(),
            VisibleSurface::Restoring | VisibleSurface::Login => {
                auth::render(&self.inputs, &self.model, cx).into_any_element()
            }
        };
        div()
            .id("stereodrome-root")
            .key_context("Stereodrome")
            .role(Role::Application)
            .aria_label("Stereodrome")
            .track_focus(&self.focus)
            .on_action(cx.listener(|_, _: &ClearFocus, window, _| window.blur()))
            .on_action(cx.listener(|this, _: &RenamePlaylist, window, cx| {
                this.library
                    .update(cx, |view, cx| view.show_rename_playlist(window, cx));
            }))
            .on_action(cx.listener(|this, _: &DeletePlaylist, window, cx| {
                this.library
                    .update(cx, |view, cx| view.show_delete_playlist(window, cx));
            }))
            .on_action(cx.listener(|this, _: &TogglePlaylistOffline, _, cx| {
                this.library
                    .update(cx, |view, cx| view.toggle_selected_playlist_offline(cx));
            }))
            .on_action(cx.listener(|this, _: &AddSongToPlaylist, window, cx| {
                this.model
                    .update(cx, DesktopModel::ensure_visible_song_selection);
                this.library
                    .update(cx, |view, cx| view.show_add_song_to_playlist(window, cx));
            }))
            .on_action(cx.listener(|this, _: &RemovePlaylistSong, _, cx| {
                this.library
                    .update(cx, |view, cx| view.remove_selected_playlist_songs(cx));
            }))
            .on_action(cx.listener(|this, _: &PlaySelected, _, cx| {
                this.model.update(cx, |model, cx| {
                    model.ensure_visible_song_selection(cx);
                    model.play_selection(cx);
                });
            }))
            .on_action(cx.listener(|this, _: &PlaySelectedNext, _, cx| {
                this.model.update(cx, |model, cx| {
                    model.ensure_visible_song_selection(cx);
                    model.add_selection_to_queue(true, cx);
                });
            }))
            .on_action(cx.listener(|this, _: &QueueSelected, _, cx| {
                this.model.update(cx, |model, cx| {
                    model.ensure_visible_song_selection(cx);
                    model.add_selection_to_queue(false, cx);
                });
            }))
            .size_full()
            .child(content)
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

#[cfg(target_os = "windows")]
pub fn main_hwnd(model: &Entity<DesktopModel>, cx: &mut App) -> Option<*mut c_void> {
    let handle = model.read(cx).windows.main?;
    handle
        .update(cx, |_, window, _| {
            let handle = HasWindowHandle::window_handle(window).ok()?;
            match handle.as_raw() {
                RawWindowHandle::Win32(handle) => Some(handle.hwnd.get() as *mut c_void),
                _ => None,
            }
        })
        .ok()
        .flatten()
}

#[cfg(target_os = "macos")]
fn apply_panel_flags(window: &mut Window) -> Result<(), String> {
    use objc2_app_kit::{
        NSFloatingWindowLevel, NSView, NSWindowCollectionBehavior, NSWindowStyleMask,
    };

    let handle = HasWindowHandle::window_handle(window).map_err(|error| error.to_string())?;
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
    Ok(())
}

#[derive(Clone, Copy)]
enum AuxiliaryPlayerKind {
    Mini,
    Nano,
}

pub fn open_mini_player(model: Entity<DesktopModel>, cx: &mut App) -> Result<(), String> {
    open_auxiliary_player(model, AuxiliaryPlayerKind::Mini, cx)
}

pub fn open_nano_player(model: Entity<DesktopModel>, cx: &mut App) -> Result<(), String> {
    open_auxiliary_player(model, AuxiliaryPlayerKind::Nano, cx)
}

fn auxiliary_player_bounds(
    model: &Entity<DesktopModel>,
    window_size: gpui::Size<gpui::Pixels>,
    cx: &App,
) -> Bounds<gpui::Pixels> {
    let Some(position) = model.read(cx).mini_player_position() else {
        return Bounds::centered(None, window_size, cx);
    };
    let mut bounds = Bounds::new(
        point(px(position.x as f32), px(position.y as f32)),
        window_size,
    );
    let center = bounds.center();
    let display = cx
        .displays()
        .into_iter()
        .find(|display| display.bounds().contains(&center))
        .or_else(|| cx.primary_display());
    let Some(display) = display else {
        return bounds;
    };
    let visible = display.visible_bounds();
    let max_x = visible.origin.x + visible.size.width - window_size.width;
    let max_y = visible.origin.y + visible.size.height - window_size.height;
    bounds.origin.x = if max_x < visible.origin.x || bounds.origin.x < visible.origin.x {
        visible.origin.x
    } else if bounds.origin.x > max_x {
        max_x
    } else {
        bounds.origin.x
    };
    bounds.origin.y = if max_y < visible.origin.y || bounds.origin.y < visible.origin.y {
        visible.origin.y
    } else if bounds.origin.y > max_y {
        max_y
    } else {
        bounds.origin.y
    };
    bounds
}

fn nano_player_bounds(
    model: &Entity<DesktopModel>,
    window_size: gpui::Size<gpui::Pixels>,
    cx: &App,
) -> Bounds<gpui::Pixels> {
    let mini_bounds = auxiliary_player_bounds(model, size(px(320.0), px(72.0)), cx);
    let center = mini_bounds.center();
    let display = cx
        .displays()
        .into_iter()
        .find(|display| display.bounds().contains(&center))
        .or_else(|| cx.primary_display());
    let Some(display) = display else {
        return Bounds::new(mini_bounds.origin, window_size);
    };
    let visible = display.visible_bounds();
    let right_distance =
        visible.origin.x + visible.size.width - mini_bounds.origin.x - mini_bounds.size.width;
    let bottom_distance =
        visible.origin.y + visible.size.height - mini_bounds.origin.y - mini_bounds.size.height;
    let x = if mini_bounds.origin.x - visible.origin.x <= right_distance {
        visible.origin.x
    } else {
        visible.origin.x + visible.size.width - window_size.width
    };
    let y = if mini_bounds.origin.y - visible.origin.y <= bottom_distance {
        visible.origin.y
    } else {
        visible.origin.y + visible.size.height - window_size.height
    };
    Bounds::new(point(x, y), window_size)
}

fn open_auxiliary_player(
    model: Entity<DesktopModel>,
    kind: AuxiliaryPlayerKind,
    cx: &mut App,
) -> Result<(), String> {
    if model.read(cx).quitting {
        return Ok(());
    }
    let existing = match kind {
        AuxiliaryPlayerKind::Mini => model.read(cx).windows.mini,
        AuxiliaryPlayerKind::Nano => model.read(cx).windows.nano,
    };
    if let Some(handle) = existing {
        if handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
        {
            return Ok(());
        }
        model.update(cx, |model, cx| {
            match kind {
                AuxiliaryPlayerKind::Mini => model.windows.mini = None,
                AuxiliaryPlayerKind::Nano => model.windows.nano = None,
            }
            cx.notify();
        });
    }

    let (window_size, title, nano) = match kind {
        AuxiliaryPlayerKind::Mini => (size(px(320.0), px(72.0)), "Mini Player", false),
        AuxiliaryPlayerKind::Nano => (size(px(30.0), px(30.0)), "Nano Player", true),
    };
    let bounds = match kind {
        AuxiliaryPlayerKind::Mini => auxiliary_player_bounds(&model, window_size, cx),
        AuxiliaryPlayerKind::Nano => nano_player_bounds(&model, window_size, cx),
    };
    let view_model = model.clone();
    let handle = cx
        .open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(window_size),
                kind: WindowKind::Floating,
                is_resizable: false,
                titlebar: None,
                ..Default::default()
            },
            move |window, cx| {
                let view = cx.new(|cx| AuxiliaryPlayerView::new(view_model, nano, window, cx));
                cx.new(|cx| Root::new(view, window, cx))
            },
        )
        .map_err(|error| format!("Failed to open {title}: {error:#}"))?;
    let handle = AnyWindowHandle::from(handle);
    #[cfg(target_os = "macos")]
    handle
        .update(cx, |_, window, _| apply_panel_flags(window))
        .map_err(|error| format!("Failed to access {title} native window: {error}"))??;
    model.update(cx, |model, cx| {
        match kind {
            AuxiliaryPlayerKind::Mini => model.windows.mini = Some(handle),
            AuxiliaryPlayerKind::Nano => model.windows.nano = Some(handle),
        }
        cx.notify();
    });
    Ok(())
}

pub fn open_settings_window(model: Entity<DesktopModel>, cx: &mut App) -> Result<(), String> {
    if model.read(cx).quitting {
        return Ok(());
    }
    if let Some(handle) = model.read(cx).windows.settings {
        if handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
        {
            return Ok(());
        }
        model.update(cx, |model, cx| {
            model.windows.settings = None;
            cx.notify();
        });
    }

    let bounds = Bounds::centered(None, size(px(720.0), px(760.0)), cx);
    let view_model = model.clone();
    let handle = cx
        .open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(560.0), px(600.0))),
                titlebar: Some(TitlebarOptions {
                    title: Some("Settings".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            move |window, cx| {
                let view = cx.new(|cx| SettingsView::new(view_model, window, cx));
                cx.new(|cx| Root::new(view, window, cx))
            },
        )
        .map_err(|error| format!("Failed to open settings window: {error:#}"))?;
    let handle = AnyWindowHandle::from(handle);
    handle
        .update(cx, |_, window, _| window.activate_window())
        .map_err(|error| format!("Failed to activate settings window: {error}"))?;
    model.update(cx, |model, cx| {
        model.windows.settings = Some(handle);
        cx.notify();
    });
    Ok(())
}

pub fn open_cover_art_window(model: Entity<DesktopModel>, cx: &mut App) -> Result<(), String> {
    if let Some(handle) = model.read(cx).windows.cover_art {
        if handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
        {
            return Ok(());
        }
        model.update(cx, |model, cx| {
            model.windows.cover_art = None;
            cx.notify();
        });
    }

    let bounds = Bounds::centered(None, size(px(640.0), px(640.0)), cx);
    let view_model = model.clone();
    let handle = cx
        .open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(320.0), px(320.0))),
                titlebar: Some(TitlebarOptions {
                    title: Some("Cover Art".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            move |_, cx| cx.new(|cx| CoverArtView::new(view_model, cx)),
        )
        .map_err(|error| format!("Failed to open cover art window: {error:#}"))?;
    let handle = AnyWindowHandle::from(handle);
    handle
        .update(cx, |_, window, _| window.activate_window())
        .map_err(|error| format!("Failed to activate cover art window: {error}"))?;
    model.update(cx, |model, cx| {
        model.windows.cover_art = Some(handle);
        cx.notify();
    });
    Ok(())
}

pub fn observe_main_window(model: &Entity<DesktopModel>, cx: &mut App) {
    let weak = model.downgrade();
    cx.on_window_closed(move |cx, closed_id| {
        weak.update(cx, |model, cx| {
            let mut changed = false;
            if model
                .windows
                .main
                .is_some_and(|handle| handle.window_id() == closed_id)
            {
                model.windows.main = None;
                if !model.tray_available && !model.quitting {
                    cx.dispatch_action(&Quit);
                }
                changed = true;
            }
            if model
                .windows
                .mini
                .is_some_and(|handle| handle.window_id() == closed_id)
            {
                model.windows.mini = None;
                changed = true;
            }
            if model
                .windows
                .nano
                .is_some_and(|handle| handle.window_id() == closed_id)
            {
                model.windows.nano = None;
                changed = true;
            }
            if model
                .windows
                .cover_art
                .is_some_and(|handle| handle.window_id() == closed_id)
            {
                model.windows.cover_art = None;
                changed = true;
            }
            if model
                .windows
                .settings
                .is_some_and(|handle| handle.window_id() == closed_id)
            {
                model.windows.settings = None;
                changed = true;
            }
            if changed {
                cx.notify();
            }
        })
        .ok();
    })
    .detach();
}
