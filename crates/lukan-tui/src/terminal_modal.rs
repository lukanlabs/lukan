use anyhow::{Context, Result};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};
use std::io::Write;
use tokio::sync::mpsc;
use unicode_width::UnicodeWidthChar;

// ──────────────────────────────────────────────────────────────────────────────
// ANSI Screen Emulator
// ──────────────────────────────────────────────────────────────────────────────

/// Terminal cell color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellColor {
    Default,
    Idx(u8),
    Rgb(u8, u8, u8),
}

/// A single character cell in the terminal grid.
#[derive(Debug, Clone)]
pub struct Cell {
    pub ch: char,
    pub fg: CellColor,
    pub bg: CellColor,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
    /// This cell is the trailing column of a wide (CJK) character.
    pub wide_continuation: bool,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            ch: ' ',
            fg: CellColor::Default,
            bg: CellColor::Default,
            bold: false,
            dim: false,
            italic: false,
            underline: false,
            inverse: false,
            wide_continuation: false,
        }
    }
}

/// Current text attributes applied to new characters.
#[derive(Debug, Clone)]
struct Attrs {
    fg: CellColor,
    bg: CellColor,
    bold: bool,
    dim: bool,
    italic: bool,
    underline: bool,
    inverse: bool,
}

impl Default for Attrs {
    fn default() -> Self {
        Self {
            fg: CellColor::Default,
            bg: CellColor::Default,
            bold: false,
            dim: false,
            italic: false,
            underline: false,
            inverse: false,
        }
    }
}

/// ANSI escape parser state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParseState {
    Normal,
    /// Got `\x1b`, waiting for next byte.
    Escape,
    /// Inside a CSI sequence (`\x1b[`).
    Csi,
    /// Inside an OSC sequence (`\x1b]`), eat until ST.
    Osc,
    /// Eat exactly one more byte then return to Normal (e.g. charset designation `\x1b(B`).
    EatOne,
}

/// Minimal VT100/xterm screen emulator.
pub struct Screen {
    rows: u16,
    cols: u16,
    grid: Vec<Vec<Cell>>,
    cursor_row: u16,
    cursor_col: u16,
    attrs: Attrs,
    saved_cursor: Option<(u16, u16, Attrs)>,
    scroll_top: u16,
    scroll_bottom: u16,
    /// Alternate screen buffer (for vim, htop, etc.)
    alt_grid: Option<Vec<Vec<Cell>>>,
    alt_cursor: Option<(u16, u16)>,
    alt_attrs: Option<Attrs>,
    /// Whether to auto-wrap at right margin.
    autowrap: bool,
    /// A character was placed at the last column; next printable wraps.
    pending_wrap: bool,
    /// Cursor visible flag (`?25h` / `?25l`).
    pub cursor_visible: bool,
    /// Parser state.
    state: ParseState,
    /// Accumulator for CSI parameter bytes.
    csi_buf: Vec<u8>,
    /// Partial UTF-8 bytes.
    utf8_buf: Vec<u8>,
    /// Response bytes to write back to the PTY (e.g. cursor-position report).
    pub responses: Vec<Vec<u8>>,
    /// Scrollback buffer — lines that scrolled off the top.
    scrollback: Vec<Vec<Cell>>,
    /// How many lines the user has scrolled back (0 = live view).
    pub scroll_offset: usize,
    /// Max scrollback lines kept.
    scrollback_limit: usize,
}

impl Screen {
    pub fn new(rows: u16, cols: u16) -> Self {
        let grid = (0..rows).map(|_| Self::blank_row(cols)).collect();
        Self {
            rows,
            cols,
            grid,
            cursor_row: 0,
            cursor_col: 0,
            attrs: Attrs::default(),
            saved_cursor: None,
            scroll_top: 0,
            scroll_bottom: rows.saturating_sub(1),
            alt_grid: None,
            alt_cursor: None,
            alt_attrs: None,
            autowrap: true,
            pending_wrap: false,
            cursor_visible: true,
            state: ParseState::Normal,
            csi_buf: Vec::new(),
            utf8_buf: Vec::new(),
            responses: Vec::new(),
            scrollback: Vec::new(),
            scroll_offset: 0,
            scrollback_limit: 5000,
        }
    }

    pub fn size(&self) -> (u16, u16) {
        (self.rows, self.cols)
    }

    pub fn cell(&self, row: u16, col: u16) -> Option<&Cell> {
        self.grid
            .get(row as usize)
            .and_then(|r| r.get(col as usize))
    }

    pub fn cursor_position(&self) -> (u16, u16) {
        (self.cursor_row, self.cursor_col)
    }

