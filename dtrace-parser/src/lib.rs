//! A small library for parsing DTrace provider files.
// Copyright 2021 Oxide Computer Company

use std::collections::HashSet;
use std::convert::TryFrom;
use std::fs;
use std::path::Path;

use pest::iterators::{Pair, Pairs};
use pest_derive::Parser;
use thiserror::Error;

#[derive(Parser, Debug)]
#[grammar = "dtrace.pest"]
pub struct DTraceParser;

/// Type representing errors that occur when parsing a D file.
#[derive(Error, Debug)]
pub enum DTraceError {
    #[error("unexpected token type, expected {expected:?}, found {found:?}")]
    UnexpectedToken { expected: Rule, found: Rule },
    #[error("this set of pairs contains no tokens")]
    EmptyPairsIterator,
    #[error("probe names must be unique: duplicated \"{0:?}\"")]
    DuplicateProbeName((String, String)),
    #[error(transparent)]
    IO(#[from] std::io::Error),
    #[error("failed to parse according to the DTrace grammar:\n{0}")]
    ParseError(String),
}

// Helper which verifies that the given `pest::Pair` conforms to the expected grammar rule.
fn expect_token(pair: &Pair<'_, Rule>, rule: Rule) -> Result<(), DTraceError> {
    if pair.as_rule() == rule {
        Ok(())
    } else {
        Err(DTraceError::UnexpectedToken {
            expected: rule,
            found: pair.as_rule(),
        })
    }
}

/// Represents the data type of a single probe argument.
#[derive(Clone, Debug, PartialEq)]
pub enum DataType {
    U8,
    U16,
    U32,
    U64,
    I8,
    I16,
    I32,
    I64,
    String,
    Float,
    Double,
}

impl TryFrom<&Pair<'_, Rule>> for DataType {
    type Error = DTraceError;

    fn try_from(pair: &Pair<'_, Rule>) -> Result<DataType, Self::Error> {
        expect_token(pair, Rule::DATA_TYPE)?;
        let inner = pair
            .clone()
            .into_inner()
            .next()
            .expect("Data type token is expected to contain a concrete type");
        let typ = match inner.as_rule() {
            Rule::UNSIGNED_INT => match inner.into_inner().as_str() {
                "8" => DataType::U8,
                "16" => DataType::U16,
                "32" => DataType::U32,
                "64" => DataType::U64,
                _ => unreachable!(),
            },
            Rule::SIGNED_INT => match inner.into_inner().as_str() {
                "8" => DataType::I8,
                "16" => DataType::I16,
                "32" => DataType::I32,
                "64" => DataType::I64,
                _ => unreachable!(),
            },
            Rule::FLOAT => DataType::Float,
            Rule::DOUBLE => DataType::Double,
            Rule::STRING => DataType::String,
            _ => unreachable!("Parsed an unexpected DATA_TYPE token"),
        };
        Ok(typ)
    }
}

impl TryFrom<&Pairs<'_, Rule>> for DataType {
    type Error = DTraceError;

    fn try_from(pairs: &Pairs<'_, Rule>) -> Result<DataType, Self::Error> {
        DataType::try_from(&pairs.peek().ok_or(DTraceError::EmptyPairsIterator)?)
    }
}

impl DataType {
    /// Return the C type representation of this data type.
    pub fn to_c_type(&self) -> String {
        match self {
            DataType::U8 => "uint8_t",
            DataType::U16 => "uint16_t",
            DataType::U32 => "uint32_t",
            DataType::U64 => "uint64_t",
            DataType::I8 => "int8_t",
            DataType::I16 => "int16_t",
            DataType::I32 => "int32_t",
            DataType::I64 => "int64_t",
            DataType::Float => "float",
            DataType::Double => "double",
            DataType::String => "const char*",
        }
        .into()
    }

