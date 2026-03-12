use crate::parser::{ArrayItem, FieldKey, ParsedType, TypeDefinition, TypeExpr};

/// A generated Rust module represented as source text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneratedModule {
    pub source: String,
}

/// Generates Rust source for the parsed schema definitions supported by the
/// current code generator.
pub fn generate_module(types: &[ParsedType]) -> GeneratedModule {
    let mut source = String::new();

    for parsed in types {
        match &parsed.definition {
            TypeDefinition::Alias(type_expr) => {
                source.push_str(&format!(
                    "pub type {} = {};\n\n",
                    to_rust_type_name(&parsed.name),
                    map_type_expr(type_expr)
                ));
            }
            TypeDefinition::Array(items) => {
                source.push_str("#[derive(Clone, Debug, Eq, PartialEq)]\n");
                source.push_str(&format!(
                    "pub struct {} {{\n",
                    to_rust_type_name(&parsed.name)
                ));

                for (index, item) in items.iter().enumerate() {
                    let field_name = if let Some(name) = &item.name {
                        to_snake_case(name)
                    } else {
                        format!("item_{index}")
                    };
                    source.push_str(&format!(
                        "    pub {}: {},\n",
                        field_name,
                        map_type_expr(&item.ty)
                    ));
                }

                source.push_str("}\n\n");
            }
            TypeDefinition::Map(fields) => {
                source.push_str("#[derive(Clone, Debug, Eq, PartialEq)]\n");
                source.push_str(&format!(
                    "pub struct {} {{\n",
                    to_rust_type_name(&parsed.name)
                ));

                for field in fields {
                    let field_name = match &field.key {
                        FieldKey::Label(label) => to_snake_case(label),
                        FieldKey::Index(idx) => format!("field_{idx}"),
                    };
                    let rust_type = map_type_expr(&field.ty);
                    let rust_type = if field.optional {
                        format!("Option<{rust_type}>")
                    } else {
                        rust_type
                    };
                    source.push_str(&format!("    pub {field_name}: {rust_type},\n"));
                }

                source.push_str("}\n\n");
            }
            TypeDefinition::GroupChoice(variants) => {
                let type_name = to_rust_type_name(&parsed.name);
                source.push_str("#[derive(Clone, Debug, Eq, PartialEq)]\n");
                source.push_str(&format!("pub enum {type_name} {{\n"));

                for (vi, fields) in variants.iter().enumerate() {
                    let variant_name = group_choice_variant_name(vi, fields);
                    if fields.is_empty() {
                        source.push_str(&format!("    {variant_name},\n"));
                    } else {
                        source.push_str(&format!("    {variant_name} {{\n"));
                        for (fi, item) in fields.iter().enumerate() {
                            let field_name = if let Some(name) = &item.name {
                                to_snake_case(name)
                            } else {
                                format!("item_{fi}")
                            };
                            source.push_str(&format!(
                                "        {}: {},\n",
                                field_name,
                                map_type_expr(&item.ty)
                            ));
                        }
                        source.push_str("    },\n");
                    }
                }

                source.push_str("}\n\n");
            }
        }
    }

    GeneratedModule { source }
}

/// Maps a `TypeExpr` to its Rust type string representation.
fn map_type_expr(expr: &TypeExpr) -> String {
    match expr {
        TypeExpr::Named(name) => map_cddl_builtin(name),
        TypeExpr::Sized(base, size) => map_sized_type(base, *size),
        TypeExpr::VarArray(inner) => format!("Vec<{}>", map_type_expr(inner)),
        TypeExpr::Optional(inner) => format!("Option<{}>", map_type_expr(inner)),
        // CBOR tags are a serialization concern; the Rust type is the inner
        // type.  CBOR encode/decode implementations will emit/check the tag.
        TypeExpr::Tagged(_, inner) => map_type_expr(inner),
    }
}

/// Maps a CDDL type with `.size N` to the appropriate Rust fixed type.
fn map_sized_type(base: &str, size: u64) -> String {
    match base {
        "uint" => match size {
            1 => "u8".to_string(),
            2 => "u16".to_string(),
            4 => "u32".to_string(),
            8 => "u64".to_string(),
            _ => format!("u64 /* uint .size {size} */"),
        },
        "int" => match size {
            1 => "i8".to_string(),
            2 => "i16".to_string(),
            4 => "i32".to_string(),
            8 => "i64".to_string(),
            _ => format!("i64 /* int .size {size} */"),
        },
        "bytes" | "bstr" => format!("[u8; {size}]"),
        other => {
            // Unknown base with size constraint — emit the type name with a comment.
            format!("{} /* .size {size} */", to_rust_type_name(other))
        }
    }
}

fn to_rust_type_name(name: &str) -> String {
    let mut output = String::new();

    for part in name.split(['-', '_']) {
        if part.is_empty() {
            continue;
        }

        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            output.push(first.to_ascii_uppercase());
            output.extend(chars.map(|ch| ch.to_ascii_lowercase()));
        }
    }

    output
}

fn to_snake_case(name: &str) -> String {
    name.chars()
        .map(|ch| if ch == '-' { '_' } else { ch.to_ascii_lowercase() })
        .collect()
}

/// Maps a CDDL builtin type name to the corresponding Rust type.
fn map_cddl_builtin(ty: &str) -> String {
    match ty.trim() {
        "uint" => "u64".to_string(),
        "int" => "i64".to_string(),
        "bool" => "bool".to_string(),
        "text" | "tstr" => "String".to_string(),
        "bytes" | "bstr" => "Vec<u8>".to_string(),
        other => to_rust_type_name(other),
    }
}

/// Derives a variant name for a group-choice alternative.
///
/// If the first field is a named field, use its name in PascalCase. Otherwise
/// fall back to `Variant{index}`.
fn group_choice_variant_name(index: usize, fields: &[ArrayItem]) -> String {
    if let Some(first) = fields.first() {
        if let Some(name) = &first.name {
            return to_rust_type_name(name);
        }
    }
    format!("Variant{index}")
}
