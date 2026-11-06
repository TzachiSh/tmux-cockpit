//! #600, the #468 follow-ups: JSON-quoted keys and camelCase are secret names
//! too; `bearer` keeps its word boundary; the precision-for-recall tradeoff is
//! pinned; and the two gaps left open are named as such.
use cockpit_tui::panel::redact::redact;

#[test]
fn a_quoted_or_camelcase_secret_name_has_its_value_hidden() {
    let rows = [
        ("JSON", r#""password": "hunter2hunter2""#, r#""password": "[hidden]""#),
        ("JSON, joined", r#""db_password": "abcdefgh","#, r#""db_password": "[hidden]","#),
        ("single quotes", "'api_key': 'abcdefgh'", "'api_key': '[hidden]'"),
        ("camelCase", "myApiKey=abcdefgh", "myApiKey=[hidden]"),
        ("camelCase, colon", "dbPassword: hunter2x", "dbPassword: [hidden]"),
        (
            "a quoted value with spaces",
            "password: \"two words here\"",
            "password: \"[hidden]\"",
        ),
        // the tradeoff (#593): a typed field named for a secret loses its type
        ("a field's type", "user_password: String,", "user_password: [hidden]"),
    ];
    for (row, input, want) in rows {
        assert_eq!(redact(input), (want.to_string(), 1), "{row}: {input}");
    }
}

#[test]
fn a_name_that_only_starts_with_a_secret_word_stays_in_every_case() {
    let rows = [
        ("bearer joined to a prefix", "x_bearer abcdefgh"),
        ("camelCase hint", "passwordHint = \"Enter it\""),
        ("camelCase length", "myApiKeyLen = 123456"),
        ("a quoted hint", r#""password_hint": "Enter it""#),
    ];
    for (row, input) in rows {
        assert_eq!(redact(input), (input.to_string(), 0), "{row}");
    }
}

/// Decided not to fix (#600): `PASSWORD_DB=` reads like `password_hint =`, which
/// must stay, and `mypassword=` would need a key word to start mid-word, which
/// would hide `OLDPWD=`, a folder. These rows pin today's behaviour as a gap,
/// not a goal: a fix that hides them is welcome, and changes these rows.
#[test]
fn known_gaps_key_first_and_unseparated() {
    for input in ["PASSWORD_DB=abcdefgh", "mypassword=abcdefgh"] {
        assert_eq!(redact(input), (input.to_string(), 0), "{input}");
    }
}

/// review-534 on #606: an escaped quote does not end a quoted value, so a JSON
/// secret holding `\"` is hidden whole; `\\"` still ends it.
#[test]
fn an_escaped_quote_stays_inside_the_hidden_value() {
    let rows = [
        (
            "tail after \\\"",
            r#""password": "abcd\"secretsecret""#,
            r#""password": "[hidden]""#,
        ),
        ("short head", r#""password": "ab\"secretsecret""#, r#""password": "[hidden]""#),
        ("two escapes", r#"{"token": "x\"y\"zzzzzzzz"}"#, r#"{"token": "[hidden]"}"#),
        (
            "\\\\ ends it",
            r#""password": "abcdefgh\\", "user": "bob""#,
            r#""password": "[hidden]", "user": "bob""#,
        ),
    ];
    for (row, input, want) in rows {
        assert_eq!(redact(input), (want.to_string(), 1), "{row}: {input}");
    }
}

/// review-534 on #606: a quoted value never closed ends at the line, so the
/// lines after it stay on screen.
#[test]
fn an_unclosed_quoted_value_ends_at_the_line() {
    let input = "\"password\": \"abcdefgh\nnext line\nthird line";
    let want = "\"password\": \"[hidden]\nnext line\nthird line";
    assert_eq!(redact(input), (want.to_string(), 1));
}

/// review-534 on #606: a hump needs a lowercase letter or digit before it, so an
/// all-caps name holding a key word is no hump: `OLDPWD` is a folder.
#[test]
fn an_all_caps_name_is_not_a_camel_hump() {
    let input = "OLDPWD=/home/user";
    assert_eq!(redact(input), (input.to_string(), 0));
}
