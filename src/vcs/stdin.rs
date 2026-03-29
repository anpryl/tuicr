use std::path::{Path, PathBuf};

use crate::error::{Result, TuicrError};
use crate::model::{DiffFile, DiffHunk, DiffLine, FileStatus, LineOrigin};
use crate::syntax::SyntaxHighlighter;

use super::traits::{VcsBackend, VcsInfo, VcsType};

/// A backend for reviewing content read from stdin without a file or VCS.
///
/// All lines are presented as additions (like a new-file diff), allowing the
/// user to annotate piped content. Useful for Claude Code hooks that pass
/// plan content via JSON on stdin.
pub struct StdinBackend {
    info: VcsInfo,
    /// The content read from stdin
    content: String,
    /// Display name for the content (used for syntax highlighting detection)
    display_name: String,
}

impl StdinBackend {
    /// Create a new `StdinBackend` with the given content.
    ///
    /// `display_name` controls the filename shown in the UI and determines
    /// syntax highlighting (e.g. "plan.md" → markdown, "config.nix" → nix).
    /// Defaults to "stdin" if not provided.
    pub fn new(content: String, display_name: Option<String>) -> Result<Self> {
        if content.is_empty() {
            return Err(TuicrError::NoChanges);
        }

        let name = display_name.unwrap_or_else(|| "stdin".to_string());

        let info = VcsInfo {
            root_path: PathBuf::from("."),
            head_commit: "stdin".to_string(),
            branch_name: None,
            vcs_type: VcsType::Stdin,
        };

        Ok(Self {
            info,
            content,
            display_name: name,
        })
    }
}

impl VcsBackend for StdinBackend {
    fn info(&self) -> &VcsInfo {
        &self.info
    }

    fn get_working_tree_diff(&self, highlighter: &SyntaxHighlighter) -> Result<Vec<DiffFile>> {
        let lines: Vec<&str> = self.content.lines().collect();

        if lines.is_empty() {
            return Err(TuicrError::NoChanges);
        }

        // Build line contents and origins for syntax highlighting
        let line_contents: Vec<String> = lines.iter().map(|l| l.replace('\t', "    ")).collect();
        let line_origins: Vec<LineOrigin> = vec![LineOrigin::Addition; line_contents.len()];

        // Apply syntax highlighting using the display name for language detection
        let highlight_sequences =
            SyntaxHighlighter::split_diff_lines_for_highlighting(&line_contents, &line_origins);
        let synthetic_path = PathBuf::from(&self.display_name);
        let new_highlighted_lines =
            highlighter.highlight_file_lines(&synthetic_path, &highlight_sequences.new_lines);

        // Build DiffLines
        let mut diff_lines = Vec::with_capacity(lines.len());
        for (i, content) in line_contents.iter().enumerate() {
            let line_num = (i + 1) as u32;

            let highlighted_spans = highlighter.highlighted_line_for_diff_with_background(
                None,
                new_highlighted_lines.as_deref(),
                None,
                highlight_sequences.new_line_indices[i],
                LineOrigin::Addition,
            );

            diff_lines.push(DiffLine {
                origin: LineOrigin::Addition,
                content: content.clone(),
                old_lineno: None,
                new_lineno: Some(line_num),
                highlighted_spans,
            });
        }

        let total_lines = lines.len() as u32;

        let rel_path = PathBuf::from(&self.display_name);

        let hunk = DiffHunk {
            header: format!("@@ -0,0 +1,{} @@", total_lines),
            lines: diff_lines,
            old_start: 0,
            old_count: 0,
            new_start: 1,
            new_count: total_lines,
        };

        let file = DiffFile {
            old_path: None,
            new_path: Some(rel_path),
            status: FileStatus::Added,
            hunks: vec![hunk],
            is_binary: false,
            is_too_large: false,
            is_commit_message: false,
        };

        Ok(vec![file])
    }

    fn fetch_context_lines(
        &self,
        _file_path: &Path,
        _file_status: FileStatus,
        start_line: u32,
        end_line: u32,
    ) -> Result<Vec<DiffLine>> {
        if start_line > end_line || start_line == 0 {
            return Ok(Vec::new());
        }

        let lines: Vec<&str> = self.content.lines().collect();
        let mut result = Vec::new();

        for line_num in start_line..=end_line {
            let idx = (line_num - 1) as usize;
            if idx < lines.len() {
                result.push(DiffLine {
                    origin: LineOrigin::Context,
                    content: lines[idx].to_string(),
                    old_lineno: Some(line_num),
                    new_lineno: Some(line_num),
                    highlighted_spans: None,
                });
            }
        }

        Ok(result)
    }
}
