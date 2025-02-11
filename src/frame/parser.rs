use std::collections::HashMap;
use std::str::from_utf8;

use nom::bytes::complete::{take, take_while, take_while1};
use nom::character::complete::{char as ch, line_ending};
use nom::error::Error;
use nom::multi::many0;
use nom::sequence::{separated_pair, terminated};
use nom::{AsChar, Finish, IResult, Parser};

use crate::frame::{unescape_header, StompCommand, StompFrame, StompFrameError};

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

const HEADER_SEP: u8 = b':';

fn is_header_octet(oct: u8) -> bool {
    !matches!(oct, b'\r' | b'\n' | HEADER_SEP)
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

type StompHeaders = HashMap<String, String>;

fn collect_headers(
    cmd: StompCommand,
    header_pairs: Vec<(&[u8], &[u8])>,
) -> Result<StompHeaders, StompFrameError> {
    let mut headers = HashMap::with_capacity(header_pairs.len());
    let unescape = if matches!(cmd, StompCommand::CONNECT | StompCommand::CONNECTED) {
        |s| Ok(s)
    } else {
        unescape_header
    };
    for pair in header_pairs {
        let key = unescape(from_utf8(pair.0)?.to_string())?;
        let value = unescape(from_utf8(pair.1)?.to_string())?;
        headers.entry(key).or_insert(value);
    }
    Ok(headers)
}

const CONTENT_LENGTH: &str = "content-length";
const DESTINATION: &str = "destination";
const RECEIPT: &str = "receipt";

fn parse_body_with_len(input: &[u8], body_len: usize) -> IResult<&[u8], &[u8]> {
    terminated(take(body_len), (ch('\0'), many0(line_ending))).parse(input)
}

fn parse_body(input: &[u8]) -> IResult<&[u8], &[u8]> {
    terminated(take_while(|c| c != b'\0'), (ch('\0'), many0(line_ending))).parse(input)
}

fn parse_frame(input: &[u8]) -> Result<(StompCommand, StompHeaders, &[u8]), StompFrameError> {
    let (rest, (cmd, header_pairs)) = (
        terminated(StompCommand::parse, line_ending),
        terminated(many0(parse_header), line_ending),
    )
        .parse(input)
        .finish()?;
    let headers = collect_headers(cmd, header_pairs)?;
    let (_, body) = if let Some(content_len) = headers.get(CONTENT_LENGTH) {
        let body_len = content_len
            .parse::<usize>()
            .map_err(|e| StompFrameError::HeaderError(CONTENT_LENGTH.into(), e.to_string()))?;
        parse_body_with_len(rest, body_len).finish()?
    } else {
        parse_body(rest).finish()?
    };
    Ok((cmd, headers, body))
}

impl TryFrom<&[u8]> for StompFrame {
    type Error = StompFrameError;

    fn try_from(input: &[u8]) -> Result<Self, Self::Error> {
        parse_frame(input).and_then(|(cmd, headers, body)| StompFrame::new(cmd, headers, body))
    }
}

impl From<Error<&[u8]>> for StompFrameError {
    fn from(value: Error<&[u8]>) -> Self {
        StompFrameError::SyntaxError(String::from_utf8_lossy(value.input).to_string())
    }
}
