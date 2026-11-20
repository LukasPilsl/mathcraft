//! Rebuilds structure from flat Mathcad text, so a region that is opened for editing shows the
//! same typeset view as one typed with templates: `derivative(t, v)` becomes the derivative
//! operator with its slots, `a/b` a fraction, `x^2` a power, `sqrt(x)` a root.
//!
//! The reading is a guess at what the editor would have built. The caller only keeps the rows
//! after checking that they write the same text again.

use super::{Item, OpKind, Row, ARROW};

/// The calls that read as operators, with the name the parser uses.
const CALLS: [(&str, OpKind); 10] = [
    ("integral", OpKind::Integral),
    ("sum", OpKind::Sum),
    ("product", OpKind::Product),
    ("limit", OpKind::Limit),
    ("root", OpKind::NthRoot),
    ("derivative", OpKind::Derivative),
    ("derivative", OpKind::NthDerivative),
    ("partial", OpKind::Partial),
    ("partial", OpKind::NthPartial),
    ("gradient", OpKind::Gradient),
];

/// The structured rows for `text`, or `None` when the text has no structure to show.
pub(super) fn rebuild(text: &str) -> Option<Row> {
    let chars: Vec<char> = text.chars().filter(|c| !c.is_control()).collect();
    let mut at = 0;
    let row = parse_row(&chars, &mut at, Stop::End, 0)?;
    (at == chars.len() && row.iter().any(|i| !matches!(i, Item::Atom(_)))).then_some(row)
}

#[derive(Clone, Copy, PartialEq)]
enum Stop {
    /// Up to the end of the text.
    End,
    /// Up to the `)` that closes a group (not consumed).
    Paren,
    /// Up to a `,` or `)` at this level: one argument of a call (not consumed).
    Argument,
    /// Up to the `]` that closes an index (not consumed).
    Bracket,
    /// Up to the `‖` that closes a norm (not consumed).
    Norm,
    /// Up to a `,`, `;` or `]`: one cell of a matrix (not consumed).
    Cell,
}

const MAX_DEPTH: usize = 40;

fn is_name(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '.' | '_' | '\'')
}

/// The operand before the cursor: the trailing name or number of `row`, or its last structured
/// item. `None` when the row ends in an operator or space.
fn take_operand(row: &mut Row) -> Option<Row> {
    match row.last()? {
        Item::Atom(c) if is_name(*c) => {
            let start = row
                .iter()
                .rposition(|i| !matches!(i, Item::Atom(c) if is_name(*c)))
                .map_or(0, |p| p + 1);
            Some(row.split_off(start))
        }
        Item::Atom(_) => None,
        _ => row.pop().map(|item| vec![item]),
    }
}

fn parse_row(chars: &[char], at: &mut usize, stop: Stop, depth: usize) -> Option<Row> {
    if depth > MAX_DEPTH {
        return None;
    }
    let mut row: Row = Vec::new();
    while *at < chars.len() {
        let c = chars[*at];
        match (c, stop) {
            (')', Stop::Paren | Stop::Argument) => break,
            (',', Stop::Argument | Stop::Cell) => break,
            (';' | ']', Stop::Cell) => break,
            (']', Stop::Bracket) => break,
            ('‖', Stop::Norm) => break,
            _ => {}
        }
        match c {
            '‖' => {
                *at += 1;
                let inner = parse_row(chars, at, Stop::Norm, depth + 1)?;
                expect(chars, at, '‖')?;
                row.push(Item::Op {
                    kind: OpKind::Norm,
                    slots: vec![inner],
                });
            }
            '(' => {
                *at += 1;
                let inner = parse_row(chars, at, Stop::Paren, depth + 1)?;
                expect(chars, at, ')')?;
                row.push(Item::Group(inner));
            }
            '[' => {
                *at += 1;
                // After an operand it indexes; otherwise it is a matrix.
                if row.last().is_some_and(is_operand_end) {
                    let base = take_operand(&mut row)?;
                    let index = parse_row(chars, at, Stop::Bracket, depth + 1)?;
                    expect(chars, at, ']')?;
                    row.push(Item::Index { base, index });
                } else {
                    row.push(parse_matrix(chars, at, depth)?);
                }
            }
            '/' | '^' => {
                *at += 1;
                let base = take_operand(&mut row)?;
                let mut other = Vec::new();
                if c == '^' && chars.get(*at) == Some(&'-') {
                    *at += 1;
                    other.push(Item::Atom('-'));
                }
                other.extend(read_operand(chars, at, depth)?);
                row.push(if c == '/' {
                    Item::Fraction {
                        num: base,
                        den: other,
                    }
                } else {
                    Item::Power { base, exp: other }
                });
            }
            '-' if chars.get(*at + 1) == Some(&'>') => {
                *at += 2;
                row.push(Item::Atom(ARROW));
            }
            c if c.is_alphabetic() => {
                let start = *at;
                while *at < chars.len() && is_name(chars[*at]) {
                    *at += 1;
                }
                let word: String = chars[start..*at].iter().collect();
                if chars.get(*at) == Some(&'(') && is_call(&word) {
                    *at += 1;
                    let args = parse_arguments(chars, at, depth)?;
                    row.push(call_item(&word, args)?);
                } else {
                    row.extend(word.chars().map(Item::Atom));
                }
            }
            c => {
                *at += 1;
                row.push(Item::Atom(c));
            }
        }
    }
    Some(row)
}

