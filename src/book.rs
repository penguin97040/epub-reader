//! Book model: open EPUB, KEPUB and MOBI files into a common shape.
//!
//! Book content is untrusted input. Every loader here returns an error on
//! malformed or encrypted files instead of panicking, and never touches the
//! network.

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};

use percent_encoding::percent_decode_str;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Epub,
    Mobi,
}

impl fmt::Display for Format {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Format::Epub => write!(f, "EPUB"),
            Format::Mobi => write!(f, "MOBI"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Chapter {
    /// XHTML source for this reading unit.
    pub html: String,
    /// Directory the source lives in, for resolving relative asset paths.
    pub base: String,
    /// Label shown in the contents list, if any.
    pub label: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TocNode {
    pub label: String,
    pub chapter: usize,
    pub children: Vec<TocNode>,
}

#[derive(Debug)]
pub struct Book {
    pub title: String,
    pub author: Option<String>,
    pub format: Format,
    pub path: PathBuf,
    pub chapters: Vec<Chapter>,
    pub toc: Vec<TocNode>,
    /// Normalised archive path of each chapter, for resolving internal links.
    pub chapter_paths: Vec<String>,
    /// Asset bytes keyed by normalised path (EPUB) or `recindex:NNNNNN` (MOBI).
    pub assets: HashMap<String, Vec<u8>>,
}

#[derive(Debug)]
pub enum BookError {
    Io(String),
    Unsupported(String),
    Parse(String),
}

impl fmt::Display for BookError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BookError::Io(msg) => write!(f, "Could not read the file: {msg}"),
            BookError::Unsupported(msg) => write!(f, "Unsupported book: {msg}"),
            BookError::Parse(msg) => write!(f, "Could not open this book: {msg}"),
        }
    }
}

impl std::error::Error for BookError {}

/// Open a book, choosing the parser from the file extension and content.
pub fn open(path: &Path) -> Result<Book, BookError> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match ext.as_str() {
        "epub" | "kepub" | "kepub.epub" => open_epub(path),
        "mobi" | "azw" | "prc" => open_mobi(path),
        _ => match sniff(path)? {
            Format::Epub => open_epub(path),
            Format::Mobi => open_mobi(path),
        },
    }
}

fn sniff(path: &Path) -> Result<Format, BookError> {
    let mut head = [0u8; 8];
    use std::io::Read;
    let mut file = std::fs::File::open(path).map_err(|e| BookError::Io(e.to_string()))?;
    let n = file
        .read(&mut head)
        .map_err(|e| BookError::Io(e.to_string()))?;
    if n >= 4 && &head[..4] == b"BOOK" {
        return Ok(Format::Mobi);
    }
    if n >= 4 && (&head[..2] == b"PK" || &head[..4] == b"\x50\x4b\x03\x04") {
        return Ok(Format::Epub);
    }
    Err(BookError::Unsupported(format!(
        "not an EPUB, KEPUB or MOBI file ({})",
        path.display()
    )))
}

