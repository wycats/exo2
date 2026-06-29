//! Parsing logic for `#[exo(...)]` attributes.
//!
//! This module extracts metadata from the derive input and generates
//! the `HasExoSpec` trait implementation that returns a `NamespaceSpec`.
//!
//! # Attribute Grammar
//!
//! ```text
//! // Enum-level (required):
//! #[exo(namespace = "name", description = "...")]
//!
//! // Variant-level (required):
//! #[exo(effect = "pure|write|exec")]
//! #[exo(effect = "exec", upgrade_gate)]
//! #[exo(effect = "write", description = "...")]
//!
//! // Field-level (on named fields):
//! #[exo(long)]                    // --field_name (default for non-positional)
//! #[exo(long, short = 'f')]       // --field_name / -f
//! #[exo(positional)]              // positional argument
//! #[exo(flag)]                    // boolean flag
//! #[exo(default = "value")]       // default value (implies optional)
//! #[exo(description = "...")]     // override auto-generated description
//! ```

use proc_macro2::TokenStream;
use quote::quote;
use syn::{DeriveInput, Error, Result};

// ============================================================================
// Metadata structs
// ============================================================================

/// Parsed namespace-level metadata from `#[exo(namespace = "...", description = "...")]`.
#[derive(Debug)]
struct NamespaceMeta {
    name: String,
    description: String,
}

/// Parsed operation-level metadata from `#[exo(effect = "...")]` on a variant.
#[derive(Debug)]
struct OperationMeta {
    /// Variant identifier (e.g., `Start`, `Red`, `Green`).
    #[allow(dead_code)]
    variant_name: syn::Ident,
    /// `Snake_case` operation name derived from variant (e.g., "start", "red").
    operation_name: String,
    /// Human-readable description.
    description: String,
    /// Effect classification: "pure", "write", or "exec".
    effect: String,
    /// Whether this operation requires an upgrade gate.
    upgrade_gate: bool,
    /// Parsed arguments from named fields.
    args: Vec<ArgMeta>,
}

/// Parsed argument-level metadata from `#[exo(...)]` on a field.
#[derive(Debug)]
struct ArgMeta {
    /// CLI argument name (hyphens for options, underscores for positionals).
    name: String,
    /// Original Rust field name (always underscores, valid identifier).
    field_name: String,
    /// Human-readable description.
    description: String,
    /// The argument kind.
    kind: ArgKindMeta,
    /// The inferred value type.
    value_type: String,
    /// Whether the arg spec should be marked optional.
    optional: bool,
    /// Whether the Rust field type is `Option<T>` (used for `from_invocation` codegen).
    is_option_type: bool,
    /// Optional short flag alias.
    short: Option<char>,
    /// Optional default value.
    default_value: Option<String>,
}

/// Argument kind as determined by attributes and type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArgKindMeta {
    Flag,
    Option,
    Positional,
}

// ============================================================================
// Main expansion
// ============================================================================

/// Main expansion entry point.
///
/// Generates `impl HasExoSpec for EnumName` that returns a `NamespaceSpec`
/// with all operations and their argument specifications.
pub fn expand_exo_spec(input: &DeriveInput) -> Result<TokenStream> {
    // Must be an enum
    let syn::Data::Enum(data) = &input.data else {
        return Err(Error::new_spanned(
            input,
            "ExoSpec can only be derived for enums",
        ));
    };

    // Parse namespace-level #[exo(namespace = "...", description = "...")]
    let namespace = parse_namespace_meta(&input.attrs)?;

    // Parse each variant's operation-level metadata
    let mut operations = Vec::new();
    for variant in &data.variants {
        let op = parse_operation_meta(variant)?;
        operations.push(op);
    }

    Ok(generate_has_exo_spec(input, &namespace, &operations))
}

// ============================================================================
// Code generation
// ============================================================================

