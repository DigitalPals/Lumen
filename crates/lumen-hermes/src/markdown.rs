/// Escapes text for safe insertion into Pango markup.
pub fn escape_pango_text(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// A conservative Markdown-ish renderer for GTK labels.
///
/// It deliberately does not pass through HTML. Today it preserves text, escapes
/// all markup-sensitive characters, and gives fenced code blocks a monospace
/// span. This is safe for untrusted assistant output and can be swapped for a
/// widget-based Markdown renderer later.
pub fn markdownish_to_pango(input: &str) -> String {
    let mut out = String::new();
    let mut in_code = false;
    let mut code_buf = String::new();

    for line in input.lines() {
        if line.trim_start().starts_with("```") {
            if in_code {
                out.push_str("<span font_family=\"monospace\">");
                out.push_str(&escape_pango_text(code_buf.trim_end()));
                out.push_str("</span>");
                out.push('\n');
                code_buf.clear();
                in_code = false;
            } else {
                in_code = true;
            }
            continue;
        }

        if in_code {
            code_buf.push_str(line);
            code_buf.push('\n');
        } else {
            out.push_str(&escape_pango_text(line));
            out.push('\n');
        }
    }

    if in_code {
        out.push_str("<span font_family=\"monospace\">");
        out.push_str(&escape_pango_text(code_buf.trim_end()));
        out.push_str("</span>");
        out.push('\n');
    }

    out.trim_end().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_pango_markup() {
        assert_eq!(escape_pango_text("<b>&</b>"), "&lt;b&gt;&amp;&lt;/b&gt;");
    }

    #[test]
    fn renders_code_blocks_as_escaped_monospace() {
        let rendered = markdownish_to_pango("```rust\nlet x = 1 < 2;\n```");
        assert!(rendered.contains("monospace"));
        assert!(rendered.contains("1 &lt; 2"));
    }
}
