//! The line syntax of the terminal.

use kosmo_sdk::kosmo_jobs::Operation;

/// What a line asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Command<'a> {
    /// Submit `expression` with `operation` and `variable`: a line that does
    /// not start with `:` is an expression to evaluate with an empty
    /// variable, and `:expand` and `:diff` name their variable.
    Submit {
        /// The operation of the request.
        operation: Operation,
        /// The variable of the request, as it was written.
        variable: &'a str,
        /// The bytes to enter into the current line.
        expression: &'a [u8],
    },
    /// `:history`: list the history.
    History,
    /// `:quit`: end the terminal.
    Quit,
    /// A command word that the syntax does not have.
    Unknown,
    /// A command word of the syntax whose fields do not fit it.
    Malformed,
}

/// The command of `line`, a line without its line end.
///
/// A line that starts with `:` is a command, and any other line, an empty
/// one included, is an expression: `:` is outside the character set and the
/// grammar of kalyx, so the two never mix. The command word runs to the
/// first space or the end of the line. `:expand` and `:diff` take a
/// variable and an expression, each after exactly one space: the variable is
/// a run of bytes from 0x21 to 0x7E, passed on as it is for kalyx to judge,
/// and the expression is all the bytes after it, spaces included.
/// `:history` and `:quit` take nothing, not even a space.
pub(crate) fn parse(line: &[u8]) -> Command<'_> {
    let Some(command) = line.strip_prefix(b":") else {
        return Command::Submit {
            operation: Operation::Eval,
            variable: "",
            expression: line,
        };
    };
    let (word, fields) = match command.iter().position(|&byte| byte == b' ') {
        Some(at) => (&command[..at], Some(&command[at + 1..])),
        None => (command, None),
    };
    let operation = match (word, fields) {
        (b"history", None) => return Command::History,
        (b"quit", None) => return Command::Quit,
        (b"history" | b"quit", Some(_)) => return Command::Malformed,
        (b"expand", _) => Operation::Expand,
        (b"diff", _) => Operation::Diff,
        _ => return Command::Unknown,
    };
    match fields.and_then(variable_and_expression) {
        Some((variable, expression)) => Command::Submit {
            operation,
            variable,
            expression,
        },
        None => Command::Malformed,
    }
}

/// The variable and the expression of the fields of `:expand` or `:diff`.
fn variable_and_expression(fields: &[u8]) -> Option<(&str, &[u8])> {
    let at = fields.iter().position(|&byte| byte == b' ')?;
    let (variable, expression) = (&fields[..at], &fields[at + 1..]);
    if variable.is_empty() || !variable.iter().all(|byte| (0x21..=0x7E).contains(byte)) {
        return None;
    }
    let variable = std::str::from_utf8(variable).ok()?;
    Some((variable, expression))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn submit<'a>(operation: Operation, variable: &'a str, expression: &'a [u8]) -> Command<'a> {
        Command::Submit {
            operation,
            variable,
            expression,
        }
    }

    #[test]
    fn each_form_of_the_syntax_gives_its_command() {
        assert_eq!(parse(b"1/3+1/6"), submit(Operation::Eval, "", b"1/3+1/6"));
        assert_eq!(
            parse(b":expand x (x+1)^2"),
            submit(Operation::Expand, "x", b"(x+1)^2")
        );
        assert_eq!(
            parse(b":diff y 2*y^3"),
            submit(Operation::Diff, "y", b"2*y^3")
        );
        assert_eq!(parse(b":history"), Command::History);
        assert_eq!(parse(b":quit"), Command::Quit);
    }

    #[test]
    fn every_line_without_a_colon_first_is_an_expression() {
        for line in [&b""[..], b"   ", b"diff x x^2", b" :quit", b"x\xc2\xb2"] {
            assert_eq!(parse(line), submit(Operation::Eval, "", line));
        }
    }

    #[test]
    fn the_expression_keeps_its_spaces_and_may_be_empty() {
        assert_eq!(
            parse(b":diff x  x ^ 2 "),
            submit(Operation::Diff, "x", b" x ^ 2 ")
        );
        assert_eq!(parse(b":expand x "), submit(Operation::Expand, "x", b""));
        assert_eq!(
            parse(b":diff pi x"),
            submit(Operation::Diff, "pi", b"x"),
            "kalyx judges the variable"
        );
        assert_eq!(parse(b":diff 2 x"), submit(Operation::Diff, "2", b"x"));
    }

    #[test]
    fn command_words_outside_the_syntax_are_unknown() {
        for line in [
            &b":"[..],
            b": quit",
            b":dif x x^2",
            b":eval 1",
            b":QUIT",
            b"::",
        ] {
            assert_eq!(parse(line), Command::Unknown, "{line:?}");
        }
    }

    #[test]
    fn fields_that_do_not_fit_are_malformed() {
        for line in [
            &b":diff"[..],
            b":diff x",
            b":diff ",
            b":diff  x x^2",
            b":expand x\tx",
            b":diff \xce\xb1 x^2",
            b":diff \x1bx x^2",
            b":diff x\x7f x^2",
            b":history 1",
            b":history ",
            b":quit now",
        ] {
            assert_eq!(parse(line), Command::Malformed, "{line:?}");
        }
    }
}