/// Generate the `HasExoSpec` impl block plus `from_invocation()` constructor.
fn generate_has_exo_spec(
    input: &DeriveInput,
    namespace: &NamespaceMeta,
    operations: &[OperationMeta],
) -> TokenStream {
    let enum_name = &input.ident;
    let ns_name = &namespace.name;
    let ns_description = &namespace.description;

    // Generate operation spec insertions
    let op_insertions: Vec<TokenStream> =
        operations.iter().map(generate_operation_insert).collect();

    // Generate from_invocation match arms
    let match_arms: Vec<TokenStream> = operations
        .iter()
        .map(|op| generate_from_invocation_arm(enum_name, op))
        .collect();

    quote! {
        impl crate::command::command_spec::HasExoSpec for #enum_name {
            fn spec() -> crate::command::command_spec::NamespaceSpec {
                use crate::command::command_spec::{
                    NamespaceSpec, OperationSpec, ArgSpec, ArgKind, ValueType,
                };
                use crate::api::protocol::Effect;
                use std::collections::BTreeMap;

                let mut operations = BTreeMap::new();
                #(#op_insertions)*

                NamespaceSpec {
                    name: #ns_name.to_string(),
                    description: #ns_description.to_string(),
                    operations,
                }
            }
        }

        impl #enum_name {
            /// Construct a typed command from a generic `Invocation`.
            ///
            /// This is generated by `#[derive(ExoSpec)]` and replaces the manual
            /// match arms in `build_command_from_invocation()`.
            #[allow(dead_code)]
            pub fn from_invocation(
                inv: &crate::command::router::Invocation,
            ) -> ::anyhow::Result<Self> {
                match inv.operation() {
                    #(#match_arms)*
                    other => ::anyhow::bail!(
                        "Unknown operation '{}' for namespace '{}'",
                        other,
                        #ns_name,
                    ),
                }
            }
        }
    }
}

/// Generate a single match arm for `from_invocation()`.
fn generate_from_invocation_arm(enum_name: &syn::Ident, op: &OperationMeta) -> TokenStream {
    let op_name = &op.operation_name;
    let variant_name = &op.variant_name;

    if op.args.is_empty() {
        // Unit variant — no fields to extract
        quote! {
            #op_name => Ok(#enum_name::#variant_name),
        }
    } else {
        // Named fields — extract each from the invocation
        let field_extractions: Vec<TokenStream> =
            op.args.iter().map(generate_field_extraction).collect();

        let field_names: Vec<syn::Ident> = op
            .args
            .iter()
            .map(|arg| syn::Ident::new(&arg.field_name, proc_macro2::Span::call_site()))
            .collect();

        quote! {
            #op_name => {
                #(#field_extractions)*
                Ok(#enum_name::#variant_name { #(#field_names),* })
            }
        }
    }
}