    pub fn set_size(&mut self, rows: u16, cols: u16) {
        self.rows = rows;
        self.cols = cols;
        self.grid
            .resize_with(rows as usize, || Self::blank_row(cols));
        for row in &mut self.grid {
            row.resize_with(cols as usize, Cell::default);
        }
        self.scroll_bottom = rows.saturating_sub(1);
        if self.cursor_row >= rows {
            self.cursor_row = rows.saturating_sub(1);
        }
        if self.cursor_col >= cols {
            self.cursor_col = cols.saturating_sub(1);
        }
        self.pending_wrap = false;
    }

    /// Feed raw bytes from the PTY into the emulator.
    pub fn process(&mut self, bytes: &[u8]) {
        // New output snaps to live view
        if !bytes.is_empty() {
            self.scroll_offset = 0;
        }
        for &b in bytes {
            match self.state {
                ParseState::Normal => {
                    if b == 0x1b {
                        self.state = ParseState::Escape;
                    } else if b < 0x20 || b == 0x7f {
                        self.control(b);
                    } else if b < 0x80 {
                        self.put_char(b as char);
                    } else {
                        // UTF-8 multi-byte
                        self.utf8_buf.push(b);
                        if let Ok(s) = std::str::from_utf8(&self.utf8_buf) {
                            let ch = s.chars().next().unwrap_or(' ');
                            self.put_char(ch);
                            self.utf8_buf.clear();
                        } else if self.utf8_buf.len() >= 4 {
                            // Invalid sequence, discard
                            self.utf8_buf.clear();
                        }
                    }
                }
                ParseState::Escape => self.escape(b),
                ParseState::Csi => self.csi_byte(b),
                ParseState::Osc => {
                    // Eat until BEL (\x07) or ST (\x1b\\)
                    if b == 0x07 || b == 0x9c {
                        self.state = ParseState::Normal;
                    } else if b == 0x1b {
                        // Could be start of ST (\x1b\\) — eat one more byte
                        self.state = ParseState::EatOne;
                    }
                }
                ParseState::EatOne => {
                    // Consume this byte and return to Normal
                    self.state = ParseState::Normal;
                }
            }
        }
    }

    // ── Control characters ──────────────────────────────────────────

    fn control(&mut self, b: u8) {
        match b {
            0x07 => {} // BEL — ignore
            0x08 => {
                // BS — cursor left
                if self.cursor_col > 0 {
                    self.cursor_col -= 1;
                }
                self.pending_wrap = false;
            }
            0x09 => {
                // TAB — advance to next 8-column stop
                let next = ((self.cursor_col / 8) + 1) * 8;
                self.cursor_col = next.min(self.cols.saturating_sub(1));
                self.pending_wrap = false;
            }
            0x0A..=0x0C => {
                // LF / VT / FF — line feed
                self.linefeed();
                self.pending_wrap = false;
            }
            0x0D => {
                // CR — carriage return
                self.cursor_col = 0;
                self.pending_wrap = false;
            }
            _ => {}
        }
    }

    fn linefeed(&mut self) {
        if self.cursor_row == self.scroll_bottom {
            self.scroll_up_region(1);
        } else if self.cursor_row + 1 < self.rows {
            self.cursor_row += 1;
        }
    }

    // ── Escape entry ────────────────────────────────────────────────

    fn escape(&mut self, b: u8) {
        match b {
            b'[' => {
                self.state = ParseState::Csi;
                self.csi_buf.clear();
            }
            b']' => {
                self.state = ParseState::Osc;
            }
            b'7' => {
                // DECSC — save cursor
                self.saved_cursor = Some((self.cursor_row, self.cursor_col, self.attrs.clone()));
                self.state = ParseState::Normal;
            }
            b'8' => {
                // DECRC — restore cursor
                if let Some((r, c, a)) = self.saved_cursor.clone() {
                    self.cursor_row = r.min(self.rows.saturating_sub(1));
                    self.cursor_col = c.min(self.cols.saturating_sub(1));
                    self.attrs = a;
                }
                self.state = ParseState::Normal;
            }
            b'c' => {
                // RIS — full reset
                *self = Self::new(self.rows, self.cols);
            }
            b'D' => {
                // IND — linefeed
                self.linefeed();
                self.state = ParseState::Normal;
            }
            b'M' => {
                // RI — reverse index (scroll down if at top of scroll region)
                if self.cursor_row == self.scroll_top {
                    self.scroll_down_region(1);
                } else if self.cursor_row > 0 {
                    self.cursor_row -= 1;
                }
                self.state = ParseState::Normal;
            }
            b'(' | b')' | b'*' | b'+' | b'-' | b'.' | b'/' => {
                // Character set designation — eat the next byte (the charset ID)
                self.state = ParseState::EatOne;
            }
            b'=' | b'>' => {
                // DECKPAM / DECKPNM — keypad mode, ignore
                self.state = ParseState::Normal;
            }
            _ => {
                self.state = ParseState::Normal;
            }
        }
    }

