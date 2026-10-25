//! Rich text (`*.RTF` in `DATA`: the credits and battle histories), read
//! as plain paragraphs: groups of font tables, colour tables, style sheets,
//! document information and `\*` destinations are skipped; `\par` ends a
//! paragraph, `\line` breaks a line, `\tab` is a tab, `\'hh` a Windows-1252
//! character and `\rquote`, `\lquote`, `\ldblquote`, `\rdblquote`, `\endash`,
//! `\emdash` their punctuation.

/// The paragraphs of rich text `data`.
pub fn paragraphs(data: &[u8]) -> Vec<String> {
    let s: Vec<char> = data.iter().map(|&b| b as char).collect();
    let mut out = String::new();
    // per group: whether its text is skipped
    let mut skip = vec![false];
    let mut i = 0;
    while i < s.len() {
        match s[i] {
            '{' => {
                skip.push(*skip.last().unwrap_or(&false));
                i += 1;
            }
            '}' => {
                skip.pop();
                if skip.is_empty() {
                    skip.push(false);
                }
                i += 1;
            }
            '\\' => {
                i += 1;
                let Some(&c) = s.get(i) else { break };
                if c.is_ascii_alphabetic() {
                    let start = i;
                    while i < s.len() && s[i].is_ascii_alphabetic() {
                        i += 1;
                    }
                    let word: String = s[start..i].iter().collect();
                    // a numeric parameter, then one space that belongs to it
                    while i < s.len() && (s[i] == '-' || s[i].is_ascii_digit()) {
                        i += 1;
                    }
                    if s.get(i) == Some(&' ') {
                        i += 1;
                    }
                    let skipping = *skip.last().unwrap_or(&false);
                    match word.as_str() {
                        "fonttbl" | "colortbl" | "stylesheet" | "info" | "pict" | "header" | "footer" => {
                            if let Some(t) = skip.last_mut() {
                                *t = true;
                            }
                        }
                        _ if skipping => {}
                        "par" => out.push('\n'),
                        "line" => out.push('\u{2028}'),
                        "tab" => out.push('\t'),
                        "rquote" | "lquote" => out.push('\''),
                        "ldblquote" | "rdblquote" => out.push('"'),
                        "endash" | "emdash" => out.push('-'),
                        _ => {}
                    }
                } else if c == '\'' {
                    let hex: String = s.get(i + 1..i + 3).map(|h| h.iter().collect()).unwrap_or_default();
                    i += 3;
                    if !*skip.last().unwrap_or(&false)
                        && let Ok(b) = u8::from_str_radix(&hex, 16)
                    {
                        out.push(match b {
                            0x91 | 0x92 => '\'',
                            0x93 | 0x94 => '"',
                            0x96 | 0x97 => '-',
                            0x85 => '…',
                            b => b as char,
                        });
                    }
                } else {
                    if c == '*'
                        && let Some(t) = skip.last_mut()
                    {
                        *t = true;
                    } else if "\\{}".contains(c) && !*skip.last().unwrap_or(&false) {
                        out.push(c);
                    }
                    i += 1;
                }
            }
            '\r' | '\n' => i += 1,
            c => {
                if !*skip.last().unwrap_or(&false) {
                    out.push(c);
                }
                i += 1;
            }
        }
    }
    out.split('\n').map(|p| p.trim().replace('\u{2028}', "\n")).filter(|p| !p.is_empty()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_rich_text() {
        let rtf = br"{\rtf1\ansi{\fonttbl{\f0 Times;}}{\info{\title X}}\pard\plain \f0\fs20 Alexander\rquote s army\par {\b Porus}\tab won\'85\par}";
        assert_eq!(paragraphs(rtf), vec!["Alexander's army".to_string(), "Porus\twon…".to_string()]);
    }
}
