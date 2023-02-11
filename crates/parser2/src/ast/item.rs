use super::ast_node;
use crate::{FeLang, SyntaxKind as SK, SyntaxToken};

use rowan::ast::{support, AstNode};

ast_node! {
    /// The top-level node of the AST tree.
    pub struct Root,
    SK::Root,
}
impl Root {
    pub fn items(&self) -> Option<ItemList> {
        support::child(self.syntax())
    }
}

ast_node! {
    /// A list of items in a module.
    pub struct ItemList,
    SK::ItemList,
    IntoIterator<Item=Item>
}

ast_node! {
    /// A single item in a module.
    /// Use `[Item::kind]` to get the specific type of item.
    pub struct Item,
    SK::Fn
    | SK::Struct
    | SK::Contract
    | SK::Enum
    | SK::TypeAlias
    | SK::Impl
    | SK::Trait
    | SK::ImplTrait
    | SK::Const
    | SK::Use
    | SK::Extern,
}
impl Item {
    pub fn kind(&self) -> ItemKind {
        match self.syntax().kind() {
            SK::Fn => ItemKind::Fn(AstNode::cast(self.syntax().clone()).unwrap()),
            SK::Struct => ItemKind::Struct(AstNode::cast(self.syntax().clone()).unwrap()),
            SK::Contract => ItemKind::Contract(AstNode::cast(self.syntax().clone()).unwrap()),
            SK::Enum => ItemKind::Enum(AstNode::cast(self.syntax().clone()).unwrap()),
            SK::TypeAlias => ItemKind::TypeAlias(AstNode::cast(self.syntax().clone()).unwrap()),
            SK::Impl => ItemKind::Impl(AstNode::cast(self.syntax().clone()).unwrap()),
            SK::Trait => ItemKind::Trait(AstNode::cast(self.syntax().clone()).unwrap()),
            SK::ImplTrait => ItemKind::ImplTrait(AstNode::cast(self.syntax().clone()).unwrap()),
            SK::Const => ItemKind::Const(AstNode::cast(self.syntax().clone()).unwrap()),
            SK::Use => ItemKind::Use(AstNode::cast(self.syntax().clone()).unwrap()),
            SK::Extern => ItemKind::Extern(AstNode::cast(self.syntax().clone()).unwrap()),
            _ => unreachable!(),
        }
    }
}

ast_node! {
    /// `pub fn foo<T, U: Trait>(_ x: T, from u: U) -> T where T: Trait2 { ... }`
    pub struct Fn,
    SK::Fn,
}
impl super::GenericParamsOwner for Fn {}
impl super::WhereClauseOwner for Fn {}
impl super::AttrListOwner for Fn {}
impl super::ItemModifierOwner for Fn {}
impl Fn {
    /// Returns the name of the function.
    pub fn name(&self) -> Option<SyntaxToken> {
        support::token(self.syntax(), SK::Ident)
    }

    /// Returns the function's parameter list.
    pub fn params(&self) -> Option<super::FnParamList> {
        support::child(self.syntax())
    }

    /// Returns the function's return type.
    pub fn ret_ty(&self) -> Option<super::Type> {
        support::child(self.syntax())
    }

    /// Returns the function's body.
    pub fn body(&self) -> Option<super::BlockExpr> {
        support::child(self.syntax())
    }
}

ast_node! {
    pub struct Struct,
    SK::Struct,
}
impl super::GenericParamsOwner for Struct {}
impl super::WhereClauseOwner for Struct {}
impl super::AttrListOwner for Struct {}
impl super::ItemModifierOwner for Struct {}
impl Struct {
    /// Returns the name of the struct.
    pub fn name(&self) -> Option<SyntaxToken> {
        support::token(self.syntax(), SK::Ident)
    }

    /// Returns the struct's field def list.
    pub fn fields(&self) -> Option<RecordFieldDefList> {
        support::child(self.syntax())
    }
}

ast_node! {
    pub struct Contract,
    SK::Contract,
}
impl super::AttrListOwner for Contract {}
impl super::ItemModifierOwner for Contract {}
impl Contract {
    /// Returns the name of the contract.
    pub fn name(&self) -> Option<SyntaxToken> {
        support::token(self.syntax(), SK::Ident)
    }

    /// Returns the contract's field def list.
    pub fn fields(&self) -> Option<RecordFieldDefList> {
        support::child(self.syntax())
    }
}

