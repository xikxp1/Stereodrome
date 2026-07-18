use gpui::{
    Context, Entity, FontWeight, IntoElement, ObjectFit, Render, Role, ScrollStrategy,
    UniformListScrollHandle, Window, div, img, prelude::*, px, uniform_list,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _,
    button::{Button, ButtonVariants as _},
};
use std::sync::Arc;

use crate::ui::model::DesktopModel;

pub struct PlayerView {
    model: Entity<DesktopModel>,
    queue_scroll: UniformListScrollHandle,
}

impl PlayerView {
    pub fn new(model: Entity<DesktopModel>, cx: &mut Context<Self>) -> Self {
        cx.observe(&model, |_, _, cx| cx.notify()).detach();
        Self {
            model,
            queue_scroll: UniformListScrollHandle::new(),
        }
    }
}

impl Render for PlayerView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.model.read(cx);
        let playback = state.playback.clone();
        let queue = state.queue.clone();
        let spectrum = state.spectrum.clone();
        let queue_open = state.navigation.queue_open;
        let spectrum_open = state.spectrum_enabled;
        let has_selection = state.selection.song_id.is_some();
        let has_queue = !queue.items.is_empty();
        let has_song = playback.song.is_some();
        let cover_art_path = state.current_cover_art_path.clone();
        let cover_art_id = playback
            .song
            .as_ref()
            .and_then(|song| song.cover_art_id.clone());
        let title = playback
            .song
            .as_ref()
            .map_or_else(|| "Nothing playing".to_string(), |song| song.title.clone());
        let artist = playback
            .song
            .as_ref()
            .map_or_else(String::new, |song| song.artist.clone());

        let previous_model = self.model.clone();
        let play_model = self.model.clone();
        let next_model = self.model.clone();
        let backward_model = self.model.clone();
        let forward_model = self.model.clone();
        let mute_model = self.model.clone();
        let volume_down_model = self.model.clone();
        let volume_up_model = self.model.clone();
        let shuffle_model = self.model.clone();
        let repeat_model = self.model.clone();
        let reroll_model = self.model.clone();
        let queue_model = self.model.clone();
        let spectrum_model = self.model.clone();
        let add_model = self.model.clone();
        let next_selection_model = self.model.clone();
        let clear_model = self.model.clone();
        let cover_model = self.model.clone();
        let queue_scroll = self.queue_scroll.clone();
        let locate_scroll = self.queue_scroll.clone();
        let current_index = queue.current_index;
        let queue_items = Arc::new(queue.items.clone());

        div()
            .id("native-player")
            .flex_none()
            .flex_col()
            .border_t_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().sidebar)
            .when(queue_open, |player| {
                let queue_items = Arc::clone(&queue_items);
                player.child(
                    div()
                        .id("play-queue")
                        .role(Role::List)
                        .aria_label("Play queue")
                        .h(px(190.0))
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .h(px(32.0))
                                .px_2()
                                .flex()
                                .items_center()
                                .child("Play Queue")
                                .child(
                                    Button::new("locate-current-queue-item")
                                        .compact()
                                        .label("Locate current")
                                        .disabled(current_index.is_none())
                                        .on_click(move |_, _, _| {
                                            if let Some(index) = current_index {
                                                locate_scroll.scroll_to_item_strict(
                                                    index,
                                                    ScrollStrategy::Center,
                                                );
                                            }
                                        }),
                                ),
                        )
                        .child(
                            uniform_list(
                                "play-queue-items",
                                queue_items.len(),
                                cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                                    let queue = this.model.read(cx).queue.clone();
                                    let last = queue.items.len().saturating_sub(1);
                                    range
                                        .filter_map(|index| {
                                            let item = queue.items.get(index)?;
                                            let play_model = this.model.clone();
                                            let row_play_model = this.model.clone();
                                            let up_model = this.model.clone();
                                            let down_model = this.model.clone();
                                            let remove_model = this.model.clone();
                                            let current = queue.current_index == Some(index);
                                            Some(
                                                div()
                                                    .id(("queue-row", index))
                                                    .role(Role::ListItem)
                                                    .aria_label(format!(
                                                        "{} by {}",
                                                        item.title, item.artist
                                                    ))
                                                    .h(px(36.0))
                                                    .px_2()
                                                    .flex()
                                                    .items_center()
                                                    .gap_2()
                                                    .when(current, |row| {
                                                        row.bg(cx.theme().list_active)
                                                    })
                                                    .on_click(move |event, _, cx| {
                                                        if event.click_count() >= 2 {
                                                            row_play_model.update(
                                                                cx,
                                                                |model, cx| {
                                                                    model.play_queue_item(index, cx)
                                                                },
                                                            );
                                                        }
                                                    })
                                                    .child(
                                                        div()
                                                            .w(px(28.0))
                                                            .text_color(cx.theme().muted_foreground)
                                                            .child(format!("{}", index + 1)),
                                                    )
                                                    .child(
                                                        div()
                                                            .flex_1()
                                                            .min_w_0()
                                                            .child(item.title.clone()),
                                                    )
                                                    .child(
                                                        div()
                                                            .w(px(170.0))
                                                            .text_color(cx.theme().muted_foreground)
                                                            .child(item.artist.clone()),
                                                    )
                                                    .child(
                                                        Button::new(("queue-play", index))
                                                            .compact()
                                                            .label("Play")
                                                            .on_click(move |_, _, cx| {
                                                                play_model.update(
                                                                    cx,
                                                                    |model, cx| {
                                                                        model.play_queue_item(
                                                                            index, cx,
                                                                        )
                                                                    },
                                                                );
                                                            }),
                                                    )
                                                    .child(
                                                        Button::new(("queue-up", index))
                                                            .compact()
                                                            .label("Up")
                                                            .disabled(index == 0)
                                                            .on_click(move |_, _, cx| {
                                                                up_model.update(cx, |model, cx| {
                                                                    model.move_queue_item(
                                                                        index,
                                                                        index.saturating_sub(1),
                                                                        cx,
                                                                    )
                                                                });
                                                            }),
                                                    )
                                                    .child(
                                                        Button::new(("queue-down", index))
                                                            .compact()
                                                            .label("Down")
                                                            .disabled(index == last)
                                                            .on_click(move |_, _, cx| {
                                                                down_model.update(
                                                                    cx,
                                                                    |model, cx| {
                                                                        model.move_queue_item(
                                                                            index,
                                                                            index
                                                                                .saturating_add(1)
                                                                                .min(last),
                                                                            cx,
                                                                        )
                                                                    },
                                                                );
                                                            }),
                                                    )
                                                    .child(
                                                        Button::new(("queue-remove", index))
                                                            .compact()
                                                            .danger()
                                                            .label("Remove")
                                                            .on_click(move |_, _, cx| {
                                                                remove_model.update(
                                                                    cx,
                                                                    |model, cx| {
                                                                        model.remove_queue_item(
                                                                            index, cx,
                                                                        )
                                                                    },
                                                                );
                                                            }),
                                                    ),
                                            )
                                        })
                                        .collect()
                                }),
                            )
                            .track_scroll(&queue_scroll)
                            .flex_1()
                            .min_h_0(),
                        ),
                )
            })
            .when(spectrum_open, |player| {
                player.child(
                    div()
                        .id("spectrum")
                        .aria_label("Audio spectrum")
                        .h(px(44.0))
                        .px_3()
                        .flex()
                        .items_end()
                        .gap_1()
                        .children(spectrum.bands.iter().map(|band| {
                            div()
                                .flex_1()
                                .h(px((band.clamp(0.0, 1.0) * 40.0).max(2.0)))
                                .bg(cx.theme().primary)
                        })),
                )
            })
            .child(
                div()
                    .id("transport-controls")
                    .role(Role::Toolbar)
                    .aria_label("Playback controls")
                    .min_h(px(84.0))
                    .p_3()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .when_some(
                        cover_art_path.zip(cover_art_id),
                        |controls, (path, cover_art_id)| {
                            controls.child(
                                div()
                                    .id("now-playing-cover")
                                    .role(Role::Button)
                                    .aria_label("Open cover art")
                                    .focusable()
                                    .tab_stop(true)
                                    .size(px(60.0))
                                    .on_click(move |_, _, cx| {
                                        cover_model.update(cx, |model, cx| {
                                            model.show_cover_art(cover_art_id.clone(), cx)
                                        });
                                    })
                                    .child(img(path).size_full().object_fit(ObjectFit::Cover)),
                            )
                        },
                    )
                    .child(
                        div()
                            .w(px(260.0))
                            .min_w_0()
                            .flex_col()
                            .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
                            .child(div().text_color(cx.theme().muted_foreground).child(artist)),
                    )
                    .child(
                        Button::new("previous-track")
                            .label("Previous")
                            .disabled(!has_queue)
                            .on_click(move |_, _, cx| {
                                previous_model.update(cx, DesktopModel::play_previous);
                            }),
                    )
                    .child(
                        Button::new("toggle-playback")
                            .label(if playback.is_playing { "Pause" } else { "Play" })
                            .disabled(!has_song)
                            .on_click(move |_, _, cx| {
                                play_model.update(cx, DesktopModel::toggle_playback);
                            }),
                    )
                    .child(
                        Button::new("next-track")
                            .label("Next")
                            .disabled(!has_queue)
                            .on_click(move |_, _, cx| {
                                next_model.update(cx, DesktopModel::play_next);
                            }),
                    )
                    .child(
                        Button::new("seek-backward")
                            .compact()
                            .label("-10s")
                            .disabled(!has_song)
                            .on_click(move |_, _, cx| {
                                backward_model.update(cx, |model, cx| model.seek_by(-10.0, cx));
                            }),
                    )
                    .child(div().w(px(112.0)).text_center().child(format!(
                        "{} / {}",
                        format_duration(playback.position),
                        format_duration(playback.duration)
                    )))
                    .child(
                        Button::new("seek-forward")
                            .compact()
                            .label("+10s")
                            .disabled(!has_song)
                            .on_click(move |_, _, cx| {
                                forward_model.update(cx, |model, cx| model.seek_by(10.0, cx));
                            }),
                    )
                    .child(
                        Button::new("mute")
                            .compact()
                            .label(if playback.volume == 0.0 {
                                "Unmute"
                            } else {
                                "Mute"
                            })
                            .on_click(move |_, _, cx| {
                                mute_model.update(cx, DesktopModel::toggle_mute);
                            }),
                    )
                    .child(
                        Button::new("volume-down")
                            .compact()
                            .label("Vol -")
                            .on_click(move |_, _, cx| {
                                volume_down_model
                                    .update(cx, |model, cx| model.adjust_volume(-0.05, cx));
                            }),
                    )
                    .child(format!("{:.0}%", playback.volume * 100.0))
                    .child(Button::new("volume-up").compact().label("Vol +").on_click(
                        move |_, _, cx| {
                            volume_up_model.update(cx, |model, cx| model.adjust_volume(0.05, cx));
                        },
                    ))
                    .child(
                        Button::new("shuffle")
                            .compact()
                            .label(if queue.shuffle {
                                "Shuffle on"
                            } else {
                                "Shuffle"
                            })
                            .disabled(!has_queue)
                            .on_click(move |_, _, cx| {
                                shuffle_model.update(cx, DesktopModel::toggle_shuffle);
                            }),
                    )
                    .child(
                        Button::new("repeat")
                            .compact()
                            .label(format!("Repeat {:?}", queue.repeat_mode))
                            .disabled(!has_queue)
                            .on_click(move |_, _, cx| {
                                repeat_model.update(cx, DesktopModel::cycle_repeat);
                            }),
                    )
                    .child(
                        Button::new("reroll")
                            .compact()
                            .label("Reroll")
                            .disabled(!queue.shuffle || queue.items.len() < 2)
                            .on_click(move |_, _, cx| {
                                reroll_model.update(cx, DesktopModel::reroll_next);
                            }),
                    )
                    .child(
                        Button::new("toggle-queue")
                            .compact()
                            .label(format!("Queue ({})", queue.items.len()))
                            .on_click(move |_, _, cx| {
                                queue_model.update(cx, DesktopModel::toggle_queue);
                            }),
                    )
                    .child(
                        Button::new("toggle-spectrum")
                            .compact()
                            .label("Spectrum")
                            .on_click(move |_, _, cx| {
                                spectrum_model.update(cx, DesktopModel::toggle_spectrum);
                            }),
                    )
                    .child(
                        Button::new("add-selected-queue")
                            .compact()
                            .label("Add selected")
                            .disabled(!has_selection)
                            .on_click(move |_, _, cx| {
                                add_model.update(cx, |model, cx| {
                                    model.add_selection_to_queue(false, cx)
                                });
                            }),
                    )
                    .child(
                        Button::new("play-next-selected")
                            .compact()
                            .label("Play next")
                            .disabled(!has_selection)
                            .on_click(move |_, _, cx| {
                                next_selection_model
                                    .update(cx, |model, cx| model.add_selection_to_queue(true, cx));
                            }),
                    )
                    .child(
                        Button::new("clear-queue")
                            .compact()
                            .danger()
                            .label("Clear")
                            .disabled(!has_queue)
                            .on_click(move |_, _, cx| {
                                clear_model.update(cx, DesktopModel::clear_queue);
                            }),
                    ),
            )
    }
}

fn format_duration(seconds: f64) -> String {
    let seconds = seconds.max(0.0) as u64;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

#[cfg(test)]
mod tests {
    use super::format_duration;

    #[test]
    fn formats_transport_duration() {
        assert_eq!(format_duration(-1.0), "0:00");
        assert_eq!(format_duration(65.9), "1:05");
    }
}
