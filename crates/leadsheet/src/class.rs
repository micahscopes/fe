//! Runtime classes as words.
//!
//! - **Values.** A scalar, a reference or a raw address is one word; an
//!   aggregate value is the words of its leaves in order (an enum: its tag,
//!   then the largest variant's payload words), as SPEC.md asks producers to
//!   flatten sums and products.
//! - **Objects.** A memory object (an object reference, a slot root, a memory
//!   provider) holds the same words, 32 bytes each, in the same order. Safe Fe
//!   reaches objects only through typed places, and Sonatina lays them out
//!   itself, so the layout is not observable; a word per leaf keeps every
//!   access a whole `mload`/`mstore` inside the object's footprint.
//! - **Raw memory and storage** keep Fe's own layout (`RuntimeMemoryLayout`),
//!   since library code computes raw addresses with it.
//! - **Scalars** are kept canonical for their Sonatina type, which is what each
//!   class lowers to (`scalar_ty` in `codegen/src/sonatina/lower_runtime.rs`):
//!   an unsigned integer zero-extended, a signed one sign-extended, a bool 0 or
//!   1, an address the whole word. Every conversion Sonatina makes between
//!   these types is made here on words, so arithmetic wraps and checks
//!   overflow at the same width.

use mir::{Layout, MirDb, RuntimeClass, ScalarClass, ScalarRepr, ScalarRole, VariantId};
use ruint::aliases::U256;

use crate::text::{Builder, V};

/// A scalar's Sonatina type: its width in bits and how its words are kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sc {
    pub width: u16,
    pub signed: bool,
}

impl Sc {
    pub const WORD: Sc = Sc { width: 256, signed: false };
    pub const BOOL: Sc = Sc { width: 1, signed: false };

    pub fn of(scalar: &ScalarClass<'_>) -> Sc {
        let width = |bits: u16| match bits {
            0 | 1 => 1,
            2..=8 => 8,
            9..=16 => 16,
            17..=32 => 32,
            33..=64 => 64,
            65..=128 => 128,
            _ => 256,
        };
        match (scalar.repr, &scalar.role) {
            (ScalarRepr::Bool, _) => Sc::BOOL,
            (ScalarRepr::Int { bits, signed }, ScalarRole::Plain) => Sc { width: width(bits), signed: signed && width(bits) > 1 },
            (ScalarRepr::Int { bits, .. }, ScalarRole::EnumTag { .. }) => Sc { width: width(bits).max(8), signed: false },
            (ScalarRepr::FixedBytes { len }, _) => Sc { width: width(len.saturating_mul(8)), signed: false },
            (ScalarRepr::Address { .. }, _) => Sc::WORD,
        }
    }

    /// A scalar class's type; references and raw addresses are words.
    pub fn of_class(class: &RuntimeClass<'_>) -> Option<Sc> {
        match class {
            RuntimeClass::Scalar(s) => Some(Sc::of(s)),
            RuntimeClass::Ref { .. } | RuntimeClass::RawAddr { .. } => Some(Sc::WORD),
            RuntimeClass::AggregateValue { .. } => None,
        }
    }

    pub fn is_bool(self) -> bool {
        self.width == 1
    }

    fn mask(self) -> U256 {
        if self.width >= 256 { U256::MAX } else { (U256::from(1u64) << self.width as usize) - U256::from(1u64) }
    }

    /// The lowest value of a signed type, as a canonical (sign-extended) word.
    pub fn signed_min(self) -> U256 {
        let half: U256 = self.mask() >> 1usize;
        !half
    }

    pub fn max(self) -> U256 {
        if self.signed { self.mask() >> 1usize } else { self.mask() }
    }

    /// `x` canonical for this type, from any word whose low `width` bits are
    /// the value.
    pub fn canon(self, b: &mut Builder, x: V) -> V {
        if self.width >= 256 {
            return x;
        }
        if self.is_bool() {
            let z = b.w("iszero", &[x]);
            return b.w("iszero", &[z]);
        }
        if self.signed {
            let k = b.k(u64::from(self.width / 8 - 1));
            b.w("signextend", &[k, x])
        } else {
            let m = b.konst(self.mask());
            b.w("and", &[x, m])
        }
    }

