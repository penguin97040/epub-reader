//! Local persistence: recent books and reading positions.
//! Plain JSON in the platform config directory. No cloud, no analytics.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recent {
    pub path: String,
    pub title: String,
    /// Monotonic open counter, newest first when sorted descending.
    pub opened: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
pub struct Position {
    pub chapter: usize,
    /// Vertical scroll fraction within the chapter, 0.0 to 1.0.
    pub fraction: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    #[serde(default)]
    pub recents: Vec<Recent>,
    #[serde(default)]
    pub positions: HashMap<String, Position>,
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    #[serde(default = "default_line_height")]
    pub line_height: f32,
    #[serde(default = "default_ui_scale")]
    pub ui_scale: f32,
    #[serde(default = "default_column_width")]
    pub column_width: f32,
    #[serde(default)]
    next_seq: u64,
}

fn default_font_size() -> f32 {
    crate::theme::BODY_SIZE
}

fn default_line_height() -> f32 {
    1.6
}

fn default_ui_scale() -> f32 {
    1.0
}

fn default_column_width() -> f32 {
    crate::theme::CONTENT_MAX_WIDTH
}

impl Default for State {
    fn default() -> Self {
        Self {
            recents: Vec::new(),
            positions: HashMap::new(),
            font_size: default_font_size(),
            line_height: default_line_height(),
            ui_scale: default_ui_scale(),
            column_width: default_column_width(),
            next_seq: 0,
        }
    }
}

impl State {
    fn path() -> Option<PathBuf> {
        directories::ProjectDirs::from("", "", "epub-reader")
            .map(|dirs| dirs.config_dir().join("state.json"))
    }

    pub fn load() -> State {
        let mut state = Self::path()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|raw| serde_json::from_str::<State>(&raw).ok())
            .unwrap_or_default();
        if !(12.0..=40.0).contains(&state.font_size) {
            state.font_size = default_font_size();
        }
        if !(1.2..=2.2).contains(&state.line_height) {
            state.line_height = default_line_height();
        }
        if !(0.8..=2.0).contains(&state.ui_scale) {
            state.ui_scale = default_ui_scale();
        }
        if !(480.0..=900.0).contains(&state.column_width) {
            state.column_width = default_column_width();
        }
        state
    }

    pub fn reset_reading(&mut self) {
        self.font_size = default_font_size();
        self.line_height = default_line_height();
        self.ui_scale = default_ui_scale();
        self.column_width = default_column_width();
    }

    pub fn save(&self) {
        let Some(path) = Self::path() else { return };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(raw) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, raw);
        }
    }

    pub fn note_recent(&mut self, path: &str, title: &str) {
        self.next_seq += 1;
        let seq = self.next_seq;
        self.recents.retain(|r| r.path != path);
        self.recents.push(Recent {
            path: path.to_owned(),
            title: title.to_owned(),
            opened: seq,
        });
        self.recents.sort_by_key(|r| std::cmp::Reverse(r.opened));
        self.recents.truncate(10);
    }

    pub fn forget(&mut self, path: &str) {
        self.recents.retain(|r| r.path != path);
        self.positions.remove(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reading_preferences_have_usable_defaults() {
        let state: State = serde_json::from_str("{}").unwrap();
        assert_eq!(state.font_size, crate::theme::BODY_SIZE);
        assert_eq!(state.line_height, 1.6);
        assert_eq!(state.ui_scale, 1.0);
        assert_eq!(state.column_width, crate::theme::CONTENT_MAX_WIDTH);
    }

    #[test]
    fn reset_restores_the_reading_defaults() {
        let mut state = State {
            font_size: 30.0,
            line_height: 2.0,
            ui_scale: 1.8,
            column_width: 900.0,
            ..State::default()
        };
        state.reset_reading();
        assert_eq!(state.font_size, crate::theme::BODY_SIZE);
        assert_eq!(state.line_height, 1.6);
        assert_eq!(state.ui_scale, 1.0);
        assert_eq!(state.column_width, crate::theme::CONTENT_MAX_WIDTH);
    }

    #[test]
    fn recent_list_stays_ordered_and_bounded() {
        let mut state = State::default();
        for i in 0..15 {
            state.note_recent(&format!("/books/{i}.epub"), &format!("Book {i}"));
        }
        assert_eq!(state.recents.len(), 10);
        assert_eq!(state.recents[0].path, "/books/14.epub");
        state.note_recent("/books/5.epub", "Book 5");
        assert_eq!(state.recents[0].path, "/books/5.epub");
        assert_eq!(state.recents.len(), 10);
    }

    #[test]
    fn forget_removes_position_too() {
        let mut state = State::default();
        state.note_recent("/a.epub", "A");
        state.positions.insert(
            "/a.epub".to_owned(),
            Position {
                chapter: 2,
                fraction: 0.5,
            },
        );
        state.forget("/a.epub");
        assert!(state.recents.is_empty());
        assert!(!state.positions.contains_key("/a.epub"));
    }
}