    /// Return the Rust type representation of this data type.
    pub fn to_rust_type(&self) -> String {
        match self {
            DataType::U8 => "u8",
            DataType::U16 => "u16",
            DataType::U32 => "u32",
            DataType::U64 => "u64",
            DataType::I8 => "i8",
            DataType::I16 => "i16",
            DataType::I32 => "i32",
            DataType::I64 => "i64",
            DataType::Float => "f32",
            DataType::Double => "f64",
            DataType::String => "String",
        }
        .into()
    }

    /// Return the Rust FFI type representation of this data type
    pub fn to_rust_ffi_type(&self) -> String {
        match self {
            DataType::U8 => "::std::os::raw::c_uchar",
            DataType::U16 => "::std::os::raw::c_ushort",
            DataType::U32 => "::std::os::raw::c_uint",
            DataType::U64 => "::std::os::raw::c_ulonglong",
            DataType::I8 => "::std::os::raw::c_schar",
            DataType::I16 => "::std::os::raw::c_short",
            DataType::I32 => "::std::os::raw::c_int",
            DataType::I64 => "::std::os::raw::c_longlong",
            DataType::Float => "::std::os::raw::c_float",
            DataType::Double => "::std::os::raw::c_double",
            DataType::String => "*const ::std::os::raw::c_char",
        }
        .into()
    }

    /// Return the code which converts the data type from Rust to C
    pub fn rust_to_c(&self) -> String {
        match self {
            DataType::String => ".as_ptr() as *const _",
            _ => " as _",
        }
        .into()
    }
}

/// Type representing a single D probe definition within a provider.
#[derive(Clone, Debug, PartialEq)]
pub struct Probe {
    name: String,
    types: Vec<DataType>,
}

impl Probe {
    /// Return the name of this probe.
    pub fn name(&self) -> &String {
        &self.name
    }

    /// Return the list of data types in this probe's signature.
    pub fn types(&self) -> &Vec<DataType> {
        &self.types
    }

