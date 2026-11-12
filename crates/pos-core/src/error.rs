use std::fmt;

#[derive(Debug)]
pub enum Error {
    Db(rusqlite::Error),
    Invalid(String),
    NotFound(String),
    Denied(String),
    Io(std::io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Db(e) => {
                let msg = e.to_string();
                if msg.contains("UNIQUE constraint failed") {
                    write!(f, "This value is already in use. Please use a different one.")
                } else {
                    write!(f, "Database error: {msg}")
                }
            }
            Error::Invalid(m) => write!(f, "{m}"),
            Error::NotFound(m) => write!(f, "Not found: {m}"),
            Error::Denied(m) => write!(f, "{m}"),
            Error::Io(e) => write!(f, "File error: {e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        match e {
            rusqlite::Error::QueryReturnedNoRows => Error::NotFound("record".into()),
            other => Error::Db(other),
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

pub fn invalid<T>(msg: impl Into<String>) -> Result<T> {
    Err(Error::Invalid(msg.into()))
}