fn open_epub(path: &Path) -> Result<Book, BookError> {
    let mut doc = epub::doc::EpubDoc::new(path).map_err(|e| BookError::Parse(e.to_string()))?;

    let title = doc
        .get_title()
        .unwrap_or_else(|| file_stem(path).unwrap_or_else(|| "Untitled".to_owned()));
    let author = doc
        .metadata
        .iter()
        .find(|m| {
            let p = m.property.to_ascii_lowercase();
            p == "creator" || p == "dc:creator" || p.ends_with(":creator")
        })
        .map(|m| m.value.clone())
        .filter(|v| !v.trim().is_empty());

    let cover = doc.get_cover().map(|(bytes, _mime)| bytes);

    let mut assets = HashMap::new();
    let image_ids: Vec<String> = doc
        .resources
        .iter()
        .filter(|(_, item)| item.mime.starts_with("image/"))
        .map(|(id, _)| id.clone())
        .collect();
    for id in image_ids {
        if let Some((bytes, _)) = doc.get_resource(&id) {
            let key = doc
                .resources
                .get(&id)
                .map(|item| normalise_path(&item.path.to_string_lossy()))
                .unwrap_or_else(|| id.clone());
            assets.insert(key, bytes);
        }
    }

    if let Some(cover) = &cover {
        assets.insert("cover:__".to_owned(), cover.clone());
    }

    let mut chapters = Vec::new();
    let mut chapter_paths: Vec<String> = Vec::new();
    let spine = doc.spine.clone();
    for spine_item in spine.iter() {
        let Some(item) = doc.resources.get(&spine_item.idref).cloned() else {
            continue;
        };
        let path_str = item.path.to_string_lossy().to_string();
        if item.mime.contains("html") || item.mime.contains("xml") {
            match doc.get_resource_str(&spine_item.idref) {
                Some((content, _)) => {
                    let base = parent_dir(&normalise_path(&path_str));
                    chapters.push(Chapter {
                        html: content,
                        base,
                        label: None,
                    });
                    chapter_paths.push(normalise_path(&path_str));
                }
                None => continue,
            }
        } else if item.mime.starts_with("image/") {
            let src = normalise_path(&path_str);
            chapters.push(Chapter {
                html: format!("<html><body><p><img src=\"{src}\" alt=\"\"/></p></body></html>"),
                base: parent_dir(&src),
                label: None,
            });
            chapter_paths.push(src);
        }
    }

    if chapters.is_empty() {
        return Err(BookError::Parse("no readable content found".to_owned()));
    }

    let toc = flatten_toc(&doc.toc, &chapter_paths);

    Ok(Book {
        title,
        author,
        format: Format::Epub,
        path: path.to_path_buf(),
        chapters,
        toc,
        chapter_paths,
        assets,
    })
}

fn flatten_toc(nav: &[epub::doc::NavPoint], chapter_paths: &[String]) -> Vec<TocNode> {
    nav.iter()
        .filter_map(|point| {
            let raw = point.content.to_string_lossy().to_string();
            let without_frag = raw.split('#').next().unwrap_or_default();
            let key = normalise_path(without_frag);
            let chapter = chapter_paths
                .iter()
                .position(|p| *p == key || p.ends_with(&format!("/{key}")))
                .or_else(|| chapter_paths.iter().position(|p| p.file_name_eq(&key)))?;
            Some(TocNode {
                label: point.label.trim().to_owned(),
                chapter,
                children: flatten_toc(&point.children, chapter_paths),
            })
        })
        .collect()
}

trait FileNameEq {
    fn file_name_eq(&self, other: &str) -> bool;
}

impl FileNameEq for String {
    fn file_name_eq(&self, other: &str) -> bool {
        self.rsplit('/').next().unwrap_or(self) == other.rsplit('/').next().unwrap_or(other)
    }
}

fn open_mobi(path: &Path) -> Result<Book, BookError> {
    let bytes = std::fs::read(path).map_err(|e| BookError::Io(e.to_string()))?;
    let mobi =
        mobi::Mobi::new(patch_mobi_records(bytes)).map_err(|e| BookError::Parse(e.to_string()))?;

    if !matches!(mobi.encryption(), mobi::headers::Encryption::No) {
        return Err(BookError::Unsupported(
            "this file is DRM-protected".to_owned(),
        ));
    }

    let raw = mobi.content_as_string_lossy();
    let html = strip_record_preamble(&raw);
    if html.trim().is_empty() {
        return Err(BookError::Parse("no readable content found".to_owned()));
    }

    let first_image = mobi.metadata.mobi.first_image_index as usize;
    let mut assets = HashMap::new();
    for (offset, record) in mobi.image_records().iter().enumerate() {
        if !looks_like_image(record.content) {
            continue;
        }
        let index = first_image + offset;
        let key = format!("recindex:{:06}", index);
        assets.insert(key, record.content.to_vec());
    }

    if let Some(bytes) = assets.get(&format!("recindex:{first_image:06}")).cloned() {
        assets.insert("cover:__".to_owned(), bytes);
    }

    let title = {
        let t = mobi.title();
        if t.trim().is_empty() {
            file_stem(path).unwrap_or_else(|| "Untitled".to_owned())
        } else {
            t
        }
    };
    let author = mobi.author().filter(|a| !a.trim().is_empty());

    let chapters = split_mobi_chapters(html);
    if chapters.is_empty() {
        return Err(BookError::Parse("no readable content found".to_owned()));
    }

    let toc = chapters
        .iter()
        .enumerate()
        .filter_map(|(i, c)| {
            let label = c.label.clone()?;
            Some(TocNode {
                label,
                chapter: i,
                children: Vec::new(),
            })
        })
        .collect();

    let chapter_paths: Vec<String> = Vec::new();

    Ok(Book {
        title,
        author,
        format: Format::Mobi,
        path: path.to_path_buf(),
        chapters,
        toc,
        chapter_paths,
        assets,
    })
}

