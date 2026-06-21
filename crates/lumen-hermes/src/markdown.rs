use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

/// Block-level markdown element suitable for rich UI rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarkdownBlock {
    /// Plain paragraph with safe Pango inline markup.
    Paragraph(String),
    /// Heading with safe Pango inline markup.
    Heading {
        /// Heading level, from 1 to 6.
        level: u8,
        /// Heading text as safe Pango inline markup.
        markup: String,
    },
    /// Fenced or indented code block.
    Code {
        /// Optional language tag from the fence info string.
        language: Option<String>,
        /// Raw code text.
        text: String,
    },
    /// Blockquote containing nested blocks.
    BlockQuote(Vec<MarkdownBlock>),
    /// Ordered or unordered list.
    List {
        /// Whether this is an ordered list.
        ordered: bool,
        /// First number for ordered lists.
        start: u64,
        /// List item blocks.
        items: Vec<Vec<MarkdownBlock>>,
    },
    /// Markdown table.
    Table {
        /// Header row cells as safe Pango inline markup.
        headers: Vec<String>,
        /// Body rows as safe Pango inline markup.
        rows: Vec<Vec<String>>,
    },
    /// Horizontal rule.
    Rule,
}

/// Escapes text for safe insertion into Pango markup.
#[must_use]
pub fn escape_pango_text(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Converts Markdown to safe Pango markup for GTK labels.
///
/// HTML from the Markdown source is escaped, never passed through as markup.
#[must_use]
pub fn markdown_to_pango(input: &str) -> String {
    let mut renderer = PangoRenderer::default();
    for event in Parser::new_ext(input, markdown_options()) {
        renderer.render(event);
    }
    renderer.finish()
}

/// Compatibility wrapper for older call sites.
#[must_use]
pub fn markdownish_to_pango(input: &str) -> String {
    markdown_to_pango(input)
}

/// Parses Markdown into block-level UI elements.
#[must_use]
pub fn markdown_to_blocks(input: &str) -> Vec<MarkdownBlock> {
    let events = Parser::new_ext(input, markdown_options()).collect::<Vec<_>>();
    let mut parser = BlockParser::new(&events);
    parser.parse_blocks_until(None)
}

fn markdown_options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_SMART_PUNCTUATION
        | Options::ENABLE_HEADING_ATTRIBUTES
}

struct BlockParser<'event, 'input> {
    events: &'event [Event<'input>],
    index: usize,
}

