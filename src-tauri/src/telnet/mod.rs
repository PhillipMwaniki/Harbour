//! A minimal telnet client, as one more kind of session.
//!
//! Telnet is a TCP stream with an in-band negotiation protocol (RFC 854): most
//! bytes are the terminal data, but `0xFF` (IAC) introduces a command - an
//! option one side offers or refuses, or a subnegotiation. This module speaks
//! just enough of that to hold an interactive session: it strips the commands
//! out of the data before the terminal sees them, answers option offers with a
//! conservative policy, and reports its window size when asked.
//!
//! It is deliberately not a complete implementation. It agrees to suppress
//! go-ahead and to let the server echo (which is what puts a login at a
//! character-at-a-time prompt), reports its size via NAWS, and refuses
//! everything else. That is the behaviour of the classic `telnet` command, and
//! enough for switches, BBSs and the odd legacy daemon.

pub mod transport;

pub use transport::{connect, Connected};

/// Interpret As Command: the byte that introduces every telnet command.
const IAC: u8 = 255;
const DONT: u8 = 254;
const DO: u8 = 253;
const WONT: u8 = 252;
const WILL: u8 = 251;
/// Subnegotiation begin and end.
const SB: u8 = 250;
const SE: u8 = 240;

// The options we have an opinion about.
const OPT_ECHO: u8 = 1;
const OPT_SGA: u8 = 3; // suppress go-ahead
const OPT_NAWS: u8 = 31; // negotiate about window size

/// What one pass over incoming bytes produced.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Processed {
    /// The terminal data, with every command removed.
    pub data: Vec<u8>,
    /// Bytes to write straight back to the server - our negotiation replies.
    pub replies: Vec<u8>,
    /// The server agreed to NAWS, so window-size updates should now be sent.
    pub naws_enabled: bool,
}

/// Where the parser is between bytes, so a command split across two reads is
/// still understood.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum State {
    #[default]
    Data,
    /// Saw IAC; the next byte is the command.
    Command,
    /// Saw IAC + one of WILL/WONT/DO/DONT; the next byte is the option.
    Option(u8),
    /// Inside a subnegotiation, waiting for IAC SE.
    Subneg,
    /// Inside a subnegotiation and just saw IAC; SE ends it, IAC is a literal.
    SubnegIac,
}

/// The running state of one telnet stream's negotiation.
#[derive(Debug, Default)]
pub struct Parser {
    state: State,
    naws_enabled: bool,
}

impl Parser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds a chunk of bytes from the server, returning the terminal data, any
    /// replies to send back, and whether NAWS is now enabled.
    pub fn feed(&mut self, bytes: &[u8]) -> Processed {
        let mut out = Processed::default();
        for &byte in bytes {
            match self.state {
                State::Data => {
                    if byte == IAC {
                        self.state = State::Command;
                    } else {
                        out.data.push(byte);
                    }
                }
                State::Command => match byte {
                    IAC => {
                        // A doubled IAC is a literal 0xFF in the data.
                        out.data.push(IAC);
                        self.state = State::Data;
                    }
                    WILL | WONT | DO | DONT => self.state = State::Option(byte),
                    SB => self.state = State::Subneg,
                    // Two-byte commands we do not act on (GA, NOP, ...): consume.
                    _ => self.state = State::Data,
                },
                State::Option(command) => {
                    self.respond(command, byte, &mut out);
                    self.state = State::Data;
                }
                State::Subneg => {
                    if byte == IAC {
                        self.state = State::SubnegIac;
                    }
                    // The subnegotiation content itself is ignored.
                }
                State::SubnegIac => {
                    // IAC SE ends the subnegotiation; IAC IAC is a literal we
                    // still ignore inside a subnegotiation.
                    self.state = if byte == SE {
                        State::Data
                    } else {
                        State::Subneg
                    };
                }
            }
        }
        out.naws_enabled = self.naws_enabled;
        out
    }

    /// The conservative option policy. Reply only to offers (`DO`/`WILL`); a
    /// refusal (`DONT`/`WONT`) needs no answer and answering it invites a loop.
    fn respond(&mut self, command: u8, option: u8, out: &mut Processed) {
        match command {
            // The server asks us to enable an option.
            DO => {
                let agree = option == OPT_SGA || option == OPT_NAWS;
                out.replies
                    .extend_from_slice(&[IAC, if agree { WILL } else { WONT }, option]);
                if agree && option == OPT_NAWS {
                    self.naws_enabled = true;
                }
            }
            // The server offers to enable an option on its side.
            WILL => {
                let agree = option == OPT_ECHO || option == OPT_SGA;
                out.replies
                    .extend_from_slice(&[IAC, if agree { DO } else { DONT }, option]);
            }
            // DONT / WONT: the server is refusing or disabling. Nothing to say.
            _ => {}
        }
    }
}

