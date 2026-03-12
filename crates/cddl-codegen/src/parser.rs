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
    Alias(String),
    Array(Vec<String>),
    Map(Vec<ParsedField>),
}

/// A parsed map field within a CDDL map definition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParsedField {
    pub name: String,
    pub ty: String,
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
}

/// Parses a restricted, deterministic subset of CDDL into named definitions.
///
/// The current parser supports comments, aliases, flat arrays, and flat maps,
/// including multi-line definitions for those shapes.
pub fn parse_schema(schema: &str) -> Result<Vec<ParsedType>, ParseError> {
    let mut parsed = Vec::new();
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

        if nesting_depth <= 0 {
            statements.push(current.trim().to_string());
            current.clear();
            nesting_depth = 0;
        }
    }

    if !current.trim().is_empty() {
        statements.push(current.trim().to_string());
    }

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
        return Ok(TypeDefinition::Array(parse_sequence_items(
            &definition[1..definition.len() - 1],
        )));
    }

    Ok(TypeDefinition::Alias(definition.to_string()))
}

fn parse_map(definition: &str) -> Result<TypeDefinition, ParseError> {
    let body = &definition[1..definition.len() - 1];
    let mut fields = Vec::new();

    for field in split_items(body) {
        let Some((name, ty)) = field.split_once(':') else {
            return Err(ParseError::InvalidField(field));
        };

        let field_name = name.trim();
        let field_ty = ty.trim();

        if field_name.is_empty() || field_ty.is_empty() {
            return Err(ParseError::InvalidField(field));
        }

        fields.push(ParsedField {
            name: field_name.to_string(),
            ty: field_ty.to_string(),
        });
    }

    Ok(TypeDefinition::Map(fields))
}

fn parse_sequence_items(body: &str) -> Vec<String> {
    split_items(body)
        .into_iter()
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect()
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
