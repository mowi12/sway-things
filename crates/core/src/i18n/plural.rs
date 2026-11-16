// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The plural rule of a catalog, from the `Plural-Forms` line of its header.
//!
//! The line names how many forms a language has and which one a count takes, as a C expression
//! in `n`: `nplurals=3; plural=(n==1 ? 0 : n%10>=2 && n%10<=4 && (n%100<12 || n%100>14) ? 1 : 2);`.
//! The expression knows what gettext's own parser knows: `n`, decimal numbers, parentheses, `!`,
//! `* / %`, `+ -`, `< > <= >=`, `== !=`, `&&`, `||` and `?:`, with C's precedence, on unsigned
//! numbers. It comes from a file, so it is read with limits on its length and nesting, and a
//! division by zero or an index past the forms gives the first form, as gettext does.

/// How long an expression may be, in bytes. Real ones have less than 200.
const MAX_LENGTH: usize = 1024;

/// How deep parentheses and conditions may nest.
const MAX_DEPTH: usize = 32;

/// How many forms a language may have. Real ones have up to 6.
const MAX_FORMS: usize = 16;

/// The plural rule of a catalog: how many forms its plural entries have, and which a count takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plural {
    forms: usize,
    rule: Expr,
}

impl Default for Plural {
    /// The rule of English and the catalogs without a `Plural-Forms` line: two forms, the first
    /// for exactly one.
    fn default() -> Self {
        Self {
            forms: 2,
            rule: Expr::Binary(Op::Ne, Box::new(Expr::N), Box::new(Expr::Number(1))),
        }
    }
}

impl Plural {
    /// The rule in a catalog's `header` (the translation of the empty msgid), or the default if
    /// it has no `Plural-Forms` line or one that can't be read.
    pub fn from_header(header: &str) -> Self {
        header
            .lines()
            .find_map(|line| line.strip_prefix("Plural-Forms:"))
            .and_then(Self::parse)
            .unwrap_or_default()
    }

    /// The rule in the value of a `Plural-Forms` line: `nplurals=N; plural=EXPR;`.
    fn parse(value: &str) -> Option<Self> {
        let mut forms = None;
        let mut rule = None;
        for part in value.split(';') {
            let Some((key, value)) = part.split_once('=') else {
                continue;
            };
            match key.trim() {
                "nplurals" => forms = value.trim().parse::<usize>().ok(),
                "plural" => rule = Some(Expr::parse(value)?),
                _ => {}
            }
        }
        let forms = forms.filter(|forms| (1..=MAX_FORMS).contains(forms))?;
        Some(Self { forms, rule: rule? })
    }

    /// The index of the form for `n`: 0 if the rule fails or points past the forms.
    pub fn index(&self, n: u64) -> usize {
        self.rule
            .eval(n)
            .and_then(|index| usize::try_from(index).ok())
            .filter(|index| *index < self.forms)
            .unwrap_or(0)
    }
}

/// An operator with two operands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Or,
    And,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}

/// A parsed expression.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Expr {
    N,
    Number(u64),
    Not(Box<Expr>),
    Binary(Op, Box<Expr>, Box<Expr>),
    Condition(Box<Expr>, Box<Expr>, Box<Expr>),
}

impl Expr {
    /// The expression in `text`, or `None` if it isn't one or is too long or too deep.
    fn parse(text: &str) -> Option<Self> {
        if text.len() > MAX_LENGTH {
            return None;
        }
        let mut parser = Parser {
            tokens: tokenize(text)?,
            position: 0,
            depth: 0,
        };
        let expr = parser.condition()?;
        (parser.position == parser.tokens.len()).then_some(expr)
    }

    /// The value for `n`, with wrapping arithmetic as C's unsigned numbers, or `None` on a
    /// division by zero.
    fn eval(&self, n: u64) -> Option<u64> {
        Some(match self {
            Self::N => n,
            Self::Number(number) => *number,
            Self::Not(operand) => u64::from(operand.eval(n)? == 0),
            Self::Condition(test, then, otherwise) => {
                if test.eval(n)? != 0 {
                    then.eval(n)?
                } else {
                    otherwise.eval(n)?
                }
            }
            Self::Binary(Op::Or, left, right) => {
                u64::from(left.eval(n)? != 0 || right.eval(n)? != 0)
            }
            Self::Binary(Op::And, left, right) => {
                u64::from(left.eval(n)? != 0 && right.eval(n)? != 0)
            }
            Self::Binary(op, left, right) => {
                let (left, right) = (left.eval(n)?, right.eval(n)?);
                match op {
                    Op::Eq => u64::from(left == right),
                    Op::Ne => u64::from(left != right),
                    Op::Lt => u64::from(left < right),
                    Op::Gt => u64::from(left > right),
                    Op::Le => u64::from(left <= right),
                    Op::Ge => u64::from(left >= right),
                    Op::Add => left.wrapping_add(right),
                    Op::Sub => left.wrapping_sub(right),
                    Op::Mul => left.wrapping_mul(right),
                    Op::Div => left.checked_div(right)?,
                    Op::Rem => left.checked_rem(right)?,
                    Op::Or | Op::And => return None,
                }
            }
        })
    }
}

