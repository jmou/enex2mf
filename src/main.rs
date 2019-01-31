// Some unwrapping of Option<String> with default values is awkward:
//     opt_string.as_ref().map_or("default", String::as_str);
// https://github.com/rust-lang/rust/issues/50264 will allow:
//     opt_string.deref().unwrap_or("untitled");
//
// August is a plaintext alternative to html2md. https://gitlab.com/alantrick/august/

use chrono::{DateTime, Local};
use failure::{Error, Fail};
use html2md::parse_html;
use pulldown_cmark::{html, Parser};
use std::ffi::OsStr;
use std::fmt;
use std::fs::File;
use std::io::{stdout, BufReader, Read, Write};
use std::path::Path;
use std::str;
use xml::reader::{EventReader, ParserConfig, XmlEvent};

#[derive(Debug, Fail)]
enum AppError {
    #[fail(display = "I/O error")]
    Io(#[cause] std::io::Error),
    #[fail(display = "XML error")]
    Xml(#[cause] xml::reader::Error),
    #[fail(display = "Chrono error")]
    Chrono(#[cause] chrono::format::ParseError),
    #[fail(display = "Parse error: {}", _0)]
    Parse(String),
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> AppError {
        AppError::Io(e)
    }
}

impl From<xml::reader::Error> for AppError {
    fn from(e: xml::reader::Error) -> AppError {
        AppError::Xml(e)
    }
}

impl From<chrono::format::ParseError> for AppError {
    fn from(e: chrono::format::ParseError) -> AppError {
        AppError::Chrono(e)
    }
}

type Result<T> = std::result::Result<T, AppError>;

#[derive(Clone, Debug, Default)]
struct NoteAttributes {
    author: Option<String>,
    source_url: Option<String>,
    source: Option<String>,
    latitude: Option<String>,
    longitude: Option<String>,
    altitude: Option<String>,
}

#[derive(Clone, Debug, Default)]
struct Note {
    title: Option<String>,
    content: Option<String>,
    created: Option<String>,
    updated: Option<String>,
    tags: Vec<String>,
    attributes: NoteAttributes,
}

struct EnexReader<R: Read> {
    reader: EventReader<R>,
}

fn event_to_error(prefix: &str, event: std::result::Result<XmlEvent, xml::reader::Error>) -> AppError {
    match event {
        Ok(e) => AppError::Parse(format!("{} {:?}", prefix, e)),
        Err(e) => e.into(),
    }
}

impl<R: Read> EnexReader<R> {
    fn read_event(&mut self) -> std::result::Result<XmlEvent, xml::reader::Error> {
        self.reader.next()
    }

    fn read_start_document(&mut self) -> Result<()> {
        match self.read_event() {
            Ok(XmlEvent::StartDocument { .. }) => Ok(()),
            x => {
                return Err(event_to_error("expected document start, found", x));
            }
        }
    }

    fn read_start_element(&mut self, start_tag: &str) -> Result<()> {
        match self.read_event() {
            Ok(XmlEvent::StartElement { ref name, .. }) if name.local_name == start_tag => Ok(()),
            x => {
                return Err(event_to_error("expected <en-export>, found", x));
            }
        }
    }

    fn read_start_or_named_end_element(&mut self, end_tag: &str) -> Result<Option<String>> {
        match self.read_event() {
            Ok(XmlEvent::StartElement { ref name, .. }) => Ok(Some(name.local_name.clone())),
            Ok(XmlEvent::EndElement { ref name, .. }) if name.local_name == end_tag => Ok(None),
            x => {
                return Err(event_to_error("expected <en-export>, found", x));
            }
        }
    }

    fn read_start_or_end_element(&mut self, start_tag: &str, end_tag: &str) -> Result<bool> {
        match self.read_event() {
            Ok(XmlEvent::StartElement { ref name, .. }) if name.local_name == start_tag => Ok(true),
            Ok(XmlEvent::EndElement { ref name, .. }) if name.local_name == end_tag => Ok(false),
            x => {
                return Err(event_to_error("expected <en-export>, found", x));
            }
        }
    }

    fn text_and_end(&mut self) -> Result<Option<String>> {
        match self.read_event() {
            Ok(XmlEvent::Characters(text)) => {
                match self.read_event() {
                    // XXX confirm matching close
                    Ok(XmlEvent::EndElement { .. }) => { }
                    x => {
                        return Err(event_to_error("expected text, found", x));
                    }
                }
                Ok(Some(text))
            }
            // XXX confirm matching close
            Ok(XmlEvent::EndElement { .. }) => Ok(None),
            x => {
                return Err(event_to_error("expected text, found", x));
            }
        }
    }
}

enum EnexParserState {
    Initial,
    EnExport,
    Note(Note),
    NoteAttributes(Note),
    Done,
}

struct EnexParser<R: Read> {
    reader: EnexReader<R>,
    state: EnexParserState,
}

// XXX better handling of tail recursion? may be simple wrap with loop
// https://users.rust-lang.org/t/when-will-rust-have-tco-tce/20790/2
impl<R: Read> EnexParser<R> {
    fn next_helper(&mut self) -> Result<Option<Note>> {
        match self.state {
            EnexParserState::Initial => {
                self.reader.read_start_document()?;
                self.reader.read_start_element("en-export")?;
                self.state = EnexParserState::EnExport;
                self.next_helper()
            }
            EnexParserState::EnExport => {
                if self.reader.read_start_or_end_element("note", "en-export")? {
                    self.state = EnexParserState::Note(Default::default());
                    self.next_helper()
                } else {
                    self.state = EnexParserState::Done;
                    Ok(None)
                }
            }
            EnexParserState::Note(ref mut note) => {
                match self.reader.read_start_or_named_end_element("note")?.as_ref().map(String::as_str) {
                    Some("title") => {
                        note.title = self.reader.text_and_end()?;
                        self.next_helper()
                    }
                    Some("content") => {
                        note.content = self.reader.text_and_end()?;
                        self.next_helper()
                    }
                    Some("created") => {
                        note.created = self.reader.text_and_end()?;
                        self.next_helper()
                    }
                    Some("updated") => {
                        note.updated = self.reader.text_and_end()?;
                        self.next_helper()
                    }
                    Some("tag") => {
                        note.tags.extend(self.reader.text_and_end()?);
                        self.next_helper()
                    }
                    Some("note-attributes") => {
                        // XXX this feels really sketchy
                        self.state = EnexParserState::NoteAttributes(note.clone());
                        self.next_helper()
                    }
                    Some("resource") => {
                        // TODO emit some sort of placeholder
                        loop {
                            match self.reader.read_event() {
                                Ok(XmlEvent::EndElement { ref name }) if name.local_name == "resource" => break,
                                _ => { }
                            }
                        }
                        self.next_helper()
                    }
                    Some(t) => return Err(AppError::Parse(format!("unexpected <{}>", t))),
                    None => {
                        let note = note.clone();
                        self.state = EnexParserState::EnExport;
                        Ok(Some(note))
                    }
                }
            }
            EnexParserState::NoteAttributes(ref mut note) => {
                let attrs = &mut note.attributes;
                match self.reader.read_start_or_named_end_element("note-attributes")?.as_ref().map(String::as_str) {
                    Some("author") => {
                        attrs.author = self.reader.text_and_end()?;
                        self.next_helper()
                    }
                    Some("source") => {
                        attrs.source = self.reader.text_and_end()?;
                        self.next_helper()
                    }
                    Some("source-url") => {
                        attrs.source_url = self.reader.text_and_end()?;
                        self.next_helper()
                    }
                    Some("latitude") => {
                        attrs.latitude = self.reader.text_and_end()?;
                        self.next_helper()
                    }
                    Some("longitude") => {
                        attrs.longitude = self.reader.text_and_end()?;
                        self.next_helper()
                    }
                    Some("altitude") => {
                        attrs.altitude = self.reader.text_and_end()?;
                        self.next_helper()
                    }
                    Some(t) => return Err(AppError::Parse(format!("unexpected <{}>", t))),
                    None => {
                        self.state = EnexParserState::Note(note.clone());
                        self.next_helper()
                    }
                }
            }
            EnexParserState::Done => Ok(None)
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

fn enex_to_mf_date(date: &str) -> Result<impl fmt::Display> {
    // %#z https://github.com/chronotope/chrono/commit/95f6a2be1c8f7a5d8d21a78664b3708e8200bd2b
    let dt = DateTime::parse_from_str(&date, "%Y%m%dT%H%M%S%#z")?.with_timezone(&Local);
    Ok(dt.format("%F %T"))
}

fn write_as_mf<W: Write>(writer: &mut W, note: &Note) -> Result<()> {
    let title = note.title.as_ref().map_or("untitled", String::as_str);
    write!(writer, "# {} <!-- Metadata: type: Note; ", title)?;
    if !note.tags.is_empty() {
        write!(writer, "tags: {}; ", note.tags.join(","))?;
    }
    if let Some(ref x) = note.created {
        write!(writer, "created: {}; ", enex_to_mf_date(x)?)?;
    }
    // Awkward to avoid moving refs.
    if let Some(x) = note.updated.as_ref().or_else(|| note.created.as_ref()) {
        write!(writer, "modified: {}; ", enex_to_mf_date(x)?)?;
    }
    writeln!(writer, "-->\n")?;
    if let Some(ref x) = note.attributes.source_url {
        writeln!(writer, "From {}\n", x)?;
    }

    let content_md = parse_html(&note.content.as_ref().map_or("", String::as_str));
    writeln!(writer, "{}", content_md.trim().replace("\\-", "-"))?;
    writeln!(writer)?;

    Ok(())
}

// TODO this is only for development
fn write_sxs<W: Write>(writer: &mut W, notes: Vec<Note>) -> std::result::Result<(), Error> {
    writeln!(writer, "<meta charset=utf-8><style>.html, .md {{ display: inline-block; width: 49%; margin: 0; vertical-align: top; overflow-x: hidden }} x.md {{ font-size: 130% }}</style>")?;
    writeln!(writer, "<br>")?;
    for note in notes {
        writeln!(
            writer,
            "<div class=html><h1>{}</h1>{}</div>",
            note.title.as_ref().map_or("untitled", String::as_str),
            note.content.as_ref().map_or("", String::as_str)
        )?;
        // Some web clip notes have unterminated <div>
        for _ in 0..30 {
            write!(writer, "</div>")?;
        }
        // writeln!(writer, "<pre class=md>")?;
        let mut md = Vec::new();
        write_as_mf(&mut md, &note)?;
        let mut md_html = String::new();
        html::push_html(&mut md_html, Parser::new(str::from_utf8(&md)?));
        writeln!(writer, "<div class=md>{}</div>", md_html)?;
        // writeln!(writer, "</pre>")?;
    }
    Ok(())
}

fn main() -> std::result::Result<(), Error> {
    let args: Vec<String> = std::env::args().collect();
    let input_path = match &args[..] {
        [_, input_path] =>  input_path,
        _ => panic!("Usage: enex2mf input.enex"),
    };

    let file = File::open(input_path)?;
    let file = BufReader::new(file);

    // XXX factor ::new()
    let mut parser = EnexParser {
        reader: EnexReader {
            reader: ParserConfig::new().trim_whitespace(true).cdata_to_characters(true).create_reader(file),
        },
        state: EnexParserState::Initial,
    };

    let writer = &mut stdout();
    let notebook_name = Path::new(input_path).file_stem().map(OsStr::to_string_lossy);
    // Is it possible to get the &str from the Cow instead of Cow'ing the default value?
    let notebook_name = notebook_name.unwrap_or("unknown".into());
    writeln!(writer, "# {} <!-- Metadata: type: Outline; created: 2018-12-19 11:13:04; reads: 9; read: 2018-12-19 17:39:29; revision: 9; modified: 2018-12-19 17:39:29; importance: 0/5; urgency: 0/5; -->", notebook_name)?;
    // TODO dev only. write_sxs(writer, notes)?;
    for note in parser {
        write_as_mf(writer, &note?)?;
    }

    Ok(())
}
