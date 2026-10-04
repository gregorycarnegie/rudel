//! Strudel's editor keymaps (`codemirror/keybindings.mjs`): CodeMirror's own
//! (egui's text editing, what rudel always had), Vim, Emacs, VS Code and Helix.
//! Upstream takes them from `@replit/codemirror-vim`/`-emacs`/`-vscode-keymap`
//! and `codemirror-helix`; this is their everyday core, plus Strudel's own
//! additions: `:w` evaluates and `:q` stops in Vim and Helix, and Vim's `gc`
//! toggles comments.
//!
//! The engine works on the text as chars and a cursor, knowing nothing of
//! egui; [`run`] feeds it the frame's key events before the `TextEdit` sees
//! them, and the `TextEdit` gets whatever the keymap passes on (typing in
//! insert mode, everything Emacs and VS Code do not rebind).

use super::{brackets::matching_bracket_index, edit, menu::EditorAction};
use eframe::egui::{self, Key, Modifiers, text::CCursorRange};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Keymap {
    #[default]
    Codemirror,
    Vim,
    Emacs,
    Vscode,
    Helix,
}

impl Keymap {
    pub(crate) const ALL: [Keymap; 5] = [
        Keymap::Codemirror,
        Keymap::Vim,
        Keymap::Emacs,
        Keymap::Vscode,
        Keymap::Helix,
    ];

    /// Upstream's setting value, which is also what the settings save.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Keymap::Codemirror => "codemirror",
            Keymap::Vim => "vim",
            Keymap::Emacs => "emacs",
            Keymap::Vscode => "vscode",
            Keymap::Helix => "helix",
        }
    }

    pub(crate) fn named(name: &str) -> Option<Keymap> {
        Keymap::ALL.into_iter().find(|k| k.label() == name)
    }

    fn modal(self) -> bool {
        matches!(self, Keymap::Vim | Keymap::Helix)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum Mode {
    #[default]
    Normal,
    Insert,
    /// Vim's `v`, Helix's select mode.
    Visual,
    /// Vim's `V`.
    VisualLine,
    /// `:` or `/` and what has been typed after it.
    Command(String),
}

/// One key, as a keymap reads it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Input {
    Char(char),
    Key(Key, Modifiers),
    Copy,
    Cut,
}

/// The text and the cursor. In a modal keymap's normal and visual modes the
/// cursor sits *on* a char and `anchor..=head` is the selection; otherwise
/// `head` is between chars and `anchor..head` is the selection, as egui has it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Buffer {
    pub(crate) text: Vec<char>,
    pub(crate) anchor: usize,
    pub(crate) head: usize,
}

impl Buffer {
    #[cfg(test)]
    fn new(text: &str, at: usize) -> Buffer {
        Buffer {
            text: text.chars().collect(),
            anchor: at,
            head: at,
        }
    }

    fn at(&mut self, pos: usize) {
        self.anchor = pos;
        self.head = pos;
    }

    fn string(&self) -> String {
        self.text.iter().collect()
    }
}

pub(crate) enum Outcome {
    /// Not the keymap's: the `TextEdit` gets it.
    Pass,
    Handled,
    Undo,
    Redo,
    /// Handled, and this goes to the system clipboard.
    Clipboard(String),
    Action(EditorAction),
}

#[derive(Clone, Default)]
pub(crate) struct KeymapState {
    pub(crate) keymap: Keymap,
    pub(crate) mode: Mode,
    /// Keys of a command still being typed (`d2`, `g`, `f`).
    pending: String,
    /// Vim's unnamed register, Emacs's kill ring (one entry deep).
    register: String,
    linewise: bool,
    search: String,
    /// Emacs's mark is set: `anchor` is it.
    mark: bool,
    last_was_kill: bool,
    /// A command line's error, shown until the next key.
    message: Option<String>,
    /// The egui cursor this state last set, and the anchor and head it came
    /// from: egui's range cannot tell a block cursor's anchor from its head,
    /// so while the cursor is still there (no mouse click moved it) these hold.
    written: Option<(CCursorRange, usize, usize)>,
}

// --- text helpers, on chars --------------------------------------------------

fn line_start(t: &[char], p: usize) -> usize {
    t[..p.min(t.len())]
        .iter()
        .rposition(|&c| c == '\n')
        .map_or(0, |i| i + 1)
}

/// Where the line holding `p` ends: its newline, or the end of the text.
fn line_end(t: &[char], p: usize) -> usize {
    t[p.min(t.len())..]
        .iter()
        .position(|&c| c == '\n')
        .map_or(t.len(), |i| p.min(t.len()) + i)
}

/// The last char of the line a normal-mode cursor can sit on.
fn line_last(t: &[char], p: usize) -> usize {
    let (start, end) = (line_start(t, p), line_end(t, p));
    if end > start { end - 1 } else { start }
}

fn first_non_blank(t: &[char], p: usize) -> usize {
    let start = line_start(t, p);
    let end = line_end(t, p);
    (start..end)
        .find(|&i| !matches!(t[i], ' ' | '\t'))
        .unwrap_or(end)
}

fn next_line(t: &[char], p: usize) -> Option<usize> {
    let end = line_end(t, p);
    (end < t.len()).then_some(end + 1)
}

fn prev_line(t: &[char], p: usize) -> Option<usize> {
    let start = line_start(t, p);
    (start > 0).then(|| line_start(t, start - 1))
}

fn line_number(t: &[char], p: usize) -> usize {
    t[..p.min(t.len())].iter().filter(|&&c| c == '\n').count()
}

fn line_at(t: &[char], n: usize) -> usize {
    let mut p = 0;
    for _ in 0..n {
        match next_line(t, p) {
            Some(next) => p = next,
            None => break,
        }
    }
    p
}

/// Where a normal-mode cursor may sit: on a char, not past the end of the
/// line (an empty line's own position is fine).
fn clamp_normal(t: &[char], p: usize) -> usize {
    if t.is_empty() {
        return 0;
    }
    let p = p.min(t.len());
    if p == t.len() && t[p - 1] != '\n' {
        return p - 1;
    }
    if p < t.len() && t[p] == '\n' && p > line_start(t, p) {
        return p - 1;
    }
    p
}

/// Move `count` lines down (or up), keeping the column where the line allows.
fn vertical(t: &[char], p: usize, count: usize, down: bool) -> usize {
    let col = p - line_start(t, p);
    let mut line = line_start(t, p);
    for _ in 0..count {
        match if down {
            next_line(t, line)
        } else {
            prev_line(t, line)
        } {
            Some(next) => line = next,
            None => break,
        }
    }
    (line + col).min(line_end(t, line))
}

fn class(c: char, big: bool) -> u8 {
    if c.is_whitespace() {
        0
    } else if big || c.is_alphanumeric() || c == '_' {
        1
    } else {
        2
    }
}

fn word_forward(t: &[char], p: usize, big: bool) -> usize {
    let mut i = p;
    if i >= t.len() {
        return t.len();
    }
    let c = class(t[i], big);
    if c != 0 {
        while i < t.len() && class(t[i], big) == c {
            i += 1;
        }
    }
    while i < t.len() && class(t[i], big) == 0 {
        i += 1;
    }
    i
}

fn word_back(t: &[char], p: usize, big: bool) -> usize {
    let mut i = p.min(t.len());
    while i > 0 && class(t[i - 1], big) == 0 {
        i -= 1;
    }
    if i == 0 {
        return 0;
    }
    let c = class(t[i - 1], big);
    while i > 0 && class(t[i - 1], big) == c {
        i -= 1;
    }
    i
}

fn word_end(t: &[char], p: usize, big: bool) -> usize {
    let mut i = p + 1;
    while i < t.len() && class(t[i], big) == 0 {
        i += 1;
    }
    if i >= t.len() {
        return t.len().saturating_sub(1);
    }
    let c = class(t[i], big);
    while i + 1 < t.len() && class(t[i + 1], big) == c {
        i += 1;
    }
    i
}

fn blank_line(t: &[char], line: usize) -> bool {
    t[line..line_end(t, line)].iter().all(|c| c.is_whitespace())
}

fn paragraph(t: &[char], p: usize, forward: bool) -> usize {
    let mut line = line_start(t, p);
    // Off the blank lines this starts on, then to the next blank one.
    let step = |l: usize| {
        if forward {
            next_line(t, l)
        } else {
            prev_line(t, l)
        }
    };
    while blank_line(t, line) {
        match step(line) {
            Some(next) => line = next,
            None => return if forward { t.len() } else { 0 },
        }
    }
    loop {
        match step(line) {
            Some(next) if blank_line(t, next) => return next,
            Some(next) => line = next,
            None => return if forward { t.len() } else { 0 },
        }
    }
}

