use std::collections::{BTreeMap, BTreeSet};

use fe_compiler_protocol::InterfaceManifest;
use serde::{Deserialize, Serialize};

use crate::{AdapterFunction, AdapterPlan, ExtendedAttributesDef, TypeRef};

pub const ADAPTER_SELECTION_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportKind {
    Function,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequiredImport {
    pub module: String,
    pub name: String,
    pub kind: ImportKind,
}

/// One operation emitted by a generated adapter.
///
/// Dependencies are already normalized by binding generation. This keeps the
/// selector independent of Web IDL syntax and lets it compute a transitive
/// closure without reparsing definitions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdapterOperationMetadata {
    pub provider: String,
    pub module: String,
    pub name: String,
    pub kind: ImportKind,
    #[serde(default)]
    pub operations: Vec<RequiredImport>,
    #[serde(default)]
    pub resources: Vec<String>,
    #[serde(default)]
    pub types: Vec<String>,
    #[serde(default)]
    pub exposures: Vec<String>,
}

impl AdapterOperationMetadata {
    fn key(&self) -> RequiredImport {
        RequiredImport {
            module: self.module.clone(),
            name: self.name.clone(),
            kind: self.kind,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdapterSelectionManifest {
    pub version: u16,
    pub required_imports: Vec<RequiredImport>,
    pub providers: Vec<String>,
    pub operations: Vec<RequiredImport>,
    pub resources: Vec<String>,
    pub types: Vec<String>,
    pub exposures: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdapterSelectionError {
    MissingProvider(RequiredImport),
    AmbiguousProvider {
        import: RequiredImport,
        providers: Vec<String>,
    },
    MetadataNotStrictlySorted {
        provider: String,
        field: &'static str,
    },
}

pub fn adapter_operation_metadata(
    plan: &AdapterPlan,
    provider: &str,
) -> Vec<AdapterOperationMetadata> {
    let resource_names = plan
        .host_abi
        .resources
        .iter()
        .map(|resource| resource.name.as_str())
        .collect::<BTreeSet<_>>();
    let mut metadata = Vec::new();
    for namespace in &plan.namespaces {
        for function in &namespace.functions {
            metadata.push(operation_metadata(
                plan,
                provider,
                function,
                None,
                [&namespace.attributes, &function.attributes],
                &resource_names,
            ));
        }
    }
    for resource in &plan.resources {
        for function in &resource.functions {
            metadata.push(operation_metadata(
                plan,
                provider,
                function,
                Some(&resource.name),
                [&resource.attributes, &function.attributes],
                &resource_names,
            ));
        }
    }
    for iterator in &plan.iterators {
        let types = iterator_item_types(&iterator.item);
        for name in [&iterator.create_import, &iterator.next_import] {
            metadata.push(synthetic_operation_metadata(
                plan,
                provider,
                name,
                [&iterator.interface, &iterator.resource],
                &types,
            ));
        }
    }
    for iterator in &plan.async_iterators {
        let types = iterator_item_types(&iterator.item);
        for name in [
            &iterator.create_import,
            &iterator.next_import,
            &iterator.cancel_import,
            &iterator.drop_import,
        ] {
            metadata.push(synthetic_operation_metadata(
                plan,
                provider,
                name,
                [&iterator.interface, &iterator.resource],
                &types,
            ));
        }
    }
    for collection in &plan.collections {
        let (names, types) = collection_operations(collection);
        for name in names {
            metadata.push(synthetic_operation_metadata(
                plan,
                provider,
                name,
                [&collection.interface, &collection.interface],
                &types,
            ));
        }
    }
    metadata.sort_by(|left, right| {
        (&left.module, &left.name, &left.provider).cmp(&(
            &right.module,
            &right.name,
            &right.provider,
        ))
    });
    metadata
}

fn iterator_item_types(item: &crate::IteratorItemBinding) -> Vec<&TypeRef> {
    match item {
        crate::IteratorItemBinding::Value(value) => vec![value],
        crate::IteratorItemBinding::Entry { key, value, .. } => vec![key, value],
    }
}

fn collection_operations(collection: &crate::AdapterCollection) -> (Vec<&str>, Vec<&TypeRef>) {
    use crate::AdapterCollectionKind;
    let mut names = vec![
        collection.size_import.as_str(),
        collection.has_import.as_str(),
    ];
    let types = match &collection.kind {
        AdapterCollectionKind::ReadonlyMaplike {
            key,
            value,
            get_import,
        } => {
            names.push(get_import);
            vec![key, value]
        }
        AdapterCollectionKind::ReadonlySetlike { value } => vec![value],
        AdapterCollectionKind::MutableMaplike {
            key,
            value,
            get_import,
            set_import,
            delete_import,
            clear_import,
        } => {
            names.extend([
                get_import.as_str(),
                set_import.as_str(),
                delete_import.as_str(),
                clear_import.as_str(),
            ]);
            vec![key, value]
        }
        AdapterCollectionKind::MutableSetlike {
            value,
            add_import,
            delete_import,
            clear_import,
        } => {
            names.extend([
                add_import.as_str(),
                delete_import.as_str(),
                clear_import.as_str(),
            ]);
            vec![value]
        }
    };
    (names, types)
}

fn synthetic_operation_metadata(
    plan: &AdapterPlan,
    provider: &str,
    name: &str,
    resource_dependencies: [&String; 2],
    type_dependencies: &[&TypeRef],
) -> AdapterOperationMetadata {
    let mut named = BTreeSet::new();
    for type_ in type_dependencies {
        collect_named_type_refs(type_, &mut named);
    }
    let resource_names = plan
        .host_abi
        .resources
        .iter()
        .map(|resource| resource.name.as_str())
        .collect::<BTreeSet<_>>();
    let mut resources = resource_dependencies
        .into_iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    resources.extend(
        named
            .iter()
            .filter(|name| resource_names.contains(name.as_str()))
            .cloned(),
    );
    let mut types = named
        .into_iter()
        .filter(|name| !resource_names.contains(name.as_str()))
        .collect::<BTreeSet<_>>();
    expand_host_types(&plan.host_abi, &mut types, &mut resources);
    let exposures = plan
        .lowering
        .exposures
        .iter()
        .filter(|exposure| resources.contains(&exposure.definition))
        .filter_map(|exposure| exposure.attributes.exposed.as_ref())
        .flatten()
        .cloned()
        .collect::<BTreeSet<_>>();
    AdapterOperationMetadata {
        provider: provider.to_owned(),
        module: plan.module.clone(),
        name: name.to_owned(),
        kind: ImportKind::Function,
        operations: Vec::new(),
        resources: resources.into_iter().collect(),
        types: types.into_iter().collect(),
        exposures: exposures.into_iter().collect(),
    }
}

fn operation_metadata(
    plan: &AdapterPlan,
    provider: &str,
    function: &AdapterFunction,
    owner: Option<&str>,
    attributes: [&ExtendedAttributesDef; 2],
    resource_names: &BTreeSet<&str>,
) -> AdapterOperationMetadata {
    let mut named = BTreeSet::new();
    for parameter in &function.params {
        collect_named_type_refs(&parameter.type_, &mut named);
    }
    collect_named_type_refs(&function.result, &mut named);
    let mut resources = owner
        .into_iter()
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    resources.extend(
        named
            .iter()
            .filter(|name| resource_names.contains(name.as_str()))
            .cloned(),
    );
    let mut types = named
        .into_iter()
        .filter(|name| !resource_names.contains(name.as_str()))
        .collect::<BTreeSet<_>>();
    expand_host_types(&plan.host_abi, &mut types, &mut resources);
    let exposures = attributes
        .into_iter()
        .filter_map(|attributes| attributes.exposed.as_ref())
        .flatten()
        .cloned()
        .collect::<BTreeSet<_>>();
    AdapterOperationMetadata {
        provider: provider.to_owned(),
        module: plan.module.clone(),
        name: function.import_name.clone(),
        kind: ImportKind::Function,
        operations: Vec::new(),
        resources: resources.into_iter().collect(),
        types: types.into_iter().collect(),
        exposures: exposures.into_iter().collect(),
    }
}

fn collect_named_type_refs(type_: &TypeRef, output: &mut BTreeSet<String>) {
    match type_ {
        TypeRef::Named(name) => {
            output.insert(name.clone());
        }
        TypeRef::Nullable(inner)
        | TypeRef::Sequence(inner)
        | TypeRef::Promise(inner)
        | TypeRef::Record(inner) => collect_named_type_refs(inner, output),
        TypeRef::Union(members) => {
            for member in members {
                collect_named_type_refs(member, output);
            }
        }
        _ => {}
    }
}

fn expand_host_types(
    world: &fe_host_abi::World,
    types: &mut BTreeSet<String>,
    resources: &mut BTreeSet<String>,
) {
    let definitions = world
        .types
        .iter()
        .map(|definition| (definition.name.as_str(), &definition.kind))
        .collect::<BTreeMap<_, _>>();
    let resource_names = world
        .resources
        .iter()
        .map(|resource| resource.name.as_str())
        .collect::<BTreeSet<_>>();
    let mut pending = types.iter().cloned().collect::<Vec<_>>();
    while let Some(name) = pending.pop() {
        let Some(kind) = definitions.get(name.as_str()) else {
            continue;
        };
        let mut referenced = BTreeSet::new();
        collect_host_type_def(kind, &mut referenced);
        for referenced in referenced {
            if resource_names.contains(referenced.as_str()) {
                resources.insert(referenced);
            } else if types.insert(referenced.clone()) {
                pending.push(referenced);
            }
        }
    }
}

fn collect_host_type_def(kind: &fe_host_abi::TypeDefKind, output: &mut BTreeSet<String>) {
    use fe_host_abi::TypeDefKind;
    match kind {
        TypeDefKind::Alias { target } => collect_host_type(target, output),
        TypeDefKind::Record { fields } => {
            for field in fields {
                collect_host_type(&field.type_, output);
            }
        }
        TypeDefKind::Tuple { fields } => {
            for field in fields {
                collect_host_type(field, output);
            }
        }
        TypeDefKind::Variant { cases } => {
            for case in cases {
                if let Some(payload) = &case.payload {
                    collect_host_type(payload, output);
                }
            }
        }
        TypeDefKind::Callback { signature } => {
            for parameter in &signature.params {
                collect_host_type(&parameter.type_, output);
            }
            if let Some(result) = &signature.result {
                collect_host_type(result, output);
            }
        }
        TypeDefKind::Enum { .. } | TypeDefKind::Flags { .. } => {}
    }
}

fn collect_host_type(type_: &fe_host_abi::Type, output: &mut BTreeSet<String>) {
    use fe_host_abi::Type;
    match type_ {
        Type::Named(name) => {
            output.insert(name.clone());
        }
        Type::Handle(handle) => {
            output.insert(handle.resource.clone());
        }
        Type::List(inner) | Type::Option(inner) => collect_host_type(inner, output),
        Type::Result(result) => {
            if let Some(ok) = &result.ok {
                collect_host_type(ok, output);
            }
            if let Some(error) = &result.error {
                collect_host_type(error, output);
            }
        }
        Type::Future(inner) | Type::Stream(inner) => {
            if let Some(inner) = inner {
                collect_host_type(inner, output);
            }
        }
        _ => {}
    }
}

impl std::fmt::Display for AdapterSelectionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for AdapterSelectionError {}

pub fn select_adapter_operations(
    interface: &InterfaceManifest,
    metadata: &[AdapterOperationMetadata],
) -> Result<AdapterSelectionManifest, AdapterSelectionError> {
    let mut index = BTreeMap::<RequiredImport, Vec<&AdapterOperationMetadata>>::new();
    for operation in metadata {
        ensure_sorted(&operation.operations, &operation.provider, "operations")?;
        ensure_sorted(&operation.resources, &operation.provider, "resources")?;
        ensure_sorted(&operation.types, &operation.provider, "types")?;
        ensure_sorted(&operation.exposures, &operation.provider, "exposures")?;
        index.entry(operation.key()).or_default().push(operation);
    }
    for providers in index.values_mut() {
        providers.sort_by(|left, right| left.provider.cmp(&right.provider));
    }

    // InterfaceFunction is specifically a function inventory, so its kind is
    // exact even though the protocol does not repeat a redundant kind field.
    let mut required_imports = interface
        .imports
        .iter()
        .map(|function| RequiredImport {
            module: function.module.clone(),
            name: function.name.clone(),
            kind: ImportKind::Function,
        })
        .collect::<Vec<_>>();
    required_imports.sort();
    required_imports.dedup();

    let mut pending = required_imports.iter().cloned().collect::<BTreeSet<_>>();
    let mut operations = BTreeSet::new();
    let mut providers = BTreeSet::new();
    let mut resources = BTreeSet::new();
    let mut types = BTreeSet::new();
    let mut exposures = BTreeSet::new();
    while let Some(required) = pending.pop_first() {
        if !operations.insert(required.clone()) {
            continue;
        }
        let matches = index
            .get(&required)
            .ok_or_else(|| AdapterSelectionError::MissingProvider(required.clone()))?;
        let provider_names = matches
            .iter()
            .map(|operation| operation.provider.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        if provider_names.len() != 1 || matches.len() != 1 {
            return Err(AdapterSelectionError::AmbiguousProvider {
                import: required,
                providers: provider_names,
            });
        }
        let selected = matches[0];
        providers.insert(selected.provider.clone());
        pending.extend(selected.operations.iter().cloned());
        resources.extend(selected.resources.iter().cloned());
        types.extend(selected.types.iter().cloned());
        exposures.extend(selected.exposures.iter().cloned());
    }

    Ok(AdapterSelectionManifest {
        version: ADAPTER_SELECTION_VERSION,
        required_imports,
        providers: providers.into_iter().collect(),
        operations: operations.into_iter().collect(),
        resources: resources.into_iter().collect(),
        types: types.into_iter().collect(),
        exposures: exposures.into_iter().collect(),
    })
}

fn ensure_sorted<T: Ord>(
    values: &[T],
    provider: &str,
    field: &'static str,
) -> Result<(), AdapterSelectionError> {
    if values.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(AdapterSelectionError::MetadataNotStrictlySorted {
            provider: provider.to_owned(),
            field,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use fe_compiler_protocol::InterfaceFunction;

    use super::*;

    fn import(module: &str, name: &str) -> RequiredImport {
        RequiredImport {
            module: module.to_owned(),
            name: name.to_owned(),
            kind: ImportKind::Function,
        }
    }

    fn interface(imports: &[(&str, &str)]) -> InterfaceManifest {
        InterfaceManifest {
            imports: imports
                .iter()
                .map(|(module, name)| InterfaceFunction {
                    module: (*module).to_owned(),
                    name: (*name).to_owned(),
                    signature_complete: false,
                    params: Vec::new(),
                    results: Vec::new(),
                })
                .collect(),
            ..InterfaceManifest::default()
        }
    }

    #[test]
    fn selects_deterministic_transitive_closure() {
        let metadata = vec![
            AdapterOperationMetadata {
                provider: "dom-adapter".to_owned(),
                module: "fe:web".to_owned(),
                name: "window-document".to_owned(),
                kind: ImportKind::Function,
                operations: vec![import("fe:web", "document-title")],
                resources: vec!["Document".to_owned(), "Window".to_owned()],
                types: vec!["DomString".to_owned()],
                exposures: vec!["Window".to_owned()],
            },
            AdapterOperationMetadata {
                provider: "dom-adapter".to_owned(),
                module: "fe:web".to_owned(),
                name: "document-title".to_owned(),
                kind: ImportKind::Function,
                operations: Vec::new(),
                resources: vec!["Document".to_owned()],
                types: vec!["DomString".to_owned()],
                exposures: vec!["Window".to_owned()],
            },
        ];
        let selected =
            select_adapter_operations(&interface(&[("fe:web", "window-document")]), &metadata)
                .unwrap();
        assert_eq!(selected.version, 1);
        assert_eq!(
            selected.operations,
            [
                import("fe:web", "document-title"),
                import("fe:web", "window-document")
            ]
        );
        assert_eq!(selected.resources, ["Document", "Window"]);
        assert_eq!(selected.types, ["DomString"]);
        assert_eq!(selected.exposures, ["Window"]);
    }

    #[test]
    fn missing_and_ambiguous_providers_fail_closed() {
        let required = interface(&[("fe:web", "console-log")]);
        assert!(matches!(
            select_adapter_operations(&required, &[]),
            Err(AdapterSelectionError::MissingProvider(_))
        ));
        let duplicate = AdapterOperationMetadata {
            provider: "a".to_owned(),
            module: "fe:web".to_owned(),
            name: "console-log".to_owned(),
            kind: ImportKind::Function,
            operations: Vec::new(),
            resources: Vec::new(),
            types: Vec::new(),
            exposures: Vec::new(),
        };
        let mut other = duplicate.clone();
        other.provider = "b".to_owned();
        assert!(matches!(
            select_adapter_operations(&required, &[duplicate, other]),
            Err(AdapterSelectionError::AmbiguousProvider { .. })
        ));
    }

    #[test]
    fn generated_plan_metadata_carries_resource_type_and_exposure_dependencies() {
        let world = crate::parse(
            r#"
                typedef DOMString Label;
                [Exposed=Window] interface Window {
                    Label title();
                };
            "#,
        )
        .unwrap();
        let plan = crate::build_adapter_plan(&world, "browser", "fe:web").unwrap();
        let metadata = adapter_operation_metadata(&plan, "generated-web");
        let operation = metadata
            .iter()
            .find(|operation| operation.name.contains("title"))
            .unwrap();
        assert_eq!(operation.module, "fe:web");
        assert_eq!(operation.resources, ["Window"]);
        assert!(operation.types.contains(&"Label".to_owned()));
        assert_eq!(operation.exposures, ["Window"]);
    }

    #[test]
    fn selected_adapter_emission_omits_unselected_operations_byte_identically() {
        let world = crate::parse(
            r#"
                [Exposed=Window] interface Console {
                    undefined log(DOMString value);
                    undefined warn(DOMString value);
                };
            "#,
        )
        .unwrap();
        let plan = crate::build_adapter_plan(&world, "browser", "fe:web").unwrap();
        let metadata = adapter_operation_metadata(&plan, "generated-web");
        let selected =
            select_adapter_operations(&interface(&[("fe:web", "console_log")]), &metadata).unwrap();
        let first =
            crate::emit_js_selected_adapter(&world, &plan, "generated-web", &selected).unwrap();
        let second =
            crate::emit_js_selected_adapter(&world, &plan, "generated-web", &selected).unwrap();
        assert_eq!(first, second);
        assert!(first.contains("\"console_log\""));
        assert!(!first.contains("\"console_warn\""));
        assert!(!first.contains("runtime.callbacks"));
        assert!(!first.contains("runtime.futures"));
    }
}
