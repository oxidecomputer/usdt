//! Tests of the DTrace provider grammar parsing.
// Copyright 2021 Oxide Computer Company

use pest_derive::Parser;

#[derive(Parser, Debug)]
#[grammar = "dtrace.pest"]
pub(crate) struct DTraceParser;

#[cfg(test)]
mod tests {
    use super::{DTraceParser, Rule};
    use ::pest::Parser;

    #[test]
    fn test_basic_tokens() {
        let tokens = [
            ("probe", Rule::PROBE_KEY),
            ("provider", Rule::PROVIDER_KEY),
            (";", Rule::SEMICOLON),
            ("(", Rule::LEFT_PAREN),
            (")", Rule::RIGHT_PAREN),
            ("{", Rule::LEFT_BRACE),
            ("}", Rule::RIGHT_BRACE),
        ];
        for &(token, rule) in &tokens {
            assert!(DTraceParser::parse(rule, token).is_ok());
        }
        assert!(DTraceParser::parse(Rule::LEFT_BRACE, "x").is_err());
    }

    #[test]
    fn test_identifier() {
        assert!(DTraceParser::parse(Rule::IDENTIFIER, "foo").is_ok());
        assert!(DTraceParser::parse(Rule::IDENTIFIER, "foo_bar").is_ok());
        assert!(DTraceParser::parse(Rule::IDENTIFIER, "foo9").is_ok());

        assert!(DTraceParser::parse(Rule::IDENTIFIER, "_bar").is_err());
        assert!(DTraceParser::parse(Rule::IDENTIFIER, "").is_err());
        assert!(DTraceParser::parse(Rule::IDENTIFIER, "9foo").is_err());
    }

    #[test]
    fn test_data_types() {
        assert!(DTraceParser::parse(Rule::DATA_TYPE, "uint8_t").is_ok());
        assert!(DTraceParser::parse(Rule::DATA_TYPE, "int").is_err());
        assert!(DTraceParser::parse(Rule::DATA_TYPE, "flaot").is_err());
    }

    #[test]
    fn test_probe() {
        let defn = "probe foo(uint8_t, float, float);";
        assert!(DTraceParser::parse(Rule::PROBE, defn).is_ok());
        assert!(DTraceParser::parse(Rule::PROBE, &defn[..defn.len() - 2]).is_err());
    }

    #[test]
    fn test_provider() {
        let defn = r#"
            provider foo {
                probe bar();
                probe baz(string, float, uint8_t);
            };"#;
        assert!(DTraceParser::parse(Rule::PROVIDER, defn).is_ok());
        assert!(DTraceParser::parse(Rule::PROVIDER, &defn[..defn.len() - 2]).is_err());

        let defn = "provider foo { };";
        assert!(DTraceParser::parse(Rule::PROVIDER, defn).is_err());
    }
}