/// A token of an expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Token {
    N,
    Number(u64),
    Op(Op),
    Not,
    Question,
    Colon,
    Open,
    Close,
}

/// The tokens of `text`, or `None` if it has a character that belongs to none.
fn tokenize(text: &str) -> Option<Vec<Token>> {
    let bytes = text.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;
    while let Some(&byte) = bytes.get(index) {
        let next = bytes.get(index + 1).copied();
        let (token, length) = match (byte, next) {
            (b' ' | b'\t' | b'\n' | b'\r', _) => {
                index += 1;
                continue;
            }
            (b'0'..=b'9', _) => {
                let digits = bytes[index..]
                    .iter()
                    .take_while(|byte| byte.is_ascii_digit())
                    .count();
                let number = text[index..index + digits].parse().ok()?;
                (Token::Number(number), digits)
            }
            (b'n', _) => (Token::N, 1),
            (b'|', Some(b'|')) => (Token::Op(Op::Or), 2),
            (b'&', Some(b'&')) => (Token::Op(Op::And), 2),
            (b'=', Some(b'=')) => (Token::Op(Op::Eq), 2),
            (b'!', Some(b'=')) => (Token::Op(Op::Ne), 2),
            (b'<', Some(b'=')) => (Token::Op(Op::Le), 2),
            (b'>', Some(b'=')) => (Token::Op(Op::Ge), 2),
            (b'<', _) => (Token::Op(Op::Lt), 1),
            (b'>', _) => (Token::Op(Op::Gt), 1),
            (b'+', _) => (Token::Op(Op::Add), 1),
            (b'-', _) => (Token::Op(Op::Sub), 1),
            (b'*', _) => (Token::Op(Op::Mul), 1),
            (b'/', _) => (Token::Op(Op::Div), 1),
            (b'%', _) => (Token::Op(Op::Rem), 1),
            (b'!', _) => (Token::Not, 1),
            (b'?', _) => (Token::Question, 1),
            (b':', _) => (Token::Colon, 1),
            (b'(', _) => (Token::Open, 1),
            (b')', _) => (Token::Close, 1),
            _ => return None,
        };
        tokens.push(token);
        index += length;
    }
    Some(tokens)
}

/// The levels of the binary operators, loosest first.
const LEVELS: &[&[Op]] = &[
    &[Op::Or],
    &[Op::And],
    &[Op::Eq, Op::Ne],
    &[Op::Lt, Op::Gt, Op::Le, Op::Ge],
    &[Op::Add, Op::Sub],
    &[Op::Mul, Op::Div, Op::Rem],
];

/// A recursive descent parser over the tokens of an expression.
struct Parser {
    tokens: Vec<Token>,
    position: usize,
    depth: usize,
}

impl Parser {
    fn peek(&self) -> Option<Token> {
        self.tokens.get(self.position).copied()
    }

    fn eat(&mut self, token: Token) -> bool {
        let found = self.peek() == Some(token);
        if found {
            self.position += 1;
        }
        found
    }

    /// Runs `parse` one level deeper, or fails past [`MAX_DEPTH`].
    fn nested(&mut self, parse: impl FnOnce(&mut Self) -> Option<Expr>) -> Option<Expr> {
        if self.depth >= MAX_DEPTH {
            return None;
        }
        self.depth += 1;
        let expr = parse(self);
        self.depth -= 1;
        expr
    }

    /// `test ? then : otherwise`, which groups to the right, or a binary expression.
    fn condition(&mut self) -> Option<Expr> {
        let test = self.binary(0)?;
        if !self.eat(Token::Question) {
            return Some(test);
        }
        let then = self.nested(Self::condition)?;
        if !self.eat(Token::Colon) {
            return None;
        }
        let otherwise = self.nested(Self::condition)?;
        Some(Expr::Condition(
            Box::new(test),
            Box::new(then),
            Box::new(otherwise),
        ))
    }

