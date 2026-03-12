use thiserror::Error;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParsedType {
    pub name: String,
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum ParseError {
    #[error("schema input is empty")]
    Empty,
}

pub fn parse_schema(schema: &str) -> Result<Vec<ParsedType>, ParseError> {
    let mut parsed = Vec::new();

    for line in schema.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let name = line
            .split('=' )
            .next()
            .map(str::trim)
            .filter(|segment| !segment.is_empty())
            .ok_or(ParseError::Empty)?;
        parsed.push(ParsedType {
            name: name.to_string(),
        });
    }

    if parsed.is_empty() {
        return Err(ParseError::Empty);
    }

    Ok(parsed)
}
