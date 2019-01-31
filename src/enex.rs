use crate::error::{Error, Result};
use chrono::{DateTime, Local};
use std::io::Read;
use xml::reader::{EventReader, ParserConfig, XmlEvent};

#[derive(Debug, Default)]
pub struct NoteAttributes {
    pub author: Option<String>,
    pub source_url: Option<String>,
    pub source: Option<String>,
    pub latitude: Option<String>,
    pub longitude: Option<String>,
    pub altitude: Option<String>,
}

#[derive(Debug, Default)]
pub struct Note {
    pub title: Option<String>,
    pub content: Option<String>,
    pub created: Option<DateTime<Local>>,
    pub updated: Option<DateTime<Local>>,
    pub tags: Vec<String>,
    pub attributes: NoteAttributes,
}

struct EnexReader<R: Read> {
    reader: EventReader<R>,
}

impl<R: Read> EnexReader<R> {
    fn consume_start_document(&mut self) -> Result<()> {
        match self.reader.next()? {
            XmlEvent::StartDocument { .. } => Ok(()),
            x => Err(Error::UnexpectedEvent(
                "expected document start".to_string(),
                x,
            )),
        }
    }

    fn consume_end_document(&mut self) -> Result<()> {
        match self.reader.next()? {
            XmlEvent::EndDocument => Ok(()),
            x => Err(Error::UnexpectedEvent(
                "expected document end".to_string(),
                x,
            )),
        }
    }

    fn consume_start_element(&mut self, start_tag: &str) -> Result<()> {
        match self.reader.next()? {
            XmlEvent::StartElement { ref name, .. } if name.local_name == start_tag => Ok(()),
            x => Err(Error::UnexpectedEvent(
                format!("expected <{}>", start_tag),
                x,
            )),
        }
    }

    fn read_start_element_until_enclosing(&mut self, end_tag: &str) -> Result<Option<String>> {
        match self.reader.next()? {
            XmlEvent::StartElement { name, .. } => Ok(Some(name.local_name)),
            XmlEvent::EndElement { ref name, .. } if name.local_name == end_tag => Ok(None),
            x => Err(Error::UnexpectedEvent(format!("in <{}>", end_tag), x)),
        }
    }

    fn read_text_until_enclosing(&mut self, end_tag: &str) -> Result<Option<String>> {
        match self.reader.next()? {
            XmlEvent::Characters(text) => {
                match self.read_text_until_enclosing(end_tag)? {
                    Some(more_text) => Ok(Some(text + &more_text)), // not an expected case
                    None => Ok(Some(text)),
                }
            }
            XmlEvent::EndElement { ref name, .. } if name.local_name == end_tag => Ok(None),
            x => Err(Error::UnexpectedEvent("expected text".to_string(), x)),
        }
    }

    fn read_datetime_until_enclosing(&mut self, end_tag: &str) -> Result<Option<DateTime<Local>>> {
        let text = self.read_text_until_enclosing(end_tag)?;
        let text = text.as_ref().map(String::as_str).unwrap_or("");
        // %#z https://github.com/chronotope/chrono/commit/95f6a2be1c8f7a5d8d21a78664b3708e8200bd2b
        Ok(Some(
            DateTime::parse_from_str(text, "%Y%m%dT%H%M%S%#z")?.with_timezone(&Local),
        ))
    }

    fn consume_resource(&mut self) -> Result<()> {
        // Skip <resource>.
        loop {
            match self.reader.next()? {
                XmlEvent::EndElement { ref name } if name.local_name == "resource" => break,
                _ => {}
            }
        }
        Ok(())
    }
}

enum EnexParserState {
    Initial,
    EnExport,
    Done,
}

pub struct EnexParser<R: Read> {
    reader: EnexReader<R>,
    state: EnexParserState,
}

impl<R: Read> EnexParser<R> {
    pub fn new(reader: R) -> Self {
        EnexParser {
            reader: EnexReader {
                reader: ParserConfig::new()
                    .trim_whitespace(true)
                    .cdata_to_characters(true)
                    .create_reader(reader),
            },
            state: EnexParserState::Initial,
        }
    }

    fn read_note(&mut self) -> Result<Note> {
        let mut note = Note::default();
        while let Some(tag) = self
            .reader
            .read_start_element_until_enclosing("note")?
            .as_ref()
            .map(String::as_str)
        {
            match tag {
                "title" => note.title = self.reader.read_text_until_enclosing(tag)?,
                "content" => note.content = self.reader.read_text_until_enclosing(tag)?,
                "created" => note.created = self.reader.read_datetime_until_enclosing(tag)?,
                "updated" => note.updated = self.reader.read_datetime_until_enclosing(tag)?,
                "tag" => note
                    .tags
                    .extend(self.reader.read_text_until_enclosing(tag)?),
                "note-attributes" => note.attributes = self.read_note_attributes()?,
                "resource" => self.reader.consume_resource()?,
                _ => return Err(Error::UnexpectedElement(tag.to_owned())),
            }
        }
        Ok(note)
    }

    fn read_note_attributes(&mut self) -> Result<NoteAttributes> {
        let mut attrs = NoteAttributes::default();
        while let Some(tag) = self
            .reader
            .read_start_element_until_enclosing("note-attributes")?
            .as_ref()
            .map(String::as_str)
        {
            match tag {
                "author" => attrs.author = self.reader.read_text_until_enclosing(tag)?,
                "source" => attrs.source = self.reader.read_text_until_enclosing(tag)?,
                "source-url" => attrs.source_url = self.reader.read_text_until_enclosing(tag)?,
                "latitude" => attrs.latitude = self.reader.read_text_until_enclosing(tag)?,
                "longitude" => attrs.longitude = self.reader.read_text_until_enclosing(tag)?,
                "altitude" => attrs.altitude = self.reader.read_text_until_enclosing(tag)?,
                _ => return Err(Error::UnexpectedElement(tag.to_owned())),
            }
        }
        Ok(attrs)
    }

    fn next_helper(&mut self) -> Result<Option<Note>> {
        loop {
            match self.state {
                EnexParserState::Initial => {
                    self.reader.consume_start_document()?;
                    self.reader.consume_start_element("en-export")?;
                    self.state = EnexParserState::EnExport;
                }
                EnexParserState::EnExport => {
                    return match self
                        .reader
                        .read_start_element_until_enclosing("en-export")?
                        .as_ref()
                        .map(String::as_str)
                    {
                        Some("note") => Ok(Some(self.read_note()?)),
                        Some(tag) => Err(Error::UnexpectedElement(tag.to_owned())),
                        None => {
                            self.reader.consume_end_document()?;
                            self.state = EnexParserState::Done;
                            Ok(None)
                        }
                    };
                }
                EnexParserState::Done => return Ok(None),
            }
        }
    }
}

impl<R: Read> Iterator for EnexParser<R> {
    type Item = Result<Note>;

    fn next(&mut self) -> Option<Result<Note>> {
        match self.next_helper() {
            Ok(Some(n)) => Some(Ok(n)),
            Ok(None) => None,
            Err(e) => Some(Err(e)),
        }
    }
}
