use fe_mir::{RuntimeBodyOrigins, RuntimeInstance, RuntimePackageBodyOrigins};

fn runtime_package_body_origin_rejects_raw_symbol<'db>(instance: RuntimeInstance<'db>) {
    let _ = RuntimePackageBodyOrigins::new("runtime:test", instance, RuntimeBodyOrigins::new());
}

fn main() {}
