use egui::{Color32, Pos2, Stroke, Ui, vec2};

#[derive(Clone, Copy)]
pub(crate) enum Icon {
    Plus,
    Ellipsis,
}

impl Icon {
    pub(crate) fn paint(self, ui: &Ui, center: Pos2, color: Color32) {
        // Lucide uses a 24 × 24 viewport. Controls render the icons at 16 points.
        let scale = 16.0 / 24.0;
        let offset = |x, y| center + vec2(x, y) * scale;
        match self {
            Self::Plus => {
                let painter = ui.painter();
                let stroke = Stroke::new(2.0 * scale, color);
                for (start, end) in [
                    (offset(-7.0, 0.0), offset(7.0, 0.0)),
                    (offset(0.0, -7.0), offset(0.0, 7.0)),
                ] {
                    painter.line_segment([start, end], stroke);
                    painter.circle_filled(start, scale, color);
                    painter.circle_filled(end, scale, color);
                }
            }
            Self::Ellipsis => {
                for x in [-7.0, 0.0, 7.0] {
                    ui.painter()
                        .circle_filled(offset(x, 0.0), 2.0 * scale, color);
                }
            }
        }
    }
}
