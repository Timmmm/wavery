use std::{collections::HashMap, ops::Range};

use eframe::{
    emath::{self, RectTransform},
    epaint,
};
use egui::{
    pos2, vec2, Align2, Color32, FontFamily, FontId, Frame, Pos2, Rect, Response, Shape, Stroke,
    Ui, Vec2,
};
use fst::{
    fst::{Fst, VarId, VarLength},
    valvec::ValAndTimeVec,
};

pub fn show_waves_widget(
    ui: &mut Ui,
    file: &Fst,
    cached_waves: &HashMap<VarId, ValAndTimeVec>,
    timespan: Range<f64>,
) -> Response {
    let wave_colour = if ui.visuals().dark_mode {
        Color32::from_additive_luminance(196)
    } else {
        Color32::from_black_alpha(240)
    };

    let x_colour = if ui.visuals().dark_mode {
        Color32::from_additive_luminance(196)
    } else {
        Color32::from_black_alpha(240)
    };

    Frame::canvas(ui.style())
        .show(ui, |ui| {
            let desired_size = ui.available_size();
            let (id, rect) = ui.allocate_space(desired_size);

            let response = ui.interact(rect, id, egui::Sense::click());

            ui.set_clip_rect(rect);

            const LINE_SPACING: f32 = 1.4;

            draw_timeline(ui, timespan.clone(), rect);

            let mut wave_rect = rect;
            wave_rect.set_top(wave_rect.top() + 30.0);

            let to_screen = emath::RectTransform::from_to(
                Rect::from_x_y_ranges(
                    timespan.start as f32..=timespan.end as f32,
                    0.0..=(file.header.num_vars as f32 * LINE_SPACING),
                ),
                wave_rect,
            );

            let mut shapes = vec![];

            for (varid, wave) in cached_waves.iter() {
                let mut wave_to_screen =
                    to_screen.translated(Vec2::UP * (varid.0 as f32 * LINE_SPACING));
                // Invert Y.
                // TODO.

                draw_single_wave(
                    file.var_lengths.length(*varid),
                    wave,
                    wave_to_screen,
                    &mut shapes,
                    wave_colour,
                    x_colour,
                    0.0..1.0, // TODO
                );
            }

            ui.painter().extend(shapes);

            response
        })
        .inner
}

fn draw_timeline(ui: &mut Ui, time_range: Range<f64>, space: Rect) {
    let text = if ui.visuals().dark_mode {
        Color32::from_additive_luminance(196)
    } else {
        Color32::from_black_alpha(240)
    };

    let line = if ui.visuals().dark_mode {
        Color32::from_additive_luminance(128)
    } else {
        Color32::from_black_alpha(128)
    };

    // Order of magnitude to show.

    // TODO: I bet it's easier and faster just to loop through [1, 2, 5, 10, 20, 50, etc.]

    let time_span = time_range.end - time_range.start;

    let log_step = (time_span / space.width() as f64).log10();
    let log_step_floor = log_step.floor();
    let fact = match log_step - log_step_floor {
        x if x < 0.2 => 1.0, // TODO: These are rough numbers.
        x if x < 0.5 => 2.0,
        _ => 5.0,
    };

    let step = 50.0 * 10.0f64.powf(log_step_floor) * fact;

    // TODO: This isn't correct for negative numbers.
    let mut t = time_range.start.div_euclid(step) * step - step;

    while t < time_range.end + step {
        // Transform to screen space.
        let fraction = (t - time_range.start) / time_span;
        let x = space.left() + space.width() * fraction as f32;
        ui.painter().text(
            Pos2 {
                x,
                y: space.top() + 10.0,
            },
            Align2::CENTER_BOTTOM,
            format!("{}", t),
            FontId {
                size: 8.0,
                family: FontFamily::Proportional,
            },
            text,
        );

        ui.painter().line_segment(
            [
                Pos2 {
                    x,
                    y: space.top() + 20.0,
                },
                Pos2 {
                    x,
                    y: space.bottom(),
                },
            ],
            Stroke::new(1.0, line),
        );

        t += step;
    }
}

