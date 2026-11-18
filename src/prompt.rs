//! Rodtty-generated shell prompts.
//!
//! The owner's **prompt style** and **prompt symbol** are rendered from one
//! segment description into both Brush `PS1` text and a settings preview, so
//! the two cannot drift apart. Running sessions learn about changes through a
//! small data file in their private profile directory. The shell hook reads
//! that file with the `read` builtin and matches it against a fixed `case`
//! whitelist generated here; the file is never sourced or evaluated.
//!
//! Colors are the sixteen indexed ANSI slots only, so the active theme
//! palette recolors Rodtty prompts like any other conventional output.
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// File name of the prompt data file inside a session's profile directory.
pub const DATA_FILE: &str = "prompt";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptStyle {
    #[default]
    Classic,
    Directory,
    Rodtty,
    Powerline,
    TwoLine,
}
impl PromptStyle {
    pub const ALL: &[Self] = &[
        Self::Classic,
        Self::Directory,
        Self::Rodtty,
        Self::Powerline,
        Self::TwoLine,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Classic => "Classic",
            Self::Directory => "Directory",
            Self::Rodtty => "Rodtty",
            Self::Powerline => "Powerline",
            Self::TwoLine => "Two-line",
        }
    }
    /// Stable data-file and settings identifier.
    pub fn key(self) -> &'static str {
        match self {
            Self::Classic => "classic",
            Self::Directory => "directory",
            Self::Rodtty => "rodtty",
            Self::Powerline => "powerline",
            Self::TwoLine => "two_line",
        }
    }
    /// Whether the prompt shows the last command's exit status.
    pub fn reports_status(self) -> bool {
        self == Self::TwoLine
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptSymbol {
    #[default]
    Dollar,
    Chevron,
    Triangle,
    DoubleChevron,
}
impl PromptSymbol {
    pub const ALL: &[Self] = &[
        Self::Dollar,
        Self::Chevron,
        Self::Triangle,
        Self::DoubleChevron,
    ];
    pub fn glyph(self) -> &'static str {
        match self {
            Self::Dollar => "$",
            Self::Chevron => "›",
            Self::Triangle => "▶",
            Self::DoubleChevron => "⟫",
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            Self::Dollar => "dollar",
            Self::Chevron => "chevron",
            Self::Triangle => "triangle",
            Self::DoubleChevron => "double_chevron",
        }
    }
    /// `\$` keeps Bash's `#` for a privileged shell, as the original prompt did.
    fn ps1(self) -> &'static str {
        match self {
            Self::Dollar => "\\$",
            other => other.glyph(),
        }
    }
}

/// Prompt style and symbol are independent owner preferences.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct PromptPreferences {
    pub style: PromptStyle,
    pub symbol: PromptSymbol,
}

/// ANSI palette slots 0–15.
const BLACK: u8 = 0;
const RED: u8 = 1;
const GREEN: u8 = 2;
const BLUE: u8 = 4;
const CYAN: u8 = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Text(&'static str),
    /// The working directory, abbreviated with `~` under the home directory.
    Directory,
    Symbol,
    LineBreak,
}

/// One run of prompt text with optional ANSI palette slots.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Segment {
    pub part: Part,
    pub fg: Option<u8>,
    pub bg: Option<u8>,
    pub bold: bool,
}
const fn plain(part: Part) -> Segment {
    Segment {
        part,
        fg: None,
        bg: None,
        bold: false,
    }
}
const fn colored(part: Part, fg: u8, bg: Option<u8>, bold: bool) -> Segment {
    Segment {
        part,
        fg: Some(fg),
        bg,
        bold,
    }
}

impl PromptPreferences {
    /// The prompt's segments after a command with the given outcome.
    pub fn segments(self, failed: bool) -> Vec<Segment> {
        use Part::*;
        match self.style {
            PromptStyle::Classic => vec![
                plain(Text("rodtty:")),
                plain(Directory),
                plain(Symbol),
                plain(Text(" ")),
            ],
            PromptStyle::Directory => vec![
                plain(Directory),
                plain(Text(" ")),
                plain(Symbol),
                plain(Text(" ")),
            ],
            PromptStyle::Rodtty => vec![plain(Text("Rodtty ")), plain(Symbol), plain(Text(" "))],
            PromptStyle::Powerline => vec![
                colored(Text(" Rodtty "), BLACK, Some(CYAN), true),
                colored(Text(" "), BLACK, Some(BLUE), false),
                colored(Directory, BLACK, Some(BLUE), false),
                colored(Text(" "), BLACK, Some(BLUE), false),
                plain(Text(" ")),
                plain(Symbol),
                plain(Text(" ")),
            ],
            PromptStyle::TwoLine => vec![
                colored(Directory, BLUE, None, true),
                plain(LineBreak),
                colored(Symbol, if failed { RED } else { GREEN }, None, true),
                plain(Text(" ")),
            ],
        }
    }

