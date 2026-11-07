//! Bounded tmux control notifications and nested tmux passthrough framing.
//! No process creation, source input, filesystem access or presenter lives here.
const MAX_LINE: usize = 128 * 1024;
const MAX_WRAPPER: usize = 64 * 1024;
const CHUNK: usize = 8192;
const PREFIX: &[u8] = b"\x1bPtmux;";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Gap {
    Malformed,
    TooLarge,
    Paused,
    Continued,
    Closed,
    Incomplete,
    Nesting,
}
pub enum Event<'a> {
    Bytes(&'a [u8]),
    Gap(Gap),
}
pub struct Control {
    pane: u32,
    line: Vec<u8>,
    discarding: bool,
    block: Option<[u64; 3]>,
    closed: bool,
}
impl Control {
    pub fn new(pane: u32) -> Self {
        Self {
            pane,
            line: Vec::new(),
            discarding: false,
            block: None,
            closed: false,
        }
    }
    /// A control-pipe command, never PTY input. tmux requires the leading '%'
    /// argument to be quoted even though a POSIX shell would not require it.
    pub fn flow_command(&self, paused: bool) -> String {
        format!(
            "refresh-client -A '%{}:{}'\n",
            self.pane,
            if paused { "pause" } else { "continue" }
        )
    }
    /// Notifications are parsed even after a source gap so an owner can keep
    /// draining the control pipe while it obtains a new verified baseline.
    pub fn feed(&mut self, data: &[u8], mut emit: impl FnMut(Event<'_>)) {
        for byte in data {
            if self.closed {
                break;
            }
            if *byte == b'\n' {
                if !self.discarding {
                    let mut line = std::mem::take(&mut self.line);
                    self.notification(&line, &mut emit);
                    line.clear();
                    self.line = line;
                }
                self.discarding = false;
            } else if !self.discarding {
                if self.line.len() == MAX_LINE {
                    self.line.clear();
                    self.discarding = true;
                    emit(Event::Gap(Gap::TooLarge));
                } else {
                    self.line.push(*byte);
                }
            }
        }
    }
    pub fn finish(&mut self, mut emit: impl FnMut(Event<'_>)) {
        if self.closed {
            return;
        }
        if !self.line.is_empty() || self.discarding || self.block.is_some() {
            emit(Event::Gap(Gap::Incomplete));
        }
        self.line.clear();
        self.block = None;
        self.closed = true;
        emit(Event::Gap(Gap::Closed));
    }
    fn notification(&mut self, line: &[u8], emit: &mut impl FnMut(Event<'_>)) {
        if let Some(block) = self.block {
            for prefix in [b"%end ".as_slice(), b"%error ".as_slice()] {
                if let Some(tail) = line.strip_prefix(prefix) {
                    if triple(tail) == Some(block) {
                        self.block = None;
                        if prefix == b"%error " {
                            emit(Event::Gap(Gap::Malformed));
                        }
                    } else {
                        emit(Event::Gap(Gap::Malformed));
                        self.closed = true;
                    }
                    break;
                }
            }
            // Command response text can imitate notifications. It never is one.
            return;
        }
        if let Some(tail) = line.strip_prefix(b"%begin ") {
            if let Some(block) = triple(tail) {
                self.block = Some(block);
            } else {
                emit(Event::Gap(Gap::Malformed));
                self.closed = true;
            }
            return;
        }
        if line == b"%exit" || line.starts_with(b"%exit ") {
            self.closed = true;
            emit(Event::Gap(Gap::Closed));
            return;
        }
        for (prefix, gap) in [
            (b"%pause ".as_slice(), Gap::Paused),
            (b"%continue ".as_slice(), Gap::Continued),
        ] {
            if let Some(tail) = line.strip_prefix(prefix) {
                if pane_id(tail) == Some(self.pane) {
                    emit(Event::Gap(gap));
                }
                return;
            }
        }
        let extended = line.starts_with(b"%extended-output ");
        let tail = if extended {
            line.strip_prefix(b"%extended-output ")
        } else {
            line.strip_prefix(b"%output ")
        };
        let Some(tail) = tail else {
            return;
        };
        let Some((pane, mut encoded)) = split(tail) else {
            emit(Event::Gap(Gap::Malformed));
            return;
        };
        let Some(pane) = pane_id(pane) else {
            emit(Event::Gap(Gap::Malformed));
            return;
        };
        if pane != self.pane {
            return;
        }
        if extended {
            let Some((age, fields)) = split(encoded) else {
                emit(Event::Gap(Gap::Malformed));
                return;
            };
            if number(age).is_none() {
                emit(Event::Gap(Gap::Malformed));
                return;
            }
            let mut remaining = fields;
            loop {
                let Some((field, next)) = split(remaining) else {
                    emit(Event::Gap(Gap::Malformed));
                    return;
                };
                if field == b":" {
                    encoded = next;
                    break;
                }
                remaining = next;
            }
        }
        let mut bytes = Vec::with_capacity(encoded.len());
        let mut index = 0;
        while index < encoded.len() {
            let byte = encoded[index];
            if byte == b'\\' {
                let Some(octal) = encoded.get(index + 1..index + 4) else {
                    emit(Event::Gap(Gap::Malformed));
                    return;
                };
                if !(b'0'..=b'3').contains(&octal[0])
                    || !octal[1..].iter().all(|b| (b'0'..=b'7').contains(b))
                {
                    emit(Event::Gap(Gap::Malformed));
                    return;
                }
                bytes.push((octal[0] - b'0') * 64 + (octal[1] - b'0') * 8 + octal[2] - b'0');
                index += 4;
            } else {
                if byte < 32 || byte == 127 {
                    emit(Event::Gap(Gap::Malformed));
                    return;
                }
                bytes.push(byte);
                index += 1;
            }
        }
        emit(Event::Bytes(&bytes));
    }
}
fn split(bytes: &[u8]) -> Option<(&[u8], &[u8])> {
    let index = bytes.iter().position(|b| *b == b' ')?;
    Some((&bytes[..index], &bytes[index + 1..]))
}
fn number(bytes: &[u8]) -> Option<u64> {
    if bytes.is_empty() || bytes.len() > 20 {
        return None;
    }
    bytes.iter().try_fold(0_u64, |n, b| {
        if b.is_ascii_digit() {
            n.checked_mul(10)?.checked_add(u64::from(b - b'0'))
        } else {
            None
        }
    })
}
fn pane_id(bytes: &[u8]) -> Option<u32> {
    number(bytes.strip_prefix(b"%")?).and_then(|n| u32::try_from(n).ok())
}
fn triple(bytes: &[u8]) -> Option<[u64; 3]> {
    let mut fields = bytes.split(|b| *b == b' ');
    let result = [
        number(fields.next()?)?,
        number(fields.next()?)?,
        number(fields.next()?)?,
    ];
    fields.next().is_none().then_some(result)
}

#[derive(Default)]
struct Layer {
    matched: usize,
    wrapped: bool,
    escaped: bool,
    count: usize,
    discarding: bool,
    reject: bool,
}
impl Layer {
    fn feed(&mut self, input: &[u8], emit: &mut impl FnMut(Event<'_>)) {
        if !self.wrapped && self.matched == 0 && !input.contains(&0x1b) {
            if !input.is_empty() {
                emit(Event::Bytes(input));
            }
            return;
        }
        let mut output = Vec::with_capacity(CHUNK);
        for byte in input {
            if output.len() + PREFIX.len() + 1 > CHUNK {
                flush(&mut output, emit);
            }
            if !self.wrapped {
                if *byte == PREFIX[self.matched] {
                    self.matched += 1;
                    if self.matched == PREFIX.len() {
                        self.matched = 0;
                        self.wrapped = true;
                        self.count = 0;
                        if self.reject {
                            self.discarding = true;
                            flush(&mut output, emit);
                            emit(Event::Gap(Gap::Nesting));
                        }
                    }
                } else {
                    output.extend_from_slice(&PREFIX[..self.matched]);
                    self.matched = 0;
                    if *byte == 0x1b {
                        self.matched = 1;
                    } else {
                        output.push(*byte);
                    }
                }
            } else {
                self.count = self.count.saturating_add(1);
                if self.count > MAX_WRAPPER && !self.discarding {
                    self.discarding = true;
                    flush(&mut output, emit);
                    emit(Event::Gap(Gap::TooLarge));
                }
                if self.escaped {
                    self.escaped = false;
                    match byte {
                        0x1b => {
                            if !self.discarding {
                                output.push(0x1b);
                            }
                        }
                        b'\\' => {
                            self.wrapped = false;
                            self.discarding = false;
                        }
                        _ => {
                            self.discarding = true;
                            flush(&mut output, emit);
                            emit(Event::Gap(Gap::Malformed));
                        }
                    }
                } else if *byte == 0x1b {
                    self.escaped = true;
                } else if !self.discarding {
                    output.push(*byte);
                }
            }
            if output.len() >= CHUNK {
                flush(&mut output, emit);
            }
        }
        flush(&mut output, emit);
    }
    fn pending(&self) -> bool {
        self.matched != 0 || self.wrapped || self.escaped
    }
}
fn flush(output: &mut Vec<u8>, emit: &mut impl FnMut(Event<'_>)) {
    if !output.is_empty() {
        emit(Event::Bytes(output));
        output.clear();
    }
}
pub struct Passthrough {
    layers: [Layer; 5],
    faulted: bool,
}
impl Default for Passthrough {
    fn default() -> Self {
        let mut layers = std::array::from_fn(|_| Layer::default());
        layers[4].reject = true;
        Self {
            layers,
            faulted: false,
        }
    }
}
impl Passthrough {
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    pub fn feed(&mut self, bytes: &[u8], mut emit: impl FnMut(Event<'_>)) {
        if self.faulted {
            return;
        }
        let mut faulted = false;
        walk(&mut self.layers, bytes, &mut |event| {
            if faulted {
                return;
            }
            if let Event::Gap(_) = event {
                faulted = true;
            }
            emit(event);
        });
        if faulted {
            self.faulted = true;
        }
    }
    pub fn finish(&mut self, mut emit: impl FnMut(Event<'_>)) {
        if !self.faulted && self.layers.iter().any(Layer::pending) {
            self.faulted = true;
            emit(Event::Gap(Gap::Incomplete));
        }
    }
}
fn walk(layers: &mut [Layer], bytes: &[u8], emit: &mut dyn FnMut(Event<'_>)) {
    if let Some((first, tail)) = layers.split_first_mut() {
        first.feed(bytes, &mut |event| match event {
            Event::Bytes(data) => walk(tail, data, emit),
            Event::Gap(gap) => emit(Event::Gap(gap)),
        });
    } else {
        emit(Event::Bytes(bytes));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn wrapped(bytes: &[u8]) -> Vec<u8> {
        let mut out = PREFIX.to_vec();
        for byte in bytes {
            out.push(*byte);
            if *byte == 0x1b {
                out.push(*byte);
            }
        }
        out.extend_from_slice(b"\x1b\\");
        out
    }
    fn escaped(bytes: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        for b in bytes {
            if *b < 32 || *b == 127 || *b == b'\\' {
                out.extend_from_slice(format!("\\{b:03o}").as_bytes());
            } else {
                out.push(*b);
            }
        }
        out
    }
    #[test]
    fn control_notifications_preserve_binary_and_unicode_at_every_boundary() {
        let payload = b"\x00\x1b_Gi=1;AAAA\x1b\\\x7f\\\xff\r\n";
        let mut stream = b"%begin 1 2 1\n%end 1 2 1\n%output %9 ignored\n%output %0 ".to_vec();
        stream.extend_from_slice(&escaped(payload));
        stream.extend_from_slice(b"\n%extended-output %0 123 future-field : caf\xc3\xa9\n");
        for size in 1..=stream.len() {
            let mut c = Control::new(0);
            let mut bytes = Vec::new();
            let mut gaps = Vec::new();
            let mut emit = |event: Event<'_>| match event {
                Event::Bytes(data) => bytes.extend_from_slice(data),
                Event::Gap(gap) => gaps.push(gap),
            };
            for chunk in stream.chunks(size) {
                c.feed(chunk, &mut emit);
            }
            assert!(gaps.is_empty(), "{gaps:?}");
            assert_eq!(bytes, [payload.as_slice(), "café".as_bytes()].concat());
            assert!(c.line.is_empty());
        }
    }
    #[test]
    fn command_blocks_cannot_spoof_source_notifications() {
        let mut c = Control::new(7);
        let mut bytes = Vec::new();
        let mut gaps = Vec::new();
        c.feed(
            b"%begin 1 2 3\n%output %7 FORGED\n%pause %7\n%exit\n%end 1 2 3\n%output %7 REAL\n",
            |event| match event {
                Event::Bytes(data) => bytes.extend_from_slice(data),
                Event::Gap(gap) => gaps.push(gap),
            },
        );
        assert_eq!(bytes, b"REAL");
        assert!(gaps.is_empty());
        c.feed(
            b"%begin 4 5 6\n%end 4 6 6\n%output %7 BAD\n",
            |event| match event {
                Event::Bytes(_) => panic!("mismatched command block published bytes"),
                Event::Gap(gap) => gaps.push(gap),
            },
        );
        assert_eq!(gaps, [Gap::Malformed]);
    }
    #[test]
    fn flow_commands_quote_typed_pane_ids_without_a_shell() {
        assert_eq!(
            Control::new(0).flow_command(true),
            "refresh-client -A '%0:pause'\n"
        );
        assert_eq!(
            Control::new(u32::MAX).flow_command(false),
            "refresh-client -A '%4294967295:continue'\n"
        );
    }
    #[test]
    fn malformed_control_data_never_publishes_partial_bytes() {
        for tail in [
            b"\\77".as_slice(),
            b"OK\\777",
            b"OK\\xyz",
            b"OK\x07",
            b"OK\x7f",
        ] {
            let mut c = Control::new(1);
            let mut gaps = Vec::new();
            let mut line = b"%output %1 ".to_vec();
            line.extend_from_slice(tail);
            line.push(b'\n');
            c.feed(&line, |event| match event {
                Event::Bytes(_) => panic!("malformed bytes leaked"),
                Event::Gap(gap) => gaps.push(gap),
            });
            assert_eq!(gaps, [Gap::Malformed]);
        }
        let mut c = Control::new(1);
        let mut gaps = Vec::new();
        c.feed(b"%pause %2\n%continue %2\n%pause %1\n%continue %1\n%extended-output %1 bad : data\n%exit bye\n%output %1 BAD\n",|event|match event {Event::Bytes(_)=>panic!("closed client published bytes"),Event::Gap(gap)=>gaps.push(gap)});
        assert_eq!(
            gaps,
            [Gap::Paused, Gap::Continued, Gap::Malformed, Gap::Closed]
        );
    }
    #[test]
    fn overlong_control_lines_and_eof_are_bounded_and_report_a_gap() {
        let mut c = Control::new(1);
        let mut gaps = Vec::new();
        let mut emit = |event: Event<'_>| match event {
            Event::Bytes(_) => panic!("overlong message leaked"),
            Event::Gap(gap) => gaps.push(gap),
        };
        for _ in 0..64 {
            c.feed(&[b'x'; 8192], &mut emit);
            assert!(c.line.len() <= MAX_LINE);
        }
        c.feed(b"\n%output %1 incomplete", &mut emit);
        c.finish(&mut emit);
        c.finish(&mut emit);
        assert_eq!(gaps, [Gap::TooLarge, Gap::Incomplete, Gap::Closed]);
    }
    #[test]
    fn passthrough_unwraps_up_to_four_layers_without_changing_other_strings() {
        let payload = "AB\x1b_Gi=1;AAAA\x1b\\界\x1b]8;;https://example.invalid/\x07C".as_bytes();
        for depth in 0..=4 {
            let mut stream = payload.to_vec();
            for _ in 0..depth {
                stream = wrapped(&stream);
            }
            let mut source = b"PRE\x1bPother;ignored\x1b\\\x1bPtmX".to_vec();
            source.extend_from_slice(&stream);
            source.extend_from_slice(b"POST");
            let expected = [
                b"PRE\x1bPother;ignored\x1b\\\x1bPtmX".as_slice(),
                payload,
                b"POST",
            ]
            .concat();
            for size in 1..=source.len() {
                let mut u = Passthrough::default();
                let mut out = Vec::new();
                let mut gaps = Vec::new();
                let mut emit = |event: Event<'_>| match event {
                    Event::Bytes(data) => out.extend_from_slice(data),
                    Event::Gap(gap) => gaps.push(gap),
                };
                for chunk in source.chunks(size) {
                    u.feed(chunk, &mut emit);
                }
                u.finish(&mut emit);
                assert!(gaps.is_empty(), "depth {depth}: {gaps:?}");
                assert_eq!(out, expected);
            }
        }
    }
    #[test]
    fn bad_passthrough_faults_until_owner_resets_to_a_verified_baseline() {
        for source in [
            b"\x1bPtmux;\x1b[bad".as_slice(),
            &wrapped(&wrapped(&wrapped(&wrapped(&wrapped(b"NESTED"))))),
        ] {
            let mut u = Passthrough::default();
            let mut gaps = Vec::new();
            u.feed(source, |event| {
                if let Event::Gap(gap) = event {
                    gaps.push(gap)
                }
            });
            assert_eq!(gaps.len(), 1);
            u.feed(b"IGNORED", |_| {
                panic!("faulted stream resumed without a baseline")
            });
            u.reset();
            let mut output = Vec::new();
            u.feed(b"READY", |event| match event {
                Event::Bytes(bytes) => output.extend_from_slice(bytes),
                Event::Gap(_) => panic!("reset failed"),
            });
            assert_eq!(output, b"READY");
        }
        let mut u = Passthrough::default();
        let mut gaps = Vec::new();
        u.feed(PREFIX, |_| {});
        for _ in 0..32 {
            u.feed(&[b'x'; 8192], |event| {
                if let Event::Gap(gap) = event {
                    gaps.push(gap)
                }
            });
        }
        assert_eq!(gaps, [Gap::TooLarge]);
        let mut u = Passthrough::default();
        u.feed(b"\x1bPtm", |_| {});
        u.finish(|event| {
            if let Event::Gap(gap) = event {
                gaps.push(gap)
            }
        });
        assert_eq!(gaps, [Gap::TooLarge, Gap::Incomplete]);
    }
    #[test]
    fn actual_control_framing_reaches_the_native_image_decoder_at_the_right_cursor() {
        use crate::graphics_transfer::Assembler;
        #[derive(Default)]
        struct Images {
            assembler: Assembler,
            rgba: Vec<u8>,
            cursor: Option<(u16, u16)>,
        }
        impl vt100::Callbacks for Images {
            fn unhandled_apc(&mut self, screen: &mut vt100::Screen, data: &[u8]) {
                if let Some(done) = self.assembler.push(data, screen.cursor_position()).unwrap() {
                    self.rgba = done.decode_image().unwrap().rgba;
                    self.cursor = Some(done.cursor);
                }
            }
            fn cancelled_apc(&mut self, _: &mut vt100::Screen) {
                self.assembler.cancel();
            }
        }
        let source = wrapped(b"A\x1b_Gf=24,s=1,v=1,m=1;\x1b\\B\x1b_Gm=0;ESIz\x1b\\C");
        let mut line = b"%output %3 ".to_vec();
        line.extend_from_slice(&escaped(&source));
        line.push(b'\n');
        for size in 1..=line.len() {
            let mut c = Control::new(3);
            let mut u = Passthrough::default();
            let mut parser = vt100::Parser::new_with_callbacks(8, 40, 0, Images::default());
            for chunk in line.chunks(size) {
                c.feed(chunk, |event| match event {
                    Event::Bytes(bytes) => u.feed(bytes, |event| match event {
                        Event::Bytes(bytes) => parser.process(bytes),
                        Event::Gap(gap) => panic!("{gap:?}"),
                    }),
                    Event::Gap(gap) => panic!("{gap:?}"),
                });
            }
            assert_eq!(parser.screen().contents(), "ABC");
            assert_eq!(parser.callbacks().rgba, [0x11, 0x22, 0x33, 255]);
            assert_eq!(parser.callbacks().cursor, Some((0, 2)));
        }
    }
}
