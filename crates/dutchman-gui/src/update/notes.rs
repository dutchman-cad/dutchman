//! Release notes for the update dialog: GitHub's HTML (or a Markdown body) reduced to a small
//! Markdown subset, then drawn as headings, bold, italics, inline code, bullet lists and links
//! shown as their text. Everything else is dropped, so release text can never load or run anything.
use eframe::egui::{self, text::LayoutJob, Color32, FontFamily, FontId, RichText, TextFormat};

/// Longest notes text kept, in bytes.
pub(super) const MAX_NOTES: usize = 8_000;
/// Longest HTML read, in bytes.
const MAX_HTML: usize = 1_048_576;

/// One piece of a line with its emphasis.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Span {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Block {
    Heading(u8, Vec<Span>),
    Paragraph(Vec<Span>),
    Item {
        depth: usize,
        marker: String,
        spans: Vec<Span>,
    },
}

/// Replaces `&amp;`, `&#39;` and the like in one pass, so `&amp;lt;` stays `&lt;`.
fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let decoded = rest.find(';').filter(|end| *end <= 10).and_then(|end| {
            let name = &rest[1..end];
            let c = match name {
                "amp" => '&',
                "lt" => '<',
                "gt" => '>',
                "quot" => '"',
                "apos" => '\'',
                "nbsp" => ' ',
                _ => {
                    let number = name.strip_prefix('#')?;
                    let code = match number.strip_prefix(['x', 'X']) {
                        Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                        None => number.parse().ok()?,
                    };
                    char::from_u32(code).filter(|c| !c.is_control())?
                }
            };
            Some((c, end + 1))
        });
        match decoded {
            Some((c, len)) => {
                out.push(c);
                rest = &rest[len..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

struct Converter {
    out: String,
    lists: Vec<Option<usize>>,
    pre: bool,
    skip: Option<String>,
    space: bool,
}

impl Converter {
    fn line_start(&self) -> bool {
        self.out.is_empty() || self.out.ends_with('\n')
    }
    /// A blank line before the next block.
    fn block(&mut self) {
        self.space = false;
        while self.out.ends_with(' ') {
            self.out.pop();
        }
        if self.out.is_empty() {
            return;
        }
        while !self.out.ends_with("\n\n") {
            self.out.push('\n');
        }
    }
    fn newline(&mut self) {
        self.space = false;
        while self.out.ends_with(' ') {
            self.out.pop();
        }
        if !self.out.is_empty() && !self.out.ends_with('\n') {
            self.out.push('\n');
        }
    }
    fn marks(&mut self, marks: &str, closing: bool) {
        if !closing && self.space {
            self.out.push(' ');
            self.space = false;
        }
        self.out.push_str(marks);
    }
    fn text(&mut self, raw: &str) {
        let text = unescape(raw);
        let mut first = true;
        for c in text.chars() {
            if !self.pre && c.is_whitespace() {
                self.space = !self.line_start();
                continue;
            }
            if self.space {
                self.out.push(' ');
                self.space = false;
            }
            // Characters the Markdown reader would take for emphasis, headings or lists.
            let at_start = self.line_start();
            let escape = matches!(c, '\\' | '*' | '`' | '[' | '!')
                || (at_start && matches!(c, '#' | '-' | '+' | '>' | '|'))
                || (first && self.out.ends_with(|p: char| p.is_ascii_digit()) && c == '.');
            if escape && !self.pre {
                self.out.push('\\');
            }
            self.out.push(c);
            first = false;
        }
    }
    fn tag(&mut self, name: &str, closing: bool) {
        match name {
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                self.block();
                if !closing {
                    let level = name[1..].parse::<usize>().unwrap_or(3).min(3);
                    self.out.push_str(&"#".repeat(level));
                    self.out.push(' ');
                }
            }
            "p" | "div" | "blockquote" | "table" | "hr" => self.block(),
            "pre" => {
                self.block();
                self.pre = !closing;
            }
            "br" => {
                if !closing {
                    self.out.push('\n');
                    self.space = false;
                }
            }
            "ul" | "ol" => {
                if closing {
                    self.lists.pop();
                } else {
                    self.lists.push(if name == "ol" { Some(0) } else { None });
                }
                self.newline();
                if self.lists.is_empty() {
                    self.block();
                }
            }
            "li" if !closing => {
                self.newline();
                let depth = self.lists.len().saturating_sub(1);
                self.out.push_str(&"  ".repeat(depth));
                match self.lists.last_mut() {
                    Some(Some(n)) => {
                        *n += 1;
                        self.out.push_str(&format!("{n}. "));
                    }
                    _ => self.out.push_str("- "),
                }
            }
            "tr" if closing => self.newline(),
            "td" | "th" if closing => {
                self.out.push_str(" | ");
                self.space = false;
            }
            "strong" | "b" => self.marks("**", closing),
            "em" | "i" => self.marks("*", closing),
            "code" if !self.pre => self.marks("`", closing),
            _ => {}
        }
    }
}

/// GitHub's release HTML as Markdown with only headings, emphasis, code and lists.
///
/// Handles h1-h3, p, ul/ol/li, strong/b, em/i, code, a (its text) and br; images, comments,
/// scripts and any other tag are dropped, with their text kept except for script and style.
pub(super) fn html_to_markdown(html: &str) -> String {
    let mut c = Converter {
        out: String::new(),
        lists: Vec::new(),
        pre: false,
        skip: None,
        space: false,
    };
    let cut = (0..=html.len().min(MAX_HTML))
        .rev()
        .find(|at| html.is_char_boundary(*at))
        .unwrap_or(0);
    let html = &html[..cut];
    let mut rest = html;
    while !rest.is_empty() && c.out.len() < MAX_NOTES * 2 {
        let Some(open) = rest.find('<') else {
            if c.skip.is_none() {
                c.text(rest);
            }
            break;
        };
        if c.skip.is_none() {
            c.text(&rest[..open]);
        }
        rest = &rest[open..];
        if let Some(body) = rest.strip_prefix("<!--") {
            rest = body.find("-->").map_or("", |end| &body[end + 3..]);
            continue;
        }
        let Some(end) = rest.find('>') else { break };
        let inner = rest[1..end].trim();
        rest = &rest[end + 1..];
        let (closing, inner) = match inner.strip_prefix('/') {
            Some(inner) => (true, inner),
            None => (false, inner),
        };
        let name: String = inner
            .chars()
            .take_while(|ch| ch.is_ascii_alphanumeric())
            .collect::<String>()
            .to_ascii_lowercase();
        match &c.skip {
            Some(skipping) => {
                if closing && *skipping == name {
                    c.skip = None;
                }
            }
            None if matches!(name.as_str(), "script" | "style") && !closing => {
                c.skip = Some(name);
            }
            None => c.tag(&name, closing),
        }
    }
    let notes = c.out.trim().to_owned();
    match notes.char_indices().nth(MAX_NOTES) {
        Some((at, _)) => notes[..at].to_owned(),
        None => notes,
    }
}

/// The index of the next `target` at or after `from`.
fn find(chars: &[char], from: usize, target: char) -> Option<usize> {
    let tail = chars.get(from..)?;
    tail.iter().position(|c| *c == target).map(|at| from + at)
}

fn collect(chars: &[char], from: usize, to: usize) -> String {
    chars.get(from..to).unwrap_or_default().iter().collect()
}

/// Inline `**bold**`, `*italic*`, `` `code` ``, `[text](url)` shown as text, images dropped.
fn spans(line: &str) -> Vec<Span> {
    let chars: Vec<char> = line.chars().collect();
    let mut out: Vec<Span> = Vec::new();
    let (mut bold, mut italic) = (false, false);
    let mut text = String::new();
    let flush = |text: &mut String, out: &mut Vec<Span>, bold, italic, code| {
        if !text.is_empty() {
            out.push(Span {
                text: std::mem::take(text),
                bold,
                italic,
                code,
            });
        }
    };
    let mut i = 0;
    while i < chars.len() {
        let current = chars.get(i).copied().unwrap_or(' ');
        match current {
            '\\' if i + 1 < chars.len() => {
                text.extend(chars.get(i + 1));
                i += 2;
            }
            '`' => match find(&chars, i + 1, '`') {
                Some(end) => {
                    flush(&mut text, &mut out, bold, italic, false);
                    let len = end - i - 1;
                    let code = collect(&chars, i + 1, end);
                    out.push(Span {
                        text: code,
                        bold,
                        italic,
                        code: true,
                    });
                    i += len + 2;
                }
                None => {
                    text.push('`');
                    i += 1;
                }
            },
            '*' => {
                flush(&mut text, &mut out, bold, italic, false);
                if chars.get(i + 1) == Some(&'*') {
                    bold = !bold;
                    i += 2;
                } else {
                    italic = !italic;
                    i += 1;
                }
            }
            c @ ('[' | '!') => {
                let start = if c == '!' && chars.get(i + 1) == Some(&'[') {
                    i + 1
                } else if c == '[' {
                    i
                } else {
                    text.push(c);
                    i += 1;
                    continue;
                };
                let link = find(&chars, start + 1, ']').and_then(|close| {
                    (chars.get(close + 1) == Some(&'('))
                        .then(|| find(&chars, close + 2, ')').map(|end| (close, end)))
                        .flatten()
                });
                match link {
                    Some((close, end)) => {
                        // An image has nothing to show here; a link shows its words.
                        if c == '[' {
                            let label = collect(&chars, start + 1, close);
                            for mut span in spans(&label) {
                                span.bold |= bold;
                                span.italic |= italic;
                                flush(&mut text, &mut out, bold, italic, false);
                                out.push(span);
                            }
                        }
                        i = end + 1;
                    }
                    None => {
                        text.push(c);
                        i += 1;
                    }
                }
            }
            c => {
                text.push(c);
                i += 1;
            }
        }
    }
    flush(&mut text, &mut out, bold, italic, false);
    out
}

fn is_rule(line: &str) -> bool {
    let line = line.trim();
    line.len() >= 3
        && line
            .chars()
            .all(|c| matches!(c, '-' | '*' | '_' | ' ' | '|' | ':'))
        && line.chars().any(|c| matches!(c, '-' | '*' | '_'))
}

fn list_marker(line: &str) -> Option<(usize, String, &str)> {
    let indent = line.len() - line.trim_start().len();
    let rest = &line[indent..];
    let (marker, tail) = if let Some(tail) = rest
        .strip_prefix("- ")
        .or_else(|| rest.strip_prefix("* "))
        .or_else(|| rest.strip_prefix("+ "))
    {
        ("\u{2022}".to_owned(), tail)
    } else {
        let digits = rest.chars().take_while(char::is_ascii_digit).count();
        let tail = rest[digits..].strip_prefix(". ")?;
        if digits == 0 || digits > 9 {
            return None;
        }
        (format!("{}.", &rest[..digits]), tail)
    };
    Some((indent / 2, marker, tail.trim()))
}

/// Markdown (as made by [`html_to_markdown`], or a GitHub release body) as blocks.
pub(super) fn blocks(markdown: &str) -> Vec<Block> {
    // Comments carry machine-readable release data, not words for people.
    let mut cleaned = String::new();
    let mut rest = markdown;
    while let Some(at) = rest.find("<!--") {
        cleaned.push_str(&rest[..at]);
        rest = rest[at..]
            .find("-->")
            .map_or("", |end| &rest[at + end + 3..]);
    }
    cleaned.push_str(rest);
    let mut blocks: Vec<Block> = Vec::new();
    let mut paragraph = String::new();
    let flush = |paragraph: &mut String, blocks: &mut Vec<Block>| {
        if !paragraph.is_empty() {
            blocks.push(Block::Paragraph(spans(paragraph)));
            paragraph.clear();
        }
    };
    for line in cleaned.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            flush(&mut paragraph, &mut blocks);
        } else if let Some(heading) = trimmed.strip_prefix('#') {
            flush(&mut paragraph, &mut blocks);
            let level = 1 + heading.chars().take_while(|c| *c == '#').count();
            let title = heading.trim_start_matches('#');
            if title.starts_with(' ') && level <= 6 {
                blocks.push(Block::Heading(level.min(3) as u8, spans(title.trim())));
            } else {
                paragraph.push_str(trimmed);
            }
        } else if let Some((depth, marker, item)) = list_marker(line) {
            flush(&mut paragraph, &mut blocks);
            blocks.push(Block::Item {
                depth,
                marker,
                spans: spans(item),
            });
        } else if is_rule(trimmed) {
            flush(&mut paragraph, &mut blocks);
        } else if trimmed.starts_with('|') {
            flush(&mut paragraph, &mut blocks);
            let row = trimmed.trim_matches('|').replace(" | ", " \u{b7} ");
            let row = row.replace('|', " \u{b7} ");
            blocks.push(Block::Paragraph(spans(row.trim())));
        } else if let (Some(Block::Item { spans: item, .. }), true, true) = (
            blocks.last_mut(),
            paragraph.is_empty(),
            line.starts_with(' '),
        ) {
            // A wrapped line of the list item above.
            item.push(Span {
                text: " ".into(),
                ..Span::default()
            });
            item.extend(spans(trimmed));
        } else {
            if !paragraph.is_empty() {
                paragraph.push(' ');
            }
            paragraph.push_str(trimmed);
        }
    }
    flush(&mut paragraph, &mut blocks);
    blocks
}

