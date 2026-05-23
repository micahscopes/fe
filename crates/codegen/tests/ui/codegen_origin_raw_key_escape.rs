use fe_codegen::origin::{
    BytecodeObjectKey, BytecodePcOrigin, BytecodePcRange, BytecodeSectionKey,
    BytecodeSectionNameKey, SonatinaInstOrigin,
};
use sonatina_ir::{InstId, module::FuncRef};

fn main() {
    let inst = SonatinaInstOrigin::pre_opt(FuncRef::from_u32(0), InstId::from_u32(7));
    let _ = inst.key();

    let section = BytecodeSectionKey::new(
        BytecodeObjectKey::new("Foo"),
        BytecodeSectionNameKey::new("runtime"),
    );
    let range = BytecodePcRange::new(0, 4).expect("non-empty PC range");
    let pc = BytecodePcOrigin::new(section, range);
    let _ = pc.key();
}