ast_node! {
    pub struct Enum,
    SK::Enum,
}
impl super::GenericParamsOwner for Enum {}
impl super::WhereClauseOwner for Enum {}
impl super::AttrListOwner for Enum {}
impl super::ItemModifierOwner for Enum {}
impl Enum {
    /// Returns the name of the enum.
    pub fn name(&self) -> Option<SyntaxToken> {
        support::token(self.syntax(), SK::Ident)
    }

    /// Returns the enum's variant def list.
    pub fn variants(&self) -> Option<EnumVariantDefList> {
        support::child(self.syntax())
    }
}

ast_node! {
    /// `type Foo<T> = Bar<T>`
    pub struct TypeAlias,
    SK::TypeAlias,
}
impl super::GenericParamsOwner for TypeAlias {}
impl super::WhereClauseOwner for TypeAlias {}
impl super::AttrListOwner for TypeAlias {}
impl super::ItemModifierOwner for TypeAlias {}
impl TypeAlias {
    /// Returns the name of the type alias.
    /// `Foo` in `type Foo<T> = Bar<T>`
    pub fn alias(&self) -> Option<SyntaxToken> {
        support::token(self.syntax(), SK::Ident)
    }

    /// Returns the type alias's type.
    /// `Bar<T>` in `type Foo<T> = Bar<T>`
    pub fn ty(&self) -> Option<super::Type> {
        support::child(self.syntax())
    }
}

ast_node! {
    /// `trait Foo<..> where .. { .. }`
    pub struct Trait,
    SK::Trait,
}
impl super::GenericParamsOwner for Trait {}
impl super::WhereClauseOwner for Trait {}
impl super::AttrListOwner for Trait {}
impl super::ItemModifierOwner for Trait {}
impl Trait {
    /// Returns the name of the trait.
    /// `Foo` in `trait Foo<..> where .. { .. }`
    pub fn name(&self) -> Option<SyntaxToken> {
        support::token(self.syntax(), SK::Ident)
    }

    /// Returns the trait's item list.
    /// `{ .. }` in `trait Foo<..> where .. { .. }`
    /// NOTE: Currently only supports `fn` items.
    pub fn item_list(&self) -> Option<TraitItemList> {
        support::child(self.syntax())
    }
}

ast_node! {
    /// `impl Foo::Bar<T> where .. { .. }`
    pub struct Impl,
    SK::Impl,
}
impl super::GenericParamsOwner for Impl {}
impl super::WhereClauseOwner for Impl {}
impl super::AttrListOwner for Impl {}
impl super::ItemModifierOwner for Impl {}
impl Impl {
    /// Returns the type of the impl.
    /// `Foo::Bar<T>` in `impl Foo::Bar<T> where .. { .. }`
    pub fn ty(&self) -> Option<super::Type> {
        support::child(self.syntax())
    }

    /// Returns the impl item list.
    /// `{ .. }` in `impl Foo::Bar<T> where .. { .. }`
    /// NOTE: Currently only supports `fn` items.
    pub fn item_list(&self) -> Option<ImplItemList> {
        support::child(self.syntax())
    }
}

ast_node! {
    /// `impl<T> Foo for Bar<T> { .. }`
    pub struct ImplTrait,
    SK::ImplTrait,
}
impl super::GenericParamsOwner for ImplTrait {}
impl super::WhereClauseOwner for ImplTrait {}
impl super::AttrListOwner for ImplTrait {}
impl super::ItemModifierOwner for ImplTrait {}
impl ImplTrait {
    /// Returns the trait of the impl.
    /// `Foo` in `impl<T> Foo for Bar<T> { .. }`
    pub fn trait_(&self) -> Option<super::Type> {
        support::child(self.syntax())
    }

    /// Returns the type of the impl.
    /// `Bar<T>` in `impl<T> Foo for Bar<T> { .. }`
    pub fn ty(&self) -> Option<super::Type> {
        support::children(self.syntax()).nth(1)
    }

    /// Returns the trait impl item list.
    /// `{ .. }` in `impl<T> Foo for Bar<T> { .. }`
    /// NOTE: Currently only supports `fn` items.
    pub fn item_list(&self) -> Option<ImplTraitItemList> {
        support::child(self.syntax())
    }
}

ast_node! {
    /// `const FOO: u32 = 42;`
    pub struct Const,
    SK::Const,
}
impl super::AttrListOwner for Const {}
impl Const {
    /// Returns the name of the const.
    /// `FOO` in `const FOO: u32 = 42;`
    pub fn name(&self) -> Option<SyntaxToken> {
        support::token(self.syntax(), SK::Ident)
    }

