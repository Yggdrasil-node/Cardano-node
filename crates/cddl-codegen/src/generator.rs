use crate::parser::{ArrayItem, FieldKey, ParsedField, ParsedType, TypeDefinition, TypeExpr};

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

// ───────────────────────────────────────────────────────────────────────────
// CBOR codec generation
// ───────────────────────────────────────────────────────────────────────────

/// Generates Rust source containing struct/enum definitions **and**
/// `CborEncode`/`CborDecode` implementations for every parsed type that
/// produces a concrete struct or enum.
///
/// The generated source assumes the consumer provides:
/// ```ignore
/// use crate::cbor::{CborEncode, CborDecode, Encoder, Decoder};
/// use crate::error::LedgerError;
/// ```
pub fn generate_module_with_codecs(types: &[ParsedType]) -> GeneratedModule {
    let defs = generate_module(types);
    let mut source = defs.source;

    for parsed in types {
        let type_name = to_rust_type_name(&parsed.name);
        match &parsed.definition {
            TypeDefinition::Alias(_) => {
                // Aliases map to `type X = Y;` — no codec impl needed;
                // the aliased type already carries its own impls.
            }
            TypeDefinition::Array(items) => {
                source.push_str(&gen_array_encode(&type_name, items));
                source.push_str(&gen_array_decode(&type_name, items));
            }
            TypeDefinition::Map(fields) => {
                source.push_str(&gen_map_encode(&type_name, fields));
                source.push_str(&gen_map_decode(&type_name, fields));
            }
            TypeDefinition::GroupChoice(variants) => {
                source.push_str(&gen_choice_encode(&type_name, variants));
                source.push_str(&gen_choice_decode(&type_name, variants));
            }
        }
    }

    GeneratedModule { source }
}

// ── Array codecs ──────────────────────────────────────────────────────────

fn gen_array_encode(type_name: &str, items: &[ArrayItem]) -> String {
    let mut s = String::new();
    s.push_str(&format!("impl CborEncode for {type_name} {{\n"));
    s.push_str("    fn encode_cbor(&self, enc: &mut Encoder) {\n");
    s.push_str(&format!("        enc.array({});\n", items.len()));
    for (i, item) in items.iter().enumerate() {
        let field = if let Some(name) = &item.name {
            to_snake_case(name)
        } else {
            format!("item_{i}")
        };
        s.push_str(&emit_encode_field(&item.ty, &format!("self.{field}"), "        "));
    }
    s.push_str("    }\n");
    s.push_str("}\n\n");
    s
}

fn gen_array_decode(type_name: &str, items: &[ArrayItem]) -> String {
    let mut s = String::new();
    s.push_str(&format!("impl CborDecode for {type_name} {{\n"));
    s.push_str("    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {\n");
    s.push_str("        let _len = dec.array()?;\n");

    let mut field_names = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let field = if let Some(name) = &item.name {
            to_snake_case(name)
        } else {
            format!("item_{i}")
        };
        s.push_str(&emit_decode_field(&item.ty, &field, "        "));
        field_names.push(field);
    }

    s.push_str("        Ok(Self {\n");
    for f in &field_names {
        s.push_str(&format!("            {f},\n"));
    }
    s.push_str("        })\n");
    s.push_str("    }\n");
    s.push_str("}\n\n");
    s
}

// ── Map codecs ────────────────────────────────────────────────────────────

fn gen_map_encode(type_name: &str, fields: &[ParsedField]) -> String {
    let mut s = String::new();
    s.push_str(&format!("impl CborEncode for {type_name} {{\n"));
    s.push_str("    fn encode_cbor(&self, enc: &mut Encoder) {\n");

    // Count required + optional fields present.
    let required_count = fields.iter().filter(|f| !f.optional).count();
    let optional_fields: Vec<_> = fields.iter().filter(|f| f.optional).collect();

    if optional_fields.is_empty() {
        s.push_str(&format!("        enc.map({required_count});\n"));
    } else {
        s.push_str(&format!("        let mut count: u64 = {required_count};\n"));
        for f in &optional_fields {
            let fname = map_field_name(f);
            s.push_str(&format!(
                "        if self.{fname}.is_some() {{ count += 1; }}\n"
            ));
        }
        s.push_str("        enc.map(count);\n");
    }

    for f in fields {
        let fname = map_field_name(f);
        let key_expr = match &f.key {
            FieldKey::Index(idx) => format!("enc.unsigned({idx});"),
            FieldKey::Label(label) => format!("enc.text(\"{label}\");"),
        };
        if f.optional {
            s.push_str(&format!("        if let Some(ref val) = self.{fname} {{\n"));
            s.push_str(&format!("            {key_expr}\n"));
            s.push_str(&emit_encode_field(&f.ty, "val", "            "));
            s.push_str("        }\n");
        } else {
            s.push_str(&format!("        {key_expr}\n"));
            s.push_str(&emit_encode_field(&f.ty, &format!("self.{fname}"), "        "));
        }
    }

    s.push_str("    }\n");
    s.push_str("}\n\n");
    s
}