    // ── CSI sequence accumulation ───────────────────────────────────

    fn csi_byte(&mut self, b: u8) {
        if (0x20..=0x3f).contains(&b) {
            // Parameter/intermediate bytes
            self.csi_buf.push(b);
        } else if (0x40..=0x7e).contains(&b) {
            // Final byte — execute
            self.execute_csi(b);
            self.state = ParseState::Normal;
        } else {
            // Unexpected, abort
            self.state = ParseState::Normal;
        }
    }

    fn parse_csi_params(&self) -> (bool, Vec<u16>) {
        let buf = &self.csi_buf;
        let (private, start) = if buf.first() == Some(&b'?') {
            (true, 1)
        } else {
            (false, 0)
        };
        let params: Vec<u16> = if start >= buf.len() {
            vec![]
        } else {
            buf[start..]
                .split(|&b| b == b';')
                .map(|part| {
                    part.iter().fold(0u16, |acc, &b| {
                        if b.is_ascii_digit() {
                            acc.saturating_mul(10).saturating_add((b - b'0') as u16)
                        } else {
                            acc
                        }
                    })
                })
                .collect()
        };
        (private, params)
    }

    fn execute_csi(&mut self, final_byte: u8) {
        let (private, params) = self.parse_csi_params();
        let p0 = params.first().copied().unwrap_or(0);
        let p1 = params.get(1).copied().unwrap_or(0);

        match final_byte {
            b'A' => {
                // CUU — cursor up
                let n = p0.max(1);
                self.cursor_row = self.cursor_row.saturating_sub(n);
                self.pending_wrap = false;
            }
            b'B' => {
                // CUD — cursor down
                let n = p0.max(1);
                self.cursor_row = (self.cursor_row + n).min(self.rows.saturating_sub(1));
                self.pending_wrap = false;
            }
            b'C' => {
                // CUF — cursor forward
                let n = p0.max(1);
                self.cursor_col = (self.cursor_col + n).min(self.cols.saturating_sub(1));
                self.pending_wrap = false;
            }
            b'D' => {
                // CUB — cursor back
                let n = p0.max(1);
                self.cursor_col = self.cursor_col.saturating_sub(n);
                self.pending_wrap = false;
            }
            b'E' => {
                // CNL — cursor next line
                let n = p0.max(1);
                self.cursor_row = (self.cursor_row + n).min(self.rows.saturating_sub(1));
                self.cursor_col = 0;
                self.pending_wrap = false;
            }
            b'F' => {
                // CPL — cursor previous line
                let n = p0.max(1);
                self.cursor_row = self.cursor_row.saturating_sub(n);
                self.cursor_col = 0;
                self.pending_wrap = false;
            }
            b'G' => {
                // CHA — cursor horizontal absolute
                let col = p0.max(1) - 1;
                self.cursor_col = col.min(self.cols.saturating_sub(1));
                self.pending_wrap = false;
            }
            b'H' | b'f' => {
                // CUP — cursor position
                let row = p0.max(1) - 1;
                let col = p1.max(1) - 1;
                self.cursor_row = row.min(self.rows.saturating_sub(1));
                self.cursor_col = col.min(self.cols.saturating_sub(1));
                self.pending_wrap = false;
            }
            b'J' => {
                // ED — erase display
                self.erase_display(p0);
            }
            b'K' => {
                // EL — erase line
                self.erase_line(p0);
            }
            b'L' => {
                // IL — insert lines
                let n = p0.max(1);
                self.insert_lines(n);
            }
            b'M' => {
                // DL — delete lines
                let n = p0.max(1);
                self.delete_lines(n);
            }
            b'P' => {
                // DCH — delete characters
                let n = p0.max(1) as usize;
                let row = self.cursor_row as usize;
                let col = self.cursor_col as usize;
                let cols = self.cols as usize;
                if row < self.grid.len() {
                    let line = &mut self.grid[row];
                    for _ in 0..n {
                        if col < line.len() {
                            line.remove(col);
                            line.push(Cell::default());
                        }
                    }
                    line.truncate(cols);
                }
            }
            b'@' => {
                // ICH — insert characters
                let n = p0.max(1) as usize;
                let row = self.cursor_row as usize;
                let col = self.cursor_col as usize;
                let cols = self.cols as usize;
                if row < self.grid.len() {
                    let line = &mut self.grid[row];
                    for _ in 0..n {
                        line.insert(col, Cell::default());
                    }
                    line.truncate(cols);
                }
            }
            b'X' => {
                // ECH — erase characters
                let n = p0.max(1) as usize;
                let row = self.cursor_row as usize;
                let col = self.cursor_col as usize;
                if row < self.grid.len() {
                    for i in col..((col + n).min(self.cols as usize)) {
                        if i < self.grid[row].len() {
                            self.grid[row][i] = Cell::default();
                        }
                    }
                }
            }
            b'S' if !private => {
                // SU — scroll up
                let n = p0.max(1);
                self.scroll_up_region(n);
            }
            b'T' if !private => {
                // SD — scroll down
                let n = p0.max(1);
                self.scroll_down_region(n);
            }
            b'd' => {
                // VPA — cursor vertical absolute
                let row = p0.max(1) - 1;
                self.cursor_row = row.min(self.rows.saturating_sub(1));
                self.pending_wrap = false;
            }
            b'm' => {
                // SGR — select graphic rendition
                self.apply_sgr(&params);
            }
            b'r' if !private => {
                // DECSTBM — set scroll region
                let top = p0.max(1) - 1;
                let bot = if p1 == 0 {
                    self.rows.saturating_sub(1)
                } else {
                    (p1 - 1).min(self.rows.saturating_sub(1))
                };
                if top < bot {
                    self.scroll_top = top;
                    self.scroll_bottom = bot;
                }
                self.cursor_row = 0;
                self.cursor_col = 0;
                self.pending_wrap = false;
            }
            b's' if !private => {
                // SCP — save cursor position
                self.saved_cursor = Some((self.cursor_row, self.cursor_col, self.attrs.clone()));
            }
            b'u' if !private => {
                // RCP — restore cursor position
                if let Some((r, c, a)) = self.saved_cursor.clone() {
                    self.cursor_row = r.min(self.rows.saturating_sub(1));
                    self.cursor_col = c.min(self.cols.saturating_sub(1));
                    self.attrs = a;
                }
                self.pending_wrap = false;
            }
            b'h' if private => {
                // DECSET
                for &p in &params {
                    match p {
                        7 => self.autowrap = true,
                        25 => self.cursor_visible = true,
                        1049 => self.enter_alt_screen(),
                        _ => {}
                    }
                }
            }
            b'l' if private => {
                // DECRST
                for &p in &params {
                    match p {
                        7 => self.autowrap = false,
                        25 => self.cursor_visible = false,
                        1049 => self.leave_alt_screen(),
                        _ => {}
                    }
                }
            }
            b'n' if !private && p0 == 6 => {
                // DSR — cursor position report
                let resp = format!("\x1b[{};{}R", self.cursor_row + 1, self.cursor_col + 1);
                self.responses.push(resp.into_bytes());
            }
            b'c' if !private => {
                // DA — device attributes
                self.responses.push(b"\x1b[?1;2c".to_vec());
            }
            _ => {} // Unknown — ignore
        }
    }