fn is_call(word: &str) -> bool {
    word == "sqrt" || CALLS.iter().any(|(name, _)| *name == word)
}

fn is_operand_end(item: &Item) -> bool {
    match item {
        Item::Atom(c) => is_name(*c),
        _ => true,
    }
}

fn expect(chars: &[char], at: &mut usize, c: char) -> Option<()> {
    (chars.get(*at) == Some(&c)).then(|| *at += 1)
}

/// The arguments of a call, after its `(`, up to and including the `)`.
fn parse_arguments(chars: &[char], at: &mut usize, depth: usize) -> Option<Vec<Row>> {
    let mut args = Vec::new();
    loop {
        args.push(parse_row(chars, at, Stop::Argument, depth + 1)?);
        match chars.get(*at)? {
            ',' => {
                *at += 1;
                // The writer puts one space after a comma between arguments.
                if chars.get(*at) == Some(&' ') {
                    *at += 1;
                }
            }
            ')' => {
                *at += 1;
                return Some(args);
            }
            _ => return None,
        }
    }
}

fn call_item(word: &str, mut args: Vec<Row>) -> Option<Item> {
    if word == "sqrt" {
        return (args.len() == 1).then(|| Item::Sqrt(args.remove(0)));
    }
    if word == "gradient" {
        // The body, then any number of variables, which share one slot: `a, b`.
        let mut args = args.into_iter();
        let body = args.next()?;
        let mut vars: Row = Vec::new();
        for (i, arg) in args.enumerate() {
            if i > 0 {
                vars.push(Item::Atom(','));
                vars.push(Item::Atom(' '));
            }
            vars.extend(arg);
        }
        return Some(Item::Op {
            kind: OpKind::Gradient,
            slots: vec![vars, body],
        });
    }
    let kind = CALLS
        .iter()
        .filter(|(name, _)| *name == word)
        .map(|(_, kind)| *kind)
        .find(|kind| kind.slot_total() == args.len())?;
    let mut slots: Vec<Row> = vec![Vec::new(); args.len()];
    for (arg, &slot) in args.into_iter().zip(kind.arguments()) {
        slots[slot] = arg;
    }
    Some(Item::Op { kind, slots })
}

/// One operand after `/` or `^`: a name or number, a group, or a call.
fn read_operand(chars: &[char], at: &mut usize, depth: usize) -> Option<Row> {
    let c = *chars.get(*at)?;
    if c == '(' {
        *at += 1;
        let inner = parse_row(chars, at, Stop::Paren, depth + 1)?;
        expect(chars, at, ')')?;
        return Some(vec![Item::Group(inner)]);
    }
    if !is_name(c) {
        return None;
    }
    let start = *at;
    while *at < chars.len() && is_name(chars[*at]) {
        *at += 1;
    }
    let word: String = chars[start..*at].iter().collect();
    if chars.get(*at) == Some(&'(') && is_call(&word) {
        *at += 1;
        let args = parse_arguments(chars, at, depth)?;
        return call_item(&word, args).map(|item| vec![item]);
    }
    Some(word.chars().map(Item::Atom).collect())
}

/// A matrix after its `[`: cells split by `,` (within a row) and `;` (between rows).
fn parse_matrix(chars: &[char], at: &mut usize, depth: usize) -> Option<Item> {
    let mut rows: Vec<Vec<Row>> = vec![Vec::new()];
    loop {
        let cell = parse_row(chars, at, Stop::Cell, depth + 1)?;
        rows.last_mut()?.push(cell);
        match chars.get(*at)? {
            ',' => *at += 1,
            ';' => {
                *at += 1;
                rows.push(Vec::new());
            }
            ']' => {
                *at += 1;
                break;
            }
            _ => return None,
        }
        // The writer puts one space after the separator.
        if chars.get(*at) == Some(&' ') {
            *at += 1;
        }
    }
    let cols = rows.first()?.len();
    if cols == 0 || rows.iter().any(|r| r.len() != cols) {
        return None;
    }
    Some(Item::Matrix {
        cols,
        cells: rows.into_iter().flatten().collect(),
    })
}
