//! Small scanners for the few pieces of keymap text that kbvm does not expose
//! through its public API.
//!
//! `kbvm::xkb::Keymap` publicly exposes the keys, groups, levels, and virtual
//! modifier names/masks. It does not expose the keycode range or the order of
//! the modifier array. Both are recovered from the keymap text, which for
//! Wayland clients is always a fully resolved `xkb_keymap_get_as_string`
//! dump.

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

fn is_ident_continue(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn read_ident(bytes: &[u8], mut i: usize) -> Option<(String, usize)> {
    if i >= bytes.len() || !is_ident_start(bytes[i]) {
        return None;
    }
    let start = i;
    i += 1;
    while i < bytes.len() && is_ident_continue(bytes[i]) {
        i += 1;
    }
    let ident = std::str::from_utf8(&bytes[start..i]).ok()?.to_string();
    Some((ident, i))
}

fn skip_line_comment(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() && bytes[i] != b'\n' {
        i += 1;
    }
    i
}

fn skip_block_comment(bytes: &[u8], mut i: usize) -> usize {
    i += 2;
    while i + 1 < bytes.len() {
        if bytes[i] == b'*' && bytes[i + 1] == b'/' {
            return i + 2;
        }
        i += 1;
    }
    bytes.len()
}

fn skip_string(bytes: &[u8], mut i: usize) -> usize {
    i += 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'"' => return i + 1,
            _ => i += 1,
        }
    }
    bytes.len()
}

fn skip_trivia(bytes: &[u8], mut i: usize) -> usize {
    loop {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'/' {
            i = skip_line_comment(bytes, i + 2);
            continue;
        }
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            i = skip_block_comment(bytes, i);
            continue;
        }
        if i < bytes.len() && bytes[i] == b'#' {
            i = skip_line_comment(bytes, i + 1);
            continue;
        }
        break;
    }
    i
}

fn find_keyword(bytes: &[u8], keyword: &str, mut i: usize) -> Option<usize> {
    while i < bytes.len() {
        i = skip_trivia(bytes, i);
        if i >= bytes.len() {
            return None;
        }
        match bytes[i] {
            b'"' => i = skip_string(bytes, i),
            b if is_ident_start(b) => {
                let (ident, end) = read_ident(bytes, i)?;
                if ident == keyword {
                    return Some(end);
                }
                i = end;
            }
            _ => i += 1,
        }
    }
    None
}

/// Returns the minimum and maximum keycode named in the `xkb_keycodes`
/// sections of `text`.
///
/// libxkbcommon derives these from the assigned key names, ignoring the
/// `minimum` and `maximum` statements, and defaults to 8 and 255 when no
/// keycode is named.
pub(crate) fn keycode_range(text: &[u8]) -> (u32, u32) {
    let mut min: Option<u32> = None;
    let mut max: Option<u32> = None;
    let mut search_from = 0;
    while let Some(after_keyword) = find_keyword(text, "xkb_keycodes", search_from) {
        let mut i = skip_trivia(text, after_keyword);
        if i < text.len() && text[i] == b'"' {
            i = skip_trivia(text, skip_string(text, i));
        }
        if i >= text.len() || text[i] != b'{' {
            search_from = search_from.max(after_keyword);
            continue;
        }
        i += 1;
        let mut depth = 1usize;
        while i < text.len() && depth > 0 {
            i = skip_trivia(text, i);
            if i >= text.len() {
                break;
            }
            match text[i] {
                b'"' => i = skip_string(text, i),
                b'{' => {
                    depth += 1;
                    i += 1;
                }
                b'}' => {
                    depth -= 1;
                    i += 1;
                }
                b'<' => {
                    let Some(offset) = text[i + 1..].iter().position(|&b| b == b'>') else {
                        i += 1;
                        continue;
                    };
                    let name_end = i + 1 + offset;
                    let mut j = skip_trivia(text, name_end + 1);
                    if j < text.len() && text[j] == b'=' {
                        j = skip_trivia(text, j + 1);
                        let digits_start = j;
                        while j < text.len() && text[j].is_ascii_digit() {
                            j += 1;
                        }
                        if j > digits_start {
                            let after = skip_trivia(text, j);
                            if after < text.len() && text[after] == b';' {
                                let value = std::str::from_utf8(&text[digits_start..j])
                                    .ok()
                                    .and_then(|v| v.parse::<u32>().ok());
                                if let Some(value) = value {
                                    min = Some(min.map_or(value, |m| m.min(value)));
                                    max = Some(max.map_or(value, |m| m.max(value)));
                                    i = after + 1;
                                    continue;
                                }
                            }
                        }
                    }
                    i = name_end + 1;
                }
                _ => i += 1,
            }
        }
        search_from = i;
    }
    match (min, max) {
        (Some(min), Some(max)) => (min, max),
        _ => (8, 255),
    }
}