/// `f`/`t`/`F`/`T`: the `count`th `c` on this line.
fn find_in_line(
    t: &[char],
    p: usize,
    c: char,
    forward: bool,
    till: bool,
    count: usize,
) -> Option<usize> {
    let (start, end) = (line_start(t, p), line_end(t, p));
    let mut found = p;
    for _ in 0..count {
        found = if forward {
            (found + 1..end).find(|&i| t[i] == c)?
        } else {
            (start..found).rev().find(|&i| t[i] == c)?
        };
    }
    Some(match (till, forward) {
        (false, _) => found,
        (true, true) => found - 1,
        (true, false) => found + 1,
    })
}

/// `%`: the bracket matching the one under (or next on the line after) `p`.
fn match_bracket(t: &[char], p: usize) -> Option<usize> {
    let end = line_end(t, p);
    let on = (p..end).find(|&i| matches!(t[i], '(' | ')' | '[' | ']' | '{' | '}'))?;
    matching_bracket_index(t, on)
}

/// `iw`/`aw`, `i"`/`a"`, `i(`/`a(` and the like: the half-open range.
fn text_object(t: &[char], p: usize, around: bool, kind: char) -> Option<(usize, usize)> {
    if t.is_empty() {
        return None;
    }
    let p = p.min(t.len() - 1);
    match kind {
        'w' | 'W' => {
            let big = kind == 'W';
            let c = class(t[p], big);
            let mut start = p;
            while start > 0 && class(t[start - 1], big) == c && t[start - 1] != '\n' {
                start -= 1;
            }
            let mut end = p + 1;
            while end < t.len() && class(t[end], big) == c && t[end] != '\n' {
                end += 1;
            }
            if around {
                while end < t.len() && matches!(t[end], ' ' | '\t') {
                    end += 1;
                }
            }
            Some((start, end))
        }
        '"' | '\'' | '`' => {
            let (ls, le) = (line_start(t, p), line_end(t, p));
            let quotes: Vec<usize> = (ls..le).filter(|&i| t[i] == kind).collect();
            let pair = quotes
                .chunks(2)
                .find(|pair| pair.len() == 2 && pair[0] <= p && p <= pair[1])
                .or_else(|| quotes.chunks(2).find(|pair| pair.len() == 2 && pair[0] > p))?;
            Some(if around {
                (pair[0], pair[1] + 1)
            } else {
                (pair[0] + 1, pair[1])
            })
        }
        '(' | ')' | 'b' | '[' | ']' | '{' | '}' | 'B' => {
            let (open, close) = match kind {
                '(' | ')' | 'b' => ('(', ')'),
                '[' | ']' => ('[', ']'),
                _ => ('{', '}'),
            };
            let mut depth = 0;
            let start = (0..=p).rev().find(|&i| {
                if t[i] == close && i != p {
                    depth += 1;
                } else if t[i] == open {
                    if depth == 0 {
                        return true;
                    }
                    depth -= 1;
                }
                false
            })?;
            let end = matching_bracket_index(t, start)?;
            Some(if around {
                (start, end + 1)
            } else {
                (start + 1, end)
            })
        }
        _ => None,
    }
}

fn toggle_case(c: char) -> char {
    if c.is_uppercase() {
        c.to_lowercase().next().unwrap_or(c)
    } else {
        c.to_uppercase().next().unwrap_or(c)
    }
}

/// The lines `start..end` touch, as a half-open range ending after the last
/// one's newline (or at the end of the text).
fn whole_lines(t: &[char], start: usize, end: usize) -> (usize, usize) {
    let last = end.max(start + 1).saturating_sub(1).min(t.len());
    let end = line_end(t, last);
    (
        line_start(t, start),
        if end < t.len() { end + 1 } else { end },
    )
}

/// Run one of the editor's own line edits (`edit.rs`) over `start..end`.
fn line_edit(
    buf: &mut Buffer,
    start: usize,
    end: usize,
    f: impl FnOnce(&mut String, CCursorRange) -> CCursorRange,
) {
    let mut text = buf.string();
    let range = CCursorRange::two(
        egui::text::CCursor::new(egui::text::CharIndex(start)),
        egui::text::CCursor::new(egui::text::CharIndex(end.max(start))),
    );
    f(&mut text, range);
    buf.text = text.chars().collect();
}

fn join_lines(buf: &mut Buffer, line: usize, count: usize) {
    for _ in 0..count.max(1) {
        let end = line_end(&buf.text, line);
        if end >= buf.text.len() {
            break;
        }
        let mut next = end + 1;
        while next < buf.text.len() && matches!(buf.text[next], ' ' | '\t') {
            next += 1;
        }
        let glue = if next >= buf.text.len() || buf.text[next] == '\n' || buf.text[next] == ')' {
            ""
        } else {
            " "
        };
        buf.text.splice(end..next, glue.chars());
        buf.at(end);
    }
}

impl KeymapState {
    pub(crate) fn new(keymap: Keymap) -> KeymapState {
        KeymapState {
            keymap,
            ..KeymapState::default()
        }
    }

    /// The state line under the editor: the mode, keys typed so far, or a
    /// command line.
    pub(crate) fn status(&self) -> Option<String> {
        if let Some(message) = &self.message {
            return Some(message.clone());
        }
        let helix = self.keymap == Keymap::Helix;
        let mode = match (&self.mode, self.keymap) {
            (_, Keymap::Codemirror | Keymap::Vscode) => return None,
            (_, Keymap::Emacs) => return self.mark.then(|| "Mark set".to_string()),
            (Mode::Command(line), _) => return Some(line.clone()),
            (Mode::Normal, _) => {
                if helix {
                    "NOR"
                } else {
                    "NORMAL"
                }
            }
            (Mode::Insert, _) => {
                if helix {
                    "INS"
                } else {
                    "INSERT"
                }
            }
            (Mode::Visual, _) => {
                if helix {
                    "SEL"
                } else {
                    "VISUAL"
                }
            }
            (Mode::VisualLine, _) => "VISUAL LINE",
        };
        Some(if self.pending.is_empty() {
            mode.to_string()
        } else {
            format!("{mode}  {}", self.pending)
        })
    }

    /// Whether the cursor sits on a char (block cursor) rather than between.
    pub(crate) fn on_char(&self) -> bool {
        self.keymap.modal() && !matches!(self.mode, Mode::Insert | Mode::Command(_))
    }

    /// The buffer's cursor as egui's: a block cursor is a one-char selection.
    pub(crate) fn to_egui(&self, buf: &Buffer) -> CCursorRange {
        use egui::text::{CCursor, CharIndex};
        let len = buf.text.len();
        let range = |a: usize, b: usize| {
            CCursorRange::two(
                CCursor::new(CharIndex(a.min(len))),
                CCursor::new(CharIndex(b.min(len))),
            )
        };
        if !self.on_char() {
            return range(buf.anchor, buf.head);
        }
        let (anchor, head) = if self.mode == Mode::VisualLine {
            let (s, e) = whole_lines(
                &buf.text,
                buf.anchor.min(buf.head),
                buf.anchor.max(buf.head) + 1,
            );
            if buf.head >= buf.anchor {
                (s, e)
            } else {
                (e, s)
            }
        } else if buf.head >= buf.anchor {
            (buf.anchor, buf.head + 1)
        } else {
            (buf.anchor + 1, buf.head)
        };
        range(anchor, head)
    }

    /// egui's cursor as the buffer's (after a mouse click moved it).
    pub(crate) fn buffer_at(&self, text: Vec<char>, range: CCursorRange) -> Buffer {
        let (primary, secondary) = (range.primary.index.0, range.secondary.index.0);
        let mut buf = Buffer {
            text,
            anchor: secondary,
            head: primary,
        };
        if self.on_char() {
            if primary > secondary {
                buf.head = primary - 1;
            } else if primary < secondary {
                buf.anchor = secondary - 1;
            }
            if self.mode == Mode::Normal && self.keymap == Keymap::Vim {
                buf.anchor = buf.head;
            }
            buf.head = clamp_normal(&buf.text, buf.head);
            buf.anchor = clamp_normal(&buf.text, buf.anchor);
        }
        buf
    }

    pub(crate) fn handle(&mut self, buf: &mut Buffer, input: Input) -> Outcome {
        self.message = None;
        match self.keymap {
            Keymap::Codemirror => Outcome::Pass,
            Keymap::Vim | Keymap::Helix => self.modal(buf, input),
            Keymap::Emacs => self.emacs(buf, input),
            Keymap::Vscode => self.vscode(buf, input),
        }
    }

    // --- shared modal plumbing ------------------------------------------------