    // Map a function to this probe's list of arguments, used in various conversions.
    fn map_arglist<'a>(&'a self, converter: impl Fn((usize, &'a DataType)) -> String) -> String {
        self.types()
            .iter()
            .enumerate()
            .map(converter)
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Return the C function declaration corresponding to this probe signature.
    ///
    /// This requires the name of the provider in which this probe is defined, to correctly
    /// generate the body of the function (which calls a defined C function).
    pub fn to_c_declaration(&self, provider: &str) -> String {
        let conv = |(i, typ)| format!("{} arg{}", DataType::to_c_type(typ), i);
        format!(
            "void {}_{}({});",
            provider,
            self.name(),
            self.map_arglist(conv)
        )
    }

    /// Return the C function definition corresponding to this probe signature.
    ///
    /// This requires the name of the provider in which this probe is defined, to correctly
    /// generate the body of the function (which calls a defined C function).
    pub fn to_c_definition(&self, provider: &str) -> String {
        // The function body is generated by just listing the argument names, and calling into the
        // capitalized function `PROVIDER_PROBE`.
        let body_conv = |(i, _): (usize, &DataType)| format!("arg{}", i);
        let body_arglist = self.map_arglist(body_conv);
        let body = format!(
            "{}_{}({});",
            provider.to_uppercase(),
            self.name().to_uppercase(),
            body_arglist
        );

        // Generate the function signature, and insert the above body.
        let arglist_conv = |(i, typ)| format!("{} arg{}", DataType::to_c_type(typ), i);
        format!(
            "void {}_{}({}) {{ {} }}",
            provider,
            self.name(),
            self.map_arglist(arglist_conv),
            body,
        )
    }

    /// Return the Rust macro corresponding to this probe signature.
    pub fn to_rust_impl(&self, provider: &str) -> String {
        // The macro body contains the Rust-to-C conversion operators. For example, given a
        // String `x`, this is passed to the C FFI function as `x.as_ptr() as *const _`.
        let body_conv = |(i, typ)| format!("$arg{}{}", i, DataType::rust_to_c(typ));
        let body_arglist = self.map_arglist(body_conv);
        let body = format!(
            "unsafe {{ {}_{}({}); }}",
            provider,
            self.name(),
            body_arglist
        );
        let matcher_conv = |(i, _)| format!("$arg{}:expr", i);
        format!(
            "macro_rules! {}_{} {{ ({}) => {{ {} }}; }}",
            provider,
            self.name(),
            self.map_arglist(matcher_conv),
            body
        )
    }

    /// Return the Rust FFI function definition which should appear in the an `extern "C"` FFI
    /// block.
    pub fn to_ffi_declaration(&self, provider: &str) -> String {
        let conv = |(i, typ)| format!("arg{}: {}", i, DataType::to_rust_ffi_type(typ));
        format!(
            "fn {}_{}({});",
            provider,
            self.name(),
            self.map_arglist(conv)
        )
    }
}

impl TryFrom<&Pair<'_, Rule>> for Probe {
    type Error = DTraceError;

    fn try_from(pair: &Pair<'_, Rule>) -> Result<Self, Self::Error> {
        expect_token(pair, Rule::PROBE)?;
        let mut inner = pair.clone().into_inner();
        expect_token(
            &inner.next().expect("Expected the literal 'probe'"),
            Rule::PROBE_KEY,
        )?;
        let name = inner
            .next()
            .expect("Expected a probe name")
            .as_str()
            .to_string();
        expect_token(
            &inner.next().expect("Expected the literal '('"),
            Rule::LEFT_PAREN,
        )?;
        let possibly_argument_list = inner
            .next()
            .expect("Expected an argument list or literal ')'");
        let mut types = Vec::new();
        if expect_token(&possibly_argument_list, Rule::ARGUMENT_LIST).is_ok() {
            let arguments = possibly_argument_list.clone().into_inner();
            for data_type in arguments {
                expect_token(&data_type, Rule::DATA_TYPE)?;
                types.push(DataType::try_from(&data_type)?);
            }
        }
        expect_token(
            &inner.next().expect("Expected a literal ')'"),
            Rule::RIGHT_PAREN,
        )?;
        expect_token(
            &inner.next().expect("Expected a literal ';'"),
            Rule::SEMICOLON,
        )?;
        Ok(Probe { name, types })
    }
}

impl TryFrom<&Pairs<'_, Rule>> for Probe {
    type Error = DTraceError;

    fn try_from(pairs: &Pairs<'_, Rule>) -> Result<Self, Self::Error> {
        Probe::try_from(&pairs.peek().ok_or(DTraceError::EmptyPairsIterator)?)
    }
}

/// Type representing a single DTrace provider and all of its probes.
#[derive(Debug, Clone, PartialEq)]
pub struct Provider {
    name: String,
    probes: Vec<Probe>,
}

impl Provider {
    /// Return the name of this provider
    pub fn name(&self) -> &String {
        &self.name
    }

    /// Return the list of probes this provider defines.
    pub fn probes(&self) -> &Vec<Probe> {
        &self.probes
    }

    /// Return a Rust type representing this provider and its probes.
    ///
    /// This must be given the name of the library against which to link, which should be the
    /// filename of the D provider file.
    pub fn to_rust_impl(&self, link_name: &str) -> String {
        // This includes:
        // - The library's link name.
        // - Extern C FFI declarations.
        // - The probe implementation macros, which call the FFI functions.
        format!(
            "{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
            format!("#[link(name = \"{}\")]", link_name),
            "extern \"C\" {",
            self.probes()
                .iter()
                .map(|probe| probe.to_ffi_declaration(&self.name))
                .collect::<Vec<_>>()
                .join("\n"),
            "}",
            "#[macro_use]",
            format!("pub(crate) mod {} {{", self.name),
            self.probes
                .iter()
                .map(|probe| probe.to_rust_impl(&self.name))
                .collect::<Vec<_>>()
                .join("\n"),
            "}",
        )
    }

