//! Minimal markdown → HTML for the local viewer.
//!
//! Uses pulldown-cmark (CommonMark + tables/strikethrough/tasklists). Raw HTML
//! in the source text is **neutralized** (rendered as literal visible text) so
//! model/copilot output can never inject markup into the viewer page.

use pulldown_cmark::{Event, Options, Parser, html};

pub fn to_html(md: &str) -> String {
    let opts = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let parser = Parser::new_ext(md, opts);
    // Html/InlineHtml events pass through `push_html` verbatim — convert them
    // to Text events, which get escaped.
    let events = parser.map(|e| match e {
        Event::Html(s) | Event::InlineHtml(s) => Event::Text(s),
        other => other,
    });
    let mut out = String::with_capacity(md.len() * 3 / 2);
    html::push_html(&mut out, events);
    // External links open in a new tab (viewer page must not navigate away).
    out.replace(
        "<a href=\"http",
        "<a target=\"_blank\" rel=\"noopener noreferrer\" href=\"http",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_html_is_neutralized() {
        let out = to_html("before <script>alert(1)</script> after");
        assert!(!out.contains("<script>"), "script tag leaked: {out}");
        assert!(out.contains("&lt;script&gt;"));
        assert!(out.contains("before"));
        assert!(out.contains("after"));
    }

    #[test]
    fn block_html_is_neutralized() {
        let out = to_html("<div onclick=\"x\">hi</div>");
        assert!(!out.contains("<div"), "div leaked: {out}");
        assert!(out.contains("&lt;div"));
    }

    #[test]
    fn tables_render() {
        let out = to_html("| a | b |\n|---|---|\n| 1 | 2 |\n");
        assert!(out.contains("<table>"), "{out}");
        assert!(out.contains("<th>a</th>"));
        assert!(out.contains("<td>1</td>"));
    }

    #[test]
    fn code_fences_and_inline() {
        let out = to_html("```rust\nfn x() {}\n```\n\nuse `vec![1]` here");
        assert!(out.contains("<pre><code class=\"language-rust\">"));
        assert!(out.contains("<code>vec![1]</code>"));
    }

    #[test]
    fn emphasis_and_links() {
        let out = to_html("**bold** and [x](https://example.com)");
        assert!(out.contains("<strong>bold</strong>"));
        assert!(out.contains("target=\"_blank\""));
    }
}
