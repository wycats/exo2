//! Shared frontend for Exo's CLI-like command text.
//!
//! This module owns syntax only: text tokenization, placeholder substitution,
//! global-format stripping, and explicit help-intent detection. Command
//! semantics still belong to `CommandSpec`/`ExoSpec`.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedCommandText {
    pub tokens: Vec<String>,
    pub intent: CommandTextIntent,
}

impl ParsedCommandText {
    pub fn help_target(&self) -> Option<&[String]> {
        match &self.intent {
            CommandTextIntent::Help { target } => Some(target),
            CommandTextIntent::Call => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandTextIntent {
    Call,
    Help { target: Vec<String> },
}

pub fn parse_command_text(command: &str, args: &[String]) -> Result<ParsedCommandText, String> {
    let tokens = tokenize_command(command, args)?;
    Ok(parse_argv_tokens(tokens))
}

#[must_use]
pub fn parse_argv(argv: &[String]) -> ParsedCommandText {
    parse_argv_tokens(argv.to_vec())
}

#[must_use]
pub fn tokens_request_json_output(tokens: &[String]) -> bool {
    let mut index = 0;
    while index < tokens.len() {
        let token = &tokens[index];
        if token == "--format"
            && tokens
                .get(index + 1)
                .is_some_and(|value| value.eq_ignore_ascii_case("json"))
        {
            return true;
        }
        if token
            .strip_prefix("--format=")
            .is_some_and(|value| value.eq_ignore_ascii_case("json"))
        {
            return true;
        }
        index += 1;
    }
    false
}

#[must_use]
pub fn tokens_without_global_format(tokens: &[String]) -> Vec<String> {
    let mut stripped = Vec::with_capacity(tokens.len());
    let mut index = 0;
    while index < tokens.len() {
        let token = &tokens[index];
        if token == "--format" && tokens.get(index + 1).is_some() {
            index += 2;
            continue;
        }
        if token.strip_prefix("--format=").is_some() {
            index += 1;
            continue;
        }
        stripped.push(token.clone());
        index += 1;
    }
    stripped
}

fn parse_argv_tokens(tokens: Vec<String>) -> ParsedCommandText {
    let intent = explicit_help_target(&tokens).map_or(CommandTextIntent::Call, |target| {
        CommandTextIntent::Help { target }
    });
    ParsedCommandText { tokens, intent }
}

fn explicit_help_target(tokens: &[String]) -> Option<Vec<String>> {
    let tokens = tokens_without_global_format(tokens);
    if tokens.is_empty() {
        return None;
    }

    if tokens[0] == "help" {
        return Some(route_target_tokens(&tokens[1..]));
    }

    if let Some(help_index) = tokens
        .iter()
        .position(|token| token == "--help" || token == "-h")
    {
        return Some(route_target_tokens(&tokens[..help_index]));
    }

    if tokens.last().is_some_and(|token| token == "help") {
        return Some(route_target_tokens(&tokens[..tokens.len() - 1]));
    }

    None
}

fn route_target_tokens(tokens: &[String]) -> Vec<String> {
    tokens
        .iter()
        .take_while(|token| !token.starts_with('-'))
        .cloned()
        .collect()
}

fn tokenize_command(command: &str, args: &[String]) -> Result<Vec<String>, String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut chars = command.chars().peekable();
    let mut quote = QuoteState::None;

    while let Some(ch) = chars.next() {
        match quote {
            QuoteState::None => match ch {
                '\'' => quote = QuoteState::Single,
                '"' => quote = QuoteState::Double,
                ' ' | '\t' => push_current_token(&mut tokens, &mut current),
                '|' | ';' | '<' | '>' => {
                    return Err(format!("Unsupported shell operator token '{ch}'"));
                }
                '&' if chars.peek() == Some(&'&') => {
                    return Err("Unsupported shell operator token '&&'".to_string());
                }
                '&' => return Err("Unsupported shell operator token '&'".to_string()),
                '$' if chars.peek() == Some(&'(') => {
                    return Err("Unsupported shell substitution '$('".to_string());
                }
                '`' => return Err("Unsupported shell substitution '`'".to_string()),
                '*' | '?' => {
                    return Err(format!("Unsupported shell glob token '{ch}'"));
                }
                _ => current.push(ch),
            },
            QuoteState::Single => {
                if ch == '\'' {
                    quote = QuoteState::None;
                } else {
                    current.push(ch);
                }
            }
            QuoteState::Double => match ch {
                '"' => quote = QuoteState::None,
                '\\' => {
                    let Some(next) = chars.next() else {
                        current.push('\\');
                        continue;
                    };
                    match next {
                        'n' => current.push('\n'),
                        't' => current.push('\t'),
                        '\\' => current.push('\\'),
                        '"' => current.push('"'),
                        _ => {
                            current.push('\\');
                            current.push(next);
                        }
                    }
                }
                '$' if chars.peek() == Some(&'(') => {
                    return Err("Unsupported shell substitution '$('".to_string());
                }
                '`' => return Err("Unsupported shell substitution '`'".to_string()),
                _ => current.push(ch),
            },
        }
    }

    match quote {
        QuoteState::None => {}
        QuoteState::Single | QuoteState::Double => {
            return Err("Unterminated quoted string".to_string());
        }
    }

    push_current_token(&mut tokens, &mut current);
    reject_env_assignment_prefix(&tokens)?;
    substitute_placeholders(tokens, args)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QuoteState {
    None,
    Single,
    Double,
}

fn push_current_token(tokens: &mut Vec<String>, current: &mut String) {
    if !current.is_empty() {
        tokens.push(std::mem::take(current));
    }
}

fn reject_env_assignment_prefix(tokens: &[String]) -> Result<(), String> {
    let Some(first) = tokens.first() else {
        return Ok(());
    };
    if looks_like_env_assignment(first) {
        return Err(format!(
            "Unsupported environment assignment prefix '{first}'"
        ));
    }
    Ok(())
}

fn looks_like_env_assignment(token: &str) -> bool {
    let Some((name, _value)) = token.split_once('=') else {
        return false;
    };
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn substitute_placeholders(tokens: Vec<String>, args: &[String]) -> Result<Vec<String>, String> {
    tokens
        .into_iter()
        .map(|token| {
            let Some(rest) = token.strip_prefix('$') else {
                return Ok(token);
            };
            if rest.is_empty() || !rest.chars().all(|ch| ch.is_ascii_digit()) {
                return Ok(token);
            }

            let index = rest
                .parse::<usize>()
                .map_err(|_| format!("Invalid placeholder '{token}'"))?;
            if index == 0 {
                return Err("Placeholder $0 is invalid (placeholders are 1-indexed)".to_string());
            }
            args.get(index - 1)
                .cloned()
                .ok_or_else(|| format!("Placeholder ${index} has no corresponding arg"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn detects_explicit_help_forms() {
        for (tokens, expected) in [
            (strings(&["task", "--help"]), strings(&["task"])),
            (strings(&["task", "help"]), strings(&["task"])),
            (strings(&["help", "task"]), strings(&["task"])),
            (
                strings(&["rfc", "promote", "--help", "--format", "json"]),
                strings(&["rfc", "promote"]),
            ),
            (
                strings(&["daemon", "ensure", "--workspace", "/tmp/project", "--help"]),
                strings(&["daemon", "ensure"]),
            ),
        ] {
            let parsed = parse_argv(&tokens);
            assert_eq!(parsed.help_target(), Some(expected.as_slice()));
        }
    }

    #[test]
    fn tokenizes_quotes_and_placeholders() {
        let parsed = parse_command_text(
            "task complete $1 --log \"line one\\nline two\"",
            &strings(&["goal::task"]),
        )
        .expect("parse command text");

        assert_eq!(
            parsed.tokens,
            strings(&[
                "task",
                "complete",
                "goal::task",
                "--log",
                "line one\nline two"
            ])
        );
        assert!(parsed.help_target().is_none());
    }

    #[test]
    fn rejects_shell_syntax() {
        let err = parse_command_text("status | cat", &[]).expect_err("shell syntax rejected");
        assert_eq!(err, "Unsupported shell operator token '|'");
    }

    #[test]
    fn detects_json_output_after_global_format_forms() {
        assert!(tokens_request_json_output(&strings(&[
            "rfc", "promote", "--help", "--format", "json",
        ])));
        assert!(tokens_request_json_output(&strings(&[
            "rfc",
            "promote",
            "--format=json",
        ])));
    }
}