    // ── SGR (colors / attributes) ───────────────────────────────────

    fn apply_sgr(&mut self, params: &[u16]) {
        if params.is_empty() {
            self.attrs = Attrs::default();
            return;
        }
        let mut i = 0;
        while i < params.len() {
            match params[i] {
                0 => self.attrs = Attrs::default(),
                1 => self.attrs.bold = true,
                2 => self.attrs.dim = true,
                3 => self.attrs.italic = true,
                4 => self.attrs.underline = true,
                7 => self.attrs.inverse = true,
                22 => {
                    self.attrs.bold = false;
                    self.attrs.dim = false;
                }
                23 => self.attrs.italic = false,
                24 => self.attrs.underline = false,
                27 => self.attrs.inverse = false,
                // Standard foreground colors
                30..=37 => self.attrs.fg = CellColor::Idx((params[i] - 30) as u8),
                38 => {
                    i += 1;
                    self.attrs.fg = self.parse_extended_color(params, &mut i);
                    continue; // i already advanced
                }
                39 => self.attrs.fg = CellColor::Default,
                // Standard background colors
                40..=47 => self.attrs.bg = CellColor::Idx((params[i] - 40) as u8),
                48 => {
                    i += 1;
                    self.attrs.bg = self.parse_extended_color(params, &mut i);
                    continue;
                }
                49 => self.attrs.bg = CellColor::Default,
                // Bright foreground
                90..=97 => self.attrs.fg = CellColor::Idx((params[i] - 90 + 8) as u8),
                // Bright background
                100..=107 => self.attrs.bg = CellColor::Idx((params[i] - 100 + 8) as u8),
                _ => {}
            }
            i += 1;
        }
    }