impl<'event, 'input> BlockParser<'event, 'input> {
    fn new(events: &'event [Event<'input>]) -> Self {
        Self { events, index: 0 }
    }

    fn parse_blocks_until(&mut self, end: Option<MarkdownEnd>) -> Vec<MarkdownBlock> {
        let mut blocks = Vec::new();
        while let Some(event) = self.next() {
            match event {
                Event::End(tag) if end.is_some_and(|end| end.matches(tag)) => break,
                Event::Start(tag) => match tag {
                    Tag::Paragraph => {
                        let markup = self.render_inline_until(MarkdownEnd::Paragraph);
                        if !markup.trim().is_empty() {
                            blocks.push(MarkdownBlock::Paragraph(markup));
                        }
                    }
                    Tag::Heading { level, .. } => {
                        let markup = self.render_inline_until(MarkdownEnd::Heading);
                        if !markup.trim().is_empty() {
                            blocks.push(MarkdownBlock::Heading {
                                level: heading_level_number(*level),
                                markup,
                            });
                        }
                    }
                    Tag::CodeBlock(kind) => {
                        blocks.push(self.collect_code_block(kind));
                    }
                    Tag::BlockQuote(_) => {
                        let quote_blocks = self.parse_blocks_until(Some(MarkdownEnd::BlockQuote));
                        if !quote_blocks.is_empty() {
                            blocks.push(MarkdownBlock::BlockQuote(quote_blocks));
                        }
                    }
                    Tag::List(start) => {
                        blocks.push(self.parse_list(*start));
                    }
                    Tag::Table(_) => {
                        blocks.push(self.parse_table());
                    }
                    Tag::FootnoteDefinition(name) => {
                        let content = self.render_inline_until(MarkdownEnd::FootnoteDefinition);
                        let label =
                            format!("[{}]: {}", escape_pango_text(name.as_ref()), content.trim());
                        blocks.push(MarkdownBlock::Paragraph(label));
                    }
                    _ => {
                        let markup = self.render_inline_tag(tag);
                        if !markup.trim().is_empty() {
                            blocks.push(MarkdownBlock::Paragraph(markup));
                        }
                    }
                },
                Event::Rule => blocks.push(MarkdownBlock::Rule),
                Event::Text(text) if !text.trim().is_empty() => {
                    blocks.push(MarkdownBlock::Paragraph(escape_pango_text(text.as_ref())));
                }
                Event::Code(code) => blocks.push(MarkdownBlock::Paragraph(format!(
                    "<span font_family=\"monospace\">{}</span>",
                    escape_pango_text(code.as_ref())
                ))),
                Event::Html(html) | Event::InlineHtml(html) if !html.trim().is_empty() => {
                    blocks.push(MarkdownBlock::Paragraph(escape_pango_text(html.as_ref())));
                }
                Event::SoftBreak | Event::HardBreak => {}
                _ => {}
            }
        }
        blocks
    }

    fn parse_list(&mut self, start: Option<u64>) -> MarkdownBlock {
        let mut items = Vec::new();
        while let Some(event) = self.next() {
            match event {
                Event::Start(Tag::Item) => {
                    let item = self.parse_blocks_until(Some(MarkdownEnd::Item));
                    if !item.is_empty() {
                        items.push(item);
                    }
                }
                Event::End(TagEnd::List(_)) => break,
                _ => {}
            }
        }
        MarkdownBlock::List {
            ordered: start.is_some(),
            start: start.unwrap_or(1),
            items,
        }
    }

    fn parse_table(&mut self) -> MarkdownBlock {
        let mut headers = Vec::new();
        let mut rows = Vec::new();
        while let Some(event) = self.next() {
            match event {
                Event::Start(Tag::TableHead) => {
                    headers = self.parse_table_cells_until(MarkdownEnd::TableHead);
                }
                Event::Start(Tag::TableRow) => {
                    rows.push(self.parse_table_cells_until(MarkdownEnd::TableRow));
                }
                Event::End(TagEnd::Table) => break,
                _ => {}
            }
        }
        MarkdownBlock::Table { headers, rows }
    }

    fn parse_table_cells_until(&mut self, end: MarkdownEnd) -> Vec<String> {
        let mut cells = Vec::new();
        while let Some(event) = self.next() {
            match event {
                Event::Start(Tag::TableCell) => {
                    cells.push(self.render_inline_until(MarkdownEnd::TableCell));
                }
                Event::End(tag) if end.matches(tag) => break,
                _ => {}
            }
        }
        cells
    }

    fn collect_code_block(&mut self, kind: &CodeBlockKind<'input>) -> MarkdownBlock {
        let language = match kind {
            CodeBlockKind::Fenced(language) => language
                .split_whitespace()
                .next()
                .map(str::trim)
                .filter(|language| !language.is_empty())
                .map(str::to_owned),
            CodeBlockKind::Indented => None,
        };
        let mut text = String::new();
        while let Some(event) = self.next() {
            match event {
                Event::End(TagEnd::CodeBlock) => break,
                Event::Text(value) | Event::Code(value) => text.push_str(value.as_ref()),
                Event::SoftBreak | Event::HardBreak => text.push('\n'),
                Event::Html(value) | Event::InlineHtml(value) => text.push_str(value.as_ref()),
                _ => {}
            }
        }
        MarkdownBlock::Code { language, text }
    }

    fn render_inline_tag(&mut self, start: &Tag<'input>) -> String {
        match start {
            Tag::Emphasis => self.render_inline_container(MarkdownEnd::Emphasis, "<i>", "</i>"),
            Tag::Strong => self.render_inline_container(MarkdownEnd::Strong, "<b>", "</b>"),
            Tag::Strikethrough => {
                self.render_inline_container(MarkdownEnd::Strikethrough, "<s>", "</s>")
            }
            Tag::Link { dest_url, .. } => self.render_link(dest_url.as_ref(), false),
            Tag::Image { dest_url, .. } => self.render_link(dest_url.as_ref(), true),
            _ => String::new(),
        }
    }

    fn render_inline_container(&mut self, end: MarkdownEnd, open: &str, close: &str) -> String {
        let content = self.render_inline_until(end);
        if content.is_empty() {
            String::new()
        } else {
            format!("{open}{content}{close}")
        }
    }

    fn render_link(&mut self, destination: &str, image: bool) -> String {
        let label = if image {
            self.render_inline_until(MarkdownEnd::Image)
        } else {
            self.render_inline_until(MarkdownEnd::Link)
        };
        let destination = destination.trim();
        let label = label.trim();
        if destination.is_empty() {
            return label.to_owned();
        }
        let label = if label.is_empty() {
            escape_pango_text(destination)
        } else if image {
            format!("Image: {label}")
        } else {
            label.to_owned()
        };
        format!(
            "<a href=\"{}\"><span foreground=\"#61afef\"><u>{}</u></span></a>",
            escape_pango_text(destination),
            label
        )
    }

    fn render_inline_until(&mut self, end: MarkdownEnd) -> String {
        let mut out = String::new();
        while let Some(event) = self.next() {
            match event {
                Event::End(tag) if end.matches(tag) => break,
                Event::Start(tag) => out.push_str(&self.render_inline_tag(tag)),
                Event::Text(text) => out.push_str(&escape_pango_text(text.as_ref())),
                Event::Code(code) => out.push_str(&format!(
                    "<span font_family=\"monospace\">{}</span>",
                    escape_pango_text(code.as_ref())
                )),
                Event::Html(html) | Event::InlineHtml(html) => {
                    out.push_str(&escape_pango_text(html.as_ref()));
                }
                Event::FootnoteReference(name) => {
                    out.push('[');
                    out.push_str(&escape_pango_text(name.as_ref()));
                    out.push(']');
                }
                Event::SoftBreak | Event::HardBreak => out.push('\n'),
                Event::TaskListMarker(checked) => {
                    out.push_str(if *checked { "[x] " } else { "[ ] " });
                }
                Event::Rule => out.push_str("-----"),
                _ => {}
            }
        }
        out
    }

    fn next(&mut self) -> Option<&'event Event<'input>> {
        let event = self.events.get(self.index)?;
        self.index += 1;
        Some(event)
    }
}

