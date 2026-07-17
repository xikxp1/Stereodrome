use gpui::{App, Entity, KeyBinding, actions};

use super::model::DesktopModel;

actions!(
    stereodrome,
    [
        TogglePlayback,
        PlaySelection,
        PreviousTrack,
        NextTrack,
        SeekBackward,
        SeekForward,
        VolumeUp,
        VolumeDown,
        SelectPrevious,
        SelectNext,
        ToggleMute,
        ToggleShuffle,
        CycleRepeat,
        ToggleQueue,
        ToggleSpectrum,
        RerollNext,
        FocusSearch,
        OpenSettings,
        ClearFocus,
        ShowMainWindow,
        Quit,
    ]
);

const APP_CONTEXT: &str = "Stereodrome && !Input";

pub fn bind_keys(cx: &mut App) {
    let mut bindings = vec![
        KeyBinding::new("space", TogglePlayback, Some(APP_CONTEXT)),
        KeyBinding::new("enter", PlaySelection, Some(APP_CONTEXT)),
        KeyBinding::new("shift-left", SeekBackward, Some(APP_CONTEXT)),
        KeyBinding::new("shift-right", SeekForward, Some(APP_CONTEXT)),
        KeyBinding::new("up", SelectPrevious, Some(APP_CONTEXT)),
        KeyBinding::new("down", SelectNext, Some(APP_CONTEXT)),
        KeyBinding::new("m", ToggleMute, Some(APP_CONTEXT)),
        KeyBinding::new("s", ToggleShuffle, Some(APP_CONTEXT)),
        KeyBinding::new("r", CycleRepeat, Some(APP_CONTEXT)),
        KeyBinding::new("q", ToggleQueue, Some(APP_CONTEXT)),
        KeyBinding::new("v", ToggleSpectrum, Some(APP_CONTEXT)),
        KeyBinding::new("d", RerollNext, Some(APP_CONTEXT)),
        KeyBinding::new("escape", ClearFocus, Some("Stereodrome")),
    ];
    bindings.extend([
        KeyBinding::new("secondary-left", PreviousTrack, Some(APP_CONTEXT)),
        KeyBinding::new("secondary-right", NextTrack, Some(APP_CONTEXT)),
        KeyBinding::new("secondary-up", VolumeUp, Some(APP_CONTEXT)),
        KeyBinding::new("secondary-down", VolumeDown, Some(APP_CONTEXT)),
        KeyBinding::new("secondary-k", FocusSearch, Some(APP_CONTEXT)),
        KeyBinding::new("secondary-,", OpenSettings, Some(APP_CONTEXT)),
    ]);
    cx.bind_keys(bindings);
}

pub fn install_model_handlers(model: &Entity<DesktopModel>, cx: &mut App) {
    let weak = model.downgrade();
    cx.on_action(move |_: &TogglePlayback, cx| {
        weak.update(cx, |model, cx| {
            if model.playback.song.is_some() {
                model.toggle_playback(cx);
            } else {
                model.play_selection(cx);
            }
        })
        .ok();
    });

    let weak = model.downgrade();
    cx.on_action(move |_: &PlaySelection, cx| {
        weak.update(cx, DesktopModel::play_selection).ok();
    });
    let weak = model.downgrade();
    cx.on_action(move |_: &PreviousTrack, cx| {
        weak.update(cx, DesktopModel::play_previous).ok();
    });
    let weak = model.downgrade();
    cx.on_action(move |_: &NextTrack, cx| {
        weak.update(cx, DesktopModel::play_next).ok();
    });
    let weak = model.downgrade();
    cx.on_action(move |_: &SeekBackward, cx| {
        weak.update(cx, |model, cx| model.seek_by(-10.0, cx)).ok();
    });
    let weak = model.downgrade();
    cx.on_action(move |_: &SeekForward, cx| {
        weak.update(cx, |model, cx| model.seek_by(10.0, cx)).ok();
    });
    let weak = model.downgrade();
    cx.on_action(move |_: &VolumeUp, cx| {
        weak.update(cx, |model, cx| model.adjust_volume(0.05, cx))
            .ok();
    });
    let weak = model.downgrade();
    cx.on_action(move |_: &VolumeDown, cx| {
        weak.update(cx, |model, cx| model.adjust_volume(-0.05, cx))
            .ok();
    });
    let weak = model.downgrade();
    cx.on_action(move |_: &SelectPrevious, cx| {
        weak.update(cx, |model, cx| model.navigate_selection(-1, cx))
            .ok();
    });
    let weak = model.downgrade();
    cx.on_action(move |_: &SelectNext, cx| {
        weak.update(cx, |model, cx| model.navigate_selection(1, cx))
            .ok();
    });
    let weak = model.downgrade();
    cx.on_action(move |_: &ToggleMute, cx| {
        weak.update(cx, DesktopModel::toggle_mute).ok();
    });
    let weak = model.downgrade();
    cx.on_action(move |_: &ToggleShuffle, cx| {
        weak.update(cx, DesktopModel::toggle_shuffle).ok();
    });
    let weak = model.downgrade();
    cx.on_action(move |_: &CycleRepeat, cx| {
        weak.update(cx, DesktopModel::cycle_repeat).ok();
    });
    let weak = model.downgrade();
    cx.on_action(move |_: &ToggleQueue, cx| {
        weak.update(cx, DesktopModel::toggle_queue).ok();
    });
    let weak = model.downgrade();
    cx.on_action(move |_: &ToggleSpectrum, cx| {
        weak.update(cx, DesktopModel::toggle_spectrum).ok();
    });
    let weak = model.downgrade();
    cx.on_action(move |_: &RerollNext, cx| {
        weak.update(cx, DesktopModel::reroll_next).ok();
    });
    let weak = model.downgrade();
    cx.on_action(move |_: &FocusSearch, cx| {
        weak.update(cx, DesktopModel::focus_search).ok();
    });
    let weak = model.downgrade();
    cx.on_action(move |_: &OpenSettings, cx| {
        weak.update(cx, DesktopModel::open_settings).ok();
    });
}