    /// A word canonical for this type, computed here.
    pub fn canon_word(self, x: U256) -> U256 {
        if self.width >= 256 {
            return x;
        }
        if self.is_bool() {
            return U256::from(!x.is_zero() as u64);
        }
        let low = x & self.mask();
        if self.signed && low.bit(self.width as usize - 1) { low | !self.mask() } else { low }
    }

    /// As `canon`, for a value whose word is already 0 or 1 when this is a
    /// bool (a comparison's result).
    pub fn canon_cmp(self, b: &mut Builder, x: V) -> V {
        if self.is_bool() { x } else { self.canon(b, x) }
    }
}

/// Sonatina's `cast_scalar_with_signedness`: the canonical word of `x` (of
/// type `from`) converted to `to`; a widening extends the sign when `signed`.
/// A bool is the test `x != 0`.
pub fn convert(b: &mut Builder, x: V, from: Sc, to: Sc, signed: bool) -> V {
    if from == to {
        return x;
    }
    if to.is_bool() {
        let z = b.w("iszero", &[x]);
        return b.w("iszero", &[z]);
    }
    if from.width >= to.width {
        return to.canon(b, x);
    }
    let extended = match (signed && !from.is_bool(), from.signed) {
        (true, true) | (false, false) => x,
        (true, false) => {
            let k = b.k(u64::from(from.width / 8 - 1));
            b.w("signextend", &[k, x])
        }
        (false, true) => {
            let m = b.konst(from.mask());
            b.w("and", &[x, m])
        }
    };
    to.canon(b, extended)
}

/// Sonatina's `coerce_value_to_ty`, which casts without sign extension.
pub fn coerce(b: &mut Builder, x: V, from: Sc, to: Sc) -> V {
    convert(b, x, from, to, false)
}

/// The number of words a value of `class` is.
pub fn flat_len(db: &dyn MirDb, class: &RuntimeClass<'_>) -> usize {
    match class {
        RuntimeClass::Scalar(_) | RuntimeClass::Ref { .. } | RuntimeClass::RawAddr { .. } => 1,
        RuntimeClass::AggregateValue { layout } => match layout.data(db) {
            Layout::Struct(s) => s.fields.iter().map(|f| flat_len(db, f)).sum(),
            Layout::Array(a) => a.len as usize * flat_len(db, &a.elem),
            Layout::Enum(e) => 1 + e.variants.iter().map(|v| v.fields.iter().map(|f| flat_len(db, f)).sum::<usize>()).max().unwrap_or(0),
        },
    }
}

/// The bytes an object of `class` takes: a word per leaf.
pub fn obj_size(db: &dyn MirDb, class: &RuntimeClass<'_>) -> u64 {
    32 * flat_len(db, class) as u64
}

/// Where a struct field's or array element's words start among its
/// aggregate's words.
pub fn element_word(db: &dyn MirDb, class: &RuntimeClass<'_>, index: usize) -> usize {
    let RuntimeClass::AggregateValue { layout } = class else { return 0 };
    match layout.data(db) {
        Layout::Struct(s) => s.fields[..index].iter().map(|f| flat_len(db, f)).sum(),
        Layout::Array(a) => index * flat_len(db, &a.elem),
        Layout::Enum(_) => 0,
    }
}

/// The class of a struct field or array element.
pub fn element_class<'db>(db: &'db dyn MirDb, class: &RuntimeClass<'db>, index: usize) -> RuntimeClass<'db> {
    let RuntimeClass::AggregateValue { layout } = class else { panic!("element of a non-aggregate") };
    match layout.data(db) {
        Layout::Struct(s) => s.fields[index].clone(),
        Layout::Array(a) => a.elem.clone(),
        Layout::Enum(_) => panic!("element of an enum"),
    }
}

/// Where a variant's field starts among its enum's words (after the tag).
pub fn variant_field_word(db: &dyn MirDb, variant: VariantId<'_>, field: usize) -> usize {
    let Layout::Enum(e) = variant.enum_layout.data(db) else { return 0 };
    1 + e.variants[variant.index as usize].fields[..field].iter().map(|f| flat_len(db, f)).sum::<usize>()
}