#[derive(Clone, Copy)]
enum MarkdownEnd {
    Paragraph,
    Heading,
    BlockQuote,
    Item,
    Emphasis,
    Strong,
    Strikethrough,
    Link,
    Image,
    TableHead,
    TableRow,
    TableCell,
    FootnoteDefinition,
}

impl MarkdownEnd {
    fn matches(self, tag: &TagEnd) -> bool {
        matches!(
            (self, tag),
            (Self::Paragraph, TagEnd::Paragraph)
                | (Self::Heading, TagEnd::Heading(_))
                | (Self::BlockQuote, TagEnd::BlockQuote(_))
                | (Self::Item, TagEnd::Item)
                | (Self::Emphasis, TagEnd::Emphasis)
                | (Self::Strong, TagEnd::Strong)
                | (Self::Strikethrough, TagEnd::Strikethrough)
                | (Self::Link, TagEnd::Link)
                | (Self::Image, TagEnd::Image)
                | (Self::TableHead, TagEnd::TableHead)
                | (Self::TableRow, TagEnd::TableRow)
                | (Self::TableCell, TagEnd::TableCell)
                | (Self::FootnoteDefinition, TagEnd::FootnoteDefinition)
        )
    }
}

struct PangoRenderer {
    out: String,
    line_start: bool,
    quote_depth: usize,
    list_stack: Vec<ListState>,
    link_stack: Vec<LinkState>,
    table_cell: usize,
}

