//! Resolve core formatter requests at their semantic dispatch boundary.
use super::{ClipboardEvents, Core, Dimensions, Event};
use alacritty_terminal::{
    Term,
    event::WindowSize,
    vte::ansi::{Handler, Rgb},
};
pub(super) const MAX_QUERY_EVENTS: usize = 32;

fn query_size(term: &Term<ClipboardEvents>, cell_pixels: Option<(u16, u16)>) -> Option<WindowSize> {
    let (cell_width, cell_height) = cell_pixels?;
    let num_lines = u16::try_from(term.screen_lines()).ok()?;
    let num_cols = u16::try_from(term.columns()).ok()?;
    // The upstream formatter multiplies u16 values. Refuse unrepresentable areas.
    num_lines.checked_mul(cell_height)?;
    num_cols.checked_mul(cell_width)?;
    Some(WindowSize {
        num_lines,
        num_cols,
        cell_width,
        cell_height,
    })
}
fn answer_queries(
    term: &Term<ClipboardEvents>,
    events: &ClipboardEvents,
    cell_pixels: Option<(u16, u16)>,
) {
    let mut replies = events.replies.borrow_mut();
    for event in events.queries.borrow_mut().drain(..) {
        if replies.overflow {
            continue;
        }
        let reply = match event {
            Event::ColorRequest(index, formatter) => {
                crate::renderer::palette::query(index, term.colors()).map(|rgb| formatter(rgb))
            }
            Event::TextAreaSizeRequest(formatter) => {
                query_size(term, cell_pixels).map(|size| formatter(size))
            }
            _ => None, // ClipboardLoad is never a permitted request.
        };
        if let Some(reply) = reply {
            replies.push(reply.as_bytes());
        }
    }
}
impl Core {
    pub(super) fn answer_queries(&self) {
        answer_queries(&self.term, &self.events, self.cell_pixels);
    }
}

/// Only used for the terminating byte of OSC 4. VTE parses all its pairs in one
/// step, so answer each query before a later pair changes the same palette entry.
/// VTE still owns parsing, validation and actual Term mutation; no color parser is duplicated.
pub(super) struct PaletteDispatch<'a> {
    pub term: &'a mut Term<ClipboardEvents>,
    pub events: &'a ClipboardEvents,
    pub cell_pixels: Option<(u16, u16)>,
}
impl Handler for PaletteDispatch<'_> {
    fn set_color(&mut self, index: usize, rgb: Rgb) {
        self.term.set_color(index, rgb);
    }
    fn dynamic_color_sequence(&mut self, prefix: String, index: usize, terminator: &str) {
        self.term.dynamic_color_sequence(prefix, index, terminator);
        answer_queries(self.term, self.events, self.cell_pixels);
    }
    // VTE executes SUB after dispatching an OSC ended by that control character.
    fn substitute(&mut self) {
        self.term.substitute();
    }
}
