//! Lenient equality judgment for typing answers.
//!
//! Normalization:
//!   - trim
//!   - collapse consecutive whitespace to a single space
//!   - replace curly quotes with straight (\' \")
//!   - strip all trailing . ! ? (and any trailing spaces before/between them)
//!
//! Not normalized:
//!   - case
//!   - inner punctuation
//!   - contractions (I've ≠ I have)

pub fn normalize(s: &str) -> String {
    let replaced: String = s
        .chars()
        .map(|c| match c {
            '\u{2018}' | '\u{2019}' => '\'',
            '\u{201C}' | '\u{201D}' => '"',
            _ => c,
        })
        .collect();
    let mut collapsed = String::with_capacity(replaced.len());
    let mut last_was_space = true; // leading trim
    for c in replaced.chars() {
        if c.is_whitespace() {
            if !last_was_space {
                collapsed.push(' ');
                last_was_space = true;
            }
        } else {
            collapsed.push(c);
            last_was_space = false;
        }
    }
    // strip trailing spaces and sentence-enders until stable
    loop {
        let before = collapsed.len();
        while collapsed.ends_with(' ') {
            collapsed.pop();
        }
        if collapsed.ends_with('.') || collapsed.ends_with('!') || collapsed.ends_with('?') {
            collapsed.pop();
        }
        if collapsed.len() == before {
            break;
        }
    }
    collapsed
}

pub fn equals(input: &str, reference: &str) -> bool {
    normalize(input) == normalize(reference)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Table-driven equality tests. Each row: (input, reference, expected_equal)
    const CASES: &[(&str, &str, bool)] = &[
        // identity
        ("hello", "hello", true),
        // case matters
        ("Hello", "hello", false),
        ("AI", "ai", false),
        // trailing punctuation is stripped
        ("hello.", "hello", true),
        ("hello!", "hello", true),
        ("hello?", "hello", true),
        ("hello.", "hello!", true),
        // inner punctuation matters
        ("hello, world", "hello world", false),
        ("hello world", "hello, world", false),
        // whitespace: leading/trailing/collapsed
        ("  hello  ", "hello", true),
        ("hello  world", "hello world", true),
        ("hello\tworld", "hello world", true),
        // curly quotes are normalized
        ("I\u{2019}ve been here", "I've been here", true),
        ("\u{201C}AI\u{201D}", "\"AI\"", true),
        // contractions do NOT expand
        ("I've", "I have", false),
        // hyphen preserved
        ("state-of-the-art", "state of the art", false),
        // multiple trailing strippable chars — all are removed, so both sides
        // normalize to "hello" and are equal
        ("hello..", "hello.", true),
        ("hello..", "hello", true),
        // empty
        ("", "", true),
        ("", "hello", false),
        // answer wrong direction
        ("hello", "hello world", false),
        // numbers
        ("2 apples", "2 apples", true),
        ("two apples", "2 apples", false),
        // spaces around punctuation inside sentence
        ("hello , world", "hello, world", false),
        // single trailing space then punctuation
        ("hello .", "hello", true),
        // many cases...
        ("The quick fox", "The quick fox", true),
        ("The quick  fox ", "The quick fox", true),
        ("The quick fox.", "The quick fox", true),
        ("the quick fox", "The quick fox", false),
        ("\"quoted\"", "\u{201C}quoted\u{201D}", true),
        ("isn't", "isnt", false),
        ("can't", "cannot", false),
    ];

    #[test]
    fn equality_table() {
        for &(input, reference, expected) in CASES {
            assert_eq!(
                equals(input, reference),
                expected,
                "input={input:?} reference={reference:?}"
            );
        }
    }

    #[test]
    fn normalize_idempotent() {
        for &(input, _, _) in CASES {
            let once = normalize(input);
            let twice = normalize(&once);
            assert_eq!(once, twice, "not idempotent for {input:?}");
        }
    }

    #[test]
    fn normalize_collapses_multiple_newlines() {
        assert_eq!(normalize("a\n\n b"), "a b");
    }
}