struct ListState {
    next: u64,
    ordered: bool,
}

struct LinkState {
    destination: String,
    text: String,
    image: bool,
}

impl Default for PangoRenderer {
    fn default() -> Self {
        Self {
            out: String::new(),
            line_start: true,
            quote_depth: 0,
            list_stack: Vec::new(),
            link_stack: Vec::new(),
            table_cell: 0,
        }
    }
}

impl PangoRenderer {
    fn render(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.start_tag(tag),
            Event::End(tag) => self.end_tag(tag),
            Event::Text(text) => self.text(&text),
            Event::Code(code) => {
                self.push_markup("<span font_family=\"monospace\">");
                self.text(&code);
                self.push_markup("</span>");
            }
            Event::Html(html) | Event::InlineHtml(html) => self.text(&html),
            Event::FootnoteReference(name) => {
                self.text("[");
                self.text(&name);
                self.text("]");
            }
            Event::SoftBreak | Event::HardBreak => self.newline(),
            Event::Rule => {
                self.ensure_block();
                self.text("-----");
                self.ensure_block();
            }
            Event::TaskListMarker(checked) => {
                self.text(if checked { "[x] " } else { "[ ] " });
            }
            _ => {}
        }
    }

    fn start_tag(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => self.ensure_line(),
            Tag::Heading { level, .. } => {
                self.ensure_block();
                self.push_markup(heading_open_tag(level));
            }
            Tag::BlockQuote(_) => {
                self.ensure_block();
                self.quote_depth += 1;
            }
            Tag::CodeBlock(kind) => {
                self.ensure_block();
                if let CodeBlockKind::Fenced(language) = kind
                    && !language.is_empty()
                {
                    self.push_markup("<span foreground=\"#8f9aa8\">");
                    self.text(language.as_ref());
                    self.push_markup("</span>");
                    self.newline();
                }
                self.push_markup("<span font_family=\"monospace\">");
            }
            Tag::List(start) => {
                self.ensure_line();
                self.list_stack.push(ListState {
                    next: start.unwrap_or(1),
                    ordered: start.is_some(),
                });
            }
            Tag::Item => self.start_list_item(),
            Tag::Emphasis => self.push_markup("<i>"),
            Tag::Strong => self.push_markup("<b>"),
            Tag::Strikethrough => self.push_markup("<s>"),
            Tag::Link { dest_url, .. } => {
                self.link_stack.push(LinkState {
                    destination: dest_url.to_string(),
                    text: String::new(),
                    image: false,
                });
                self.push_markup("<span foreground=\"#61afef\"><u>");
            }
            Tag::Image { dest_url, .. } => {
                self.link_stack.push(LinkState {
                    destination: dest_url.to_string(),
                    text: String::new(),
                    image: true,
                });
                self.push_markup("<i>");
                self.text("Image: ");
            }
            Tag::Table(_) => self.ensure_block(),
            Tag::TableHead | Tag::TableRow => {
                self.ensure_line();
                self.table_cell = 0;
            }
            Tag::TableCell => {
                if self.table_cell > 0 {
                    self.text(" | ");
                }
                self.table_cell += 1;
            }
            Tag::FootnoteDefinition(name) => {
                self.ensure_block();
                self.text("[");
                self.text(&name);
                self.text("]: ");
            }
            _ => {}
        }
    }

    fn end_tag(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => self.ensure_block(),
            TagEnd::Heading(_) => {
                self.push_markup("</span>");
                self.ensure_block();
            }
            TagEnd::BlockQuote(_) => {
                self.quote_depth = self.quote_depth.saturating_sub(1);
                self.ensure_block();
            }
            TagEnd::CodeBlock => {
                self.push_markup("</span>");
                self.ensure_block();
            }
            TagEnd::List(_) => {
                self.list_stack.pop();
                self.ensure_line();
            }
            TagEnd::Item => self.newline(),
            TagEnd::Emphasis => self.push_markup("</i>"),
            TagEnd::Strong => self.push_markup("</b>"),
            TagEnd::Strikethrough => self.push_markup("</s>"),
            TagEnd::Link => self.end_link(false),
            TagEnd::Image => self.end_link(true),
            TagEnd::TableHead | TagEnd::TableRow => self.newline(),
            TagEnd::Table => self.ensure_block(),
            TagEnd::TableCell | TagEnd::FootnoteDefinition => {}
            _ => {}
        }
    }

    fn start_list_item(&mut self) {
        self.ensure_line();
        let indent = self.list_stack.len().saturating_sub(1) * 2;
        self.text(&" ".repeat(indent));
        if let Some(list) = self.list_stack.last_mut() {
            if list.ordered {
                let marker = format!("{}. ", list.next);
                list.next += 1;
                self.text(&marker);
            } else {
                self.text("- ");
            }
        } else {
            self.text("- ");
        }
    }

    fn end_link(&mut self, image: bool) {
        let Some(link) = self.link_stack.pop() else {
            return;
        };
        if image || link.image {
            self.push_markup("</i>");
        } else {
            self.push_markup("</u></span>");
        }
        let destination = link.destination.trim();
        if !destination.is_empty() && destination != link.text.trim() {
            self.text(" (");
            self.text(destination);
            self.text(")");
        }
    }

    fn text(&mut self, text: &str) {
        if let Some(link) = self.link_stack.last_mut() {
            link.text.push_str(text);
        }

        for (index, segment) in text.split('\n').enumerate() {
            if index > 0 {
                self.newline();
            }
            if segment.is_empty() {
                continue;
            }
            self.ensure_prefixed_line();
            self.out.push_str(&escape_pango_text(segment));
            self.line_start = false;
        }
    }

    fn push_markup(&mut self, markup: &str) {
        self.ensure_prefixed_line();
        self.out.push_str(markup);
        self.line_start = false;
    }

    fn ensure_prefixed_line(&mut self) {
        if !self.line_start {
            return;
        }
        if self.quote_depth > 0 {
            self.out.push_str(&"&gt;".repeat(self.quote_depth));
            self.out.push(' ');
        }
        self.line_start = false;
    }

    fn ensure_line(&mut self) {
        if !self.out.is_empty() && !self.out.ends_with('\n') {
            self.newline();
        }
    }

    fn ensure_block(&mut self) {
        if self.out.is_empty() {
            return;
        }
        if !self.out.ends_with('\n') {
            self.newline();
        }
        if !self.out.ends_with("\n\n") {
            self.newline();
        }
    }

    fn newline(&mut self) {
        self.out.push('\n');
        self.line_start = true;
    }

    fn finish(mut self) -> String {
        while self.out.ends_with('\n') {
            self.out.pop();
        }
        self.out
    }
}

