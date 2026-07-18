use gpui::{
    Context, Entity, FocusHandle, FontWeight, InteractiveElement, IntoElement, ParentElement,
    PathPromptOptions, PromptLevel, Render, Role, StatefulInteractiveElement, Styled, Window, div,
    prelude::FluentBuilder, px,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _,
    button::{Button, ButtonVariants as _},
};
use stereodrome_audio::{binaural::BinauralPreset, compressor::DynamicsPreset};
use stereodrome_desktop::operations::settings::NormalizationMode;

use crate::ui::{
    actions::{CheckForUpdates, InstallUpdate, OpenMiniPlayer, OpenNanoPlayer},
    model::DesktopModel,
};

const GIB: u64 = 1024 * 1024 * 1024;
const CACHE_SIZE_PRESETS: [u64; 7] = [GIB / 2, GIB, 2 * GIB, 5 * GIB, 10 * GIB, 20 * GIB, 50 * GIB];

pub struct SettingsView {
    model: Entity<DesktopModel>,
    focus: FocusHandle,
}

impl SettingsView {
    pub fn new(model: Entity<DesktopModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        cx.observe(&model, |_, _, cx| cx.notify()).detach();
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        Self { model, focus }
    }
}

impl Render for SettingsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.model.read(cx);
        let playback = state.playback_settings.clone();
        let sync = state.sync_settings.clone();
        let notifications = state.notification_settings.clone();
        let normalization = state.normalization_settings.clone();
        let lastfm = state.lastfm_status.clone();
        let connection = state.auth.status.clone();
        let scan_status = state.scan_status.clone();
        let sync_status = state.library_sync_status.clone();
        let incremental_running = sync_status
            .as_ref()
            .is_some_and(|status| status.incremental.running);
        let full_running = sync_status
            .as_ref()
            .is_some_and(|status| status.full_reconcile.running);
        let offline = state.connectivity.manual_offline_enabled;
        let updater = state.updater.clone();
        let error = state.action_error.clone();
        let cache = state.cache_summary();
        let cache_max = cache.as_ref().ok().map(|(_, _, _, maximum)| *maximum);
        let cache_available = cache.is_ok();

        let offline_model = self.model.clone();
        let scan_model = self.model.clone();
        let disconnect_model = self.model.clone();
        let incremental_now_model = self.model.clone();
        let full_now_model = self.model.clone();
        let incremental_model = self.model.clone();
        let incremental_interval_model = self.model.clone();
        let full_model = self.model.clone();
        let full_interval_model = self.model.clone();
        let display_model = self.model.clone();
        let notification_model = self.model.clone();
        let focus_notification_model = self.model.clone();
        let mini_notification_model = self.model.clone();
        let lastfm_model = self.model.clone();
        let lastfm_complete_model = self.model.clone();
        let lastfm_retry_model = self.model.clone();
        let lastfm_disconnect_model = self.model.clone();
        let gapless_model = self.model.clone();
        let crossfade_model = self.model.clone();
        let crossfade_duration_model = self.model.clone();
        let manual_crossfade_model = self.model.clone();
        let binaural_model = self.model.clone();
        let binaural_preset_model = self.model.clone();
        let equalizer_model = self.model.clone();
        let normalization_model = self.model.clone();
        let normalization_mode_model = self.model.clone();
        let target_model = self.model.clone();
        let preamp_down_model = self.model.clone();
        let preamp_up_model = self.model.clone();
        let clipping_model = self.model.clone();
        let dynamics_model = self.model.clone();
        let dynamics_preset_model = self.model.clone();
        let analyze_model = self.model.clone();
        let clear_normalization_model = self.model.clone();
        let choose_cache_model = self.model.clone();
        let default_cache_model = self.model.clone();
        let clear_cache_model = self.model.clone();
        let cache_size_model = self.model.clone();
        let refresh_cache_model = self.model.clone();

        div()
            .id("settings-root")
            .role(Role::Application)
            .aria_label("Stereodrome settings")
            .key_context("Stereodrome")
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                div()
                    .h(px(52.0))
                    .flex_none()
                    .px_4()
                    .flex()
                    .items_center()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Settings"),
            )
            .child(
                div()
                    .id("settings-sections")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_4()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .when_some(error, |content, error| {
                        content.child(
                            div()
                                .id("settings-error")
                                .role(Role::Alert)
                                .p_3()
                                .border_1()
                                .border_color(cx.theme().danger)
                                .text_color(cx.theme().danger)
                                .child(error),
                        )
                    })
                    .child(
                        section("Updates", cx)
                            .child(setting_row("Current version", env!("CARGO_PKG_VERSION")))
                            .child(setting_row("Status", updater.status))
                            .when_some(updater.version.clone(), |section, version| {
                                section.child(setting_row("Available version", version))
                            })
                            .when_some(updater.notes.clone(), |section, notes| {
                                section
                                    .child(div().id("update-notes").role(Role::Note).child(notes))
                            })
                            .child(
                                div()
                                    .flex()
                                    .flex_wrap()
                                    .gap_2()
                                    .child(
                                        Button::new("check-for-updates")
                                            .label(if updater.busy {
                                                "Checking..."
                                            } else {
                                                "Check for updates"
                                            })
                                            .disabled(updater.busy || offline)
                                            .on_click(|_, _, cx| {
                                                cx.dispatch_action(&CheckForUpdates)
                                            }),
                                    )
                                    .when(updater.version.is_some(), |buttons| {
                                        buttons.child(
                                            Button::new("install-update")
                                                .label("Install and relaunch")
                                                .disabled(updater.busy)
                                                .on_click(|_, _, cx| {
                                                    cx.dispatch_action(&InstallUpdate)
                                                }),
                                        )
                                    }),
                            )
                            .child("Updates are installed only after signature verification."),
                    )
                    .child(
                        section("Server", cx)
                            .child(setting_row(
                                "Connection",
                                if offline {
                                    "Offline"
                                } else if connection.connected {
                                    "Connected"
                                } else {
                                    "Disconnected"
                                },
                            ))
                            .child(setting_row(
                                "URL",
                                connection
                                    .server_url
                                    .clone()
                                    .unwrap_or_else(|| "Not connected".to_string()),
                            ))
                            .child(setting_row(
                                "Username",
                                connection
                                    .username
                                    .clone()
                                    .unwrap_or_else(|| "-".to_string()),
                            ))
                            .child(setting_row(
                                "Server version",
                                connection
                                    .server_version
                                    .clone()
                                    .unwrap_or_else(|| "-".to_string()),
                            ))
                            .child(setting_row(
                                "Scan status",
                                scan_status
                                    .as_ref()
                                    .map(|status| match (status.scanning, status.count) {
                                        (true, Some(count)) => {
                                            format!("Scanning ({count} items)")
                                        }
                                        (true, None) => "Scanning".to_string(),
                                        (false, Some(count)) => format!("Idle ({count} items)"),
                                        (false, None) => "Idle".to_string(),
                                    })
                                    .unwrap_or_else(|| "-".to_string()),
                            ))
                            .child(
                                Button::new("toggle-offline")
                                    .label(if offline { "Go online" } else { "Work offline" })
                                    .on_click(move |_, _, cx| {
                                        offline_model.update(cx, |model, cx| {
                                            model.set_manual_offline(!offline, cx)
                                        });
                                    }),
                            )
                            .child(
                                div()
                                    .flex()
                                    .gap_2()
                                    .child(
                                        Button::new("start-server-scan")
                                            .label("Start scan")
                                            .disabled(
                                                offline
                                                    || !connection.connected
                                                    || scan_status
                                                        .as_ref()
                                                        .is_some_and(|status| status.scanning),
                                            )
                                            .on_click(move |_, _, cx| {
                                                scan_model.update(cx, DesktopModel::start_scan);
                                            }),
                                    )
                                    .child(
                                        Button::new("disconnect-server")
                                            .danger()
                                            .label("Disconnect")
                                            .disabled(!connection.connected)
                                            .on_click(move |_, window, cx| {
                                                let answer = window.prompt(
                                                    PromptLevel::Warning,
                                                    "Disconnect from the server?",
                                                    Some(
                                                        "Saved server credentials will be removed.",
                                                    ),
                                                    &["Disconnect", "Cancel"],
                                                    cx,
                                                );
                                                let model = disconnect_model.clone();
                                                cx.spawn(async move |cx| {
                                                    if answer.await == Ok(0) {
                                                        model.update(cx, DesktopModel::disconnect);
                                                    }
                                                })
                                                .detach();
                                            }),
                                    ),
                            ),
                    )
                    .child(
                        section("Library Sync", cx)
                            .child(
                                Button::new("toggle-incremental-sync")
                                    .label(toggle_label(
                                        "Incremental sync",
                                        sync.incremental_enabled,
                                    ))
                                    .disabled(offline)
                                    .on_click({
                                        let settings = sync.clone();
                                        move |_, _, cx| {
                                            let mut settings = settings.clone();
                                            settings.incremental_enabled =
                                                !settings.incremental_enabled;
                                            incremental_model.update(cx, |model, cx| {
                                                model.save_sync_settings(settings.clone(), cx)
                                            });
                                        }
                                    }),
                            )
                            .child(
                                Button::new("cycle-incremental-interval")
                                    .label(format!(
                                        "Incremental interval: {} min",
                                        sync.incremental_interval_minutes
                                    ))
                                    .disabled(offline || !sync.incremental_enabled)
                                    .on_click({
                                        let settings = sync.clone();
                                        move |_, _, cx| {
                                            let mut settings = settings.clone();
                                            settings.incremental_interval_minutes = next_value(
                                                settings.incremental_interval_minutes,
                                                &[5, 15, 30, 60, 180, 360, 720],
                                            );
                                            incremental_interval_model.update(cx, |model, cx| {
                                                model.save_sync_settings(settings.clone(), cx)
                                            });
                                        }
                                    }),
                            )
                            .child(
                                Button::new("toggle-full-sync")
                                    .label(toggle_label(
                                        "Full reconciliation",
                                        sync.full_reconcile_enabled,
                                    ))
                                    .disabled(offline)
                                    .on_click({
                                        let settings = sync.clone();
                                        move |_, _, cx| {
                                            let mut settings = settings.clone();
                                            settings.full_reconcile_enabled =
                                                !settings.full_reconcile_enabled;
                                            full_model.update(cx, |model, cx| {
                                                model.save_sync_settings(settings.clone(), cx)
                                            });
                                        }
                                    }),
                            )
                            .child(
                                Button::new("cycle-full-interval")
                                    .label(format!(
                                        "Full interval: {} h",
                                        sync.full_reconcile_interval_hours
                                    ))
                                    .disabled(offline || !sync.full_reconcile_enabled)
                                    .on_click({
                                        let settings = sync.clone();
                                        move |_, _, cx| {
                                            let mut settings = settings.clone();
                                            settings.full_reconcile_interval_hours = next_value(
                                                settings.full_reconcile_interval_hours,
                                                &[1, 6, 12, 24, 48, 72, 168],
                                            );
                                            full_interval_model.update(cx, |model, cx| {
                                                model.save_sync_settings(settings.clone(), cx)
                                            });
                                        }
                                    }),
                            )
                            .when_some(sync_status.clone(), |section, status| {
                                section
                                    .child(setting_row(
                                        "Incremental status",
                                        if status.incremental.running {
                                            "Running"
                                        } else {
                                            "Idle"
                                        },
                                    ))
                                    .child(setting_row(
                                        "Incremental last success",
                                        status
                                            .incremental
                                            .last_success_at
                                            .unwrap_or_else(|| "-".to_string()),
                                    ))
                                    .child(setting_row(
                                        "Incremental next run",
                                        status
                                            .incremental
                                            .next_run_at
                                            .unwrap_or_else(|| "-".to_string()),
                                    ))
                                    .child(setting_row(
                                        "Full status",
                                        if status.full_reconcile.running {
                                            "Running"
                                        } else {
                                            "Idle"
                                        },
                                    ))
                                    .child(setting_row(
                                        "Full last success",
                                        status
                                            .full_reconcile
                                            .last_success_at
                                            .unwrap_or_else(|| "-".to_string()),
                                    ))
                                    .child(setting_row(
                                        "Full next run",
                                        status
                                            .full_reconcile
                                            .next_run_at
                                            .unwrap_or_else(|| "-".to_string()),
                                    ))
                            })
                            .child(
                                div()
                                    .flex()
                                    .gap_2()
                                    .child(
                                        Button::new("sync-now")
                                            .label("Sync now")
                                            .disabled(
                                                offline
                                                    || !connection.connected
                                                    || incremental_running,
                                            )
                                            .on_click(move |_, _, cx| {
                                                incremental_now_model.update(cx, |model, cx| {
                                                    model.sync_library(false, cx)
                                                });
                                            }),
                                    )
                                    .child(
                                        Button::new("reconcile-now")
                                            .label("Full reconcile now")
                                            .disabled(
                                                offline || !connection.connected || full_running,
                                            )
                                            .on_click(move |_, _, cx| {
                                                full_now_model.update(cx, |model, cx| {
                                                    model.sync_library(true, cx)
                                                });
                                            }),
                                    ),
                            ),
                    )
                    .child(
                        section("Display", cx)
                            .child(
                                Button::new("show-next-mini")
                                    .label(toggle_label(
                                        "Show next song in mini player",
                                        playback.show_next_song_in_miniplayer,
                                    ))
                                    .on_click({
                                        let settings = playback.clone();
                                        move |_, _, cx| {
                                            let mut settings = settings.clone();
                                            settings.show_next_song_in_miniplayer =
                                                !settings.show_next_song_in_miniplayer;
                                            display_model.update(cx, |model, cx| {
                                                model.save_playback_settings(settings.clone(), cx)
                                            });
                                        }
                                    }),
                            )
                            .child(
                                div()
                                    .flex()
                                    .gap_2()
                                    .child(
                                        Button::new("open-mini-settings")
                                            .label("Open mini player")
                                            .on_click(|_, _, cx| {
                                                cx.dispatch_action(&OpenMiniPlayer)
                                            }),
                                    )
                                    .child(
                                        Button::new("open-nano-settings")
                                            .label("Open nano player")
                                            .on_click(|_, _, cx| {
                                                cx.dispatch_action(&OpenNanoPlayer)
                                            }),
                                    ),
                            ),
                    )
                    .child(
                        section("Desktop Notifications", cx)
                            .child(
                                Button::new("notifications-enabled")
                                    .label(toggle_label("Enable", notifications.enabled))
                                    .on_click({
                                        let settings = notifications.clone();
                                        move |_, _, cx| {
                                            let mut settings = settings.clone();
                                            settings.enabled = !settings.enabled;
                                            notification_model.update(cx, |model, cx| {
                                                model.save_notification_settings(
                                                    settings.clone(),
                                                    cx,
                                                )
                                            });
                                        }
                                    }),
                            )
                            .child(
                                Button::new("notifications-focused")
                                    .label(toggle_label(
                                        "Notify while focused",
                                        notifications.notify_when_focused,
                                    ))
                                    .disabled(!notifications.enabled)
                                    .on_click({
                                        let settings = notifications.clone();
                                        move |_, _, cx| {
                                            let mut settings = settings.clone();
                                            settings.notify_when_focused =
                                                !settings.notify_when_focused;
                                            focus_notification_model.update(cx, |model, cx| {
                                                model.save_notification_settings(
                                                    settings.clone(),
                                                    cx,
                                                )
                                            });
                                        }
                                    }),
                            )
                            .child(
                                Button::new("notifications-mini")
                                    .label(toggle_label(
                                        "Notify with mini player open",
                                        notifications.notify_when_miniplayer_open,
                                    ))
                                    .disabled(!notifications.enabled)
                                    .on_click({
                                        let settings = notifications.clone();
                                        move |_, _, cx| {
                                            let mut settings = settings.clone();
                                            settings.notify_when_miniplayer_open =
                                                !settings.notify_when_miniplayer_open;
                                            mini_notification_model.update(cx, |model, cx| {
                                                model.save_notification_settings(
                                                    settings.clone(),
                                                    cx,
                                                )
                                            });
                                        }
                                    }),
                            ),
                    )
                    .child(
                        section("Last.fm", cx)
                            .child(setting_row(
                                "Status",
                                if lastfm.authenticated {
                                    "Connected"
                                } else if lastfm.pending_auth {
                                    "Authorization pending"
                                } else if lastfm.available {
                                    "Available"
                                } else {
                                    "Unavailable"
                                },
                            ))
                            .child(setting_row(
                                "Queued scrobbles",
                                lastfm.queue_count.to_string(),
                            ))
                            .when_some(lastfm.username.clone(), |section, username| {
                                section.child(setting_row("Account", username))
                            })
                            .when_some(lastfm.last_error.clone(), |section, error| {
                                section.child(div().text_color(cx.theme().danger).child(error))
                            })
                            .child(
                                div()
                                    .flex()
                                    .flex_wrap()
                                    .gap_2()
                                    .child(
                                        Button::new("lastfm-authorize")
                                            .label("Authorize in browser")
                                            .disabled(!lastfm.available || lastfm.authenticated)
                                            .on_click(move |_, _, cx| {
                                                lastfm_model
                                                    .update(cx, DesktopModel::begin_lastfm_auth);
                                            }),
                                    )
                                    .child(
                                        Button::new("lastfm-complete")
                                            .label("Complete authorization")
                                            .disabled(!lastfm.pending_auth)
                                            .on_click(move |_, _, cx| {
                                                lastfm_complete_model
                                                    .update(cx, DesktopModel::complete_lastfm_auth);
                                            }),
                                    )
                                    .child(
                                        Button::new("lastfm-retry")
                                            .label("Retry queued scrobbles")
                                            .disabled(
                                                !lastfm.authenticated || lastfm.queue_count == 0,
                                            )
                                            .on_click(move |_, _, cx| {
                                                lastfm_retry_model
                                                    .update(cx, DesktopModel::retry_lastfm_queue);
                                            }),
                                    )
                                    .child(
                                        Button::new("lastfm-disconnect")
                                            .danger()
                                            .label("Disconnect")
                                            .disabled(!lastfm.authenticated)
                                            .on_click(move |_, window, cx| {
                                                let answer = window.prompt(
                                                    PromptLevel::Warning,
                                                    "Disconnect Last.fm?",
                                                    Some(
                                                        "The saved Last.fm session will be removed.",
                                                    ),
                                                    &["Disconnect", "Cancel"],
                                                    cx,
                                                );
                                                let model = lastfm_disconnect_model.clone();
                                                cx.spawn(async move |cx| {
                                                    if answer.await == Ok(0) {
                                                        model.update(
                                                            cx,
                                                            DesktopModel::disconnect_lastfm,
                                                        );
                                                    }
                                                })
                                                .detach();
                                            }),
                                    ),
                            ),
                    )
                    .child(
                        section("Playback", cx)
                            .child(
                                Button::new("gapless")
                                    .label(toggle_label(
                                        "Gapless playback",
                                        playback.gapless_enabled,
                                    ))
                                    .on_click({
                                        let settings = playback.clone();
                                        move |_, _, cx| {
                                            let mut settings = settings.clone();
                                            settings.gapless_enabled = !settings.gapless_enabled;
                                            gapless_model.update(cx, |model, cx| {
                                                model.save_playback_settings(settings.clone(), cx)
                                            });
                                        }
                                    }),
                            )
                            .child(
                                Button::new("crossfade")
                                    .label(toggle_label("Crossfade", playback.crossfade_enabled))
                                    .on_click({
                                        let settings = playback.clone();
                                        move |_, _, cx| {
                                            let mut settings = settings.clone();
                                            settings.crossfade_enabled =
                                                !settings.crossfade_enabled;
                                            crossfade_model.update(cx, |model, cx| {
                                                model.save_playback_settings(settings.clone(), cx)
                                            });
                                        }
                                    }),
                            )
                            .child(
                                Button::new("crossfade-duration")
                                    .label(format!(
                                        "Crossfade duration: {} s",
                                        playback.crossfade_duration_ms / 1000
                                    ))
                                    .disabled(!playback.crossfade_enabled)
                                    .on_click({
                                        let settings = playback.clone();
                                        move |_, _, cx| {
                                            let mut settings = settings.clone();
                                            settings.crossfade_duration_ms = next_value(
                                                settings.crossfade_duration_ms,
                                                &[1_000, 3_000, 5_000, 8_000, 12_000],
                                            );
                                            crossfade_duration_model.update(cx, |model, cx| {
                                                model.save_playback_settings(settings.clone(), cx)
                                            });
                                        }
                                    }),
                            )
                            .child(
                                Button::new("manual-crossfade")
                                    .label(toggle_label(
                                        "Crossfade on manual advance",
                                        playback.crossfade_on_manual_queue_advance,
                                    ))
                                    .disabled(!playback.crossfade_enabled)
                                    .on_click({
                                        let settings = playback.clone();
                                        move |_, _, cx| {
                                            let mut settings = settings.clone();
                                            settings.crossfade_on_manual_queue_advance =
                                                !settings.crossfade_on_manual_queue_advance;
                                            manual_crossfade_model.update(cx, |model, cx| {
                                                model.save_playback_settings(settings.clone(), cx)
                                            });
                                        }
                                    }),
                            )
                            .child(
                                Button::new("binaural")
                                    .label(toggle_label(
                                        "Binaural audio",
                                        playback.binaural_enabled,
                                    ))
                                    .on_click({
                                        let settings = playback.clone();
                                        move |_, _, cx| {
                                            let mut settings = settings.clone();
                                            settings.binaural_enabled = !settings.binaural_enabled;
                                            binaural_model.update(cx, |model, cx| {
                                                model.save_playback_settings(settings.clone(), cx)
                                            });
                                        }
                                    }),
                            )
                            .child(
                                Button::new("binaural-preset")
                                    .label(format!(
                                        "Binaural preset: {:?}",
                                        playback.binaural_preset
                                    ))
                                    .disabled(!playback.binaural_enabled)
                                    .on_click({
                                        let settings = playback.clone();
                                        move |_, _, cx| {
                                            let mut settings = settings.clone();
                                            settings.binaural_preset =
                                                match settings.binaural_preset {
                                                    BinauralPreset::Default => BinauralPreset::Cmoy,
                                                    BinauralPreset::Cmoy => BinauralPreset::Jmeier,
                                                    BinauralPreset::Jmeier => {
                                                        BinauralPreset::Aggressive
                                                    }
                                                    BinauralPreset::Aggressive => {
                                                        BinauralPreset::Default
                                                    }
                                                };
                                            binaural_preset_model.update(cx, |model, cx| {
                                                model.save_playback_settings(settings.clone(), cx)
                                            });
                                        }
                                    }),
                            )
                            .child(
                                Button::new("equalizer")
                                    .label(toggle_label("Equalizer", playback.equalizer_enabled))
                                    .on_click({
                                        let settings = playback.clone();
                                        move |_, _, cx| {
                                            let mut settings = settings.clone();
                                            settings.equalizer_enabled =
                                                !settings.equalizer_enabled;
                                            equalizer_model.update(cx, |model, cx| {
                                                model.save_playback_settings(settings.clone(), cx)
                                            });
                                        }
                                    }),
                            )
                            .children(playback.equalizer_bands_db.iter().enumerate().map(
                                |(index, value)| {
                                    let down_model = self.model.clone();
                                    let up_model = self.model.clone();
                                    let down_settings = playback.clone();
                                    let up_settings = playback.clone();
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_2()
                                        .child(format!("Band {}: {value:.1} dB", index + 1))
                                        .child(
                                            Button::new(("eq-down", index))
                                                .compact()
                                                .label("-")
                                                .disabled(!playback.equalizer_enabled)
                                                .on_click(move |_, _, cx| {
                                                    let mut settings = down_settings.clone();
                                                    if let Some(value) =
                                                        settings.equalizer_bands_db.get_mut(index)
                                                    {
                                                        *value -= 1.0;
                                                    }
                                                    down_model.update(cx, |model, cx| {
                                                        model.save_playback_settings(settings, cx)
                                                    });
                                                }),
                                        )
                                        .child(
                                            Button::new(("eq-up", index))
                                                .compact()
                                                .label("+")
                                                .disabled(!playback.equalizer_enabled)
                                                .on_click(move |_, _, cx| {
                                                    let mut settings = up_settings.clone();
                                                    if let Some(value) =
                                                        settings.equalizer_bands_db.get_mut(index)
                                                    {
                                                        *value += 1.0;
                                                    }
                                                    up_model.update(cx, |model, cx| {
                                                        model.save_playback_settings(settings, cx)
                                                    });
                                                }),
                                        )
                                },
                            )),
                    )
                    .child(
                        section("Volume Normalization", cx)
                            .child(
                                Button::new("normalization")
                                    .label(toggle_label("Enable", normalization.enabled))
                                    .on_click({
                                        let settings = normalization.clone();
                                        move |_, _, cx| {
                                            let mut settings = settings.clone();
                                            settings.enabled = !settings.enabled;
                                            normalization_model.update(cx, |model, cx| {
                                                model.save_normalization_settings(
                                                    settings.clone(),
                                                    cx,
                                                )
                                            });
                                        }
                                    }),
                            )
                            .child(
                                Button::new("normalization-mode")
                                    .label(format!("Mode: {:?}", normalization.mode))
                                    .disabled(!normalization.enabled)
                                    .on_click({
                                        let settings = normalization.clone();
                                        move |_, _, cx| {
                                            let mut settings = settings.clone();
                                            settings.mode = match settings.mode {
                                                NormalizationMode::Track => {
                                                    NormalizationMode::Album
                                                }
                                                NormalizationMode::Album => {
                                                    NormalizationMode::Track
                                                }
                                            };
                                            normalization_mode_model.update(cx, |model, cx| {
                                                model.save_normalization_settings(
                                                    settings.clone(),
                                                    cx,
                                                )
                                            });
                                        }
                                    }),
                            )
                            .child(
                                Button::new("normalization-target")
                                    .label(format!(
                                        "Target loudness: {} LUFS",
                                        normalization.target_lufs
                                    ))
                                    .disabled(!normalization.enabled)
                                    .on_click({
                                        let settings = normalization.clone();
                                        move |_, _, cx| {
                                            let mut settings = settings.clone();
                                            settings.target_lufs = next_f64(
                                                settings.target_lufs,
                                                &[-18.0, -16.0, -14.0, -12.0, -10.0],
                                            );
                                            target_model.update(cx, |model, cx| {
                                                model.save_normalization_settings(
                                                    settings.clone(),
                                                    cx,
                                                )
                                            });
                                        }
                                    }),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(format!("Preamp: {:+.1} dB", normalization.pre_amp_db))
                                    .child(
                                        Button::new("preamp-down")
                                            .compact()
                                            .label("-")
                                            .disabled(!normalization.enabled)
                                            .on_click({
                                                let settings = normalization.clone();
                                                move |_, _, cx| {
                                                    let mut settings = settings.clone();
                                                    settings.pre_amp_db -= 0.5;
                                                    preamp_down_model.update(cx, |model, cx| {
                                                        model.save_normalization_settings(
                                                            settings.clone(),
                                                            cx,
                                                        )
                                                    });
                                                }
                                            }),
                                    )
                                    .child(
                                        Button::new("preamp-up")
                                            .compact()
                                            .label("+")
                                            .disabled(!normalization.enabled)
                                            .on_click({
                                                let settings = normalization.clone();
                                                move |_, _, cx| {
                                                    let mut settings = settings.clone();
                                                    settings.pre_amp_db += 0.5;
                                                    preamp_up_model.update(cx, |model, cx| {
                                                        model.save_normalization_settings(
                                                            settings.clone(),
                                                            cx,
                                                        )
                                                    });
                                                }
                                            }),
                                    ),
                            )
                            .child(
                                Button::new("prevent-clipping")
                                    .label(toggle_label(
                                        "Prevent clipping",
                                        normalization.prevent_clipping,
                                    ))
                                    .disabled(!normalization.enabled)
                                    .on_click({
                                        let settings = normalization.clone();
                                        move |_, _, cx| {
                                            let mut settings = settings.clone();
                                            settings.prevent_clipping = !settings.prevent_clipping;
                                            clipping_model.update(cx, |model, cx| {
                                                model.save_normalization_settings(
                                                    settings.clone(),
                                                    cx,
                                                )
                                            });
                                        }
                                    }),
                            )
                            .child(
                                Button::new("dynamics")
                                    .label(toggle_label(
                                        "Dynamics processing",
                                        normalization.dynamics_enabled,
                                    ))
                                    .disabled(!normalization.enabled)
                                    .on_click({
                                        let settings = normalization.clone();
                                        move |_, _, cx| {
                                            let mut settings = settings.clone();
                                            settings.dynamics_enabled = !settings.dynamics_enabled;
                                            dynamics_model.update(cx, |model, cx| {
                                                model.save_normalization_settings(
                                                    settings.clone(),
                                                    cx,
                                                )
                                            });
                                        }
                                    }),
                            )
                            .child(
                                Button::new("dynamics-preset")
                                    .label(format!(
                                        "Dynamics amount: {:?}",
                                        normalization.dynamics_preset
                                    ))
                                    .disabled(
                                        !normalization.enabled || !normalization.dynamics_enabled,
                                    )
                                    .on_click({
                                        let settings = normalization.clone();
                                        move |_, _, cx| {
                                            let mut settings = settings.clone();
                                            settings.dynamics_preset =
                                                match settings.dynamics_preset {
                                                    DynamicsPreset::Light => DynamicsPreset::Medium,
                                                    DynamicsPreset::Medium => DynamicsPreset::Heavy,
                                                    DynamicsPreset::Heavy => DynamicsPreset::Light,
                                                };
                                            dynamics_preset_model.update(cx, |model, cx| {
                                                model.save_normalization_settings(
                                                    settings.clone(),
                                                    cx,
                                                )
                                            });
                                        }
                                    }),
                            )
                            .child(
                                div()
                                    .flex()
                                    .gap_2()
                                    .child(
                                        Button::new("analyze-loudness")
                                            .label("Analyze library")
                                            .on_click(move |_, _, cx| {
                                                analyze_model.update(
                                                    cx,
                                                    DesktopModel::analyze_library_loudness,
                                                );
                                            }),
                                    )
                                    .child(
                                        Button::new("clear-loudness")
                                            .danger()
                                            .label("Clear analysis")
                                            .on_click(move |_, window, cx| {
                                                let answer = window.prompt(
                                                    PromptLevel::Warning,
                                                    "Clear all loudness analysis?",
                                                    Some(
                                                        "ReplayGain and loudness values will be removed from every song.",
                                                    ),
                                                    &["Clear analysis", "Cancel"],
                                                    cx,
                                                );
                                                let model = clear_normalization_model.clone();
                                                cx.spawn(async move |cx| {
                                                    if answer.await == Ok(0) {
                                                        model.update(
                                                            cx,
                                                            DesktopModel::clear_normalization_data,
                                                        );
                                                    }
                                                })
                                                .detach();
                                            }),
                                    ),
                            ),
                    )
                    .child(
                        section("Audio Cache", cx)
                            .child(match cache {
                                Ok((path, files, size, maximum)) => div()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(format!("Location: {path}"))
                                    .child(format!(
                                        "{files} files · {} / {}",
                                        format_bytes(size),
                                        format_bytes(maximum)
                                    )),
                                Err(error) => div().text_color(cx.theme().danger).child(error),
                            })
                            .child(
                                div().flex().flex_wrap().gap_2().children(
                                    CACHE_SIZE_PRESETS.into_iter().enumerate().map(
                                        |(index, size)| {
                                            let model = cache_size_model.clone();
                                            let button = Button::new(("cache-size", index))
                                                .label(format_bytes(size));
                                            let button = if cache_max == Some(size) {
                                                button.primary()
                                            } else {
                                                button
                                            };
                                            button.disabled(!cache_available).on_click(
                                                move |_, _, cx| {
                                                    model.update(cx, |model, cx| {
                                                        model.set_max_cache_size(size, cx)
                                                    });
                                                },
                                            )
                                        },
                                    ),
                                ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .gap_2()
                                    .child(
                                        Button::new("choose-cache-folder")
                                            .label("Choose folder")
                                            .on_click(move |_, _, cx| {
                                                let receiver =
                                                    cx.prompt_for_paths(PathPromptOptions {
                                                        files: false,
                                                        directories: true,
                                                        multiple: false,
                                                        prompt: Some("Use this folder".into()),
                                                    });
                                                let model = choose_cache_model.clone();
                                                cx.spawn(async move |cx| {
                                                    if let Ok(Ok(Some(paths))) = receiver.await
                                                        && let Some(path) = paths.into_iter().next()
                                                    {
                                                        model.update(cx, |model, cx| {
                                                            model.set_cache_root(
                                                                Some(path.to_string_lossy().into()),
                                                                cx,
                                                            )
                                                        });
                                                    }
                                                })
                                                .detach();
                                            }),
                                    )
                                    .child(
                                        Button::new("default-cache-folder")
                                            .label("Use default")
                                            .on_click(move |_, window, cx| {
                                                let answer = window.prompt(
                                                    PromptLevel::Warning,
                                                    "Use the default cache folder?",
                                                    Some(
                                                        "Cached audio and artwork will be moved to the default location.",
                                                    ),
                                                    &["Use default", "Cancel"],
                                                    cx,
                                                );
                                                let model = default_cache_model.clone();
                                                cx.spawn(async move |cx| {
                                                    if answer.await == Ok(0) {
                                                        model.update(cx, |model, cx| {
                                                            model.set_cache_root(None, cx)
                                                        });
                                                    }
                                                })
                                                .detach();
                                            }),
                                    )
                                    .child(
                                        Button::new("clear-audio-cache")
                                            .danger()
                                            .label("Clear cache")
                                            .on_click(move |_, window, cx| {
                                                let answer = window.prompt(
                                                    PromptLevel::Warning,
                                                    "Clear the audio cache?",
                                                    Some(
                                                        "Downloaded audio files will be removed and must be fetched again.",
                                                    ),
                                                    &["Clear cache", "Cancel"],
                                                    cx,
                                                );
                                                let model = clear_cache_model.clone();
                                                cx.spawn(async move |cx| {
                                                    if answer.await == Ok(0) {
                                                        model.update(
                                                            cx,
                                                            DesktopModel::clear_audio_cache,
                                                        );
                                                    }
                                                })
                                                .detach();
                                            }),
                                    )
                                    .child(
                                        Button::new("refresh-audio-cache")
                                            .label("Refresh")
                                            .on_click(move |_, _, cx| {
                                                refresh_cache_model.update(
                                                    cx,
                                                    DesktopModel::refresh_cache_summary,
                                                );
                                            }),
                                    ),
                            ),
                    ),
            )
    }
}