    fn parse_extended_color(&self, params: &[u16], i: &mut usize) -> CellColor {
        if *i >= params.len() {
            return CellColor::Default;
        }
        match params[*i] {
            5 => {
                // 256-color: 38;5;n
                *i += 1;
                if *i < params.len() {
                    let idx = params[*i] as u8;
                    *i += 1;
                    CellColor::Idx(idx)
                } else {
                    *i += 1;
                    CellColor::Default
                }
            }
            2 => {
                // Truecolor: 38;2;r;g;b
                *i += 1;
                if *i + 2 < params.len() {
                    let r = params[*i] as u8;
                    let g = params[*i + 1] as u8;
                    let b = params[*i + 2] as u8;
                    *i += 3;
                    CellColor::Rgb(r, g, b)
                } else {
                    *i = params.len();
                    CellColor::Default
                }
            }
            _ => {
                *i += 1;
                CellColor::Default
            }
        }
    }

    // ── Screen operations ───────────────────────────────────────────

    fn put_char(&mut self, ch: char) {
        let width = ch.width().unwrap_or(0) as u16;
        if width == 0 {
            return; // Control / zero-width, skip
        }

        if self.pending_wrap && self.autowrap {
            self.cursor_col = 0;
            self.linefeed();
            self.pending_wrap = false;
        }

        // Place character
        let r = self.cursor_row as usize;
        let c = self.cursor_col as usize;
        if r < self.grid.len() && c < self.grid[r].len() {
            self.grid[r][c] = Cell {
                ch,
                fg: self.attrs.fg,
                bg: self.attrs.bg,
                bold: self.attrs.bold,
                dim: self.attrs.dim,
                italic: self.attrs.italic,
                underline: self.attrs.underline,
                inverse: self.attrs.inverse,
                wide_continuation: false,
            };
            // Mark second column for wide chars
            if width == 2 && c + 1 < self.grid[r].len() {
                self.grid[r][c + 1] = Cell {
                    wide_continuation: true,
                    ..Cell::default()
                };
            }
        }

        // Advance cursor
        let advance = width;
        if self.cursor_col + advance >= self.cols {
            self.cursor_col = self.cols.saturating_sub(1);
            self.pending_wrap = true;
        } else {
            self.cursor_col += advance;
        }
    }

    fn erase_display(&mut self, mode: u16) {
        let r = self.cursor_row as usize;
        let c = self.cursor_col as usize;
        match mode {
            0
                // Erase below (including cursor position to end)
                if r < self.grid.len() => {
                    for cell in &mut self.grid[r][c..] {
                        *cell = Cell::default();
                    }
                    for row in &mut self.grid[(r + 1)..] {
                        for cell in row.iter_mut() {
                            *cell = Cell::default();
                        }
                    }
                }
            1 => {
                // Erase above (including start to cursor position)
                for row in &mut self.grid[..r] {
                    for cell in row.iter_mut() {
                        *cell = Cell::default();
                    }
                }
                if r < self.grid.len() {
                    let end = c.min(self.grid[r].len().saturating_sub(1));
                    for cell in &mut self.grid[r][..=end] {
                        *cell = Cell::default();
                    }
                }
            }
            2 | 3 => {
                // Erase all
                for row in &mut self.grid {
                    for cell in row.iter_mut() {
                        *cell = Cell::default();
                    }
                }
            }
            _ => {}
        }
    }

    fn erase_line(&mut self, mode: u16) {
        let r = self.cursor_row as usize;
        let c = self.cursor_col as usize;
        if r >= self.grid.len() {
            return;
        }
        let line = &mut self.grid[r];
        match mode {
            0 => {
                for cell in &mut line[c..] {
                    *cell = Cell::default();
                }
            }
            1 => {
                let end = c.min(line.len().saturating_sub(1));
                for cell in &mut line[..=end] {
                    *cell = Cell::default();
                }
            }
            2 => {
                for cell in line.iter_mut() {
                    *cell = Cell::default();
                }
            }
            _ => {}
        }
    }

    fn scroll_up_region(&mut self, n: u16) {
        let top = self.scroll_top as usize;
        let bot = self.scroll_bottom as usize;
        let n = (n as usize).min(bot - top + 1);
        for _ in 0..n {
            if top < self.grid.len() && bot < self.grid.len() {
                let removed = self.grid.remove(top);
                // Save to scrollback only when scrolling the full screen region
                // (not a sub-region like a scroll area inside vim)
                if self.scroll_top == 0 && self.alt_grid.is_none() {
                    self.scrollback.push(removed);
                    if self.scrollback.len() > self.scrollback_limit {
                        self.scrollback.remove(0);
                    }
                }
                self.grid.insert(bot, Self::blank_row(self.cols));
            }
        }
    }

