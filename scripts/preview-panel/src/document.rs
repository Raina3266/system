use std::error::Error;
use std::fmt;
use std::fs::File;
use std::io::{self, IsTerminal, Read};

use crate::cli::Source;

#[derive(Debug)]
pub struct DocumentError {
    context: String,
    source: io::Error,
}

impl DocumentError {
    fn new(context: impl Into<String>, source: io::Error) -> Self {
        Self {
            context: context.into(),
            source,
        }
    }
}

impl fmt::Display for DocumentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.context, self.source)
    }
}

impl Error for DocumentError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.source)
    }
}

pub fn load(source: &Source) -> Result<String, DocumentError> {
    match source {
        Source::Stdin => {
            let stdin = io::stdin();
            if stdin.is_terminal() {
                return Ok(String::new());
            }
            read_utf8(stdin.lock())
                .map_err(|error| DocumentError::new("read standard input", error))
        }
        Source::File(path) => {
            let file = File::open(path).map_err(|error| {
                DocumentError::new(format!("open {}", path.display()), error)
            })?;
            read_utf8(file)
                .map_err(|error| DocumentError::new(format!("read {}", path.display()), error))
        }
    }
}

pub fn read_utf8(mut reader: impl Read) -> io::Result<String> {
    let mut text = String::new();
    reader.read_to_string(&mut text)?;
    Ok(text)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn preserves_every_whitespace_character_exactly() {
        let original = "heading\r\n\t  first    value\n\n    indented\tcolumn  \n";
        let loaded = read_utf8(Cursor::new(original.as_bytes())).unwrap();
        assert_eq!(loaded, original);
    }

    #[test]
    fn preserves_unicode_without_normalizing_it() {
        let original = "café\n中文\n👩🏽‍💻\n";
        let loaded = read_utf8(Cursor::new(original.as_bytes())).unwrap();
        assert_eq!(loaded.as_bytes(), original.as_bytes());
    }

    #[test]
    fn rejects_non_utf8_input_instead_of_replacing_bytes() {
        let error = read_utf8(Cursor::new([0xff, 0xfe])).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }
}
