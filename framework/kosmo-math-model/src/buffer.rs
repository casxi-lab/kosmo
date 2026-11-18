//! The editing buffer of one line, its operations and its snapshots.

use std::fmt;

use crate::parse::{self, Analysis};
use crate::structure::{Fragment, Item, NodeId, Position, Selection, Structure, copy_item};
use crate::version::SourceVersion;
use crate::{Completeness, MAX_INSERT_BYTES, MAX_ITEMS, MAX_PARSE_DEPTH, Token};

/// Whether a character may be typed into a line: an ASCII letter or digit,
/// `_`, `.`, `+`, `-`, `*`, `/`, `^`, `(`, `)` or a space.
pub(crate) const fn in_character_set(c: char) -> bool {
    c.is_ascii_alphanumeric()
        || matches!(c, '_' | '.' | '+' | '-' | '*' | '/' | '^' | '(' | ')' | ' ')
}

/// Why an edit was rejected. A rejected edit changes nothing: not the
/// items, the cursor, the selection or the source version.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Rejected {
    /// The character is not in the character set.
    Character(char),
    /// The text or fragment to insert is longer than `MAX_INSERT_BYTES`.
    InsertTooLong,
    /// The line would hold more than `MAX_ITEMS` items.
    TooManyItems,
    /// The parse depth of the line would exceed `MAX_PARSE_DEPTH`.
    TooDeep,
    /// The fragment does not lie in the snapshot.
    NotInSnapshot,
    /// The source version is at its maximum, so the content cannot change.
    VersionExhausted,
}

impl fmt::Display for Rejected {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Rejected::Character(c) => write!(f, "the character {c:?} cannot be typed"),
            Rejected::InsertTooLong => {
                write!(f, "an insertion holds at most {MAX_INSERT_BYTES} bytes")
            }
            Rejected::TooManyItems => write!(f, "a line holds at most {MAX_ITEMS} items"),
            Rejected::TooDeep => write!(
                f,
                "brackets and unary minus signs nest at most {MAX_PARSE_DEPTH} deep"
            ),
            Rejected::NotInSnapshot => write!(f, "the fragment does not lie in the snapshot"),
            Rejected::VersionExhausted => write!(f, "the source version is at its maximum"),
        }
    }
}

impl std::error::Error for Rejected {}

/// The editing buffer of one line of input: the root block, the cursor, the
/// selection and the source version.
///
/// Every edit is checked before it is applied, and a rejected edit changes
/// nothing. An accepted edit that changes the items increases the source
/// version by exactly 1; cursor and selection commands and edits that leave
/// the items as they were do not change it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditBuffer {
    structure: Structure,
    /// The cursor, an index into the root block.
    cursor: usize,
    /// The anchor of the selection; never equal to the cursor.
    anchor: Option<usize>,
    version: SourceVersion,
}

impl Default for EditBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl EditBuffer {
    /// An empty line with the cursor at its start, no selection and source
    /// version 0.
    pub fn new() -> EditBuffer {
        EditBuffer {
            structure: Structure::new(),
            cursor: 0,
            anchor: None,
            version: SourceVersion::default(),
        }
    }

    /// The identity of the root block, which keeps it for the life of the
    /// buffer.
    pub fn root(&self) -> NodeId {
        self.structure.root()
    }

    /// The items of the line.
    pub fn items(&self) -> &[Item] {
        self.structure.root_items()
    }

    /// The cursor.
    pub fn cursor(&self) -> Position {
        self.at(self.cursor)
    }

    /// The selection, if any.
    pub fn selection(&self) -> Option<Selection> {
        self.anchor
            .map(|anchor| Selection::new(self.at(anchor), self.at(self.cursor)))
    }

    /// The source version.
    pub fn version(&self) -> SourceVersion {
        self.version
    }

    /// Whether the line is complete, incomplete with its holes, or invalid.
    pub fn completeness(&self) -> Completeness {
        self.analysis_of(self.items()).completeness
    }

