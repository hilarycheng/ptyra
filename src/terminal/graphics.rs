use std::{cell::Cell, rc::Rc};
const BASE_BYTES: usize = 16 * 1024;
const COMMAND_BYTES: usize = 128 * 1024;
const DECODE_WORK: usize = 32 * 1024 * 1024;
pub(crate) const GLOBAL_BYTES: usize = 128 * 1024 * 1024;
pub(crate) struct Budget {
    used: Cell<usize>,
    limit: usize,
}
impl Budget {
    pub fn new(limit: usize) -> Self {
        Self {
            used: Cell::new(0),
            limit,
        }
    }
    fn reserve(&self, bytes: usize) -> Result<(), AppError> {
        let used = self.used.get();
        if bytes > self.limit.saturating_sub(used) {
            return Err(error(std::io::ErrorKind::OutOfMemory));
        }
        self.used.set(used + bytes);
        Ok(())
    }
}
use crate::{
    TabId,
    error::AppError,
    kitty::graphics::{
        GraphicsCommand, GraphicsParser,
        decode::{ImageLimits, decode_with_limits},
        store::ImageStore,
        transfer::Assembler,
    },
};

pub(super) struct Graphics {
    parser: GraphicsParser,
    transfer: Assembler,
    pub store: ImageStore,
    limits: ImageLimits,
    pub dirty: bool,
    budget: Rc<Budget>,
    held: usize,
    pub work: usize,
}
impl Graphics {
    pub fn new(tab: TabId, limits: ImageLimits) -> Result<Self, AppError> {
        Ok(Self {
            parser: GraphicsParser::default(),
            transfer: Assembler::with_limit(limits.bytes())?,
            store: ImageStore::new(tab, limits, limits.bytes())?,
            limits,
            dirty: false,
            budget: Rc::new(Budget {
                used: Cell::new(BASE_BYTES),
                limit: GLOBAL_BYTES,
            }),
            held: BASE_BYTES,
            work: 0,
        })
    }
    pub fn bind_budget(&mut self, budget: Rc<Budget>) -> Result<(), AppError> {
        budget.reserve(self.held)?;
        self.budget.used.set(self.budget.used.get() - self.held);
        self.budget = budget;
        Ok(())
    }
    fn settle(&mut self, temporary: usize) {
        let held = BASE_BYTES + self.store.allocated_bytes() + self.transfer.allocated_bytes();
        self.budget
            .used
            .set(self.budget.used.get() - self.held - temporary + held);
        self.held = held;
    }
    pub fn push(
        &mut self,
        byte: u8,
        term: &mut alacritty_terminal::Term<super::ClipboardEvents>,
        cell: Option<(u16, u16)>,
        events: &super::ClipboardEvents,
    ) -> Result<(), AppError> {
        if matches!(byte, 0x18 | 0x1a) {
            self.transfer.cancel();
            self.settle(0);
        }
        match self.parser.push(byte) {
            Ok(Some(command)) => {
                let mut header = self.transfer.header().unwrap_or(&command.header).clone();
                if let Some(q) = command.header.get(b'q') {
                    header.set_quiet(q);
                }
                let decode = command.header.value(b'm', 0) == 0
                    && matches!(
                        header.value(b'a', i64::from(b't')) as u8,
                        b't' | b'T' | b'q'
                    );
                // Reserve the replacement allocation before a pending buffer can grow.
                let temporary = COMMAND_BYTES
                    + if decode {
                        4 * self.limits.bytes()
                    } else {
                        self.transfer.allocated_bytes()
                    };
                let result = match self.budget.reserve(temporary) {
                    Ok(()) => {
                        let result = self.command(command, term, cell);
                        self.settle(temporary);
                        result
                    }
                    Err(error) => Err(error),
                };
                match result {
                    Ok(true) => reply(events, &header, None),
                    Ok(false) => {}
                    Err(error) => {
                        self.transfer.cancel();
                        self.settle(0);
                        reply(events, &header, Some(error));
                    }
                }
            }
            Err(error) => {
                let header = self.parser.recover(byte);
                self.transfer.cancel();
                self.settle(0);
                reply(events, &header, Some(error));
            }
            _ => {}
        }
        Ok(())
    }
    pub fn clear_screen(&mut self, screen: crate::selection::Screen) {
        self.transfer.cancel();
        self.dirty |= self.store.clear_screen(screen);
        self.settle(0);
    }
    pub fn reset(&mut self) {
        self.transfer.cancel();
        self.store = ImageStore::new(self.store.tab(), self.limits, self.limits.bytes())
            .expect("validated image limits");
        self.parser = GraphicsParser::default();
        self.dirty = true;
        self.settle(0);
    }
    fn command(
        &mut self,
        command: GraphicsCommand,
        term: &mut alacritty_terminal::Term<super::ClipboardEvents>,
        cell: Option<(u16, u16)>,
    ) -> Result<bool, AppError> {
        use crate::kitty::graphics::store::ImageId;
        let mut header = command.header.clone();
        let mut action = header.value(b'a', i64::from(b't')) as u8;
        if action == b'd' {
            if !command.payload.is_empty() {
                return Err(bad());
            }
            self.store.delete(
                &header,
                crate::selection::Screen::from_mode(term.mode()),
                &mut self.transfer,
            )?;
            self.dirty = true;
            return Ok(true);
        }
        let (id, geometry) = if action == b'p' {
            if !command.payload.is_empty() || self.transfer.header().is_some() {
                return Err(bad());
            }
            let id = ImageId::Named(header.get(b'i').ok_or_else(bad)? as u32);
            let image = self
                .store
                .get(id)
                .ok_or_else(|| error(std::io::ErrorKind::NotFound))?;
            (
                id,
                Some(geometry(&header, image.width(), image.height(), cell)?),
            )
        } else {
            let Some(complete) = self.transfer.push(command)? else {
                return Ok(false);
            };
            header = complete.header.clone();
            action = header.value(b'a', i64::from(b't')) as u8;
            let dimensions = if header.value(b'f', 32) == 100 && complete.bytes.len() >= 24 {
                let w = u32::from_be_bytes(complete.bytes[16..20].try_into().map_err(|_| bad())?);
                let h = u32::from_be_bytes(complete.bytes[20..24].try_into().map_err(|_| bad())?);
                (w, h)
            } else {
                (header.value(b's', 0) as u32, header.value(b'v', 0) as u32)
            };
            let cost =
                self.limits.check_dimensions(dimensions.0, dimensions.1)? + complete.bytes.len();
            if cost > DECODE_WORK.saturating_sub(self.work) {
                return Err(error(std::io::ErrorKind::FileTooLarge));
            }
            self.work += cost;
            let image = decode_with_limits(complete, self.limits)?;
            if action == b'q' {
                return Ok(true);
            }
            let geometry = (action == b'T')
                .then(|| geometry(image.header(), image.width(), image.height(), cell))
                .transpose()?;
            let id = self.store.put(image)?;
            self.dirty = true;
            (id, geometry)
        };
        if let Some((source, offset, pixels, extent)) = geometry {
            let used: u64 = self
                .store
                .placements()
                .iter()
                .map(|p| u64::from(p.pixels.0) * u64::from(p.pixels.1))
                .sum();
            if used + u64::from(pixels.0) * u64::from(pixels.1) > 8 * 1024 * 1024 {
                return Err(error(std::io::ErrorKind::FileTooLarge));
            }
            use alacritty_terminal::{
                grid::Dimensions,
                index::{Column, Line},
            };
            let point = term.grid().cursor.point;
            let rect = crate::damage::Rect {
                x: point.column.0 as i32,
                y: point.line.0,
                width: extent.0,
                height: extent.1,
            };
            let pid = self.store.place(
                id,
                header.get(b'p').map(|n| n as u32),
                crate::selection::Screen::from_mode(term.mode()),
                rect,
            )?;
            let placement = self
                .store
                .placements_mut()
                .iter_mut()
                .find(|p| p.image == id && p.id == pid)
                .ok_or_else(bad)?;
            placement.source = source;
            placement.offset = offset;
            placement.pixels = pixels;
            placement.z = header.value(b'z', 0) as i32;
            let cell = cell.ok_or_else(bad)?;
            placement.cell = (u32::from(cell.0), u32::from(cell.1));
            if header.value(b'C', 0) == 0 {
                let row = (point.line.0 as u32 + extent.1).min(term.screen_lines() as u32 - 1);
                let col = (point.column.0 as u32 + extent.0).min(term.columns() as u32 - 1);
                term.grid_mut().cursor.point.line = Line(row as i32);
                term.grid_mut().cursor.point.column = Column(col as usize);
                term.grid_mut().cursor.input_needs_wrap = false;
            }
            self.dirty = true;
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests;

fn error(kind: std::io::ErrorKind) -> AppError {
    AppError::new(crate::error::Component::Terminal, kind)
}
fn bad() -> AppError {
    error(std::io::ErrorKind::InvalidData)
}
type Geometry = (crate::damage::Rect, (u32, u32), (u32, u32), (u32, u32));
fn geometry(
    h: &crate::kitty::graphics::Header,
    width: u32,
    height: u32,
    cell: Option<(u16, u16)>,
) -> Result<Geometry, AppError> {
    let (cw, ch) = cell.ok_or_else(bad)?;
    let (cw, ch) = (u32::from(cw), u32::from(ch));
    if h.value(b'C', 0) > 1 || h.value(b'z', 0) < -1_073_741_823 {
        return Err(error(std::io::ErrorKind::Unsupported));
    }
    let (x, y) = (h.value(b'x', 0) as u32, h.value(b'y', 0) as u32);
    if x >= width || y >= height {
        return Err(bad());
    }
    let w = h.value(b'w', 0) as u32;
    let hgt = h.value(b'h', 0) as u32;
    let w = if w == 0 { width - x } else { w.min(width - x) };
    let hgt = if hgt == 0 {
        height - y
    } else {
        hgt.min(height - y)
    };
    let (ox, oy) = (h.value(b'X', 0) as u32, h.value(b'Y', 0) as u32);
    if ox >= cw || oy >= ch {
        return Err(bad());
    }
    let (cols, rows) = (h.value(b'c', 0) as u32, h.value(b'r', 0) as u32);
    if cols > 4096 || rows > 4096 {
        return Err(bad());
    }
    let (boxw, boxh) = match (cols, rows) {
        (0, 0) => (w, hgt),
        (c, 0) => (
            c * cw - ox,
            u32::try_from((u64::from(c * cw - ox) * u64::from(hgt) / u64::from(w)).max(1))
                .map_err(|_| bad())?,
        ),
        (0, r) => (
            u32::try_from((u64::from(r * ch - oy) * u64::from(w) / u64::from(hgt)).max(1))
                .map_err(|_| bad())?,
            r * ch - oy,
        ),
        (c, r) => (c * cw - ox, r * ch - oy),
    };
    if u64::from(boxw) * u64::from(boxh) > 8 * 1024 * 1024 {
        return Err(error(std::io::ErrorKind::FileTooLarge));
    }
    let (dw, dh) = if u64::from(boxw) * u64::from(hgt) <= u64::from(boxh) * u64::from(w) {
        (
            boxw,
            (u64::from(boxw) * u64::from(hgt) / u64::from(w)).max(1) as u32,
        )
    } else {
        (
            (u64::from(boxh) * u64::from(w) / u64::from(hgt)).max(1) as u32,
            boxh,
        )
    };
    let extent = ((boxw + ox).div_ceil(cw), (boxh + oy).div_ceil(ch));
    Ok((
        crate::damage::Rect {
            x: x as i32,
            y: y as i32,
            width: w,
            height: hgt,
        },
        (ox + (boxw - dw) / 2, oy + (boxh - dh) / 2),
        (dw, dh),
        extent,
    ))
}

fn reply(
    events: &super::ClipboardEvents,
    header: &crate::kitty::graphics::Header,
    error: Option<AppError>,
) {
    let quiet = header.value(b'q', 0);
    if quiet == 2 || quiet == 1 && error.is_none() {
        return;
    }
    let Some(id) = header.get(b'i').filter(|id| *id > 0) else {
        return;
    };
    use std::io::ErrorKind;
    let status = match error.map(|e| e.kind()) {
        None => "OK",
        Some(ErrorKind::PermissionDenied) => "EACCES:transport disabled",
        Some(ErrorKind::NotFound) => "ENOENT:image unavailable",
        Some(ErrorKind::Unsupported) => "ENOTSUP:unsupported command",
        Some(ErrorKind::FileTooLarge | ErrorKind::OutOfMemory) => "E2BIG:resource limit",
        _ => "EINVAL:invalid data",
    };
    let placement = header
        .get(b'p')
        .filter(|id| *id > 0)
        .map(|id| format!(",p={id}"))
        .unwrap_or_default();
    events
        .replies
        .borrow_mut()
        .push(format!("\x1b_Gi={id}{placement};{status}\x1b\\").as_bytes());
}

impl Drop for Graphics {
    fn drop(&mut self) {
        self.budget.used.set(self.budget.used.get() - self.held);
    }
}
