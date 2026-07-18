use chrono::{DateTime, Local};
use std::collections::{HashMap, HashSet};
use std::{rc::Rc, sync::Arc};

use gpui::{
    Context, Entity, Focusable as _, FontWeight, IntoElement, MouseButton, ObjectFit, Render, Role,
    Subscription, Window, div, img, prelude::*, px, uniform_list,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Selectable as _, WindowExt as _,
    button::{Button, ButtonVariant, ButtonVariants as _},
    dialog::DialogButtonProps,
    input::{Input, InputEvent, InputState},
    menu::ContextMenuExt as _,
    scroll::ScrollableElement as _,
};
use stereodrome_desktop::operations::library::{Song, SyncJobKind};

use crate::ui::model::{DesktopModel, NavigationView};

gpui::actions!(
    library,
    [
        RenamePlaylist,
        DeletePlaylist,
        TogglePlaylistOffline,
        AddSongToPlaylist,
        RemovePlaylistSong,
        PlaySelected,
        PlaySelectedNext,
        QueueSelected,
    ]
);

pub struct LibraryView {
    model: Entity<DesktopModel>,
    search: Entity<InputState>,
    last_search_focus_generation: u64,
    _subscriptions: Vec<Subscription>,
}

impl LibraryView {
    pub fn new(model: Entity<DesktopModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search your library"));
        let search_model = model.clone();
        let subscriptions = vec![
            cx.subscribe(&search, move |_, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    let query = input.read(cx).value().to_string();
                    search_model.update(cx, |model, cx| model.set_search_query(query, cx));
                }
            }),
            cx.observe(&model, |_, _, cx| cx.notify()),
        ];
        Self {
            model,
            search,
            last_search_focus_generation: 0,
            _subscriptions: subscriptions,
        }
    }

    pub fn show_create_playlist(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_playlist_name_dialog("New Playlist", "Create", None, "", window, cx);
    }

    pub fn show_rename_playlist(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let state = self.model.read(cx);
        let Some(playlist_id) = state.library.selected_playlist_id.clone() else {
            return;
        };
        let name = state
            .library
            .playlists
            .iter()
            .find(|playlist| playlist.id == playlist_id)
            .map(|playlist| playlist.name.clone())
            .unwrap_or_default();
        self.show_playlist_name_dialog(
            "Rename Playlist",
            "Rename",
            Some(playlist_id),
            &name,
            window,
            cx,
        );
    }

    fn show_playlist_name_dialog(
        &self,
        title: &'static str,
        confirmation: &'static str,
        playlist_id: Option<String>,
        initial_name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Playlist name")
                .default_value(initial_name)
        });
        let model = self.model.clone();
        let dialog_input = input.clone();
        let dialog_playlist_id = playlist_id.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            let input_for_confirmation = dialog_input.clone();
            let playlist_id = dialog_playlist_id.clone();
            let model = model.clone();
            dialog
                .title(title)
                .child(Input::new(&dialog_input))
                .button_props(
                    DialogButtonProps::default()
                        .show_cancel(true)
                        .ok_text(confirmation)
                        .on_ok(move |_, _, cx| {
                            let name = input_for_confirmation.read(cx).value().trim().to_string();
                            if name.is_empty() {
                                return false;
                            }
                            model.update(cx, |model, cx| {
                                if let Some(playlist_id) = playlist_id.clone() {
                                    model.rename_playlist(playlist_id, name, cx);
                                } else {
                                    model.create_playlist(name, cx);
                                }
                            });
                            true
                        }),
                )
        });
        window.focus(&input.focus_handle(cx), cx);
    }

    pub fn show_delete_playlist(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let state = self.model.read(cx);
        let Some(playlist_id) = state.library.selected_playlist_id.clone() else {
            return;
        };
        let name = state
            .library
            .playlists
            .iter()
            .find(|playlist| playlist.id == playlist_id)
            .map(|playlist| playlist.name.clone())
            .unwrap_or_else(|| "this playlist".to_string());
        let model = self.model.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            let model = model.clone();
            let playlist_id = playlist_id.clone();
            dialog
                .title("Delete Playlist")
                .child(format!("Delete “{name}”? This cannot be undone."))
                .button_props(
                    DialogButtonProps::default()
                        .show_cancel(true)
                        .ok_text("Delete")
                        .ok_variant(ButtonVariant::Danger)
                        .on_ok(move |_, _, cx| {
                            model.update(cx, |model, cx| {
                                model.delete_playlist(playlist_id.clone(), cx)
                            });
                            true
                        }),
                )
        });
    }

    pub fn show_add_song_to_playlist(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let state = self.model.read(cx);
        let song_ids = if state.selection.song_ids.is_empty() {
            state.selection.song_id.iter().cloned().collect::<Vec<_>>()
        } else {
            state.selection.song_ids.clone()
        };
        if song_ids.is_empty() {
            return;
        }
        let playlists = state.library.playlists.clone();
        let model = self.model.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            let rows = playlists.clone().into_iter().map(|playlist| {
                let model = model.clone();
                let song_ids = song_ids.clone();
                let playlist_id = playlist.id;
                Button::new(format!("add-to-{playlist_id}"))
                    .label(playlist.name)
                    .on_click(move |_, window, cx| {
                        model.update(cx, |model, cx| {
                            model.add_songs_to_playlist(playlist_id.clone(), song_ids.clone(), cx)
                        });
                        window.close_dialog(cx);
                    })
            });
            dialog
                .title("Add to Playlist")
                .child(div().flex().flex_col().gap_1().children(rows))
        });
    }

    pub fn toggle_selected_playlist_offline(&mut self, cx: &mut Context<Self>) {
        let state = self.model.read(cx);
        let Some(playlist) = state
            .library
            .playlists
            .iter()
            .find(|playlist| Some(&playlist.id) == state.library.selected_playlist_id.as_ref())
        else {
            return;
        };
        let playlist_id = playlist.id.clone();
        let saved_offline = !playlist.saved_offline;
        self.model.update(cx, |model, cx| {
            model.set_playlist_saved_offline(playlist_id, saved_offline, cx)
        });
    }

    pub fn remove_selected_playlist_songs(&mut self, cx: &mut Context<Self>) {
        let state = self.model.read(cx);
        let positions = state
            .library
            .playlist_songs
            .iter()
            .filter(|song| state.selection.song_ids.contains(&song.id))
            .map(|song| song.position)
            .collect::<Vec<_>>();
        if positions.is_empty() {
            return;
        }
        self.model
            .update(cx, |model, cx| model.remove_playlist_songs(positions, cx));
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let active = self.model.read(cx).navigation.active_view;
        let playlists = self
            .model
            .read(cx)
            .library
            .playlists
            .iter()
            .map(|playlist| (playlist.id.clone(), playlist.name.clone()))
            .collect::<Vec<_>>();

        let mut sidebar = div()
            .id("library-navigation")
            .role(Role::Navigation)
            .aria_label("Library navigation")
            .w(px(190.0))
            .h_full()
            .flex()
            .flex_col()
            .gap_1()
            .p_2()
            .border_r_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().sidebar)
            .child(
                div()
                    .px_2()
                    .py_1()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Stereodrome"),
            );

        for (view, label) in [
            (NavigationView::Music, "Music"),
            (NavigationView::Artists, "Artists"),
            (NavigationView::Albums, "Albums"),
            (NavigationView::RecentlyAdded, "Recently Added"),
            (NavigationView::RecentlyPlayed, "Recently Played"),
            (NavigationView::MostPlayed, "Most Played"),
        ] {
            let model = self.model.clone();
            sidebar = sidebar.child(
                Button::new(format!("nav-{label}"))
                    .label(label)
                    .selected(active == view)
                    .on_click(move |_, _, cx| {
                        model.update(cx, |model, cx| model.navigate(view, cx));
                    }),
            );
        }

        let playlists_model = self.model.clone();
        sidebar = sidebar
            .child(div().mt_2().px_2().text_sm().child("Playlists"))
            .child(
                Button::new("nav-playlists")
                    .label("All Playlists")
                    .selected(active == NavigationView::Playlists)
                    .on_click(move |_, _, cx| {
                        playlists_model.update(cx, |model, cx| {
                            model.navigate(NavigationView::Playlists, cx)
                        });
                    }),
            );
        for (id, name) in playlists {
            let model = self.model.clone();
            sidebar = sidebar.child(Button::new(format!("playlist-{id}")).label(name).on_click(
                move |_, _, cx| {
                    model.update(cx, |model, cx| model.select_playlist(id.clone(), cx));
                },
            ));
        }
        sidebar.overflow_y_scroll().into_any_element()
    }

    fn render_toolbar(&mut self, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let state = self.model.read(cx);
        let focus_generation = state.navigation.search_focus_generation;
        let pending = state.library.search_pending;
        let offline = state.offline();
        let sync_active = state
            .library_sync_status
            .as_ref()
            .and_then(|status| status.active_job)
            .is_some();
        let scan_active = state
            .scan_status
            .as_ref()
            .is_some_and(|status| status.scanning);
        if focus_generation != self.last_search_focus_generation {
            self.last_search_focus_generation = focus_generation;
            window.focus(&self.search.focus_handle(cx), cx);
        }
        let refresh_model = self.model.clone();
        let new_playlist_view = cx.entity();
        let sync_playlists_model = self.model.clone();
        let sync_library_model = self.model.clone();
        let reconcile_model = self.model.clone();
        let scan_model = self.model.clone();
        div()
            .h(px(52.0))
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .id("library-search")
                    .role(Role::Search)
                    .aria_label("Search library")
                    .flex_1()
                    .child(Input::new(&self.search).cleanable(true)),
            )
            .child(
                Button::new("refresh-library")
                    .label(if pending { "Searching…" } else { "Refresh" })
                    .disabled(pending)
                    .on_click(move |_, _, cx| {
                        refresh_model.update(cx, |model, cx| {
                            model.refresh_library(cx);
                            model.refresh_library_statuses(cx);
                        });
                    }),
            )
            .child(
                Button::new("sync-library")
                    .label(if sync_active { "Syncing…" } else { "Sync" })
                    .disabled(offline || sync_active)
                    .on_click(move |_, _, cx| {
                        sync_library_model.update(cx, |model, cx| model.sync_library(false, cx));
                    }),
            )
            .child(
                Button::new("reconcile-library")
                    .label("Reconcile")
                    .disabled(offline || sync_active)
                    .on_click(move |_, _, cx| {
                        reconcile_model.update(cx, |model, cx| model.sync_library(true, cx));
                    }),
            )
            .child(
                Button::new("scan-library")
                    .label(if scan_active { "Scanning…" } else { "Scan" })
                    .disabled(offline || scan_active)
                    .on_click(move |_, _, cx| {
                        scan_model.update(cx, DesktopModel::start_scan);
                    }),
            )
            .child(
                Button::new("new-playlist")
                    .label("New Playlist")
                    .disabled(offline)
                    .on_click(move |_, window, cx| {
                        new_playlist_view
                            .update(cx, |view, cx| view.show_create_playlist(window, cx));
                    }),
            )
            .child(
                Button::new("sync-playlists")
                    .label("Playlists")
                    .disabled(offline)
                    .on_click(move |_, _, cx| {
                        sync_playlists_model.update(cx, DesktopModel::sync_playlists);
                    }),
            )
            .when(offline, |toolbar| {
                toolbar.child(
                    div()
                        .id("offline-status")
                        .role(Role::Status)
                        .text_color(cx.theme().warning)
                        .child("Offline"),
                )
            })
            .into_any_element()
    }

    fn filtered_song_indices(&self, cx: &Context<Self>) -> Vec<usize> {
        let state = self.model.read(cx);
        let genre = state.library.selected_genre.as_deref();
        let artist = state.library.selected_artist_id.as_deref();
        let album = state.library.selected_album_id.as_deref();
        state
            .library
            .songs
            .iter()
            .enumerate()
            .filter(|(_, song)| {
                genre.is_none_or(|value| song.genre.as_deref() == Some(value))
                    && artist.is_none_or(|value| song.artist_id == value)
                    && album.is_none_or(|value| song.album_id == value)
            })
            .map(|(index, _)| index)
            .collect()
    }

    fn render_song_list(
        &self,
        indices: Vec<usize>,
        empty: &'static str,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        if indices.is_empty() {
            return div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(cx.theme().muted_foreground)
                .child(empty)
                .into_any_element();
        }
        let visible_song_ids = {
            let state = self.model.read(cx);
            Arc::new(
                indices
                    .iter()
                    .filter_map(|index| state.library.songs.get(*index).map(|song| song.id.clone()))
                    .collect::<Vec<_>>(),
            )
        };
        let indices = Arc::new(indices);
        let row_indices = Arc::clone(&indices);
        uniform_list(
            "library-songs",
            indices.len(),
            cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                let state = this.model.read(cx);
                let selected = &state.selection.song_ids;
                range
                    .filter_map(|visible_row| {
                        let song_index = *row_indices.get(visible_row)?;
                        let song = state.library.songs.get(song_index)?;
                        let song_id = song.id.clone();
                        let context_song_id = song_id.clone();
                        let select_model = this.model.clone();
                        let context_model = this.model.clone();
                        let link_model = this.model.clone();
                        let selection_ids = Arc::clone(&visible_song_ids);
                        Some(song_row(
                            visible_row,
                            song,
                            selected.contains(&song.id),
                            link_model,
                            Rc::new(move |event, cx| {
                                select_model.update(cx, |model, cx| {
                                    model.select_song_with_modifiers(
                                        visible_row,
                                        song_id.clone(),
                                        &selection_ids,
                                        event.modifiers(),
                                        cx,
                                    );
                                    if event.click_count() >= 2 {
                                        model.play_selection(cx);
                                    }
                                });
                            }),
                            Rc::new(move |cx| {
                                context_model.update(cx, |model, cx| {
                                    if !model.selection.song_ids.contains(&context_song_id) {
                                        model.select_song(visible_row, context_song_id.clone(), cx);
                                    }
                                });
                            }),
                            cx,
                        ))
                    })
                    .collect()
            }),
        )
        .h_full()
        .into_any_element()
    }

    fn render_music(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let state = self.model.read(cx);
        let selected_genre = state.library.selected_genre.clone();
        let selected_artist = state.library.selected_artist_id.clone();
        let selected_album = state.library.selected_album_id.clone();

        let mut genres = state
            .library
            .songs
            .iter()
            .filter_map(|song| song.genre.clone())
            .collect::<Vec<_>>();
        genres.sort_unstable();
        genres.dedup();

        let represented_artists = state
            .library
            .songs
            .iter()
            .filter(|song| {
                selected_genre
                    .as_deref()
                    .is_none_or(|genre| song.genre.as_deref() == Some(genre))
            })
            .map(|song| song.artist_id.as_str())
            .collect::<HashSet<_>>();
        let artists = state
            .library
            .artists
            .iter()
            .filter(|artist| represented_artists.contains(artist.id.as_str()))
            .map(|artist| (artist.id.clone(), artist.name.clone()))
            .collect::<Vec<_>>();

        let represented_albums = state
            .library
            .songs
            .iter()
            .filter(|song| {
                selected_genre
                    .as_deref()
                    .is_none_or(|genre| song.genre.as_deref() == Some(genre))
                    && selected_artist
                        .as_deref()
                        .is_none_or(|artist| song.artist_id == artist)
            })
            .map(|song| song.album_id.as_str())
            .collect::<HashSet<_>>();
        let albums = state
            .library
            .albums
            .iter()
            .filter(|album| represented_albums.contains(album.id.as_str()))
            .map(|album| (album.id.clone(), album.name.clone()))
            .collect::<Vec<_>>();

        let mut genre_column = column("Genres", cx);
        let all_model = self.model.clone();
        genre_column = genre_column.child(
            Button::new("genre-all")
                .label("All Genres")
                .selected(selected_genre.is_none())
                .on_click(move |_, _, cx| {
                    all_model.update(cx, |model, cx| model.select_genre(None, cx));
                }),
        );
        for genre in genres {
            let selected = selected_genre.as_ref() == Some(&genre);
            let model = self.model.clone();
            genre_column = genre_column.child(
                Button::new(format!("genre-{genre}"))
                    .label(genre.clone())
                    .selected(selected)
                    .on_click(move |_, _, cx| {
                        model.update(cx, |model, cx| model.select_genre(Some(genre.clone()), cx));
                    }),
            );
        }

        let mut artist_column = column("Artists", cx);
        let all_model = self.model.clone();
        artist_column = artist_column.child(
            Button::new("artist-all")
                .label("All Artists")
                .selected(selected_artist.is_none())
                .on_click(move |_, _, cx| {
                    all_model.update(cx, |model, cx| model.select_artist(None, cx));
                }),
        );
        for (id, name) in artists {
            let selected = selected_artist.as_ref() == Some(&id);
            let model = self.model.clone();
            artist_column = artist_column.child(
                Button::new(format!("artist-{id}"))
                    .label(name)
                    .selected(selected)
                    .on_click(move |_, _, cx| {
                        model.update(cx, |model, cx| model.select_artist(Some(id.clone()), cx));
                    }),
            );
        }

        let mut album_column = column("Albums", cx);
        let all_model = self.model.clone();
        album_column = album_column.child(
            Button::new("album-all")
                .label("All Albums")
                .selected(selected_album.is_none())
                .on_click(move |_, _, cx| {
                    all_model.update(cx, |model, cx| model.select_album(None, cx));
                }),
        );
        for (id, name) in albums {
            let selected = selected_album.as_ref() == Some(&id);
            let model = self.model.clone();
            album_column = album_column.child(
                Button::new(format!("album-{id}"))
                    .label(name)
                    .selected(selected)
                    .on_click(move |_, _, cx| {
                        model.update(cx, |model, cx| model.select_album(Some(id.clone()), cx));
                    }),
            );
        }

        let indices = self.filtered_song_indices(cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(180.0))
                    .flex()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(genre_column)
                    .child(artist_column)
                    .child(album_column),
            )
            .child(div().flex_1().min_h_0().child(self.render_song_list(
                indices,
                "No songs match this selection",
                cx,
            )))
            .into_any_element()
    }

    fn render_grid(
        &self,
        artists: bool,
        album_order: Option<Vec<String>>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let width = (f32::from(window.viewport_size().width) - 230.0).max(180.0);
        let cards_per_row = (width / 180.0).floor().max(1.0) as usize;
        let state = self.model.read(cx);
        let indices = if artists {
            (0..state.library.artists.len()).collect::<Vec<_>>()
        } else if let Some(order) = album_order {
            let by_id = state
                .library
                .albums
                .iter()
                .enumerate()
                .map(|(index, album)| (album.id.as_str(), index))
                .collect::<HashMap<_, _>>();
            order
                .iter()
                .filter_map(|id| by_id.get(id.as_str()).copied())
                .collect()
        } else {
            (0..state.library.albums.len()).collect()
        };
        let indices = Arc::new(indices);
        let count = indices.len();
        if count == 0 {
            return div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .child(if artists { "No artists" } else { "No albums" })
                .into_any_element();
        }
        uniform_list(
            if artists { "artist-rows" } else { "album-rows" },
            count.div_ceil(cards_per_row),
            cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                let state = this.model.read(cx);
                let indices = Arc::clone(&indices);
                let mut requested_cover_ids = Vec::new();
                let rows = range
                    .map(|row| {
                        let first = row * cards_per_row;
                        div()
                            .id((if artists { "artist-row" } else { "album-row" }, row))
                            .role(Role::ListItem)
                            .h(px(156.0))
                            .flex()
                            .gap_2()
                            .p_2()
                            .children((first..(first + cards_per_row).min(count)).map(|visible| {
                                let index = indices[visible];
                                let model = this.model.clone();
                                let (id, title, subtitle, cover_art_id) = if artists {
                                    let artist = &state.library.artists[index];
                                    (
                                        artist.id.clone(),
                                        artist.name.clone(),
                                        format!("{} albums", artist.album_count),
                                        artist.cover_art_id.clone(),
                                    )
                                } else {
                                    let album = &state.library.albums[index];
                                    (
                                        album.id.clone(),
                                        album.name.clone(),
                                        album.artist_name.clone().unwrap_or_default(),
                                        album.cover_art_id.clone(),
                                    )
                                };
                                let cover_art_path =
                                    cover_art_id.as_ref().and_then(|cover_art_id| {
                                        state.cover_art_paths.get(cover_art_id).cloned()
                                    });
                                if cover_art_path.is_none()
                                    && let Some(cover_art_id) = cover_art_id
                                {
                                    requested_cover_ids.push(cover_art_id);
                                }
                                card(
                                    (if artists { "artist-card" } else { "album-card" }, index),
                                    title,
                                    subtitle,
                                    cover_art_path,
                                    Rc::new(move |play, cx| {
                                        model.update(cx, |model, cx| {
                                            if artists {
                                                model.select_artist(Some(id.clone()), cx);
                                            } else {
                                                model.select_album(Some(id.clone()), cx);
                                            }
                                            if play {
                                                model.ensure_visible_song_selection(cx);
                                                model.play_selection(cx);
                                            }
                                        });
                                    }),
                                    cx,
                                )
                            }))
                    })
                    .collect();
                if !requested_cover_ids.is_empty() {
                    let model = this.model.clone();
                    cx.defer(move |cx| {
                        model.update(cx, |model, cx| {
                            for cover_art_id in requested_cover_ids {
                                model.request_cover_art(cover_art_id, cx);
                            }
                        });
                    });
                }
                rows
            }),
        )
        .h_full()
        .into_any_element()
    }

    fn render_detail(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let state = self.model.read(cx);
        let active = state.navigation.active_view;
        let album = state
            .library
            .selected_album_id
            .as_ref()
            .and_then(|id| state.library.albums.iter().find(|album| &album.id == id))
            .cloned();
        let artist = state
            .library
            .selected_artist_id
            .as_ref()
            .and_then(|id| state.library.artists.iter().find(|artist| &artist.id == id))
            .cloned();
        let artist_albums = artist
            .as_ref()
            .filter(|_| album.is_none())
            .map(|artist| {
                state
                    .library
                    .albums
                    .iter()
                    .filter(|album| album.artist_id == artist.id)
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let (title, subtitle, indices, back_artist, cover_art_id, artist_link) =
            if let Some(album) = album {
                let indices = state
                    .library
                    .songs
                    .iter()
                    .enumerate()
                    .filter(|(_, song)| song.album_id == album.id)
                    .map(|(index, _)| index)
                    .collect();
                (
                    album.name,
                    album.artist_name.unwrap_or_default(),
                    indices,
                    active == NavigationView::Artists,
                    album.cover_art_id,
                    Some(album.artist_id),
                )
            } else if let Some(artist) = artist {
                let indices = state
                    .library
                    .songs
                    .iter()
                    .enumerate()
                    .filter(|(_, song)| song.artist_id == artist.id)
                    .map(|(index, _)| index)
                    .collect();
                (
                    artist.name,
                    format!("{} albums", artist.album_count),
                    indices,
                    false,
                    artist.cover_art_id,
                    None,
                )
            } else {
                return div().into_any_element();
            };
        let back_model = self.model.clone();
        let artist_model = self.model.clone();
        let cover_model = self.model.clone();
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .p_3()
                    .flex()
                    .items_center()
                    .gap_3()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        Button::new("detail-back")
                            .label("Back")
                            .on_click(move |_, _, cx| {
                                back_model.update(cx, |model, cx| {
                                    if back_artist {
                                        model.select_album(None, cx);
                                    } else if active == NavigationView::Artists {
                                        model.select_artist(None, cx);
                                    } else {
                                        model.select_album(None, cx);
                                    }
                                });
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_2xl()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(title),
                            )
                            .child(
                                div()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(subtitle),
                            ),
                    )
                    .when_some(artist_link, |header, artist_id| {
                        header.child(Button::new("detail-artist").label("View Artist").on_click(
                            move |_, _, cx| {
                                artist_model.update(cx, |model, cx| {
                                    model.navigate(NavigationView::Artists, cx);
                                    model.select_artist(Some(artist_id.clone()), cx);
                                });
                            },
                        ))
                    })
                    .when_some(cover_art_id, |header, cover_art_id| {
                        header.child(Button::new("detail-cover").label("View Cover").on_click(
                            move |_, _, cx| {
                                cover_model.update(cx, |model, cx| {
                                    model.show_cover_art(cover_art_id.clone(), cx)
                                });
                            },
                        ))
                    }),
            )
            .when(!artist_albums.is_empty(), |detail| {
                detail.child(
                    div()
                        .id("artist-album-rail")
                        .role(Role::List)
                        .aria_label("Artist albums")
                        .h(px(86.0))
                        .p_2()
                        .flex()
                        .gap_2()
                        .overflow_x_scroll()
                        .children(artist_albums.into_iter().map(|album| {
                            let model = self.model.clone();
                            let album_id = album.id;
                            Button::new(format!("artist-album-{album_id}"))
                                .label(album.name)
                                .on_click(move |_, _, cx| {
                                    model.update(cx, |model, cx| {
                                        model.select_album(Some(album_id.clone()), cx)
                                    });
                                })
                        })),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .child(self.render_song_list(indices, "No songs", cx)),
            )
            .into_any_element()
    }

    fn render_search(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let state = self.model.read(cx);
        let query = state.library.search_query.clone();
        let pending = state.library.search_pending;
        let results = state.library.search_results.clone();
        if query.trim().is_empty() {
            return div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .child("Type to search songs, albums, and artists")
                .into_any_element();
        }
        if pending {
            return div()
                .size_full()
                .id("search-status")
                .role(Role::Status)
                .flex()
                .items_center()
                .justify_center()
                .child("Searching…")
                .into_any_element();
        }
        let Some(results) = results else {
            return div().size_full().child("No results").into_any_element();
        };
        let song_ids = results
            .songs
            .iter()
            .map(|song| song.id.as_str())
            .collect::<HashSet<_>>();
        let indices = self
            .model
            .read(cx)
            .library
            .songs
            .iter()
            .enumerate()
            .filter(|(_, song)| song_ids.contains(song.id.as_str()))
            .map(|(index, _)| index)
            .collect();
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(div().px_3().py_2().child(format!(
                "{} songs · {} albums · {} artists",
                results.songs.len(),
                results.albums.len(),
                results.artists.len()
            )))
            .child(div().flex_1().min_h_0().child(self.render_song_list(
                indices,
                "No matching songs",
                cx,
            )))
            .into_any_element()
    }

    fn render_playlists(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let state = self.model.read(cx);
        let selected = state.library.selected_playlist_id.clone();
        let playlists = state.library.playlists.clone();
        let playlist_songs = state.library.playlist_songs.clone();
        let selected_song_ids = state.selection.song_ids.clone();
        let playlist_song_ids = Arc::new(
            playlist_songs
                .iter()
                .map(|song| song.id.clone())
                .collect::<Vec<_>>(),
        );
        let offline = state.offline();
        let view = cx.entity();
        if let Some(selected) = selected {
            let playlist = playlists.iter().find(|playlist| playlist.id == selected);
            let name = playlist
                .map(|playlist| playlist.name.clone())
                .unwrap_or_else(|| "Playlist".to_string());
            let saved_offline = playlist.is_some_and(|playlist| playlist.saved_offline);
            let back_model = self.model.clone();
            let rename_view = view.clone();
            let delete_view = view.clone();
            let save_view = view.clone();
            return div()
                .size_full()
                .flex()
                .flex_col()
                .child(
                    div()
                        .p_3()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(Button::new("playlist-back").label("Back").on_click(
                            move |_, _, cx| {
                                back_model.update(cx, |model, cx| model.clear_playlist(cx));
                            },
                        ))
                        .child(div().text_2xl().child(name))
                        .child(
                            Button::new("rename-playlist")
                                .label("Rename")
                                .disabled(offline)
                                .on_click(move |_, window, cx| {
                                    rename_view.update(cx, |view, cx| {
                                        view.show_rename_playlist(window, cx)
                                    });
                                }),
                        )
                        .child(
                            Button::new("save-playlist-offline")
                                .label(if saved_offline {
                                    "Remove Offline Save"
                                } else {
                                    "Save Offline"
                                })
                                .disabled(offline && !saved_offline)
                                .on_click(move |_, _, cx| {
                                    save_view.update(cx, |view, cx| {
                                        view.toggle_selected_playlist_offline(cx)
                                    });
                                }),
                        )
                        .child(
                            Button::new("delete-playlist")
                                .label("Delete")
                                .danger()
                                .disabled(offline)
                                .on_click(move |_, window, cx| {
                                    delete_view.update(cx, |view, cx| {
                                        view.show_delete_playlist(window, cx)
                                    });
                                }),
                        ),
                )
                .child(
                    div()
                        .id("playlist-song-list")
                        .role(Role::List)
                        .aria_label("Playlist songs")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scrollbar()
                        .children(playlist_songs.into_iter().map(|song| {
                            let position = song.position.max(0) as usize;
                            let song_id = song.id;
                            let right_click_song_id = song_id.clone();
                            let visible_song_ids = Arc::clone(&playlist_song_ids);
                            let title = song.title;
                            let artist = song.artist.unwrap_or_default();
                            let album = song.album.unwrap_or_default();
                            let click_model = self.model.clone();
                            let right_click_model = self.model.clone();
                            div()
                                .id(("playlist-song", position))
                                .role(Role::ListItem)
                                .aria_label(format!("{title}, {artist}, {album}"))
                                .aria_selected(selected_song_ids.contains(&song_id))
                                .focusable()
                                .tab_stop(true)
                                .h(px(32.0))
                                .px_3()
                                .flex()
                                .items_center()
                                .gap_2()
                                .border_b_1()
                                .border_color(cx.theme().border)
                                .on_click(move |event, _, cx| {
                                    click_model.update(cx, |model, cx| {
                                        model.select_song_with_modifiers(
                                            position,
                                            song_id.clone(),
                                            &visible_song_ids,
                                            event.modifiers(),
                                            cx,
                                        );
                                        if event.click_count() >= 2 {
                                            model.play_selection(cx);
                                        }
                                    });
                                })
                                .on_mouse_down(MouseButton::Right, move |_, _, cx| {
                                    right_click_model.update(cx, |model, cx| {
                                        if !model.selection.song_ids.contains(&right_click_song_id)
                                        {
                                            model.select_song(
                                                position,
                                                right_click_song_id.clone(),
                                                cx,
                                            );
                                        }
                                    });
                                })
                                .child(format!("{}.", position + 1))
                                .child(div().flex_1().child(title))
                                .child(artist)
                                .context_menu(|menu, _, _| {
                                    menu.menu("Play", Box::new(PlaySelected))
                                        .menu("Play Next", Box::new(PlaySelectedNext))
                                        .menu("Add to Queue", Box::new(QueueSelected))
                                        .menu("Add to Playlist…", Box::new(AddSongToPlaylist))
                                        .menu("Remove from Playlist", Box::new(RemovePlaylistSong))
                                })
                        })),
                )
                .into_any_element();
        }

        let new_view = view.clone();
        let sync_model = self.model.clone();
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .p_3()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().text_2xl().child("Playlists"))
                    .child(
                        Button::new("create-playlist")
                            .label("New Playlist")
                            .disabled(offline)
                            .on_click(move |_, window, cx| {
                                new_view
                                    .update(cx, |view, cx| view.show_create_playlist(window, cx));
                            }),
                    )
                    .child(
                        Button::new("sync-playlists-list")
                            .label("Sync")
                            .disabled(offline)
                            .on_click(move |_, _, cx| {
                                sync_model.update(cx, DesktopModel::sync_playlists);
                            }),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .p_3()
                    .children(playlists.into_iter().map(|playlist| {
                        let id = playlist.id.clone();
                        let click_model = self.model.clone();
                        let right_click_model = self.model.clone();
                        div()
                            .id(format!("playlist-{id}"))
                            .role(Role::Button)
                            .aria_label(format!("{} playlist", playlist.name))
                            .focusable()
                            .tab_stop(true)
                            .p_3()
                            .mb_2()
                            .border_1()
                            .rounded_md()
                            .border_color(cx.theme().border)
                            .on_click(move |_, _, cx| {
                                click_model
                                    .update(cx, |model, cx| model.select_playlist(id.clone(), cx));
                            })
                            .on_mouse_down(MouseButton::Right, move |_, _, cx| {
                                right_click_model.update(cx, |model, cx| {
                                    model.select_playlist(playlist.id.clone(), cx)
                                });
                            })
                            .child(playlist.name)
                            .child(format!(" · {} songs", playlist.song_count))
                            .context_menu(move |menu, _, _| {
                                menu.menu("Rename", Box::new(RenamePlaylist))
                                    .menu(
                                        if playlist.saved_offline {
                                            "Remove Offline Save"
                                        } else {
                                            "Save Offline"
                                        },
                                        Box::new(TogglePlaylistOffline),
                                    )
                                    .separator()
                                    .menu("Delete", Box::new(DeletePlaylist))
                            })
                    })),
            )
            .into_any_element()
    }

    fn render_content(&self, window: &Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let state = self.model.read(cx);
        if state.library.loading && state.library.songs.is_empty() {
            return div()
                .size_full()
                .id("library-loading-status")
                .role(Role::Status)
                .flex()
                .items_center()
                .justify_center()
                .child("Loading library…")
                .into_any_element();
        }
        let active = state.navigation.active_view;
        let has_artist_detail =
            active == NavigationView::Artists && state.library.selected_artist_id.is_some();
        let has_album_detail = matches!(active, NavigationView::Artists | NavigationView::Albums)
            && state.library.selected_album_id.is_some();
        if has_artist_detail || has_album_detail {
            return self.render_detail(cx);
        }
        match active {
            NavigationView::Music => self.render_music(cx),
            NavigationView::Artists => self.render_grid(true, None, window, cx),
            NavigationView::Albums => self.render_grid(false, None, window, cx),
            NavigationView::RecentlyAdded
            | NavigationView::RecentlyPlayed
            | NavigationView::MostPlayed => {
                if self.model.read(cx).offline() {
                    div()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child("This server-only view is unavailable offline")
                        .into_any_element()
                } else if self.model.read(cx).library.discovery_loading {
                    div()
                        .id("discovery-loading-status")
                        .role(Role::Status)
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child("Loading albums…")
                        .into_any_element()
                } else {
                    let order = self
                        .model
                        .read(cx)
                        .library
                        .discovery_albums
                        .iter()
                        .map(|album| album.id.clone())
                        .collect();
                    self.render_grid(false, Some(order), window, cx)
                }
            }
            NavigationView::Playlists => self.render_playlists(cx),
            NavigationView::Search => self.render_search(cx),
        }
    }

    fn render_status(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let state = self.model.read(cx);
        let songs = state.library.songs.len();
        let albums = state.library.albums.len();
        let artists = state.library.artists.len();
        let duration = state
            .library
            .songs
            .iter()
            .filter_map(|song| song.duration)
            .map(i64::from)
            .sum::<i64>();
        let size = state
            .library
            .songs
            .iter()
            .filter_map(|song| song.size)
            .sum::<i64>();
        let error = state.library.error.clone().or(state.action_error.clone());
        let activity = if state
            .scan_status
            .as_ref()
            .is_some_and(|status| status.scanning)
        {
            Some("Scanning server…")
        } else {
            state
                .library_sync_status
                .as_ref()
                .and_then(|status| status.active_job)
                .map(|job| match job {
                    SyncJobKind::Incremental => "Syncing library…",
                    SyncJobKind::FullReconcile => "Reconciling library…",
                })
        };
        let last_sync = state.library_sync_status.as_ref().and_then(|status| {
            [
                status.incremental.last_success_at.as_deref(),
                status.full_reconcile.last_success_at.as_deref(),
            ]
            .into_iter()
            .flatten()
            .max()
            .map(|value| format_sync_timestamp(value, &state.time_preferences))
        });
        let server = state
            .auth
            .status
            .server_url
            .as_deref()
            .map(|url| {
                url.strip_prefix("https://")
                    .or_else(|| url.strip_prefix("http://"))
                    .unwrap_or(url)
                    .trim_end_matches('/')
                    .to_string()
            })
            .unwrap_or_else(|| "No server".to_string());
        let connection = if state.offline() {
            "Offline"
        } else if state.auth.status.connected {
            "Online"
        } else {
            "Disconnected"
        };
        let refresh_model = self.model.clone();
        div()
            .id("library-status")
            .role(Role::Status)
            .h(px(28.0))
            .px_3()
            .flex()
            .items_center()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(format!(
                "{songs} songs · {albums} albums · {artists} artists · {} · {}",
                format_duration(duration),
                format_size(size)
            ))
            .child(
                div()
                    .ml_auto()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(format!("{server} · {connection}"))
                    .when_some(last_sync, |status, timestamp| {
                        status.child(format!("· Last sync {timestamp}"))
                    })
                    .when_some(activity, |status, activity| {
                        status.text_color(cx.theme().primary).child(activity)
                    })
                    .child(
                        Button::new("refresh-connection")
                            .compact()
                            .label("Refresh")
                            .on_click(move |_, _, cx| {
                                refresh_model.update(cx, DesktopModel::refresh_connection_status);
                            }),
                    ),
            )
            .when_some(error, |status, error| {
                status.ml_2().text_color(cx.theme().danger).child(error)
            })
            .into_any_element()
    }
}

impl Render for LibraryView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.render_sidebar(cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    .child(self.render_toolbar(window, cx))
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .child(self.render_content(window, cx)),
                    )
                    .child(self.render_status(cx)),
            )
    }
}

