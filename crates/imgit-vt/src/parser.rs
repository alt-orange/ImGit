//! The escape-sequence state machine: bytes in, [`Perform`] calls out.

/// The longest OSC payload kept; the rest of an overlong one is dropped.
const MAX_OSC: usize = 4096;
/// Parameters saturate here rather than overflow.
const MAX_PARAM: u32 = 99_999;

/// What the byte stream asks of the screen.
pub(crate) trait Perform {
    /// A printable character.
    fn print(&mut self, ch: char);
    /// A C0 control (below 0x20).
    fn execute(&mut self, control: u8);
    /// `ESC final`, without intermediates.
    fn esc_dispatch(&mut self, final_byte: u8);
    /// `CSI ... final`.
    fn csi_dispatch(&mut self, csi: &Csi, final_byte: u8);
    /// The payload of an OSC, up to its terminator.
    fn osc_dispatch(&mut self, payload: &[u8]);
}

/// One CSI parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Param {
    /// `None` when left empty, as in `CSI ;5H`.
    value: Option<u32>,
    /// Introduced by ':' rather than ';': a sub-parameter of the one before it, as in `4:3` or `38:2:r:g:b`.
    sub: bool,
}

/// A control sequence being collected, then dispatched.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Csi {
    params: Vec<Param>,
    /// A digit of the last parameter has been seen.
    param_started: bool,
    /// The last separator was ':'.
    next_is_sub: bool,
    /// '?', '>', '<' or '=' before the parameters.
    private: Option<u8>,
    intermediates: Vec<u8>,
}

impl Csi {
    /// Parameter `index`, or `fallback` when it is missing or empty.
    pub(crate) fn param(&self, index: usize, fallback: usize) -> usize {
        self.params
            .get(index)
            .and_then(|param| param.value)
            .map_or(fallback, |value| value as usize)
    }

    /// Whether parameter `index` followed a ':'.
    pub(crate) fn is_sub_param(&self, index: usize) -> bool {
        self.params.get(index).is_some_and(|param| param.sub)
    }

    pub(crate) fn param_count(&self) -> usize {
        self.params.len()
    }

    pub(crate) const fn private(&self) -> Option<u8> {
        self.private
    }

    pub(crate) fn intermediates(&self) -> &[u8] {
        &self.intermediates
    }

    fn push_param(&mut self) {
        self.params.push(Param {
            value: None,
            sub: self.next_is_sub,
        });
    }

    fn digit(&mut self, digit: u8) {
        if !self.param_started {
            self.push_param();
            self.param_started = true;
        }
        if let Some(param) = self.params.last_mut() {
            let value = param.value.unwrap_or(0) * 10 + u32::from(digit);
            param.value = Some(value.min(MAX_PARAM));
        }
    }

    fn separator(&mut self, colon: bool) {
        if !self.param_started {
            self.push_param();
        }
        self.param_started = false;
        self.next_is_sub = colon;
    }
}

/// A multi-byte UTF-8 sequence in progress; it may straddle two `feed` calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Utf8 {
    code_point: u32,
    remaining: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
enum State {
    #[default]
    Ground,
    Escape,
    /// Character set designations and the like: skipped up to the final byte.
    EscapeIntermediate,
    Csi(Csi),
    Osc(Vec<u8>),
    /// An ESC inside an OSC: the start of ST, or of a new sequence.
    OscEscape(Vec<u8>),
    /// DCS / SOS / PM / APC: ignored up to ST.
    String,
    StringEscape,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Parser {
    state: State,
    utf8: Option<Utf8>,
}

impl Parser {
    pub(crate) fn advance(&mut self, byte: u8, performer: &mut impl Perform) {
        match byte {
            0x1B => {
                self.state = match std::mem::take(&mut self.state) {
                    State::Osc(payload) => State::OscEscape(payload),
                    State::String => State::StringEscape,
                    _ => {
                        self.utf8 = None;
                        State::Escape
                    }
                };
                return;
            }
            // CAN, SUB: abandon the sequence.
            0x18 | 0x1A => {
                self.state = State::Ground;
                return;
            }
            _ => {}
        }
        // The state is Ground from here on unless an arm says otherwise.
        match std::mem::take(&mut self.state) {
            State::Ground => self.ground(byte, performer),
            State::Escape => match byte {
                0x00..=0x1F => {
                    performer.execute(byte);
                    self.state = State::Escape;
                }
                0x20..=0x2F => self.state = State::EscapeIntermediate,
                b'[' => self.state = State::Csi(Csi::default()),
                b']' => self.state = State::Osc(Vec::new()),
                b'P' | b'X' | b'^' | b'_' => self.state = State::String,
                _ => performer.esc_dispatch(byte),
            },
            State::EscapeIntermediate => match byte {
                0x00..=0x1F => {
                    performer.execute(byte);
                    self.state = State::EscapeIntermediate;
                }
                0x20..=0x2F => self.state = State::EscapeIntermediate,
                _ => {}
            },
            State::Csi(mut csi) => {
                match byte {
                    0x00..=0x1F => performer.execute(byte),
                    b'0'..=b'9' => csi.digit(byte - b'0'),
                    b';' | b':' => csi.separator(byte == b':'),
                    0x3C..=0x3F => csi.private = Some(byte),
                    0x20..=0x2F => csi.intermediates.push(byte),
                    0x40..=0x7E => {
                        performer.csi_dispatch(&csi, byte);
                        return;
                    }
                    _ => {}
                }
                self.state = State::Csi(csi);
            }
            State::Osc(mut payload) => {
                if byte == 0x07 {
                    performer.osc_dispatch(&payload);
                } else {
                    if payload.len() < MAX_OSC {
                        payload.push(byte);
                    }
                    self.state = State::Osc(payload);
                }
            }
            State::OscEscape(payload) => {
                performer.osc_dispatch(&payload);
                self.after_string_escape(byte, performer);
            }
            State::String => {
                if byte != 0x07 {
                    self.state = State::String;
                }
            }
            State::StringEscape => self.after_string_escape(byte, performer),
        }
    }

    /// The byte after an ESC that ended a string: `\` completes ST, anything else belongs to a new sequence that
    /// the ESC started.
    fn after_string_escape(&mut self, byte: u8, performer: &mut impl Perform) {
        if byte != b'\\' {
            self.state = State::Escape;
            self.advance(byte, performer);
        }
    }

    fn ground(&mut self, byte: u8, performer: &mut impl Perform) {
        if let Some(mut utf8) = self.utf8.take() {
            if byte & 0xC0 == 0x80 {
                utf8.code_point = (utf8.code_point << 6) | u32::from(byte & 0x3F);
                utf8.remaining -= 1;
                if utf8.remaining == 0 {
                    performer.print(
                        char::from_u32(utf8.code_point).unwrap_or(char::REPLACEMENT_CHARACTER),
                    );
                } else {
                    self.utf8 = Some(utf8);
                }
                return;
            }
            // Cut short: the byte starts over below.
            performer.print(char::REPLACEMENT_CHARACTER);
        }
        let lead = |mask: u8, remaining: u8| {
            Some(Utf8 {
                code_point: u32::from(byte & mask),
                remaining,
            })
        };
        match byte {
            0x00..=0x1F => performer.execute(byte),
            0x20..=0x7E => performer.print(char::from(byte)),
            0x7F => {}
            0xC0..=0xDF => self.utf8 = lead(0x1F, 1),
            0xE0..=0xEF => self.utf8 = lead(0x0F, 2),
            0xF0..=0xF7 => self.utf8 = lead(0x07, 3),
            _ => performer.print(char::REPLACEMENT_CHARACTER),
        }
    }
}
