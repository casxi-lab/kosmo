//! Random trees under random constraints from fixed seeds. Each group lays
//! out a random tree of rows, columns, scroll containers, text leaves and
//! fixed-size leaves under a random root constraint, twice, with one font
//! set for all groups. One group in `OUT_OF_RANGE_EVERY` has one input out
//! of its range: a side of the root constraint or of a fixed-size leaf one
//! unit beyond `MAX_LAYOUT_INPUT`, a root constraint whose smallest size
//! exceeds its largest, or a scroll request one unit beyond its range.
//!
//! Nothing panics. A group out of range gives
//! `LayoutError::InputOutOfRange` and no tree. In every other group, every
//! box is within its constraint; every child lies in its parent's content,
//! the parent less its padding, and where the parent overflows along an
//! axis, the child starts no earlier than the content and ends within the
//! parent's natural size less its padding; a box marks overflow exactly
//! where its natural size exceeds its largest size; the child of a scroll
//! container lies within the content's range along the axis and within
//! the viewport across it, and the offset is the request brought into 0 to
//! the content's length less the viewport's; and the two layouts write the
//! same text. A failure names the seed and the group.

use std::panic::{AssertUnwindSafe, catch_unwind};

use kosmo_text::FontSet;
use kosmo_types::{Fx, Size};

use super::{Part, Spec, build, fonts};
use crate::hit::LayoutError;
use crate::layout::{
    Align, Axis, Column, Constraint, Font, LayoutTree, MAX_LAYOUT_INPUT, Overflow, Row, RowAlign,
    TreeBox, layout,
};
use crate::random::SplitMix64;
use crate::tokens::{ColorToken, FontSize, Space, TOKENS};

/// The seed of the first group; group `n` uses `SEED + n`.
const SEED: u64 = 0x6b6f_736d_6f00_0303;

/// The number of groups.
const GROUPS: u64 = 1_000;

/// One group in this many, the last of each run of this many, has an input
/// out of its range.
const OUT_OF_RANGE_EVERY: u64 = 16;

/// The most nodes of a tree.
const MAX_NODES: usize = 48;

/// The greatest depth of a tree, the root counted as 1.
const MAX_DEPTH: usize = 12;

/// The most text leaves of a tree.
const MAX_TEXTS: usize = 8;

/// The most characters of the text of a leaf.
const MAX_CHARACTERS: usize = 24;

/// The operators and arrows of the interface strings in Fira Sans.
const SANS_OPERATORS: &str = "−∕√∞∑∫∂∆≈≠≤≥⋅←↑→↓";

/// The operators and arrows of the interface strings in Fira Mono.
const MONO_OPERATORS: &str = "−∕√∞∑∫∂≈≠≤≥←↑→↓";

/// The characters of the interface strings that the texts of `font` take
/// their fragments from: printable ASCII, Latin-1, Greek letters, and the
/// operators and arrows of the font.
fn character_sets(font: Font) -> [Vec<char>; 4] {
    let range = |from: u32, to: u32| (from..=to).filter_map(char::from_u32).collect::<Vec<_>>();
    let mut greek = range(0x391, 0x3a9);
    greek.retain(|&character| character != '\u{3a2}');
    greek.extend(range(0x3b1, 0x3c9));
    let operators = match font {
        Font::Sans => SANS_OPERATORS,
        Font::Mono => MONO_OPERATORS,
    };
    [
        range(0x20, 0x7e),
        range(0xa0, 0xff),
        greek,
        operators.chars().collect(),
    ]
}

/// A node of a random tree, before it becomes a [`Spec`].
struct Draft {
    part: Part,
    depth: usize,
    children: Vec<usize>,
}

/// Which input of a group is out of its range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Beyond {
    RootSide,
    MinAboveMax,
    LeafSide,
    Request,
}

/// The values that a side of a fixed-size leaf or of a constraint takes:
/// zero, one unit, up to 64 pixels, from 64 pixels to `MAX_LAYOUT_INPUT`,
/// or exactly `MAX_LAYOUT_INPUT`.
fn side(rng: &mut SplitMix64) -> Fx {
    let limit = MAX_LAYOUT_INPUT.units();
    Fx::from_units(match rng.below(5) {
        0 => 0,
        1 => 1,
        2 => rng.below(64 * 64 + 1) as i32,
        3 => 64 * 64 + rng.below((limit - 64 * 64 + 1) as usize) as i32,
        _ => limit,
    })
}