fn column(
    title: &'static str,
    cx: &Context<LibraryView>,
) -> gpui_component::scroll::Scrollable<gpui::Div> {
    div()
        .flex_1()
        .min_w_0()
        .h_full()
        .flex()
        .flex_col()
        .gap_1()
        .p_2()
        .overflow_y_scrollbar()
        .border_r_1()
        .border_color(cx.theme().border)
        .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
}

type SongSelectHandler = Rc<dyn Fn(&gpui::ClickEvent, &mut gpui::App)>;
type CardSelectHandler = Rc<dyn Fn(bool, &mut gpui::App)>;

fn song_row(
    row: usize,
    song: &Song,
    selected: bool,
    model: Entity<DesktopModel>,
    on_select: SongSelectHandler,
    on_context: Rc<dyn Fn(&mut gpui::App)>,
    cx: &Context<LibraryView>,
) -> gpui::AnyElement {
    let playing = model
        .read(cx)
        .playback
        .song
        .as_ref()
        .is_some_and(|current| current.id == song.id);
    let on_click = on_select;
    let on_right_click = on_context;
    let artist_model = model.clone();
    let album_model = model;
    let artist_id = song.artist_id.clone();
    let album_id = song.album_id.clone();
    let artist = song.artist.clone().unwrap_or_default();
    let album = song.album.clone().unwrap_or_default();
    let artist_link = Button::new(("song-artist", row))
        .label(artist)
        .on_click(move |_, _, cx| {
            artist_model.update(cx, |model, cx| {
                model.navigate(NavigationView::Artists, cx);
                model.select_artist(Some(artist_id.clone()), cx);
            });
        });
    let album_link = Button::new(("song-album", row))
        .label(album)
        .on_click(move |_, _, cx| {
            album_model.update(cx, |model, cx| {
                model.navigate(NavigationView::Albums, cx);
                model.select_album(Some(album_id.clone()), cx);
            });
        });
    div()
        .id(("song", row))
        .role(Role::ListItem)
        .aria_label(format!(
            "{} by {}",
            song.title,
            song.artist.as_deref().unwrap_or("Unknown artist")
        ))
        .aria_selected(selected)
        .focusable()
        .tab_stop(true)
        .h(px(32.0))
        .px_3()
        .flex()
        .items_center()
        .gap_2()
        .border_b_1()
        .border_color(cx.theme().border)
        .when(selected, |row| row.bg(cx.theme().list_active))
        .when(playing, |row| row.text_color(cx.theme().primary))
        .focus(|row| row.border_2().border_color(cx.theme().ring))
        .on_click(move |event, _, cx| on_click(event, cx))
        .on_mouse_down(MouseButton::Right, move |_, _, cx| on_right_click(cx))
        .child(div().w(px(42.0)).child(format!(
            "{}.{}",
            song.disc_number,
            song.track_number.unwrap_or(0)
        )))
        .child(div().flex_1().min_w_0().child(song.title.clone()))
        .child(div().w(px(180.0)).child(artist_link))
        .child(div().w(px(180.0)).child(album_link))
        .child(format_duration(i64::from(
            song.duration.unwrap_or_default(),
        )))
        .context_menu(|menu, _, _| {
            menu.menu("Play", Box::new(PlaySelected))
                .menu("Play Next", Box::new(PlaySelectedNext))
                .menu("Add to Queue", Box::new(QueueSelected))
                .menu("Add to Playlist…", Box::new(AddSongToPlaylist))
        })
        .into_any_element()
}

