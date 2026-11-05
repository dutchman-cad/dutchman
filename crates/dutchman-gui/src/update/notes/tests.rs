use super::*;

fn plain(spans: &[Span]) -> String {
    spans.iter().map(|s| s.text.as_str()).collect()
}

#[test]
fn html_becomes_markdown_with_only_the_supported_elements() {
    let html = "<h2>What&#39;s new</h2>\n<p>Intro with <strong>bold</strong>, <em>soft</em> and <code>dutchman report</code> and a <a href=\"https://x\">link</a>.<br>Next.</p>\
<ul><li><b>One.</b> First &amp; best</li><li>Two<ul><li>Inner</li></ul></li></ul><ol><li>a</li><li>b</li></ol>\
<p><img src=\"https://x/y.png\"></p><script>alert(1)</script><!-- hidden --><h1>Big</h1>";
    assert_eq!(
        html_to_markdown(html),
        "## What's new\n\nIntro with **bold**, *soft* and `dutchman report` and a link.\nNext.\n\n- **One.** First & best\n- Two\n  - Inner\n\n1. a\n2. b\n\n# Big"
    );
}

#[test]
fn literal_markdown_characters_in_html_text_stay_literal() {
    let md = html_to_markdown("<p>2 * 3 `x` [a](b)</p><p># not a heading</p><p>- not a list</p>");
    let found = blocks(&md);
    assert_eq!(found.len(), 3);
    assert_eq!(
        plain(match &found[0] {
            Block::Paragraph(s) => s,
            _ => panic!(),
        }),
        "2 * 3 `x` [a](b)"
    );
    assert_eq!(
        plain(match &found[1] {
            Block::Paragraph(s) => s,
            _ => panic!(),
        }),
        "# not a heading"
    );
    assert_eq!(
        plain(match &found[2] {
            Block::Paragraph(s) => s,
            _ => panic!(),
        }),
        "- not a list"
    );
}

#[test]
fn entities_are_decoded_once() {
    assert_eq!(
        unescape("a &amp;lt; &lt;b&gt; &#65;&#x42; &bogus; &"),
        "a &lt; <b> AB &bogus; &"
    );
}

#[test]
fn markdown_blocks_have_headings_lists_and_inline_styles() {
    let found = blocks("## What's new\n\n- **Bold.** with `code` and [a link](https://x)\n  wrapped\n- two\n\nText ![img](https://x/i.png)here\n\n<!-- dutchman-release {} -->");
    assert_eq!(found[0], Block::Heading(2, spans("What's new")));
    let Block::Item {
        marker,
        spans: item,
        depth,
    } = &found[1]
    else {
        panic!("{found:?}")
    };
    assert_eq!((marker.as_str(), *depth), ("\u{2022}", 0));
    assert_eq!(plain(item), "Bold. with code and a link wrapped");
    assert!(item[0].bold && item[0].text == "Bold.");
    assert!(item.iter().any(|s| s.code && s.text == "code"));
    assert!(matches!(&found[2], Block::Item { .. }));
    assert_eq!(found.len(), 4, "the comment is not shown");
    assert_eq!(
        plain(match &found[3] {
            Block::Paragraph(s) => s,
            _ => panic!(),
        }),
        "Text here"
    );
}

#[test]
fn the_real_0_2_7_notes_render_as_headings_bullets_bold_and_code() {
    let found = blocks(&html_to_markdown(&release_html()));
    let headings: Vec<String> = found
        .iter()
        .filter_map(|b| match b {
            Block::Heading(_, s) => Some(plain(s)),
            _ => None,
        })
        .collect();
    assert_eq!(
        headings,
        ["What's new", "Measured on SolidCraft 0.30", "Known issues"].map(String::from)
            [..headings.len()]
    );
    assert_eq!(headings[0], "What's new");
    let items: Vec<&Vec<Span>> = found
        .iter()
        .filter_map(|b| match b {
            Block::Item { spans, .. } => Some(spans),
            _ => None,
        })
        .collect();
    assert!(items.len() >= 7);
    assert!(items[0][0].bold && items[0][0].text == "Windows installer.");
    assert!(items
        .iter()
        .any(|s| s.iter().any(|p| p.code && p.text == "dutchman report")));
    for block in &found {
        let text = match block {
            Block::Heading(_, s) | Block::Paragraph(s) | Block::Item { spans: s, .. } => plain(s),
        };
        assert!(
            !text.contains('<') && !text.contains("**") && !text.contains("&#"),
            "{text}"
        );
    }
}

fn release_html() -> String {
    let md = include_str!("../../../../../release-notes/0.2.7.md");
    // The published notes are the same Markdown, rendered to HTML by GitHub.
    let mut html = String::new();
    for line in md.lines() {
        if let Some(h) = line.strip_prefix("## ") {
            html.push_str(&format!("<h2>{h}</h2>\n"));
        } else if let Some(item) = line.strip_prefix("- ") {
            let mut item = item.replace("`dutchman report`", "<code>dutchman report</code>");
            if let Some(rest) = item.strip_prefix("**") {
                if let Some((bold, tail)) = rest.split_once("**") {
                    item = format!("<strong>{bold}</strong>{tail}");
                }
            }
            html.push_str(&format!("<ul><li>{item}</li></ul>\n"));
        } else if !line.trim().is_empty() && !line.starts_with('|') && !line.starts_with('!') {
            html.push_str(&format!("<p>{line}</p>\n"));
        }
    }
    html
}

#[test]
fn notes_text_is_bounded() {
    assert!(
        html_to_markdown(&"<p>é</p>".repeat(MAX_HTML))
            .chars()
            .count()
            <= MAX_NOTES
    );
    assert!(html_to_markdown("<p>unterminated <b").len() < 40);
}