    /// Return the C-style function declarations implied by this provider's probes.
    pub fn to_c_declaration(&self) -> String {
        self.probes
            .iter()
            .map(|probe| probe.to_c_declaration(&self.name))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Return the C-style function definitions implied by this provider's probes.
    pub fn to_c_definition(&self) -> String {
        self.probes
            .iter()
            .map(|probe| probe.to_c_definition(&self.name))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl TryFrom<&Pair<'_, Rule>> for Provider {
    type Error = DTraceError;

    fn try_from(pair: &Pair<'_, Rule>) -> Result<Self, Self::Error> {
        expect_token(pair, Rule::PROVIDER)?;
        let mut inner = pair.clone().into_inner();
        expect_token(
            &inner.next().expect("Expected the literal 'provider'"),
            Rule::PROVIDER_KEY,
        )?;
        let name = inner
            .next()
            .expect("Expected a provider name")
            .as_str()
            .to_string();
        expect_token(
            &inner.next().expect("Expected the literal '{'"),
            Rule::LEFT_BRACE,
        )?;
        let mut probes = Vec::new();
        let mut possibly_probe = inner
            .next()
            .expect("Expected at least one probe in the provider");
        while expect_token(&possibly_probe, Rule::PROBE).is_ok() {
            probes.push(Probe::try_from(&possibly_probe)?);
            possibly_probe = inner.next().expect("Expected a token");
        }
        expect_token(&possibly_probe, Rule::RIGHT_BRACE)?;
        expect_token(
            &inner.next().expect("Expected a literal ';'"),
            Rule::SEMICOLON,
        )?;
        Ok(Provider { name, probes })
    }
}

impl TryFrom<&Pairs<'_, Rule>> for Provider {
    type Error = DTraceError;

    fn try_from(pairs: &Pairs<'_, Rule>) -> Result<Self, Self::Error> {
        Provider::try_from(&pairs.peek().ok_or(DTraceError::EmptyPairsIterator)?)
    }
}

/// Type representing a single D file and all the providers it defines.
#[derive(Debug, Clone, PartialEq)]
pub struct File {
    name: String,
    providers: Vec<Provider>,
}

impl TryFrom<&Pair<'_, Rule>> for File {
    type Error = DTraceError;

    fn try_from(pair: &Pair<'_, Rule>) -> Result<Self, Self::Error> {
        expect_token(&pair, Rule::FILE)?;
        let mut providers = Vec::new();
        let mut names = HashSet::new();
        for item in pair.clone().into_inner() {
            if item.as_rule() == Rule::PROVIDER {
                let provider = Provider::try_from(&item)?;
                for probe in provider.probes() {
                    let name = (provider.name().clone(), probe.name().clone());
                    if names.contains(&name) {
                        return Err(DTraceError::DuplicateProbeName(name));
                    }
                    names.insert(name.clone());
                }
                providers.push(provider);
            }
        }

        Ok(File {
            name: "".to_string(),
            providers,
        })
    }
}

impl TryFrom<&Pairs<'_, Rule>> for File {
    type Error = DTraceError;

    fn try_from(pairs: &Pairs<'_, Rule>) -> Result<Self, Self::Error> {
        File::try_from(&pairs.peek().ok_or(DTraceError::EmptyPairsIterator)?)
    }
}

impl File {
    /// Load and parse a provider from a D file at the given path.
    pub fn from_file(filename: &Path) -> Result<Self, DTraceError> {
        let mut f = File::try_from(fs::read_to_string(filename)?.as_str())?;
        f.name = filename
            .file_stem()
            .unwrap()
            .to_os_string()
            .into_string()
            .unwrap();
        Ok(f)
    }

    /// Return the name of the file.
    pub fn name(&self) -> &String {
        &self.name
    }

    /// Return the list of providers this file defines.
    pub fn providers(&self) -> &Vec<Provider> {
        &self.providers
    }

