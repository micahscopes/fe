/// Byte order of a target's word representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Endianness {
    Big,
    Little,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TargetDataLayout {
    pub word_size_bytes: usize,
    pub discriminant_size_bytes: usize,
    /// Width of a pointer/reference on this target, in bytes.
    pub pointer_size_bytes: usize,
    /// Byte order of the target's word representation.
    pub endianness: Endianness,
}

impl TargetDataLayout {
    /// EVM target: 32-byte (256-bit) big-endian words; pointers are full words.
    pub const fn evm() -> Self {
        Self {
            word_size_bytes: 32,
            discriminant_size_bytes: 1,
            pointer_size_bytes: 32,
            endianness: Endianness::Big,
        }
    }

    /// Wasm target: 8-byte (64-bit) little-endian words with 32-bit (u32)
    /// linear-memory pointers.
    ///
    /// Purely additive: nothing consumes this yet except
    /// `fe_codegen::layout_for(BackendKind::Wasm)`. It carries no lowering
    /// authority until a wasm-capable Sonatina ISA is pinned.
    pub const fn wasm() -> Self {
        Self {
            word_size_bytes: 8,
            discriminant_size_bytes: 1,
            pointer_size_bytes: 4,
            endianness: Endianness::Little,
        }
    }
}

pub const EVM_LAYOUT: TargetDataLayout = TargetDataLayout::evm();
pub const WASM_LAYOUT: TargetDataLayout = TargetDataLayout::wasm();
pub const WORD_SIZE_BYTES: usize = EVM_LAYOUT.word_size_bytes;
pub const DISCRIMINANT_SIZE_BYTES: usize = EVM_LAYOUT.discriminant_size_bytes;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evm_layout_fields() {
        assert_eq!(EVM_LAYOUT.word_size_bytes, 32);
        assert_eq!(EVM_LAYOUT.discriminant_size_bytes, 1);
        assert_eq!(EVM_LAYOUT.pointer_size_bytes, 32);
        assert_eq!(EVM_LAYOUT.endianness, Endianness::Big);
        // The derived compatibility constants must not move.
        assert_eq!(WORD_SIZE_BYTES, 32);
        assert_eq!(DISCRIMINANT_SIZE_BYTES, 1);
    }

    #[test]
    fn wasm_layout_fields() {
        assert_eq!(WASM_LAYOUT.word_size_bytes, 8);
        assert_eq!(WASM_LAYOUT.discriminant_size_bytes, 1);
        assert_eq!(WASM_LAYOUT.pointer_size_bytes, 4);
        assert_eq!(WASM_LAYOUT.endianness, Endianness::Little);
    }
}
