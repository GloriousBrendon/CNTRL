//! Canvas tab: SignalRGB-style arrangement view. One virtual canvas renders
//! the active effect; devices are dragged into position on top of it and
//! their LEDs sample the pixels underneath — preview and hardware share the
//! same pure effect functions, so what you see is what streams.

use crate::app::{hexc, muted, Message};
use cntrl_core::canvas::{self, CanvasLayout, LedSpec, CANVAS_H, CANVAS_W};
use cntrl_core::theme::ThemeColors;
use iced::widget::canvas::{self as icanvas, Canvas};
use iced::widget::image::{FilterMethod, Handle};
use iced::{mouse, Color, Element, Length, Point, Rectangle, Renderer, Theme};

/// Preview framebuffer resolution (2× canvas units for a smoother backdrop;
/// the stream thread samples at native canvas resolution — same pure effect).
pub const PREVIEW_W: usize = 320;
pub const PREVIEW_H: usize = 180;

/// Render the current effect into an iced image handle for the backdrop.
pub fn render_preview(layout: &CanvasLayout, t: f32) -> Handle {
    let s = layout.settings;
    let mut frame = cntrl_core::canvas::Frame::new(PREVIEW_W, PREVIEW_H);
    let params = cntrl_core::canvas_effects::EffectParams {
        speed: s.speed,
        scale: s.scale,
        color: s.color,
    };
    cntrl_core::canvas_effects::render(s.effect, &mut frame, t, &params);
    Handle::from_rgba(PREVIEW_W as u32, PREVIEW_H as u32, frame.to_rgba())
}

/// The interactive arrangement scene (backdrop + draggable device boxes).
pub struct Scene<'a> {
    pub layout: &'a CanvasLayout,
    pub preview: &'a Handle,
    pub selected: Option<&'a str>,
    pub colors: &'a ThemeColors,
}

/// Drag state kept by the canvas widget between events.
#[derive(Default)]
pub struct DragState {
    drag: Option<Drag>,
}

struct Drag {
    id: String,
    /// Grab offset from the device's placement origin, in canvas units.
    dx: f32,
    dy: f32,
    moved: bool,
}

/// Map widget bounds → (pixels-per-unit, top-left offset) letterboxing the
/// fixed-aspect canvas area inside the widget.
fn viewport(bounds: Rectangle) -> (f32, Point) {
    let ppu = (bounds.width / CANVAS_W).min(bounds.height / CANVAS_H);
    let ox = (bounds.width - CANVAS_W * ppu) / 2.0;
    let oy = (bounds.height - CANVAS_H * ppu) / 2.0;
    (ppu, Point::new(ox, oy))
}

fn to_units(bounds: Rectangle, p: Point) -> (f32, f32) {
    let (ppu, o) = viewport(bounds);
    ((p.x - o.x) / ppu, (p.y - o.y) / ppu)
}

/// Topmost device under a canvas-unit point (reverse draw order).
fn hit_test(layout: &CanvasLayout, unit: (f32, f32)) -> Option<&canvas::PlacedDevice> {
    layout.devices.iter().rev().find(|d| {
        layout
            .spec(&d.id)
            .is_some_and(|spec| d.placement.contains(spec.size, unit))
    })
}

