use std::borrow::Cow;

#[derive(Clone, Copy, Debug)]
pub struct CompactTheme;

impl cliclack::Theme for CompactTheme {
    fn format_log(&self, text: &str, symbol: &str) -> String {
        // Default cliclack log formatting intentionally adds extra spacing after
        // each log line. For Exosuit, we want denser output (especially in
        // nested/grouped steering).
        self.format_log_with_spacing(text, symbol, false)
    }
}

#[derive(Debug)]
pub struct ThemeGuard;

impl ThemeGuard {
    pub fn install_compact() -> Self {
        cliclack::set_theme(CompactTheme);
        Self
    }
}

impl Drop for ThemeGuard {
    fn drop(&mut self) {
        cliclack::reset_theme();
    }
}

pub fn tty_wrap_width() -> usize {
    // `cliclack` renders with its own prefix + guide-lines. Keep some headroom
    // so long details wrap before the terminal hard-wraps mid-word.
    let (_rows, cols) = console::Term::stdout().size();
    let cols = cols as usize;

    // Empirically, cliclack's tree prefixes consume more columns than expected,
    // and terminal hard-wrap mid-word looks terrible. Prefer being conservative.
    cols.saturating_sub(24).max(60)
}

pub fn wrap_for_tty(text: &str, width: usize) -> Vec<String> {
    let text = text.trim();
    if text.is_empty() {
        return vec![];
    }

    textwrap::wrap(text, textwrap::Options::new(width).break_words(true))
        .into_iter()
        .map(Cow::into_owned)
        .collect()
}

pub fn remark_wrapped(text: impl AsRef<str>) {
    let raw = text.as_ref();
    let width = tty_wrap_width();

    // Prefer splitting on clause boundaries first, then wrapping each clause.
    // This keeps important tokens intact and avoids terminal hard-wrap.
    let clauses: Vec<&str> = raw.split(';').collect();
    if clauses.len() <= 1 {
        for line in wrap_for_tty(raw, width) {
            let _ = cliclack::log::remark(line);
        }
        return;
    }

    for clause in clauses {
        let clause = clause.trim();
        if clause.is_empty() {
            continue;
        }
        for line in wrap_for_tty(clause, width) {
            let _ = cliclack::log::remark(line);
        }
    }
}

pub fn error_wrapped(text: impl AsRef<str>) {
    let raw = text.as_ref();
    let width = tty_wrap_width();

    for line in wrap_for_tty(raw, width) {
        let _ = cliclack::log::error(line);
    }
}

pub fn warning_wrapped(text: impl AsRef<str>) {
    let raw = text.as_ref();
    let width = tty_wrap_width();

    for line in wrap_for_tty(raw, width) {
        let _ = cliclack::log::warning(line);
    }
}

pub fn info_wrapped(text: impl AsRef<str>) {
    let raw = text.as_ref();
    let width = tty_wrap_width();

    for line in wrap_for_tty(raw, width) {
        let _ = cliclack::log::info(line);
    }
}
