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
            let file = File::open(path)
                .map_err(|error| DocumentError::new(format!("open {}", path.display()), error))?;
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