    /// Brush `PS1` text for the prompt after a command with the given outcome.
    pub fn ps1(self, failed: bool) -> String {
        let mut out = String::new();
        // The active SGR run; adjacent segments with equal styling share it.
        let mut current: Option<String> = None;
        for segment in self.segments(failed) {
            let mut sgr = Vec::new();
            if segment.bold {
                sgr.push("1".to_owned());
            }
            if let Some(fg) = segment.fg {
                sgr.push(ansi(fg, 30, 90));
            }
            if let Some(bg) = segment.bg {
                sgr.push(ansi(bg, 40, 100));
            }
            let wanted = (!sgr.is_empty() && segment.part != Part::LineBreak)
                .then(|| format!("\\[\\e[0;{}m\\]", sgr.join(";")));
            if wanted != current {
                match &wanted {
                    Some(start) => out.push_str(start),
                    None => out.push_str(RESET),
                }
                current = wanted;
            }
            match segment.part {
                Part::Text(text) => out.push_str(text),
                Part::Directory => out.push_str("\\w"),
                Part::Symbol => out.push_str(self.symbol.ps1()),
                Part::LineBreak => out.push_str("\\n"),
            }
        }
        if current.is_some() {
            out.push_str(RESET);
        }
        out
    }

    /// Example prompt text for the settings preview, one entry per line.
    pub fn preview(self, failed: bool, directory: &'static str) -> Vec<Vec<(String, Segment)>> {
        let mut lines = vec![Vec::new()];
        for segment in self.segments(failed) {
            let text = match segment.part {
                Part::Text(text) => text,
                Part::Directory => directory,
                Part::Symbol => self.symbol.glyph(),
                Part::LineBreak => {
                    lines.push(Vec::new());
                    continue;
                }
            };
            lines.last_mut().unwrap().push((text.to_owned(), segment));
        }
        lines
    }

