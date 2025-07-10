// Copyright (C) 2025  Krzysztof Molski
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::collections::HashMap;
use std::str::from_utf8;

use nom::bytes::complete::{take, take_while, take_while1};
use nom::character::complete::{char as ch, line_ending};
use nom::error::Error;
use nom::multi::many0;
use nom::sequence::{separated_pair, terminated};
use nom::{AsChar, Finish, IResult, Parser};

use crate::frame::{StompCommand, StompFrame, StompFrameError, CONTENT_LENGTH, HEADER_SEP};

macro_rules! stomp_command_parse_impl {
    ($typename: ident, $($command:ident),+) => {
        use nom::{IResult, Parser, branch::alt, bytes::complete::tag, combinator::value};
        impl $typename {
            fn parse(input: &[u8]) -> IResult<&[u8], $typename> {
                alt((
                    $(value(Self::$command, tag(stringify!($command)))),+
                )).parse(input)
            }
        }
    };
}

pub(super) use stomp_command_parse_impl;

impl TryFrom<&[u8]> for StompFrame {
    type Error = StompFrameError;

    fn try_from(input: &[u8]) -> Result<Self, Self::Error> {
        parse_frame(input).and_then(|(cmd, headers, body)| StompFrame::new(cmd, headers, body))
    }
}

type StompHeaders = HashMap<String, String>;

fn parse_frame(input: &[u8]) -> Result<(StompCommand, StompHeaders, &[u8]), StompFrameError> {
    let (rest, (cmd, header_pairs)) = (
        terminated(StompCommand::parse, line_ending),
        terminated(many0(parse_header), line_ending),
    )
        .parse(input)
        .finish()?;
    let headers = collect_headers(cmd, header_pairs)?;
    let (_, body) = if let Some(content_len) = headers.get(CONTENT_LENGTH) {
        let Ok(body_len) = content_len.parse::<usize>() else {
            return Err(StompFrameError::HeaderError(
                CONTENT_LENGTH.into(),
                content_len.clone(),
            ));
        };
        parse_body_with_len(rest, body_len).finish()?
    } else {
        parse_body(rest).finish()?
    };
    Ok((cmd, headers, body))
}

fn parse_header(input: &[u8]) -> IResult<&[u8], (&[u8], &[u8])> {
    terminated(
        separated_pair(
            take_while1(is_header_octet),
            ch(HEADER_SEP.as_char()),
            take_while(is_header_octet),
        ),
        line_ending,
    )
    .parse(input)
}

fn is_header_octet(oct: u8) -> bool {
    !matches!(oct, b'\r' | b'\n' | HEADER_SEP)
}

fn collect_headers(
    cmd: StompCommand,
    header_pairs: Vec<(&[u8], &[u8])>,
) -> Result<StompHeaders, StompFrameError> {
    let mut headers = HashMap::with_capacity(header_pairs.len());
    for pair in header_pairs {
        let key = unescape_header(from_utf8(pair.0)?.to_string(), cmd)?;
        let value = unescape_header(from_utf8(pair.1)?.to_string(), cmd)?;
        headers.entry(key).or_insert(value);
    }
    Ok(headers)
}

fn unescape_header(header: String, cmd: StompCommand) -> Result<String, StompFrameError> {
    if cmd.has_escaped_headers() {
        let chars = header.chars().collect::<Vec<_>>();
        let mut new_char;
        let mut ch_view = chars.as_slice();
        let mut unescaped = String::new();
        loop {
            (new_char, ch_view) = match ch_view {
                ['\\', '\\', rest @ ..] => ('\\', rest),
                ['\\', 'r', rest @ ..] => ('\r', rest),
                ['\\', 'n', rest @ ..] => ('\n', rest),
                ['\\', 'c', rest @ ..] => (':', rest),
                ['\\', ..] => return Err(StompFrameError::SyntaxError(header)),
                [ch, rest @ ..] => (*ch, rest),
                [] => break,
            };
            unescaped.push(new_char)
        }
        Ok(unescaped)
    } else {
        Ok(header)
    }
}

fn parse_body_with_len(input: &[u8], body_len: usize) -> IResult<&[u8], &[u8]> {
    terminated(take(body_len), (ch('\0'), many0(line_ending))).parse(input)
}

fn parse_body(input: &[u8]) -> IResult<&[u8], &[u8]> {
    terminated(take_while(|c| c != b'\0'), (ch('\0'), many0(line_ending))).parse(input)
}

