// Some unwrapping of Option<String> with default values is awkward:
//     opt_string.as_ref().map_or("default", String::as_str);
// https://github.com/rust-lang/rust/issues/50264 will allow:
//     opt_string.deref().unwrap_or("untitled");
//
// August is a plaintext alternative to html2md. https://gitlab.com/alantrick/august/

mod enex;
mod error;

use crate::enex::{EnexParser, Note};
use crate::error::Result;
use html2md::parse_html;
use pulldown_cmark::{html, Parser};
use std::ffi::OsStr;
use std::fs::File;
use std::io::{stdout, BufReader, Write};
use std::path::Path;
use std::str;

/// Write a single note in MindForger-compatible Markdown.
fn write_as_mf<W: Write>(writer: &mut W, note: &Note) -> Result<()> {
    let title = note.title.as_ref().map_or("untitled", String::as_str);
    write!(writer, "# {} <!-- Metadata: type: Note; ", title)?;
    if !note.tags.is_empty() {
        write!(writer, "tags: {}; ", note.tags.join(","))?;
    }
    if let Some(ref created) = note.created {
        write!(writer, "created: {}; ", created.format("%F %T"))?;
    }
    // Awkward to avoid moving refs.
    if let Some(modified) = note.updated.as_ref().or_else(|| note.created.as_ref()) {
        write!(writer, "modified: {}; ", modified.format("%F %T"))?;
    }
    writeln!(writer, "-->\n")?;
    if let Some(ref from) = note.attributes.source_url {
        writeln!(writer, "From {}\n", from)?;
    }

    let content_md = parse_html(&note.content.as_ref().map_or("", String::as_str));
    writeln!(writer, "{}", content_md.trim().replace("\\-", "-"))?;
    writeln!(writer)?;

    Ok(())
}

// TODO this is only for development
fn write_sxs<W: Write>(
    writer: &mut W,
    notes: Vec<Note>,
) -> std::result::Result<(), Box<std::error::Error>> {
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
        [_, input_path] => input_path,
        _ => panic!("Usage: enex2mf input.enex"),
    };

    let file = File::open(input_path)?;
    let file = BufReader::new(file);
    let parser = EnexParser::new(file);

    let writer = &mut stdout();
    let notebook_name = Path::new(input_path)
        .file_stem()
        .map(OsStr::to_string_lossy);
    // Is it possible to get the &str from the Cow instead of Cow'ing the default value?
    let notebook_name = notebook_name.unwrap_or_else(|| "unknown".into());
    writeln!(writer, "# {} <!-- Metadata: type: Outline; created: 2018-12-19 11:13:04; reads: 9; read: 2018-12-19 17:39:29; revision: 9; modified: 2018-12-19 17:39:29; importance: 0/5; urgency: 0/5; -->", notebook_name)?;
    // TODO dev only. write_sxs(writer, notes)?;
    for note in parser {
        write_as_mf(writer, &note?)?;
    }

    Ok(())
}
