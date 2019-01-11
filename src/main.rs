// Some unwrapping of Option<String> with default values is awkward:
//     opt_string.as_ref().map_or("default", String::as_str);
// https://github.com/rust-lang/rust/issues/50264 will allow:
//     opt_string.deref().unwrap_or("untitled");
//
// August is a plaintext alternative to html2md. https://gitlab.com/alantrick/august/

use chrono::{DateTime, Local};
use html2md::parse_html;
use pulldown_cmark::{html, Parser};
use roxmltree::{Document, NodeType};
use std::ffi::OsStr;
use std::fmt;
use std::fs;
use std::io::{stdout, Write};
use std::path::Path;
use std::str;

#[derive(Debug)]
enum Error {
    Io(std::io::Error),
    Xml(roxmltree::Error),
    Chrono(chrono::format::ParseError),
    Parse(String),
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Error {
        Error::Io(e)
    }
}

impl From<roxmltree::Error> for Error {
    fn from(e: roxmltree::Error) -> Error {
        Error::Xml(e)
    }
}

impl From<chrono::format::ParseError> for Error {
    fn from(e: chrono::format::ParseError) -> Error {
        Error::Chrono(e)
    }
}

type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::Io(e) => e.fmt(f),
            Error::Xml(e) => e.fmt(f),
            Error::Chrono(e) => e.fmt(f),
            Error::Parse(s) => s.fmt(f),
        }
    }
}

impl std::error::Error for Error {
    fn description(&self) -> &str {
        match self {
            Error::Io(e) => e.description(),
            Error::Xml(e) => e.description(),
            Error::Chrono(e) => e.description(),
            Error::Parse(s) => s,
        }
    }
}

#[derive(Debug, Default)]
struct NoteAttributes {
    author: Option<String>,
    source_url: Option<String>,
    source: Option<String>,
    latitude: Option<String>,
    longitude: Option<String>,
    altitude: Option<String>,
}

#[derive(Debug, Default)]
struct Note {
    title: Option<String>,
    content: Option<String>,
    created: Option<String>,
    updated: Option<String>,
    tags: Vec<String>,
    attributes: NoteAttributes,
}

// TODO try pull parser again. Use state machine to break up match rules. quick-xml or xml-rs.
// https://github.com/media-io/yaserde/blob/master/yaserde/src/de/mod.rs
fn xml_to_notes(buf: &str) -> Result<Vec<Note>> {
    let doc = Document::parse(buf)?;
    let node = doc.root_element();
    if node.node_type() != NodeType::Element || node.tag_name().name() != "en-export" {
        return Err(Error::Parse("expected <en-export>".to_string()));
    }
    let mut notes = Vec::new();
    for node in node.children() {
        if node.node_type() == NodeType::Text && node.text().unwrap().trim().is_empty() {
            continue;
        }
        if node.node_type() != NodeType::Element || node.tag_name().name() != "note" {
            return Err(Error::Parse("expected <note>".to_string()));
        }
        let mut note: Note = Default::default();
        for node in node.children() {
            if node.node_type() == NodeType::Element {
                match node.tag_name().name() {
                    "title" => note.title = node.text().map(str::to_owned),
                    "content" => note.content = node.text().map(str::to_owned),
                    "created" => note.created = node.text().map(str::to_owned),
                    "updated" => note.updated = node.text().map(str::to_owned),
                    "tag" => note.tags.extend(node.text().map(str::to_owned)),
                    "note-attributes" => {
                        let attrs = &mut note.attributes;
                        for node in node.children() {
                            if node.node_type() == NodeType::Element {
                                match node.tag_name().name() {
                                    "author" => attrs.author = node.text().map(str::to_owned),
                                    "source" => attrs.source = node.text().map(str::to_owned),
                                    "source-url" => attrs.source_url = node.text().map(str::to_owned),
                                    "latitude" => attrs.latitude = node.text().map(str::to_owned),
                                    "longitude" => attrs.longitude = node.text().map(str::to_owned),
                                    "altitude" => attrs.altitude = node.text().map(str::to_owned),
                                    t => return Err(Error::Parse(format!("unexpected <{}>", t))),
                                }
                            }
                        }
                    }
                    "resource" => {
                        // TODO emit some sort of placeholder
                    }
                    t => return Err(Error::Parse(format!("unexpected <{}>", t))),
                }
            }
        }
        notes.push(note);
    }
    Ok(notes)
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
fn write_sxs<W: Write>(writer: &mut W, notes: Vec<Note>) -> std::result::Result<(), Box<std::error::Error>> {
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

fn main() -> std::result::Result<(), Box<std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let input_path = match &args[..] {
        [_, input_path] =>  input_path,
        _ => panic!("Usage: enex2mf input.enex"),
    };

    let buf = fs::read_to_string(input_path)?;
    let notes = xml_to_notes(&buf)?;

    let writer = &mut stdout();
    let notebook_name = Path::new(input_path).file_stem().map(OsStr::to_string_lossy);
    // Is it possible to get the &str from the Cow instead of Cow'ing the default value?
    let notebook_name = notebook_name.unwrap_or("unknown".into());
    writeln!(writer, "# {} <!-- Metadata: type: Outline; created: 2018-12-19 11:13:04; reads: 9; read: 2018-12-19 17:39:29; revision: 9; modified: 2018-12-19 17:39:29; importance: 0/5; urgency: 0/5; -->", notebook_name)?;
    // TODO dev only. write_sxs(writer, notes)?;
    for note in notes {
        write_as_mf(writer, &note)?;
    }

    Ok(())
}
