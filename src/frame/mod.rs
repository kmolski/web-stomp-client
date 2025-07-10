mod parser;

use std::collections::HashMap;
use std::str::Utf8Error;

use thiserror::Error;

use parser::stomp_command_parse_impl;

macro_rules! stomp_command_impl {
    ($($command:ident),+) => {
        #[allow(clippy::upper_case_acronyms)]
        #[derive(Copy, Clone, Debug, Eq, PartialEq)]
        pub enum StompCommand {
            $($command),+
        }

        impl From<StompCommand> for &str {
            fn from(cmd: StompCommand) -> Self {
                match cmd {
                    $(StompCommand::$command => stringify!($command),)+
                }
            }
        }

        stomp_command_parse_impl!(StompCommand, $($command),+);
    };
}

stomp_command_impl!(
    // server
    CONNECTED,
    MESSAGE,
    RECEIPT,
    ERROR,
    // client
    SEND,
    UNSUBSCRIBE,
    SUBSCRIBE,
    BEGIN,
    COMMIT,
    ABORT,
    NACK,
    ACK,
    DISCONNECT,
    CONNECT,
    STOMP
);

impl StompCommand {
    fn is_server_cmd(&self) -> bool {
        matches!(
            self,
            Self::CONNECTED | Self::MESSAGE | Self::RECEIPT | Self::ERROR
        )
    }

    fn may_have_body(&self) -> bool {
        matches!(self, Self::SEND | Self::MESSAGE | Self::ERROR)
    }

    fn has_escaped_headers(&self) -> bool {
        !matches!(self, Self::CONNECT | Self::CONNECTED)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StompFrame {
    pub(crate) cmd: StompCommand,
    pub(crate) headers: HashMap<String, String>, // headers are UTF-8 encoded
    pub(crate) body: Option<Vec<u8>>,
}

#[derive(Error, Debug)]
pub enum StompFrameError {
    #[error("invalid encoding: {0}")]
    EncodingError(#[from] Utf8Error),
    #[error("invalid {0} header value: {1}")]
    HeaderError(String, String),
    #[error("syntax error at: {0}")]
    SyntaxError(String),
}

impl StompFrame {
    pub(crate) fn new(
        cmd: StompCommand,
        headers: HashMap<String, String>,
        body: impl AsRef<[u8]>,
    ) -> Result<Self, StompFrameError> {
        let body = if body.as_ref().is_empty() {
            None
        } else {
            Some(body.as_ref().to_vec())
        };
        if !cmd.may_have_body() && body.is_some() {
            return Err(StompFrameError::SyntaxError(format!(
                "frame type {cmd:?} must not have a body"
            )));
        }
        Ok(StompFrame { cmd, headers, body })
    }
}

impl From<&StompFrame> for Vec<u8> {
    fn from(frame: &StompFrame) -> Self {
        let mut serialized = Vec::new();
        let cmd: &str = frame.cmd.into();
        serialized.extend_from_slice(cmd.as_bytes());
        serialized.push(b'\n');
        for (key, value) in &frame.headers {
            serialized.extend_from_slice(escape_header(key, frame.cmd).as_bytes());
            serialized.push(HEADER_SEP);
            serialized.extend_from_slice(escape_header(value, frame.cmd).as_bytes());
            serialized.push(b'\n');
        }
        serialized.push(b'\n');
        if let Some(body) = &frame.body {
            serialized.extend_from_slice(body);
        }
        serialized.push(b'\0');
        serialized
    }
}

fn escape_header(header: &str, cmd: StompCommand) -> String {
    if cmd.has_escaped_headers() {
        let mut escaped = String::new();
        for ch in header.chars() {
            match ch {
                '\\' => escaped.push_str("\\\\"),
                '\r' => escaped.push_str("\\r"),
                '\n' => escaped.push_str("\\n"),
                ':' => escaped.push_str("\\c"),
                ch => escaped.push(ch),
            }
        }
        escaped
    } else {
        header.to_string()
    }
}

const HEADER_SEP: u8 = b':';
const CONTENT_LENGTH: &str = "content-length";
const DESTINATION: &str = "destination";
const RECEIPT: &str = "receipt";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_send_frame_into_preserves_content() {
        let frame = StompFrame::new(
            StompCommand::SEND,
            HashMap::from([("header1".to_string(), "a\\\\r\r\n:".to_string())]),
            b"body",
        )
        .unwrap();
        let serialized: Vec<u8> = (&frame).into();
        let deserialized = StompFrame::try_from(serialized.as_slice()).unwrap();
        assert_eq!(frame, deserialized);
    }

    #[test]
    fn from_connected_frame_into_preserves_content() {
        let frame = StompFrame::new(
            StompCommand::CONNECTED,
            HashMap::from([("header1".to_string(), "  ab\\r\\n\\c\\\\".to_string())]),
            b"",
        )
        .unwrap();
        let serialized: Vec<u8> = (&frame).into();
        let deserialized = StompFrame::try_from(serialized.as_slice()).unwrap();
        assert_eq!(frame, deserialized);
    }
}
