use app_platform::{
    SaveComplete,
    cloud::{CloudComplete, CloudRequest, Setup},
};
use chrono::TimeDelta;
use egui::{
    Key, Pos2, Rect,
    accesskit::{Role, Toggled},
    vec2,
};
use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};
use tracking_core::Period;

use super::*;

const SHORT_NOTE: &str = "Короткая запись";
const LONG_NOTE: &str =
    "Длинная заметка с деталями: подготовка материалов, сводка и следующие шаги.";

struct TestPlatform;

impl Platform for TestPlatform {
    fn save(&mut self, _: &TrackerState, _: &CloudState, complete: SaveComplete) {
        complete(Ok(()));
    }

    fn cloud(&mut self, _: CloudRequest, _: CloudComplete) {
        panic!("the UI fixture uses local mode");
    }

    fn open_archive(&self, complete: Complete<Option<ArchiveFile>>) {
        complete(Ok(None));
    }

    fn save_archive(&self, _: &str, _: String, complete: Complete<bool>) {
        complete(Ok(false));
    }

    fn notify(&self, _: &str, _: &str) {}

    fn request_notifications(&self, complete: Complete<bool>) {
        complete(Ok(false));
    }

    fn set_title(&self, _: &str) {}
}

fn tracker(
    width: f32,
    height: f32,
    pixels_per_point: f32,
    theme: &str,
) -> Harness<'static, Option<Tracker>> {
    let now = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    let mut state = TrackerState::default();
    state.dataset.settings.theme = theme.into();
    for (id, offset, note) in [(1, 15, SHORT_NOTE), (2, 75, LONG_NOTE)] {
        let end = now - TimeDelta::minutes(offset);
        state.dataset.periods.insert(
            Uuid::from_u128(id),
            Period {
                start: end - TimeDelta::minutes(25),
                end,
                note: note.into(),
                category: None,
                kind: Kind::Work,
                pauses: Vec::new(),
            },
        );
    }
    let cloud = CloudState {
        setup: Setup::Local,
        ..CloudState::default()
    };
    let mut fixture = Some((state, cloud));
    Harness::builder()
        .with_size(vec2(width, height))
        .with_pixels_per_point(pixels_per_point)
        .build_ui_state(
            move |ui, app: &mut Option<Tracker>| {
                app.get_or_insert_with(|| {
                    let (state, cloud) = fixture.take().unwrap();
                    Tracker::new(ui.ctx(), state, cloud, TestPlatform)
                })
                .ui(ui, now);
            },
            None,
        )
}

#[test]
fn navigation_and_primary_timer_action_fit_the_viewport() {
    for (width, scale, theme) in [
        (1100.0, 1.0, "light"),
        (360.0, 1.25, "dark"),
        (320.0, 2.0, "light"),
    ] {
        let mut harness = tracker(width, 760.0, scale, theme);
        harness.run_steps(2);
        let viewport = Rect::from_min_size(Pos2::ZERO, vec2(width, 760.0));
        let buttons = Screen::ALL.map(|screen| harness.get_by_label(screen.title()).rect());
        for (index, button) in buttons.iter().enumerate() {
            assert!(
                viewport.contains_rect(*button),
                "tab outside {viewport:?}: {button:?}"
            );
            assert!(button.height() >= 30.0);
            assert!(
                buttons[index + 1..]
                    .iter()
                    .all(|other| !button.intersects(*other))
            );
        }
        let left = buttons
            .iter()
            .map(|rect| rect.left())
            .fold(f32::INFINITY, f32::min);
        let right = buttons.iter().map(|rect| rect.right()).fold(0.0, f32::max);
        assert!((left - (width - right)).abs() <= 2.0);
        let start = harness.get_by_label("Запустить");
        assert_eq!(start.accesskit_node().role(), Role::Button);
        assert!(viewport.contains_rect(start.rect()));
    }
}

#[test]
fn timeline_segments_fit_and_switch_exclusive_selection_with_the_keyboard() {
    let mut harness = tracker(320.0, 760.0, 2.0, "light");
    let full = harness.get_by_label("Весь день");
    let since_first = harness.get_by_label("От первой сессии");
    assert!(full.rect().left() >= 0.0);
    assert!(since_first.rect().right() <= 320.0);
    assert!((full.rect().right() - since_first.rect().left()).abs() < 1.0);
    assert_eq!(since_first.accesskit_node().toggled(), Some(Toggled::True));
    full.focus();
    harness.run_steps(1);
    harness.key_press(Key::Enter);
    harness.run_steps(2);
    assert_eq!(
        harness.get_by_label("Весь день").accesskit_node().toggled(),
        Some(Toggled::True)
    );
    assert_eq!(
        harness
            .get_by_label("От первой сессии")
            .accesskit_node()
            .toggled(),
        Some(Toggled::False)
    );
}

#[test]
fn history_actions_stay_aligned_and_navigation_works_with_the_keyboard() {
    for (width, scale, theme) in [(320.0, 2.0, "dark"), (1100.0, 1.0, "light")] {
        let mut harness = tracker(width, 760.0, scale, theme);
        harness.get_by_label("История").focus();
        harness.run_steps(1);
        assert!(harness.get_by_label("История").is_focused());
        harness.key_press(Key::Enter);
        harness.run_steps(2);
        assert_eq!(harness.state().as_ref().unwrap().screen, Screen::History);
        let date_filter = harness.get_by_value("За всё время").rect();
        let kind_filter = harness.get_by_value("Все типы").rect();
        assert!((date_filter.center().y - kind_filter.center().y).abs() < 1.0);
        let actions: Vec<_> = harness
            .get_all_by_label("Действия")
            .map(|node| node.rect())
            .collect();
        assert_eq!(actions.len(), 2);
        assert!((actions[0].right() - actions[1].right()).abs() < 1.0);
        for (action, note) in actions.iter().zip([SHORT_NOTE, LONG_NOTE]) {
            let title = harness.get_by_label(note).rect();
            assert!(title.right() <= action.left());
            assert!(action.y_range().intersects(title.y_range()));
            assert!(action.right() <= width && action.left() >= 0.0);
        }
        harness.get_all_by_label("Действия").next().unwrap().focus();
        harness.run_steps(1);
        harness.key_press(Key::Enter);
        harness.run_steps(2);
        assert!(harness.query_by_label("Редактировать").is_some());
    }
}

#[test]
fn editor_actions_fit_a_short_window_and_escape_closes_the_dialog() {
    let mut harness = tracker(320.0, 420.0, 2.0, "light");
    harness.get_by_label("Новая категория").focus();
    harness.run_steps(1);
    harness.key_press(Key::Enter);
    harness.run_steps(3);
    let viewport = Rect::from_min_size(Pos2::ZERO, vec2(320.0, 420.0));
    for label in ["Сохранить", "Отмена"] {
        assert!(viewport.contains_rect(harness.get_by_label(label).rect()));
    }
    harness.key_press(Key::Escape);
    harness.run_steps(2);
    assert!(harness.state().as_ref().unwrap().editor.is_none());
}
