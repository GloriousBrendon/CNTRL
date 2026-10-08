//! Fans tab: temperature→duty curve editor, à la Windows "Fan Control".
//!
//! The GUI never writes `pwm*` itself — it only edits `fan_curves.json` via
//! `fan_curves::save()`; the daemon (or `cntrl fans run`) picks the change up on
//! its next tick. This is the same relationship `settings.json` has with the
//! theme watcher, and it avoids two writers racing over a sysfs file.
//!
//! The curve editor is an `iced::widget::canvas::Program` (same pattern as the
//! Canvas tab's arrangement scene): drag points, click empty space to add a
//! point, right-click a point to delete it.

use crate::app::Message;
use cntrl_core::fan_curves::CurvePoint;
use cntrl_core::theme::ThemeColors;
use iced::widget::canvas::{self as icanvas, Canvas};
use iced::{mouse, Color, Element, Length, Point, Rectangle, Renderer, Theme};

/// Inner plot margins (px) reserved for axis labels.
const M_LEFT: f32 = 34.0;
const M_RIGHT: f32 = 10.0;
const M_TOP: f32 = 10.0;
const M_BOTTOM: f32 = 22.0;
/// Pixel radius for grabbing / hovering a point.
const HIT_RADIUS: f32 = 14.0;

/// The interactive curve editor for one graph curve.
pub struct CurveScene<'a> {
    pub curve_id: String,
    pub points: &'a [CurvePoint],
    /// Current smoothed temperature for this curve's sensor (draws a marker).
    pub current_temp: Option<f32>,
    pub colors: &'a ThemeColors,
}

#[derive(Default)]
pub struct EditState {
    drag: Option<usize>,
}

/// Plot rectangle inside the widget bounds (excludes label margins).
fn plot_rect(bounds: Rectangle) -> Rectangle {
    Rectangle::new(
        Point::new(M_LEFT, M_TOP),
        iced::Size::new(
            (bounds.width - M_LEFT - M_RIGHT).max(1.0),
            (bounds.height - M_TOP - M_BOTTOM).max(1.0),
        ),
    )
}

/// (temp 0..100, duty 0..100) → pixel point within the plot.
fn to_px(plot: Rectangle, temp_c: f32, duty: f32) -> Point {
    let x = plot.x + (temp_c.clamp(0.0, 100.0) / 100.0) * plot.width;
    let y = plot.y + (1.0 - duty.clamp(0.0, 100.0) / 100.0) * plot.height;
    Point::new(x, y)
}

/// pixel point → (temp 0..100, duty 0..100), clamped.
fn to_data(plot: Rectangle, p: Point) -> (f32, f32) {
    let temp = ((p.x - plot.x) / plot.width * 100.0).clamp(0.0, 100.0);
    let duty = ((1.0 - (p.y - plot.y) / plot.height) * 100.0).clamp(0.0, 100.0);
    (temp, duty)
}

/// Index of the nearest point within HIT_RADIUS of the cursor, if any.
fn nearest_point(plot: Rectangle, points: &[CurvePoint], cursor: Point) -> Option<usize> {
    let mut best: Option<(usize, f32)> = None;
    for (i, pt) in points.iter().enumerate() {
        let pp = to_px(plot, pt.temp_c, pt.duty_pct as f32);
        let d = pp.distance(cursor);
        if d <= HIT_RADIUS && best.map(|(_, bd)| d < bd).unwrap_or(true) {
            best = Some((i, d));
        }
    }
    best.map(|(i, _)| i)
}