fn gen_map_decode(type_name: &str, fields: &[ParsedField]) -> String {
    let mut s = String::new();
    s.push_str(&format!("impl CborDecode for {type_name} {{\n"));
    s.push_str("    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {\n");
    s.push_str("        let map_len = dec.map()?;\n");

    // Declare temporary variables for each field.
    for f in fields {
        let fname = map_field_name(f);
        let rust_ty = map_type_expr(&f.ty);
        s.push_str(&format!("        let mut {fname}: Option<{rust_ty}> = None;\n"));
    }

    // Determine whether keys are integer or label.
    let uses_int_keys = fields
        .iter()
        .any(|f| matches!(f.key, FieldKey::Index(_)));

    s.push_str("        for _ in 0..map_len {\n");
    if uses_int_keys {
        s.push_str("            let key = dec.unsigned()?;\n");
        s.push_str("            match key {\n");
        for f in fields {
            let fname = map_field_name(f);
            let idx = match &f.key {
                FieldKey::Index(i) => *i,
                FieldKey::Label(_) => continue,
            };
            s.push_str(&format!("                {idx} => {{\n"));
            s.push_str(&emit_decode_assign(&f.ty, &fname, "                    "));
            s.push_str("                }\n");
        }
        s.push_str("                _ => { dec.skip()?; }\n");
        s.push_str("            }\n");
    } else {
        s.push_str("            let key = dec.text()?;\n");
        s.push_str("            match key {\n");
        for f in fields {
            let fname = map_field_name(f);
            let label = match &f.key {
                FieldKey::Label(l) => l.clone(),
                FieldKey::Index(_) => continue,
            };
            s.push_str(&format!("                \"{label}\" => {{\n"));
            s.push_str(&emit_decode_assign(&f.ty, &fname, "                    "));
            s.push_str("                }\n");
        }
        s.push_str("                _ => { dec.skip()?; }\n");
        s.push_str("            }\n");
    }
    s.push_str("        }\n");

    // Build result struct.
    s.push_str("        Ok(Self {\n");
    for f in fields {
        let fname = map_field_name(f);
        if f.optional {
            s.push_str(&format!("            {fname},\n"));
        } else {
            s.push_str(&format!(
                "            {fname}: {fname}.ok_or(LedgerError::CborInvalidLength {{ expected: 1, actual: 0 }})?,\n"
            ));
        }
    }
    s.push_str("        })\n");
    s.push_str("    }\n");
    s.push_str("}\n\n");
    s
}

// ── GroupChoice codecs ────────────────────────────────────────────────────

fn gen_choice_encode(type_name: &str, variants: &[Vec<ArrayItem>]) -> String {
    let mut s = String::new();
    s.push_str(&format!("impl CborEncode for {type_name} {{\n"));
    s.push_str("    fn encode_cbor(&self, enc: &mut Encoder) {\n");
    s.push_str("        match self {\n");

    for (vi, fields) in variants.iter().enumerate() {
        let variant = group_choice_variant_name(vi, fields);
        if fields.is_empty() {
            s.push_str(&format!("            Self::{variant} => {{\n"));
            s.push_str("                enc.array(0);\n");
        } else {
            let bindings: Vec<String> = fields
                .iter()
                .enumerate()
                .map(|(fi, item)| {
                    if let Some(name) = &item.name {
                        to_snake_case(name)
                    } else {
                        format!("item_{fi}")
                    }
                })
                .collect();
            let pat = bindings.join(", ");
            s.push_str(&format!(
                "            Self::{variant} {{ {pat} }} => {{\n"
            ));
            s.push_str(&format!("                enc.array({});\n", fields.len()));
            for (fi, item) in fields.iter().enumerate() {
                let name = &bindings[fi];
                s.push_str(&emit_encode_field(&item.ty, name, "                "));
            }
        }
        s.push_str("            }\n");
    }

    s.push_str("        }\n");
    s.push_str("    }\n");
    s.push_str("}\n\n");
    s
}