    // Helper to map a function to each provider
    fn map_providers<'a>(&'a self, f: impl Fn(&'a Provider) -> String) -> String {
        self.providers.iter().map(f).collect::<Vec<_>>().join("\n")
    }

    /// Return the C declarations of the providers and probes in this file
    pub fn to_c_declaration(&self) -> String {
        self.map_providers(Provider::to_c_declaration)
    }

    /// Return the C definitions of the providers and probes in this file
    pub fn to_c_definition(&self) -> String {
        self.map_providers(Provider::to_c_definition)
    }

    /// Return the Rust implementation of the providers and probes in this file
    pub fn to_rust_impl(&self) -> String {
        self.map_providers(|provider| provider.to_rust_impl(&self.name))
    }
}

impl TryFrom<&str> for File {
    type Error = DTraceError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        use pest::Parser;
        File::try_from(
            &DTraceParser::parse(Rule::FILE, s)
                .map_err(|e| DTraceError::ParseError(e.to_string()))?,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{DTraceParser, DataType, File, Probe, Provider, Rule, TryFrom};
    use ::pest::Parser;

    use rstest::rstest;

    #[rstest(
        token,
        rule,
        case("probe", Rule::PROBE_KEY),
        case("provider", Rule::PROVIDER_KEY),
        case(";", Rule::SEMICOLON),
        case("(", Rule::LEFT_PAREN),
        case(")", Rule::RIGHT_PAREN),
        case("{", Rule::LEFT_BRACE),
        case("}", Rule::RIGHT_BRACE)
    )]
    fn test_basic_tokens(token: &str, rule: Rule) {
        assert!(DTraceParser::parse(rule, token).is_ok());
    }

    #[test]
    #[should_panic]
    fn test_bad_basic_token() {
        assert!(DTraceParser::parse(Rule::LEFT_BRACE, "x").is_ok())
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
    fn test_basic_provider() {
        let defn = r#"
            provider foo {
                probe bar();
                probe baz(char*, float, uint8_t);
            };"#;
        println!("{:?}", DTraceParser::parse(Rule::FILE, defn));
        assert!(DTraceParser::parse(Rule::FILE, defn).is_ok());
        assert!(DTraceParser::parse(Rule::FILE, &defn[..defn.len() - 2]).is_err());
    }

    #[test]
    fn test_null_provider() {
        let defn = "provider foo { };";
        assert!(DTraceParser::parse(Rule::FILE, defn).is_err());
    }

    #[test]
    fn test_comment_provider() {
        let defn = r#"
            /* Check out this fly provider */
            provider foo {
                probe bar();
                probe baz(char*, float, uint8_t);
            };"#;
        assert!(DTraceParser::parse(Rule::FILE, defn).is_ok());
    }

    #[test]
    fn test_pragma_provider() {
        let defn = r#"
            #pragma I am a robot
            provider foo {
                probe bar();
                probe baz(char*, float, uint8_t);
            };
            "#;
        println!("{}", defn);
        assert!(DTraceParser::parse(Rule::FILE, defn).is_ok());
    }

    #[test]
    fn test_two_providers() {
        let defn = r#"
            provider foo {
                probe bar();
                probe baz(char*, float, uint8_t);
            };
            provider bar {
                probe bar();
                probe baz(char*, float, uint8_t);
            };
            "#;
        println!("{}", defn);
        assert!(DTraceParser::parse(Rule::FILE, defn).is_ok());
    }

    #[rstest(
        defn,
        data_type,
        case("uint8_t", DataType::U8),
        case("uint16_t", DataType::U16),
        case("uint32_t", DataType::U32),
        case("uint64_t", DataType::U64),
        case("int8_t", DataType::I8),
        case("int16_t", DataType::I16),
        case("int32_t", DataType::I32),
        case("int64_t", DataType::I64),
        case("float", DataType::Float),
        case("double", DataType::Double),
        case("char*", DataType::String)
    )]
    fn test_data_type_enum(defn: &str, data_type: DataType) {
        let dtype =
            DataType::try_from(&DTraceParser::parse(Rule::DATA_TYPE, defn).unwrap()).unwrap();
        assert_eq!(dtype, data_type);
    }

    #[test]
    fn test_data_type_conversion() {
        let dtype =
            DataType::try_from(&DTraceParser::parse(Rule::DATA_TYPE, "uint8_t").unwrap()).unwrap();
        assert_eq!(dtype.to_rust_ffi_type(), "::std::os::raw::c_uchar");
    }

    #[test]
    fn test_probe_struct() {
        let defn = "probe baz(char*, float, uint8_t);";
        let probe = Probe::try_from(&DTraceParser::parse(Rule::PROBE, defn).unwrap())
            .expect("Could not parse probe tokens");
        let provider = "foo";
        assert_eq!(probe.name(), "baz");
        assert_eq!(
            probe.types(),
            &vec![DataType::String, DataType::Float, DataType::U8]
        );

        assert_eq!(
            probe.to_c_declaration(provider),
            "void foo_baz(const char* arg0, float arg1, uint8_t arg2);"
        );
        assert_eq!(
            probe.to_rust_impl(provider),
            concat!(
                "macro_rules! foo_baz { ",
                "($arg0:expr, $arg1:expr, $arg2:expr) => ",
                "{ unsafe { foo_baz($arg0.as_ptr() as *const _, $arg1 as _, $arg2 as _); } }; }",
            )
        );

        assert_eq!(
            probe.to_ffi_declaration(provider),
            concat!(
                "fn foo_baz(arg0: *const ::std::os::raw::c_char, ",
                "arg1: ::std::os::raw::c_float, arg2: ::std::os::raw::c_uchar);"
            )
        );
    }

    #[test]
    fn test_provider_struct() {
        let defn = r#"
            provider foo {
                probe bar();
                probe baz(char*, float, uint8_t);
            };"#;
        let provider = Provider::try_from(
            &DTraceParser::parse(Rule::FILE, defn)
                .unwrap()
                .next()
                .unwrap()
                .into_inner(),
        );
        let provider = provider.unwrap();
        assert_eq!(provider.probes().len(), 2);
        assert_eq!(provider.name(), "foo");
        assert_eq!(provider.probes()[0].name(), "bar");

        let expected = concat!(
            "#[link(name = \"name\")]\n",
            "extern \"C\" {\n",
            "fn foo_bar();\n",
            "fn foo_baz(arg0: *const ::std::os::raw::c_char, ",
            "arg1: ::std::os::raw::c_float, arg2: ::std::os::raw::c_uchar);\n",
            "}\n",
            "#[macro_use]\n",
            "pub(crate) mod foo {\n",
            "macro_rules! foo_bar { () => { unsafe { foo_bar(); } }; }\n",
            "macro_rules! foo_baz { ($arg0:expr, $arg1:expr, $arg2:expr) => ",
            "{ unsafe { foo_baz($arg0.as_ptr() as *const _, $arg1 as _, $arg2 as _); } }; }\n",
            "}",
        );
        let actual = provider.to_rust_impl("name");
        println!("{}\n{}", actual, expected);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_file_struct() {
        let defn = r#"
            /* a comment */
            #pragma do stuff
            provider foo {
                probe quux();
                probe quack(char*, float, uint8_t);
            };
            provider bar {
                probe bar();
                probe baz(char*, float, uint8_t);
            };
            "#;
        let file = File::try_from(&DTraceParser::parse(Rule::FILE, defn).unwrap()).unwrap();
        assert_eq!(file.providers().len(), 2);
        assert_eq!(file.providers()[0].name(), "foo");
        assert_eq!(file.providers()[1].probes()[1].name(), "baz");

        let file2 = File::try_from(defn).unwrap();
        assert_eq!(file, file2);

        assert!(File::try_from("this is not a D file").is_err());
    }
}
