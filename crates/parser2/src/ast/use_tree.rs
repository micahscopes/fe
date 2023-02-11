use rowan::ast::{support, AstNode};

use super::ast_node;

use crate::{SyntaxKind as SK, SyntaxToken};

ast_node! {
    /// A use tree.
    /// `Foo::Foo2::{Bar::*, Baz::{x, y}}`
    pub struct UseTree,
    SK::UseTree,
}
impl UseTree {
    /// Returns the path of this use tree.
    /// `Foo::Foo2` in `Foo::Foo2::{Bar::*, Baz::{x, y}}`
    ///
    /// NOTE: If the tree root is started with `{}`, then this method will
    /// return `None`.
    pub fn path(&self) -> Option<UsePath> {
        support::child(self.syntax())
    }

    /// Returns the children of this use tree.
    ///
    /// `Bar::*` and `Baz::{x, y}` in `Foo::Foo2::{Bar::*, Baz::{x, y}}`.
    pub fn children(&self) -> Option<UseTreeList> {
        support::child(self.syntax())
    }

    //// Returns the alias of this use tree.
    /// `Bar` in `Foo as Bar;`
    pub fn alias(&self) -> Option<UseTreeAlias> {
        support::child(self.syntax())
    }
}

ast_node! {
    pub struct UseTreeList,
    SK::UseTreeList,
    IntoIterator<Item=UseTree>,
}

ast_node! {
    pub struct UsePath,
    SK::UsePath,
    IntoIterator<Item = UsePathSegment>,
}

ast_node! {
    pub struct UsePathSegment,
    SK::UsePathSegment,
}
impl UsePathSegment {
    pub fn kind(&self) -> Option<UsePathSegmentKind> {
        match self.syntax().first_child_or_token() {
            Some(node) => match node.kind() {
                SK::SelfKw => Some(UsePathSegmentKind::SelfPath(node.into_token().unwrap())),
                SK::Ident => Some(UsePathSegmentKind::Ident(node.into_token().unwrap())),
                SK::Star => Some(UsePathSegmentKind::Wildcard(node.into_token().unwrap())),
                _ => None,
            },
            _ => None,
        }
    }

    pub fn ident(&self) -> Option<SyntaxToken> {
        support::token(self.syntax(), SK::Ident)
    }

    pub fn self_token(&self) -> Option<SyntaxToken> {
        support::token(self.syntax(), SK::SelfKw)
    }

    pub fn wildcard(&self) -> Option<SyntaxToken> {
        support::token(self.syntax(), SK::Star)
    }
}

ast_node! {
    pub struct UseTreeAlias,
    SK::UseTreeRename,
}
impl UseTreeAlias {
    //// Returns `Some` if the alias is specified as an ident.
    pub fn ident(&self) -> Option<SyntaxToken> {
        support::token(self.syntax(), SK::Ident)
    }

    /// Returns `Some` if the alias is specified as `_`.
    pub fn underscore(&self) -> Option<SyntaxToken> {
        support::token(self.syntax(), SK::Underscore)
    }
}

/// A path segment in a use tree.
pub enum UsePathSegmentKind {
    /// `self`
    SelfPath(SyntaxToken),
    /// `foo`
    Ident(SyntaxToken),
    /// `*`
    /// This is only allowed in the last segment of a path.
    Wildcard(SyntaxToken),
}