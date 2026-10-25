//! End-to-end tests: generate a small EPUB and a small MOBI in a temp dir,
//! open them through the same code path the app uses, and check the model.

use std::io::{Cursor, Write};
use std::path::Path;

const TINY_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x62, 0xF8, 0xCF, 0xC0, 0xF0,
    0x1F, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45,
    0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
];

fn write_epub(path: &Path) {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    zip.start_file("mimetype", options).unwrap();
    zip.write_all(b"application/epub+zip").unwrap();

    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("META-INF/container.xml", options).unwrap();
    zip.write_all(
        br#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#,
    )
    .unwrap();

    zip.start_file("OEBPS/content.opf", options).unwrap();
    zip.write_all(
        br#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0" unique-identifier="uid">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:opf="http://www.idpf.org/2007/opf">
    <dc:title>Fixture EPUB</dc:title>
    <dc:creator opf:role="aut">Fixture Author</dc:creator>
    <dc:identifier id="uid">fixture-epub-1</dc:identifier>
    <dc:language>en</dc:language>
  </metadata>
  <manifest>
    <item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>
    <item id="ch1" href="text/ch1.xhtml" media-type="application/xhtml+xml"/>
    <item id="ch2" href="text/ch2.xhtml" media-type="application/xhtml+xml"/>
    <item id="pic" href="images/pic.png" media-type="image/png"/>
  </manifest>
  <spine toc="ncx">
    <itemref idref="ch1"/>
    <itemref idref="ch2"/>
  </spine>
</package>"#,
    )
    .unwrap();

    zip.start_file("OEBPS/toc.ncx", options).unwrap();
    zip.write_all(
        br#"<?xml version="1.0"?>
<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1">
  <head><meta name="dtb:uid" content="fixture-epub-1"/></head>
  <docTitle><text>Fixture EPUB</text></docTitle>
  <navMap>
    <navPoint id="n1" playOrder="1">
      <navLabel><text>Chapter One</text></navLabel>
      <content src="text/ch1.xhtml"/>
    </navPoint>
    <navPoint id="n2" playOrder="2">
      <navLabel><text>Chapter Two</text></navLabel>
      <content src="text/ch2.xhtml"/>
    </navPoint>
  </navMap>
</ncx>"#,
    )
    .unwrap();

    zip.start_file("OEBPS/text/ch1.xhtml", options).unwrap();
    zip.write_all(
        br#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml">
<head><title>One</title></head>
<body>
  <h1>Chapter One</h1>
  <p>Opening paragraph with <b>bold</b> and a <a href="ch2.xhtml">link onward</a>.</p>
  <p><img src="../images/pic.png" alt="A pixel"/></p>
</body>
</html>"#,
    )
    .unwrap();

    zip.start_file("OEBPS/text/ch2.xhtml", options).unwrap();
    zip.write_all(
        br#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml">
<head><title>Two</title></head>
<body>
  <h2>Chapter Two</h2>
  <ul><li>first</li><li>second</li></ul>
</body>
</html>"#,
    )
    .unwrap();

    zip.start_file("OEBPS/images/pic.png", options).unwrap();
    zip.write_all(TINY_PNG).unwrap();

    let cursor = zip.finish().unwrap();
    std::fs::write(path, cursor.into_inner()).unwrap();
}

fn be16(v: u16) -> Vec<u8> {
    v.to_be_bytes().to_vec()
}

fn be32(v: u32) -> Vec<u8> {
    v.to_be_bytes().to_vec()
}

