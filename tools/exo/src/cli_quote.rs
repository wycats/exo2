pub fn shell_quote_arg(raw: &str) -> String {
    if raw
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '_' | '-'))
    {
        return raw.to_string();
    }

    format!("'{}'", raw.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaves_simple_cli_args_unquoted() {
        assert_eq!(
            shell_quote_arg("/tmp/exo-workspace_1.2"),
            "/tmp/exo-workspace_1.2"
        );
    }

    #[test]
    fn single_quotes_args_with_spaces() {
        assert_eq!(
            shell_quote_arg("/tmp/exo workspace"),
            "'/tmp/exo workspace'"
        );
    }

    #[test]
    fn escapes_single_quotes_inside_arg() {
        assert_eq!(shell_quote_arg("/tmp/it's-here"), "'/tmp/it'\\''s-here'");
    }
}