fn card(
    id: impl Into<gpui::ElementId>,
    title: String,
    subtitle: String,
    cover_art_path: Option<std::path::PathBuf>,
    on_select: CardSelectHandler,
    cx: &Context<LibraryView>,
) -> gpui::AnyElement {
    let on_click = Rc::clone(&on_select);
    let on_right_click = on_select;
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(format!("{title}, {subtitle}"))
        .focusable()
        .tab_stop(true)
        .flex_1()
        .h_full()
        .p_3()
        .flex()
        .flex_col()
        .justify_end()
        .rounded_md()
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().secondary)
        .focus(|card| card.border_2().border_color(cx.theme().ring))
        .on_click(move |event, _, cx| on_click(event.click_count() >= 2, cx))
        .on_mouse_down(MouseButton::Right, move |_, _, cx| {
            on_right_click(false, cx)
        })
        .when_some(cover_art_path, |card, path| {
            card.child(
                img(path)
                    .w_full()
                    .flex_1()
                    .min_h_0()
                    .object_fit(ObjectFit::Cover)
                    .rounded_sm(),
            )
        })
        .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
        .child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(subtitle),
        )
        .context_menu(|menu, _, _| {
            menu.menu("Play", Box::new(PlaySelected))
                .menu("Play Next", Box::new(PlaySelectedNext))
                .menu("Add to Queue", Box::new(QueueSelected))
                .menu("Add to Playlist…", Box::new(AddSongToPlaylist))
        })
        .into_any_element()
}

