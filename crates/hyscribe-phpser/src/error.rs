// SPDX-License-Identifier: AGPL-3.0-or-later

use std::fmt;

/// Why decoding failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    /// The input ended in the middle of a value.
    UnexpectedEnd,
    /// A specific byte or token was required here.
    Expected(&'static str),
    /// A type marker PHP does not produce (`S:`, lowercase `o:` and others).
    UnsupportedType(u8),
    /// A length or count does not fit in memory on this platform.
    LengthOverflow,
    /// A float is not in PHP's `d:` grammar.
    InvalidFloat,
    /// A class or enum name PHP would reject.
    InvalidClassName,
    /// An array or object key that is not `i:` or `s:`.
    InvalidKey,
    /// A declared element count is larger than the remaining input can hold.
    CountTooLarge,
    /// Arrays and objects are nested deeper than [`crate::MAX_DEPTH`].
    TooDeep,
    /// Bytes remain after the top-level value.
    TrailingData,
}

/// A decoding error with the byte offset where it was detected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Error {
    /// Offset into the input.
    pub offset: usize,
    /// What went wrong.
    pub kind: ErrorKind,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self.kind {
            ErrorKind::UnexpectedEnd => "unexpected end of input".into(),
            ErrorKind::Expected(what) => format!("expected {what}"),
            ErrorKind::UnsupportedType(b) => {
                format!("unsupported type marker {:?}", char::from(b))
            }
            ErrorKind::LengthOverflow => "length overflows".into(),
            ErrorKind::InvalidFloat => "invalid float".into(),
            ErrorKind::InvalidClassName => "invalid class name".into(),
            ErrorKind::InvalidKey => "invalid array key".into(),
            ErrorKind::CountTooLarge => "element count exceeds input size".into(),
            ErrorKind::TooDeep => "nesting too deep".into(),
            ErrorKind::TrailingData => "trailing data".into(),
        };
        write!(f, "{message} at offset {}", self.offset)
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_names_every_kind_and_offset() {
        let cases = [
            (
                ErrorKind::UnexpectedEnd,
                "unexpected end of input at offset 3",
            ),
            (ErrorKind::Expected("';'"), "expected ';' at offset 3"),
            (
                ErrorKind::UnsupportedType(b'S'),
                "unsupported type marker 'S' at offset 3",
            ),
            (ErrorKind::LengthOverflow, "length overflows at offset 3"),
            (ErrorKind::InvalidFloat, "invalid float at offset 3"),
            (
                ErrorKind::InvalidClassName,
                "invalid class name at offset 3",
            ),
            (ErrorKind::InvalidKey, "invalid array key at offset 3"),
            (
                ErrorKind::CountTooLarge,
                "element count exceeds input size at offset 3",
            ),
            (ErrorKind::TooDeep, "nesting too deep at offset 3"),
            (ErrorKind::TrailingData, "trailing data at offset 3"),
        ];
        for (kind, text) in cases {
            assert_eq!(Error { offset: 3, kind }.to_string(), text);
        }
    }

    #[test]
    fn is_a_std_error() {
        let err: Box<dyn std::error::Error> = Box::new(Error {
            offset: 0,
            kind: ErrorKind::TooDeep,
        });
        assert!(err.source().is_none());
    }
}
