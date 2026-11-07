/// A parser for terminal output which produces an in-memory representation of
/// the terminal contents.
pub struct Parser<CB: crate::callbacks::Callbacks = ()> {
    parser: vte::Parser,
    screen: crate::perform::WrappedScreen<CB>,
    osc_state: u8,
    osc: Vec<u8>,
    string_kind: u8,
}

impl Parser {
    /// Creates a new terminal parser of the given size and with the given
    /// amount of scrollback.
    #[must_use]
    pub fn new(rows: u16, cols: u16, scrollback_len: usize) -> Self {
        Self {
            parser: vte::Parser::new(),
            osc_state: 0,
            osc: Vec::new(),
            string_kind: 0,
            screen: crate::perform::WrappedScreen::new(rows, cols, scrollback_len),
        }
    }
}

impl<CB: crate::callbacks::Callbacks> Parser<CB> {
    /// Creates a new terminal parser of the given size and with the given
    /// amount of scrollback. Terminal events will be reported via method
    /// calls on the provided [`Callbacks`](crate::callbacks::Callbacks)
    /// implementation.
    pub fn new_with_callbacks(rows: u16, cols: u16, scrollback_len: usize, callbacks: CB) -> Self {
        Self {
            parser: vte::Parser::new(),
            osc_state: 0,
            osc: Vec::new(),
            string_kind: 0,
            screen: crate::perform::WrappedScreen::new_with_callbacks(
                rows,
                cols,
                scrollback_len,
                callbacks,
            ),
        }
    }

    /// Processes the contents of the given byte string, and updates the
    /// in-memory terminal state.
    pub fn process(&mut self, bytes: &[u8]) {
        let mut index = 0;
        while index < bytes.len() {
            let byte = bytes[index];
            match self.osc_state {
                0 => {
                    let remaining = &bytes[index..];
                    if let Some(length) = string_start(remaining) {
                        self.parser
                            .advance(&mut self.screen, &remaining[..length + 2]);
                        self.start_string(remaining[length + 1]);
                        index += length + 2;
                    } else {
                        // CSI/SGR and ordinary output stay in the VTE batch
                        // path. Only hold an ESC that may start a string next call.
                        let trailing = remaining.last() == Some(&0x1b);
                        let length = remaining.len() - usize::from(trailing);
                        self.parser.advance(&mut self.screen, &remaining[..length]);
                        if trailing {
                            self.osc_state = 1;
                        }
                        break;
                    }
                    continue;
                }
                1 => {
                    if matches!(byte, b']' | b'_' | b'P' | b'^' | b'X') {
                        self.parser.advance(&mut self.screen, &[0x1b, byte]);
                        self.start_string(byte);
                    } else {
                        self.parser.advance(&mut self.screen, b"\x1b");
                        self.osc_state = 0;
                        continue;
                    }
                }
                2 => match byte {
                    7 if self.string_kind == 1 => self.finish_string(false),
                    0x18 | 0x1a => self.cancel_string(),
                    0x1b => self.osc_state = 3,
                    _ => {
                        let limit = if self.string_kind == 2 {
                            8192
                        } else {
                            128 * 1024
                        };
                        if self.osc.len() < limit {
                            self.osc.push(byte);
                        } else {
                            let kind = self.string_kind;
                            self.cancel_string();
                            self.string_kind = kind;
                            self.osc_state = 4;
                        }
                    }
                },
                3 => {
                    if byte == b'\\' {
                        self.finish_string(false);
                    } else {
                        // ESC ends a legacy OSC even without ST. OSC 8 requires
                        // a real terminator; never publish a partial target.
                        self.finish_string(true);
                        self.osc_state = 1;
                        continue;
                    }
                }
                4 => match byte {
                    7 if self.string_kind == 1 => self.osc_state = 0,
                    0x18 | 0x1a => self.osc_state = 0,
                    0x1b => self.osc_state = 5,
                    _ => (),
                },
                5 => {
                    if self.string_kind == 2 && !matches!(byte, b'\\' | 0x18 | 0x1a | 0x1b) {
                        // Even after overflow, a fresh ESC command cancels
                        // APC. A repaint/reset must not leave the pane stuck
                        // waiting for an ST that the old writer never sends.
                        self.osc_state = 1;
                        continue;
                    }
                    self.osc_state = match byte {
                        7 if self.string_kind == 1 => 0,
                        b'\\' | 0x18 | 0x1a => 0,
                        0x1b => 5,
                        _ => 4,
                    };
                }
                6 => {
                    // DCS/SOS/PM stay in VTE. Do not recognize a nested string
                    // until ESC or cancellation actually ends the old one.
                    let remaining = &bytes[index..];
                    let length =
                        memchr::memchr3(0x1b, 0x18, 0x1a, remaining).unwrap_or(remaining.len());
                    self.parser.advance(&mut self.screen, &remaining[..length]);
                    index += length;
                    if index == bytes.len() {
                        break;
                    }
                    if bytes[index] == 0x1b {
                        self.osc_state = 1;
                    } else {
                        self.parser
                            .advance(&mut self.screen, &bytes[index..index + 1]);
                        self.osc_state = 0;
                    }
                }
                _ => unreachable!(),
            }
            index += 1;
        }
    }