fn format_sync_timestamp(
    value: &str,
    preferences: &stereodrome_desktop::operations::settings::SystemTimePreferences,
) -> String {
    let Ok(timestamp) = DateTime::parse_from_rfc3339(value) else {
        return "Unknown".to_string();
    };
    let timestamp = timestamp.with_timezone(&Local);
    let locale = preferences
        .locale
        .as_deref()
        .unwrap_or_default()
        .to_lowercase();
    let date_format = if locale.starts_with("en-us") {
        "%-m/%-d/%Y"
    } else if ["ja", "zh", "ko"]
        .iter()
        .any(|language| locale.starts_with(language))
    {
        "%Y/%m/%d"
    } else {
        "%-d/%-m/%Y"
    };
    let time_format = if preferences.use_24_hour_clock {
        "%H:%M"
    } else {
        "%-I:%M %p"
    };
    format!(
        "{} {}",
        timestamp.format(date_format),
        timestamp.format(time_format)
    )
}

fn format_duration(seconds: i64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let seconds = seconds % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

fn format_size(bytes: i64) -> String {
    const MIB: f64 = 1024.0 * 1024.0;
    const GIB: f64 = 1024.0 * MIB;
    if bytes as f64 >= GIB {
        format!("{:.1} GiB", bytes as f64 / GIB)
    } else {
        format!("{:.1} MiB", bytes as f64 / MIB)
    }
}

#[cfg(test)]
mod tests {
    use super::{format_duration, format_size, format_sync_timestamp};
    use stereodrome_desktop::operations::settings::SystemTimePreferences;

    #[test]
    fn formats_library_totals() {
        assert_eq!(format_duration(65), "1:05");
        assert_eq!(format_duration(3661), "1:01:01");
        assert_eq!(format_size(1024 * 1024), "1.0 MiB");
    }

    #[test]
    fn formats_sync_timestamp_with_system_preferences() {
        let formatted = format_sync_timestamp(
            "2026-07-16T13:25:24+00:00",
            &SystemTimePreferences {
                use_24_hour_clock: true,
                locale: Some("en-US".to_string()),
            },
        );
        assert!(formatted.contains('/'));
        assert!(formatted.contains(':'));
        assert!(!formatted.contains('T'));
        assert_eq!(
            format_sync_timestamp(
                "not-a-timestamp",
                &SystemTimePreferences {
                    use_24_hour_clock: false,
                    locale: None,
                },
            ),
            "Unknown"
        );
    }
}
