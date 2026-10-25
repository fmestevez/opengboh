//! The computer opponent's script language (`*.AI` files; see
//! `docs/formats.md`, "AI scripts"): a parser and an interpreter. The
//! built-in functions the scripts call are supplied by a [`Host`].
//!
//! Names are case-sensitive (a local `routing` must not hide the constant
//! `ROUTING`, or every unit looks routed); keywords are not. A file holds `import "file";`,
//! `const NAME value, ...;`, `var a, b;` and `procedure Name(args)` (a
//! forward declaration ending in `;`, or a body in braces), and optionally
//! one plan: `Name { ... }` with the same items plus a `Conditions` block of
//! statements and `Orders for Leader { Condition Call(args); ... }` blocks.
//! Statements: `var`, assignment, calls, `if (e) s [else s]`, `for v = a to
//! b s`, `while (e) s`, `return [e]`, and `write` / `watch` with a string
//! and expressions. Expressions are integers with `NOT`, `AND`, `OR`,
//! comparisons and arithmetic; lists are integer handles.

use std::collections::HashMap;

/// Constants the original registers before any script is read.
pub const BUILTIN_CONSTS: &[(&str, i32)] = &[
    ("TQ", 1),
    ("COHESION_HIT", 2),
    ("SIZE", 3),
    ("MOVEMENT_POINTS", 4),
    ("MOVEMENT_REMAINING", 5),
    ("MISSLE_TYPE", 6),
    ("MISSLE_RANGE", 7),
    ("MISSLES_REMAINING", 8),
    ("UNIT_TYPE", 9),
    ("FAILED", 0),
    ("CENTER", 0),
    ("NORTH", 1),
    ("NORTHEAST", 2),
    ("EAST", 3),
    ("SOUTHEAST", 4),
    ("SOUTH", 5),
    ("SOUTHWEST", 6),
    ("WEST", 7),
    ("NORTHWEST", 8),
    ("FALSE", 0),
    ("TRUE", 1),
    ("CLOCKWISE", 1),
    ("COUNTERCLOCKWISE", 2),
];

/// Built-in functions the original registers, with their argument counts.
/// The host is called with the name in lower case.
pub const BUILTINS: &[(&str, usize)] = &[
    ("MoveTo", 2),
    ("MoveToward", 2),
    ("Rotate", 2),
    ("CanMove", 1),
    ("CanRotate", 2),
    ("CanAttack", 2),
    ("Fire", 2),
    ("HexOccupancy", 1),
    ("AdjacentHex", 2),
    ("Query", 2),
    ("GroupListReset", 0),
    ("GroupListGetNext", 0),
    ("GroupListGetPrevious", 0),
    ("GroupListGet", 0),
    ("GroupListGetSize", 0),
    ("Direction", 2),
    ("Distance", 2),
    ("GetList", 0),
    ("AddToList", 2),
    ("GetNextInList", 1),
    ("ReleaseList", 1),
    ("SizeOfList", 1),
    ("RemoveFromList", 2),
    ("ResetList", 1),
    ("GetLast", 1),
    ("AssignList", 3),
    ("IsMemberOf", 2),
    ("ClearList", 1),
    ("DumpList", 1),
    ("GetLeader", 0),
    ("FinishLeader", 0),
    ("GetUnitPaths", 2),
    ("InCommandRange", 1),
    ("IsLeader", 1),
    ("Attack", 2),
    ("IsEnemy", 2),
    ("AttemptToRally", 1),
    ("BreakPoint", 0),
    ("Recover", 1),
    ("GetValidMovement", 1),
    ("CanBeOrdered", 1),
    ("IsInGeneralDir", 2),
    ("AnyoneToCombat", 1),
    ("TurnToAttack", 1),
    ("FindClosestEnemyToAttack", 1),
    ("FindClosestEnemyToFire", 1),
    ("GetTurnNum", 0),
    ("Random", 1),
    ("EnemyWithinRange", 2),
    ("FindHurtingUnit", 0),
    // (Caesar's: the hits it must have more than)
    ("FindHurtingUnit", 1),
    // Hannibal's
    ("DoGroupBerserk", 0),
    ("DoGroupRetreat", 1),
    ("GetGroup", 2),
    ("GetGroupLeader", 1),
    ("GroupMove", 2),
    ("GroupRestoreCohesion", 0),
    ("InMomentumPhase", 0),
];

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Num(i32),
    Var(String),
    Call(String, Vec<Expr>),
    Not(Box<Expr>),
    Neg(Box<Expr>),
    Bin(Op, Box<Expr>, Box<Expr>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Or,
    And,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Add,
    Sub,
    Mul,
    Div,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Var(Vec<String>),
    Assign(String, Expr),
    Expr(Expr),
    If(Expr, Vec<Stmt>, Vec<Stmt>),
    For(String, Expr, Expr, Vec<Stmt>),
    While(Expr, Vec<Stmt>),
    Return(Option<Expr>),
    /// `write` / `watch`: a message and values (debug output).
    Write(String, Vec<Expr>),
}

