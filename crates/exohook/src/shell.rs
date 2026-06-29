pub(crate) fn shell_command_parts(command: String) -> Vec<String> {
    #[cfg(windows)]
    {
        vec![
            "cmd.exe".to_string(),
            "/C".to_string(),
            windows_command_with_env_assignments(&command),
        ]
    }

    #[cfg(not(windows))]
    {
        vec!["bash".to_string(), "-lc".to_string(), command]
    }
}

#[cfg(windows)]
fn windows_command_with_env_assignments(command: &str) -> String {
    let (assignments, rest) = parse_leading_env_assignments(command);
    if assignments.is_empty() || rest.trim().is_empty() {
        return command.to_string();
    }

    let mut parts = assignments
        .into_iter()
        .map(|(name, value)| format!("set \"{name}={}\"", escape_cmd_set_value(&value)))
        .collect::<Vec<_>>();
    parts.push(rest.trim_start().to_string());
    parts.join("&& ")
}

#[cfg(windows)]
fn parse_leading_env_assignments(command: &str) -> (Vec<(String, String)>, &str) {
    let mut assignments = Vec::new();
    let mut rest = command;

    loop {
        let trimmed = rest.trim_start();
        let Some((name, after_name)) = parse_env_name(trimmed) else {
            break;
        };
        let Some(after_equals) = after_name.strip_prefix('=') else {
            break;
        };
        let Some((value, consumed)) = parse_shell_word(after_equals) else {
            break;
        };

        rest = &trimmed[name.len() + 1 + consumed..];
        assignments.push((name, value));
    }

    if assignments.is_empty() {
        (assignments, command)
    } else {
        (assignments, rest)
    }
}

#[cfg(windows)]
fn parse_env_name(input: &str) -> Option<(String, &str)> {
    let mut chars = input.char_indices();
    let (_, first) = chars.next()?;
    if !(first == '_' || first.is_ascii_alphabetic()) {
        return None;
    }

    let mut end = first.len_utf8();
    for (idx, ch) in chars {
        if ch == '_' || ch.is_ascii_alphanumeric() {
            end = idx + ch.len_utf8();
        } else {
            break;
        }
    }

    Some((input[..end].to_string(), &input[end..]))
}

#[cfg(windows)]
fn parse_shell_word(input: &str) -> Option<(String, usize)> {
    let mut chars = input.char_indices();
    let (_, first) = chars.next()?;

    if first == '\'' || first == '"' {
        let quote = first;
        let value_start = first.len_utf8();
        for (idx, ch) in input[value_start..].char_indices() {
            if ch == quote {
                let end = value_start + idx;
                return Some((input[value_start..end].to_string(), end + quote.len_utf8()));
            }
        }
        return None;
    }

    let mut end = input.len();
    for (idx, ch) in input.char_indices() {
        if ch.is_whitespace() {
            end = idx;
            break;
        }
    }

    Some((input[..end].to_string(), end))
}

#[cfg(windows)]
fn escape_cmd_set_value(value: &str) -> String {
    let mut escaped = String::new();
    for ch in value.chars() {
        match ch {
            '^' | '&' | '|' | '<' | '>' | '%' | '!' | '(' | ')' => {
                escaped.push('^');
                escaped.push(ch);
            }
            '"' => escaped.push_str("\\\""),
            _ => escaped.push(ch),
        }
    }
    escaped
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn translates_leading_posix_env_assignments_for_cmd() {
        let parts = shell_command_parts(
            "CARGO_INCREMENTAL=0 RUSTFLAGS='-C debuginfo=0' cargo test --workspace".to_string(),
        );

        assert_eq!(
            parts,
            vec![
                "cmd.exe".to_string(),
                "/C".to_string(),
                "set \"CARGO_INCREMENTAL=0\"&& set \"RUSTFLAGS=-C debuginfo=0\"&& cargo test --workspace".to_string(),
            ]
        );
    }

    #[test]
    fn leaves_regular_cmd_commands_unchanged() {
        let parts = shell_command_parts("cargo check".to_string());

        assert_eq!(
            parts,
            vec![
                "cmd.exe".to_string(),
                "/C".to_string(),
                "cargo check".to_string(),
            ]
        );
    }
}