fn write_mobi(path: &Path) {
    let title = "Fixture MOBI Book";
    let html = format!(
        "{title}<html><body><h1>Part One</h1><p>MOBI text one.</p><mbp:pagebreak/><h1>Part Two</h1><p>MOBI text two.</p></body></html>"
    );
    let html_bytes = html.as_bytes();

    let mut record0 = Vec::new();
    // PalmDOC header
    record0.extend(be16(1)); // no compression
    record0.extend(be16(0));
    record0.extend(be32(html_bytes.len() as u32 + title.len() as u32));
    record0.extend(be16(1)); // record count
    record0.extend(be16(4096));
    record0.extend(be16(0)); // encryption
    record0.extend(be16(0));
    // MOBI header
    record0.extend(b"MOBI");
    record0.extend(be32(232));
    let mut body = vec![0u8; 224];
    let put = |body: &mut Vec<u8>, off: usize, v: u32| {
        body[off..off + 4].copy_from_slice(&v.to_be_bytes());
    };
    put(&mut body, 0, 2); // book
    put(&mut body, 4, 65001); // utf-8
    put(&mut body, 8, 1234); // id
    put(&mut body, 12, 6); // version
    for off in [16, 20, 24, 28] {
        put(&mut body, off, 0xFFFF_FFFF);
    }
    for off in [32, 36, 40, 44, 48, 52] {
        put(&mut body, off, 0xFFFF_FFFF);
    }
    put(&mut body, 56, 1); // first non-book index
    put(&mut body, 60, 248); // name offset (start of the title text)
    put(&mut body, 64, title.len() as u32);
    put(&mut body, 84, 0xFFFF_FFFF); // first image index
    put(&mut body, 88, 0xFFFF_FFFF); // first huff record
    put(&mut body, 104, 0); // exth flags
    put(&mut body, 144, 0xFFFF_FFFF); // drm offset
    put(&mut body, 148, 0xFFFF_FFFF); // drm count
    put(&mut body, 152, 0xFFFF_FFFF); // drm size
    put(&mut body, 156, 0xFFFF_FFFF); // drm flags
    body[168..170].copy_from_slice(&0u16.to_be_bytes()); // first content record
    body[170..172].copy_from_slice(&0u16.to_be_bytes()); // last content record
    put(&mut body, 176, 0xFFFF_FFFF); // fcis
    put(&mut body, 184, 0xFFFF_FFFF); // flis
    put(&mut body, 208, 0xFFFF_FFFF); // data section count
    put(&mut body, 220, 0xFFFF_FFFF); // first index record
    record0.extend(body);
    record0.extend_from_slice(html_bytes);

    let record0_offset: u32 = 76 + 2 + 8 + 2;
    let mut file = Vec::new();
    let mut name = [0u8; 32];
    name[..12].copy_from_slice(b"Fixture MOBI");
    file.extend(name);
    file.extend(be16(0)); // attributes
    file.extend(be16(0)); // version
    file.extend(be32(0));
    file.extend(be32(0));
    file.extend(be32(0));
    file.extend(be32(0));
    file.extend(be32(0)); // app info
    file.extend(be32(0)); // sort info
    file.extend(b"BOOK");
    file.extend(b"MOBI");
    file.extend(be32(1)); // seed
    file.extend(be32(0)); // next record list
    file.extend(be16(1)); // num records
    file.extend(be32(record0_offset));
    file.push(0);
    file.extend(&[0u8, 0, 0]); // attributes + unique id
    file.extend(be16(0)); // padding
    assert_eq!(file.len() as u32, record0_offset);
    file.extend(record0);

    std::fs::write(path, file).unwrap();
}

#[test]
fn opens_generated_epub() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fixture.epub");
    write_epub(&path);

    let book = epub_reader::book::open(&path).expect("book opens");
    assert_eq!(book.title, "Fixture EPUB");
    assert_eq!(book.author.as_deref(), Some("Fixture Author"));
    assert_eq!(book.chapters.len(), 2);
    assert!(book.chapters[0].html.contains("Chapter One"));
    assert_eq!(book.toc.len(), 2);
    assert_eq!(book.toc[0].label, "Chapter One");
    assert_eq!(book.toc[0].chapter, 0);
    assert_eq!(book.toc[1].chapter, 1);
    assert!(book.assets.contains_key("OEBPS/images/pic.png"));
    assert_eq!(book.chapter_paths[0], "OEBPS/text/ch1.xhtml");
}

#[test]
fn opens_generated_kepub_extension() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fixture.kepub.epub");
    write_epub(&path);
    let book = epub_reader::book::open(&path).expect("book opens");
    assert_eq!(book.title, "Fixture EPUB");
}

#[test]
fn opens_generated_mobi() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fixture.mobi");
    write_mobi(&path);

    let book = epub_reader::book::open(&path).expect("book opens");
    assert_eq!(book.title, "Fixture MOBI Book");
    assert_eq!(book.chapters.len(), 2);
    assert!(book.chapters[0].html.contains("MOBI text one"));
    assert!(book.chapters[1].html.contains("MOBI text two"));
    assert_eq!(book.toc.len(), 2);
    assert_eq!(book.toc[0].label, "Part One");
}

#[test]
fn rejects_garbage() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("junk.epub");
    std::fs::write(&path, b"this is not a book at all").unwrap();
    assert!(epub_reader::book::open(&path).is_err());
}