    /// An immutable copy of the content and the source version, which does
    /// not borrow the buffer.
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            structure: self.structure.clone(),
            version: self.version,
        }
    }

    /// Inserts a character at the cursor, replacing the selection if there is
    /// one, and puts the cursor after it.
    pub fn insert_char(&mut self, c: char) -> Result<(), Rejected> {
        if !in_character_set(c) {
            return Err(Rejected::Character(c));
        }
        self.replace_selection(&[Item::Char(c)])
    }

    /// Inserts text at the cursor, replacing the selection if there is one,
    /// and puts the cursor after it. The text is inserted whole or not at
    /// all: text longer than `MAX_INSERT_BYTES` bytes or with a character
    /// outside the character set is rejected.
    pub fn insert_text(&mut self, text: &str) -> Result<(), Rejected> {
        if text.len() > MAX_INSERT_BYTES {
            return Err(Rejected::InsertTooLong);
        }
        if let Some(c) = text.chars().find(|&c| !in_character_set(c)) {
            return Err(Rejected::Character(c));
        }
        let inserted: Vec<Item> = text.chars().map(Item::Char).collect();
        self.replace_selection(&inserted)
    }

    /// Inserts a copy of a fragment of a snapshot at the cursor, replacing
    /// the selection if there is one, and puts the cursor after it.
    ///
    /// When the fragment is not a single operand (a number, a name, a call
    /// or a bracketed expression) and an operand is missing where it goes,
    /// the copy is put in brackets, so that it stays one operand: the line
    /// `x+1` inserted after `2*` gives `2*(x+1)`. A fragment whose brackets
    /// do not pair is put in brackets too, but the added brackets can then
    /// pair with brackets of the copy or of the line, so the copy need not
    /// stay one operand: `x)*2` inserted after `(3-` gives `(3-(x)*2)`,
    /// which is complete but groups differently from typing the same
    /// characters, `(3-x)*2`.
    ///
    /// The fragment must come from `snapshot`, such as its
    /// [`line`](Snapshot::line) or the range of an error about it: only a
    /// range outside the snapshot is detected, as
    /// [`Rejected::NotInSnapshot`].
    pub fn insert_fragment(
        &mut self,
        snapshot: &Snapshot,
        fragment: Fragment,
    ) -> Result<(), Rejected> {
        let source = snapshot.items_of(fragment).ok_or(Rejected::NotInSnapshot)?;
        // Every character of the character set is one byte.
        if source.len() > MAX_INSERT_BYTES {
            return Err(Rejected::InsertTooLong);
        }
        let mut inserted: Vec<Item> = source.iter().map(copy_item).collect();
        let (items, at) = self.without_selection();
        if parse::needs_grouping(&inserted) && self.analysis_of(&items).needs_operand_at(at) {
            inserted.insert(0, Item::Char('('));
            inserted.push(Item::Char(')'));
        }
        self.insert_into(items, at, &inserted)
    }

    /// Deletes the selection, or else the item before the cursor. At the
    /// start of the line without a selection it changes nothing.
    pub fn delete_backward(&mut self) -> Result<(), Rejected> {
        if self.anchor.is_some() || self.cursor == 0 {
            let (items, at) = self.without_selection();
            return self.commit(items, at);
        }
        let at = self.cursor - 1;
        let mut items = self.items().to_vec();
        if at < items.len() {
            items.remove(at);
        }
        self.commit(items, at)
    }

    /// Deletes the selection, or else the item after the cursor. At the end
    /// of the line without a selection it changes nothing.
    pub fn delete_forward(&mut self) -> Result<(), Rejected> {
        if self.anchor.is_some() {
            let (items, at) = self.without_selection();
            return self.commit(items, at);
        }
        let mut items = self.items().to_vec();
        if self.cursor < items.len() {
            items.remove(self.cursor);
        }
        self.commit(items, self.cursor)
    }

    /// Moves the cursor one item to the left; with a selection, to its start
    /// instead, ending the selection. At the start of the line without a
    /// selection the cursor stays.
    pub fn move_left(&mut self) {
        self.cursor = match self.selected() {
            Some((start, _)) => start,
            None => self.cursor.saturating_sub(1),
        };
        self.anchor = None;
    }

    /// Moves the cursor one item to the right; with a selection, to its end
    /// instead, ending the selection. At the end of the line without a
    /// selection the cursor stays.
    pub fn move_right(&mut self) {
        self.cursor = match self.selected() {
            Some((_, end)) => end,
            None => (self.cursor + 1).min(self.items().len()),
        };
        self.anchor = None;
    }

    /// Moves the cursor to the start of the line and ends the selection.
    pub fn move_to_start(&mut self) {
        self.cursor = 0;
        self.anchor = None;
    }

    /// Moves the cursor to the end of the line and ends the selection.
    pub fn move_to_end(&mut self) {
        self.cursor = self.items().len();
        self.anchor = None;
    }

    /// Moves the active end of the selection one item to the left; without a
    /// selection, starts one at the cursor. At the start of the line it
    /// changes nothing.
    pub fn extend_left(&mut self) {
        if self.cursor > 0 {
            self.extend_to(self.cursor - 1);
        }
    }

    /// Moves the active end of the selection one item to the right; without
    /// a selection, starts one at the cursor. At the end of the line it
    /// changes nothing.
    pub fn extend_right(&mut self) {
        if self.cursor < self.items().len() {
            self.extend_to(self.cursor + 1);
        }
    }

    /// Selects the whole line: the anchor at its start and the cursor at its
    /// end. An empty line has no selection.
    pub fn select_all(&mut self) {
        let end = self.items().len();
        self.anchor = (end > 0).then_some(0);
        self.cursor = end;
    }

    /// Empties the line and puts the cursor at its start. The root keeps its
    /// identity. An empty line stays as it is.
    pub fn clear(&mut self) -> Result<(), Rejected> {
        self.commit(Vec::new(), 0)
    }

    fn at(&self, index: usize) -> Position {
        Position {
            block: self.root(),
            index,
        }
    }

    fn analysis_of(&self, items: &[Item]) -> Analysis {
        parse::analyse(self.root(), items)
    }

    /// The selected item range, if any.
    fn selected(&self) -> Option<(usize, usize)> {
        self.anchor
            .map(|anchor| (anchor.min(self.cursor), anchor.max(self.cursor)))
    }

    /// The items with the selection removed, and the place where it was or,
    /// without a selection, the cursor.
    fn without_selection(&self) -> (Vec<Item>, usize) {
        let mut items = self.items().to_vec();
        match self.selected() {
            Some((start, end)) => {
                if end <= items.len() {
                    items.drain(start..end);
                }
                (items, start)
            }
            None => (items, self.cursor),
        }
    }

    fn extend_to(&mut self, index: usize) {
        let anchor = self.anchor.unwrap_or(self.cursor);
        self.cursor = index;
        self.anchor = (anchor != index).then_some(anchor);
    }

    fn replace_selection(&mut self, inserted: &[Item]) -> Result<(), Rejected> {
        let (items, at) = self.without_selection();
        self.insert_into(items, at, inserted)
    }

    fn insert_into(
        &mut self,
        mut items: Vec<Item>,
        at: usize,
        inserted: &[Item],
    ) -> Result<(), Rejected> {
        // `commit` checks the same limit; this early check only avoids
        // building a line that is too long.
        if items.len() + inserted.len() > MAX_ITEMS {
            return Err(Rejected::TooManyItems);
        }
        let at = at.min(items.len());
        items.splice(at..at, inserted.iter().copied());
        self.commit(items, at + inserted.len())
    }

    /// Makes `items` the content, with the cursor at `cursor` and no
    /// selection, after checking the limits. When the items are the same as
    /// before only the cursor and the selection change.
    fn commit(&mut self, items: Vec<Item>, cursor: usize) -> Result<(), Rejected> {
        if items.as_slice() != self.items() {
            if items.len() > MAX_ITEMS {
                return Err(Rejected::TooManyItems);
            }
            if self.analysis_of(&items).depth > MAX_PARSE_DEPTH {
                return Err(Rejected::TooDeep);
            }
            self.version = self.version.next().ok_or(Rejected::VersionExhausted)?;
            self.structure.set_root_items(items);
        }
        self.cursor = cursor.min(self.items().len());
        self.anchor = None;
        Ok(())
    }
}

