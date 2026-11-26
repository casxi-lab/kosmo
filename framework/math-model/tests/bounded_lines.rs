//! Every line of up to `MAX_LENGTH` characters over a small alphabet, with
//! numbers, a name, binary operators, unary minus, brackets and spaces. Its
//! completeness agrees with a recursive-descent reading of the grammar
//! written here, apart from the library, and filling each hole of an
//! incomplete line with what the hole misses completes the line.

mod support;

use kosmo_math_model::{Completeness, EditBuffer, HoleKind, Rejected};
use support::{line, text};

/// The characters of the lines.
const ALPHABET: [char; 10] = ['1', 'x', '.', '+', '-', '*', '^', '(', ')', ' '];

/// The longest line; there are 111,111 lines up to this length.
const MAX_LENGTH: usize = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Token {
    /// A run of digits and points, and whether it forms a number.
    Number(bool),
    Name,
    Operator(char),
    Open,
    Close,
}

/// The tokens of a line by the lexical rules: spaces separate, a number is a
/// run of digits and points with at most one point and at least one digit,
/// and a name is a letter or `_` followed by letters, digits and `_`.
fn tokens(line: &[char]) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut rest = line;
    while let Some((&c, tail)) = rest.split_first() {
        if c.is_ascii_digit() || c == '.' {
            let length = rest
                .iter()
                .take_while(|d| d.is_ascii_digit() || **d == '.')
                .count();
            let (run, tail) = rest.split_at(length);
            let points = run.iter().filter(|d| **d == '.').count();
            tokens.push(Token::Number(points <= 1 && points < run.len()));
            rest = tail;
            continue;
        }
        if c.is_ascii_alphabetic() || c == '_' {
            let length = rest
                .iter()
                .take_while(|d| d.is_ascii_alphanumeric() || **d == '_')
                .count();
            tokens.push(Token::Name);
            rest = rest.split_at(length).1;
            continue;
        }
        match c {
            '(' => tokens.push(Token::Open),
            ')' => tokens.push(Token::Close),
            ' ' => {}
            _ => tokens.push(Token::Operator(c)),
        }
        rest = tail;
    }
    tokens
}

/// A recursive-descent reader of the grammar, one function per production.
struct Reader<'a> {
    tokens: &'a [Token],
    next: usize,
}

impl Reader<'_> {
    fn peek(&self) -> Option<Token> {
        self.tokens.get(self.next).copied()
    }

    fn eat(&mut self, token: Token) -> bool {
        let found = self.peek() == Some(token);
        if found {
            self.next += 1;
        }
        found
    }

    /// expression = term , { ( "+" | "-" ) , term } ;
    fn expression(&mut self) -> bool {
        if !self.term() {
            return false;
        }
        while self.eat(Token::Operator('+')) || self.eat(Token::Operator('-')) {
            if !self.term() {
                return false;
            }
        }
        true
    }

    /// term = unary , { ( "*" | "/" ) , unary } ;
    fn term(&mut self) -> bool {
        if !self.unary() {
            return false;
        }
        while self.eat(Token::Operator('*')) || self.eat(Token::Operator('/')) {
            if !self.unary() {
                return false;
            }
        }
        true
    }

    /// unary = "-" , unary | power ;
    fn unary(&mut self) -> bool {
        if self.eat(Token::Operator('-')) {
            self.unary()
        } else {
            self.power()
        }
    }

    /// power = primary , [ "^" , unary ] ;
    fn power(&mut self) -> bool {
        self.primary() && (!self.eat(Token::Operator('^')) || self.unary())
    }

    /// primary = number | name | call | "(" , expression , ")" ; a call is
    /// a name followed by a bracketed expression.
    fn primary(&mut self) -> bool {
        match self.peek() {
            Some(Token::Number(valid)) => {
                self.next += 1;
                valid
            }
            Some(Token::Name) => {
                self.next += 1;
                !self.eat(Token::Open) || (self.expression() && self.eat(Token::Close))
            }
            Some(Token::Open) => {
                self.next += 1;
                self.expression() && self.eat(Token::Close)
            }
            _ => false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Class {
    Complete,
    Incomplete,
    Invalid,
}

/// The class of a line: invalid with a run that forms no number or a right
/// bracket without a left one before it; otherwise complete when the reader
/// reads all of it and incomplete when it does not.
fn expected_class(line: &[char]) -> Class {
    let tokens = tokens(line);
    if tokens.contains(&Token::Number(false)) {
        return Class::Invalid;
    }
    let mut open = 0usize;
    for token in &tokens {
        match token {
            Token::Open => open += 1,
            Token::Close if open == 0 => return Class::Invalid,
            Token::Close => open -= 1,
            _ => {}
        }
    }
    let mut reader = Reader {
        tokens: &tokens,
        next: 0,
    };
    if reader.expression() && reader.next == tokens.len() {
        Class::Complete
    } else {
        Class::Incomplete
    }
}

fn class_of(completeness: &Completeness) -> Class {
    match completeness {
        Completeness::Complete => Class::Complete,
        Completeness::Incomplete(_) => Class::Incomplete,
        Completeness::Invalid(_) => Class::Invalid,
    }
}

/// Every line over the alphabet with up to `MAX_LENGTH` characters.
fn every_line() -> Vec<Vec<char>> {
    let mut lines = vec![Vec::new()];
    let mut previous = vec![Vec::new()];
    for _ in 0..MAX_LENGTH {
        let longer: Vec<Vec<char>> = previous
            .iter()
            .flat_map(|line: &Vec<char>| {
                ALPHABET.iter().map(move |&c| {
                    let mut longer = line.clone();
                    longer.push(c);
                    longer
                })
            })
            .collect();
        lines.extend(longer.iter().cloned());
        previous = longer;
    }
    lines
}

/// The line with each hole filled with what it misses: an operand `1`, the
/// operator `*` or a right bracket. The holes are filled from the last, so
/// that the places of the earlier ones stay as they are.
fn fill_holes(buffer: &EditBuffer) -> Result<Option<EditBuffer>, Rejected> {
    let Completeness::Incomplete(holes) = buffer.completeness() else {
        return Ok(None);
    };
    let mut filled = buffer.clone();
    for hole in holes.iter().rev() {
        let filling = match hole.kind {
            HoleKind::EmptyLine | HoleKind::MissingOperand => '1',
            HoleKind::MissingOperator => '*',
            HoleKind::MissingRightBracket => ')',
            _ => return Ok(None),
        };
        filled.move_to_start();
        for _ in 0..hole.position.index {
            filled.move_right();
        }
        filled.insert_char(filling)?;
    }
    Ok(Some(filled))
}

#[test]
fn every_short_line_is_classified_as_the_grammar_reads_it() -> Result<(), Rejected> {
    let lines = every_line();
    assert_eq!(lines.len(), 111_111);
    for chars in &lines {
        let written: String = chars.iter().collect();
        let buffer = line(&written)?;
        assert_eq!(
            class_of(&buffer.completeness()),
            expected_class(chars),
            "{written:?}: {:?}",
            buffer.completeness()
        );
    }
    Ok(())
}

#[test]
fn filling_every_hole_completes_the_line() -> Result<(), Rejected> {
    let mut incomplete = 0;
    for chars in every_line() {
        let written: String = chars.iter().collect();
        let buffer = line(&written)?;
        let Some(filled) = fill_holes(&buffer)? else {
            continue;
        };
        incomplete += 1;
        assert_eq!(
            filled.completeness(),
            Completeness::Complete,
            "{written:?} filled as {:?}",
            text(filled.items())
        );
    }
    assert!(incomplete > 10_000, "{incomplete}");
    Ok(())
}