/// `mobi-rs` classifies any record that is not a known index record as an
/// image, so check the magic bytes ourselves before trusting a record.
fn looks_like_image(bytes: &[u8]) -> bool {
    matches!(
        bytes,
        [0x89, b'P', b'N', b'G', ..]
            | [0xFF, 0xD8, 0xFF, ..]
            | [b'G', b'I', b'F', b'8', ..]
            | [b'B', b'M', ..]
            | [b'R', b'I', b'F', b'F', ..]
    )
}

/// `mobi-rs` reads the readable record range as `first_content..first_non_book`
/// but its range helper treats the end as one record early, so the last text
/// record of a book is dropped. Rewriting the header in memory (bumping the
/// first non-book index and appending a dummy trailing record) makes the
/// crate return every text record without touching the file on disk.
pub fn patch_mobi_records(bytes: Vec<u8>) -> Vec<u8> {
    const LIST: usize = 78;
    if bytes.len() < LIST + 2 {
        return bytes;
    }
    let num = u16::from_be_bytes([bytes[76], bytes[77]]) as usize;
    if num == 0 || bytes.len() < LIST + num * 8 + 2 {
        return bytes;
    }
    let data_start = LIST + num * 8 + 2;
    let rec0 = u32::from_be_bytes([
        bytes[LIST],
        bytes[LIST + 1],
        bytes[LIST + 2],
        bytes[LIST + 3],
    ]) as usize;
    let fnb_at = rec0 + 80;
    let fc_at = rec0 + 192;
    if fnb_at + 4 > bytes.len() || fc_at + 2 > bytes.len() || rec0 < data_start {
        return bytes;
    }
    let fnb = u32::from_be_bytes([
        bytes[fnb_at],
        bytes[fnb_at + 1],
        bytes[fnb_at + 2],
        bytes[fnb_at + 3],
    ]);
    let fc = u16::from_be_bytes([bytes[fc_at], bytes[fc_at + 1]]) as u32;
    let num_u32 = num as u32;
    let new_fnb = if fnb == 0xFFFF_FFFF {
        num_u32 + 1
    } else {
        fnb.min(num_u32).max(fc + 1) + 1
    };

    let mut out = Vec::with_capacity(bytes.len() + 8 + 4);
    out.extend_from_slice(&bytes[..76]);
    out.extend_from_slice(&(num_u32 as u16 + 1).to_be_bytes());
    for i in 0..num {
        let at = LIST + i * 8;
        let offset = u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
        out.extend_from_slice(&(offset + 8).to_be_bytes());
        out.extend_from_slice(&bytes[at + 4..at + 8]);
    }
    let dummy_offset = (bytes.len() + 8) as u32;
    out.extend_from_slice(&dummy_offset.to_be_bytes());
    out.extend_from_slice(&[0u8, 0, 0, 0]);
    out.extend_from_slice(&bytes[data_start - 2..data_start]);
    out.extend_from_slice(&bytes[data_start..]);
    let fnb_at = fnb_at + 8;
    out[fnb_at..fnb_at + 4].copy_from_slice(&new_fnb.to_be_bytes());
    out.extend_from_slice(&[0u8, 0, 0, 0]);
    out
}

/// Record 0 of a MOBI file carries binary headers (and the book's name) before
/// the markup; drop everything up to the first tag so the renderer sees HTML only.
fn strip_record_preamble(html: &str) -> &str {
    match html.find('<') {
        Some(i) if i <= 512 => &html[i..],
        _ => html,
    }
}

