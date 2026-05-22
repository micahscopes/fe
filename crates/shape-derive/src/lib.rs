use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{Data, DeriveInput, Fields, LitStr};

#[proc_macro_derive(ShapeDescribe, attributes(shape))]
pub fn derive_shape_describe(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as DeriveInput);
    match derive_impl(input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

#[derive(Default)]
struct ContainerAttrs {
    kind: Option<String>,
    stable_key: Option<syn::Path>,
}

#[derive(Default)]
struct FieldAttrs {
    mode: Option<FieldMode>,
    label: Option<String>,
}

enum FieldMode {
    Field(TokenStream2),
    Child,
    With(syn::Path),
    Skip(String),
}

fn parse_container_attrs(attrs: &[syn::Attribute]) -> syn::Result<ContainerAttrs> {
    let mut result = ContainerAttrs::default();
    for attr in attrs {
        if !attr.path().is_ident("shape") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("kind") {
                if result.kind.is_some() {
                    return Err(meta.error("duplicate shape kind attribute"));
                }
                result.kind = Some(meta.value()?.parse::<LitStr>()?.value());
                Ok(())
            } else if meta.path.is_ident("stable_key") {
                if result.stable_key.is_some() {
                    return Err(meta.error("duplicate shape stable_key attribute"));
                }
                result.stable_key = Some(meta.value()?.parse()?);
                Ok(())
            } else {
                Err(meta.error("unknown shape attribute on item or variant"))
            }
        })?;
    }
    Ok(result)
}

fn parse_field_attrs(field: &syn::Field) -> syn::Result<FieldAttrs> {
    let mut result = FieldAttrs::default();
    let mut saw_shape_attr = false;

    for attr in &field.attrs {
        if !attr.path().is_ident("shape") {
            continue;
        }
        saw_shape_attr = true;
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("field") || meta.path.is_ident("dim") {
                set_mode(
                    field,
                    &mut result.mode,
                    FieldMode::Field(parse_dimension(meta.value()?.parse()?)?),
                )
            } else if meta.path.is_ident("child") {
                set_mode(field, &mut result.mode, FieldMode::Child)
            } else if meta.path.is_ident("with") {
                set_mode(
                    field,
                    &mut result.mode,
                    FieldMode::With(meta.value()?.parse()?),
                )
            } else if meta.path.is_ident("skip") {
                let reason = meta.value()?.parse::<LitStr>()?.value();
                if reason.trim().is_empty() {
                    return Err(meta.error("shape skip reason must not be empty"));
                }
                set_mode(field, &mut result.mode, FieldMode::Skip(reason))
            } else if meta.path.is_ident("label") {
                if result.label.is_some() {
                    return Err(meta.error("duplicate shape label attribute"));
                }
                result.label = Some(meta.value()?.parse::<LitStr>()?.value());
                Ok(())
            } else {
                Err(meta.error("unknown shape attribute on field"))
            }
        })?;
    }

    if !saw_shape_attr {
        let field_name = field_name(field);
        return Err(syn::Error::new_spanned(
            field,
            format!(
                "field `{field_name}` is missing #[shape(...)] policy; use \
                 #[shape(field = Structure)], #[shape(child)], #[shape(with = path)], \
                 or #[shape(skip = \"reason\")]"
            ),
        ));
    }

    if result.mode.is_none() {
        return Err(syn::Error::new_spanned(
            field,
            "shape field attribute must declare field, child, with, or skip policy",
        ));
    }

    Ok(result)
}

fn set_mode(field: &syn::Field, slot: &mut Option<FieldMode>, mode: FieldMode) -> syn::Result<()> {
    if slot.is_some() {
        return Err(syn::Error::new_spanned(
            field,
            "shape field has multiple policies; choose exactly one of field, child, with, or skip",
        ));
    }
    *slot = Some(mode);
    Ok(())
}

