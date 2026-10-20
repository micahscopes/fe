//! `StorageMap` entries as slot hashes.
//!
//! Fe finds a map's entry with `StateIndex::locate(map, root, key)`, which
//! writes `key`'s encoding and then the map's root slot into a fresh
//! allocation and hashes them: the entry's slot is `keccak256(key ‖ root)`,
//! as a Solidity mapping's (`ingots/std/src/evm/storage_map.fe`). When every
//! part of the key encodes as one 32-byte word (integers, `bool`, `Address`,
//! and tuples of them, whose encodings Fe concatenates), the lowering hashes
//! the key's words and the root itself, written where free memory starts:
//! the same hash, and the allocation it skips is not observable (safe Fe
//! cannot read the free pointer or `msize`). Other keys keep the library
//! call.
//!
//! Treating such a slot as the map's own, apart from every literal slot and
//! from other maps' entries, is the hash assumption (keccak256 does not
//! collide); each module lists where it rests on it.

use hir::analysis::ty::ty_check::BodyOwner;
use mir::{RuntimeClass, RuntimeInstance};

use crate::Db;

/// A call of `StorageMap<K, V>::locate` whose key is words.
pub struct Locate {
    /// The key type, as Fe prints it.
    pub key: String,
}

/// The integer, `bool` and `Address` types, each one word in a map key's
/// encoding: its value as a word (a signed integer sign-extended, as
/// `IntWord::to_word` gives it, which is also how the lowering keeps it).
const WORD_KEYS: &[&str] = &[
    "u8", "u16", "u32", "u64", "u128", "u256", "usize", "i8", "i16", "i32", "i64", "i128", "i256", "isize", "bool",
    "Address",
];

/// Whether a key type, as Fe prints it, encodes as words: a word key, or a
/// tuple of up to five of them (the tuples `StorageKey` is implemented for).
fn word_key(ty: &str) -> bool {
    let ty = ty.trim();
    if WORD_KEYS.contains(&ty) {
        return true;
    }
    let Some(inner) = ty.strip_prefix('(').and_then(|t| t.strip_suffix(')')) else { return false };
    let parts: Vec<&str> = inner.split(',').map(str::trim).filter(|p| !p.is_empty()).collect();
    (2..=5).contains(&parts.len()) && parts.iter().all(|p| WORD_KEYS.contains(p))
}

/// The map-entry lookup `callee` is, if it is one with a word key.
pub fn locate<'db>(db: Db<'db>, callee: RuntimeInstance<'db>) -> Option<Locate> {
    let semantic = callee.key(db).semantic(db)?;
    let key = semantic.key(db);
    let BodyOwner::Func(func, _) = key.owner(db) else { return None };
    let name = func.name(db).to_opt()?.data(db).to_string();
    if name != "locate" {
        return None;
    }
    let target = func.containing_impl_trait(db)?.ty(db).pretty_print(db).to_string();
    if !target.starts_with("StorageMap<") {
        return None;
    }
    let k = key.subst(db).generic_args(db).first()?.pretty_print(db).to_string();
    let sig = callee.interface_signature(db);
    // (map, root, key) -> Location { slot, offset }.
    let returns_two = matches!(&sig.ret, Some(RuntimeClass::AggregateValue { .. }));
    (word_key(&k) && sig.params.len() == 3 && returns_two).then_some(Locate { key: k })
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
        assert!(!word_key("String<31>"));
        assert!(!word_key("(u256, String<4>)"));
        assert!(!word_key("Pair"));
    }
}
