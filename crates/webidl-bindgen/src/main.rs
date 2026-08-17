use std::env;
use std::fs;
use std::path::PathBuf;

use fe_webidl_bindgen::{
    build_adapter_plan, emit_fe_flat_host_imports, emit_fe_raw, emit_js_adapter,
    emit_js_canonical_adapter, parse,
};

fn usage() -> &'static str {
    "usage: fe-webidl-bindgen <input.webidl> [--fe-out <raw.fe>] \
     [--flat-fe-out <raw.fe>] [--js-out <v0.js>] \
     [--canonical-js-out <adapter.js>] [--abi-out <world.json>] \
     [--module <name>] [--world <name>]"
}

fn main() {
    if let Err(error) = run() {
        eprintln!("fe-webidl-bindgen: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args_os().skip(1);
    let input = args
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| usage().to_owned())?;
    let mut fe_out = None;
    let mut flat_fe_out = None;
    let mut js_out = None;
    let mut canonical_js_out = None;
    let mut abi_out = None;
    let mut module = "fe:web".to_owned();
    let mut world_name = "web".to_owned();

    while let Some(flag) = args.next() {
        let value = args
            .next()
            .ok_or_else(|| format!("missing value for {}; {}", flag.to_string_lossy(), usage()))?;
        match flag.to_str() {
            Some("--fe-out") => fe_out = Some(PathBuf::from(value)),
            Some("--flat-fe-out") => flat_fe_out = Some(PathBuf::from(value)),
            Some("--js-out") => js_out = Some(PathBuf::from(value)),
            Some("--canonical-js-out") => canonical_js_out = Some(PathBuf::from(value)),
            Some("--abi-out") => abi_out = Some(PathBuf::from(value)),
            Some("--module") => {
                module = value
                    .into_string()
                    .map_err(|_| "--module must be valid UTF-8".to_owned())?;
            }
            Some("--world") => {
                world_name = value
                    .into_string()
                    .map_err(|_| "--world must be valid UTF-8".to_owned())?;
            }
            _ => {
                return Err(format!(
                    "unknown option {}; {}",
                    flag.to_string_lossy(),
                    usage()
                ));
            }
        }
    }

    if fe_out.is_none()
        && flat_fe_out.is_none()
        && js_out.is_none()
        && canonical_js_out.is_none()
        && abi_out.is_none()
    {
        return Err(format!("at least one output is required; {}", usage()));
    }
    let source = fs::read_to_string(&input)
        .map_err(|error| format!("could not read {}: {error}", input.display()))?;
    let world = parse(&source).map_err(|error| error.to_string())?;
    if let Some(path) = fe_out {
        let fe = emit_fe_raw(&world, &module).map_err(|error| error.to_string())?;
        fs::write(&path, fe)
            .map_err(|error| format!("could not write {}: {error}", path.display()))?;
    }
    if let Some(path) = flat_fe_out {
        let fe = emit_fe_flat_host_imports(&world, &module).map_err(|error| error.to_string())?;
        fs::write(&path, fe)
            .map_err(|error| format!("could not write {}: {error}", path.display()))?;
    }
    if let Some(path) = js_out {
        let js = emit_js_adapter(&world, &module).map_err(|error| error.to_string())?;
        fs::write(&path, js)
            .map_err(|error| format!("could not write {}: {error}", path.display()))?;
    }
    if canonical_js_out.is_some() || abi_out.is_some() {
        let plan =
            build_adapter_plan(&world, &world_name, &module).map_err(|error| error.to_string())?;
        if let Some(path) = canonical_js_out {
            let js = emit_js_canonical_adapter(&world, &plan).map_err(|error| error.to_string())?;
            fs::write(&path, js)
                .map_err(|error| format!("could not write {}: {error}", path.display()))?;
        }
        if let Some(path) = abi_out {
            let json = serde_json::to_vec_pretty(&plan.host_abi)
                .map_err(|error| format!("could not serialize host ABI: {error}"))?;
            fs::write(&path, json)
                .map_err(|error| format!("could not write {}: {error}", path.display()))?;
        }
    }
    Ok(())
}