fn heading_open_tag(level: pulldown_cmark::HeadingLevel) -> &'static str {
    match level {
        pulldown_cmark::HeadingLevel::H1 => "<span weight=\"bold\" size=\"x-large\">",
        pulldown_cmark::HeadingLevel::H2 => "<span weight=\"bold\" size=\"large\">",
        pulldown_cmark::HeadingLevel::H3
        | pulldown_cmark::HeadingLevel::H4
        | pulldown_cmark::HeadingLevel::H5
        | pulldown_cmark::HeadingLevel::H6 => "<span weight=\"bold\">",
    }
}

fn heading_level_number(level: pulldown_cmark::HeadingLevel) -> u8 {
    match level {
        pulldown_cmark::HeadingLevel::H1 => 1,
        pulldown_cmark::HeadingLevel::H2 => 2,
        pulldown_cmark::HeadingLevel::H3 => 3,
        pulldown_cmark::HeadingLevel::H4 => 4,
        pulldown_cmark::HeadingLevel::H5 => 5,
        pulldown_cmark::HeadingLevel::H6 => 6,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_pango_markup() {
        assert_eq!(escape_pango_text("<b>&</b>"), "&lt;b&gt;&amp;&lt;/b&gt;");
    }

    #[test]
    fn renders_common_inline_markdown() {
        let rendered = markdown_to_pango("**bold** _italic_ `code` ~~gone~~");

        assert!(rendered.contains("<b>bold</b>"));
        assert!(rendered.contains("<i>italic</i>"));
        assert!(rendered.contains("<span font_family=\"monospace\">code</span>"));
        assert!(rendered.contains("<s>gone</s>"));
    }

    #[test]
    fn renders_code_blocks_as_escaped_monospace() {
        let rendered = markdown_to_pango("```rust\nlet x = 1 < 2;\n```");

        assert!(rendered.contains("rust"));
        assert!(rendered.contains("font_family=\"monospace\""));
        assert!(rendered.contains("1 &lt; 2"));
    }

    #[test]
    fn renders_markdown_blocks() {
        let rendered = markdown_to_pango(
            "# Plan\n\n> quote\n\n- one\n- [x] done\n\n| A | B |\n| - | - |\n| 1 | 2 |",
        );

        assert!(rendered.contains("size=\"x-large\""));
        assert!(rendered.contains("&gt; quote"));
        assert!(rendered.contains("- one"));
        assert!(rendered.contains("- [x] done"));
        assert!(rendered.contains("A | B"));
        assert!(rendered.contains("1 | 2"));
    }

    #[test]
    fn renders_leading_blockquote_marker() {
        assert_eq!(markdown_to_pango("> hello"), "&gt; hello");
    }

    #[test]
    fn renders_links_without_passing_html_through() {
        let rendered = markdown_to_pango("[docs](https://example.test) <b>raw</b>");

        assert!(rendered.contains("<u>docs</u>"));
        assert!(rendered.contains("(https://example.test)"));
        assert!(rendered.contains("&lt;b&gt;raw&lt;/b&gt;"));
    }

    #[test]
    fn parses_rich_markdown_blocks_for_ui_rendering() {
        let blocks = markdown_to_blocks(
            "# Markdown examples\n\n**Bold** and _italic_\n\n- Bullet item\n- Another item\n\n1. Numbered item\n2. Second item\n\n```python\nprint(\"Hello\")\n```\n\n> Blockquote example\n\n[Example link](https://example.com)\n\n| Column A | Column B |\n| --- | --- |\n| Hello | World |",
        );

        assert!(matches!(
            &blocks[0],
            MarkdownBlock::Heading {
                level: 1,
                markup
            } if markup == "Markdown examples"
        ));
        assert!(matches!(
            &blocks[2],
            MarkdownBlock::List {
                ordered: false,
                items,
                ..
            } if items.len() == 2
        ));
        assert!(matches!(
            &blocks[3],
            MarkdownBlock::List {
                ordered: true,
                start: 1,
                items,
            } if items.len() == 2
        ));
        assert!(matches!(
            &blocks[4],
            MarkdownBlock::Code {
                language: Some(language),
                text
            } if language == "python" && text.contains("print")
        ));
        assert!(matches!(&blocks[5], MarkdownBlock::BlockQuote(blocks) if blocks.len() == 1));
        assert!(matches!(
            &blocks[7],
            MarkdownBlock::Table { headers, rows }
                if headers == &["Column A".to_owned(), "Column B".to_owned()]
                    && rows == &vec![vec!["Hello".to_owned(), "World".to_owned()]]
        ));
    }

    #[test]
    fn block_links_are_clickable_without_raw_url_suffix() {
        let blocks = markdown_to_blocks("[docs](https://example.test)");

        if let MarkdownBlock::Paragraph(markup) = &blocks[0] {
            assert!(markup.contains("<a href=\"https://example.test\">"));
            assert!(markup.contains("<u>docs</u>"));
            assert!(!markup.contains("(https://example.test)"));
        } else {
            assert!(matches!(&blocks[0], MarkdownBlock::Paragraph(_)));
        }
    }
}