    fn modal(&mut self, buf: &mut Buffer, input: Input) -> Outcome {
        let escape = matches!(input, Input::Key(Key::Escape, _))
            || matches!(input, Input::Key(Key::OpenBracket, m) if m.ctrl);
        match self.mode.clone() {
            Mode::Insert => {
                if !escape {
                    return Outcome::Pass;
                }
                self.mode = Mode::Normal;
                let head = buf.head;
                let back = if head > line_start(&buf.text, head) {
                    head - 1
                } else {
                    head
                };
                buf.at(clamp_normal(&buf.text, back));
                Outcome::Handled
            }
            Mode::Command(line) => self.command_line(buf, line, input, escape),
            _ => {
                if escape {
                    self.pending.clear();
                    if self.mode != Mode::Normal {
                        self.mode = Mode::Normal;
                    }
                    if self.keymap == Keymap::Vim || self.mode == Mode::Normal {
                        buf.anchor = buf.head;
                    }
                    return Outcome::Handled;
                }
                let c = match input {
                    Input::Char(c) => c,
                    Input::Key(key, m) if m.ctrl || m.command => {
                        return match (self.keymap, key) {
                            (Keymap::Vim, Key::R) => Outcome::Redo,
                            (Keymap::Helix, Key::C) => {
                                self.comment(buf);
                                Outcome::Handled
                            }
                            _ => Outcome::Pass,
                        };
                    }
                    Input::Key(key, m) if m.alt => {
                        let _ = key;
                        return Outcome::Pass;
                    }
                    Input::Key(key, m) => match key {
                        Key::ArrowLeft => 'h',
                        Key::ArrowRight => 'l',
                        Key::ArrowUp => 'k',
                        Key::ArrowDown => 'j',
                        Key::Backspace => 'h',
                        Key::Home => '0',
                        Key::End => '$',
                        Key::Delete if self.keymap == Keymap::Vim => 'x',
                        Key::Delete => 'd',
                        Key::Enter if self.keymap == Keymap::Vim && !m.shift => {
                            let next = next_line(&buf.text, buf.head).unwrap_or(buf.head);
                            buf.head = first_non_blank(&buf.text, next);
                            if self.mode == Mode::Normal {
                                buf.anchor = buf.head;
                            }
                            return Outcome::Handled;
                        }
                        // A printable key arrives again as text; anything
                        // else (Tab) is not typing in this mode.
                        _ => return Outcome::Handled,
                    },
                    Input::Copy | Input::Cut => return Outcome::Pass,
                };
                self.pending.push(c);
                let pending = self.pending.clone();
                let outcome = if self.keymap == Keymap::Vim {
                    self.vim(buf, &pending)
                } else {
                    self.helix(buf, &pending)
                };
                match outcome {
                    Some(outcome) => {
                        self.pending.clear();
                        outcome
                    }
                    None => Outcome::Handled,
                }
            }
        }
    }

    fn command_line(
        &mut self,
        buf: &mut Buffer,
        mut line: String,
        input: Input,
        escape: bool,
    ) -> Outcome {
        if escape {
            self.mode = Mode::Normal;
            return Outcome::Handled;
        }
        match input {
            Input::Char(c) => line.push(c),
            Input::Key(Key::Backspace, _) => {
                line.pop();
                if line.is_empty() {
                    self.mode = Mode::Normal;
                    return Outcome::Handled;
                }
            }
            Input::Key(Key::Enter, _) => {
                self.mode = Mode::Normal;
                return self.execute(buf, &line);
            }
            _ => return Outcome::Handled,
        }
        self.mode = Mode::Command(line);
        Outcome::Handled
    }

    /// A `:` command or a `/` search.
    fn execute(&mut self, buf: &mut Buffer, line: &str) -> Outcome {
        if let Some(needle) = line.strip_prefix('/') {
            if !needle.is_empty() {
                self.search = needle.to_string();
            }
            self.search_next(buf, true);
            return Outcome::Handled;
        }
        let command = line.trim_start_matches(':').trim();
        match command {
            "w" | "write" => Outcome::Action(EditorAction::PrimaryEval),
            "q" | "quit" => Outcome::Action(EditorAction::Hush),
            "" => Outcome::Handled,
            n if n.chars().all(|c| c.is_ascii_digit()) => {
                let line = n.parse::<usize>().unwrap_or(1).max(1) - 1;
                buf.at(first_non_blank(&buf.text, line_at(&buf.text, line)));
                Outcome::Handled
            }
            other => {
                self.message = Some(format!("Not an editor command: {other}"));
                Outcome::Handled
            }
        }
    }

    fn search_next(&mut self, buf: &mut Buffer, forward: bool) {
        let needle: Vec<char> = self.search.chars().collect();
        if needle.is_empty() || needle.len() > buf.text.len() {
            return;
        }
        let n = buf.text.len();
        let hit = |i: usize| buf.text[i..].starts_with(&needle);
        let found = if forward {
            (buf.head + 1..n)
                .chain(0..=buf.head.min(n - 1))
                .find(|&i| hit(i))
        } else {
            (0..buf.head)
                .rev()
                .chain((buf.head..n).rev())
                .find(|&i| hit(i))
        };
        match found {
            Some(i) => buf.at(i),
            None => self.message = Some(format!("Pattern not found: {}", self.search)),
        }
    }

    fn yank(&mut self, t: &[char], start: usize, end: usize, linewise: bool) {
        self.register = t[start..end.min(t.len())].iter().collect();
        if linewise && !self.register.ends_with('\n') {
            self.register.push('\n');
        }
        self.linewise = linewise;
    }

    fn comment(&self, buf: &mut Buffer) {
        let (start, end) = (buf.anchor.min(buf.head), buf.anchor.max(buf.head) + 1);
        line_edit(buf, start, end, edit::toggle_line_comments);
        buf.head = clamp_normal(&buf.text, buf.head);
        buf.anchor = clamp_normal(&buf.text, buf.anchor);
    }

    /// Paste the register after (`p`) or before (`P`) the cursor.
    fn paste(&mut self, buf: &mut Buffer, after: bool, count: usize) {
        let text: String = self.register.repeat(count.max(1));
        if text.is_empty() {
            return;
        }
        let t = &buf.text;
        if self.linewise {
            let (at, insert) = match (after, next_line(t, buf.head)) {
                (true, Some(next)) => (next, text),
                (true, None) => (t.len(), format!("\n{}", text.trim_end_matches('\n'))),
                (false, _) => (line_start(t, buf.head), text),
            };
            let land = if insert.starts_with('\n') { at + 1 } else { at };
            buf.text.splice(at..at, insert.chars());
            buf.at(first_non_blank(&buf.text, land));
        } else {
            let at = if after && !t.is_empty() && buf.head < t.len() && t[buf.head] != '\n' {
                buf.head + 1
            } else {
                buf.head
            };
            let n = text.chars().count();
            buf.text.splice(at..at, text.chars());
            buf.at(at + n - 1);
        }
    }

    /// Delete `start..end`, keeping it in the register.
    fn cut(&mut self, buf: &mut Buffer, start: usize, end: usize, linewise: bool) {
        let end = end.min(buf.text.len());
        self.yank(&buf.text, start, end, linewise);
        buf.text.drain(start..end);
    }

    fn insert_at(&mut self, buf: &mut Buffer, pos: usize) {
        self.mode = Mode::Insert;
        buf.at(pos.min(buf.text.len()));
    }

    fn open_line(&mut self, buf: &mut Buffer, below: bool) {
        let t = &buf.text;
        let indent: String = t[line_start(t, buf.head)..first_non_blank(t, buf.head)]
            .iter()
            .collect();
        let at = if below {
            line_end(t, buf.head)
        } else {
            line_start(t, buf.head)
        };
        let insert = if below {
            format!("\n{indent}")
        } else {
            format!("{indent}\n")
        };
        buf.text.splice(at..at, insert.chars());
        let pos = if below {
            at + 1 + indent.chars().count()
        } else {
            at + indent.chars().count()
        };
        self.insert_at(buf, pos);
    }

    // --- Vim --------------------------------------------------------------------