fn gen_choice_decode(type_name: &str, variants: &[Vec<ArrayItem>]) -> String {
    let mut s = String::new();
    s.push_str(&format!("impl CborDecode for {type_name} {{\n"));
    s.push_str("    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {\n");
    s.push_str("        let len = dec.array()?;\n");

    if variants.is_empty() {
        s.push_str("        Err(LedgerError::CborInvalidLength { expected: 0, actual: len as usize })\n");
    } else {
        s.push_str("        match len {\n");

        // Group variants by field count for match arms.
        // If multiple variants share the same length, we need a discriminant.
        let mut by_len: std::collections::BTreeMap<usize, Vec<(usize, &Vec<ArrayItem>)>> =
            std::collections::BTreeMap::new();
        for (vi, fields) in variants.iter().enumerate() {
            by_len.entry(fields.len()).or_default().push((vi, fields));
        }

        for (len, group) in &by_len {
            s.push_str(&format!("            {len} => {{\n"));
            if group.len() == 1 {
                // Unambiguous — decode directly.
                let (vi, fields) = &group[0];
                let variant = group_choice_variant_name(*vi, fields);
                let mut fnames = Vec::new();
                for (fi, item) in fields.iter().enumerate() {
                    let fname = if let Some(name) = &item.name {
                        to_snake_case(name)
                    } else {
                        format!("item_{fi}")
                    };
                    s.push_str(&emit_decode_field(&item.ty, &fname, "                "));
                    fnames.push(fname);
                }
                let fields_init = fnames.join(", ");
                s.push_str(&format!(
                    "                Ok(Self::{variant} {{ {fields_init} }})\n"
                ));
            } else {
                // Multiple variants with the same field count — use first
                // element as a discriminant tag (common for certificate-style
                // sum types).
                s.push_str("                let tag = dec.unsigned()?;\n");
                s.push_str("                match tag {\n");
                for (vi, fields) in group {
                    let variant = group_choice_variant_name(*vi, fields);
                    // Use variant index as expected tag.
                    s.push_str(&format!("                    {vi} => {{\n"));
                    let mut fnames = Vec::new();
                    // First field is the tag, already consumed.
                    for (fi, item) in fields.iter().enumerate() {
                        let fname = if let Some(name) = &item.name {
                            to_snake_case(name)
                        } else {
                            format!("item_{fi}")
                        };
                        if fi == 0 {
                            // Tag is already decoded as `tag`.
                            s.push_str(&format!(
                                "                        let {fname} = tag;\n"
                            ));
                        } else {
                            s.push_str(&emit_decode_field(
                                &item.ty,
                                &fname,
                                "                        ",
                            ));
                        }
                        fnames.push(fname);
                    }
                    let init = fnames.join(", ");
                    s.push_str(&format!(
                        "                        Ok(Self::{variant} {{ {init} }})\n"
                    ));
                    s.push_str("                    }\n");
                }
                s.push_str("                    _ => Err(LedgerError::CborInvalidAdditionalInfo(tag as u8)),\n");
                s.push_str("                }\n");
            }
            s.push_str("            }\n");
        }

        s.push_str("            _ => Err(LedgerError::CborInvalidLength { expected: 0, actual: len as usize }),\n");
        s.push_str("        }\n");
    }

    s.push_str("    }\n");
    s.push_str("}\n\n");
    s
}

// ── Type expression encode/decode helpers ─────────────────────────────────

/// Returns `true` if `expr` maps to a CDDL builtin that uses a primitive
/// encoder method (unsigned, integer, bytes, text, bool) rather than
/// `encode_cbor`.
fn is_builtin_primitive(expr: &TypeExpr) -> bool {
    match expr {
        TypeExpr::Named(name) => matches!(
            name.as_str(),
            "uint" | "int" | "bool" | "bytes" | "bstr" | "text" | "tstr"
        ),
        TypeExpr::Sized(base, _) => matches!(
            base.as_str(),
            "uint" | "int" | "bytes" | "bstr"
        ),
        TypeExpr::VarArray(_) | TypeExpr::Optional(_) => false,
        TypeExpr::Tagged(_, inner) => is_builtin_primitive(inner),
    }
}

