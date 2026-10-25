//! Minimal ANSI styling, on only for terminals and unless `--no-color`.

use std::io::IsTerminal;

/// Colour switch decided once from the flags and the terminal.
#[derive(Clone, Copy, Debug)]
pub struct Style {
    enabled: bool,
}

impl Style {
    /// Colour on when stdout is a terminal and `--no-color` was not given.
    pub fn detect(no_color: bool) -> Self {
        let enabled =
            !no_color && std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none();
        Self { enabled }
    }

    fn wrap(&self, code: &str, text: &str) -> String {
        if self.enabled {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_owned()
        }
    }

    /// Bold.
    pub fn bold(&self, t: &str) -> String {
        self.wrap("1", t)
    }
    /// Dimmed.
    pub fn dim(&self, t: &str) -> String {
        self.wrap("2", t)
    }
    /// Green.
    pub fn green(&self, t: &str) -> String {
        self.wrap("32", t)
    }
    /// Red.
    pub fn red(&self, t: &str) -> String {
        self.wrap("31", t)
    }
    /// Yellow.
    pub fn yellow(&self, t: &str) -> String {
        self.wrap("33", t)
    }
    /// Cyan.
    pub fn cyan(&self, t: &str) -> String {
        self.wrap("36", t)
    }
}