    /// One more key of a Vim command: `None` while it is still incomplete.
    fn vim(&mut self, buf: &mut Buffer, keys: &str) -> Option<Outcome> {
        let chars: Vec<char> = keys.chars().collect();
        let mut i = 0;
        let count = |i: &mut usize| {
            let start = *i;
            while *i < chars.len()
                && chars[*i].is_ascii_digit()
                && !(*i == start && chars[*i] == '0')
            {
                *i += 1;
            }
            (*i > start).then(|| {
                chars[start..*i]
                    .iter()
                    .collect::<String>()
                    .parse::<usize>()
                    .unwrap_or(1)
            })
        };
        let count1 = count(&mut i);
        let visual = matches!(self.mode, Mode::Visual | Mode::VisualLine);
        let mut op = None;
        if !visual && i < chars.len() && "dcy<>".contains(chars[i]) {
            op = Some(chars[i].to_string());
            i += 1;
        } else if i < chars.len() && chars[i] == 'g' {
            match chars.get(i + 1) {
                None => return None,
                Some('c') if !visual => {
                    op = Some("gc".into());
                    i += 2;
                }
                _ => {}
            }
        }
        let count2 = if op.is_some() { count(&mut i) } else { None };
        let explicit = count1.is_some() || count2.is_some();
        let n = count1.unwrap_or(1) * count2.unwrap_or(1);
        let rest: String = chars[i..].iter().collect();
        if rest.is_empty() {
            return None;
        }
        // The keys that wait for one more: a motion's target char, `gg`, `r`.
        let waiting = matches!(rest.as_str(), "g" | "f" | "t" | "F" | "T" | "r");
        if visual {
            return self.vim_visual(buf, &rest, n, explicit);
        }
        if let Some(op) = op {
            let same = op.chars().last() == rest.chars().next() && rest.chars().count() == 1;
            let range = if same {
                let line = line_start(&buf.text, buf.head);
                let last = line_at(&buf.text, line_number(&buf.text, line) + n - 1);
                Some((line, line_end(&buf.text, last), true))
            } else if let Some(kind) = rest.strip_prefix(['i', 'a']) {
                let mut k = kind.chars();
                let kind = k.next()?;
                text_object(&buf.text, buf.head, rest.starts_with('a'), kind)
                    .map(|(s, e)| (s, e, false))
            } else {
                let rest = if op == "c"
                    && matches!(rest.as_str(), "w" | "W")
                    && buf.head < buf.text.len()
                    && !buf.text[buf.head].is_whitespace()
                {
                    // Vim's own quirk: `cw` changes to the end of the word.
                    rest.replace('w', "e").replace('W', "E")
                } else {
                    rest.clone()
                };
                let Some(motion) = self.vim_motion(buf, &rest, n, explicit) else {
                    // Still typing the motion, or one that does not exist.
                    return (!waiting).then_some(Outcome::Handled);
                };
                match motion {
                    Some((to, kind)) => {
                        let (from, to) = (buf.head.min(to), buf.head.max(to));
                        match kind {
                            Kind::Linewise => Some((from, line_end(&buf.text, to), true)),
                            Kind::Inclusive => Some((from, (to + 1).min(buf.text.len()), false)),
                            Kind::Exclusive => Some((from, to, false)),
                        }
                    }
                    None => return Some(Outcome::Handled),
                }
            };
            let Some((start, end, linewise)) = range else {
                return Some(Outcome::Handled);
            };
            return Some(self.vim_operate(buf, &op, start, end, linewise));
        }
        if waiting && rest != "r" {
            return None;
        }
        if let Some(motion) = self.vim_motion(buf, &rest, n, explicit) {
            if let Some((to, _)) = motion {
                buf.at(clamp_normal(&buf.text, to));
            }
            return Some(Outcome::Handled);
        }
        self.vim_command(buf, &rest, n)
    }

    /// `Some(None)`: a motion that went nowhere; `None` (outer): not a motion,
    /// or one still being typed — told apart by the caller trying commands.
    #[allow(clippy::option_option)]
    fn vim_motion(
        &mut self,
        buf: &Buffer,
        keys: &str,
        n: usize,
        explicit: bool,
    ) -> Option<Option<(usize, Kind)>> {
        let t = &buf.text;
        let p = buf.head;
        let mut chars = keys.chars();
        let first = chars.next()?;
        let second = chars.next();
        let excl = |to: usize| Some(Some((to, Kind::Exclusive)));
        let repeat = |f: &dyn Fn(usize) -> usize| (0..n).fold(p, |at, _| f(at));
        match (first, second) {
            ('h', None) => excl(p.saturating_sub(n).max(line_start(t, p))),
            ('l', None) => excl((p + n).min(line_end(t, p))),
            ('j', None) => Some(Some((vertical(t, p, n, true), Kind::Linewise))),
            ('k', None) => Some(Some((vertical(t, p, n, false), Kind::Linewise))),
            ('w' | 'W', None) => excl(repeat(&|at| word_forward(t, at, first == 'W'))),
            ('b' | 'B', None) => excl(repeat(&|at| word_back(t, at, first == 'B'))),
            ('e' | 'E', None) => Some(Some((
                repeat(&|at| word_end(t, at, first == 'E')),
                Kind::Inclusive,
            ))),
            ('0', None) => excl(line_start(t, p)),
            ('^', None) => excl(first_non_blank(t, p)),
            ('$', None) => Some(Some((
                line_last(t, vertical(t, p, n - 1, true)),
                Kind::Inclusive,
            ))),
            ('G', None) => {
                let line = if explicit {
                    line_at(t, n - 1)
                } else {
                    line_start(t, t.len())
                };
                Some(Some((first_non_blank(t, line), Kind::Linewise)))
            }
            ('g', None) => None,
            ('g', Some('g')) => Some(Some((
                first_non_blank(t, line_at(t, n - 1)),
                Kind::Linewise,
            ))),
            ('}', None) => excl(repeat(&|at| paragraph(t, at, true))),
            ('{', None) => excl(repeat(&|at| paragraph(t, at, false))),
            ('%', None) => Some(match_bracket(t, p).map(|to| (to, Kind::Inclusive))),
            ('f' | 't' | 'F' | 'T', None) => None,
            ('f' | 't' | 'F' | 'T', Some(c)) => {
                let forward = first.is_lowercase();
                let till = matches!(first, 't' | 'T');
                let kind = if forward {
                    Kind::Inclusive
                } else {
                    Kind::Exclusive
                };
                Some(find_in_line(t, p, c, forward, till, n).map(|to| (to, kind)))
            }
            _ => None,
        }
    }

    fn vim_operate(
        &mut self,
        buf: &mut Buffer,
        op: &str,
        start: usize,
        end: usize,
        linewise: bool,
    ) -> Outcome {
        let (start, end) = if linewise {
            whole_lines(&buf.text, start, end.max(start + 1))
        } else {
            (start, end)
        };
        match op {
            "y" => {
                self.yank(&buf.text, start, end, linewise);
                buf.at(clamp_normal(&buf.text, start));
            }
            "d" => {
                self.cut(buf, start, end, linewise);
                // Deleting the last lines leaves the cursor on the one before.
                let at = if linewise && start >= buf.text.len() && start > 0 {
                    line_start(&buf.text, start - 1)
                } else {
                    start
                };
                if linewise
                    && start >= buf.text.len()
                    && start > 0
                    && buf.text.last() == Some(&'\n')
                {
                    buf.text.pop();
                }
                let at = at.min(buf.text.len());
                buf.at(if linewise {
                    first_non_blank(&buf.text, at)
                } else {
                    clamp_normal(&buf.text, at)
                });
            }
            "c" => {
                if linewise {
                    let indent: String = buf.text[start..first_non_blank(&buf.text, start)]
                        .iter()
                        .collect();
                    let keep_newline = end > start && buf.text.get(end - 1) == Some(&'\n');
                    self.cut(buf, start, end, true);
                    let fill = format!("{indent}{}", if keep_newline { "\n" } else { "" });
                    buf.text.splice(start..start, fill.chars());
                    self.insert_at(buf, start + indent.chars().count());
                } else {
                    self.cut(buf, start, end, false);
                    self.insert_at(buf, start);
                }
            }
            ">" | "<" => {
                line_edit(buf, start, end, |text, range| {
                    edit::indent_lines(text, range, op == ">")
                });
                buf.at(first_non_blank(&buf.text, start.min(buf.text.len())));
            }
            "gc" => {
                line_edit(buf, start, end, edit::toggle_line_comments);
                buf.at(clamp_normal(&buf.text, start));
            }
            _ => {}
        }
        Outcome::Handled
    }

    fn vim_command(&mut self, buf: &mut Buffer, keys: &str, n: usize) -> Option<Outcome> {
        let mut chars = keys.chars();
        let first = chars.next()?;
        let second = chars.next();
        let p = buf.head;
        let line_end_here = line_end(&buf.text, p);
        match (first, second) {
            ('x', None) => {
                if p < line_end_here {
                    let end = (p + n).min(line_end_here);
                    self.cut(buf, p, end, false);
                    buf.at(clamp_normal(&buf.text, p));
                }
            }
            ('X', None) => {
                let start = p.saturating_sub(n).max(line_start(&buf.text, p));
                self.cut(buf, start, p, false);
                buf.at(start);
            }
            ('D', None) => {
                self.cut(buf, p, line_end_here, false);
                buf.at(clamp_normal(&buf.text, p));
            }
            ('C', None) => {
                self.cut(buf, p, line_end_here, false);
                self.insert_at(buf, p);
            }
            ('s', None) => {
                let end = (p + n).min(line_end_here);
                self.cut(buf, p, end, false);
                self.insert_at(buf, p);
            }
            ('S', None) => return Some(self.vim_operate(buf, "c", p, p, true)),
            ('Y', None) => {
                let last = line_at(&buf.text, line_number(&buf.text, p) + n - 1);
                return Some(self.vim_operate(buf, "y", p, line_end(&buf.text, last), true));
            }
            ('p' | 'P', None) => self.paste(buf, first == 'p', n),
            ('r', None) => return None,
            ('r', Some(c)) => {
                if p + n <= line_end_here {
                    for i in p..p + n {
                        buf.text[i] = c;
                    }
                    buf.at(p + n - 1);
                }
            }
            ('~', None) => {
                let end = (p + n).min(line_end_here);
                for i in p..end {
                    buf.text[i] = toggle_case(buf.text[i]);
                }
                buf.at(clamp_normal(&buf.text, end));
            }
            ('J', None) => join_lines(buf, p, n.max(2) - 1),
            ('u', None) => return Some(Outcome::Undo),
            ('i', None) => self.insert_at(buf, p),
            ('a', None) => {
                let after = if p < line_end_here { p + 1 } else { p };
                self.insert_at(buf, after);
            }
            ('I', None) => self.insert_at(buf, first_non_blank(&buf.text, p)),
            ('A', None) => self.insert_at(buf, line_end_here),
            ('o' | 'O', None) => self.open_line(buf, first == 'o'),
            ('v', None) => self.mode = Mode::Visual,
            ('V', None) => self.mode = Mode::VisualLine,
            (':', None) => self.mode = Mode::Command(":".into()),
            ('/', None) => self.mode = Mode::Command("/".into()),
            ('n' | 'N', None) => {
                for _ in 0..n {
                    self.search_next(buf, first == 'n');
                }
            }
            _ => {}
        }
        Some(Outcome::Handled)
    }

