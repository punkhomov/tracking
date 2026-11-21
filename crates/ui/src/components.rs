use egui::{
    Align, Atom, Button, Color32, CornerRadius, Frame, Id, IntoAtoms as _, Layout, Margin, Popup,
    Response, RichText, Sense, Stroke, StrokeKind, Ui, WidgetInfo, WidgetType, pos2, vec2,
};

use crate::{
    icons::Icon,
    pages::Screen,
    theme::{CARD_RADIUS, CONTROL_HEIGHT, CONTROL_RADIUS, accent, palette},
};

const PAGE_WIDTH: f32 = 920.0;
const PAGE_INSET: i8 = 16;
const CARD_INSET: i8 = 16;
const TAB_MIN_WIDTH: f32 = 96.0;
const TAB_GAP: f32 = 6.0;

pub(crate) fn page(ui: &mut Ui, body: impl FnOnce(&mut Ui)) {
    egui::CentralPanel::default()
        .frame(
            Frame::new()
                .fill(ui.visuals().extreme_bg_color)
                .inner_margin(Margin::same(PAGE_INSET)),
        )
        .show(ui, |ui| {
            let width = ui.available_width().min(PAGE_WIDTH);
            ui.vertical_centered(|ui| {
                ui.allocate_ui_with_layout(
                    vec2(width, ui.available_height()),
                    Layout::top_down(Align::Min),
                    |ui| {
                        ui.set_width(width);
                        body(ui);
                    },
                );
            });
        });
}

pub(crate) fn navigation(ui: &mut Ui, current: Screen) -> Option<Screen> {
    let available = ui.available_width();
    let columns = [6, 3, 2]
        .into_iter()
        .find(|columns| {
            available >= TAB_MIN_WIDTH * *columns as f32 + TAB_GAP * (*columns - 1) as f32
        })
        .unwrap_or(1);
    let width = ((available - TAB_GAP * (columns - 1) as f32) / columns as f32).floor();
    let mut changed = None;
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing = vec2(TAB_GAP, TAB_GAP);
        for row in Screen::ALL.chunks(columns) {
            ui.horizontal(|ui| {
                for &screen in row {
                    let selected = current == screen;
                    let button = Button::selectable(selected, screen.title())
                        .fill(Color32::TRANSPARENT)
                        .stroke(Stroke::NONE)
                        .truncate()
                        .min_size(vec2(width, CONTROL_HEIGHT));
                    let response = ui.add(button);
                    if selected {
                        let rect = response.rect;
                        let y = rect.bottom() - 2.0;
                        ui.painter().line_segment(
                            [pos2(rect.left() + 12.0, y), pos2(rect.right() - 12.0, y)],
                            Stroke::new(2.0, accent(ui)),
                        );
                    }
                    focus_ring(ui, &response);
                    if response.clicked() && !selected {
                        changed = Some(screen);
                    }
                }
            });
        }
    });
    changed
}

pub(crate) fn action_row<R>(ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> R {
    ui.horizontal_wrapped(body).inner
}

pub(crate) fn segmented<T: Copy + PartialEq>(ui: &mut Ui, value: &mut T, options: [(T, &str); 2]) {
    let border = ui.visuals().widgets.noninteractive.bg_stroke.color;
    let selection = palette(ui.visuals().dark_mode).selection;
    Frame::new()
        .corner_radius(CONTROL_RADIUS)
        .stroke(Stroke::new(1.0, border))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.horizontal(|ui| {
                for (index, (option, label)) in options.into_iter().enumerate() {
                    let outer = CONTROL_RADIUS - 1;
                    let (left, right) = if index == 0 { (outer, 0) } else { (0, outer) };
                    let selected = *value == option;
                    let fill = if selected {
                        selection
                    } else {
                        Color32::TRANSPARENT
                    };
                    let button = Button::selectable(selected, label)
                        .fill(fill)
                        .corner_radius(CornerRadius {
                            nw: left,
                            sw: left,
                            ne: right,
                            se: right,
                        })
                        .truncate();
                    if ui.add(button).clicked() {
                        *value = option;
                    }
                }
            });
        });
}

pub(crate) fn primary(ui: &mut Ui, label: &str) -> Response {
    accent_button(ui, label, None)
}

pub(crate) fn primary_add(ui: &mut Ui, label: &str) -> Response {
    accent_button(ui, label, Some(Icon::Plus))
}

fn accent_button(ui: &mut Ui, label: &str, icon: Option<Icon>) -> Response {
    let color = palette(ui.visuals().dark_mode).on_accent;
    let label = RichText::new(label).strong().color(color);
    let icon_id = Id::new("button-icon");
    let atoms = match icon {
        Some(_) => (Atom::custom(icon_id, vec2(16.0, 16.0)), label).into_atoms(),
        None => label.into_atoms(),
    };
    let button = Button::new(atoms).fill(accent(ui)).atom_ui(ui);
    if let Some(icon) = icon
        && let Some(rect) = button.rect(icon_id)
    {
        icon.paint(ui, rect.center(), color);
    }
    focus_ring(ui, &button.response);
    button.response
}

pub(crate) fn actions_menu(ui: &mut Ui, body: impl FnOnce(&mut Ui)) {
    let response = ui.add(
        Button::new(Atom::default())
            .frame_when_inactive(false)
            .min_size(vec2(CONTROL_HEIGHT, CONTROL_HEIGHT)),
    );
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, ui.is_enabled(), "Действия"));
    Icon::Ellipsis.paint(ui, response.rect.center(), ui.visuals().text_color());
    Popup::menu(&response).show(body);
    response.on_hover_text("Действия с сессией");
}

fn focus_ring(ui: &Ui, response: &Response) {
    if response.has_focus() {
        ui.painter().rect_stroke(
            response.rect.expand(2.0),
            CONTROL_RADIUS + 2,
            Stroke::new(1.0, accent(ui)),
            StrokeKind::Outside,
        );
    }
}

pub(crate) fn card<R>(ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> R {
    Frame::new()
        .fill(ui.visuals().panel_fill)
        .stroke(Stroke::new(
            1.0,
            ui.visuals().widgets.noninteractive.bg_stroke.color,
        ))
        .corner_radius(CARD_RADIUS)
        .inner_margin(Margin::same(CARD_INSET))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            body(ui)
        })
        .inner
}

pub(crate) fn color_dot(ui: &mut Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(9.0, 9.0), Sense::hover());
    if ui.is_rect_visible(rect) {
        ui.painter()
            .circle_filled(rect.center(), rect.width() * 0.5, color);
    }
}