/// Generate the extraction code for a single field from an `Invocation`.
fn generate_field_extraction(arg: &ArgMeta) -> TokenStream {
    let field_name = syn::Ident::new(&arg.field_name, proc_macro2::Span::call_site());
    let arg_name = &arg.name;

    // Use is_option_type (Rust type is Option<T>) for extraction, not optional
    // (which also includes fields with defaults that are still required String types).
    match (arg.is_option_type, arg.value_type.as_str()) {
        // Bool (flags) — default to false
        (_, "Bool") => quote! {
            let #field_name = inv.get_bool(#arg_name).unwrap_or(false);
        },
        // Optional Int
        (true, "Int") => quote! {
            let #field_name = inv.get_int(#arg_name);
        },
        // Required Int
        (false, "Int") => quote! {
            let #field_name = inv.get_int(#arg_name)
                .ok_or_else(|| ::anyhow::anyhow!("Missing required argument: {}", #arg_name))?;
        },
        // Optional Json
        (true, "Json") => quote! {
            let #field_name = inv.get_json(#arg_name).map(str::to_string);
        },
        // Required Json
        (false, "Json") => quote! {
            let #field_name = inv.get_json(#arg_name)
                .map(str::to_string)
                .ok_or_else(|| ::anyhow::anyhow!("Missing required argument: {}", #arg_name))?;
        },
        // Optional string-like (String and all other types)
        (true, _) => quote! {
            let #field_name = inv.get_string(#arg_name).map(str::to_string);
        },
        // Required string-like with default
        (false, _) if arg.default_value.is_some() => {
            // Safety: guarded by `is_some()` in the match arm condition
            let Some(default) = arg.default_value.as_deref() else {
                unreachable!()
            };
            quote! {
                let #field_name = inv.get_string(#arg_name)
                    .map(str::to_string)
                    .unwrap_or_else(|| #default.to_string());
            }
        }
        // Required string-like (String and all other types)
        (false, _) => quote! {
            let #field_name = inv.get_string(#arg_name)
                .map(str::to_string)
                .ok_or_else(|| ::anyhow::anyhow!("Missing required argument: {}", #arg_name))?;
        },
    }
}

/// Generate the `operations.insert(...)` call for a single operation.
fn generate_operation_insert(op: &OperationMeta) -> TokenStream {
    let op_name = &op.operation_name;
    let op_description = &op.description;
    let upgrade_gate = op.upgrade_gate;

    let effect_expr = match op.effect.as_str() {
        "pure" => quote! { Effect::Pure },
        "write" => quote! { Effect::Write },
        "exec" => quote! { Effect::Exec },
        _ => unreachable!("effect validated during parsing"),
    };

    let arg_exprs: Vec<TokenStream> = op.args.iter().map(generate_arg_expr).collect();

    quote! {
        {
            let mut op = OperationSpec::new(#op_name, #op_description, #effect_expr)
                .with_upgrade_gate(#upgrade_gate);
            #(op = op.with_arg(#arg_exprs);)*
            operations.insert(#op_name.to_string(), op);
        }
    }
}

/// Generate an `ArgSpec` expression for a single argument.
fn generate_arg_expr(arg: &ArgMeta) -> TokenStream {
    let name = &arg.name;
    let description = &arg.description;

    let value_type_ident = match arg.value_type.as_str() {
        "Bool" => "Bool",
        "Int" => "Int",
        "Float" => "Float",
        "Path" => "Path",
        "Json" => "Json",
        // String and any unknown type
        _ => "String",
    };
    let value_type_ident = syn::Ident::new(value_type_ident, proc_macro2::Span::call_site());
    let value_type_expr = quote! { ValueType::#value_type_ident };

    // Start with the constructor based on kind
    let constructor = match arg.kind {
        ArgKindMeta::Flag => quote! { ArgSpec::flag(#name, #description) },
        ArgKindMeta::Option => {
            quote! { ArgSpec::option(#name, #description, #value_type_expr) }
        }
        ArgKindMeta::Positional => {
            quote! { ArgSpec::positional(#name, #description, #value_type_expr) }
        }
    };

    // Chain builder methods
    let optional_chain = if arg.optional && arg.kind != ArgKindMeta::Flag {
        quote! { .optional() }
    } else {
        quote! {}
    };

    let short_chain = arg
        .short
        .map_or_else(|| quote! {}, |ch| quote! { .with_short(#ch) });

    let default_chain = arg
        .default_value
        .as_ref()
        .map_or_else(|| quote! {}, |default| quote! { .with_default(#default) });

    quote! { #constructor #optional_chain #short_chain #default_chain }
}

// ============================================================================
// Namespace-level parsing
// ============================================================================

/// Parse `#[exo(namespace = "...", description = "...")]` from enum-level attributes.
fn parse_namespace_meta(attrs: &[syn::Attribute]) -> Result<NamespaceMeta> {
    for attr in attrs {
        if !attr.path().is_ident("exo") {
            continue;
        }

        let mut namespace_name = None;
        let mut description = None;

        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("namespace") {
                let value = meta.value()?;
                let lit: syn::LitStr = value.parse()?;
                namespace_name = Some(lit.value());
                Ok(())
            } else if meta.path.is_ident("description") {
                let value = meta.value()?;
                let lit: syn::LitStr = value.parse()?;
                description = Some(lit.value());
                Ok(())
            } else {
                Err(meta.error(format!(
                    "unknown exo enum attribute: `{}`",
                    meta.path
                        .get_ident()
                        .map_or_else(|| "<unknown>".to_string(), ToString::to_string,)
                )))
            }
        })?;

        if let Some(name) = namespace_name {
            return Ok(NamespaceMeta {
                description: description.unwrap_or_default(),
                name,
            });
        }
    }

    Err(Error::new(
        proc_macro2::Span::call_site(),
        "ExoSpec requires #[exo(namespace = \"...\")] on the enum",
    ))
}

// ============================================================================
// Operation-level parsing
// ============================================================================

/// Parse `#[exo(effect = "...", upgrade_gate, description = "...", operation = "...")]` from variant-level attributes.
fn parse_operation_meta(variant: &syn::Variant) -> Result<OperationMeta> {
    let mut effect = None;
    let mut upgrade_gate = false;
    let mut description = None;
    let mut explicit_operation = None;

    for attr in &variant.attrs {
        if !attr.path().is_ident("exo") {
            continue;
        }

        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("effect") {
                let value = meta.value()?;
                let lit: syn::LitStr = value.parse()?;
                let effect_str = lit.value();
                match effect_str.as_str() {
                    "pure" | "write" | "exec" => {
                        effect = Some(effect_str);
                    }
                    _ => {
                        return Err(meta.error(format!(
                            "invalid effect: `{effect_str}`. Expected \"pure\", \"write\", or \"exec\""
                        )));
                    }
                }
                Ok(())
            } else if meta.path.is_ident("upgrade_gate") {
                upgrade_gate = true;
                Ok(())
            } else if meta.path.is_ident("description") {
                let value = meta.value()?;
                let lit: syn::LitStr = value.parse()?;
                description = Some(lit.value());
                Ok(())
            } else if meta.path.is_ident("operation") {
                let value = meta.value()?;
                let lit: syn::LitStr = value.parse()?;
                explicit_operation = Some(lit.value());
                Ok(())
            } else {
                Err(meta.error(format!(
                    "unknown exo variant attribute: `{}`",
                    meta.path
                        .get_ident()
                        .map_or_else(|| "<unknown>".to_string(), ToString::to_string,)
                )))
            }
        })?;
    }

    let effect = effect.ok_or_else(|| {
        Error::new_spanned(
            &variant.ident,
            format!(
                "ExoSpec variant `{}` requires #[exo(effect = \"pure|write|exec\")]",
                variant.ident
            ),
        )
    })?;

    let operation_name =
        explicit_operation.unwrap_or_else(|| to_snake_case(&variant.ident.to_string()));
    let description = description.unwrap_or_default();

    // Parse field-level arguments
    let args = parse_variant_args(variant)?;

    Ok(OperationMeta {
        variant_name: variant.ident.clone(),
        operation_name,
        description,
        effect,
        upgrade_gate,
        args,
    })
}

// ============================================================================
// Field-level (argument) parsing
// ============================================================================

/// Parse arguments from a variant's named fields.
fn parse_variant_args(variant: &syn::Variant) -> Result<Vec<ArgMeta>> {
    let fields = match &variant.fields {
        syn::Fields::Named(fields) => &fields.named,
        syn::Fields::Unit => return Ok(Vec::new()),
        syn::Fields::Unnamed(_) => {
            return Err(Error::new_spanned(
                variant,
                "ExoSpec variants must use named fields or be unit variants",
            ));
        }
    };

    let mut args = Vec::new();
    for field in fields {
        let arg = parse_field_arg(field)?;
        args.push(arg);
    }
    Ok(args)
}

/// Parse a single named field into an `ArgMeta`.
fn parse_field_arg(field: &syn::Field) -> Result<ArgMeta> {
    let field_name = field
        .ident
        .as_ref()
        .ok_or_else(|| Error::new_spanned(field, "ExoSpec fields must be named"))?
        .to_string();

    // Detect Option<T> wrapper
    let (is_option, inner_type) = unwrap_option_type(&field.ty);

    // Infer value type from the Rust type
    let value_type = infer_value_type(inner_type);

    // Parse #[exo(...)] attributes on the field
    let mut kind = None;
    let mut short = None;
    let mut default_value = None;
    let mut description = None;
    let mut explicit_optional = false;
    let mut explicit_value_type: Option<String> = None;

    for attr in &field.attrs {
        if !attr.path().is_ident("exo") {
            continue;
        }

        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("long") {
                if kind.is_none() {
                    kind = Some(ArgKindMeta::Option);
                }
                Ok(())
            } else if meta.path.is_ident("positional") {
                kind = Some(ArgKindMeta::Positional);
                Ok(())
            } else if meta.path.is_ident("flag") {
                kind = Some(ArgKindMeta::Flag);
                Ok(())
            } else if meta.path.is_ident("short") {
                let value = meta.value()?;
                let lit: syn::LitChar = value.parse()?;
                short = Some(lit.value());
                // short implies long
                if kind.is_none() {
                    kind = Some(ArgKindMeta::Option);
                }
                Ok(())
            } else if meta.path.is_ident("default") {
                let value = meta.value()?;
                let lit: syn::LitStr = value.parse()?;
                default_value = Some(lit.value());
                Ok(())
            } else if meta.path.is_ident("description") {
                let value = meta.value()?;
                let lit: syn::LitStr = value.parse()?;
                description = Some(lit.value());
                Ok(())
            } else if meta.path.is_ident("optional") {
                explicit_optional = true;
                Ok(())
            } else if meta.path.is_ident("json") {
                explicit_value_type = Some("Json".to_string());
                Ok(())
            } else {
                Err(meta.error(format!(
                    "unknown exo field attribute: `{}`",
                    meta.path
                        .get_ident()
                        .map_or_else(|| "<unknown>".to_string(), ToString::to_string,)
                )))
            }
        })?;
    }

    // Override inferred value type if explicitly set
    let value_type = explicit_value_type.unwrap_or(value_type);

    // Default kind: bool fields become flags, everything else becomes an option
    let kind = kind.unwrap_or(if value_type == "Bool" {
        ArgKindMeta::Flag
    } else {
        ArgKindMeta::Option
    });

    let optional = is_option || explicit_optional || default_value.is_some();
    let is_option_type = is_option;
    let description = description.unwrap_or_default();

    // For option/flag args, convert underscores to hyphens (standard CLI convention).
    // Positional args keep underscores since they're not prefixed with --.
    let name = match kind {
        ArgKindMeta::Option | ArgKindMeta::Flag => field_name.replace('_', "-"),
        ArgKindMeta::Positional => field_name.clone(),
    };

    Ok(ArgMeta {
        name,
        field_name,
        description,
        kind,
        value_type,
        optional,
        is_option_type,
        short,
        default_value,
    })
}

// ============================================================================
// Type inference helpers
// ============================================================================

/// Check if a type is `Option<T>` and return (`is_option`, `inner_type`).
fn unwrap_option_type(ty: &syn::Type) -> (bool, &syn::Type) {
    if let syn::Type::Path(type_path) = ty
        && let Some(segment) = type_path.path.segments.last()
        && segment.ident == "Option"
        && let syn::PathArguments::AngleBracketed(args) = &segment.arguments
        && let Some(syn::GenericArgument::Type(inner)) = args.args.first()
    {
        return (true, inner);
    }
    (false, ty)
}

/// Infer the `ValueType` variant name from a Rust type.
fn infer_value_type(ty: &syn::Type) -> String {
    if let syn::Type::Path(type_path) = ty
        && let Some(segment) = type_path.path.segments.last()
    {
        return match segment.ident.to_string().as_str() {
            "bool" => "Bool",
            "i8" | "i16" | "i32" | "i64" | "i128" | "isize" | "u8" | "u16" | "u32" | "u64"
            | "u128" | "usize" => "Int",
            "f32" | "f64" => "Float",
            "PathBuf" | "Path" => "Path",
            "Value" => "Json",
            // Default: String, &str, or anything else
            _ => "String",
        }
        .to_string();
    }
    "String".to_string()
}

/// Convert a `PascalCase` identifier to `snake_case`.
///
/// Examples: "Start" -> "start", "`RedGreen`" -> "`red_green`", "`TDDNew`" -> "`tdd_new`"
fn to_snake_case(s: &str) -> String {
    let mut result = String::with_capacity(s.len() + 4);
    let chars: Vec<char> = s.chars().collect();

    for (i, &ch) in chars.iter().enumerate() {
        if ch.is_uppercase() {
            if i > 0 {
                let prev_lower = chars[i - 1].is_lowercase();
                let next_lower = chars.get(i + 1).is_some_and(|c| c.is_lowercase());
                // Insert underscore if:
                // - Previous char was lowercase (e.g., "dN" in "RedNew"), OR
                // - This is an uppercase followed by lowercase AND previous was uppercase
                //   (e.g., the "N" in "TDDNew" — we want "tdd_new" not "tddn_ew")
                if prev_lower || (next_lower && chars[i - 1].is_uppercase()) {
                    result.push('_');
                }
            }
            result.push(ch.to_lowercase().next().unwrap_or(ch));
        } else {
            result.push(ch);
        }
    }

    result
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    // --- Namespace parsing ---

    #[test]
    fn parse_namespace_from_enum_attrs() {
        let input: DeriveInput = parse_quote! {
            #[exo(namespace = "tdd", description = "TDD workflow commands")]
            enum TddCommands {
                #[exo(effect = "exec")]
                Start,
                #[exo(effect = "write")]
                Red,
            }
        };

        let result = expand_exo_spec(&input);
        assert!(result.is_ok(), "Expected Ok, got: {result:?}");
    }

    #[test]
    fn parse_namespace_without_description() {
        let input: DeriveInput = parse_quote! {
            #[exo(namespace = "tdd")]
            enum TddCommands {
                #[exo(effect = "exec")]
                Start,
            }
        };

        let result = expand_exo_spec(&input);
        assert!(result.is_ok(), "Expected Ok, got: {result:?}");
    }

    #[test]
    fn reject_missing_namespace() {
        let input: DeriveInput = parse_quote! {
            enum TddCommands {
                #[exo(effect = "exec")]
                Start,
            }
        };

        let result = expand_exo_spec(&input);
        assert!(result.is_err());
    }

    // --- Effect parsing ---

    #[test]
    fn reject_missing_effect() {
        let input: DeriveInput = parse_quote! {
            #[exo(namespace = "tdd")]
            enum TddCommands {
                Start,
            }
        };

        let result = expand_exo_spec(&input);
        assert!(result.is_err());
    }

    #[test]
    fn reject_invalid_effect() {
        let input: DeriveInput = parse_quote! {
            #[exo(namespace = "tdd")]
            enum TddCommands {
                #[exo(effect = "destroy")]
                Start,
            }
        };

        let result = expand_exo_spec(&input);
        assert!(result.is_err());
    }

    #[test]
    fn reject_struct() {
        let input: DeriveInput = parse_quote! {
            #[exo(namespace = "tdd")]
            struct NotAnEnum {
                field: String,
            }
        };

        let result = expand_exo_spec(&input);
        assert!(result.is_err());
    }

    #[test]
    fn accept_upgrade_gate() {
        let input: DeriveInput = parse_quote! {
            #[exo(namespace = "phase")]
            enum PhaseCommands {
                #[exo(effect = "exec", upgrade_gate)]
                Start,
            }
        };

        let result = expand_exo_spec(&input);
        assert!(result.is_ok());
    }

    // --- Field/argument parsing ---

    #[test]
    fn parse_variant_with_named_fields() {
        let input: DeriveInput = parse_quote! {
            #[exo(namespace = "tdd", description = "TDD workflow")]
            enum TddCommands {
                #[exo(effect = "exec", description = "Start a new TDD cycle")]
                New {
                    #[exo(long, short = 'n', description = "Task selector")]
                    name: String,
                    #[exo(long, short = 't', description = "Test command or file")]
                    test: String,
                },
                #[exo(effect = "exec", description = "Confirm test fails")]
                Red,
                #[exo(effect = "exec", description = "Confirm test passes")]
                Green,
            }
        };

        let result = expand_exo_spec(&input);
        assert!(result.is_ok(), "Expected Ok, got: {result:?}");

        // Verify the generated code contains expected tokens
        let tokens = result.unwrap().to_string();
        assert!(
            tokens.contains("HasExoSpec"),
            "Should generate HasExoSpec impl"
        );
        assert!(tokens.contains("\"tdd\""), "Should contain namespace name");
        assert!(tokens.contains("\"new\""), "Should contain operation name");
        assert!(tokens.contains("\"name\""), "Should contain arg name");
        assert!(tokens.contains("'n'"), "Should contain short flag");
        // Verify from_invocation is generated
        assert!(
            tokens.contains("from_invocation"),
            "Should generate from_invocation"
        );
        assert!(tokens.contains("get_string"), "Should extract string args");
    }

    #[test]
    fn from_invocation_unit_variant() {
        let input: DeriveInput = parse_quote! {
            #[exo(namespace = "tdd")]
            enum TddCommands {
                #[exo(effect = "exec")]
                Red,
            }
        };

        let tokens = expand_exo_spec(&input).unwrap().to_string();
        // Unit variants should produce a simple Ok(Enum::Variant)
        assert!(tokens.contains("Red"), "Should reference the variant name");
        assert!(
            tokens.contains("from_invocation"),
            "Should generate from_invocation"
        );
    }

    #[test]
    fn parse_optional_field() {
        let input: DeriveInput = parse_quote! {
            #[exo(namespace = "test")]
            enum TestCommands {
                #[exo(effect = "pure")]
                List {
                    #[exo(long, description = "Optional filter")]
                    filter: Option<String>,
                },
            }
        };

        let result = expand_exo_spec(&input);
        assert!(result.is_ok(), "Expected Ok, got: {result:?}");
        let tokens = result.unwrap().to_string();
        assert!(
            tokens.contains("optional"),
            "Option<T> should produce optional arg"
        );
    }

    #[test]
    fn parse_positional_field() {
        let input: DeriveInput = parse_quote! {
            #[exo(namespace = "test")]
            enum TestCommands {
                #[exo(effect = "write")]
                Add {
                    #[exo(positional, description = "The item to add")]
                    item: String,
                },
            }
        };

        let result = expand_exo_spec(&input);
        assert!(result.is_ok(), "Expected Ok, got: {result:?}");
        let tokens = result.unwrap().to_string();
        assert!(
            tokens.contains("positional"),
            "Should use positional constructor"
        );
    }

    #[test]
    fn parse_flag_field() {
        let input: DeriveInput = parse_quote! {
            #[exo(namespace = "test")]
            enum TestCommands {
                #[exo(effect = "pure")]
                List {
                    #[exo(flag, description = "Show verbose output")]
                    verbose: bool,
                },
            }
        };

        let result = expand_exo_spec(&input);
        assert!(result.is_ok(), "Expected Ok, got: {result:?}");
        let tokens = result.unwrap().to_string();
        assert!(
            tokens.contains("flag"),
            "bool field with #[exo(flag)] should use flag constructor"
        );
    }

    #[test]
    fn parse_default_value() {
        let input: DeriveInput = parse_quote! {
            #[exo(namespace = "test")]
            enum TestCommands {
                #[exo(effect = "pure")]
                List {
                    #[exo(long, default = "10", description = "Max results")]
                    limit: String,
                },
            }
        };

        let result = expand_exo_spec(&input);
        assert!(result.is_ok(), "Expected Ok, got: {result:?}");
        let tokens = result.unwrap().to_string();
        assert!(tokens.contains("with_default"), "Should chain with_default");
    }

    #[test]
    fn reject_unnamed_fields() {
        let input: DeriveInput = parse_quote! {
            #[exo(namespace = "test")]
            enum TestCommands {
                #[exo(effect = "pure")]
                Bad(String, i32),
            }
        };

        let result = expand_exo_spec(&input);
        assert!(result.is_err());
    }

    // --- Snake case conversion ---

    #[test]
    fn snake_case_simple() {
        assert_eq!(to_snake_case("Start"), "start");
        assert_eq!(to_snake_case("Red"), "red");
        assert_eq!(to_snake_case("Green"), "green");
    }

    #[test]
    fn snake_case_multi_word() {
        assert_eq!(to_snake_case("RedGreen"), "red_green");
        assert_eq!(to_snake_case("PhaseStart"), "phase_start");
    }

    #[test]
    fn snake_case_acronym() {
        assert_eq!(to_snake_case("TDDNew"), "tdd_new");
        assert_eq!(to_snake_case("HTTPServer"), "http_server");
    }

    #[test]
    fn snake_case_already_lower() {
        assert_eq!(to_snake_case("start"), "start");
    }

    // --- Type inference ---

    #[test]
    fn infer_types() {
        let string_ty: syn::Type = parse_quote! { String };
        assert_eq!(infer_value_type(&string_ty), "String");

        let bool_ty: syn::Type = parse_quote! { bool };
        assert_eq!(infer_value_type(&bool_ty), "Bool");

        let i32_ty: syn::Type = parse_quote! { i32 };
        assert_eq!(infer_value_type(&i32_ty), "Int");

        let f64_ty: syn::Type = parse_quote! { f64 };
        assert_eq!(infer_value_type(&f64_ty), "Float");

        let path_ty: syn::Type = parse_quote! { PathBuf };
        assert_eq!(infer_value_type(&path_ty), "Path");
    }

    #[test]
    fn unwrap_option() {
        let opt_ty: syn::Type = parse_quote! { Option<String> };
        let (is_opt, _inner) = unwrap_option_type(&opt_ty);
        assert!(is_opt);

        let plain_ty: syn::Type = parse_quote! { String };
        let (is_opt, _inner) = unwrap_option_type(&plain_ty);
        assert!(!is_opt);
    }
}