    /// Returns the type of the const.
    /// `u32` in `const FOO: u32 = 42;`
    pub fn ty(&self) -> Option<super::Type> {
        support::child(self.syntax())
    }

    /// Returns the value of the const.
    /// `42` in `const FOO: u32 = 42;`
    pub fn value(&self) -> Option<super::Expr> {
        support::child(self.syntax())
    }
}

ast_node! {
    /// `use foo::{bar, Baz::*}`
    pub struct Use,
    SK::Use,
}
impl super::AttrListOwner for Use {}
impl Use {
    /// Returns the use tree.
    /// `foo::{bar, Baz::*}` in `use foo::{bar, Baz::*}`
    pub fn use_tree(&self) -> Option<super::UseTree> {
        support::child(self.syntax())
    }
}

ast_node! {
    /// `extern { .. }`
    pub struct Extern,
    SK::Extern,
}
impl super::AttrListOwner for Extern {}
impl Extern {
    /// Returns the item list.
    /// NOTE: Currently only supports `fn` items.
    pub fn extern_block(&self) -> Option<ExternItemList> {
        support::child(self.syntax())
    }
}

ast_node! {
    pub struct RecordFieldDefList,
    SK::RecordFieldDefList,
    IntoIterator<Item=RecordFieldDef>
}
ast_node! {
    pub struct RecordFieldDef,
    SK::RecordFieldDef,
}
impl RecordFieldDef {
    /// Returns the pub keyword if exists.
    pub fn pub_kw(&self) -> Option<SyntaxToken> {
        support::token(self.syntax(), SK::PubKw)
    }

    /// Returns the name of the field.
    pub fn name(&self) -> Option<SyntaxToken> {
        support::token(self.syntax(), SK::Ident)
    }

    /// Returns the type of the field.
    pub fn ty(&self) -> Option<super::Type> {
        support::child(self.syntax())
    }
}

ast_node! {
    pub struct EnumVariantDefList,
    SK::VariantDefList,
    IntoIterator<Item=EnumVariantDef>
}

ast_node! {
    /// `Foo(i32, u32)`
    pub struct EnumVariantDef,
    SK::VariantDef,
}
impl EnumVariantDef {
    /// Returns the name of the variant.
    /// `Foo` in `Foo(i32, u32)`
    pub fn name(&self) -> Option<SyntaxToken> {
        support::token(self.syntax(), SK::Ident)
    }

    /// Returns the type of the variant.
    /// `(i32, u32)` in `Foo(i32, u32)`
    /// Currently only tuple variants are supported.
    pub fn ty(&self) -> Option<super::Type> {
        support::child(self.syntax())
    }
}

ast_node! {
    pub struct TraitItemList,
    SK::TraitItemList,
    IntoIterator<Item=Fn>,
}

ast_node! {
    pub struct ImplItemList,
    SK::ImplItemList,
    IntoIterator<Item=Fn>,
}

ast_node! {
    pub struct ImplTraitItemList,
    SK::ImplTraitItemList,
    IntoIterator<Item=Fn>,
}

ast_node! {
    pub struct ExternItemList,
    SK::ExternItemList,
    IntoIterator<Item=Fn>,
}

ast_node! {
    /// A modifier on an item.
    /// `pub unsafe`
    pub struct ItemModifier,
    SK::ItemModifier,
}
impl ItemModifier {
    pub fn pub_kw(&self) -> Option<SyntaxToken> {
        support::token(self.syntax(), SK::PubKw)
    }

    pub fn unsafe_kw(&self) -> Option<SyntaxToken> {
        support::token(self.syntax(), SK::UnsafeKw)
    }
}

pub trait ItemModifierOwner: AstNode<Language = FeLang> {
    fn item_modifier(&self) -> Option<ItemModifier> {
        support::child(self.syntax())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, derive_more::From, derive_more::TryInto)]
pub enum ItemKind {
    Fn(Fn),
    Struct(Struct),
    Contract(Contract),
    Enum(Enum),
    TypeAlias(TypeAlias),
    Impl(Impl),
    Trait(Trait),
    ImplTrait(ImplTrait),
    Const(Const),
    Use(Use),
    Extern(Extern),
}