/// A scroll request: zero, either end of its range, or any value in it.
fn request(rng: &mut SplitMix64) -> Fx {
    let limit = MAX_LAYOUT_INPUT.units();
    Fx::from_units(match rng.below(4) {
        0 => 0,
        1 => -limit,
        2 => limit,
        _ => rng.below(2 * limit as usize + 1) as i32 - limit,
    })
}

/// A spacing token, or none.
fn space(rng: &mut SplitMix64) -> Option<Space> {
    [
        None,
        Some(Space::Xs),
        Some(Space::S),
        Some(Space::M),
        Some(Space::L),
    ][rng.below(5)]
}

/// A fragment of at most `MAX_CHARACTERS` characters of the interface
/// strings of `font`, now and then with a line break in it.
fn fragment(rng: &mut SplitMix64, font: Font) -> String {
    let sets = character_sets(font);
    let set = &sets[rng.below(sets.len())];
    let length = rng.below(MAX_CHARACTERS + 1);
    let start = rng.below(set.len());
    let mut characters: Vec<char> = (0..length)
        .map(|offset| set[(start + offset) % set.len()])
        .collect();
    if length > 0 && rng.below(4) == 0 {
        characters[rng.below(length)] = '\n';
    }
    characters.into_iter().collect()
}

/// A random node: a row 20%, a column 20%, a scroll container 10%, a text
/// leaf 25% while the tree has fewer than `MAX_TEXTS`, and otherwise a
/// fixed-size leaf.
fn part(rng: &mut SplitMix64, texts: &mut usize) -> Part {
    match rng.below(100) {
        0..20 => {
            let align = [
                RowAlign::Start,
                RowAlign::Center,
                RowAlign::End,
                RowAlign::Baseline,
            ][rng.below(4)];
            let mut row = Row::new().align(align);
            row.spacing = space(rng);
            row.padding = space(rng);
            Part::Row(row)
        }
        20..40 => {
            let align = [Align::Start, Align::Center, Align::End][rng.below(3)];
            let mut column = Column::new().align(align);
            column.spacing = space(rng);
            column.padding = space(rng);
            Part::Column(column)
        }
        40..50 => Part::Scroll([Axis::X, Axis::Y][rng.below(2)], request(rng)),
        50..75 if *texts < MAX_TEXTS => {
            *texts += 1;
            let font = [Font::Sans, Font::Mono][rng.below(2)];
            let size = [FontSize::Small, FontSize::Body, FontSize::Large][rng.below(3)];
            Part::Text(fragment(rng, font), font, size, ColorToken::Text)
        }
        _ => Part::Fixed(Size::new(side(rng), side(rng)).unwrap()),
    }
}

/// A random tree of 1 to `MAX_NODES` nodes, at most `MAX_DEPTH` deep: each
/// node after the root goes under a random row, column or scroll container
/// that can take it, a scroll container only while it has no child.
fn tree(rng: &mut SplitMix64) -> Vec<Draft> {
    let count = 1 + rng.below(MAX_NODES);
    let mut texts = 0;
    let mut drafts = vec![Draft {
        part: part(rng, &mut texts),
        depth: 1,
        children: Vec::new(),
    }];
    for _ in 1..count {
        let open: Vec<usize> = (0..drafts.len())
            .filter(|&index| {
                let draft = &drafts[index];
                draft.depth < MAX_DEPTH
                    && match draft.part {
                        Part::Row(_) | Part::Column(_) => true,
                        Part::Scroll(..) => draft.children.is_empty(),
                        Part::Text(..) | Part::Fixed(_) => false,
                    }
            })
            .collect();
        if open.is_empty() {
            break;
        }
        let parent = open[rng.below(open.len())];
        let depth = drafts[parent].depth + 1;
        let index = drafts.len();
        drafts.push(Draft {
            part: part(rng, &mut texts),
            depth,
            children: Vec::new(),
        });
        drafts[parent].children.push(index);
    }
    drafts
}