fn draw_single_wave(
    varlength: VarLength,
    wave: &Vec<(u64, fst::valvec::Value)>,
    to_screen: emath::RectTransform,
    shapes: &mut Vec<Shape>,
    wave_colour: Color32,
    // Colour for 'x' values.
    x_colour: Color32,
    time_range: Range<f64>,
) {
    match varlength {
        VarLength::Bits(bits) => {
            if bits == 1 {
                // The points for a green line. We draw this for the whole
                // wave even if there are X's. Then we draw red boxes over it
                // where there are X's.
                let mut points: Vec<Pos2> = Vec::with_capacity(wave.len() * 2);

                let mut prev_bit4 = None;

                for (time, value) in wave.iter() {
                    let bit4 = value.0[0] & 0b11;
                    let bit2 = bit4 & 0b1;
                    if let Some(prev_bit4) = prev_bit4 {
                        if bit4 == prev_bit4 {
                            continue;
                        }

                        let prev_bit2 = prev_bit4 & 0b1;

                        // Draw a vertical line.
                        points.push(to_screen * pos2(*time as f32, prev_bit2 as f32));
                        points.push(to_screen * pos2(*time as f32, bit2 as f32));
                    } else {
                        // First point.
                        points.push(to_screen * pos2(*time as f32, bit2 as f32));
                    }

                    prev_bit4 = Some(bit4);
                }

                // TODO: Draw to the end time.

                let thickness = 1.0;
                shapes.push(epaint::Shape::line(
                    points,
                    Stroke::new(thickness, wave_colour),
                ));
            } else {
                // Multiple bits get drawn like this:
                //
                // _____ⵃ⁐⁐⁐⁐X⁐⁐⁐⁐Ⲗ____
                //   0       1      2      0

                // When we get a split we can start a second line, and stop
                // one of them when we have a join.

                // Line 0: _____/⎺⎺⎺⎺\____/
                // Line 1:      \____/⎺⎺⎺⎺\___

                // We also draw the actual number inside the space (but for
                // the previous one because then we know how much space we have).

                let mut line_bottom: Vec<Pos2> = Vec::new();
                let mut line_top: Vec<Pos2> = Vec::new();

                let mut prev_value = None;
                let mut prev_is_zero = true;

                let thickness = 1.0;

                for (time, value) in wave.iter() {
                    // TODO: Have to do custom Eq here.
                    if Some(value) == prev_value {
                        continue;
                    }

                    let is_zero = value.0.iter().all(|b| *b == 0);

                    match (prev_is_zero, is_zero) {
                        (true, true) => {
                            // _
                            line_bottom.push(to_screen * pos2(*time as f32, 0.0));
                        }
                        (true, false) => {
                            // ⵃ
                            line_bottom.push(to_screen * pos2(*time as f32, 0.0));
                            line_bottom.push(to_screen * pos2(*time as f32, 1.0) + vec2(2.0, 0.0));
                            line_top.push(to_screen * pos2(*time as f32, 0.5) + vec2(1.0, 0.0));
                            line_top.push(to_screen * pos2(*time as f32, 0.0) + vec2(2.0, 0.0));
                            // Ensure line_bottom is still the bottom.
                            std::mem::swap(&mut line_top, &mut line_bottom);
                        }
                        (false, true) => {
                            // Ⲗ
                            line_top.push(to_screen * pos2(*time as f32, 1.0));
                            line_top.push(to_screen * pos2(*time as f32, 0.0) + vec2(2.0, 0.0));
                            line_bottom.push(to_screen * pos2(*time as f32, 0.0));
                            line_bottom.push(to_screen * pos2(*time as f32, 0.5) + vec2(1.0, 0.0));
                            // Ensure line_bottom is still the bottom.
                            std::mem::swap(&mut line_top, &mut line_bottom);

                            // The bottom (now top) line is finished.
                            shapes.push(epaint::Shape::line(
                                std::mem::take(&mut line_top),
                                Stroke::new(thickness, wave_colour),
                            ));
                        }
                        (false, false) => {
                            // X
                            line_bottom.push(to_screen * pos2(*time as f32, 0.0));
                            line_bottom.push(to_screen * pos2(*time as f32, 1.0) + vec2(2.0, 0.0));
                            line_top.push(to_screen * pos2(*time as f32, 1.0));
                            line_top.push(to_screen * pos2(*time as f32, 0.0) + vec2(2.0, 0.0));
                            // Ensure line_bottom is still the bottom.
                            std::mem::swap(&mut line_top, &mut line_bottom);
                        }
                    }

                    prev_value = Some(value);
                    prev_is_zero = is_zero;
                }

                // TODO: Draw to the end time.

                if !line_bottom.is_empty() {
                    shapes.push(epaint::Shape::line(
                        line_bottom,
                        Stroke::new(thickness, wave_colour),
                    ));
                }
                if !line_top.is_empty() {
                    shapes.push(epaint::Shape::line(
                        line_top,
                        Stroke::new(thickness, wave_colour),
                    ));
                }
            }
        }
        VarLength::Real => {
            // TODO
        }
    }
}
trait TransformTransform {
    fn translated(&self, v: Vec2) -> Self;
}

impl TransformTransform for RectTransform {
    fn translated(&self, v: Vec2) -> Self {
        Self::from_to(self.from().translate(v), *self.to())
    }
}