fn section(title: &'static str, cx: &mut Context<SettingsView>) -> gpui::Stateful<gpui::Div> {
    div()
        .id(title)
        .role(Role::Group)
        .aria_label(title)
        .p_3()
        .border_1()
        .border_color(cx.theme().border)
        .rounded_md()
        .flex()
        .flex_col()
        .gap_2()
        .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
}

fn setting_row(label: impl IntoElement, value: impl IntoElement) -> gpui::Div {
    div().flex().justify_between().child(label).child(value)
}

fn toggle_label(label: &str, enabled: bool) -> String {
    format!("{label}: {}", if enabled { "On" } else { "Off" })
}

fn next_value<T: Copy + PartialEq>(current: T, values: &[T]) -> T {
    values
        .iter()
        .position(|value| *value == current)
        .and_then(|index| values.get(index + 1))
        .copied()
        .unwrap_or(values[0])
}

fn next_f64(current: f64, values: &[f64]) -> f64 {
    values
        .iter()
        .position(|value| (*value - current).abs() < f64::EPSILON)
        .and_then(|index| values.get(index + 1))
        .copied()
        .unwrap_or(values[0])
}

fn format_bytes(bytes: u64) -> String {
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
    format!("{:.1} GiB", bytes as f64 / GIB)
}

#[cfg(test)]
mod tests {
    use super::{next_f64, next_value};

    #[test]
    fn setting_cycles_wrap() {
        assert_eq!(next_value(15, &[5, 15, 30]), 30);
        assert_eq!(next_value(30, &[5, 15, 30]), 5);
        assert_eq!(next_f64(-14.0, &[-16.0, -14.0, -12.0]), -12.0);
    }
}
