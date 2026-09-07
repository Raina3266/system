//! The four dials on the system card.
//!
//! Wayle drew these with a widget from its own library, which is the one part
//! of that card a separate program cannot borrow, so it is drawn here.

use std::cell::Cell;
use std::f64::consts::PI;
use std::rc::Rc;

use gtk::cairo::{Context, LineCap};
use gtk::prelude::*;

/// Outer diameter in logical pixels, and how thick the ring is drawn. Four of
/// these sit across one column, so the diameter is what actually decides how
/// narrow the panel can be.
const DIAMETER: i32 = 38;
const THICKNESS: f64 = 3.0;

/// Where the arc starts: twelve o'clock rather than three.
const START: f64 = -PI / 2.0;

/// A dial's colour, as the stylesheet would have given it.
#[derive(Clone, Copy)]
pub struct Colour(pub f64, pub f64, pub f64);

/// A dial with its reading in the middle and a caption underneath.
pub struct Ring {
    pub widget: gtk::Box,
    fraction: Rc<Cell<f64>>,
    area: gtk::DrawingArea,
    value: gtk::Label,
}

impl Ring {
    pub fn new(caption: &str, colour: Colour) -> Self {
        let fraction = Rc::new(Cell::new(0.0));

        let value = gtk::Label::new(Some("--"));
        value.add_css_class("ring-value");

        let area = gtk::DrawingArea::new();
        area.set_content_width(DIAMETER);
        area.set_content_height(DIAMETER);
        area.set_draw_func({
            let fraction = Rc::clone(&fraction);
            move |_, context, width, height| {
                draw(context, width, height, fraction.get(), colour);
            }
        });

        // The reading sits inside the ring rather than beside it, so the card
        // stays narrow enough for four of these in a row.
        let overlay = gtk::Overlay::new();
        overlay.set_child(Some(&area));
        overlay.add_overlay(&value);

        let label = gtk::Label::new(Some(caption));
        label.add_css_class("ring-caption");

        let widget = gtk::Box::new(gtk::Orientation::Vertical, 2);
        widget.add_css_class("ring");
        widget.set_halign(gtk::Align::Center);
        widget.append(&overlay);
        widget.append(&label);

        Ring {
            widget,
            fraction,
            area,
            value,
        }
    }

    pub fn set(&self, fraction: f64, text: &str) {
        self.fraction.set(fraction.clamp(0.0, 1.0));
        self.value.set_label(text);
        self.area.queue_draw();
    }
}

fn draw(context: &Context, width: i32, height: i32, fraction: f64, colour: Colour) {
    let size = f64::from(width.min(height));
    let radius = (size - THICKNESS) / 2.0;
    let (centre_x, centre_y) = (f64::from(width) / 2.0, f64::from(height) / 2.0);

    context.set_line_width(THICKNESS);
    context.set_line_cap(LineCap::Round);

    // The track, so an idle ring still reads as a ring rather than as nothing.
    let Colour(red, green, blue) = colour;
    context.set_source_rgba(red, green, blue, 0.16);
    context.arc(centre_x, centre_y, radius, 0.0, 2.0 * PI);
    let _ = context.stroke();

    if fraction <= 0.0 {
        return;
    }

    context.set_source_rgba(red, green, blue, 1.0);
    context.arc(
        centre_x,
        centre_y,
        radius,
        START,
        START + 2.0 * PI * fraction,
    );
    let _ = context.stroke();
}
