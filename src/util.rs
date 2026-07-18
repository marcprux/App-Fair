//! Small cross-platform helpers.

/// Drop the calling thread to background priority, so heavy work (the first catalog sync, image
/// decoding) yields CPU to the UI thread. No-op off Android.
#[cfg(target_os = "android")]
pub fn lower_priority() {
    // Linux nice values are per-thread; `who = 0` targets the calling thread. 10 ≈ Android's
    // THREAD_PRIORITY_BACKGROUND.
    unsafe {
        libc::setpriority(libc::PRIO_PROCESS, 0, 10);
    }
}

#[cfg(not(target_os = "android"))]
pub fn lower_priority() {}

/// Flatten the light HTML some F-Droid catalogs put in app descriptions (`<b>`, `<i>`, `<a>`,
/// lists, paragraphs) down to plain text. Inline tags are dropped; block-level tags (`<p>`,
/// `<br>`, `<li>`, headings, `<div>`) become line breaks so paragraphs and list items stay
/// separated. A few common HTML entities are decoded.
///
/// TODO(styled-text): this is a lossy fallback. Once Day labels support attributed/styled runs
/// (bold, italic, tappable links), render the description as styled text and keep the emphasis and
/// links instead of discarding them — see `crate::detail::description`.
pub fn strip_html(input: &str) -> String {
    // Tags that should read as a line break once removed.
    fn is_block(tag: &str) -> bool {
        let t = tag.trim_start_matches('/');
        matches!(
            t,
            "br" | "br/"
                | "p"
                | "div"
                | "li"
                | "ul"
                | "ol"
                | "tr"
                | "h1"
                | "h2"
                | "h3"
                | "h4"
                | "h5"
                | "h6"
                | "blockquote"
        )
    }

    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '<' => {
                // Consume up to the matching '>', collecting the tag name for block detection.
                let mut tag = String::new();
                for tc in chars.by_ref() {
                    if tc == '>' {
                        break;
                    }
                    tag.push(tc);
                }
                let name: String = tag
                    .trim()
                    .split(|ch: char| ch.is_whitespace())
                    .next()
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if is_block(&name) {
                    out.push('\n');
                }
            }
            '&' => {
                // Decode a single entity, or emit the '&' verbatim if it isn't one.
                let mut ent = String::new();
                while let Some(&ec) = chars.peek() {
                    if ec == ';' {
                        chars.next();
                        break;
                    }
                    if ent.len() > 8 || ec == '<' || ec == '&' {
                        break;
                    }
                    ent.push(ec);
                    chars.next();
                }
                out.push_str(&decode_entity(&ent));
            }
            _ => out.push(c),
        }
    }

    // Collapse runs of spaces/tabs, and cap blank lines to a single separator.
    let mut result = String::with_capacity(out.len());
    let mut blank_lines = 0usize;
    for line in out.lines() {
        let trimmed = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if trimmed.is_empty() {
            blank_lines += 1;
            if blank_lines <= 1 && !result.is_empty() {
                result.push('\n');
            }
        } else {
            blank_lines = 0;
            result.push_str(&trimmed);
            result.push('\n');
        }
    }
    result.trim().to_string()
}

/// Decode the handful of HTML entities that show up in catalog descriptions.
fn decode_entity(ent: &str) -> String {
    match ent {
        "amp" => "&".to_string(),
        "lt" => "<".to_string(),
        "gt" => ">".to_string(),
        "quot" => "\"".to_string(),
        "apos" | "#39" => "'".to_string(),
        "nbsp" => " ".to_string(),
        _ => {
            // Numeric character reference (&#NN; or &#xHH;).
            if let Some(hex) = ent.strip_prefix("#x").or_else(|| ent.strip_prefix("#X")) {
                u32::from_str_radix(hex, 16)
                    .ok()
                    .and_then(char::from_u32)
                    .map(String::from)
                    .unwrap_or_default()
            } else if let Some(dec) = ent.strip_prefix('#') {
                dec.parse::<u32>()
                    .ok()
                    .and_then(char::from_u32)
                    .map(String::from)
                    .unwrap_or_default()
            } else {
                // Not a recognized entity: preserve it so text isn't silently dropped.
                format!("&{ent}")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::strip_html;

    #[test]
    fn strips_inline_tags() {
        assert_eq!(
            strip_html("A <b>bold</b> and <i>italic</i> word."),
            "A bold and italic word."
        );
    }

    #[test]
    fn block_tags_become_newlines() {
        // <br> is a single line break; a paragraph boundary reads as a blank line between them.
        assert_eq!(strip_html("One<br>Two"), "One\nTwo");
        assert_eq!(
            strip_html("<p>Para one</p><p>Para two</p>"),
            "Para one\n\nPara two"
        );
    }

    #[test]
    fn decodes_entities() {
        assert_eq!(
            strip_html("Tom &amp; Jerry &lt;3 &#39;quotes&#39;"),
            "Tom & Jerry <3 'quotes'"
        );
    }

    #[test]
    fn keeps_bare_ampersand() {
        assert_eq!(strip_html("R&D not an entity"), "R&D not an entity");
    }
}