    fn scroll_down_region(&mut self, n: u16) {
        let top = self.scroll_top as usize;
        let bot = self.scroll_bottom as usize;
        let n = (n as usize).min(bot - top + 1);
        for _ in 0..n {
            if bot < self.grid.len() {
                self.grid.remove(bot);
                self.grid.insert(top, Self::blank_row(self.cols));
            }
        }
    }

    fn insert_lines(&mut self, n: u16) {
        let row = self.cursor_row as usize;
        let bot = self.scroll_bottom as usize;
        let n = (n as usize).min(bot - row + 1);
        for _ in 0..n {
            if bot < self.grid.len() {
                self.grid.remove(bot);
            }
            self.grid.insert(row, Self::blank_row(self.cols));
        }
    }

    fn delete_lines(&mut self, n: u16) {
        let row = self.cursor_row as usize;
        let bot = self.scroll_bottom as usize;
        let n = (n as usize).min(bot - row + 1);
        for _ in 0..n {
            if row < self.grid.len() {
                self.grid.remove(row);
            }
            if bot <= self.grid.len() {
                self.grid.insert(bot, Self::blank_row(self.cols));
            }
        }
    }

    fn enter_alt_screen(&mut self) {
        let main_grid = std::mem::replace(
            &mut self.grid,
            (0..self.rows).map(|_| Self::blank_row(self.cols)).collect(),
        );
        self.alt_grid = Some(main_grid);
        self.alt_cursor = Some((self.cursor_row, self.cursor_col));
        self.alt_attrs = Some(self.attrs.clone());
        self.cursor_row = 0;
        self.cursor_col = 0;
        self.pending_wrap = false;
    }

    fn leave_alt_screen(&mut self) {
        if let Some(main_grid) = self.alt_grid.take() {
            self.grid = main_grid;
        }
        if let Some((r, c)) = self.alt_cursor.take() {
            self.cursor_row = r.min(self.rows.saturating_sub(1));
            self.cursor_col = c.min(self.cols.saturating_sub(1));
        }
        if let Some(a) = self.alt_attrs.take() {
            self.attrs = a;
        }
        self.pending_wrap = false;
    }

    fn blank_row(cols: u16) -> Vec<Cell> {
        vec![Cell::default(); cols as usize]
    }

    /// Total scrollback lines available.
    pub fn scrollback_len(&self) -> usize {
        self.scrollback.len()
    }

    /// Scroll up (back into history) by `n` lines.
    pub fn scroll_up(&mut self, n: usize) {
        self.scroll_offset = (self.scroll_offset + n).min(self.scrollback.len());
    }

    /// Scroll down (toward live view) by `n` lines.
    pub fn scroll_down(&mut self, n: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(n);
    }

    /// Snap back to live view (scroll_offset = 0).
    pub fn snap_to_bottom(&mut self) {
        self.scroll_offset = 0;
    }