fn job(ui: &egui::Ui, spans: &[Span], size: f32, strong: bool) -> LayoutJob {
    let visuals = ui.visuals();
    let mut job = LayoutJob::default();
    for span in spans {
        let color: Color32 = if strong || span.bold {
            visuals.strong_text_color()
        } else {
            visuals.text_color()
        };
        let format = if span.code {
            TextFormat {
                font_id: FontId::monospace(size * 0.92),
                color,
                background: visuals.code_bg_color,
                ..Default::default()
            }
        } else {
            TextFormat {
                font_id: if strong || span.bold {
                    FontId::new(size, FontFamily::Name(crate::theme::SEMIBOLD.into()))
                } else {
                    FontId::proportional(size)
                },
                color,
                italics: span.italic,
                ..Default::default()
            }
        };
        job.append(&span.text, 0.0, format);
    }
    job
}

/// Draws the notes into `ui`.
pub(super) fn show(ui: &mut egui::Ui, markdown: &str) {
    let body = ui.text_style_height(&egui::TextStyle::Body) * 0.92;
    for block in blocks(markdown) {
        match block {
            Block::Heading(level, spans) => {
                ui.add_space(4.0);
                let size = body
                    * if level == 1 {
                        1.5
                    } else if level == 2 {
                        1.3
                    } else {
                        1.12
                    };
                ui.add(egui::Label::new(job(ui, &spans, size, true)).wrap());
            }
            Block::Paragraph(spans) => {
                ui.add(egui::Label::new(job(ui, &spans, body, false)).wrap());
            }
            Block::Item {
                depth,
                marker,
                spans,
            } => {
                ui.horizontal_top(|ui| {
                    ui.add_space(6.0 + depth as f32 * 16.0);
                    ui.label(RichText::new(marker));
                    ui.add(egui::Label::new(job(ui, &spans, body, false)).wrap());
                });
            }
        }
    }
}

#[cfg(test)]
mod tests;