/// Emits encoding statements for a field value.
///
/// `accessor` is the expression that evaluates to the value (e.g.
/// `self.field_0` or `val`).
fn emit_encode_field(expr: &TypeExpr, accessor: &str, indent: &str) -> String {
    match expr {
        TypeExpr::Named(name) => {
            match name.as_str() {
                "uint" => format!("{indent}enc.unsigned({accessor});\n"),
                "int" => format!("{indent}enc.integer({accessor});\n"),
                "bool" => format!("{indent}enc.bool({accessor});\n"),
                "bytes" | "bstr" => format!("{indent}enc.bytes(&{accessor});\n"),
                "text" | "tstr" => format!("{indent}enc.text(&{accessor});\n"),
                _ => format!("{indent}{accessor}.encode_cbor(enc);\n"),
            }
        }
        TypeExpr::Sized(base, _) => {
            match base.as_str() {
                "uint" => format!("{indent}enc.unsigned({accessor} as u64);\n"),
                "int" => format!("{indent}enc.integer({accessor} as i64);\n"),
                "bytes" | "bstr" => format!("{indent}enc.bytes(&{accessor});\n"),
                _ => format!("{indent}{accessor}.encode_cbor(enc);\n"),
            }
        }
        TypeExpr::VarArray(inner) => {
            let mut s = format!("{indent}enc.array({accessor}.len() as u64);\n");
            s.push_str(&format!("{indent}for item in &{accessor} {{\n"));
            // For the loop body, the item is referenced as `*item` for
            // primitives and `item` for compound types.
            if is_builtin_primitive(inner) {
                s.push_str(&emit_encode_field(inner, "*item", &format!("{indent}    ")));
            } else {
                s.push_str(&emit_encode_field(inner, "item", &format!("{indent}    ")));
            }
            s.push_str(&format!("{indent}}}\n"));
            s
        }
        TypeExpr::Optional(inner) => {
            // Optional fields are handled at the map/field level, not here.
            // If called directly, encode the inner value.
            emit_encode_field(inner, accessor, indent)
        }
        TypeExpr::Tagged(tag, inner) => {
            let mut s = format!("{indent}enc.tag({tag});\n");
            s.push_str(&emit_encode_field(inner, accessor, indent));
            s
        }
    }
}

/// Emits a `let field_name = ...;` decode statement.
fn emit_decode_field(expr: &TypeExpr, field_name: &str, indent: &str) -> String {
    let rhs = emit_decode_expr(expr, indent);
    format!("{indent}let {field_name} = {rhs};\n")
}

/// Emits a `field_name = Some(...);` assignment for map key-dispatch decoding.
fn emit_decode_assign(expr: &TypeExpr, field_name: &str, indent: &str) -> String {
    let rhs = emit_decode_expr(expr, indent);
    format!("{indent}{field_name} = Some({rhs});\n")
}

/// Emits the decode expression for a type expression.
fn emit_decode_expr(expr: &TypeExpr, indent: &str) -> String {
    match expr {
        TypeExpr::Named(name) => {
            match name.as_str() {
                "uint" => "dec.unsigned()?".to_string(),
                "int" => "dec.integer()?".to_string(),
                "bool" => "dec.bool()?".to_string(),
                "bytes" | "bstr" => "dec.bytes()?.to_vec()".to_string(),
                "text" | "tstr" => "dec.text()?.to_string()".to_string(),
                other => format!("{}::decode_cbor(dec)?", to_rust_type_name(other)),
            }
        }
        TypeExpr::Sized(base, size) => {
            match base.as_str() {
                "uint" => {
                    match size {
                        8 => "dec.unsigned()?".to_string(),
                        _ => format!("dec.unsigned()? as {}", map_sized_type(base, *size)),
                    }
                }
                "int" => {
                    match size {
                        8 => "dec.integer()?".to_string(),
                        _ => format!("dec.integer()? as {}", map_sized_type(base, *size)),
                    }
                }
                "bytes" | "bstr" => {
                    format!(
                        "{{\n{indent}    let b = dec.bytes()?;\n{indent}    let arr: [{base_ty}; {size}] = b.try_into().map_err(|_| LedgerError::CborInvalidLength {{ expected: {size}, actual: b.len() }})?;\n{indent}    arr\n{indent}}}",
                        base_ty = "u8",
                    )
                }
                _ => format!("{}::decode_cbor(dec)?", to_rust_type_name(base)),
            }
        }
        TypeExpr::VarArray(inner) => {
            let inner_decode = emit_decode_expr(inner, &format!("{indent}    "));
            format!(
                "{{\n{indent}    let count = dec.array()?;\n{indent}    let mut v = Vec::with_capacity(count as usize);\n{indent}    for _ in 0..count {{\n{indent}        v.push({inner_decode});\n{indent}    }}\n{indent}    v\n{indent}}}"
            )
        }
        TypeExpr::Optional(inner) => {
            // Optional is handled at field level; if reached here, just decode inner.
            emit_decode_expr(inner, indent)
        }
        TypeExpr::Tagged(tag, inner) => {
            let inner_decode = emit_decode_expr(inner, indent);
            format!(
                "{{\n{indent}    let t = dec.tag()?;\n{indent}    if t != {tag} {{ return Err(LedgerError::CborInvalidAdditionalInfo(t as u8)); }}\n{indent}    {inner_decode}\n{indent}}}"
            )
        }
    }
}

fn map_field_name(f: &ParsedField) -> String {
    match &f.key {
        FieldKey::Label(label) => to_snake_case(label),
        FieldKey::Index(idx) => format!("field_{idx}"),
    }
}