/// A random root constraint: along each axis tight, zero, up to exactly
/// `MAX_LAYOUT_INPUT`, from zero, or between two values.
fn root(rng: &mut SplitMix64) -> Constraint {
    let mut axis = || {
        let (min, max) = match rng.below(5) {
            0 => {
                let value = side(rng);
                (value, value)
            }
            1 => (Fx::ZERO, Fx::ZERO),
            2 => (side(rng), MAX_LAYOUT_INPUT),
            3 => (Fx::ZERO, side(rng)),
            _ => (side(rng), side(rng)),
        };
        (min.min(max), max.max(min))
    };
    let (min_width, max_width) = axis();
    let (min_height, max_height) = axis();
    Constraint::new(
        Size::new(min_width, min_height).unwrap(),
        Size::new(max_width, max_height).unwrap(),
    )
}

/// The indices of the drafts that `wanted` picks.
fn indices(drafts: &[Draft], wanted: fn(&Part) -> bool) -> Vec<usize> {
    (0..drafts.len())
        .filter(|&index| wanted(&drafts[index].part))
        .collect()
}

/// Puts one input of `drafts` or `constraint` out of its range, and says
/// which. Without a fixed-size leaf or a scroll container to change, a
/// side of the root constraint goes beyond its range instead.
fn beyond(rng: &mut SplitMix64, drafts: &mut [Draft], constraint: &mut Constraint) -> Beyond {
    let past = Fx::from_units(MAX_LAYOUT_INPUT.units() + 1);
    let wanted = rng.below(4);
    let leaves = indices(drafts, |part| matches!(part, Part::Fixed(_)));
    if wanted == 2 && !leaves.is_empty() {
        let index = leaves[rng.below(leaves.len())];
        if let Part::Fixed(size) = &mut drafts[index].part {
            *size = if rng.below(2) == 0 {
                Size::new(past, size.height()).unwrap()
            } else {
                Size::new(size.width(), past).unwrap()
            };
        }
        return Beyond::LeafSide;
    }
    let scrolls = indices(drafts, |part| matches!(part, Part::Scroll(..)));
    if wanted == 3 && !scrolls.is_empty() {
        let index = scrolls[rng.below(scrolls.len())];
        if let Part::Scroll(_, request) = &mut drafts[index].part {
            *request = if rng.below(2) == 0 {
                past
            } else {
                Fx::from_units(-past.units())
            };
        }
        return Beyond::Request;
    }
    let (min, max) = (constraint.min, constraint.max);
    if wanted == 1 {
        // A largest size below the limit, and a smallest one unit above it.
        let top = Fx::from_units(rng.below(MAX_LAYOUT_INPUT.units() as usize) as i32);
        let above = Fx::from_units(top.units() + 1);
        *constraint = if rng.below(2) == 0 {
            Constraint::new(
                Size::new(above, min.height()).unwrap(),
                Size::new(top, max.height()).unwrap(),
            )
        } else {
            Constraint::new(
                Size::new(min.width(), above).unwrap(),
                Size::new(max.width(), top).unwrap(),
            )
        };
        return Beyond::MinAboveMax;
    }
    constraint.max = if rng.below(2) == 0 {
        Size::new(past, max.height()).unwrap()
    } else {
        Size::new(max.width(), past).unwrap()
    };
    Beyond::RootSide
}

/// The spec of the draft `index` and its subtree.
fn spec(drafts: &[Draft], index: usize) -> Spec {
    let draft = &drafts[index];
    Spec {
        part: draft.part.clone(),
        children: draft
            .children
            .iter()
            .map(|&child| spec(drafts, child))
            .collect(),
    }
}

/// The parts of `spec` and its subtree, in preorder.
fn preorder<'s>(spec: &'s Spec, out: &mut Vec<&'s Part>) {
    out.push(&spec.part);
    for child in &spec.children {
        preorder(child, out);
    }
}

/// Lays `spec` out under `constraint`, catching a panic.
fn attempt(
    spec: &Spec,
    constraint: Constraint,
    fonts: &mut FontSet,
) -> Result<Result<LayoutTree, LayoutError>, String> {
    let tree = build(spec);
    catch_unwind(AssertUnwindSafe(|| {
        layout(&tree, constraint, &TOKENS, fonts)
    }))
    .map_err(|_| "the layout panicked".to_string())
}

/// `x` and `y` along `axis` and across it, in units.
fn oriented(x: Fx, y: Fx, axis: Axis) -> (i64, i64) {
    let (x, y) = (i64::from(x.units()), i64::from(y.units()));
    match axis {
        Axis::X => (x, y),
        Axis::Y => (y, x),
    }
}

