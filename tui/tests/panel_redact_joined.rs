//! #468: a secret name joined to a prefix by `_`, `-` or `.` is still a secret
//! name (DB_PASSWORD=, MY_API_KEY:), while an identifier that only starts with
//! one (password_hint, api_key_len) is not. Rows, one per case.
use cockpit_tui::panel::redact::redact;

#[test]
fn a_secret_name_after_a_prefix_has_its_value_hidden() {
    let rows = [
        ("_ prefix", "DB_PASSWORD=hunter2hunter2", "DB_PASSWORD=[hidden]"),
        ("_ prefix, colon", "MY_API_KEY: abcdefabcdef", "MY_API_KEY: [hidden]"),
        (
            "two words",
            "aws_secret_access_key = wJalrXUtnFEMIK7",
            "aws_secret_access_key = [hidden]",
        ),
        ("_ prefix, token", "GITHUB_TOKEN=abcd1234efgh", "GITHUB_TOKEN=[hidden]"),
        ("- prefix", "x-api-key: abcd1234efgh", "x-api-key: [hidden]"),
        (". prefix", "db.password=hunter2hunter2", "db.password=[hidden]"),
        ("lower case", "my_pwd = s3cr3tvalue", "my_pwd = [hidden]"),
    ];
    for (row, input, want) in rows {
        assert_eq!(redact(input), (want.to_string(), 1), "{row}: {input}");
    }
}

#[test]
fn an_identifier_that_only_starts_with_a_secret_word_stays() {
    let rows = [
        ("password_hint", "password_hint_text = \"Enter it here\""),
        ("api_key_len", "api_key_len = 123456"),
        ("token_count", "token_count = 123456"),
        ("a prefix and a suffix", "db_password_hint: something"),
        ("a word, not a name", "the password was long"),
        ("joined with no separator", "mypassword=abcdefgh"),
    ];
    for (row, input) in rows {
        assert_eq!(redact(input), (input.to_string(), 0), "{row}");
    }
}
