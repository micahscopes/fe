//! `StorageMap` entries as slot hashes, and collection cursor steps.
//!
//! Fe finds a map's entry with `StateIndex::locate(map, root, key)` (or
//! `mapping_slot(root, key)`), which writes `key`'s encoding and then the
//! map's root slot into a fresh allocation and hashes them: the entry's slot
//! is `keccak256(key ‖ root)`, as a Solidity mapping's
//! (`ingots/std/src/evm/storage_map.fe`). When every part of the key encodes
//! as one 32-byte word (integers, `bool`, `Address`, and tuples of them, whose
//! encodings Fe concatenates), the lowering emits `keccak_words` on the key's
//! words and the root directly, without touching the memory pool. Other keys
//! keep the library call.
//!
//! Treating such a slot as the map's own, apart from every literal slot and
//! from other maps' entries, is the hash assumption (keccak256 does not
//! collide); each module lists where it rests on it.
//!
//! Likewise, Fe lowers `for` loops through `Collection` / `Bidirectional` /
//! `Driver` (`Identity`) methods `start`, `has`, `next`, `last`, `prev` on
//! fixed arrays `[T; N]` and `Range<Known<0>, Unknown>`. Before type erasure
//! those calls still name the collection type and array length `N`, so the
//! lowering turns them directly into scalar cursor arithmetic (`0`, `lt %c, N`,
//! `add %c, 1`) instead of opaque helper calls that take a dummy collection
//! memory piece.

use hir::analysis::ty::ty_check::BodyOwner;
use mir::{RuntimeClass, RuntimeInstance};

use crate::Db;

/// A call of `StorageMap<K, V>::locate` or `mapping_slot<K>` whose key is words.
pub struct Locate {
    /// The key type, as Fe prints it.
    pub key: String,
    /// Index of the root slot argument.
    pub root_arg: usize,
    /// Index of the key argument.
    pub key_arg: usize,
    /// Whether the return value is `Location { slot, offset }` (two words) or a
    /// bare slot word.
    pub has_offset: bool,
}

/// A collection cursor step or wrapping scalar helper recognized before erasure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// `start` -> `0`, or `last` on `[T; N]` -> `N - 1`.
    Start(u64),
    /// `has` on `[T; N]` -> `lt(c, N)`.
    HasConst(u64),
    /// `has` on `Range<Known<0>, Unknown>` -> `lt(c, end)` where `end` is arg 0.
    HasArg,
    /// `next` -> `(add(c, 1), c)`.
    Next(u64),
    /// `prev` -> `(sub(c, 1), c)`.
    Prev(u64),
    /// `WrappingAdd::wrapping_add(a, b)` -> `add(a, b)` (canonicalized by dst).
    WrappingAdd,
    /// `WrappingSub::wrapping_sub(a, b)` -> `sub(a, b)` (canonicalized by dst).
    WrappingSub,
}

/// The integer, `bool` and `Address` types, each one word in a map key's
/// encoding: its value as a word (a signed integer sign-extended, as
/// `IntWord::to_word` gives it, which is also how the lowering keeps it).
const WORD_KEYS: &[&str] = &[
    "u8", "u16", "u32", "u64", "u128", "u256", "usize", "i8", "i16", "i32", "i64", "i128", "i256", "isize", "bool",
    "Address",
];

fn split_top_tuple(inner: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (i, ch) in inner.char_indices() {
        match ch {
            '(' | '<' | '[' => depth += 1,
            ')' | '>' | ']' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                let s = inner[start..i].trim();
                if !s.is_empty() {
                    parts.push(s);
                }
                start = i + 1;
            }
            _ => {}
        }
    }
    let tail = inner[start..].trim();
    if !tail.is_empty() {
        parts.push(tail);
    }
    parts
}

/// Whether a key type, as Fe prints it, encodes as words: a word key, or a
/// tuple of two to six word keys (including nested tuples).
fn word_key(ty: &str) -> bool {
    let ty = ty.trim();
    if WORD_KEYS.contains(&ty) {
        return true;
    }
    let Some(inner) = ty.strip_prefix('(').and_then(|t| t.strip_suffix(')')) else { return false };
    let parts = split_top_tuple(inner);
    (2..=6).contains(&parts.len()) && parts.iter().all(|p| word_key(p))
}