impl From<Error<&[u8]>> for StompFrameError {
    fn from(value: Error<&[u8]>) -> Self {
        StompFrameError::SyntaxError(String::from_utf8_lossy(value.input).to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_unknown_command_returns_error() {
        let frame = b"nonsense\r\n\r\n\0";
        let result = StompFrame::try_from(&frame[..]);
        assert!(matches!(result, Err(StompFrameError::SyntaxError(..))));
    }

    #[test]
    fn from_cr_lf_swapped_returns_error() {
        let frame = b"CONNECT\n\r\r\n\0";
        let result = StompFrame::try_from(&frame[..]);
        assert!(matches!(result, Err(StompFrameError::SyntaxError(..))));
    }

    #[test]
    fn from_non_utf8_headers_returns_error() {
        let frame = b"SEND\n\
                      header1:val\xc3\x28e1\n\
                      \n\
                      body\0";
        let result = StompFrame::try_from(&frame[..]);
        assert!(matches!(result, Err(StompFrameError::EncodingError(..))));
    }

    #[test]
    fn from_missing_null_terminator_returns_error() {
        let frame = b"CONNECT\r\n\r\n";
        let result = StompFrame::try_from(&frame[..]);
        assert!(matches!(result, Err(StompFrameError::SyntaxError(..))));
    }

    #[test]
    fn from_content_len_overrun_returns_error() {
        let frame = b"SEND\n\
                      content-length:5\n\
                      \n\
                      body\0\n";
        let result = StompFrame::try_from(&frame[..]);
        assert!(matches!(result, Err(StompFrameError::SyntaxError(..))));
    }

    #[test]
    fn from_invalid_escape_returns_error() {
        let frame = b"SEND\n\
                      header1:abc\\tdef\n\
                      \n\0";
        let result = StompFrame::try_from(&frame[..]);
        assert!(matches!(result, Err(StompFrameError::SyntaxError(..))));
    }

    #[test]
    fn from_incomplete_escape_returns_error() {
        let frame = b"SEND\n\
                      header1:abc\\\n\
                      \n\0";
        let result = StompFrame::try_from(&frame[..]);
        assert!(matches!(result, Err(StompFrameError::SyntaxError(..))));
    }

    #[test]
    fn from_ack_with_body_returns_error() {
        let frame = b"ACK\n\nbody\0\n\n";
        let result = StompFrame::try_from(&frame[..]);
        assert!(matches!(result, Err(StompFrameError::SyntaxError(..))));
    }

    #[test]
    fn from_send_frame_returns_ok() {
        let frame = b"SEND\n\nbody\0\n\n";
        let result = StompFrame::try_from(&frame[..]).unwrap();
        assert_eq!(result.cmd, StompCommand::SEND);
        assert!(result.headers.is_empty());
        assert_eq!(result.body, Some(b"body".to_vec()));
    }

    #[test]
    fn from_connected_frame_returns_ok() {
        let frame = b"CONNECTED\n\
                      header1:  a\\\\b\\r\\n\\c   \n\
                      \n\0";
        let result = StompFrame::try_from(&frame[..]).unwrap();
        assert_eq!(result.cmd, StompCommand::CONNECTED);
        assert_eq!(
            result.headers,
            HashMap::from([("header1".to_string(), "  a\\\\b\\r\\n\\c   ".to_string())])
        );
        assert!(result.body.is_none());
    }

    #[test]
    fn from_error_frame_returns_ok() {
        let frame = b"ERROR\n\
                      header1:foobar\n\
                      header1:oldvalue1\n\
                      header1:oldvalue2\n\
                      \n\
                      body123\0";
        let result = StompFrame::try_from(&frame[..]).unwrap();
        assert_eq!(result.cmd, StompCommand::ERROR);
        assert_eq!(
            result.headers,
            HashMap::from([("header1".to_string(), "foobar".to_string())])
        );
        assert_eq!(result.body, Some(b"body123".to_vec()));
    }

    #[test]
    fn from_message_frame_returns_ok() {
        let frame = b"MESSAGE\n\
                      content-length:9\r\n\
                      header1:a\\\\r\\r\\n\\c\n\
                      \n\
                      body123\0\0\0\n\
                      \n\
                      \n";
        let result = StompFrame::try_from(&frame[..]).unwrap();
        assert_eq!(result.cmd, StompCommand::MESSAGE);
        assert_eq!(
            result.headers,
            HashMap::from([
                ("content-length".to_string(), "9".to_string()),
                ("header1".to_string(), "a\\r\r\n:".to_string())
            ])
        );
        assert_eq!(result.body, Some(b"body123\0\0".to_vec()));
    }
}