/// The length of `size` along `axis` and across it, in units.
fn lengths(size: Size, axis: Axis) -> (i64, i64) {
    oriented(size.width(), size.height(), axis)
}

/// The overflow of a box of natural size `natural`, its width and height
/// in units, under `constraint`.
fn expected_overflow(natural: (i64, i64), constraint: Constraint) -> Overflow {
    Overflow {
        x: natural.0 > i64::from(constraint.max.width().units()),
        y: natural.1 > i64::from(constraint.max.height().units()),
    }
}

/// Checks that each box is within its constraint, that each child lies
/// in its parent's content, that overflow is marked exactly where a
/// natural size exceeds the largest size, and that each scroll offset is
/// the request brought into its range. `parts` are the nodes of the boxes.
fn check(tree: &LayoutTree, parts: &[&Part]) -> Result<(), String> {
    let boxes = tree.boxes();
    if boxes.len() != parts.len() {
        return Err(format!("{} boxes for {} nodes", boxes.len(), parts.len()));
    }
    let mut children: Vec<Vec<&TreeBox>> = vec![Vec::new(); boxes.len()];
    for laid in boxes {
        if let Some(parent) = laid.parent {
            children[parent].push(laid);
        }
    }
    for (index, (laid, part)) in boxes.iter().zip(parts).enumerate() {
        let (constraint, size) = (laid.constraint, laid.size);
        let within = constraint.min.width() <= size.width()
            && size.width() <= constraint.max.width()
            && constraint.min.height() <= size.height()
            && size.height() <= constraint.max.height();
        if !within {
            return Err(format!("box {index} is outside its constraint"));
        }
        let kids = &children[index];
        let checked = match part {
            Part::Row(row) => {
                let by_baseline = row.align == RowAlign::Baseline;
                check_lane(laid, kids, Axis::X, row.spacing, row.padding, by_baseline)
            }
            Part::Column(column) => {
                check_lane(laid, kids, Axis::Y, column.spacing, column.padding, false)
            }
            Part::Scroll(axis, request) => check_scroll(laid, kids, *axis, *request),
            Part::Fixed(natural) => {
                let natural = lengths(*natural, Axis::X);
                (laid.overflow == expected_overflow(natural, constraint))
                    .then_some(())
                    .ok_or_else(|| "wrong overflow".to_string())
            }
            Part::Text(..) => match &laid.text {
                Some(text) => {
                    let paragraph = &text.paragraph;
                    let natural = oriented(paragraph.width(), paragraph.height(), Axis::X);
                    (laid.overflow == expected_overflow(natural, constraint))
                        .then_some(())
                        .ok_or_else(|| "wrong overflow".to_string())
                }
                None => Err("no paragraph".to_string()),
            },
        };
        checked.map_err(|why| format!("box {index}: {why}"))?;
    }
    Ok(())
}

/// Checks a row or column `laid` and its children `kids`.
fn check_lane(
    laid: &TreeBox,
    kids: &[&TreeBox],
    axis: Axis,
    spacing: Option<Space>,
    padding: Option<Space>,
    by_baseline: bool,
) -> Result<(), String> {
    let units =
        |space: Option<Space>| space.map_or(0, |space| i64::from(TOKENS.space(space).units()));
    let (spacing, padding) = (units(spacing), units(padding));
    // The natural size: the children with the spacing and padding.
    let mut along = 2 * padding;
    for (number, kid) in kids.iter().enumerate() {
        along += lengths(kid.size, axis).0 + if number > 0 { spacing } else { 0 };
    }
    let content = if by_baseline {
        let (mut above, mut below) = (0, 0);
        for kid in kids {
            let height = lengths(kid.size, axis).1;
            let baseline = kid
                .baseline
                .map_or(height, |baseline| i64::from(baseline.units()));
            above = above.max(baseline);
            below = below.max(height - baseline);
        }
        above + below
    } else {
        kids.iter()
            .map(|kid| lengths(kid.size, axis).1)
            .max()
            .unwrap_or(0)
    };
    let across = content + 2 * padding;
    let natural = match axis {
        Axis::X => (along, across),
        Axis::Y => (across, along),
    };
    let overflow = expected_overflow(natural, laid.constraint);
    if laid.overflow != overflow {
        return Err(format!(
            "overflow {:?} where the natural size {natural:?} gives {overflow:?}",
            laid.overflow
        ));
    }
    // Each child within the content, or within the natural size less the
    // padding along an axis that overflows.
    let size = lengths(laid.size, Axis::X);
    let limit = (
        if overflow.x { natural.0 } else { size.0 } - padding,
        if overflow.y { natural.1 } else { size.1 } - padding,
    );
    for kid in kids {
        let (x, y) = oriented(kid.position.x, kid.position.y, Axis::X);
        let (width, height) = lengths(kid.size, Axis::X);
        if x < padding || y < padding || x + width > limit.0 || y + height > limit.1 {
            return Err(format!(
                "a child at ({x}, {y}) of size ({width}, {height}) is outside the content \
                 from ({padding}, {padding}) to {limit:?}"
            ));
        }
    }
    Ok(())
}