    /// The operators of `level` and the tighter ones, grouped to the left.
    fn binary(&mut self, level: usize) -> Option<Expr> {
        let Some(ops) = LEVELS.get(level) else {
            return self.unary();
        };
        let mut left = self.binary(level + 1)?;
        while let Some(Token::Op(op)) = self.peek() {
            if !ops.contains(&op) {
                break;
            }
            self.position += 1;
            let right = self.binary(level + 1)?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
        Some(left)
    }

    /// `!operand`, `n`, a number or a parenthesized expression.
    fn unary(&mut self) -> Option<Expr> {
        let token = self.peek()?;
        self.position += 1;
        match token {
            Token::Not => self.nested(|parser| Some(Expr::Not(Box::new(parser.unary()?)))),
            Token::N => Some(Expr::N),
            Token::Number(number) => Some(Expr::Number(number)),
            Token::Open => {
                let expr = self.nested(Self::condition)?;
                self.eat(Token::Close).then_some(expr)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::{Expr, Plural};

    fn rule(value: &str) -> Plural {
        Plural::from_header(&format!(
            "Content-Type: text/plain; charset=UTF-8\nPlural-Forms: {value}\n"
        ))
    }

    fn indexes(plural: &Plural, counts: &[u64]) -> Vec<usize> {
        counts.iter().map(|n| plural.index(*n)).collect()
    }

    #[test]
    fn germanic_languages_have_two_forms() {
        let german = rule("nplurals=2; plural=(n != 1);");
        assert_eq!(indexes(&german, &[0, 1, 2, 11, 21]), [1, 0, 1, 1, 1]);
        assert_eq!(Plural::default(), german);
    }

    #[test]
    fn french_takes_the_singular_for_zero() {
        let french = rule("nplurals=2; plural=(n > 1);");
        assert_eq!(indexes(&french, &[0, 1, 2, 100]), [0, 0, 1, 1]);
    }

    #[test]
    fn japanese_has_one_form() {
        let japanese = rule("nplurals=1; plural=0;");
        assert_eq!(indexes(&japanese, &[0, 1, 2]), [0, 0, 0]);
    }

    #[test]
    fn slavic_languages_have_three_forms() {
        let polish = rule(
            "nplurals=3; plural=(n==1 ? 0 : n%10>=2 && n%10<=4 && (n%100<12 || n%100>14) ? 1 : 2);",
        );
        assert_eq!(
            indexes(&polish, &[1, 2, 4, 5, 12, 14, 22, 25, 102, 112]),
            [0, 1, 1, 2, 2, 2, 1, 2, 1, 2]
        );
        let russian = rule(
            "nplurals=3; plural=(n%10==1 && n%100!=11 ? 0 : n%10>=2 && n%10<=4 && (n%100<10 || n%100>=20) ? 1 : 2);",
        );
        assert_eq!(
            indexes(&russian, &[1, 2, 5, 11, 12, 21, 22, 25, 111]),
            [0, 1, 2, 2, 2, 0, 1, 2, 2]
        );
    }

    #[test]
    fn arabic_has_six_forms() {
        let arabic = rule(
            "nplurals=6; plural=n==0 ? 0 : n==1 ? 1 : n==2 ? 2 : n%100>=3 && n%100<=10 ? 3 : n%100>=11 ? 4 : 5;",
        );
        assert_eq!(
            indexes(&arabic, &[0, 1, 2, 3, 10, 11, 99, 100, 102, 103]),
            [0, 1, 2, 3, 3, 4, 4, 5, 5, 3]
        );
    }

    #[test]
    fn operators_bind_as_in_c() {
        let eval = |text: &str| Expr::parse(text).unwrap().eval(7);
        assert_eq!(eval("1 + 2 * 3"), Some(7));
        assert_eq!(eval("(1 + 2) * 3"), Some(9));
        assert_eq!(eval("10 - 4 - 3"), Some(3));
        assert_eq!(eval("n % 4 == 3 && !0"), Some(1));
        assert_eq!(eval("0 || n > 6"), Some(1));
        assert_eq!(eval("1 ? 2 : 0 ? 3 : 4"), Some(2));
        assert_eq!(eval("0 ? 2 : 0 ? 3 : 4"), Some(4));
        assert_eq!(eval("!!n"), Some(1));
        assert_eq!(eval("0 - 1"), Some(u64::MAX));
        assert_eq!(eval("n / 0"), None);
    }

    #[test]
    fn broken_rules_fall_back_to_the_default() {
        for value in [
            "nplurals=2; plural=n != ;",
            "nplurals=2; plural=(n != 1;",
            "nplurals=2; plural=n x 1;",
            "nplurals=0; plural=0;",
            "nplurals=99; plural=n;",
            "nplurals=2;",
            "plural=n != 1;",
            "",
        ] {
            assert_eq!(rule(value), Plural::default(), "{value}");
        }
        assert_eq!(Plural::from_header(""), Plural::default());
        let deep = format!(
            "nplurals=2; plural={}n{};",
            "(".repeat(100),
            ")".repeat(100)
        );
        assert_eq!(rule(&deep), Plural::default());
        let long = format!("nplurals=2; plural=n{};", " + 1".repeat(1000));
        assert_eq!(rule(&long), Plural::default());
    }

    #[test]
    fn indexes_past_the_forms_give_the_first() {
        assert_eq!(rule("nplurals=2; plural=n;").index(5), 0);
        assert_eq!(rule("nplurals=2; plural=n / (n - 3);").index(3), 0);
    }

    proptest! {
        #[test]
        fn any_rule_is_read_and_used_without_a_panic(value in ".{0,200}", n: u64) {
            let plural = rule(&value);
            prop_assert!(plural.index(n) < plural.forms);
        }

        #[test]
        fn rules_of_operators_are_read_without_a_panic(
            value in "[n0-9 ()!?:<>=&|+*/%-]{0,200}",
            n: u64,
        ) {
            let plural = rule(&format!("nplurals=3; plural={value};"));
            prop_assert!(plural.index(n) < plural.forms);
        }
    }
}