    fn vim_visual(
        &mut self,
        buf: &mut Buffer,
        keys: &str,
        n: usize,
        explicit: bool,
    ) -> Option<Outcome> {
        let lines = self.mode == Mode::VisualLine;
        let (lo, hi) = (buf.anchor.min(buf.head), buf.anchor.max(buf.head));
        let (start, end) = if lines {
            whole_lines(&buf.text, lo, hi + 1)
        } else {
            (lo, (hi + 1).min(buf.text.len()))
        };
        let done = |state: &mut KeymapState| {
            if state.mode != Mode::Insert {
                state.mode = Mode::Normal;
            }
            Some(Outcome::Handled)
        };
        match keys {
            "o" => {
                std::mem::swap(&mut buf.anchor, &mut buf.head);
                return Some(Outcome::Handled);
            }
            "v" | "V" => {
                let target = if keys == "v" {
                    Mode::Visual
                } else {
                    Mode::VisualLine
                };
                self.mode = if self.mode == target {
                    Mode::Normal
                } else {
                    target
                };
                if self.mode == Mode::Normal {
                    buf.anchor = buf.head;
                }
                return Some(Outcome::Handled);
            }
            "y" => {
                self.yank(&buf.text, start, end, lines);
                buf.at(clamp_normal(&buf.text, start));
                return done(self);
            }
            "d" | "x" => {
                self.vim_operate(buf, "d", start, end, lines);
                return done(self);
            }
            "c" | "s" => {
                self.vim_operate(buf, "c", start, end, lines);
                return done(self);
            }
            ">" | "<" => {
                self.vim_operate(buf, keys, start, end, true);
                return done(self);
            }
            "~" => {
                for i in start..end {
                    buf.text[i] = toggle_case(buf.text[i]);
                }
                buf.at(start);
                return done(self);
            }
            "J" => {
                let count = line_number(&buf.text, hi) - line_number(&buf.text, lo);
                join_lines(buf, lo, count.max(1));
                return done(self);
            }
            "p" | "P" => {
                let register = (self.register.clone(), self.linewise);
                buf.text.drain(start..end);
                buf.at(clamp_normal(&buf.text, start));
                (self.register, self.linewise) = register;
                self.paste(buf, false, 1);
                return done(self);
            }
            ":" => {
                self.mode = Mode::Command(":".into());
                return Some(Outcome::Handled);
            }
            "g" => return None,
            "gc" => {
                line_edit(buf, start, end, edit::toggle_line_comments);
                buf.at(clamp_normal(&buf.text, start));
                return done(self);
            }
            _ => {}
        }
        if let Some(kind) = keys.strip_prefix(['i', 'a']) {
            let kind = kind.chars().next()?;
            if let Some((s, e)) = text_object(&buf.text, buf.head, keys.starts_with('a'), kind) {
                buf.anchor = s;
                buf.head = e.saturating_sub(1).max(s);
            }
            return Some(Outcome::Handled);
        }
        match self.vim_motion(buf, keys, n, explicit) {
            Some(Some((to, _))) => buf.head = clamp_normal(&buf.text, to),
            Some(None) => {}
            // Still typing the motion; anything else is not a visual command.
            None if matches!(keys, "f" | "t" | "F" | "T") => return None,
            None => {}
        }
        Some(Outcome::Handled)
    }

    // --- Helix ------------------------------------------------------------------

    fn helix(&mut self, buf: &mut Buffer, keys: &str) -> Option<Outcome> {
        let t = &buf.text;
        let select = self.mode == Mode::Visual;
        let (lo, hi) = (buf.anchor.min(buf.head), buf.anchor.max(buf.head));
        let end = (hi + 1).min(t.len());
        let mut chars = keys.chars();
        let first = chars.next()?;
        let second = chars.next();
        // A move: from where the selection's head is, extending in select mode.
        let goto = |state: &KeymapState, buf: &mut Buffer, to: usize| {
            buf.head = clamp_normal(&buf.text, to);
            if state.mode != Mode::Visual {
                buf.anchor = buf.head;
            }
        };
        // A word motion selects what it passes over.
        let span = |state: &KeymapState, buf: &mut Buffer, from: usize, to: usize| {
            if state.mode != Mode::Visual {
                buf.anchor = clamp_normal(&buf.text, from);
            }
            buf.head = clamp_normal(&buf.text, to);
        };
        let base = if buf.head != buf.anchor && buf.head > buf.anchor {
            buf.head + 1
        } else {
            buf.head
        };
        match (first, second) {
            ('h', None) => goto(
                self,
                buf,
                buf.head.saturating_sub(1).max(line_start(t, buf.head)),
            ),
            ('l', None) => goto(self, buf, (buf.head + 1).min(line_last(t, buf.head))),
            ('j', None) => goto(self, buf, vertical(t, buf.head, 1, true)),
            ('k', None) => goto(self, buf, vertical(t, buf.head, 1, false)),
            ('w' | 'W', None) => {
                let to = word_forward(t, base, first == 'W');
                span(self, buf, base, to.saturating_sub(1).max(base));
            }
            ('b' | 'B', None) => {
                let from = if buf.head < buf.anchor {
                    buf.head.saturating_sub(1)
                } else {
                    buf.head
                };
                let to = word_back(t, from, first == 'B');
                span(self, buf, from, to);
            }
            ('e' | 'E', None) => {
                let to = word_end(
                    t,
                    base.saturating_sub(1).max(buf.head.min(base)),
                    first == 'E',
                );
                span(self, buf, base.min(to), to);
            }
            ('x', None) => {
                let (s, e) = whole_lines(t, lo, end);
                let full = buf.anchor == s && buf.head + 1 == e;
                let e = if full {
                    whole_lines(t, e.min(t.len()), e + 1).1
                } else {
                    e
                };
                buf.anchor = s;
                buf.head = e.saturating_sub(1).max(s);
            }
            ('%', None) => {
                buf.anchor = 0;
                buf.head = t.len().saturating_sub(1);
            }
            (';', None) => buf.anchor = buf.head,
            ('v', None) => {
                self.mode = if select { Mode::Normal } else { Mode::Visual };
            }
            ('d', None) => {
                self.cut(buf, lo, end, false);
                buf.at(clamp_normal(&buf.text, lo));
                self.mode = Mode::Normal;
            }
            ('c', None) => {
                self.cut(buf, lo, end, false);
                self.insert_at(buf, lo);
            }
            ('y', None) => {
                self.yank(t, lo, end, false);
                self.mode = Mode::Normal;
            }
            ('p' | 'P', None) => {
                let at = if first == 'p' { end } else { lo };
                let text: Vec<char> = self.register.chars().collect();
                if !text.is_empty() {
                    let n = text.len();
                    buf.text.splice(at..at, text);
                    buf.anchor = at;
                    buf.head = at + n - 1;
                }
            }
            ('i', None) => self.insert_at(buf, lo),
            ('a', None) => self.insert_at(buf, end),
            ('I', None) => self.insert_at(buf, first_non_blank(t, lo)),
            ('A', None) => self.insert_at(buf, line_end(t, hi)),
            ('o' | 'O', None) => self.open_line(buf, first == 'o'),
            ('u', None) => return Some(Outcome::Undo),
            ('U', None) => return Some(Outcome::Redo),
            ('r', None) | ('g', None) | ('f' | 't' | 'F' | 'T', None) => return None,
            ('r', Some(c)) => {
                for i in lo..end {
                    if buf.text[i] != '\n' {
                        buf.text[i] = c;
                    }
                }
            }
            ('~', None) => {
                for i in lo..end {
                    buf.text[i] = toggle_case(buf.text[i]);
                }
            }
            ('>' | '<', None) => {
                line_edit(buf, lo, end, |text, range| {
                    edit::indent_lines(text, range, first == '>')
                });
                buf.head = clamp_normal(&buf.text, buf.head);
                buf.anchor = clamp_normal(&buf.text, buf.anchor);
            }
            ('J', None) => {
                let count = line_number(t, hi) - line_number(t, lo);
                join_lines(buf, lo, count.max(1));
            }
            ('g', Some(c)) => {
                let to = match c {
                    'g' => 0,
                    'e' => line_start(t, t.len()),
                    'h' => line_start(t, buf.head),
                    'l' => line_last(t, buf.head),
                    's' => first_non_blank(t, buf.head),
                    _ => return Some(Outcome::Handled),
                };
                goto(self, buf, to);
            }
            ('f' | 't' | 'F' | 'T', Some(c)) => {
                let forward = first.is_lowercase();
                if let Some(to) =
                    find_in_line(t, buf.head, c, forward, matches!(first, 't' | 'T'), 1)
                {
                    span(self, buf, buf.head, to);
                }
            }
            (':', None) => self.mode = Mode::Command(":".into()),
            _ => {}
        }
        Some(Outcome::Handled)
    }

