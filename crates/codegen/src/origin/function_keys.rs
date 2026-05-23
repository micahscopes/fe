use std::{collections::BTreeMap, fmt};

use common::origin::OriginLink;
use mir::RuntimeOriginOwnerKey;
use sonatina_ir::module::FuncRef;

common::define_origin_owner_key! {
    pub struct SonatinaFunctionExportKey;
}

impl RuntimeOriginOwnerKey for SonatinaFunctionExportKey {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MissingSonatinaFunctionKey {
    function: FuncRef,
}

impl MissingSonatinaFunctionKey {
    pub const fn new(function: FuncRef) -> Self {
        Self { function }
    }

    pub const fn function(self) -> FuncRef {
        self.function
    }
}

impl fmt::Display for MissingSonatinaFunctionKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "missing stable Sonatina function key for func{}",
            self.function.as_u32()
        )
    }
}

impl std::error::Error for MissingSonatinaFunctionKey {}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct SonatinaFunctionKeyMap {
    keys: BTreeMap<FuncRef, SonatinaFunctionExportKey>,
}

impl SonatinaFunctionKeyMap {
    fn resolve_function(
        &mut self,
        function: FuncRef,
        stable_function_key: &mut impl FnMut(FuncRef) -> Option<SonatinaFunctionExportKey>,
    ) -> Result<(), MissingSonatinaFunctionKey> {
        if self.keys.contains_key(&function) {
            return Ok(());
        }

        let Some(key) = stable_function_key(function) else {
            return Err(MissingSonatinaFunctionKey::new(function));
        };
        self.keys.insert(function, key);
        Ok(())
    }

    pub(super) fn get(
        &self,
        function: FuncRef,
    ) -> Result<&SonatinaFunctionExportKey, MissingSonatinaFunctionKey> {
        self.keys
            .get(&function)
            .ok_or_else(|| MissingSonatinaFunctionKey::new(function))
    }

    pub(super) fn get_optional(&self, function: FuncRef) -> Option<&SonatinaFunctionExportKey> {
        self.keys.get(&function)
    }
}

pub(super) fn collect_sonatina_function_keys<'a, Node: 'a>(
    links: impl IntoIterator<Item = &'a OriginLink<Node>>,
    stable_function_key: &mut impl FnMut(FuncRef) -> Option<SonatinaFunctionExportKey>,
    node_function: impl Fn(&Node) -> Option<FuncRef>,
) -> Result<SonatinaFunctionKeyMap, MissingSonatinaFunctionKey> {
    let mut function_keys = SonatinaFunctionKeyMap::default();
    for link in links {
        if let Some(function) = node_function(link.from()) {
            function_keys.resolve_function(function, stable_function_key)?;
        }
        if let Some(function) = node_function(link.to()) {
            function_keys.resolve_function(function, stable_function_key)?;
        }
    }
    Ok(function_keys)
}