impl icanvas::Program<Message> for CurveScene<'_> {
    type State = EditState;

    fn update(
        &self,
        state: &mut Self::State,
        event: &icanvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<icanvas::Action<Message>> {
        let plot = plot_rect(bounds);
        match event {
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let pos = cursor.position_in(bounds)?;
                match nearest_point(plot, self.points, pos) {
                    Some(i) => {
                        state.drag = Some(i);
                        Some(icanvas::Action::request_redraw().and_capture())
                    }
                    None => {
                        // Click on empty space → add a point there.
                        let (t, d) = to_data(plot, pos);
                        Some(
                            icanvas::Action::publish(Message::FanCurveAddPoint(
                                self.curve_id.clone(),
                                t,
                                d,
                            ))
                            .and_capture(),
                        )
                    }
                }
            }
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) => {
                let pos = cursor.position_in(bounds)?;
                let i = nearest_point(plot, self.points, pos)?;
                Some(
                    icanvas::Action::publish(Message::FanCurveDeletePoint(self.curve_id.clone(), i))
                        .and_capture(),
                )
            }
            iced::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let i = state.drag?;
                let pos = cursor.position_in(bounds)?;
                let (t, d) = to_data(plot, pos);
                Some(
                    icanvas::Action::publish(Message::FanCurvePointMoved(
                        self.curve_id.clone(),
                        i,
                        t,
                        d,
                    ))
                    .and_capture(),
                )
            }
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                state.drag.take()?;
                Some(icanvas::Action::publish(Message::FanCurveEditDone).and_capture())
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<icanvas::Geometry> {
        let plot = plot_rect(bounds);
        let accent = crate::app::hexc(&self.colors.accent, Color::from_rgb8(0xd8, 0x8f, 0x92));
        let grid_c = Color::from_rgba(1.0, 1.0, 1.0, 0.10);
        let label_c = crate::app::muted(self.colors);

        let mut f = icanvas::Frame::new(renderer, bounds.size());

        // Plot border.
        f.stroke_rectangle(
            plot.position(),
            plot.size(),
            icanvas::Stroke::default().with_color(grid_c).with_width(1.0),
        );

        // Gridlines + axis labels every 20 units.
        for g in (0..=100).step_by(20) {
            let gv = g as f32;
            // Vertical (temp) line.
            let x = to_px(plot, gv, 0.0).x;
            f.stroke(
                &icanvas::Path::line(Point::new(x, plot.y), Point::new(x, plot.y + plot.height)),
                icanvas::Stroke::default().with_color(grid_c).with_width(1.0),
            );
            f.fill_text(icanvas::Text {
                content: format!("{g}"),
                position: Point::new(x, plot.y + plot.height + 4.0),
                color: label_c,
                size: 9.0.into(),
                align_x: iced::widget::text::Alignment::Center,
                align_y: iced::alignment::Vertical::Top,
                ..icanvas::Text::default()
            });
            // Horizontal (duty) line.
            let y = to_px(plot, 0.0, gv).y;
            f.stroke(
                &icanvas::Path::line(Point::new(plot.x, y), Point::new(plot.x + plot.width, y)),
                icanvas::Stroke::default().with_color(grid_c).with_width(1.0),
            );
            f.fill_text(icanvas::Text {
                content: format!("{g}"),
                position: Point::new(plot.x - 4.0, y),
                color: label_c,
                size: 9.0.into(),
                align_x: iced::widget::text::Alignment::Right,
                align_y: iced::alignment::Vertical::Center,
                ..icanvas::Text::default()
            });
        }

        // Current-temperature marker (vertical line + °C readout).
        if let Some(t) = self.current_temp {
            let x = to_px(plot, t, 0.0).x;
            f.stroke(
                &icanvas::Path::line(Point::new(x, plot.y), Point::new(x, plot.y + plot.height)),
                icanvas::Stroke::default()
                    .with_color(Color::from_rgba(accent.r, accent.g, accent.b, 0.7))
                    .with_width(1.5),
            );
            f.fill_text(icanvas::Text {
                content: format!("{t:.0}°C"),
                position: Point::new(x + 3.0, plot.y + 2.0),
                color: accent,
                size: 10.0.into(),
                ..icanvas::Text::default()
            });
        }

        // The curve: sorted points, connected, with clamped flat ends.
        let mut pts: Vec<&CurvePoint> = self.points.iter().collect();
        pts.sort_by(|a, b| a.temp_c.partial_cmp(&b.temp_c).unwrap_or(std::cmp::Ordering::Equal));
        if !pts.is_empty() {
            let first = pts[0];
            let last = pts[pts.len() - 1];
            let path = icanvas::Path::new(|b| {
                // Flat lead-in from the left edge at the first point's duty.
                b.move_to(to_px(plot, 0.0, first.duty_pct as f32));
                for p in &pts {
                    b.line_to(to_px(plot, p.temp_c, p.duty_pct as f32));
                }
                // Flat lead-out to the right edge at the last point's duty.
                b.line_to(to_px(plot, 100.0, last.duty_pct as f32));
            });
            f.stroke(
                &path,
                icanvas::Stroke::default().with_color(accent).with_width(2.0),
            );

            // Point handles.
            for p in &pts {
                let c = to_px(plot, p.temp_c, p.duty_pct as f32);
                f.fill(&icanvas::Path::circle(c, 5.0), accent);
                f.fill(
                    &icanvas::Path::circle(c, 2.5),
                    Color::from_rgb8(0x10, 0x12, 0x16),
                );
            }
        }

        vec![f.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.drag.is_some() {
            return mouse::Interaction::Grabbing;
        }
        if let Some(pos) = cursor.position_in(bounds) {
            if nearest_point(plot_rect(bounds), self.points, pos).is_some() {
                return mouse::Interaction::Grab;
            }
        }
        mouse::Interaction::Crosshair
    }
}

/// Build the curve-editor canvas element.
pub fn curve_editor<'a>(
    curve_id: String,
    points: &'a [CurvePoint],
    current_temp: Option<f32>,
    colors: &'a ThemeColors,
) -> Element<'a, Message> {
    Canvas::new(CurveScene {
        curve_id,
        points,
        current_temp,
        colors,
    })
    .width(Length::Fill)
    .height(Length::Fixed(240.0))
    .into()
}