    /// Get a cell accounting for scroll offset.
    /// When scrolled back, the top rows come from scrollback, the rest from the grid.
    pub fn visible_cell(&self, vis_row: u16, col: u16) -> Option<&Cell> {
        if self.scroll_offset == 0 {
            return self.cell(vis_row, col);
        }
        let sb_len = self.scrollback.len();
        // The visible window covers:
        //   scrollback[sb_len - scroll_offset ..] (first `scroll_offset` rows from scrollback)
        //   grid[0 .. rows - scroll_offset]       (remaining rows from live grid)
        let row = vis_row as usize;
        if row < self.scroll_offset {
            // This row comes from scrollback
            let sb_idx = sb_len - self.scroll_offset + row;
            self.scrollback
                .get(sb_idx)
                .and_then(|r| r.get(col as usize))
        } else {
            // This row comes from the live grid
            let grid_row = row - self.scroll_offset;
            self.grid.get(grid_row).and_then(|r| r.get(col as usize))
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// PTY + Modal (using portable-pty)
// ──────────────────────────────────────────────────────────────────────────────

/// Text selection anchor and endpoint (row, col) in screen coordinates.
#[derive(Debug, Clone, Copy)]
pub struct Selection {
    pub anchor_row: u16,
    pub anchor_col: u16,
    pub end_row: u16,
    pub end_col: u16,
}

impl Selection {
    /// Return (start, end) normalized so start <= end in reading order.
    pub fn ordered(&self) -> ((u16, u16), (u16, u16)) {
        let a = (self.anchor_row, self.anchor_col);
        let b = (self.end_row, self.end_col);
        if a <= b { (a, b) } else { (b, a) }
    }

    /// Check if a cell at (row, col) falls within this selection.
    pub fn contains(&self, row: u16, col: u16) -> bool {
        let ((sr, sc), (er, ec)) = self.ordered();
        if row < sr || row > er {
            return false;
        }
        if sr == er {
            return col >= sc && col <= ec;
        }
        if row == sr {
            return col >= sc;
        }
        if row == er {
            return col <= ec;
        }
        true
    }
}

/// Embedded interactive terminal modal backed by a PTY + hand-rolled screen emulator.
pub struct TerminalModal {
    screen: Screen,
    writer: Option<Box<dyn Write + Send>>,
    pty_output_rx: mpsc::UnboundedReceiver<Vec<u8>>,
    _reader_handle: Option<std::thread::JoinHandle<()>>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    master: Box<dyn portable_pty::MasterPty + Send>,
    cols: u16,
    rows: u16,
    exited: bool,
    /// Current mouse text selection (if any).
    pub selection: Option<Selection>,
}

impl TerminalModal {
    /// Spawn a new PTY shell session with the given dimensions.
    pub fn open(cols: u16, rows: u16) -> Result<Self> {
        let pty_system = NativePtySystem::default();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("failed to open PTY")?;

        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
        let mut cmd = CommandBuilder::new(&shell);
        cmd.cwd(std::env::current_dir().unwrap_or_else(|_| "/".into()));

        let child = pair
            .slave
            .spawn_command(cmd)
            .context("failed to spawn shell")?;

        drop(pair.slave);

        let writer = pair
            .master
            .take_writer()
            .context("failed to take PTY writer")?;

        let reader = pair
            .master
            .try_clone_reader()
            .context("failed to clone PTY reader")?;

        let (tx, rx) = mpsc::unbounded_channel();

        let handle = std::thread::spawn(move || {
            let mut reader = reader;
            let mut buf = [0u8; 4096];
            loop {
                match std::io::Read::read(&mut reader, &mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if tx.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(Self {
            screen: Screen::new(rows, cols),
            writer: Some(writer),
            pty_output_rx: rx,
            _reader_handle: Some(handle),
            child,
            master: pair.master,
            cols,
            rows,
            exited: false,
            selection: None,
        })
    }

    /// Drain any pending PTY output into the screen emulator.
    pub fn process_output(&mut self) {
        while let Ok(bytes) = self.pty_output_rx.try_recv() {
            self.screen.process(&bytes);
        }
        // Write back any responses (e.g. cursor position reports)
        if !self.screen.responses.is_empty() {
            for resp in self.screen.responses.drain(..) {
                if let Some(ref mut w) = self.writer {
                    let _ = w.write_all(&resp);
                }
            }
            if let Some(ref mut w) = self.writer {
                let _ = w.flush();
            }
        }
        // Check if child has exited
        if !self.exited
            && let Ok(Some(_status)) = self.child.try_wait()
        {
            self.exited = true;
        }
    }

    pub fn send_key(&mut self, key: &KeyEvent) {
        if self.exited {
            return;
        }
        if let Some(bytes) = key_to_bytes(key)
            && let Some(ref mut w) = self.writer
        {
            let _ = w.write_all(&bytes);
            let _ = w.flush();
        }
    }

    pub fn send_paste(&mut self, text: &str) {
        if self.exited {
            return;
        }
        if let Some(ref mut w) = self.writer {
            // Wrap in bracketed paste sequences so shells (bash, zsh) handle
            // multi-byte UTF-8 and special characters correctly.
            let _ = w.write_all(b"\x1b[200~");
            // Write in chunks to avoid overflowing the PTY buffer (~4KB on Linux).
            for chunk in text.as_bytes().chunks(4096) {
                let _ = w.write_all(chunk);
                let _ = w.flush();
            }
            let _ = w.write_all(b"\x1b[201~");
            let _ = w.flush();
        }
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        if cols == self.cols && rows == self.rows {
            return;
        }
        self.cols = cols;
        self.rows = rows;
        let _ = self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        });
        self.screen.set_size(rows, cols);
    }

    pub fn screen(&self) -> &Screen {
        &self.screen
    }

    pub fn screen_mut(&mut self) -> &mut Screen {
        &mut self.screen
    }

    pub fn has_exited(&self) -> bool {
        self.exited
    }

    /// Begin a new text selection at the given screen coordinate.
    pub fn start_selection(&mut self, row: u16, col: u16) {
        self.selection = Some(Selection {
            anchor_row: row,
            anchor_col: col,
            end_row: row,
            end_col: col,
        });
    }

    /// Update the endpoint of the current selection.
    pub fn update_selection(&mut self, row: u16, col: u16) {
        if let Some(ref mut sel) = self.selection {
            sel.end_row = row;
            sel.end_col = col;
        }
    }

    /// Clear the current selection.
    pub fn clear_selection(&mut self) {
        self.selection = None;
    }

    /// Extract selected text from the screen (accounts for scrollback).
    pub fn extract_selected_text(&self) -> Option<String> {
        let sel = self.selection.as_ref()?;
        let ((sr, sc), (er, ec)) = sel.ordered();
        let mut lines: Vec<String> = Vec::new();

        for row in sr..=er {
            let mut line = String::new();
            let col_start = if row == sr { sc } else { 0 };
            let col_end = if row == er {
                ec
            } else {
                self.screen.size().1.saturating_sub(1)
            };
            for col in col_start..=col_end {
                if let Some(cell) = self.screen.visible_cell(row, col) {
                    if cell.wide_continuation {
                        continue;
                    }
                    let ch = if cell.ch == '\0' { ' ' } else { cell.ch };
                    line.push(ch);
                }
            }
            // Trim trailing spaces from each line
            let trimmed = line.trim_end().to_string();
            lines.push(trimmed);
        }

        // Remove empty trailing lines
        while lines.last().is_some_and(|l| l.is_empty()) {
            lines.pop();
        }

        let text = lines.join("\n");
        if text.is_empty() { None } else { Some(text) }
    }

    pub fn close(mut self) {
        self.writer.take();
        let _ = self.child.kill();
    }
}

impl Drop for TerminalModal {
    fn drop(&mut self) {
        self.writer.take();
        let _ = self.child.kill();
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Key → byte-sequence translation
// ──────────────────────────────────────────────────────────────────────────────

fn key_to_bytes(key: &KeyEvent) -> Option<Vec<u8>> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);

    match key.code {
        KeyCode::Char(c) if ctrl => {
            let byte = (c.to_ascii_lowercase() as u8)
                .wrapping_sub(b'a')
                .wrapping_add(1);
            if alt {
                Some(vec![0x1b, byte])
            } else {
                Some(vec![byte])
            }
        }
        KeyCode::Char(c) if alt => {
            let mut bytes = vec![0x1b];
            let mut buf = [0u8; 4];
            let s = c.encode_utf8(&mut buf);
            bytes.extend_from_slice(s.as_bytes());
            Some(bytes)
        }
        KeyCode::Char(c) => {
            let mut buf = [0u8; 4];
            let s = c.encode_utf8(&mut buf);
            Some(s.as_bytes().to_vec())
        }
        KeyCode::Enter => Some(vec![b'\r']),
        KeyCode::Backspace => Some(vec![0x7f]),
        KeyCode::Tab => Some(vec![b'\t']),
        KeyCode::BackTab => Some(vec![0x1b, b'[', b'Z']),
        KeyCode::Esc => Some(vec![0x1b]),
        KeyCode::Up => Some(b"\x1b[A".to_vec()),
        KeyCode::Down => Some(b"\x1b[B".to_vec()),
        KeyCode::Right => Some(b"\x1b[C".to_vec()),
        KeyCode::Left => Some(b"\x1b[D".to_vec()),
        KeyCode::Home => Some(b"\x1b[H".to_vec()),
        KeyCode::End => Some(b"\x1b[F".to_vec()),
        KeyCode::PageUp => Some(b"\x1b[5~".to_vec()),
        KeyCode::PageDown => Some(b"\x1b[6~".to_vec()),
        KeyCode::Delete => Some(b"\x1b[3~".to_vec()),
        KeyCode::Insert => Some(b"\x1b[2~".to_vec()),
        KeyCode::F(1) => Some(b"\x1bOP".to_vec()),
        KeyCode::F(2) => Some(b"\x1bOQ".to_vec()),
        KeyCode::F(3) => Some(b"\x1bOR".to_vec()),
        KeyCode::F(4) => Some(b"\x1bOS".to_vec()),
        KeyCode::F(5) => Some(b"\x1b[15~".to_vec()),
        KeyCode::F(6) => Some(b"\x1b[17~".to_vec()),
        KeyCode::F(7) => Some(b"\x1b[18~".to_vec()),
        KeyCode::F(8) => Some(b"\x1b[19~".to_vec()),
        KeyCode::F(9) => Some(b"\x1b[20~".to_vec()),
        KeyCode::F(10) => Some(b"\x1b[21~".to_vec()),
        KeyCode::F(11) => Some(b"\x1b[23~".to_vec()),
        KeyCode::F(12) => Some(b"\x1b[24~".to_vec()),
        _ => None,
    }
}