#[derive(Debug, Clone, Default)]
pub struct Procedure {
    pub params: Vec<String>,
    /// Which parameters are `var` ones (Caesar's scripts): passed by
    /// reference, their last values written back to the caller's variables.
    pub by_ref: Vec<bool>,
    pub body: Option<Vec<Stmt>>,
}

/// A battle plan: per-leader orders chosen by named conditions.
#[derive(Debug, Clone, Default)]
pub struct Plan {
    pub name: String,
    pub conditions: Vec<Stmt>,
    /// Leader name → `(condition variable, call)` in order.
    pub orders: Vec<(String, Vec<(String, Expr)>)>,
}

/// A parsed script with everything it imports.
#[derive(Debug, Clone, Default)]
pub struct Script {
    pub consts: HashMap<String, i32>,
    pub globals: Vec<String>,
    /// Procedures by name and how many parameters (the language overloads
    /// on the count: `basicman.ai` has two `FigurePriority`).
    pub procs: HashMap<(String, usize), Procedure>,
    pub plan: Option<Plan>,
    imported: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Id(String),
    Num(i32),
    Str(String),
    Sym(&'static str),
}

/// A parse or run error, with the file and line for parse errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

const SYMS: &[&str] = &["==", "!=", "<=", ">=", "{", "}", "(", ")", ";", ",", "=", "<", ">", "+", "-", "*", "/"];

fn lex(src: &str, file: &str) -> Result<Vec<(Tok, usize)>, Error> {
    let b = src.as_bytes();
    let mut out = Vec::new();
    let (mut i, mut line) = (0, 1);
    while i < b.len() {
        let c = b[i];
        if c == b'\n' {
            line += 1;
            i += 1;
        } else if c.is_ascii_whitespace() {
            i += 1;
        } else if c == b'\'' {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
        } else if c == b'"' {
            let start = i + 1;
            i = start;
            while i < b.len() && b[i] != b'"' && b[i] != b'\n' {
                i += 1;
            }
            out.push((Tok::Str(String::from_utf8_lossy(&b[start..i]).into_owned()), line));
            i += 1;
        } else if c.is_ascii_digit() {
            let start = i;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
            let n = src[start..i].parse().map_err(|_| Error(format!("{file}:{line}: bad number")))?;
            out.push((Tok::Num(n), line));
        } else if c.is_ascii_alphabetic() || c == b'_' {
            let start = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            out.push((Tok::Id(src[start..i].to_string()), line));
        } else if let Some(s) = SYMS.iter().find(|s| b[i..].starts_with(s.as_bytes())) {
            out.push((Tok::Sym(s), line));
            i += s.len();
        } else {
            return Err(Error(format!("{file}:{line}: unexpected character {:?}", c as char)));
        }
    }
    Ok(out)
}

struct Parser<'a> {
    toks: Vec<(Tok, usize)>,
    pos: usize,
    file: &'a str,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos).map(|t| &t.0)
    }

    fn peek_at(&self, n: usize) -> Option<&Tok> {
        self.toks.get(self.pos + n).map(|t| &t.0)
    }

    fn err<T>(&self, what: &str) -> Result<T, Error> {
        let line = self.toks.get(self.pos).or(self.toks.last()).map_or(0, |t| t.1);
        Err(Error(format!("{}:{line}: {what} (found {:?})", self.file, self.peek())))
    }

    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).map(|t| t.0.clone());
        self.pos += 1;
        t
    }

    fn is_sym(&self, s: &str) -> bool {
        matches!(self.peek(), Some(Tok::Sym(x)) if *x == s)
    }

    /// Keywords match in any case; names are case-sensitive (`routing`
    /// and `ROUTING` are different).
    fn is_kw(&self, k: &str) -> bool {
        matches!(self.peek(), Some(Tok::Id(x)) if x.eq_ignore_ascii_case(k))
    }

    fn eat_sym(&mut self, s: &str) -> bool {
        let yes = self.is_sym(s);
        if yes {
            self.pos += 1;
        }
        yes
    }

    fn expect_sym(&mut self, s: &str) -> Result<(), Error> {
        if self.eat_sym(s) { Ok(()) } else { self.err(&format!("expected {s}")) }
    }

    /// The `;` ending a simple statement; the original also accepts its
    /// absence before `else` or `}`.
    fn end_stmt(&mut self) -> Result<(), Error> {
        if self.eat_sym(";") || self.is_kw("else") || self.is_sym("}") { Ok(()) } else { self.err("expected ;") }
    }

    fn ident(&mut self) -> Result<String, Error> {
        match self.next() {
            Some(Tok::Id(s)) => Ok(s),
            _ => {
                self.pos -= 1;
                self.err("expected a name")
            }
        }
    }

    fn names(&mut self) -> Result<Vec<String>, Error> {
        let mut v = vec![self.ident()?];
        while self.eat_sym(",") {
            v.push(self.ident()?);
        }
        Ok(v)
    }

    /// Top-level or plan-level items until `}` or the end.
    fn items(&mut self, script: &mut Script, load: &mut dyn FnMut(&str) -> Option<String>, in_plan: bool) -> Result<(), Error> {
        loop {
            if self.eat_sym(";") {
                continue;
            }
            match self.peek().cloned() {
                None => return Ok(()),
                Some(Tok::Sym("}")) if in_plan => return Ok(()),
                Some(Tok::Id(k)) if k.eq_ignore_ascii_case("import") => {
                    self.pos += 1;
                    let Some(Tok::Str(name)) = self.next() else { return self.err("expected a file name") };
                    self.expect_sym(";")?;
                    script.import(&name, load)?;
                }
                Some(Tok::Id(k)) if k.eq_ignore_ascii_case("const") => {
                    self.pos += 1;
                    loop {
                        let name = self.ident()?;
                        let neg = self.eat_sym("-");
                        let Some(Tok::Num(v)) = self.next() else { return self.err("expected a number") };
                        script.consts.insert(name, if neg { -v } else { v });
                        if !self.eat_sym(",") {
                            break;
                        }
                    }
                    self.expect_sym(";")?;
                }
                Some(Tok::Id(k)) if k.eq_ignore_ascii_case("var") => {
                    self.pos += 1;
                    let names = self.names()?;
                    self.expect_sym(";")?;
                    script.globals.extend(names);
                }
                Some(Tok::Id(k)) if k.eq_ignore_ascii_case("procedure") => {
                    self.pos += 1;
                    let name = self.ident()?;
                    self.expect_sym("(")?;
                    let (mut params, mut by_ref) = (Vec::new(), Vec::new());
                    while !self.is_sym(")") {
                        let var = self.is_kw("var");
                        if var {
                            self.pos += 1;
                        }
                        params.push(self.ident()?);
                        by_ref.push(var);
                        if !self.eat_sym(",") {
                            break;
                        }
                    }
                    self.expect_sym(")")?;
                    let body = if self.is_sym("{") { Some(self.block()?) } else { None };
                    let p = script.procs.entry((name, params.len())).or_default();
                    p.params = params;
                    p.by_ref = by_ref;
                    if body.is_some() {
                        p.body = body;
                    }
                }
                Some(Tok::Id(k)) if in_plan && k.eq_ignore_ascii_case("conditions") => {
                    self.pos += 1;
                    let body = self.block()?;
                    script.plan.get_or_insert_with(Default::default).conditions.extend(body);
                }
                Some(Tok::Id(k)) if in_plan && k.eq_ignore_ascii_case("orders") => {
                    self.pos += 1;
                    if !self.is_kw("for") {
                        return self.err("expected for");
                    }
                    self.pos += 1;
                    let leader = self.ident()?;
                    self.expect_sym("{")?;
                    let mut entries = Vec::new();
                    while !self.eat_sym("}") {
                        if self.eat_sym(";") {
                            continue;
                        }
                        let cond = self.ident()?;
                        let call = self.expr()?;
                        self.expect_sym(";")?;
                        entries.push((cond, call));
                    }
                    script.plan.get_or_insert_with(Default::default).orders.push((leader, entries));
                }
                Some(Tok::Id(name)) if !in_plan && matches!(self.peek_at(1), Some(Tok::Sym("{"))) => {
                    self.pos += 2;
                    script.plan.get_or_insert_with(Default::default).name = name;
                    self.items(script, load, true)?;
                    self.expect_sym("}")?;
                }
                _ => return self.err("expected a declaration"),
            }
        }
    }

    fn block(&mut self) -> Result<Vec<Stmt>, Error> {
        self.expect_sym("{")?;
        let mut v = Vec::new();
        while !self.eat_sym("}") {
            if self.peek().is_none() {
                return self.err("unclosed block");
            }
            if self.eat_sym(";") {
                continue;
            }
            v.push(self.stmt()?);
        }
        Ok(v)
    }

    /// A statement, or a block, as the body of `if`, `for` and `while`.
    fn body(&mut self) -> Result<Vec<Stmt>, Error> {
        if self.is_sym("{") { self.block() } else { Ok(vec![self.stmt()?]) }
    }

    fn stmt(&mut self) -> Result<Stmt, Error> {
        let Some(Tok::Id(k)) = self.peek().cloned() else { return self.err("expected a statement") };
        let s = match k.to_ascii_lowercase().as_str() {
            "var" => {
                self.pos += 1;
                let n = self.names()?;
                self.end_stmt()?;
                Stmt::Var(n)
            }
            "if" => {
                self.pos += 1;
                let cond = self.expr()?;
                let then = self.body()?;
                // `if (..) { } ;` may still be followed by `else`
                let save = self.pos;
                self.eat_sym(";");
                let other = if self.is_kw("else") {
                    self.pos += 1;
                    self.body()?
                } else {
                    self.pos = save;
                    Vec::new()
                };
                Stmt::If(cond, then, other)
            }
            "for" => {
                self.pos += 1;
                let v = self.ident()?;
                self.expect_sym("=")?;
                let from = self.expr()?;
                if !self.is_kw("to") {
                    return self.err("expected to");
                }
                self.pos += 1;
                let to = self.expr()?;
                Stmt::For(v, from, to, self.body()?)
            }
            "while" => {
                self.pos += 1;
                let cond = self.expr()?;
                Stmt::While(cond, self.body()?)
            }
            "return" => {
                self.pos += 1;
                let e = if self.is_sym(";") || self.is_kw("else") || self.is_sym("}") { None } else { Some(self.expr()?) };
                self.end_stmt()?;
                Stmt::Return(e)
            }
            "write" | "watch" => {
                self.pos += 1;
                let msg = match self.peek().cloned() {
                    Some(Tok::Str(s)) => {
                        self.pos += 1;
                        s
                    }
                    _ => String::new(),
                };
                // the terminating `;` may be missing before `else` or `}`
                let mut args = Vec::new();
                while !self.eat_sym(";") && !self.is_kw("else") && !self.is_sym("}") {
                    self.eat_sym(",");
                    args.push(self.expr()?);
                }
                Stmt::Write(msg, args)
            }
            _ if matches!(self.peek_at(1), Some(Tok::Sym("="))) => {
                self.pos += 2;
                let e = self.expr()?;
                self.end_stmt()?;
                Stmt::Assign(k, e)
            }
            _ => {
                let e = self.expr()?;
                self.end_stmt()?;
                Stmt::Expr(e)
            }
        };
        Ok(s)
    }

    fn expr(&mut self) -> Result<Expr, Error> {
        self.binary(0)
    }

    fn binary(&mut self, level: usize) -> Result<Expr, Error> {
        const LEVELS: &[&[(&str, Op)]] = &[
            &[("or", Op::Or)],
            &[("and", Op::And)],
            &[("==", Op::Eq), ("!=", Op::Ne), ("<=", Op::Le), (">=", Op::Ge), ("<", Op::Lt), (">", Op::Gt)],
            &[("+", Op::Add), ("-", Op::Sub)],
            &[("*", Op::Mul), ("/", Op::Div)],
        ];
        if level == LEVELS.len() {
            return self.unary();
        }
        let mut lhs = self.binary(level + 1)?;
        'more: loop {
            for &(tok, op) in LEVELS[level] {
                let hit = match self.peek() {
                    Some(Tok::Sym(s)) => *s == tok,
                    Some(Tok::Id(s)) => s.eq_ignore_ascii_case(tok),
                    _ => false,
                };
                if hit {
                    self.pos += 1;
                    let rhs = self.binary(level + 1)?;
                    lhs = Expr::Bin(op, Box::new(lhs), Box::new(rhs));
                    continue 'more;
                }
            }
            return Ok(lhs);
        }
    }

    fn unary(&mut self) -> Result<Expr, Error> {
        if self.is_kw("not") {
            self.pos += 1;
            return Ok(Expr::Not(Box::new(self.unary()?)));
        }
        if self.eat_sym("-") {
            return Ok(Expr::Neg(Box::new(self.unary()?)));
        }
        match self.next() {
            Some(Tok::Num(n)) => Ok(Expr::Num(n)),
            Some(Tok::Sym("(")) => {
                let e = self.expr()?;
                self.expect_sym(")")?;
                Ok(e)
            }
            Some(Tok::Id(name)) => {
                if self.eat_sym("(") {
                    let mut args = Vec::new();
                    if !self.eat_sym(")") {
                        loop {
                            args.push(self.expr()?);
                            if self.eat_sym(")") {
                                break;
                            }
                            self.expect_sym(",")?;
                        }
                    }
                    Ok(Expr::Call(name, args))
                } else {
                    Ok(Expr::Var(name))
                }
            }
            _ => {
                self.pos -= 1;
                self.err("expected an expression")
            }
        }
    }
}