fn parse_dimension(ident: syn::Ident) -> syn::Result<TokenStream2> {
    match ident.to_string().as_str() {
        "Structure" | "Names" | "Constants" | "Types" | "TraceEvents" => {
            Ok(quote! { ::common::shape::ShapeDimension::#ident })
        }
        _ => Err(syn::Error::new_spanned(
            ident,
            "unknown shape dimension; expected one of Structure, Names, Constants, Types, TraceEvents",
        )),
    }
}

fn field_name(field: &syn::Field) -> String {
    field
        .ident
        .as_ref()
        .map(ToString::to_string)
        .unwrap_or_else(|| "unnamed".to_string())
}

fn emit_field(access: &TokenStream2, label: &str, attrs: FieldAttrs) -> TokenStream2 {
    match attrs.mode.expect("field mode should be checked") {
        FieldMode::Field(dimension) => quote! {
            __builder.add_field_value(__node, #dimension, #label, #access);
        },
        FieldMode::Child => quote! {
            __builder.add_child_node(__node, #label, #access);
        },
        FieldMode::With(path) => quote! {
            #path(__builder, __node, #label, #access);
        },
        FieldMode::Skip(reason) => {
            let _ = reason;
            quote! {}
        }
    }
}

fn derive_impl(input: DeriveInput) -> syn::Result<TokenStream2> {
    let name = &input.ident;
    let attrs = parse_container_attrs(&input.attrs)?;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let body = match &input.data {
        Data::Struct(data) => {
            let kind = attrs.kind.unwrap_or_else(|| name.to_string());
            derive_struct_body(&data.fields, &kind, attrs.stable_key.as_ref())?
        }
        Data::Enum(data) => derive_enum_body(name, data, attrs.stable_key.as_ref())?,
        Data::Union(_) => {
            return Err(syn::Error::new_spanned(
                name,
                "ShapeDescribe cannot be derived for unions",
            ));
        }
    };

    Ok(quote! {
        impl #impl_generics ::common::shape::ShapeDescribe for #name #ty_generics #where_clause {
            fn describe_shape(
                &self,
                __builder: &mut ::common::shape::ShapeBuilder,
            ) -> ::common::shape::ShapeNodeId {
                #body
            }
        }
    })
}

fn stable_key_expr(stable_key: Option<&syn::Path>) -> TokenStream2 {
    match stable_key {
        Some(path) => quote! { Some(#path(self).into()) },
        None => quote! { None },
    }
}

fn derive_struct_body(
    fields: &Fields,
    kind: &str,
    stable_key: Option<&syn::Path>,
) -> syn::Result<TokenStream2> {
    let stable_key = stable_key_expr(stable_key);
    let field_emissions = emit_struct_fields(fields)?;
    Ok(quote! {
        let __node = __builder.add_described_node(#kind, #stable_key);
        #(#field_emissions)*
        __node
    })
}

fn emit_struct_fields(fields: &Fields) -> syn::Result<Vec<TokenStream2>> {
    let mut emissions = Vec::new();
    match fields {
        Fields::Named(fields) => {
            for field in &fields.named {
                let attrs = parse_field_attrs(field)?;
                let field_name = field.ident.as_ref().unwrap();
                let label = attrs
                    .label
                    .clone()
                    .unwrap_or_else(|| field_name.to_string());
                let access = quote! { &self.#field_name };
                emissions.push(emit_field(&access, &label, attrs));
            }
        }
        Fields::Unnamed(fields) => {
            for (idx, field) in fields.unnamed.iter().enumerate() {
                let attrs = parse_field_attrs(field)?;
                let field_idx = syn::Index::from(idx);
                let label = attrs.label.clone().unwrap_or_else(|| idx.to_string());
                let access = quote! { &self.#field_idx };
                emissions.push(emit_field(&access, &label, attrs));
            }
        }
        Fields::Unit => {}
    }
    Ok(emissions)
}

fn derive_enum_body(
    enum_name: &syn::Ident,
    data: &syn::DataEnum,
    stable_key: Option<&syn::Path>,
) -> syn::Result<TokenStream2> {
    let mut arms = Vec::new();
    for variant in &data.variants {
        let variant_name = &variant.ident;
        let variant_attrs = parse_container_attrs(&variant.attrs)?;
        let kind = variant_attrs
            .kind
            .unwrap_or_else(|| format!("{enum_name}::{variant_name}"));
        let stable_key = stable_key_expr(variant_attrs.stable_key.as_ref().or(stable_key));

        let (pattern, emissions) = match &variant.fields {
            Fields::Named(fields) => {
                let mut pattern_fields = Vec::new();
                let mut emissions = Vec::new();
                for field in &fields.named {
                    let field_name = field.ident.as_ref().unwrap();
                    let attrs = parse_field_attrs(field)?;
                    if matches!(&attrs.mode, Some(FieldMode::Skip(_))) {
                        pattern_fields.push(quote! { #field_name: _ });
                        continue;
                    }
                    pattern_fields.push(quote! { #field_name });
                    let label = attrs
                        .label
                        .clone()
                        .unwrap_or_else(|| field_name.to_string());
                    let access = quote! { #field_name };
                    emissions.push(emit_field(&access, &label, attrs));
                }
                (
                    quote! { Self::#variant_name { #(#pattern_fields),* } },
                    emissions,
                )
            }
            Fields::Unnamed(fields) => {
                let mut pattern_fields = Vec::new();
                let mut emissions = Vec::new();
                for (idx, field) in fields.unnamed.iter().enumerate() {
                    let binding =
                        syn::Ident::new(&format!("__field{idx}"), proc_macro2::Span::call_site());
                    let attrs = parse_field_attrs(field)?;
                    if matches!(&attrs.mode, Some(FieldMode::Skip(_))) {
                        pattern_fields.push(quote! { _ });
                        continue;
                    }
                    pattern_fields.push(quote! { #binding });
                    let label = attrs.label.clone().unwrap_or_else(|| idx.to_string());
                    let access = quote! { #binding };
                    emissions.push(emit_field(&access, &label, attrs));
                }
                (
                    quote! { Self::#variant_name(#(#pattern_fields),*) },
                    emissions,
                )
            }
            Fields::Unit => (quote! { Self::#variant_name }, Vec::new()),
        };

        arms.push(quote! {
            #pattern => {
                let __node = __builder.add_described_node(#kind, #stable_key);
                #(#emissions)*
                __node
            }
        });
    }

    Ok(quote! {
        match self {
            #(#arms)*
        }
    })
}

#[cfg(test)]
mod tests {
    use super::derive_impl;
    use quote::quote;
    use syn::parse_quote;

    #[test]
    fn missing_field_policy_is_rejected() {
        let input = parse_quote! {
            struct MissingPolicy {
                value: u32,
            }
        };

        let err = derive_impl(input).expect_err("missing policy should fail");
        assert!(err.to_string().contains("missing #[shape(...)] policy"));
    }

    #[test]
    fn skip_policy_requires_reason() {
        let input = parse_quote! {
            struct EmptySkip {
                #[shape(skip = "")]
                value: u32,
            }
        };

        let err = derive_impl(input).expect_err("empty skip reason should fail");
        assert!(err.to_string().contains("skip reason"));
    }

    #[test]
    fn classified_struct_fields_are_emitted() {
        let input = parse_quote! {
            struct Classified {
                #[shape(field = Constants)]
                value: u32,
                #[shape(skip = "source span is represented by origin")]
                span: u32,
            }
        };

        let tokens = derive_impl(input).expect("classified fields should derive");
        let rendered = quote! { #tokens }.to_string();
        assert!(rendered.contains("add_field_value"));
        assert!(rendered.contains("ShapeDimension :: Constants"));
    }

    #[test]
    fn enum_variant_fields_require_policy() {
        let input = parse_quote! {
            enum MissingVariantPolicy {
                Value(u32),
            }
        };

        let err = derive_impl(input).expect_err("variant field policy should be required");
        assert!(err.to_string().contains("missing #[shape(...)] policy"));
    }

    #[test]
    fn enum_variant_stable_key_overrides_container_stable_key() {
        let input = parse_quote! {
            #[shape(stable_key = enum_stable_key)]
            enum VariantKey {
                #[shape(stable_key = literal_stable_key)]
                Literal {
                    #[shape(field = Constants)]
                    value: u32,
                },
                Name {
                    #[shape(field = Names)]
                    value: String,
                },
            }
        };

        let tokens = derive_impl(input).expect("variant stable key should derive");
        let rendered = quote! { #tokens }.to_string();
        assert!(
            rendered.contains("literal_stable_key (self)"),
            "variant-specific stable key should be used for the annotated variant: {rendered}"
        );
        assert!(
            rendered.contains("enum_stable_key (self)"),
            "container stable key should still be used for unannotated variants: {rendered}"
        );
    }

    #[test]
    fn duplicate_container_attrs_are_rejected() {
        let input = parse_quote! {
            #[shape(kind = "first")]
            #[shape(kind = "second")]
            struct DuplicateKind;
        };

        let err = derive_impl(input).expect_err("duplicate kind should fail");
        assert!(err.to_string().contains("duplicate shape kind"));

        let input = parse_quote! {
            #[shape(stable_key = first_key)]
            #[shape(stable_key = second_key)]
            struct DuplicateStableKey;
        };

        let err = derive_impl(input).expect_err("duplicate stable_key should fail");
        assert!(err.to_string().contains("duplicate shape stable_key"));
    }

    #[test]
    fn duplicate_field_label_is_rejected() {
        let input = parse_quote! {
            struct DuplicateLabel {
                #[shape(field = Names, label = "first", label = "second")]
                value: String,
            }
        };

        let err = derive_impl(input).expect_err("duplicate label should fail");
        assert!(err.to_string().contains("duplicate shape label"));
    }
}