    /// Plain preview text, lines joined with `\n`.
    pub fn preview_text(self, failed: bool, directory: &'static str) -> String {
        self.preview(failed, directory)
            .iter()
            .map(|line| {
                line.iter()
                    .map(|(text, _)| text.as_str())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Contents of the session data file: `<style> <symbol>` on one line.
    pub fn data(self) -> String {
        format!("{} {}\n", self.style.key(), self.symbol.key())
    }

    /// Atomically replace the data file in a session profile directory. The
    /// shell picks it up when it next draws a prompt.
    pub fn publish(self, profile: &Path) -> Result<()> {
        use std::io::Write;
        let mut file = tempfile::NamedTempFile::new_in(profile)
            .with_context(|| format!("publish prompt to {}", profile.display()))?;
        file.write_all(self.data().as_bytes())?;
        file.persist(profile.join(DATA_FILE))?;
        Ok(())
    }
}

const RESET: &str = "\\[\\e[0m\\]";

fn ansi(slot: u8, normal: u8, bright: u8) -> String {
    if slot < 8 {
        (normal + slot).to_string()
    } else {
        (bright + slot - 8).to_string()
    }
}

fn single_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// Shell functions maintaining a Rodtty-managed `PS1` from `data`.
///
/// * `__rodtty_prompt_status` must run first among the prompt commands. It
///   records `$?` and returns it unchanged for the owner's later hooks.
/// * `__rodtty_prompt_update` reads the data file and selects a generated
///   `PS1` from a fixed whitelist. Unknown data keeps the current prompt.
///   Once `PS1` differs from what Rodtty last generated, the owner has taken
///   over, and Rodtty stops managing it for the rest of the shell's life.
pub fn shell_hook(data: &Path) -> Result<String> {
    let data = crate::profile::quote(data)?;
    let mut arms = String::new();
    for &style in PromptStyle::ALL {
        for &symbol in PromptSymbol::ALL {
            let prefs = PromptPreferences { style, symbol };
            let key = format!("{} {}", style.key(), symbol.key());
            if style.reports_status() {
                arms.push_str(&format!(
                    "  '{key} 0') __rodtty_prompt_next={} ;;\n  '{key} '*) __rodtty_prompt_next={} ;;\n",
                    single_quote(&prefs.ps1(false)),
                    single_quote(&prefs.ps1(true)),
                ));
            } else {
                arms.push_str(&format!(
                    "  '{key} '*) __rodtty_prompt_next={} ;;\n",
                    single_quote(&prefs.ps1(false)),
                ));
            }
        }
    }
    let fallback = single_quote(&PromptPreferences::default().ps1(false));
    // Brush applies redirections left to right, so stderr is silenced before
    // the data file opens. A missing or unreadable file keeps the prompt.
    Ok(format!(
        "__rodtty_prompt_status() {{ __rodtty_prompt_exit=$?; return $__rodtty_prompt_exit; }}\n\
__rodtty_prompt_update() {{\n\
 if [[ -n ${{__rodtty_prompt_owned-}} ]]; then return 0; fi\n\
 if [[ -n ${{__rodtty_ps1+set}} && \"$PS1\" != \"$__rodtty_ps1\" ]]; then __rodtty_prompt_owned=1; return 0; fi\n\
 __rodtty_prompt_style=; __rodtty_prompt_symbol=\n\
 if [[ -f {data} ]]; then read -r __rodtty_prompt_style __rodtty_prompt_symbol 2>/dev/null < {data}; fi\n\
 __rodtty_prompt_next=${{__rodtty_ps1-{fallback}}}\n\
 case \"$__rodtty_prompt_style $__rodtty_prompt_symbol ${{__rodtty_prompt_exit:-0}}\" in\n\
{arms} esac\n\
 __rodtty_ps1=$__rodtty_prompt_next\n\
 PS1=$__rodtty_ps1\n\
}}\n"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_the_original_classic_dollar_prompt() {
        let prefs = PromptPreferences::default();
        assert_eq!(prefs.style, PromptStyle::Classic);
        assert_eq!(prefs.symbol, PromptSymbol::Dollar);
        assert_eq!(prefs.ps1(false), "rodtty:\\w\\$ ");
        assert_eq!(prefs.ps1(true), "rodtty:\\w\\$ ");
        assert_eq!(prefs.data(), "classic dollar\n");
    }

    #[test]
    fn every_style_renders_each_symbol_in_ps1_and_preview() {
        let expect = [
            (PromptStyle::Classic, "rodtty:~/project$ "),
            (PromptStyle::Directory, "~/project $ "),
            (PromptStyle::Rodtty, "Rodtty $ "),
            (PromptStyle::Powerline, " Rodtty  ~/project  $ "),
            (PromptStyle::TwoLine, "~/project\n$ "),
        ];
        for (style, text) in expect {
            for &symbol in PromptSymbol::ALL {
                let prefs = PromptPreferences { style, symbol };
                assert_eq!(
                    prefs.preview_text(false, "~/project"),
                    text.replace('$', symbol.glyph())
                );
                let ps1 = prefs.ps1(false);
                assert!(ps1.contains(symbol.ps1()), "{ps1}");
                // Only conventional ANSI slots, so themes recolor prompts.
                assert!(!ps1.contains("38;") && !ps1.contains("48;"), "{ps1}");
            }
        }
        let powerline = PromptPreferences {
            style: PromptStyle::Powerline,
            symbol: PromptSymbol::Triangle,
        };
        assert_eq!(
            powerline.ps1(false),
            "\\[\\e[0;1;30;46m\\] Rodtty \\[\\e[0;30;44m\\] \\w \\[\\e[0m\\] ▶ "
        );
    }

    #[test]
    fn two_line_symbol_color_reports_the_last_exit_status() {
        let prefs = PromptPreferences {
            style: PromptStyle::TwoLine,
            symbol: PromptSymbol::Chevron,
        };
        assert_eq!(
            prefs.ps1(false),
            "\\[\\e[0;1;34m\\]\\w\\[\\e[0m\\]\\n\\[\\e[0;1;32m\\]›\\[\\e[0m\\] "
        );
        assert!(prefs.ps1(true).contains("\\e[0;1;31m\\]›"));
        let (_, symbol) = &prefs.preview(true, "~")[1][0];
        assert_eq!(symbol.fg, Some(RED));
    }

    #[test]
    fn publishing_replaces_the_data_file_atomically() {
        let dir = crate::test_support::tempdir().unwrap();
        PromptPreferences::default().publish(dir.path()).unwrap();
        let prefs = PromptPreferences {
            style: PromptStyle::TwoLine,
            symbol: PromptSymbol::DoubleChevron,
        };
        prefs.publish(dir.path()).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join(DATA_FILE)).unwrap(),
            "two_line double_chevron\n"
        );
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn hook_whitelists_every_combination_and_never_evaluates_data() {
        let hook = shell_hook(Path::new("/tmp/x/prompt")).unwrap();
        for &style in PromptStyle::ALL {
            for &symbol in PromptSymbol::ALL {
                assert!(hook.contains(&format!("'{} {} ", style.key(), symbol.key())));
            }
        }
        assert!(!hook.contains("source") && !hook.contains("eval"));
        assert!(hook.contains("read -r __rodtty_prompt_style __rodtty_prompt_symbol"));
    }
}
