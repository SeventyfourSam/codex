//! Custom release notice; official update renderers and fixtures stay intact.

use super::*;
use crate::style::accent_color;

#[cfg_attr(debug_assertions, allow(dead_code))]
#[derive(Debug)]
pub(crate) struct CustomUpdateAvailableHistoryCell {
    latest_version: String,
    update_action: Option<UpdateAction>,
}

#[cfg_attr(debug_assertions, allow(dead_code))]
impl CustomUpdateAvailableHistoryCell {
    pub(crate) fn new(latest_version: String, update_action: Option<UpdateAction>) -> Self {
        Self {
            latest_version,
            update_action,
        }
    }
}

impl HistoryCell for CustomUpdateAvailableHistoryCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        use ratatui_macros::line;
        use ratatui_macros::text;
        let update_instruction = if self.update_action.is_some() {
            line!["Run ", "codex update".fg(accent_color()), " to update."]
        } else {
            line![
                "See ",
                "https://github.com/SeventyfourSam/codex"
                    .fg(accent_color())
                    .underlined(),
                " for installation options."
            ]
        };

        let content = text![
            line![
                "✨\u{200A}".bold().fg(accent_color()),
                "Update available!".bold().fg(accent_color()),
                " ",
                format!(
                    "{} -> {}",
                    codex_utils_cli::CUSTOM_VERSION,
                    self.latest_version
                )
                .bold(),
            ],
            update_instruction,
            "",
            "See full release notes:",
            codex_install_context::CUSTOM_RELEASE_URL
                .fg(accent_color())
                .underlined(),
        ];

        let inner_width = content
            .width()
            .min(usize::from(width.saturating_sub(4)))
            .max(1);
        let lines = adaptive_wrap_lines(content.lines, RtOptions::new(inner_width));
        with_border_with_inner_width(lines, inner_width)
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        let update_instruction = if self.update_action.is_some() {
            "Run codex update to update.".to_string()
        } else {
            "See https://github.com/SeventyfourSam/codex for installation options.".to_string()
        };
        vec![
            Line::from("Update available!"),
            Line::from(format!(
                "{} -> {}",
                codex_utils_cli::CUSTOM_VERSION,
                self.latest_version
            )),
            Line::from(update_instruction),
            Line::from(""),
            Line::from("See full release notes:"),
            Line::from(codex_install_context::CUSTOM_RELEASE_URL),
        ]
    }

    fn display_hyperlink_lines(&self, width: u16) -> Vec<HyperlinkLine> {
        crate::terminal_hyperlinks::annotate_web_urls(self.display_lines(width))
    }

    fn transcript_hyperlink_lines(&self, width: u16) -> Vec<HyperlinkLine> {
        self.display_hyperlink_lines(width)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_notice_links_to_custom_release_and_keeps_the_custom_version() {
        let cell = CustomUpdateAvailableHistoryCell::new(
            "999.0.0-custom.4".to_string(),
            Some(UpdateAction::StandaloneWindows),
        );
        let text = cell
            .raw_lines()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains(codex_utils_cli::CUSTOM_VERSION));
        assert!(text.contains("codex update"));
        assert!(text.contains(codex_install_context::CUSTOM_RELEASE_URL));
        assert!(
            cell.display_hyperlink_lines(120)
                .iter()
                .flat_map(|line| &line.hyperlinks)
                .any(|link| link.destination == codex_install_context::CUSTOM_RELEASE_URL)
        );
    }
}