impl Script {
    /// Parse the script `file` (read with `load`, which also resolves its
    /// imports).
    pub fn load(file: &str, load: &mut dyn FnMut(&str) -> Option<String>) -> Result<Self, Error> {
        let mut s = Script { consts: BUILTIN_CONSTS.iter().map(|&(n, v)| (n.into(), v)).collect(), ..Default::default() };
        s.import(file, load)?;
        Ok(s)
    }

    fn import(&mut self, file: &str, load: &mut dyn FnMut(&str) -> Option<String>) -> Result<(), Error> {
        let key = file.to_ascii_lowercase();
        if self.imported.contains(&key) {
            return Ok(());
        }
        self.imported.push(key);
        let src = load(file).ok_or_else(|| Error(format!("{file} not found")))?;
        let mut p = Parser { toks: lex(&src, file)?, pos: 0, file };
        p.items(self, load, false)
    }
}

fn calls_in_expr(e: &Expr, out: &mut Vec<(String, usize)>) {
    match e {
        Expr::Call(n, args) => {
            out.push((n.clone(), args.len()));
            args.iter().for_each(|a| calls_in_expr(a, out));
        }
        Expr::Not(e) | Expr::Neg(e) => calls_in_expr(e, out),
        Expr::Bin(_, a, b) => {
            calls_in_expr(a, out);
            calls_in_expr(b, out);
        }
        Expr::Num(_) | Expr::Var(_) => {}
    }
}