    // --- Emacs ------------------------------------------------------------------

    fn emacs(&mut self, buf: &mut Buffer, input: Input) -> Outcome {
        let kill = self.last_was_kill;
        self.last_was_kill = false;
        let (key, m) = match input {
            Input::Key(key, m) if m.ctrl || m.alt => (key, m),
            Input::Char(_) => {
                // Typing with the mark set types at point, not over a region.
                if self.mark {
                    self.mark = false;
                    buf.anchor = buf.head;
                    return Outcome::Pass;
                }
                return Outcome::Pass;
            }
            _ => return Outcome::Pass,
        };
        let t = &buf.text;
        let p = buf.head;
        let ctrl = m.ctrl && !m.alt;
        let alt = m.alt && !m.ctrl;
        let to = match key {
            Key::F if ctrl => (p + 1).min(t.len()),
            Key::B if ctrl => p.saturating_sub(1),
            Key::N if ctrl => vertical(t, p, 1, true),
            Key::P if ctrl => vertical(t, p, 1, false),
            Key::A if ctrl => line_start(t, p),
            Key::E if ctrl => line_end(t, p),
            Key::F if alt => {
                // forward-word: to the end of the next word.
                let mut i = p;
                while i < t.len() && class(t[i], false) != 1 {
                    i += 1;
                }
                while i < t.len() && class(t[i], false) == 1 {
                    i += 1;
                }
                i
            }
            Key::B if alt => word_back(t, p, false),
            Key::Comma if m.alt && m.shift => 0,
            Key::Period if m.alt && m.shift => t.len(),
            Key::Space if ctrl => {
                self.mark = true;
                buf.anchor = p;
                return Outcome::Handled;
            }
            Key::G if ctrl => {
                self.mark = false;
                buf.anchor = p;
                return Outcome::Handled;
            }
            Key::Slash if ctrl => return Outcome::Undo,
            Key::Minus if m.ctrl && m.shift => return Outcome::Undo,
            Key::D if ctrl => {
                if p < t.len() {
                    buf.text.remove(p);
                }
                buf.at(p);
                return Outcome::Handled;
            }
            Key::D if alt => {
                let mut end = p;
                while end < t.len() && class(t[end], false) != 1 {
                    end += 1;
                }
                while end < t.len() && class(t[end], false) == 1 {
                    end += 1;
                }
                self.kill(buf, p, end, kill, false);
                return Outcome::Handled;
            }
            Key::Backspace if alt => {
                let start = word_back(t, p, false);
                self.kill(buf, start, p, kill, true);
                return Outcome::Handled;
            }
            Key::K if ctrl => {
                let end = line_end(t, p);
                let end = if end == p && p < t.len() { p + 1 } else { end };
                self.kill(buf, p, end, kill, false);
                return Outcome::Handled;
            }
            Key::W if ctrl => {
                if self.mark {
                    let (s, e) = (buf.anchor.min(p), buf.anchor.max(p));
                    self.kill(buf, s, e, false, false);
                    self.mark = false;
                }
                return Outcome::Handled;
            }
            Key::W if alt => {
                if self.mark {
                    let (s, e) = (buf.anchor.min(p), buf.anchor.max(p));
                    self.register = t[s..e].iter().collect();
                    self.mark = false;
                    buf.anchor = p;
                    return Outcome::Clipboard(self.register.clone());
                }
                return Outcome::Handled;
            }
            Key::Y if ctrl => {
                let text: Vec<char> = self.register.chars().collect();
                let n = text.len();
                buf.text.splice(p..p, text);
                buf.at(p + n);
                self.mark = false;
                return Outcome::Handled;
            }
            Key::T if ctrl => {
                // Transpose the chars around point, moving past them.
                let p = if p >= t.len() || t[p] == '\n' {
                    p.saturating_sub(1)
                } else {
                    p
                };
                if p > 0 && p < buf.text.len() {
                    buf.text.swap(p - 1, p);
                    buf.at(p + 1);
                }
                return Outcome::Handled;
            }
            _ => return Outcome::Pass,
        };
        buf.head = to;
        if !self.mark {
            buf.anchor = to;
        }
        Outcome::Handled
    }

    /// Kill `start..end` into the ring; a kill right after a kill adds to it.
    fn kill(&mut self, buf: &mut Buffer, start: usize, end: usize, append: bool, before: bool) {
        let text: String = buf.text[start..end].iter().collect();
        if append {
            if before {
                self.register.insert_str(0, &text);
            } else {
                self.register.push_str(&text);
            }
        } else {
            self.register = text;
        }
        buf.text.drain(start..end);
        buf.at(start);
        self.mark = false;
        self.last_was_kill = true;
    }

    // --- VS Code ----------------------------------------------------------------