/// Checks a scroll container `laid`, its children `kids` and its offset.
fn check_scroll(laid: &TreeBox, kids: &[&TreeBox], axis: Axis, request: Fx) -> Result<(), String> {
    if laid.overflow != Overflow::default() {
        return Err("a scroll container marks overflow".to_string());
    }
    let (viewport, across) = lengths(laid.size, axis);
    let content = match kids {
        [] => 0,
        [kid] => {
            let (start, cross_start) = oriented(kid.position.x, kid.position.y, axis);
            let (length, cross_length) = lengths(kid.size, axis);
            if start != 0 || cross_start < 0 || cross_start + cross_length > across {
                return Err("the content is outside its range or the viewport".to_string());
            }
            length
        }
        _ => return Err("more than one child".to_string()),
    };
    let (offset, cross_offset) = oriented(laid.scroll.x, laid.scroll.y, axis);
    let range = (content - viewport).max(0);
    let wanted = i64::from(request.units()).clamp(0, range);
    if cross_offset != 0 || offset < 0 || offset > range || offset != wanted {
        return Err(format!(
            "offset {offset} across {cross_offset} for the request {} in 0 to {range}",
            request.units()
        ));
    }
    Ok(())
}

/// Runs group `number`, and says which input it put out of its range.
fn group(number: u64, fonts: &mut FontSet) -> Result<Option<Beyond>, String> {
    let mut rng = SplitMix64::new(SEED + number);
    let mut drafts = tree(&mut rng);
    let mut constraint = root(&mut rng);
    let out_of_range = (number % OUT_OF_RANGE_EVERY == OUT_OF_RANGE_EVERY - 1)
        .then(|| beyond(&mut rng, &mut drafts, &mut constraint));
    let spec = spec(&drafts, 0);
    let first = attempt(&spec, constraint, fonts)?;
    if let Some(input) = out_of_range {
        return match first {
            Err(LayoutError::InputOutOfRange) => Ok(Some(input)),
            other => Err(format!("{input:?} out of range gave {other:?}")),
        };
    }
    let first = first.map_err(|error| format!("the layout failed with {error:?}"))?;
    if first.boxes().first().map(|laid| laid.constraint) != Some(constraint) {
        return Err("the root's box has another constraint".to_string());
    }
    let mut parts = Vec::new();
    preorder(&spec, &mut parts);
    check(&first, &parts)?;
    let second = attempt(&spec, constraint, fonts)?
        .map_err(|error| format!("the second layout failed with {error:?}"))?;
    if first.text() != second.text() {
        return Err("two layouts wrote different text".to_string());
    }
    Ok(None)
}

#[test]
fn random_trees_lay_out_within_their_constraints_without_panicking() {
    let mut fonts = fonts();
    let mut beyond = Vec::new();
    for number in 0..GROUPS {
        match group(number, &mut fonts) {
            Ok(Some(input)) => beyond.push(input),
            Ok(None) => {}
            Err(why) => panic!("seed {:#x}, group {number}: {why}", SEED + number),
        }
    }
    assert_eq!(beyond.len() as u64, GROUPS / OUT_OF_RANGE_EVERY);
    for input in [
        Beyond::RootSide,
        Beyond::MinAboveMax,
        Beyond::LeafSide,
        Beyond::Request,
    ] {
        assert!(beyond.contains(&input), "no group had {input:?}");
    }
}