fn calls_in(body: &[Stmt], out: &mut Vec<(String, usize)>) {
    for s in body {
        match s {
            Stmt::Assign(_, e) | Stmt::Expr(e) | Stmt::Return(Some(e)) => calls_in_expr(e, out),
            Stmt::If(c, a, b) => {
                calls_in_expr(c, out);
                calls_in(a, out);
                calls_in(b, out);
            }
            Stmt::For(_, a, b, body) => {
                calls_in_expr(a, out);
                calls_in_expr(b, out);
                calls_in(body, out);
            }
            Stmt::While(c, body) => {
                calls_in_expr(c, out);
                calls_in(body, out);
            }
            Stmt::Write(_, args) => args.iter().for_each(|a| calls_in_expr(a, out)),
            Stmt::Var(_) | Stmt::Return(None) => {}
        }
    }
}

impl Script {
    /// Calls that are neither a procedure with a body nor a built-in with
    /// that many arguments.
    pub fn unresolved(&self) -> Vec<(String, usize)> {
        let mut calls = Vec::new();
        for p in self.procs.values() {
            calls_in(p.body.as_deref().unwrap_or(&[]), &mut calls);
        }
        if let Some(plan) = &self.plan {
            calls_in(&plan.conditions, &mut calls);
            for (_, entries) in &plan.orders {
                entries.iter().for_each(|(_, e)| calls_in_expr(e, &mut calls));
            }
        }
        calls.retain(|(n, k)| {
            !self.procs.get(&(n.clone(), *k)).is_some_and(|p| p.body.is_some()) && !BUILTINS.iter().any(|(b, a)| b == n && a == k)
        });
        calls.sort();
        calls.dedup();
        calls
    }
}