    fn start_string(&mut self, introducer: u8) {
        self.osc.clear();
        self.string_kind = match introducer {
            b']' => 1,
            b'_' => 2,
            _ => 3,
        };
        self.osc_state = if self.string_kind == 3 { 6 } else { 2 };
    }
    fn cancel_string(&mut self) {
        self.parser = vte::Parser::new();
        if self.string_kind == 2 {
            self.screen.callbacks.cancelled_apc(&mut self.screen.screen);
        }
        self.osc.clear();
        self.osc_state = 0;
        if self.string_kind == 1 {
            self.screen.screen.osc8(b";");
        }
    }
    fn finish_string(&mut self, malformed: bool) {
        if self.string_kind == 2 {
            self.parser = vte::Parser::new();
            if !malformed {
                self.screen
                    .callbacks
                    .unhandled_apc(&mut self.screen.screen, &self.osc);
            } else {
                self.screen.callbacks.cancelled_apc(&mut self.screen.screen);
            }
        } else if self.osc.starts_with(b"8;") {
            self.parser = vte::Parser::new();
            self.screen
                .screen
                .osc8(if malformed { b";" } else { &self.osc[2..] });
        } else {
            self.parser.advance(&mut self.screen, &self.osc);
            self.parser.advance(&mut self.screen, b"\x07");
        }
        self.osc.clear();
        self.osc_state = 0;
    }

    /// Returns a reference to a [`Screen`](crate::Screen) object containing
    /// the terminal state.
    #[must_use]
    pub fn screen(&self) -> &crate::Screen {
        &self.screen.screen
    }

    /// Returns a mutable reference to a [`Screen`](crate::Screen) object
    /// containing the terminal state.
    #[must_use]
    pub fn screen_mut(&mut self) -> &mut crate::Screen {
        &mut self.screen.screen
    }

    /// Returns a reference to the [`Callbacks`](crate::callbacks::Callbacks)
    /// state object passed into the constructor.
    pub fn callbacks(&self) -> &CB {
        &self.screen.callbacks
    }

    /// Returns a mutable reference to the
    /// [`Callbacks`](crate::callbacks::Callbacks) state object passed into
    /// the constructor.
    pub fn callbacks_mut(&mut self) -> &mut CB {
        &mut self.screen.callbacks
    }
}

fn string_start(bytes: &[u8]) -> Option<usize> {
    let first = memchr::memchr(0x1b, bytes)?;
    if matches!(
        bytes.get(first + 1).copied(),
        Some(b']' | b'_' | b'P' | b'^' | b'X')
    ) {
        return Some(first);
    }
    let offset = first + 1;
    let tail = &bytes[offset..];
    let osc_apc = candidate(tail, b"]_");
    let ignored = candidate(&tail[..osc_apc.map_or(tail.len(), |p| p + 1)], b"P^X");
    osc_apc.into_iter().chain(ignored).min().map(|p| p + offset)
}
fn candidate(bytes: &[u8], targets: &[u8]) -> Option<usize> {
    let mut index = 0;
    // Search rare introducer bytes, not every SGR ESC. If ordinary text uses
    // many underscores/brackets, fall back to bounded whole-pattern searches.
    for _ in 0..8 {
        let offset = if targets.len() == 2 {
            memchr::memchr2(targets[0], targets[1], &bytes[index..])
        } else {
            memchr::memchr3(targets[0], targets[1], targets[2], &bytes[index..])
        }?;
        let position = index + offset;
        if position > 0 && bytes[position - 1] == 0x1b {
            return Some(position - 1);
        }
        index = position + 1;
    }
    let tail = &bytes[index..];
    targets
        .iter()
        .filter_map(|target| memchr::memmem::find(tail, &[0x1b, *target]).map(|p| p + index))
        .min()
}

impl Default for Parser {
    /// Returns a parser with dimensions 80x24 and no scrollback.
    fn default() -> Self {
        Self::new(24, 80, 0)
    }
}

impl std::io::Write for Parser {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.process(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
