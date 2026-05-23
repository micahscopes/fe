use fe_mir::{RBlockId, RuntimeInstance, RuntimeTerminatorOrigin};

fn runtime_terminator_origin_rejects_raw_block<'db>(instance: RuntimeInstance<'db>) {
    let _ = RuntimeTerminatorOrigin::new(instance, RBlockId::from_u32(0));
}

fn main() {}