/// An immutable copy of a line's content with the source version it had.
///
/// A snapshot does not borrow its buffer, which goes on changing; the
/// snapshot keeps the items, identities and version of the moment it was
/// taken. A computation receives a snapshot, never the buffer, and its result
/// refers to fragments of the snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    structure: Structure,
    version: SourceVersion,
}

impl Snapshot {
    /// The identity of the root block.
    pub fn root(&self) -> NodeId {
        self.structure.root()
    }

    /// The items of the line.
    pub fn items(&self) -> &[Item] {
        self.structure.root_items()
    }

    /// The source version when the snapshot was taken.
    pub fn version(&self) -> SourceVersion {
        self.version
    }

    /// Whether the line is complete, incomplete with its holes, or invalid.
    pub fn completeness(&self) -> Completeness {
        parse::analyse(self.root(), self.items()).completeness
    }

    /// The tokens of the line in reading order, as the parser reads them
    /// to find holes. Spaces separate tokens and belong to none.
    pub fn tokens(&self) -> Vec<Token> {
        parse::tokens_of(self.root(), self.items())
    }

    /// The whole line as a fragment.
    pub fn line(&self) -> Fragment {
        Fragment::between(self.root(), 0, self.items().len())
    }

    /// The items of a fragment, or `None` when it does not lie in the
    /// snapshot. The fragment must come from this snapshot: a fragment of
    /// another snapshot whose range fits gives that range of this one.
    pub fn items_of(&self, fragment: Fragment) -> Option<&[Item]> {
        self.structure.items_of(fragment)
    }
}

#[cfg(test)]
mod tests;