    fn vscode(&mut self, buf: &mut Buffer, input: Input) -> Outcome {
        let t = &buf.text;
        let (lo, hi) = (buf.anchor.min(buf.head), buf.anchor.max(buf.head));
        // The lines the selection covers, whole.
        let (ls, le) = whole_lines(t, lo, hi.max(lo + 1));
        match input {
            // With nothing selected, copy and cut take the whole line.
            Input::Copy if lo == hi => Outcome::Clipboard(t[ls..le].iter().collect()),
            Input::Cut if lo == hi => {
                let line: String = t[ls..le].iter().collect();
                buf.text.drain(ls..le);
                buf.at(ls.min(buf.text.len()));
                Outcome::Clipboard(line)
            }
            Input::Key(key @ (Key::ArrowUp | Key::ArrowDown), m) if m.alt && !m.ctrl => {
                let down = key == Key::ArrowDown;
                let block: Vec<char> = t[ls..le].to_vec();
                let block = if block.last() == Some(&'\n') {
                    block
                } else {
                    [block, vec!['\n']].concat()
                };
                if m.shift {
                    // Copy the lines above or below themselves.
                    let at = if down { le.min(t.len()) } else { ls };
                    let fix = if down && (le == t.len() && t.last() != Some(&'\n')) {
                        buf.text.push('\n');
                        1
                    } else {
                        0
                    };
                    let n = block.len();
                    buf.text.splice(at + fix..at + fix, block);
                    let shift = if down { n } else { 0 };
                    buf.anchor += shift;
                    buf.head += shift;
                    return Outcome::Handled;
                }
                let neighbour = if down {
                    next_line(t, le.saturating_sub(1).max(ls))
                } else {
                    prev_line(t, ls)
                };
                let Some(other) = neighbour.filter(|&o| if down { o >= le } else { true }) else {
                    return Outcome::Handled;
                };
                let (os, oe) = whole_lines(t, other, other + 1);
                let other_line: Vec<char> = t[os..oe].to_vec();
                let other_line = if other_line.last() == Some(&'\n') {
                    other_line
                } else {
                    [other_line, vec!['\n']].concat()
                };
                let (start, end, joined, offset) = if down {
                    (
                        ls,
                        oe,
                        [other_line.clone(), block].concat(),
                        other_line.len() as isize,
                    )
                } else {
                    (
                        os,
                        le,
                        [block, other_line.clone()].concat(),
                        -(other_line.len() as isize),
                    )
                };
                let mut joined = joined;
                if end == t.len() && t.last() != Some(&'\n') {
                    joined.pop();
                }
                buf.text.splice(start..end, joined);
                let shift = |x: usize| (x as isize + offset).max(0) as usize;
                buf.anchor = shift(buf.anchor);
                buf.head = shift(buf.head);
                Outcome::Handled
            }
            Input::Key(Key::K, m) if m.ctrl && m.shift => {
                buf.text.drain(ls..le);
                buf.at(ls.min(buf.text.len()));
                Outcome::Handled
            }
            Input::Key(Key::L, m) if m.ctrl && !m.shift => {
                // Select the line; again, the next one too.
                let full = lo == ls && hi == le;
                let end = if full {
                    whole_lines(t, le.min(t.len()), le + 1).1
                } else {
                    le
                };
                buf.anchor = ls;
                buf.head = end;
                Outcome::Handled
            }
            Input::Key(Key::D, m) if m.ctrl && !m.shift => {
                if lo == hi {
                    if let Some((s, e)) =
                        text_object(t, lo.min(t.len().saturating_sub(1)), false, 'w')
                    {
                        buf.anchor = s;
                        buf.head = e;
                    }
                } else {
                    // ponytail: one cursor, so the selection moves to the next
                    // occurrence rather than adding one.
                    let needle: Vec<char> = t[lo..hi].to_vec();
                    let n = t.len();
                    if let Some(i) = (hi..n).chain(0..lo).find(|&i| t[i..].starts_with(&needle)) {
                        buf.anchor = i;
                        buf.head = i + needle.len();
                    }
                }
                Outcome::Handled
            }
            Input::Key(Key::CloseBracket | Key::OpenBracket, m) if m.ctrl && !m.shift => {
                let indent = matches!(input, Input::Key(Key::CloseBracket, _));
                let (a, h) = (buf.anchor, buf.head);
                let mut text = buf.string();
                let range = CCursorRange::two(
                    egui::text::CCursor::new(egui::text::CharIndex(a)),
                    egui::text::CCursor::new(egui::text::CharIndex(h)),
                );
                let range = edit::indent_lines(&mut text, range, indent);
                buf.text = text.chars().collect();
                buf.anchor = range.secondary.index.0;
                buf.head = range.primary.index.0;
                Outcome::Handled
            }
            Input::Key(Key::Backslash, m) if m.ctrl && m.shift => {
                if let Some(to) = match_bracket(t, buf.head)
                    .or_else(|| buf.head.checked_sub(1).and_then(|p| match_bracket(t, p)))
                {
                    buf.at(to);
                }
                Outcome::Handled
            }
            Input::Key(Key::Home, m) if !m.ctrl && !m.alt => {
                // Smart home: to the first non-blank, and from there to the
                // line's start.
                let indent = first_non_blank(t, buf.head);
                let to = if buf.head == indent {
                    line_start(t, buf.head)
                } else {
                    indent
                };
                buf.head = to;
                if !m.shift {
                    buf.anchor = to;
                }
                Outcome::Handled
            }
            _ => Outcome::Pass,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Exclusive,
    Inclusive,
    Linewise,
}

// --- egui -------------------------------------------------------------------

/// The keys `event` is to a keymap, or `None` for one it never sees.
fn input_of(event: &egui::Event) -> Option<Vec<Input>> {
    match event {
        egui::Event::Text(text) => Some(text.chars().map(Input::Char).collect()),
        egui::Event::Key {
            key,
            pressed: true,
            modifiers,
            ..
        } => Some(vec![Input::Key(*key, *modifiers)]),
        egui::Event::Copy => Some(vec![Input::Copy]),
        egui::Event::Cut => Some(vec![Input::Cut]),
        _ => None,
    }
}

/// Run the keymap over this frame's key events, before the `TextEdit` reads
/// them: what it handles is taken out of the input, the rest is left for the
/// editor. Returns an action for the app (`:w`, `:q`) and the status line.
pub(super) fn run(
    ui: &mut egui::Ui,
    editor_id: egui::Id,
    code: &mut String,
    keymap: Keymap,
) -> (Option<EditorAction>, Option<String>, bool) {
    let state_id = editor_id.with("keymap");
    if keymap == Keymap::Codemirror {
        return (None, None, false);
    }
    let mut state: KeymapState = ui
        .data(|d| d.get_temp::<KeymapState>(state_id))
        .filter(|s| s.keymap == keymap)
        .unwrap_or_else(|| KeymapState::new(keymap));
    let focused = ui.memory(|m| m.has_focus(editor_id));
    if !focused {
        let status = state.status();
        let on_char = state.on_char();
        ui.data_mut(|d| d.insert_temp(state_id, state));
        return (None, status, on_char);
    }
    let mut text_state = egui::TextEdit::load_state(ui.ctx(), editor_id).unwrap_or_default();
    let range = text_state
        .cursor
        .char_range()
        .unwrap_or_else(|| CCursorRange::one(egui::text::CCursor::new(egui::text::CharIndex(0))));
    let mut buf = state.buffer_at(code.chars().collect(), range);
    if let Some((written, anchor, head)) = state.written
        && written == range
    {
        buf.anchor = anchor.min(buf.text.len());
        buf.head = head.min(buf.text.len());
    }

    let mut action = None;
    // A click leaves egui's thin cursor; in normal mode it becomes the block.
    let mut touched = state.on_char() && range.is_empty() && !buf.text.is_empty();
    let mut clipboard = None;
    ui.input_mut(|i| {
        let events = std::mem::take(&mut i.events);
        for event in events {
            let Some(inputs) = input_of(&event) else {
                i.events.push(event);
                continue;
            };
            let mut keep = false;
            for input in inputs {
                match state.handle(&mut buf, input) {
                    Outcome::Pass => keep = true,
                    Outcome::Handled => touched = true,
                    Outcome::Clipboard(text) => {
                        clipboard = Some(text);
                        touched = true;
                    }
                    Outcome::Action(a) => action = Some(a),
                    outcome @ (Outcome::Undo | Outcome::Redo) => {
                        let mut undoer = text_state.undoer();
                        let current = (state.to_egui(&buf), buf.string());
                        let restored = if matches!(outcome, Outcome::Undo) {
                            undoer.undo(&current).cloned()
                        } else {
                            undoer.redo(&current).cloned()
                        };
                        text_state.set_undoer(undoer);
                        if let Some((range, text)) = restored {
                            let at = range.primary.index.0;
                            buf.text = text.chars().collect();
                            buf.at(if state.on_char() {
                                clamp_normal(&buf.text, at)
                            } else {
                                at
                            });
                        }
                        touched = true;
                    }
                }
            }
            if keep {
                i.events.push(event);
            }
        }
    });
    if let Some(text) = clipboard {
        ui.ctx().copy_text(text);
    }
    if touched {
        let new_text = buf.string();
        if *code != new_text {
            *code = new_text;
        }
        let range = state.to_egui(&buf);
        text_state.cursor.set_char_range(Some(range));
        text_state.store(ui.ctx(), editor_id);
        state.written = Some((range, buf.anchor, buf.head));
        ui.ctx().request_repaint();
    }
    let status = state.status();
    let on_char = state.on_char();
    ui.data_mut(|d| d.insert_temp(state_id, state));
    (action, status, on_char)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Type `keys` into a fresh `keymap` over `text`, the cursor at `at`.
    /// `<Esc>`, `<CR>`, `<BS>`, `<C-x>` and `<A-x>` name keys.
    fn run_keys(keymap: Keymap, text: &str, at: usize, keys: &str) -> (KeymapState, Buffer) {
        let mut state = KeymapState::new(keymap);
        let mut buf = Buffer::new(text, at);
        let mut rest = keys;
        while let Some(c) = rest.chars().next() {
            let input = if c == '<'
                && let Some(end) = rest.find('>')
            {
                let name = &rest[1..end];
                rest = &rest[end + 1..];
                named_key(name)
            } else {
                rest = &rest[c.len_utf8()..];
                Input::Char(c)
            };
            let typed = match &input {
                Input::Char(c) => Some(*c),
                _ => None,
            };
            if let (Outcome::Pass, Some(c)) = (state.handle(&mut buf, input), typed) {
                // What the TextEdit does with typing the keymap passes on.
                let at = buf.head;
                buf.text.insert(at, c);
                buf.at(at + 1);
            }
        }
        (state, buf)
    }

    fn named_key(name: &str) -> Input {
        let (mods, key) = match name.split_once('-') {
            Some(("C", k)) => (Modifiers::CTRL, k),
            Some(("A", k)) => (Modifiers::ALT, k),
            Some(("AS", k)) => (Modifiers::ALT | Modifiers::SHIFT, k),
            Some(("CS", k)) => (Modifiers::CTRL | Modifiers::SHIFT, k),
            _ => (Modifiers::NONE, name),
        };
        let key = match key {
            "Esc" => Key::Escape,
            "CR" => Key::Enter,
            "BS" => Key::Backspace,
            "Up" => Key::ArrowUp,
            "Down" => Key::ArrowDown,
            "Home" => Key::Home,
            "Space" => Key::Space,
            "/" => Key::Slash,
            "]" => Key::CloseBracket,
            "[" => Key::OpenBracket,
            "\\" => Key::Backslash,
            k => Key::from_name(k).unwrap_or_else(|| panic!("key {k}")),
        };
        Input::Key(key, mods)
    }

    fn vim(text: &str, at: usize, keys: &str) -> (String, usize, Mode) {
        let (state, buf) = run_keys(Keymap::Vim, text, at, keys);
        (buf.string(), buf.head, state.mode)
    }

    #[test]
    fn vim_motions_move_the_cursor() {
        let text = "note(\"c e g\").fast(2)\n  .room(0.5)\n\ns(\"bd\")";
        assert_eq!(vim(text, 0, "w").1, 4);
        assert_eq!(vim(text, 0, "3l").1, 3);
        assert_eq!(vim(text, 0, "$").1, 20);
        assert_eq!(vim(text, 0, "j").1, 22);
        assert_eq!(vim(text, 0, "j^").1, 24);
        assert_eq!(vim(text, 0, "G").1, 36);
        assert_eq!(vim(text, 30, "gg").1, 0);
        assert_eq!(vim(text, 0, "}").1, 35);
        assert_eq!(vim(text, 0, "fe").1, 3);
        assert_eq!(vim(text, 0, "2fe").1, 8);
        assert_eq!(vim(text, 0, "tg").1, 9);
        assert_eq!(vim(text, 4, "%").1, 12);
        assert_eq!(vim(text, 0, "e").1, 3);
        assert_eq!(vim(text, 10, "b").1, 8);
        assert_eq!(vim(text, 0, "/room<CR>").1, 25);
    }

    #[test]
    fn vim_operators_edit_with_motions_and_objects() {
        assert_eq!(vim("one two three", 0, "dw").0, "two three");
        assert_eq!(vim("one two three", 0, "d2w").0, "three");
        assert_eq!(vim("one two three", 0, "cwuno<Esc>").0, "uno two three");
        let (text, _, mode) = vim("one two three", 0, "cw");
        assert_eq!((text.as_str(), mode), (" two three", Mode::Insert));
        assert_eq!(vim("a\nb\nc", 2, "dd").0, "a\nc");
        assert_eq!(vim("a\nb\nc", 4, "dd").0, "a\nb");
        assert_eq!(vim("a\nb\nc", 0, "2dd").0, "c");
        assert_eq!(vim("s(\"bd sd\")", 4, "di\"").0, "s(\"\")");
        assert_eq!(vim("s(\"bd sd\")", 4, "da(").0, "s");
        assert_eq!(vim("hello", 1, "x").0, "hllo");
        assert_eq!(vim("hello", 1, "D").0, "h");
        assert_eq!(vim("a\nb", 0, "yyp").0, "a\na\nb");
        assert_eq!(vim("ab", 0, "ylp").0, "aab");
        assert_eq!(vim("a\nb", 0, "J").0, "a b");
        assert_eq!(vim("abc", 0, "rx").0, "xbc");
        assert_eq!(vim("x", 0, ">>").0, "  x");
        assert_eq!(vim("s(\"bd\")\nn(1)", 0, "gcj").0, "// s(\"bd\")\n// n(1)");
        assert_eq!(vim("s(\"bd\")", 0, "gcc").0, "// s(\"bd\")");
    }

    #[test]
    fn vim_modes_and_visual_selections() {
        let (text, at, mode) = vim("abc", 1, "a");
        assert_eq!((text.as_str(), at, mode), ("abc", 2, Mode::Insert));
        let (_, at, mode) = vim("abc", 1, "a<Esc>");
        assert_eq!((at, mode), (1, Mode::Normal));
        assert_eq!(vim("abc", 0, "o").0, "abc\n");
        assert_eq!(vim("  abc", 2, "O").0, "  \n  abc");
        assert_eq!(vim("one two", 0, "vld").0, "e two");
        assert_eq!(vim("a\nb\nc", 0, "Vjd").0, "c");
        assert_eq!(vim("ab", 0, "v~").0, "Ab");
        let (state, buf) = run_keys(Keymap::Vim, "one two", 0, "viw");
        assert_eq!((buf.anchor, buf.head, state.mode), (0, 2, Mode::Visual));
    }

    #[test]
    fn vim_ex_commands_evaluate_stop_and_jump() {
        let action = |keys: &str| {
            let mut state = KeymapState::new(Keymap::Vim);
            let mut buf = Buffer::new("a\nb\nc", 0);
            let mut last = None;
            for c in keys.chars() {
                let input = if c == '\n' {
                    Input::Key(Key::Enter, Modifiers::NONE)
                } else {
                    Input::Char(c)
                };
                if let Outcome::Action(a) = state.handle(&mut buf, input) {
                    last = Some(a);
                }
            }
            (last, buf.head, state.status())
        };
        assert_eq!(action(":w\n").0, Some(EditorAction::PrimaryEval));
        assert_eq!(action(":q\n").0, Some(EditorAction::Hush));
        assert_eq!(action(":3\n").1, 4);
        assert_eq!(
            action(":nope\n").2.as_deref(),
            Some("Not an editor command: nope")
        );
        assert_eq!(action(":wr").2.as_deref(), Some(":wr"));
        assert_eq!(action("d").2.as_deref(), Some("NORMAL  d"));
        assert_eq!(action("2f").2.as_deref(), Some("NORMAL  2f"));
        // A sequence that means nothing is dropped, not left waiting.
        assert_eq!(action("dz").2.as_deref(), Some("NORMAL"));
    }

    #[test]
    fn helix_selects_then_acts() {
        let helix = |text: &str, at: usize, keys: &str| {
            let (state, buf) = run_keys(Keymap::Helix, text, at, keys);
            (buf.string(), buf.anchor, buf.head, state.mode)
        };
        // `w` selects the word and the space after it; `d` deletes that.
        assert_eq!(
            helix("one two", 0, "w").1..helix("one two", 0, "w").2 + 1,
            0..4
        );
        assert_eq!(helix("one two", 0, "wd").0, "two");
        assert_eq!(helix("a\nb\nc", 0, "xd").0, "b\nc");
        assert_eq!(helix("a\nb\nc", 0, "xxd").0, "c");
        assert_eq!(helix("one", 0, "%d").0, "");
        assert_eq!(helix("one two", 0, "wc").3, Mode::Insert);
        // Helix acts on the selection first: `y` takes "a", then `l` moves on.
        assert_eq!(helix("ab", 0, "ylp").0, "aba");
        assert_eq!(helix("abc", 0, "gl").2, 2);
        assert_eq!(helix("ab", 0, "vl~").0, "AB");
        assert_eq!(helix("s(1)", 0, "<C-C>").0, "// s(1)");
    }

    #[test]
    fn emacs_moves_kills_and_yanks() {
        let emacs = |text: &str, at: usize, keys: &str| {
            let (_, buf) = run_keys(Keymap::Emacs, text, at, keys);
            (buf.string(), buf.anchor, buf.head)
        };
        assert_eq!(emacs("abc\ndef", 0, "<C-E>").2, 3);
        assert_eq!(emacs("abc\ndef", 1, "<C-N>").2, 5);
        assert_eq!(emacs("abc\ndef", 5, "<C-A>").2, 4);
        assert_eq!(emacs("one two", 0, "<A-F>").2, 3);
        assert_eq!(emacs("one two", 0, "<C-K>").0, "");
        // Two kills in a row make one; yank brings it back.
        assert_eq!(emacs("ab\ncd", 0, "<C-K><C-K><C-Y>").0, "ab\ncd");
        assert_eq!(emacs("one two", 0, "<C-Space><A-F><C-W>").0, " two");
        assert_eq!(emacs("one two", 0, "<C-Space><C-F><C-F>").1, 0);
        assert_eq!(emacs("ab", 1, "<C-T>").0, "ba");
        assert_eq!(emacs("abc", 1, "<C-D>").0, "ac");
    }

    #[test]
    fn vscode_moves_copies_and_selects_lines() {
        let vscode = |text: &str, at: usize, keys: &str| {
            let (_, buf) = run_keys(Keymap::Vscode, text, at, keys);
            (buf.string(), buf.anchor, buf.head)
        };
        assert_eq!(vscode("a\nb\nc", 0, "<A-Down>").0, "b\na\nc");
        assert_eq!(vscode("a\nb\nc", 4, "<A-Up>").0, "a\nc\nb");
        assert_eq!(vscode("a\nb", 0, "<AS-Down>").0, "a\na\nb");
        assert_eq!(vscode("a\nb\nc", 2, "<CS-K>").0, "a\nc");
        assert_eq!(
            vscode("ab\ncd", 0, "<C-L>").1..vscode("ab\ncd", 0, "<C-L>").2,
            0..3
        );
        assert_eq!(vscode("ab cd ab", 0, "<C-D>").2, 2);
        assert_eq!(vscode("ab cd ab", 0, "<C-D><C-D>").1, 6);
        assert_eq!(vscode("  x", 3, "<Home>").2, 2);
        assert_eq!(vscode("  x", 2, "<Home>").2, 0);
        assert_eq!(vscode("x", 0, "<C-]>").0, "  x");
        assert_eq!(vscode("f(a)", 1, "<CS-\\>").2, 3);
    }

    #[test]
    fn the_cursor_round_trips_through_egui() {
        // A block cursor is a one-char selection, a visual one covers its end.
        let mut state = KeymapState::new(Keymap::Vim);
        let buf = Buffer::new("abc", 1);
        let range = state.to_egui(&buf);
        assert_eq!(
            range.as_sorted_char_range(),
            egui::text::CharIndex(1)..egui::text::CharIndex(2)
        );
        assert_eq!(state.buffer_at(buf.text.clone(), range).head, 1);
        state.mode = Mode::Insert;
        assert!(state.to_egui(&buf).is_empty());
    }
}
