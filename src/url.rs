use std::fmt;

use thiserror::Error;
use url::{ParseError, Url};

/// URL for a secure STOMP-over-WebSocket connection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StompUrl {
    url: Url,
}

impl StompUrl {
    /// Parse a secure WebSocket URL from a string.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use leptos_stomp::StompUrl;
    ///
    /// let result = StompUrl::new("wss://example.com").unwrap();
    /// assert_eq!(result.to_string(), "wss://example.com/");
    /// ```
    ///
    /// # Errors
    ///
    /// The [`StompUrlError`] will be returned when the URL:
    /// - uses a scheme other than `wss`,
    /// - has a fragment (`wss://example.com/#fragment` is not a valid WebSocket address, see
    ///   [MDN](https://developer.mozilla.org/en-US/docs/Web/API/WebSocket/WebSocket#exceptions)),
    /// - contains syntax errors.
    pub fn new(url: impl AsRef<str>) -> Result<Self, StompUrlError> {
        let url = Url::parse(url.as_ref())?;
        if url.scheme() != "wss" {
            Err(StompUrlError::InvalidScheme)
        } else if url.fragment().is_some() {
            Err(StompUrlError::HasFragment)
        } else {
            Ok(Self { url })
        }
    }
}

/// An error which can be returned by [`StompUrl::new`].
///
/// # Examples
///
/// ```rust
/// use leptos_stomp::{StompUrl, StompUrlError};
///
/// let scheme_err = StompUrl::new("http://example.com"); // URL doesn't use the WSS scheme
/// assert_eq!(scheme_err, Err(StompUrlError::InvalidScheme));
///
/// let fragment_err = StompUrl::new("wss://example.com/#fragment"); // URL contains a fragment
/// assert_eq!(fragment_err, Err(StompUrlError::HasFragment));
///
/// let parse_err = StompUrl::new("foobar"); // missing URL base
/// assert!(parse_err.is_err());
/// ```
#[derive(Error, Debug, Eq, PartialEq)]
pub enum StompUrlError {
    #[error("invalid URL: {0}")]
    InvalidUrl(#[from] ParseError),
    #[error("URL must use the WSS scheme")]
    InvalidScheme,
    #[error("URL cannot contain a fragment")]
    HasFragment,
}

impl fmt::Display for StompUrl {
    #[inline]
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.url, formatter)
    }
}
