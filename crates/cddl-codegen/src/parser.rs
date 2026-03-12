use thiserror::Error;

/// A named CDDL type definition parsed from the supported schema subset.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParsedType {
    pub name: String,
    pub definition: TypeDefinition,
}

/// The supported CDDL shapes understood by the current parser.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TypeDefinition {
    /// A type alias: `name = type_expr`
    Alias(TypeExpr),
    /// A fixed-length array/tuple: `name = [field1, field2, ...]`
    Array(Vec<ArrayItem>),
    /// A map/record: `name = { key: type, ... }`
    Map(Vec<ParsedField>),
}

/// A parsed type expression, capturing the base type and optional constraints.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TypeExpr {
    /// A plain named type reference or builtin: `uint`, `bytes`, `hash32`.
    Named(String),
    /// A size-constrained type: `uint .size 4`, `bytes .size 32`.
    Sized(String, u64),
    /// A variable-length sequence: `[* element_type]`.
    VarArray(Box<TypeExpr>),
    /// An optional alternative with nil: `type / nil`.
    Optional(Box<TypeExpr>),
}

/// An item in a CDDL array definition, which may be named or positional.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArrayItem {
    /// Field name if provided (`name: type`), otherwise `None` for positional.
    pub name: Option<String>,
    pub ty: TypeExpr,
}

/// The key used for a map field—either a string label or an integer index.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FieldKey {
    /// A string-labeled field: `fee: uint`.
    Label(String),
    /// An integer-keyed field: `0: type` (CBOR map with integer keys).
    Index(u64),
}

/// A parsed map field within a CDDL map definition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParsedField {
    pub key: FieldKey,
    pub ty: TypeExpr,
    pub optional: bool,
}

/// Errors surfaced while parsing the supported CDDL subset.
#[derive(Debug, Error, Eq, PartialEq)]
pub enum ParseError {
    #[error("schema input is empty")]
    Empty,
    #[error("definition is missing '=': {0}")]
    MissingAssignment(String),
    #[error("type name is invalid: {0}")]
    InvalidTypeName(String),
    #[error("type definition is empty for: {0}")]
    EmptyDefinition(String),
    #[error("map field is invalid: {0}")]
    InvalidField(String),
    #[error("invalid size constraint: {0}")]
    InvalidSize(String),
}

/// Parses a restricted, deterministic subset of CDDL into named definitions.
///
/// Supported constructs:
/// - Comments (`;`), multi-line definitions via nesting depth tracking.
/// - Aliases: `name = type_expr`
/// - Flat arrays: `name = [item1, item2, ...]` with optional named fields.
/// - Maps: `name = { key: type, ... }` with string or integer keys.
/// - Size constraints: `uint .size N`, `bytes .size N`.
/// - Variable-length arrays: `[* type]`.
/// - Optional fields: `? key: type`.
/// - Nil alternatives: `type / nil`.
pub fn parse_schema(schema: &str) -> Result<Vec<ParsedType>, ParseError> {
    let mut parsed = Vec::new();
    let statements = collect_statements(schema);

    for line in statements {
        let line = line.trim();

        let (name, definition) = line
            .split_once('=')
            .ok_or_else(|| ParseError::MissingAssignment(line.to_string()))?;
        let name = name.trim();
        let definition = definition.trim();

        if !is_valid_type_name(name) {
            return Err(ParseError::InvalidTypeName(name.to_string()));
        }

        if definition.is_empty() {
            return Err(ParseError::EmptyDefinition(name.to_string()));
        }

        parsed.push(ParsedType {
            name: name.to_string(),
            definition: parse_definition(definition)?,
        });
    }

    if parsed.is_empty() {
        return Err(ParseError::Empty);
    }

    Ok(parsed)
}

/// Collects logical CDDL statements from raw source, joining multi-line
/// definitions by tracking brace/bracket nesting depth.
fn collect_statements(schema: &str) -> Vec<String> {
    let mut statements = Vec::new();
    let mut current = String::new();
    let mut nesting_depth = 0_i32;

    for line in schema.lines() {
        let line = strip_comment(line).trim();

        if line.is_empty() {
            continue;
        }

        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(line);
        nesting_depth += nesting_delta(line);

        if nesting_depth <= 0 && !current.trim().ends_with('=') {
            statements.push(current.trim().to_string());
            current.clear();
            nesting_depth = 0;
        }
    }

    if !current.trim().is_empty() {
        statements.push(current.trim().to_string());
    }

    statements
}

fn nesting_delta(line: &str) -> i32 {
    line.chars().fold(0_i32, |depth, ch| {
        depth
            + match ch {
                '{' | '[' => 1,
                '}' | ']' => -1,
                _ => 0,
            }
    })
}

