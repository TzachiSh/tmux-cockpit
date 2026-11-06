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
