use crate::parser::{ParsedField, ParsedType, TypeDefinition};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneratedModule {
    pub source: String,
}

pub fn generate_module(types: &[ParsedType]) -> GeneratedModule {
    let mut source = String::new();

    for parsed in types {
        match &parsed.definition {
            TypeDefinition::Alias(target) => {
                source.push_str(&format!(
                    "pub type {} = {};\n\n",
                    to_rust_type_name(&parsed.name),
                    map_cddl_type(target)
                ));
            }
            TypeDefinition::Array(items) => {
                source.push_str("#[derive(Clone, Debug, Eq, PartialEq)]\n");
                source.push_str(&format!("pub struct {} {{\n", to_rust_type_name(&parsed.name)));

                for (index, item) in items.iter().enumerate() {
                    source.push_str(&format!(
                        "    pub item_{}: {},\n",
                        index,
                        map_cddl_type(item)
                    ));
                }

                source.push_str("}\n\n");
            }
            TypeDefinition::Map(fields) => {
                source.push_str("#[derive(Clone, Debug, Eq, PartialEq)]\n");
                source.push_str(&format!("pub struct {} {{\n", to_rust_type_name(&parsed.name)));

                for field in fields {
                    source.push_str(&format!(
                        "    pub {}: {},\n",
                        to_rust_field_name(field),
                        map_cddl_type(&field.ty)
                    ));
                }

                source.push_str("}\n\n");
            }
        }
    }

    GeneratedModule { source }
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

fn to_rust_field_name(field: &ParsedField) -> String {
    field
        .name
        .chars()
        .map(|ch| if ch == '-' { '_' } else { ch.to_ascii_lowercase() })
        .collect()
}

fn map_cddl_type(ty: &str) -> String {
    match ty.trim() {
        "uint" => "u64".to_string(),
        "int" => "i64".to_string(),
        "bool" => "bool".to_string(),
        "text" | "tstr" => "String".to_string(),
        "bytes" | "bstr" => "Vec<u8>".to_string(),
        other => to_rust_type_name(other),
    }
}