impl icanvas::Program<Message> for Scene<'_> {
    type State = DragState;

    fn update(
        &self,
        state: &mut Self::State,
        event: &icanvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<icanvas::Action<Message>> {
        match event {
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let pos = cursor.position_in(bounds)?;
                let unit = to_units(bounds, pos);
                match hit_test(self.layout, unit) {
                    Some(d) => {
                        state.drag = Some(Drag {
                            id: d.id.clone(),
                            dx: unit.0 - d.placement.x,
                            dy: unit.1 - d.placement.y,
                            moved: false,
                        });
                        Some(
                            icanvas::Action::publish(Message::CanvasSelect(Some(d.id.clone())))
                                .and_capture(),
                        )
                    }
                    None => Some(icanvas::Action::publish(Message::CanvasSelect(None))),
                }
            }
            iced::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let drag = state.drag.as_mut()?;
                let pos = cursor.position_in(bounds)?;
                let unit = to_units(bounds, pos);
                drag.moved = true;
                let x = (unit.0 - drag.dx).clamp(-40.0, CANVAS_W + 20.0);
                let y = (unit.1 - drag.dy).clamp(-40.0, CANVAS_H + 20.0);
                Some(icanvas::Action::publish(Message::CanvasMove(drag.id.clone(), x, y)).and_capture())
            }
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                let drag = state.drag.take()?;
                drag.moved
                    .then(|| icanvas::Action::publish(Message::CanvasDragEnd).and_capture())
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<icanvas::Geometry> {
        let mut f = icanvas::Frame::new(renderer, bounds.size());
        let (ppu, o) = viewport(bounds);
        let area = Rectangle::new(o, iced::Size::new(CANVAS_W * ppu, CANVAS_H * ppu));

        // Effect backdrop.
        f.draw_image(
            area,
            icanvas::Image::new(self.preview.clone()).filter_method(FilterMethod::Linear),
        );
        // Frame line around the canvas area.
        f.stroke_rectangle(
            area.position(),
            area.size(),
            icanvas::Stroke::default()
                .with_color(Color::from_rgba(1.0, 1.0, 1.0, 0.25))
                .with_width(1.0),
        );

        let accent = hexc(&self.colors.accent, Color::from_rgb8(0xd8, 0x8f, 0x92));
        let to_px = |unit: (f32, f32)| Point::new(o.x + unit.0 * ppu, o.y + unit.1 * ppu);

        for d in &self.layout.devices {
            let Some(spec) = self.layout.spec(&d.id) else {
                continue;
            };
            let pl = &d.placement;
            let selected = self.selected == Some(d.id.as_str());
            let dim = if d.enabled { 0.28 } else { 0.55 };
            let outline_color = if selected {
                accent
            } else {
                Color::from_rgba(1.0, 1.0, 1.0, if d.enabled { 0.55 } else { 0.25 })
            };
            let outline = icanvas::Stroke::default()
                .with_color(outline_color)
                .with_width(if selected { 2.5 } else { 1.0 });

            let shape = if spec.is_fan {
                // Fan: circular outline around the LED ring.
                let center = to_px(pl.to_canvas(spec.size, (spec.size.0 / 2.0, spec.size.1 / 2.0)));
                let radius = spec.size.0 / 2.0 * pl.scale * ppu;
                icanvas::Path::circle(center, radius)
            } else {
                let corners = [
                    (0.0, 0.0),
                    (spec.size.0, 0.0),
                    (spec.size.0, spec.size.1),
                    (0.0, spec.size.1),
                ]
                .map(|c| to_px(pl.to_canvas(spec.size, c)));
                icanvas::Path::new(|b| {
                    b.move_to(corners[0]);
                    b.line_to(corners[1]);
                    b.line_to(corners[2]);
                    b.line_to(corners[3]);
                    b.close();
                })
            };
            f.fill(&shape, Color::from_rgba(0.0, 0.0, 0.0, dim));
            f.stroke(&shape, outline);

            // LED dots: each LED's actual sampling point. The chain's first
            // LED is drawn in accent so backwards wiring is easy to spot
            // (flip it with the device's Reverse toggle).
            let r = (0.45 * pl.scale * ppu).clamp(1.5, 6.0);
            let led_alpha = if d.enabled { 0.95 } else { 0.35 };
            for (i, &p) in spec.led_positions.iter().enumerate() {
                let c = to_px(pl.to_canvas(spec.size, p));
                let is_first = i == if d.reversed { spec.led_positions.len() - 1 } else { 0 };
                f.fill(
                    &icanvas::Path::circle(c, if is_first { r * 1.4 } else { r }),
                    if is_first {
                        accent
                    } else {
                        Color::from_rgba(1.0, 1.0, 1.0, led_alpha)
                    },
                );
            }

            // Label above the box center.
            let center = to_px(pl.to_canvas(spec.size, (spec.size.0 / 2.0, spec.size.1 / 2.0)));
            let mut label = spec.name.to_string();
            if d.reversed {
                label.push_str(" ⇄");
            }
            f.fill_text(icanvas::Text {
                content: label,
                position: Point::new(center.x, center.y),
                color: if d.enabled {
                    Color::WHITE
                } else {
                    Color::from_rgba(1.0, 1.0, 1.0, 0.5)
                },
                size: 12.0.into(),
                align_x: iced::widget::text::Alignment::Center,
                align_y: iced::alignment::Vertical::Center,
                ..icanvas::Text::default()
            });
        }

        // Cursor hint: crosshair position readout while dragging.
        if state.drag.is_some() {
            if let Some(pos) = cursor.position_in(bounds) {
                let unit = to_units(bounds, pos);
                f.fill_text(icanvas::Text {
                    content: format!("{:.0} · {:.0}", unit.0, unit.1),
                    position: Point::new(area.x + 8.0, area.y + 8.0),
                    color: muted(self.colors),
                    size: 11.0.into(),
                    ..icanvas::Text::default()
                });
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
            if hit_test(self.layout, to_units(bounds, pos)).is_some() {
                return mouse::Interaction::Grab;
            }
        }
        mouse::Interaction::default()
    }
}

/// Build the scene element (the caller wraps it with the controls).
pub fn scene_element<'a>(
    layout: &'a CanvasLayout,
    preview: &'a Handle,
    selected: Option<&'a str>,
    colors: &'a ThemeColors,
) -> Element<'a, Message> {
    Canvas::new(Scene {
        layout,
        preview,
        selected,
        colors,
    })
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// Convenience: pretty spec list in stable order (used by the enable chips).
pub fn placed_specs(layout: &CanvasLayout) -> Vec<(String, LedSpec, bool)> {
    layout
        .devices
        .iter()
        .filter_map(|d| layout.spec(&d.id).map(|s| (d.id.clone(), s, d.enabled)))
        .collect()
}