fn parse_definition(definition: &str) -> Result<TypeDefinition, ParseError> {
    if definition.starts_with('{') && definition.ends_with('}') {
        return parse_map(definition);
    }

    if definition.starts_with('[') && definition.ends_with(']') {
        let inner = definition[1..definition.len() - 1].trim();
        // [* type] is a variable-length array type, not a fixed-position tuple.
        if inner.starts_with('*') {
            return Ok(TypeDefinition::Alias(parse_type_expr(definition)?));
        }
        return parse_array(inner);
    }

    Ok(TypeDefinition::Alias(parse_type_expr(definition)?))
}

/// Parses a type expression string into a `TypeExpr`.
///
/// Handles: plain names, `.size N` constraints, `[* type]` var-arrays,
/// and `type / nil` optionals.
fn parse_type_expr(expr: &str) -> Result<TypeExpr, ParseError> {
    let expr = expr.trim();

    // Variable-length array: [* type]
    if expr.starts_with('[') && expr.ends_with(']') {
        let inner = expr[1..expr.len() - 1].trim();
        if let Some(rest) = inner.strip_prefix('*') {
            let element = parse_type_expr(rest.trim())?;
            return Ok(TypeExpr::VarArray(Box::new(element)));
        }
    }

    // Nil alternative: type / nil
    if let Some((left, right)) = split_nil_alternative(expr) {
        if right == "nil" {
            return Ok(TypeExpr::Optional(Box::new(parse_type_expr(left)?)));
        }
    }

    // Size constraint: type .size N
    if let Some((base, size_str)) = split_size_constraint(expr) {
        let size = size_str
            .parse::<u64>()
            .map_err(|_| ParseError::InvalidSize(expr.to_string()))?;
        return Ok(TypeExpr::Sized(base.to_string(), size));
    }

    Ok(TypeExpr::Named(expr.to_string()))
}

/// Splits `type / nil` alternatives. Only recognizes the simple case
/// of exactly one `/` with `nil` on one side.
fn split_nil_alternative(expr: &str) -> Option<(&str, &str)> {
    let idx = expr.find('/')?;
    let left = expr[..idx].trim();
    let right = expr[idx + 1..].trim();
    if left.is_empty() || right.is_empty() {
        return None;
    }
    Some((left, right))
}

/// Splits `type .size N` into `(base_type, size_string)`.
fn split_size_constraint(expr: &str) -> Option<(&str, &str)> {
    let idx = expr.find(".size")?;
    let base = expr[..idx].trim();
    let size_str = expr[idx + 5..].trim();
    if base.is_empty() || size_str.is_empty() {
        return None;
    }
    Some((base, size_str))
}

fn parse_array(body: &str) -> Result<TypeDefinition, ParseError> {
    let items = split_items(body);
    let mut result = Vec::new();

    for item in items {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }

        // Check for named field: `name: type` (but not integer-keyed)
        if let Some((name, ty_str)) = try_split_named_field(item) {
            result.push(ArrayItem {
                name: Some(name.to_string()),
                ty: parse_type_expr(ty_str)?,
            });
        } else {
            result.push(ArrayItem {
                name: None,
                ty: parse_type_expr(item)?,
            });
        }
    }

    Ok(TypeDefinition::Array(result))
}

/// Attempts to split `name: type` for array fields. Returns `None` if
/// the left side looks like an integer (which would be an integer key).
fn try_split_named_field(item: &str) -> Option<(&str, &str)> {
    let (name, ty) = item.split_once(':')?;
    let name = name.trim();
    let ty = ty.trim();

    // Reject if the "name" is actually an integer (for int-keyed maps).
    if name.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }

    if name.is_empty() || ty.is_empty() {
        return None;
    }

    Some((name, ty))
}

fn parse_map(definition: &str) -> Result<TypeDefinition, ParseError> {
    let body = &definition[1..definition.len() - 1];
    let mut fields = Vec::new();

    for field in split_items(body) {
        let field = field.trim().to_string();
        if field.is_empty() {
            continue;
        }

        // Check for optional marker: `? key: type`
        let (optional, rest) = if let Some(rest) = field.strip_prefix('?') {
            (true, rest.trim())
        } else {
            (false, field.as_str())
        };

        let Some((key_str, ty_str)) = rest.split_once(':') else {
            return Err(ParseError::InvalidField(field));
        };

        let key_str = key_str.trim();
        let ty_str = ty_str.trim();

        if key_str.is_empty() || ty_str.is_empty() {
            return Err(ParseError::InvalidField(field));
        }

        // Determine if the key is an integer index or a string label.
        let key = if let Ok(index) = key_str.parse::<u64>() {
            FieldKey::Index(index)
        } else {
            FieldKey::Label(key_str.to_string())
        };

        fields.push(ParsedField {
            key,
            ty: parse_type_expr(ty_str)?,
            optional,
        });
    }

    Ok(TypeDefinition::Map(fields))
}

fn split_items(body: &str) -> Vec<String> {
    body.split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn strip_comment(line: &str) -> &str {
    line.split_once(';').map_or(line, |(content, _)| content)
}

fn is_valid_type_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };

    (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
}
