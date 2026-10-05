//! The record shapes of format v1 (`EXPORT_FORMAT_V1.md`), one type per record kind.
//! Field names follow the format; the writer emits each record with its keys sorted.
use serde::Serialize;

/// A source position: 1-based line and column of the start of a construct.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Site {
    pub line: usize,
    pub col: usize,
}

/// The kind of a generic parameter or associated constant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Type,
    Nat,
    Bool,
}

/// A type or const expression.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum Term {
    Nat {
        nat: u64,
    },
    Bool {
        bool: bool,
    },
    Var {
        var: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        kind: Option<Kind>,
    },
    Con {
        con: String,
        args: Vec<Term>,
    },
    Assoc {
        assoc: Box<Term>,
        #[serde(rename = "trait")]
        trait_: String,
        targs: Vec<Term>,
        c: String,
    },
    Op {
        op: &'static str,
        a: Box<Term>,
        b: Box<Term>,
    },
    Not {
        not: Box<Term>,
    },
}

impl Term {
    /// `<goal.self as goal.trait<goal.args>>::name`.
    pub fn assoc(goal: Goal, name: String) -> Self {
        Self::Assoc {
            assoc: Box::new(goal.self_ty),
            trait_: goal.trait_,
            targs: goal.args,
            c: name,
        }
    }
}

/// `self: trait<args>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Goal {
    #[serde(rename = "self")]
    pub self_ty: Term,
    #[serde(rename = "trait")]
    pub trait_: String,
    pub args: Vec<Term>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Param {
    pub name: String,
    pub kind: Kind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Method {
    pub name: String,
    pub conds: Vec<Term>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub span_aliases: Vec<Site>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConstValue {
    pub name: String,
    pub value: Term,
}

/// One use in a function body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Use {
    #[serde(flatten)]
    pub kind: UseKind,
    pub span: Site,
    pub span_aliases: Vec<Site>,
    /// The start of the enclosing `with` expression, for a single named opening.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub construct_span: Option<Site>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "use", rename_all = "snake_case")]
pub enum UseKind {
    Method {
        goal: Goal,
        method: String,
        qualified: bool,
    },
    Const {
        goal: Goal,
        c: String,
    },
    Call {
        #[serde(rename = "fn")]
        function: String,
        args: Vec<Term>,
    },
    Type {
        #[serde(rename = "type")]
        ty: Term,
    },
    With {
        name: String,
        body: Vec<Use>,
    },
    WithList {
        names: Vec<String>,
        body: Vec<Use>,
    },
}

/// A program declaration (`kind: "item"`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ItemRecord {
    #[serde(flatten)]
    pub item: Item,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span_aliases: Option<Vec<Site>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "item", rename_all = "snake_case")]
pub enum Item {
    Trait {
        name: String,
        params: Vec<Param>,
        assoc_consts: Vec<Param>,
        methods: Vec<Method>,
        fixed: bool,
    },
    Struct {
        name: String,
        params: Vec<Param>,
        bounds: Vec<Goal>,
        conds: Vec<Term>,
    },
    Impl {
        index: u32,
        name: Option<String>,
        params: Vec<Param>,
        #[serde(rename = "trait")]
        trait_: String,
        targs: Vec<Term>,
        #[serde(rename = "self")]
        self_ty: Term,
        bounds: Vec<Goal>,
        conds: Vec<Term>,
        consts: Vec<ConstValue>,
        methods: Vec<Method>,
    },
    Fn {
        name: String,
        params: Vec<Param>,
        #[serde(rename = "const")]
        is_const: bool,
        bounds: Vec<Goal>,
        conds: Vec<Term>,
        body: Vec<Use>,
    },
    Value {
        name: String,
    },
    OutOfFragment(OutOfFragment),
}

/// Something format v1 cannot express, written in place of the item or decision.
/// `what` is an item kind (`fn`, `enum`, ...) or a decision kind (`goal`,
/// `condition`, `with`, `diagnostic`, `overlap`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OutOfFragment {
    pub what: &'static str,
    /// The function whose body holds the decision.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Where the decision was taken.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub site: Option<Site>,
    pub reason: &'static str,
}

impl Record {
    /// An `out_of_fragment` item in place of something v1 cannot express.
    pub fn outside(
        what: &'static str,
        name: Option<String>,
        site: Option<Site>,
        reason: &'static str,
    ) -> Self {
        Self::Item(ItemRecord {
            item: Item::OutOfFragment(OutOfFragment {
                what,
                name,
                site,
                reason,
            }),
            span_aliases: None,
        })
    }
}

/// How a goal at a use was answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Via {
    Bound,
    Impl(u32),
    Selected(u32),
}

/// The result of a named opening.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OpeningOutcome {
    Impl(u32),
    Error(String),
}

/// One line of the export.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Record {
    Header {
        schema: &'static str,
        compiler_commit: &'static str,
        source_sha256: String,
    },
    Note {
        text: &'static str,
    },
    Item(ItemRecord),
    Goal {
        site: Site,
        goal: Goal,
        via: Via,
    },
    Condition {
        site: Site,
        source: String,
        pred: Term,
        sels: Vec<u32>,
        outcome: String,
    },
    With {
        site: Site,
        name: String,
        outcome: OpeningOutcome,
    },
    Overlap {
        a: u32,
        b: u32,
        conflict: bool,
    },
    Diagnostic {
        code: String,
        site: Site,
    },
}

impl Record {
    /// The record as one JSON object, with `"v": 1` and its keys sorted.
    pub fn to_json(&self) -> serde_json::Value {
        #[derive(Serialize)]
        struct Line<'a> {
            v: u8,
            #[serde(flatten)]
            record: &'a Record,
        }
        let mut value = serde_json::to_value(Line { v: 1, record: self })
            .expect("format-v1 records serialize to JSON");
        value.sort_all_objects();
        value
    }
}