#[cfg(test)]
mod tests {
    use crate::{
        ast::{prelude::*, ExprKind, TypeKind},
        lexer::Lexer,
        parser::{ItemListScope, Parser},
    };

    use super::*;

    fn parse_item<T>(source: &str) -> T
    where
        T: TryFrom<ItemKind, Error = &'static str>,
    {
        let lexer = Lexer::new(source);
        let mut parser = Parser::new(lexer);

        parser.parse(ItemListScope::default(), None);
        let item_list = ItemList::cast(parser.finish().0).unwrap();
        let mut items = item_list.into_iter().collect::<Vec<_>>();
        assert_eq!(items.len(), 1);
        items.pop().unwrap().kind().try_into().unwrap()
    }

    #[test]
    fn func() {
        let source = r#" 
                /// This is doc comment
                #evm
                pub unsafe fn foo<T, U: Trait>(_ x: T, from u: U) -> (T, U) where T: Trait2 { return }
            "#;
        let func: Fn = parse_item(source);

        assert_eq!(func.name().unwrap().text(), "foo");
        assert_eq!(func.attr_list().unwrap().iter().count(), 2);
        assert_eq!(func.generic_params().unwrap().iter().count(), 2);
        assert!(func.where_clause().is_some());
        assert!(func.body().is_some());
        assert!(matches!(func.ret_ty().unwrap().kind(), TypeKind::Tuple(_)));
        let modifier = func.item_modifier().unwrap();
        assert!(modifier.pub_kw().is_some());
        assert!(modifier.unsafe_kw().is_some());
    }

