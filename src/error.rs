#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Xml(xml::reader::Error),
    Chrono(chrono::format::ParseError),
    UnexpectedElement(String),
    UnexpectedEvent(String, xml::reader::XmlEvent),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::Io(e) => e.fmt(f),
            Error::Xml(e) => e.fmt(f),
            Error::Chrono(e) => e.fmt(f),
            Error::UnexpectedElement(s) => f.write_fmt(format_args!("Unexpected <{}>", s)),
            Error::UnexpectedEvent(s, e) => f.write_fmt(format_args!("Unexpected {:?}, {}", e, s)),
        }
    }
}

impl std::error::Error for Error {
    fn description(&self) -> &str {
        match self {
            Error::Io(e) => e.description(),
            Error::Xml(e) => e.description(),
            Error::Chrono(e) => e.description(),
            Error::UnexpectedElement(_) => "Unexpected element",
            Error::UnexpectedEvent(_, _) => "Unexpected event",
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Error {
        Error::Io(e)
    }
}

impl From<xml::reader::Error> for Error {
    fn from(e: xml::reader::Error) -> Error {
        Error::Xml(e)
    }
}

impl From<chrono::format::ParseError> for Error {
    fn from(e: chrono::format::ParseError) -> Error {
        Error::Chrono(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
