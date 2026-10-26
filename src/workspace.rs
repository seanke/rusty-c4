//! The open project's source files, held in memory so they can be edited: the 3D view is
//! rebuilt from these texts a moment after you stop typing, and saving writes them back.

use std::path::PathBuf;

use bevy::prelude::*;

use crate::scene::{Nav, NavQueue};

/// How long typing must pause before the model is rebuilt.
const SETTLE: f32 = 0.35;

pub struct SourceFile {
    /// Path relative to the project root, with `/` separators: also the name diagnostics use.
    pub rel: String,
    /// Where it lives on disk (None for the built-in examples).
    pub path: Option<PathBuf>,
    pub text: String,
    /// The text as last loaded or saved.
    pub saved: String,
    /// Bumped when the text is replaced from outside the editor (a reload).
    pub rev: u64,
}

impl SourceFile {
    pub fn dirty(&self) -> bool {
        self.text != self.saved
    }
}

#[derive(Resource, Default)]
pub struct Workspace {
    pub files: Vec<SourceFile>,
    /// The file open in the editor.
    pub active: usize,
    /// Bumped on every edit; the model is rebuilt when it settles.
    pub edits: u64,
    parsed: u64,
    idle: f32,
    /// Bumped whenever a different project is loaded.
    generation: u64,
}

impl Workspace {
    pub fn set(&mut self, files: Vec<SourceFile>) {
        self.files = files;
        self.active = 0;
        self.parsed = self.edits;
        self.generation += 1;
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn sources(&self) -> Vec<(String, String)> {
        self.files.iter().map(|f| (f.rel.clone(), f.text.clone())).collect()
    }

    /// Note an edit: the model is rebuilt once typing pauses.
    pub fn edited(&mut self) {
        self.edits += 1;
        self.idle = 0.0;
    }
}

/// The models bundled into the app.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Example {
    Cloud,
    Mermaid,
}

impl Example {
    pub fn files(self) -> Vec<(&'static str, &'static str)> {
        match self {
            Example::Cloud => vec![("cloud.c4", include_str!("../examples/cloud.c4"))],
            Example::Mermaid => vec![
                ("platform.mmd", include_str!("../examples/mermaid/platform.mmd")),
                ("publish.mmd", include_str!("../examples/mermaid/publish.mmd")),
            ],
        }
    }
}

/// Rebuild the model from the edited texts once typing has paused.
pub fn live(time: Res<Time>, mut ws: ResMut<Workspace>, mut queue: ResMut<NavQueue>) {
    if ws.edits == ws.parsed {
        return;
    }
    ws.idle += time.delta_secs();
    if ws.idle >= SETTLE {
        ws.parsed = ws.edits;
        queue.0.push(Nav::Reparse);
    }
}

#[cfg(test)]
mod tests {
    use super::Example;
    use crate::parser::parse_sources;

    #[test]
    fn examples_parse_cleanly() {
        for ex in [Example::Cloud, Example::Mermaid] {
            let sources: Vec<(String, String)> = ex.files().into_iter().map(|(n, t)| (n.to_string(), t.to_string())).collect();
            let (m, d) = parse_sources(&sources);
            assert!(d.is_empty(), "{ex:?}: {d:?}");
            assert!(!m.views.is_empty(), "{ex:?} has dynamic views");
        }
    }
}
