//! The cases of the layout tree goldens: a tree of rows, columns, scroll
//! containers, text leaves and fixed-size leaves and the constraint of its
//! root, each named by its golden. The unit tests of the layout check
//! their boxes against reference values, and the integration test
//! `tests/golden.rs` compares their layout trees with the goldens, so both
//! include this file: each defines the helpers that it uses, `Case`,
//! `constraint`, `row`, `column`, `scroll`, `text` and `fixed`, and the
//! types of the layout and the tokens, where this file finds them.

use super::*;

pub fn row_spacing() -> Case {
    Case {
        constraint: constraint(0, 0, 200, 100),
        spec: row(
            Row::new().spacing(Space::M),
            vec![fixed(20, 10), fixed(30, 20), fixed(10, 5)],
        ),
    }
}

pub fn row_cross() -> Case {
    let aligned = |align| row(Row::new().align(align), vec![fixed(20, 10), fixed(20, 30)]);
    Case {
        constraint: constraint(0, 0, 300, 200),
        spec: column(
            Column::new().spacing(Space::S),
            vec![
                aligned(RowAlign::Start),
                aligned(RowAlign::Center),
                aligned(RowAlign::End),
            ],
        ),
    }
}

pub fn row_padding() -> Case {
    Case {
        constraint: constraint(0, 0, 200, 100),
        spec: row(
            Row::new().spacing(Space::S).padding(Space::L),
            vec![fixed(10, 10), fixed(10, 10)],
        ),
    }
}

pub fn row_overflow() -> Case {
    Case {
        constraint: constraint(0, 0, 200, 50),
        spec: row(
            Row::new().spacing(Space::M),
            vec![fixed(120, 10), fixed(120, 10)],
        ),
    }
}

pub fn row_tight() -> Case {
    Case {
        constraint: constraint(300, 40, 300, 40),
        spec: row(Row::new(), vec![fixed(20, 10)]),
    }
}

pub fn column_spacing() -> Case {
    Case {
        constraint: constraint(0, 0, 200, 200),
        spec: column(
            Column::new().spacing(Space::M),
            vec![fixed(40, 10), fixed(20, 20), fixed(30, 5)],
        ),
    }
}

pub fn column_cross() -> Case {
    let aligned = |align| {
        column(
            Column::new().align(align),
            vec![fixed(40, 10), fixed(20, 10)],
        )
    };
    Case {
        constraint: constraint(0, 0, 300, 200),
        spec: row(
            Row::new().spacing(Space::L),
            vec![
                aligned(Align::Start),
                aligned(Align::Center),
                aligned(Align::End),
            ],
        ),
    }
}

pub fn column_wrap() -> Case {
    Case {
        constraint: constraint(0, 0, 120, 200),
        spec: column(
            Column::new(),
            vec![text(
                "The result is 1/3 and the error is 0",
                Font::Sans,
                FontSize::Body,
            )],
        ),
    }
}

pub fn column_overflow() -> Case {
    Case {
        constraint: constraint(0, 0, 100, 100),
        spec: column(Column::new(), vec![fixed(10, 80), fixed(10, 80)]),
    }
}

pub fn scroll_y() -> Case {
    Case {
        constraint: constraint(100, 50, 100, 50),
        spec: scroll(Axis::Y, 0, column(Column::new(), vec![fixed(100, 20); 6])),
    }
}

pub fn scroll_y_offset() -> Case {
    Case {
        constraint: constraint(100, 50, 100, 50),
        spec: scroll(Axis::Y, 30, column(Column::new(), vec![fixed(100, 20); 6])),
    }
}

pub fn scroll_x() -> Case {
    Case {
        constraint: constraint(120, 20, 120, 20),
        spec: scroll(
            Axis::X,
            0,
            row(Row::new().spacing(Space::Xs), vec![fixed(50, 20); 4]),
        ),
    }
}

pub fn scroll_cross() -> Case {
    Case {
        constraint: constraint(100, 50, 100, 50),
        spec: scroll(Axis::Y, 0, fixed(150, 200)),
    }
}

pub fn baseline_sizes() -> Case {
    Case {
        constraint: constraint(0, 0, 400, 100),
        spec: row(
            Row::new().align(RowAlign::Baseline).spacing(Space::S),
            vec![
                text("Ag", Font::Sans, FontSize::Large),
                text("Ag", Font::Sans, FontSize::Body),
            ],
        ),
    }
}

pub fn baseline_fonts() -> Case {
    Case {
        constraint: constraint(0, 0, 400, 100),
        spec: row(
            Row::new().align(RowAlign::Baseline),
            vec![
                text("x = ", Font::Mono, FontSize::Body),
                text("1/3", Font::Sans, FontSize::Large),
            ],
        ),
    }
}

pub fn baseline_box() -> Case {
    Case {
        constraint: constraint(0, 0, 400, 100),
        spec: row(
            Row::new().align(RowAlign::Baseline).spacing(Space::S),
            vec![text("Ag", Font::Sans, FontSize::Body), fixed(20, 10)],
        ),
    }
}

pub fn baseline_nested() -> Case {
    Case {
        constraint: constraint(0, 0, 400, 200),
        spec: row(
            Row::new().align(RowAlign::Baseline).spacing(Space::M),
            vec![
                column(
                    Column::new().spacing(Space::Xs),
                    vec![
                        text("1", Font::Sans, FontSize::Body),
                        text("2", Font::Sans, FontSize::Body),
                    ],
                ),
                text("= 3", Font::Sans, FontSize::Large),
            ],
        ),
    }
}

pub fn baseline_lines() -> Case {
    Case {
        constraint: constraint(0, 0, 400, 100),
        spec: row(
            Row::new().align(RowAlign::Baseline).spacing(Space::S),
            vec![
                text("a\nb", Font::Sans, FontSize::Small),
                text("c", Font::Sans, FontSize::Body),
            ],
        ),
    }
}

/// A case with the name of its golden.
pub type Named = (&'static str, fn() -> Case);

/// Every case, with the name of its golden.
pub const CASES: [Named; 18] = [
    ("layout-row-spacing", row_spacing),
    ("layout-row-cross", row_cross),
    ("layout-row-padding", row_padding),
    ("layout-row-overflow", row_overflow),
    ("layout-row-tight", row_tight),
    ("layout-column-spacing", column_spacing),
    ("layout-column-cross", column_cross),
    ("layout-column-wrap", column_wrap),
    ("layout-column-overflow", column_overflow),
    ("layout-scroll-y", scroll_y),
    ("layout-scroll-y-offset", scroll_y_offset),
    ("layout-scroll-x", scroll_x),
    ("layout-scroll-cross", scroll_cross),
    ("layout-baseline-sizes", baseline_sizes),
    ("layout-baseline-fonts", baseline_fonts),
    ("layout-baseline-box", baseline_box),
    ("layout-baseline-nested", baseline_nested),
    ("layout-baseline-lines", baseline_lines),
];
