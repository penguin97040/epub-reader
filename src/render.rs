//! XHTML → block tree. Book markup is untrusted, so this parser never panics:
//! anything it does not understand is rendered as plain text or skipped.

use html5ever::tendril::TendrilSink;
use html5ever::{local_name, parse_document};
use markup5ever_rcdom::{Handle, NodeData, RcDom};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpanStyle {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub code: bool,
    pub superscript: bool,
    pub subscript: bool,
    /// Relative font size, e.g. from `<small>` or an inline `font-size`.
    pub scale: f32,
}

impl Default for SpanStyle {
    fn default() -> Self {
        Self {
            bold: false,
            italic: false,
            underline: false,
            strike: false,
            code: false,
            superscript: false,
            subscript: false,
            scale: 1.0,
        }
    }
}

impl SpanStyle {
    fn new() -> Self {
        Self::default()
    }

    fn merged(&self, other: &SpanStyle) -> Self {
        Self {
            bold: self.bold || other.bold,
            italic: self.italic || other.italic,
            underline: self.underline || other.underline,
            strike: self.strike || other.strike,
            code: self.code || other.code,
            superscript: self.superscript || other.superscript,
            subscript: self.subscript || other.subscript,
            scale: self.scale * other.scale,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Inline {
    Text {
        text: String,
        style: SpanStyle,
        href: Option<String>,
    },
    Break,
    Image {
        src: String,
        alt: String,
    },
}

#[derive(Debug, Clone)]
pub enum Block {
    Heading {
        level: u8,
        inlines: Vec<Inline>,
        align: Align,
    },
    Paragraph {
        inlines: Vec<Inline>,
        align: Align,
        first_indent: f32,
    },
    List {
        ordered: bool,
        start: u64,
        items: Vec<Vec<Block>>,
    },
    Quote(Vec<Block>),
    Pre(String),
    Image {
        src: String,
        alt: String,
    },
    Rule,
    Table(Vec<Vec<Vec<Block>>>),
}

impl Block {
    pub fn is_empty(&self) -> bool {
        match self {
            Block::Paragraph { inlines, .. } | Block::Heading { inlines, .. } => {
                inlines.iter().all(|i| match i {
                    Inline::Text { text, .. } => text.trim().is_empty(),
                    _ => false,
                })
            }
            _ => false,
        }
    }
}

/// Parse a chapter's XHTML into a block tree.
pub fn parse_blocks(html: &str) -> Vec<Block> {
    let dom = parse_document(RcDom::default(), Default::default())
        .from_utf8()
        .read_from(&mut html.as_bytes())
        .unwrap_or_default();
    let mut out = Vec::new();
    let state = StyleState::default();
    walk_blocks(&dom.document, &state, &mut out);
    out
}

#[derive(Debug, Clone, Default)]
struct StyleState {
    span: SpanStyle,
    href: Option<String>,
    align: Option<Align>,
}

fn walk_blocks(node: &Handle, state: &StyleState, out: &mut Vec<Block>) {
    match &node.data {
        NodeData::Element { name, attrs, .. } => {
            let tag = name.local.as_ref();
            let attrs = attrs.borrow();
            if is_skipped(tag) {
                return;
            }
            if style_attr(&attrs)
                .iter()
                .any(|(k, v)| k == "display" && v == "none" || k == "visibility" && v == "hidden")
            {
                return;
            }
            let mut state = state.clone();
            apply_block_attrs(&attrs, &mut state);

            match tag {
                "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                    let level = tag[1..].parse().unwrap_or(6);
                    let mut inlines = Vec::new();
                    for child in node.children.borrow().iter() {
                        walk_inlines(child, &state, &mut inlines);
                    }
                    clean_inlines(&mut inlines);
                    out.push(Block::Heading {
                        level,
                        inlines,
                        align: state.align.unwrap_or_default(),
                    });
                }
                "p" => {
                    let mut inlines = Vec::new();
                    for child in node.children.borrow().iter() {
                        walk_inlines(child, &state, &mut inlines);
                    }
                    clean_inlines(&mut inlines);
                    let first_indent = indent_attr(&attrs);
                    out.push(Block::Paragraph {
                        inlines,
                        align: state.align.unwrap_or_default(),
                        first_indent,
                    });
                }
                "ul" | "ol" => {
                    let ordered = tag == "ol";
                    let start = attrs
                        .iter()
                        .find(|a| a.name.local == local_name!("start"))
                        .and_then(|a| a.value.parse().ok())
                        .unwrap_or(1);
                    let mut items = Vec::new();
                    for child in node.children.borrow().iter() {
                        let child_tag = element_tag(child);
                        if child_tag.as_deref() == Some("li") {
                            let mut item = Vec::new();
                            let mut inlines = Vec::new();
                            for grand in child.children.borrow().iter() {
                                if is_block_level(element_tag(grand).as_deref()) {
                                    if !inlines.is_empty() {
                                        clean_inlines(&mut inlines);
                                        item.push(Block::Paragraph {
                                            inlines: std::mem::take(&mut inlines),
                                            align: Align::Left,
                                            first_indent: 0.0,
                                        });
                                    }
                                    walk_blocks(grand, &state, &mut item);
                                } else {
                                    walk_inlines(grand, &state, &mut inlines);
                                }
                            }
                            if !inlines.is_empty() {
                                clean_inlines(&mut inlines);
                                item.push(Block::Paragraph {
                                    inlines,
                                    align: Align::Left,
                                    first_indent: 0.0,
                                });
                            }
                            items.push(item);
                        }
                    }
                    out.push(Block::List {
                        ordered,
                        start,
                        items,
                    });
                }
                "blockquote" => {
                    let mut inner = Vec::new();
                    for child in node.children.borrow().iter() {
                        walk_blocks(child, &state, &mut inner);
                    }
                    out.push(Block::Quote(inner));
                }
                "pre" => {
                    let mut text = String::new();
                    collect_text(node, &mut text);
                    out.push(Block::Pre(text));
                }
                "hr" => out.push(Block::Rule),
                "img" => {
                    let src = image_src(&attrs);
                    if let Some(src) = src {
                        let alt = attr(&attrs, "alt").unwrap_or_default();
                        out.push(Block::Image { src, alt });
                    }
                }
                "svg" => {
                    if let Some(src) = find_svg_image(node) {
                        out.push(Block::Image {
                            src,
                            alt: String::new(),
                        });
                    }
                }
                "table" => {
                    let mut rows = Vec::new();
                    collect_table_rows(node, &state, &mut rows);
                    if !rows.is_empty() {
                        out.push(Block::Table(rows));
                    }
                }
                "br" => {}
                _ => {
                    // Container or unknown element: try blocks first, fall back
                    // to a paragraph when it only holds inline content.
                    let mut inner = Vec::new();
                    for child in node.children.borrow().iter() {
                        walk_blocks(child, &state, &mut inner);
                    }
                    if inner.is_empty() {
                        let mut inlines = Vec::new();
                        for child in node.children.borrow().iter() {
                            walk_inlines(child, &state, &mut inlines);
                        }
                        clean_inlines(&mut inlines);
                        if !inlines.is_empty() {
                            out.push(Block::Paragraph {
                                inlines,
                                align: state.align.unwrap_or_default(),
                                first_indent: 0.0,
                            });
                        }
                    } else {
                        out.extend(inner);
                    }
                }
            }
        }
        NodeData::Text { contents } => {
            let text = contents.borrow().to_string();
            if !text.trim().is_empty() {
                out.push(Block::Paragraph {
                    inlines: vec![Inline::Text {
                        text,
                        style: state.span,
                        href: state.href.clone(),
                    }],
                    align: state.align.unwrap_or_default(),
                    first_indent: 0.0,
                });
            }
        }
        _ => {
            for child in node.children.borrow().iter() {
                walk_blocks(child, state, out);
            }
        }
    }
}

fn walk_inlines(node: &Handle, state: &StyleState, out: &mut Vec<Inline>) {
    match &node.data {
        NodeData::Text { contents } => {
            let text = contents.borrow().to_string();
            if !text.is_empty() {
                out.push(Inline::Text {
                    text,
                    style: state.span,
                    href: state.href.clone(),
                });
            }
        }
        NodeData::Element { name, attrs, .. } => {
            let tag = name.local.as_ref();
            let attrs = attrs.borrow();
            if is_skipped(tag) {
                return;
            }
            if style_attr(&attrs)
                .iter()
                .any(|(k, v)| k == "display" && v == "none" || k == "visibility" && v == "hidden")
            {
                return;
            }
            let mut state = state.clone();
            apply_inline_tag(tag, &mut state);
            apply_style_attr(&attrs, &mut state);

            match tag {
                "br" => out.push(Inline::Break),
                "img" => {
                    if let Some(src) = image_src(&attrs) {
                        let alt = attr(&attrs, "alt").unwrap_or_default();
                        out.push(Inline::Image { src, alt });
                    }
                }
                "svg" => {
                    if let Some(src) = find_svg_image(node) {
                        out.push(Inline::Image {
                            src,
                            alt: String::new(),
                        });
                    }
                }
                "a" => {
                    if let Some(href) = attr(&attrs, "href") {
                        let href = href.trim().to_owned();
                        if !href.is_empty() && !href.starts_with('#') {
                            state.href = Some(href);
                        }
                    }
                    recurse_inlines(node, &state, out);
                }
                _ => recurse_inlines(node, &state, out),
            }
        }
        _ => recurse_inlines(node, state, out),
    }
}

fn recurse_inlines(node: &Handle, state: &StyleState, out: &mut Vec<Inline>) {
    for child in node.children.borrow().iter() {
        walk_inlines(child, state, out);
    }
}

fn apply_inline_tag(tag: &str, state: &mut StyleState) {
    let mut extra = SpanStyle::new();
    match tag {
        "strong" | "b" => extra.bold = true,
        "em" | "i" | "cite" | "var" | "dfn" | "address" => extra.italic = true,
        "u" | "ins" => extra.underline = true,
        "s" | "strike" | "del" => extra.strike = true,
        "code" | "tt" | "kbd" | "samp" => extra.code = true,
        "sup" => extra.superscript = true,
        "sub" => extra.subscript = true,
        "small" => extra.scale *= 0.85,
        "big" => extra.scale *= 1.15,
        _ => {}
    }
    state.span = state.span.merged(&extra);
}

fn apply_block_attrs(attrs: &[markup5ever::Attribute], state: &mut StyleState) {
    if let Some(align) = attr(attrs, "align").as_deref().and_then(parse_align) {
        state.align = Some(align);
    }
    apply_style_attr(attrs, state);
}

fn apply_style_attr(attrs: &[markup5ever::Attribute], state: &mut StyleState) {
    for (key, value) in style_attr(attrs) {
        match key.as_str() {
            "font-weight" => {
                if matches!(
                    value.as_str(),
                    "bold" | "bolder" | "600" | "700" | "800" | "900"
                ) {
                    state.span.bold = true;
                }
            }
            "font-style" => {
                if value == "italic" || value == "oblique" {
                    state.span.italic = true;
                }
            }
            "text-decoration" => {
                if value.contains("underline") {
                    state.span.underline = true;
                }
                if value.contains("line-through") {
                    state.span.strike = true;
                }
            }
            "text-align" => {
                if let Some(align) = parse_align(&value) {
                    state.align = Some(align);
                }
            }
            "font-size" => {
                if let Some(scale) = parse_font_size_keyword(&value) {
                    state.span.scale *= scale;
                }
            }
            _ => {}
        }
    }
}

fn parse_font_size_keyword(value: &str) -> Option<f32> {
    Some(match value.trim() {
        "xx-small" => 0.7,
        "x-small" => 0.78,
        "small" => 0.86,
        "medium" => 1.0,
        "large" => 1.15,
        "x-large" => 1.3,
        "xx-large" => 1.5,
        "smaller" => 0.85,
        "larger" => 1.2,
        _ => return None,
    })
}

fn parse_align(value: &str) -> Option<Align> {
    Some(match value.trim() {
        "center" | "centre" => Align::Center,
        "right" => Align::Right,
        "justify" => Align::Justify,
        "left" | "start" => Align::Left,
        _ => return None,
    })
}

fn indent_attr(attrs: &[markup5ever::Attribute]) -> f32 {
    style_attr(attrs)
        .iter()
        .find(|(k, _)| k == "text-indent")
        .and_then(|(_, v)| {
            let trimmed = v.trim_end_matches("px").trim_end_matches("em").trim();
            trimmed.parse::<f32>().ok()
        })
        .unwrap_or(0.0)
        .clamp(-64.0, 64.0)
}

fn style_attr(attrs: &[markup5ever::Attribute]) -> Vec<(String, String)> {
    let Some(raw) = attr(attrs, "style") else {
        return Vec::new();
    };
    raw.split(';')
        .filter_map(|pair| {
            let (k, v) = pair.split_once(':')?;
            Some((k.trim().to_ascii_lowercase(), v.trim().to_ascii_lowercase()))
        })
        .collect()
}

fn attr(attrs: &[markup5ever::Attribute], name: &str) -> Option<String> {
    attrs
        .iter()
        .find(|a| a.name.local.as_ref() == name)
        .map(|a| a.value.to_string())
}

fn image_src(attrs: &[markup5ever::Attribute]) -> Option<String> {
    attr(attrs, "src")
        .or_else(|| attr(attrs, "href"))
        .or_else(|| attr(attrs, "xlink:href"))
        .or_else(|| {
            attr(attrs, "recindex").and_then(|i| {
                i.trim()
                    .parse::<u32>()
                    .ok()
                    .map(|n| format!("recindex:{n:06}"))
            })
        })
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
}

fn find_svg_image(node: &Handle) -> Option<String> {
    for child in node.children.borrow().iter() {
        if let NodeData::Element { name, attrs, .. } = &child.data {
            if name.local.as_ref() == "image" {
                let attrs = attrs.borrow();
                if let Some(src) = attr(&attrs, "href").or_else(|| attr(&attrs, "xlink:href")) {
                    return Some(src);
                }
            }
            if let Some(src) = find_svg_image(child) {
                return Some(src);
            }
        } else if let Some(src) = find_svg_image(child) {
            return Some(src);
        }
    }
    None
}

fn collect_table_rows(node: &Handle, state: &StyleState, rows: &mut Vec<Vec<Vec<Block>>>) {
    for child in node.children.borrow().iter() {
        match element_tag(child).as_deref() {
            Some("tr") => {
                let mut cells = Vec::new();
                for cell in child.children.borrow().iter() {
                    let tag = element_tag(cell).unwrap_or_default();
                    if tag == "td" || tag == "th" {
                        let mut blocks = Vec::new();
                        let mut inlines = Vec::new();
                        for grand in cell.children.borrow().iter() {
                            if is_block_level(element_tag(grand).as_deref()) {
                                if !inlines.is_empty() {
                                    clean_inlines(&mut inlines);
                                    blocks.push(Block::Paragraph {
                                        inlines: std::mem::take(&mut inlines),
                                        align: Align::Left,
                                        first_indent: 0.0,
                                    });
                                }
                                walk_blocks(grand, state, &mut blocks);
                            } else {
                                walk_inlines(grand, state, &mut inlines);
                            }
                        }
                        if !inlines.is_empty() {
                            clean_inlines(&mut inlines);
                            blocks.push(Block::Paragraph {
                                inlines,
                                align: Align::Left,
                                first_indent: 0.0,
                            });
                        }
                        cells.push(blocks);
                    }
                }
                if !cells.is_empty() {
                    rows.push(cells);
                }
            }
            Some("thead") | Some("tbody") | Some("tfoot") => {
                collect_table_rows(child, state, rows);
            }
            _ => {}
        }
    }
}

fn collect_text(node: &Handle, out: &mut String) {
    match &node.data {
        NodeData::Text { contents } => out.push_str(&contents.borrow()),
        NodeData::Element { name, .. } if name.local == local_name!("br") => out.push('\n'),
        NodeData::Element { .. } => {
            for child in node.children.borrow().iter() {
                collect_text(child, out);
            }
        }
        _ => {
            for child in node.children.borrow().iter() {
                collect_text(child, out);
            }
        }
    }
}

fn element_tag(node: &Handle) -> Option<String> {
    match &node.data {
        NodeData::Element { name, .. } => Some(name.local.to_string()),
        _ => None,
    }
}

fn is_skipped(tag: &str) -> bool {
    matches!(
        tag,
        "script" | "style" | "head" | "title" | "meta" | "link" | "noscript" | "template"
    )
}

fn is_block_level(tag: Option<&str>) -> bool {
    matches!(
        tag,
        Some(
            "p" | "div"
                | "h1"
                | "h2"
                | "h3"
                | "h4"
                | "h5"
                | "h6"
                | "ul"
                | "ol"
                | "li"
                | "blockquote"
                | "pre"
                | "hr"
                | "table"
                | "section"
                | "article"
                | "figure"
                | "figcaption"
                | "header"
                | "footer"
                | "nav"
                | "aside"
                | "main"
        )
    )
}

/// Collapse whitespace across inline fragments and trim the ends.
fn clean_inlines(inlines: &mut Vec<Inline>) {
    let mut cleaned: Vec<Inline> = Vec::with_capacity(inlines.len());
    let mut pending_space = false;
    for inline in inlines.drain(..) {
        match inline {
            Inline::Text { text, style, href } => {
                let mut collapsed = String::with_capacity(text.len());
                let mut ws = pending_space;
                for ch in text.chars() {
                    if ch.is_whitespace() {
                        ws = true;
                    } else {
                        if ws && (!collapsed.is_empty() || !cleaned.is_empty()) {
                            collapsed.push(' ');
                        }
                        ws = false;
                        collapsed.push(ch);
                    }
                }
                pending_space = ws;
                if !collapsed.is_empty() {
                    cleaned.push(Inline::Text {
                        text: collapsed,
                        style,
                        href,
                    });
                }
            }
            Inline::Break => {
                cleaned.push(Inline::Break);
                pending_space = false;
            }
            Inline::Image { src, alt } => {
                cleaned.push(Inline::Image { src, alt });
                pending_space = false;
            }
        }
    }
    if let Some(Inline::Text { text, .. }) = cleaned.first_mut() {
        *text = text.trim_start().to_owned();
    }
    if let Some(Inline::Text { text, .. }) = cleaned.last_mut() {
        *text = text.trim_end().to_owned();
    }
    cleaned.retain(|i| !matches!(i, Inline::Text { text, .. } if text.is_empty()));
    *inlines = cleaned;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn first_paragraph(blocks: &[Block]) -> String {
        blocks
            .iter()
            .find_map(|b| match b {
                Block::Paragraph { inlines, .. } => Some(
                    inlines
                        .iter()
                        .map(|i| match i {
                            Inline::Text { text, .. } => text.clone(),
                            _ => String::new(),
                        })
                        .collect::<String>(),
                ),
                _ => None,
            })
            .unwrap_or_default()
    }

    #[test]
    fn parses_headings_and_paragraphs() {
        let blocks =
            parse_blocks("<html><body><h1>Title</h1><p>One <b>bold</b> word.</p></body></html>");
        assert!(matches!(blocks[0], Block::Heading { level: 1, .. }));
        assert_eq!(first_paragraph(&blocks), "One bold word.");
        let Block::Paragraph { inlines, .. } = &blocks[1] else {
            panic!("expected paragraph");
        };
        assert!(matches!(&inlines[1], Inline::Text { style, .. } if style.bold));
    }

    #[test]
    fn plain_text_uses_the_full_font_scale() {
        let blocks = parse_blocks("<p>plain decade</p>");
        let Block::Paragraph { inlines, .. } = &blocks[0] else {
            panic!("expected paragraph");
        };
        for inline in inlines {
            if let Inline::Text { style, .. } = inline {
                assert_eq!(style.scale, 1.0);
            }
        }
    }

    #[test]
    fn collapses_whitespace_across_nodes() {
        let blocks = parse_blocks("<p>Hello   <i>  world </i> again</p>");
        assert_eq!(first_paragraph(&blocks), "Hello world again");
    }

    #[test]
    fn parses_lists_and_quotes() {
        let blocks =
            parse_blocks("<ul><li>one</li><li>two</li></ul><blockquote><p>quoted</p></blockquote>");
        match &blocks[0] {
            Block::List { ordered, items, .. } => {
                assert!(!ordered);
                assert_eq!(items.len(), 2);
            }
            other => panic!("unexpected {other:?}"),
        }
        assert!(matches!(&blocks[1], Block::Quote(inner) if inner.len() == 1));
    }

    #[test]
    fn keeps_links_and_images() {
        let blocks = parse_blocks(
            "<p><a href=\"https://example.com\">site</a> <img src=\"img/a.png\" alt=\"A\"/></p>",
        );
        let Block::Paragraph { inlines, .. } = &blocks[0] else {
            panic!("expected paragraph");
        };
        assert!(
            matches!(&inlines[0], Inline::Text { href: Some(h), .. } if h == "https://example.com")
        );
        assert!(matches!(&inlines[1], Inline::Image { src, .. } if src == "img/a.png"));
    }

    #[test]
    fn maps_mobi_recindex_images() {
        let blocks = parse_blocks("<p><img recindex=\"00012\"/></p>");
        let Block::Paragraph { inlines, .. } = &blocks[0] else {
            panic!("expected paragraph");
        };
        assert!(matches!(&inlines[0], Inline::Image { src, .. } if src == "recindex:000012"));
    }

    #[test]
    fn malformed_markup_does_not_panic() {
        let blocks = parse_blocks("<p>unclosed <b>bold <i>and more</p><div></div>");
        assert!(!blocks.is_empty());
        let _ = parse_blocks("<<<>>>");
        let _ = parse_blocks("");
    }

    #[test]
    fn honours_inline_style_and_align() {
        let blocks = parse_blocks("<p style=\"text-align:center;font-weight:bold\">centred</p>");
        match &blocks[0] {
            Block::Paragraph { inlines, align, .. } => {
                assert_eq!(*align, Align::Center);
                assert!(matches!(&inlines[0], Inline::Text { style, .. } if style.bold));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn skips_hidden_and_script_content() {
        let blocks =
            parse_blocks("<script>evil()</script><p style=\"display:none\">hidden</p><p>shown</p>");
        assert_eq!(blocks.len(), 1);
        assert_eq!(first_paragraph(&blocks), "shown");
    }
}