/// Returns the order of the virtual modifier names as they appear in
/// `virtual_modifiers` statements.
pub(crate) fn virtual_modifier_order(text: &[u8]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let mut search_from = 0;
    while let Some(after_keyword) = find_keyword(text, "virtual_modifiers", search_from) {
        let mut i = after_keyword;
        loop {
            i = skip_trivia(text, i);
            if i >= text.len() {
                break;
            }
            match text[i] {
                b';' => {
                    i += 1;
                    break;
                }
                b',' => i += 1,
                b if is_ident_start(b) => {
                    let Some((name, mut end)) = read_ident(text, i) else {
                        i += 1;
                        continue;
                    };
                    let after = skip_trivia(text, end);
                    if after < text.len() && text[after] == b'=' {
                        end = skip_trivia(text, after + 1);
                        if let Some((_, value_end)) = read_ident(text, end) {
                            end = value_end;
                        } else if end < text.len() && text[end].is_ascii_digit() {
                            while end < text.len() && text[end].is_ascii_digit() {
                                end += 1;
                            }
                        }
                    }
                    if !names.contains(&name) {
                        names.push(name);
                    }
                    i = end;
                }
                _ => i += 1,
            }
        }
        search_from = search_from.max(i);
    }
    names
}

#[cfg(test)]
mod tests {
    use super::keycode_range;
    use super::virtual_modifier_order;

    #[test]
    fn keycode_range_defaults() {
        assert_eq!(keycode_range(b"xkb_keymap { };"), (8, 255));
        assert_eq!(keycode_range(b""), (8, 255));
    }

    #[test]
    fn keycode_range_ignores_bounds_and_aliases() {
        let text = br#"
            xkb_keymap {
                xkb_keycodes "x" {
                    minimum = 1;
                    maximum = 999;
                    <A> = 9;
                    alias <B> = <A>;
                    <C> = 200;
                };
            };
        "#;
        assert_eq!(keycode_range(text), (9, 200));
    }

    #[test]
    fn keycode_range_skips_comments_and_strings() {
        let text = br#"
            xkb_keycodes "a { <fake> = 1; }" {
                // <fake> = 2;
                /* <fake> = 3; */
                <real> = 42;
            };
        "#;
        assert_eq!(keycode_range(text), (42, 42));
    }

    #[test]
    fn virtual_modifier_order_parses_list() {
        let text = b"xkb_types { virtual_modifiers A, B=Mod3, C; };";
        assert_eq!(virtual_modifier_order(text), ["A", "B", "C"]);
        assert_eq!(
            virtual_modifier_order(b"virtual_modifiers X; virtual_modifiers Y, X;"),
            ["X", "Y"]
        );
    }

    #[test]
    fn virtual_modifier_order_ignores_comments() {
        let text = b"// virtual_modifiers Fake;\n/* virtual_modifiers AlsoFake; */";
        assert!(virtual_modifier_order(text).is_empty());
    }
}