/// Classic MOBI marks section breaks with `<mbp:pagebreak/>`; each section
/// becomes a chapter so the contents list and progress stay useful.
fn split_mobi_chapters(html: &str) -> Vec<Chapter> {
    let mut chapters = Vec::new();
    let mut rest = html;
    loop {
        let Some(pos) = find_tag_ci(rest, "mbp:pagebreak") else {
            chapters.push(mobi_chapter(rest));
            break;
        };
        let end = rest[pos..]
            .find('>')
            .map(|i| pos + i + 1)
            .unwrap_or(rest.len());
        chapters.push(mobi_chapter(&rest[..pos]));
        rest = &rest[end..];
    }
    // Drop trailing sections with no visible text, but keep at least one.
    while chapters.len() > 1
        && chapters
            .last()
            .is_some_and(|c| strip_tags(&c.html).is_empty())
    {
        chapters.pop();
    }
    chapters
}

fn mobi_chapter(html: &str) -> Chapter {
    let label = first_heading(html);
    Chapter {
        html: html.to_owned(),
        base: String::new(),
        label,
    }
}

fn first_heading(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    for tag in ["<h1", "<h2", "<h3"] {
        let Some(start) = lower.find(tag) else {
            continue;
        };
        let Some(gt) = lower[start..].find('>') else {
            continue;
        };
        let Some(close) = lower[start + gt..].find("</h") else {
            continue;
        };
        let text = &html[start + gt + 1..start + gt + close];
        let cleaned = strip_tags(text);
        let cleaned = cleaned.trim().to_owned();
        if !cleaned.is_empty() {
            return Some(cleaned);
        }
    }
    None
}

fn strip_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    collapse_ws(&out)
}

fn collapse_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_ws = false;
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !prev_ws && !out.is_empty() {
                out.push(' ');
            }
            prev_ws = true;
        } else {
            out.push(ch);
            prev_ws = false;
        }
    }
    out.trim().to_owned()
}

fn find_tag_ci(hay: &str, tag: &str) -> Option<usize> {
    let lower = hay.to_ascii_lowercase();
    lower.find(&format!("<{tag}"))
}

/// Normalise an archive path: percent-decode, strip a leading slash, drop
/// `.` segments and resolve `..` lexically.
pub fn normalise_path(raw: &str) -> String {
    let decoded = percent_decode_str(raw).decode_utf8_lossy();
    let trimmed = decoded.trim_start_matches('/');
    let mut parts: Vec<&str> = Vec::new();
    for part in trimmed.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

fn parent_dir(path: &str) -> String {
    match path.rfind('/') {
        Some(i) => path[..i].to_owned(),
        None => String::new(),
    }
}

/// Resolve a possibly-relative href against a chapter base directory.
pub fn resolve_href(base: &str, href: &str) -> String {
    let decoded = percent_decode_str(href.split('#').next().unwrap_or(href))
        .decode_utf8_lossy()
        .to_string();
    if decoded.starts_with('/') {
        return normalise_path(&decoded);
    }
    let joined = if base.is_empty() {
        decoded
    } else {
        format!("{base}/{decoded}")
    };
    normalise_path(&joined)
}

fn file_stem(path: &Path) -> Option<String> {
    path.file_stem().map(|s| s.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalise_resolves_dot_segments() {
        assert_eq!(
            normalise_path("/OEBPS/../OEBPS/./img/a.png"),
            "OEBPS/img/a.png"
        );
        assert_eq!(normalise_path("text%20one/x.html"), "text one/x.html");
    }

    #[test]
    fn resolve_href_joins_relative() {
        assert_eq!(
            resolve_href("OEBPS/text", "../img/a.png"),
            "OEBPS/img/a.png"
        );
        assert_eq!(resolve_href("", "img/a.png"), "img/a.png");
        assert_eq!(resolve_href("OEBPS", "/img/a.png"), "img/a.png");
    }

    #[test]
    fn mobi_split_uses_pagebreaks() {
        let html = "<html><body><h1>One</h1>text<mbp:pagebreak/><h2>Two</h2>more<mbp:pagebreak/></body></html>";
        let chapters = split_mobi_chapters(html);
        assert_eq!(chapters.len(), 2);
        assert_eq!(chapters[0].label.as_deref(), Some("One"));
        assert_eq!(chapters[1].label.as_deref(), Some("Two"));
    }

    #[test]
    fn mobi_split_keeps_single_chapter_without_breaks() {
        let chapters = split_mobi_chapters("<html><body><p>hello</p></body></html>");
        assert_eq!(chapters.len(), 1);
        assert!(chapters[0].html.contains("hello"));
    }
}