/// Escapes user input for the wire: a literal `0xFF` must be doubled so it is
/// not read as IAC, and a bare carriage return becomes CR LF, which is what
/// telnet's network virtual terminal expects for a line ending.
pub fn escape_input(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        let byte = data[i];
        match byte {
            IAC => out.extend_from_slice(&[IAC, IAC]),
            b'\r' => {
                out.push(b'\r');
                // Only add the LF if the client did not already send one.
                if data.get(i + 1) != Some(&b'\n') {
                    out.push(b'\n');
                }
            }
            other => out.push(other),
        }
        i += 1;
    }
    out
}

/// The NAWS subnegotiation reporting `cols` x `rows`. Width and height are
/// 16-bit, big-endian, and any `0xFF` among those bytes is doubled like any
/// other IAC in the stream.
pub fn naws_subneg(cols: u16, rows: u16) -> Vec<u8> {
    let mut body = Vec::new();
    for value in [cols, rows] {
        for byte in value.to_be_bytes() {
            if byte == IAC {
                body.push(IAC);
            }
            body.push(byte);
        }
    }
    let mut out = vec![IAC, SB, OPT_NAWS];
    out.extend_from_slice(&body);
    out.extend_from_slice(&[IAC, SE]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_data_passes_straight_through() {
        let mut parser = Parser::new();
        let out = parser.feed(b"hello world");
        assert_eq!(out.data, b"hello world");
        assert!(out.replies.is_empty());
    }

    #[test]
    fn a_doubled_iac_is_one_literal_byte() {
        let mut parser = Parser::new();
        let out = parser.feed(&[b'a', IAC, IAC, b'b']);
        assert_eq!(out.data, vec![b'a', 0xFF, b'b']);
    }

    #[test]
    fn we_suppress_go_ahead_and_refuse_the_rest_when_asked_to_do() {
        let mut parser = Parser::new();
        // DO SGA -> WILL SGA; DO ECHO -> WONT ECHO.
        let out = parser.feed(&[IAC, DO, OPT_SGA, IAC, DO, OPT_ECHO]);
        assert_eq!(out.replies, vec![IAC, WILL, OPT_SGA, IAC, WONT, OPT_ECHO]);
        assert!(out.data.is_empty());
    }

    #[test]
    fn we_let_the_server_echo_and_suppress_go_ahead_when_it_will() {
        let mut parser = Parser::new();
        // WILL ECHO -> DO ECHO; WILL SGA -> DO SGA; WILL STATUS(5) -> DONT.
        let out = parser.feed(&[IAC, WILL, OPT_ECHO, IAC, WILL, OPT_SGA, IAC, WILL, 5]);
        assert_eq!(
            out.replies,
            vec![IAC, DO, OPT_ECHO, IAC, DO, OPT_SGA, IAC, DONT, 5]
        );
    }

    #[test]
    fn agreeing_to_naws_is_reported() {
        let mut parser = Parser::new();
        let out = parser.feed(&[IAC, DO, OPT_NAWS]);
        assert_eq!(out.replies, vec![IAC, WILL, OPT_NAWS]);
        assert!(out.naws_enabled);
    }

    #[test]
    fn a_refusal_gets_no_reply() {
        let mut parser = Parser::new();
        let out = parser.feed(&[IAC, WONT, OPT_ECHO, IAC, DONT, OPT_SGA]);
        assert!(out.replies.is_empty());
    }

    #[test]
    fn a_subnegotiation_is_swallowed_whole() {
        let mut parser = Parser::new();
        // IAC SB TTYPE ... IAC SE, wrapped in ordinary data.
        let mut bytes = vec![b'x', IAC, SB, 24, 0, b'a', b'n', b'y'];
        bytes.extend_from_slice(&[IAC, SE, b'y']);
        let out = parser.feed(&bytes);
        assert_eq!(out.data, b"xy");
    }

    #[test]
    fn a_command_split_across_two_feeds_is_still_understood() {
        let mut parser = Parser::new();
        assert!(parser.feed(&[b'a', IAC]).data == b"a");
        let out = parser.feed(&[DO, OPT_SGA, b'b']);
        assert_eq!(out.data, b"b");
        assert_eq!(out.replies, vec![IAC, WILL, OPT_SGA]);
    }

    #[test]
    fn input_doubles_iac_and_turns_cr_into_crlf() {
        assert_eq!(
            escape_input(&[b'a', 0xFF, b'b']),
            vec![b'a', IAC, IAC, b'b']
        );
        assert_eq!(escape_input(b"ls\r"), b"ls\r\n".to_vec());
        // A CR the client already paired with LF is left alone.
        assert_eq!(escape_input(b"ls\r\n"), b"ls\r\n".to_vec());
    }

    #[test]
    fn naws_encodes_width_then_height_big_endian() {
        let sub = naws_subneg(80, 24);
        assert_eq!(sub, vec![IAC, SB, OPT_NAWS, 0, 80, 0, 24, IAC, SE]);
    }

    #[test]
    fn naws_doubles_an_iac_valued_dimension() {
        // 255 columns: the byte 0xFF must be doubled inside the subnegotiation.
        let sub = naws_subneg(255, 24);
        assert_eq!(sub, vec![IAC, SB, OPT_NAWS, 0, IAC, IAC, 0, 24, IAC, SE]);
    }
}
