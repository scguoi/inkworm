/// Generate a skeleton placeholder from an English reference string.
///
/// Rules:
///   [A-Za-z] → '_'
///   [0-9]    → '#'
///   space    → space
///   other    → itself
pub fn skeleton(english: &str) -> String {
    english
        .chars()
        .map(|c| {
            if c.is_ascii_alphabetic() {
                '_'
            } else if c.is_ascii_digit() {
                '#'
            } else {
                c
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const CASES: &[(&str, &str)] = &[
        ("hello", "_____"),
        ("AI", "__"),
        (
            "I've been working on it for 2 years.",
            "_'__ ____ _______ __ __ ___ # _____.",
        ),
        ("", ""),
        ("123", "###"),
        ("hello world", "_____ _____"),
        ("It's a \"test\"", "__'_ _ \"____\""),
        ("C-3PO", "_-#__"),
        ("  two  spaces  ", "  ___  ______  "),
        ("Hello, World!", "_____, _____!"),
    ];

    #[test]
    fn skeleton_table() {
        for &(input, expected) in CASES {
            assert_eq!(skeleton(input), expected, "input={input:?}");
        }
    }
}
