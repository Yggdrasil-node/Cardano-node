use crate::parser::ParsedType;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneratedModule {
    pub source: String,
}

pub fn generate_module(types: &[ParsedType]) -> GeneratedModule {
    let mut source = String::new();

    for parsed in types {
        source.push_str("#[derive(Clone, Debug, Eq, PartialEq)]\n");
        source.push_str(&format!("pub struct {} {{}}\n\n", parsed.name));
    }

    GeneratedModule { source }
}
