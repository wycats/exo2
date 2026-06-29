use regex::Regex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HoleKind {
    #[allow(dead_code)]
    Any,
    Rfc3339,
    Uuid,
    Regex(&'static str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fragment {
    Lit(&'static str),
    Hole(HoleKind),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedHole {
    pub kind: HoleKind,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateMismatch {
    pub message: String,
}

impl TemplateMismatch {
    #[must_use]
    pub fn with_context(mut self, template: &[Fragment], input: &str) -> Self {
        // Keep this intentionally compact: when these fail, we want to see the exact
        // input line and the fragments we were trying to match.
        self.message = format!("{}\ninput: {input:?}\ntemplate: {template:?}", self.message);
        self
    }
}

pub fn match_template(
    template: &[Fragment],
    mut input: &str,
) -> Result<Vec<CapturedHole>, TemplateMismatch> {
    let original = input;
    let mut captured = Vec::new();

    for (i, frag) in template.iter().enumerate() {
        match frag {
            Fragment::Lit(lit) => {
                if let Some(rest) = input.strip_prefix(lit) {
                    input = rest;
                } else {
                    return Err(TemplateMismatch {
                        message: format!("expected literal {lit:?} at fragment {i}"),
                    }
                    .with_context(template, original));
                }
            }
            Fragment::Hole(kind) => {
                let next_lit = template.iter().skip(i + 1).find_map(|f| match f {
                    Fragment::Lit(s) if !s.is_empty() => Some(*s),
                    _ => None,
                });

                let (matched, rest) = if let Some(delim) = next_lit {
                    if delim.is_empty() {
                        (input, "")
                    } else if let Some(idx) = input.find(delim) {
                        (&input[..idx], &input[idx..])
                    } else {
                        return Err(TemplateMismatch {
                            message: format!(
                                "expected delimiter {delim:?} after hole at fragment {i}"
                            ),
                        }
                        .with_context(template, original));
                    }
                } else {
                    (input, "")
                };

                validate_hole(kind, matched).map_err(|msg| {
                    TemplateMismatch {
                        message: format!("hole at fragment {i} did not validate: {msg}"),
                    }
                    .with_context(template, original)
                })?;

                captured.push(CapturedHole {
                    kind: kind.clone(),
                    text: matched.to_string(),
                });

                input = rest;
            }
        }
    }

    if !input.is_empty() {
        return Err(TemplateMismatch {
            message: format!("unconsumed input remaining: {input:?}"),
        }
        .with_context(template, original));
    }

    Ok(captured)
}

fn validate_hole(kind: &HoleKind, text: &str) -> Result<(), String> {
    match kind {
        HoleKind::Any => Ok(()),
        HoleKind::Rfc3339 => chrono::DateTime::parse_from_rfc3339(text)
            .map(|_| ())
            .map_err(|e| format!("invalid RFC3339 timestamp: {e}")),
        HoleKind::Uuid => uuid::Uuid::parse_str(text)
            .map(|_| ())
            .map_err(|e| format!("invalid uuid: {e}")),
        HoleKind::Regex(pattern) => {
            let re = Regex::new(pattern).map_err(|e| format!("invalid regex {pattern:?}: {e}"))?;
            if re.is_match(text) {
                Ok(())
            } else {
                Err(format!("value {text:?} did not match {pattern:?}"))
            }
        }
    }
}

#[macro_export]
macro_rules! tmpl {
    ($($frag:expr),* $(,)?) => {
        &[
            $( $frag ),*
        ]
    };
}

#[macro_export]
macro_rules! lit {
    ($s:literal) => {
        $crate::support::template::Fragment::Lit($s)
    };
}

#[macro_export]
macro_rules! hole {
    (any) => {
        $crate::support::template::Fragment::Hole($crate::support::template::HoleKind::Any)
    };
    (rfc3339) => {
        $crate::support::template::Fragment::Hole($crate::support::template::HoleKind::Rfc3339)
    };
    (uuid) => {
        $crate::support::template::Fragment::Hole($crate::support::template::HoleKind::Uuid)
    };
    (re($re:literal)) => {
        $crate::support::template::Fragment::Hole($crate::support::template::HoleKind::Regex($re))
    };
}