/// The map-entry lookup `callee` is, if it is one with a word key.
pub fn locate<'db>(db: Db<'db>, callee: RuntimeInstance<'db>) -> Option<Locate> {
    let semantic = callee.key(db).semantic(db)?;
    let key = semantic.key(db);
    let BodyOwner::Func(func, _) = key.owner(db) else { return None };
    let name = func.name(db).to_opt()?.data(db).to_string();
    let sig = callee.interface_signature(db);
    if name == "locate" {
        let target = func.containing_impl_trait(db)?.ty(db).pretty_print(db).to_string();
        if !target.starts_with("StorageMap<") {
            return None;
        }
        let k = key.subst(db).generic_args(db).first()?.pretty_print(db).to_string();
        // (map, root, key) -> Location { slot, offset }.
        let returns_two = matches!(&sig.ret, Some(RuntimeClass::AggregateValue { .. }));
        return (word_key(&k) && sig.params.len() == 3 && returns_two).then_some(Locate {
            key: k,
            root_arg: 1,
            key_arg: 2,
            has_offset: true,
        });
    }
    if name == "mapping_slot" && func.containing_impl_trait(db).is_none() {
        let k = key.subst(db).generic_args(db).first()?.pretty_print(db).to_string();
        let returns_one = matches!(&sig.ret, Some(RuntimeClass::Scalar(_)));
        return (word_key(&k) && sig.params.len() == 2 && returns_one).then_some(Locate {
            key: k,
            root_arg: 0,
            key_arg: 1,
            has_offset: false,
        });
    }
    None
}

/// The collection cursor step or wrapping arithmetic helper `callee` is, if
/// recognized before type erasure.
pub fn step<'db>(db: Db<'db>, callee: RuntimeInstance<'db>) -> Option<Step> {
    let semantic = callee.key(db).semantic(db)?;
    let key = semantic.key(db);
    let BodyOwner::Func(func, _) = key.owner(db) else { return None };
    let name = func.name(db).to_opt()?.data(db).to_string();
    let imp = func.containing_impl_trait(db)?;
    let trait_name = imp.trait_def(db)?.name(db).to_opt()?.data(db).to_string();
    let target = imp.ty(db).pretty_print(db).to_string();
    let sig = callee.interface_signature(db);

    if matches!(trait_name.as_str(), "WrappingAdd" | "WrappingSub")
        && matches!(name.as_str(), "wrapping_add" | "wrapping_sub")
        && WORD_KEYS.contains(&target.as_str())
        && sig.params.len() == 2
    {
        return Some(if name == "wrapping_add" { Step::WrappingAdd } else { Step::WrappingSub });
    }

    if !matches!(trait_name.as_str(), "Collection" | "Bidirectional" | "Driver") {
        return None;
    }
    if trait_name == "Driver" && !matches!(target.as_str(), "Identity" | "Reversed") {
        return None;
    }

    let base_param = match name.as_str() {
        "start" | "last" => sig.params.last()?,
        "has" | "next" | "prev" if sig.params.len() >= 2 => &sig.params[sig.params.len() - 2],
        _ => return None,
    };

    // Check if the collection parameter is a fixed array `[T; N]`.
    let array_len = base_param.class.ref_pointee().and_then(|c| c.array_len(db));
    let is_zero_range = if matches!(target.as_str(), "Identity" | "Reversed") {
        key.subst(db)
            .generic_args(db)
            .first()
            .is_some_and(|a| a.pretty_print(db) == "Range<Known<0>, Unknown>")
    } else {
        target == "Range<Known<0>, Unknown>"
    };
    let reversed = target == "Reversed";

    match (name.as_str(), array_len, is_zero_range, reversed) {
        ("start", Some(_), _, false) => Some(Step::Start(0)),
        ("start", Some(n), _, true) if n > 0 => Some(Step::Start(n - 1)),
        ("start", None, true, false) => Some(Step::Start(0)),
        ("last", Some(n), _, false) if n > 0 => Some(Step::Start(n - 1)),
        ("has", Some(n), _, _) => Some(Step::HasConst(n)),
        ("has", None, true, false) => Some(Step::HasArg),
        ("next", Some(_), _, false) | ("next", None, true, false) => Some(Step::Next(1)),
        ("next", Some(_), _, true) | ("prev", Some(_), _, false) => Some(Step::Prev(1)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::word_key;

    #[test]
    fn word_keys() {
        assert!(word_key("u256"));
        assert!(word_key("Address"));
        assert!(word_key("(Address, Address)"));
        assert!(word_key("(u256, Address, bool)"));
        assert!(word_key("((Address, Address), u256)"));
        assert!(!word_key("String<31>"));
        assert!(!word_key("(u256, String<4>)"));
        assert!(!word_key("Pair"));
    }
}