    #[test]
    fn r#struct() {
        let source = r#"
                pub struct Foo<T, U: Trait> where T: Trait2 {
                    pub x: T,
                    y: (U, i32),
                }
            "#;
        let s: Struct = parse_item(source);
        assert_eq!(s.name().unwrap().text(), "Foo");
        let mut count = 0;
        for field in s.fields().unwrap() {
            match count {
                0 => {
                    assert!(field.pub_kw().is_some());
                    assert_eq!(field.name().unwrap().text(), "x");
                    assert!(matches!(field.ty().unwrap().kind(), TypeKind::Path(_)));
                }
                1 => {
                    assert!(field.pub_kw().is_none());
                    assert_eq!(field.name().unwrap().text(), "y");
                    assert!(matches!(field.ty().unwrap().kind(), TypeKind::Tuple(_)));
                }
                _ => unreachable!(),
            }
            count += 1;
        }
        assert_eq!(count, 2);
    }

    #[test]
    fn contract() {
        let source = r#"
                pub contract Foo {
                    pub x: u32,
                    y: (i32, u32),
                }
            "#;
        let c: Contract = parse_item(source);
        assert_eq!(c.name().unwrap().text(), "Foo");
        let mut count = 0;
        for field in c.fields().unwrap() {
            match count {
                0 => {
                    assert!(field.pub_kw().is_some());
                    assert_eq!(field.name().unwrap().text(), "x");
                    assert!(matches!(field.ty().unwrap().kind(), TypeKind::Path(_)));
                }
                1 => {
                    assert!(field.pub_kw().is_none());
                    assert_eq!(field.name().unwrap().text(), "y");
                    assert!(matches!(field.ty().unwrap().kind(), TypeKind::Tuple(_)));
                }
                _ => unreachable!(),
            }
            count += 1;
        }
        assert_eq!(count, 2);
    }

    #[test]
    fn r#enum() {
        let source = r#"
                pub enum Foo<T, U: Trait> where T: Trait2 {
                    Bar
                    Baz(T, U)
                }
            "#;
        let e: Enum = parse_item(source);
        assert_eq!(e.name().unwrap().text(), "Foo");

        let mut count = 0;
        for variant in e.variants().unwrap() {
            match count {
                0 => {
                    assert_eq!(variant.name().unwrap().text(), "Bar");
                    assert!(variant.ty().is_none());
                }
                1 => {
                    assert_eq!(variant.name().unwrap().text(), "Baz");
                    assert!(matches!(variant.ty().unwrap().kind(), TypeKind::Tuple(_)));
                }
                _ => unreachable!(),
            }
            count += 1;
        }
        assert_eq!(count, 2);
    }

    #[test]
    fn r#type() {
        let source = r#"
                type MyError<T> where T: Debug = Error<T, String>
            "#;
        let t: TypeAlias = parse_item(source);
        assert_eq!(t.alias().unwrap().text(), "MyError");
        assert!(matches!(t.ty().unwrap().kind(), TypeKind::Path(_)));
    }

    #[test]
    fn r#impl() {
        let source = r#"
                impl Foo {
                    pub fn foo<T>(self, t: T) -> T { return t }
                    pub fn bar(self) -> u32 { return 1 }
                    pub fn baz(mut self) { self.x = 1 }
                }
            "#;
        let i: Impl = parse_item(source);
        assert!(matches!(i.ty().unwrap().kind(), TypeKind::Path(_)));
        assert_eq!(i.item_list().unwrap().iter().count(), 3);
    }

    #[test]
    fn r#trait() {
        let source = r#"
                pub trait Foo {
                    pub fn foo<T>(self, t: T) -> T
                    pub fn default(self) -> u32 { return 1 }
                }
            "#;
        let t: Trait = parse_item(source);
        assert_eq!(t.name().unwrap().text(), "Foo");

        let mut count = 0;
        for f in t.item_list().unwrap() {
            match count {
                0 => {
                    assert!(f.body().is_none());
                }
                1 => {
                    assert!(f.body().is_some());
                }
                _ => unreachable!(),
            }
            count += 1;
        }
        assert_eq!(count, 2);
    }

    #[test]
    fn impl_trait() {
        let source = r#"
            impl Trait::Foo for (i32)  {
                fn foo<T>(self, _t: T) -> u32 { return 1 };
            }"#;
        let i: ImplTrait = parse_item(source);
        assert!(matches!(i.trait_().unwrap().kind(), TypeKind::Path(_)));
        assert!(matches!(i.ty().unwrap().kind(), TypeKind::Tuple(_)));
        assert!(i.item_list().unwrap().iter().count() == 1);
    }

    #[test]
    fn r#const() {
        let source = r#"
            pub const FOO: u32 = 1 + 1
        "#;
        let c: Const = parse_item(source);
        assert_eq!(c.name().unwrap().text(), "FOO");
        assert!(matches!(c.ty().unwrap().kind(), TypeKind::Path(_)));
        assert!(matches!(c.value().unwrap().kind(), ExprKind::Bin(_)));
    }

    #[test]
    fn r#use() {
        let source = r#"
            use foo::bar::{bar::*, baz::{Baz, Baz2}}
        "#;
        let u: Use = parse_item(source);
        let use_tree = u.use_tree().unwrap();
        let mut count = 0;
        for segment in use_tree.path().unwrap() {
            match count {
                0 => {
                    assert_eq!(segment.ident().unwrap().text(), "foo");
                }
                1 => {
                    assert_eq!(segment.ident().unwrap().text(), "bar");
                }
                _ => unreachable!(),
            }
            count += 1;
        }

        count = 0;
        let children = use_tree.children().unwrap();
        for child in children {
            match count {
                0 => {
                    let mut segments = child.path().unwrap().iter();
                    assert_eq!(segments.next().unwrap().ident().unwrap().text(), "bar");
                    assert!(segments.next().unwrap().wildcard().is_some());
                    assert!(segments.next().is_none());
                    assert!(child.children().is_none());
                }
                1 => {
                    let mut segments = child.path().unwrap().iter();
                    assert_eq!(segments.next().unwrap().ident().unwrap().text(), "baz");
                    assert!(child.children().unwrap().iter().count() == 2);
                }
                _ => unreachable!(),
            }
            count += 1;
        }
        assert_eq!(count, 2);

        let source = r#"
            use {foo as _foo, bar::Baz as _}
        "#;
        let u: Use = parse_item(source);
        let use_tree = u.use_tree().unwrap();
        assert!(use_tree.path().is_none());
        let mut count = 0;
        for child in use_tree.children().unwrap() {
            match count {
                0 => {
                    let alias = child.alias().unwrap();
                    assert_eq!(alias.ident().unwrap().text(), "_foo");
                }
                1 => {
                    let alias = child.alias().unwrap();
                    assert!(alias.underscore().is_some());
                }
                _ => unreachable!(),
            }
            count += 1;
        }
        assert_eq!(count, 2);
    }

    #[test]
    fn r#extern() {
        let source = r#"
            extern {
                pub unsafe fn foo()
                pub unsafe fn bar()
            }"#;
        let e: Extern = parse_item(source);

        for f in e.extern_block().unwrap() {
            assert!(f.body().is_none());
        }
        assert_eq!(e.extern_block().unwrap().iter().count(), 2);
    }
}