/// The built-in functions, implemented by the game.
pub trait Host {
    /// Call built-in `name` (lower case); `None` if there is no such
    /// function.
    fn call(&mut self, name: &str, args: &[i32]) -> Option<i32>;
    /// Debug output from `write` and `watch`.
    fn write(&mut self, _message: &str, _values: &[i32]) {}
}

enum Flow {
    Next,
    Return(i32),
}

/// Running a script: its global variables persist between calls.
pub struct Machine {
    pub script: Script,
    pub globals: HashMap<String, i32>,
    /// Steps left before a run is abandoned (guards against endless loops).
    budget: u64,
}

/// Steps one run may take.
const BUDGET: u64 = 5_000_000;

impl Machine {
    pub fn new(script: Script) -> Self {
        let globals = script.globals.iter().map(|g| (g.clone(), 0)).collect();
        Self { script, globals, budget: BUDGET }
    }

    /// Run the plan's orders for `leader` (matched, case insensitively, by
    /// its name's letters and digits — Caesar's `Tribune III (Caesarian)` is
    /// `TribuneIIICaesarian` — else by its first word, as Alexander's
    /// scripts name theirs): evaluate the conditions, then call the first
    /// entry whose condition holds. Returns whether an entry ran.
    pub fn orders(&mut self, leader: &str, host: &mut dyn Host) -> Result<bool, Error> {
        self.budget = BUDGET;
        let Some(plan) = self.script.plan.clone() else { return Ok(false) };
        let whole: String = leader.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
        let first = leader.split_whitespace().next().unwrap_or("");
        let find = |key: &str| plan.orders.iter().find(|(l, _)| l.eq_ignore_ascii_case(key));
        let Some((_, entries)) = find(&whole).or_else(|| find(first)) else { return Ok(false) };
        let mut frame = HashMap::new();
        self.exec(&plan.conditions, &mut frame, host)?;
        for (cond, call) in entries {
            if self.get(cond, &frame) != 0 {
                self.eval(call, &mut frame, host)?;
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Name each command group after its leader, where the scripts haven't
    /// a constant of that name: `Tribune III (Caesarian)` is
    /// `TRIBUNE_III_CAESARIAN`, its value the group's id. The original
    /// defines none (Caesar's scripts that use them fail to compile there;
    /// Hannibal's `constant.ai` lists its leaders under numbers no
    /// scenario's groups have, and those stand); the rewrite lets Caesar's
    /// plans run as their authors meant.
    pub fn define_groups(&mut self, leaders: impl IntoIterator<Item = (String, i32)>) {
        for (name, group) in leaders {
            let mut key = String::new();
            for word in name.split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| !w.is_empty()) {
                if !key.is_empty() {
                    key.push('_');
                }
                key.push_str(&word.to_ascii_uppercase());
            }
            self.script.consts.entry(key).or_insert(group);
        }
    }

    /// Call procedure or built-in `name`.
    pub fn call(&mut self, name: &str, args: &[i32], host: &mut dyn Host) -> Result<i32, Error> {
        self.call_with(name, args, host).map(|(v, _)| v)
    }

    /// Call `name`, returning also a script procedure's final values of
    /// its parameters.
    fn call_with(&mut self, name: &str, args: &[i32], host: &mut dyn Host) -> Result<(i32, Vec<i32>), Error> {
        if let Some(p) = self.script.procs.get(&(name.to_string(), args.len())).cloned()
            && let Some(body) = &p.body
        {
            let mut frame: HashMap<String, i32> = p.params.iter().cloned().zip(args.iter().copied()).collect();
            let v = match self.exec(body, &mut frame, host)? {
                Flow::Return(v) => v,
                Flow::Next => 0,
            };
            return Ok((v, p.params.iter().map(|n| frame.get(n).copied().unwrap_or(0)).collect()));
        }
        host.call(&name.to_ascii_lowercase(), args).map(|v| (v, Vec::new())).ok_or_else(|| Error(format!("unknown procedure {name}")))
    }

    fn get(&self, name: &str, frame: &HashMap<String, i32>) -> i32 {
        frame.get(name).or_else(|| self.globals.get(name)).or_else(|| self.script.consts.get(name)).copied().unwrap_or(0)
    }

    fn set(&mut self, name: &str, v: i32, frame: &mut HashMap<String, i32>) {
        if let Some(x) = frame.get_mut(name) {
            *x = v;
        } else if let Some(x) = self.globals.get_mut(name) {
            *x = v;
        } else {
            frame.insert(name.into(), v);
        }
    }

    fn tick(&mut self) -> Result<(), Error> {
        self.budget = self.budget.checked_sub(1).ok_or_else(|| Error("script ran too long".into()))?;
        Ok(())
    }

    fn exec(&mut self, body: &[Stmt], frame: &mut HashMap<String, i32>, host: &mut dyn Host) -> Result<Flow, Error> {
        for s in body {
            self.tick()?;
            match s {
                Stmt::Var(names) => {
                    for n in names {
                        frame.insert(n.clone(), 0);
                    }
                }
                Stmt::Assign(n, e) => {
                    let v = self.eval(e, frame, host)?;
                    self.set(n, v, frame);
                }
                Stmt::Expr(e) => {
                    self.eval(e, frame, host)?;
                }
                Stmt::If(c, a, b) => {
                    let branch = if self.eval(c, frame, host)? != 0 { a } else { b };
                    if let Flow::Return(v) = self.exec(branch, frame, host)? {
                        return Ok(Flow::Return(v));
                    }
                }
                Stmt::For(v, from, to, b) => {
                    let (from, to) = (self.eval(from, frame, host)?, self.eval(to, frame, host)?);
                    let mut i = from;
                    while i <= to {
                        self.set(v, i, frame);
                        if let Flow::Return(r) = self.exec(b, frame, host)? {
                            return Ok(Flow::Return(r));
                        }
                        i = self.get(v, frame) + 1;
                    }
                }
                Stmt::While(c, b) => {
                    while self.eval(c, frame, host)? != 0 {
                        self.tick()?;
                        if let Flow::Return(r) = self.exec(b, frame, host)? {
                            return Ok(Flow::Return(r));
                        }
                    }
                }
                Stmt::Return(e) => {
                    let v = match e {
                        Some(e) => self.eval(e, frame, host)?,
                        None => 0,
                    };
                    return Ok(Flow::Return(v));
                }
                Stmt::Write(msg, args) => {
                    let vals = args.iter().map(|a| self.eval(a, frame, host)).collect::<Result<Vec<_>, _>>()?;
                    host.write(msg, &vals);
                }
            }
        }
        Ok(Flow::Next)
    }

    fn eval(&mut self, e: &Expr, frame: &mut HashMap<String, i32>, host: &mut dyn Host) -> Result<i32, Error> {
        Ok(match e {
            Expr::Num(n) => *n,
            Expr::Var(n) => self.get(n, frame),
            Expr::Not(e) => (self.eval(e, frame, host)? == 0) as i32,
            Expr::Neg(e) => -self.eval(e, frame, host)?,
            Expr::Call(name, args) => {
                let vals = args.iter().map(|a| self.eval(a, frame, host)).collect::<Result<Vec<_>, _>>()?;
                let (v, out) = self.call_with(name, &vals, host)?;
                let by_ref = self.script.procs.get(&(name.to_string(), args.len())).map(|p| p.by_ref.clone()).unwrap_or_default();
                for ((arg, &r), &value) in args.iter().zip(&by_ref).zip(&out) {
                    if let (true, Expr::Var(n)) = (r, arg) {
                        self.set(n, value, frame);
                    }
                }
                v
            }
            Expr::Bin(op, a, b) => {
                let a = self.eval(a, frame, host)?;
                let b = self.eval(b, frame, host)?;
                match op {
                    Op::Or => (a != 0 || b != 0) as i32,
                    Op::And => (a != 0 && b != 0) as i32,
                    Op::Eq => (a == b) as i32,
                    Op::Ne => (a != b) as i32,
                    Op::Lt => (a < b) as i32,
                    Op::Le => (a <= b) as i32,
                    Op::Gt => (a > b) as i32,
                    Op::Ge => (a >= b) as i32,
                    Op::Add => a.wrapping_add(b),
                    Op::Sub => a.wrapping_sub(b),
                    Op::Mul => a.wrapping_mul(b),
                    Op::Div => a.checked_div(b).unwrap_or(0),
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Recorder(Vec<(String, Vec<i32>)>);

    impl Host for Recorder {
        fn call(&mut self, name: &str, args: &[i32]) -> Option<i32> {
            self.0.push((name.into(), args.to_vec()));
            Some(match name {
                "getturnnum" => 2,
                _ => 0,
            })
        }
    }

    fn script(src: &str) -> Script {
        let src = src.to_string();
        Script::load("t.ai", &mut |_| Some(src.clone())).unwrap()
    }

    #[test]
    fn procedures_loops_and_precedence() {
        let s = script(
            "procedure Sum(n) { var t, i; t = 0; for i = 1 to n { t = t + i; }; return t; };
             procedure Check() { if (3 == 3 AND NOT 0) { return 1; } else { return 2; }; };",
        );
        let mut m = Machine::new(s);
        let mut h = Recorder(Vec::new());
        assert_eq!(m.call("Sum", &[4], &mut h), Ok(10));
        assert_eq!(m.call("Check", &[], &mut h), Ok(1));
    }

    /// A `var` parameter's last value goes back to the caller's variable
    /// (Caesar's scripts mark their moves done that way); group names are
    /// constants.
    #[test]
    fn var_parameters_and_group_names() {
        let s = script(
            "var done; procedure Mark(hex, var flag) { flag = hex + 1; return 0; };
             procedure Run() { Mark(4, done); return done; };",
        );
        let mut m = Machine::new(s);
        let mut h = Recorder(Vec::new());
        assert_eq!(m.call("Run", &[], &mut h), Ok(5));
        assert_eq!(m.globals["done"], 5);
        m.define_groups([("Tribune III (Caesarian)".to_string(), 13)]);
        assert_eq!(m.script.consts["TRIBUNE_III_CAESARIAN"], 13);
    }

    /// Parses every script in the game; runs only with `GBOH_DATA` set to
    /// the game's DATA directory.
    #[test]
    fn parses_the_game_scripts() {
        let Ok(dir) = std::env::var("GBOH_DATA") else { return };
        let files: Vec<_> = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).collect();
        let mut load = |name: &str| {
            let p = files.iter().find(|p| p.file_name().is_some_and(|f| f.eq_ignore_ascii_case(name)))?;
            Some(String::from_utf8_lossy(&std::fs::read(p).ok()?).into_owned())
        };
        let mut plans = 0;
        for f in files.clone() {
            let name = f.file_name().unwrap().to_string_lossy().into_owned();
            if !name.to_ascii_lowercase().ends_with(".ai") {
                continue;
            }
            let s = Script::load(&name, &mut load).unwrap_or_else(|e| panic!("{e}"));
            plans += s.plan.is_some() as usize;
            // libraries (BASICMAN, SPECIAL) rely on their importers
            if s.plan.is_some() {
                assert_eq!(s.unresolved(), vec![], "{name}");
            }
        }
        assert_eq!(plans, 42);
    }

    #[test]
    fn plans_pick_the_first_true_condition() {
        let s = script(
            "' a plan
             Test
             {
                var done;
                procedure GenericMove(hex);
                Conditions { var First, Second; First = GetTurnNum() == 1; Second = TRUE; };
                Orders for Porus { First GenericMove(1016); Second Beserk(); };
             }",
        );
        let mut m = Machine::new(s);
        let mut h = Recorder(Vec::new());
        assert_eq!(m.orders("Porus", &mut h), Ok(true));
        assert_eq!(h.0.last().unwrap().0, "beserk");
    }
}
