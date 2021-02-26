// Module implementing the DTrace parsing logic
// Copyright 2021 Oxide Computer Company

use std::collections::HashSet;
use std::convert::TryFrom;
use std::fs;
use std::path::Path;

use pest::iterators::{Pair, Pairs};
use pest_derive::Parser;
use textwrap::indent;

use crate::DTraceError;

#[derive(Parser, Debug)]
#[grammar = "dtrace.pest"]
struct DTraceParser;

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
            DataType::String => "&str",
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
            DataType::String => "*const ::std::os::raw::c_char",
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
            "void _{}_{}({});",
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
        // This C code unpacks the tuple (length, pointer) that we construct on the Rust side from
        // a &str passed into the macro. Note that the local copy of a string is named the same as
        // the Rust string variable, but with "_" appended. `to_rust_impl` for details on how the
        // input to this function is packed.
        let unpack_string_argument = |(i, typ): (usize, &DataType)| match typ {
            DataType::String => Some(format!(
                concat!(
                    "const char* data = *(const char**) arg{i};\n",
                    "uint64_t size = *(const uint64_t*) (arg{i} + sizeof(char*));\n",
                    "char* arg{i}_ = malloc(size + 1);\n",
                    "assert(arg{i}_ != NULL);\n",
                    "memcpy(arg{i}_, data, size);\n",
                    "arg{i}_[size] = '\\0';\n",
                ),
                i = i,
            )),
            _ => None,
        };

        // Collect the part of the body that does the unpacking for each string argument.
        let unpacked_string_args = self
            .types()
            .iter()
            .enumerate()
            .filter_map(unpack_string_argument)
            .collect::<Vec<_>>()
            .join("\n");

        // Collect the argument names, adding the "_" to any string arguments to reference the
        // locally-malloc'd copy of the string.
        let body_conv = |(i, typ): (usize, &DataType)| {
            format!(
                "arg{}{}",
                i,
                if *typ == DataType::String { "_" } else { "" }
            )
        };

        // Create the actual FFI function body.
        let if_condition = &format!(
            "if ({provider}_{probe}_ENABLED()) {{",
            provider = provider.to_uppercase(),
            probe = self.name().to_uppercase(),
        );
        let probe_call = &format!(
            "{provider}_{probe}({body_arglist});",
            provider = provider.to_uppercase(),
            probe = self.name().to_uppercase(),
            body_arglist = self.map_arglist(body_conv),
        );
        let body = format!(
            "{if_condition}\n{unpacked}{probe_call}\n{brace}",
            if_condition = indent(if_condition, "    "),
            unpacked = indent(&unpacked_string_args, "        "),
            probe_call = indent(probe_call, "        "),
            brace = indent("}", "    "),
        );

        // Generate the function signature, and insert the above body.
        let arglist_conv = |(i, typ)| format!("{} arg{}", DataType::to_c_type(typ), i);
        format!(
            "void _{provider}_{probe}({arglist}) {{\n{body}\n}}",
            provider = provider,
            probe = self.name(),
            arglist = self.map_arglist(arglist_conv),
            body = body,
        )
    }

    /// Return the Rust macro corresponding to this probe signature.
    pub fn to_rust_impl(&self, provider: &str) -> String {
        // For most data types, a straight cast to the corresponding type in std::os::raw is
        // appropriate, so we can just do write the argument itself, which will be coerced at the
        // function call site.
        //
        // Strings are different. Rust strings are a pair of `*const u8` and a `usize` length. They
        // may contain intervening null bytes and may not be null-terminated. The approach here
        // relies on the layout of string slices, which is a "fat pointer": a C-layout of a pointer
        // to data and a length.
        //
        // - Pass a pointer to the actual &str object, casted to *const u8.
        // - Extract the data pointer
        // - Extract the length
        // - Allocate a buffer of `length + 1`, to guarantee NULL-termination
        // - Copy `length` bytes from the extracted pointer
        // - NULL-terminate
        let body_conv = |(i, typ)| {
            if matches!(typ, &DataType::String) {
                format!("::std::mem::transmute::<_, *const _>(&$arg{})", i)
            } else {
                format!("$arg{}", i)
            }
        };
        let transcriber = indent(
            &format!(
                "unsafe {{ _{provider}_{probe}({body_arglist}); }}",
                provider = provider,
                probe = self.name(),
                body_arglist = self.map_arglist(body_conv),
            ),
            "    ",
        );
        let matcher_conv = |(i, _)| format!("$arg{}:expr", i);
        let macro_body = format!(
            "({matcher}) => {{\n{transcriber}\n}};",
            matcher = self.map_arglist(matcher_conv),
            transcriber = transcriber,
        );
        format!(
            "macro_rules! {provider}_{probe} {{\n{macro_body}\n}}",
            provider = provider,
            probe = self.name(),
            macro_body = indent(&macro_body, "    "),
        )
    }

    /// Return the Rust FFI function definition which should appear in the an `extern "C"` FFI
    /// block.
    pub fn to_ffi_declaration(&self, provider: &str) -> String {
        let conv = |(i, typ)| format!("arg{}: {}", i, DataType::to_rust_ffi_type(typ));
        format!(
            "fn _{}_{}({});",
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
        let link_attr = format!("#[link(name = \"{link_name}\")]", link_name = link_name);
        let extern_body = self
            .probes()
            .iter()
            .map(|probe| probe.to_ffi_declaration(&self.name))
            .collect::<Vec<_>>()
            .join("\n");
        let impl_body = self
            .probes()
            .iter()
            .map(|probe| probe.to_rust_impl(&self.name))
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            concat!(
                "{link_attr}\n",
                "{extern_decl}\n",
                "{extern_body}\n",
                "{brace}\n",
                "{use_decl}\n",
                "{crate_decl}\n",
                "{impl_body}\n",
                "{brace}",
            ),
            link_attr = link_attr,
            extern_decl = "extern \"C\" {",
            extern_body = indent(&extern_body, "    "),
            brace = "}",
            use_decl = "#[macro_use]",
            crate_decl = format!("pub(crate) mod {} {{", self.name),
            impl_body = indent(&impl_body, "        "),
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
    use expectorate::assert_contents;
    use rstest::{fixture, rstest};

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
        let defn = "probe foo(uint8_t, uint16_t, uint16_t);";
        assert!(DTraceParser::parse(Rule::PROBE, defn).is_ok());
        assert!(DTraceParser::parse(Rule::PROBE, &defn[..defn.len() - 2]).is_err());
    }

    #[test]
    fn test_basic_provider() {
        let defn = r#"
            provider foo {
                probe bar();
                probe baz(char*, uint16_t, uint8_t);
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
                probe baz(char*, uint16_t, uint8_t);
            };"#;
        assert!(DTraceParser::parse(Rule::FILE, defn).is_ok());
    }

    #[test]
    fn test_pragma_provider() {
        let defn = r#"
            #pragma I am a robot
            provider foo {
                probe bar();
                probe baz(char*, uint16_t, uint8_t);
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
                probe baz(char*, uint16_t, uint8_t);
            };
            provider bar {
                probe bar();
                probe baz(char*, uint16_t, uint8_t);
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

    #[fixture]
    fn probe_data() -> (String, String) {
        let provider = String::from("foo");
        let probe = String::from("probe baz(char*, uint16_t, uint8_t);");
        (provider, probe)
    }

    #[fixture]
    fn probe(probe_data: (String, String)) -> (String, Probe) {
        (
            probe_data.0,
            Probe::try_from(&DTraceParser::parse(Rule::PROBE, &probe_data.1).unwrap()).unwrap(),
        )
    }

    #[rstest]
    fn test_probe_struct_parse(probe_data: (String, String)) {
        let (_, probe) = probe_data;
        let probe = Probe::try_from(&DTraceParser::parse(Rule::PROBE, &probe).unwrap())
            .expect("Could not parse probe tokens");
        assert_eq!(probe.name(), "baz");
        assert_eq!(
            probe.types(),
            &vec![DataType::String, DataType::U16, DataType::U8]
        );
    }

    fn data_file(name: &str) -> String {
        format!("{}/test-data/{}", env!("CARGO_MANIFEST_DIR"), name)
    }

    #[rstest]
    fn test_probe_to_c_declaration(probe: (String, Probe)) {
        let (provider, probe) = probe;
        let actual = probe.to_c_declaration(&provider);
        assert_contents(&data_file(&format!("{}-decl.txt", provider)), &actual);
    }

    #[rstest]
    fn test_probe_to_c_definition(probe: (String, Probe)) {
        let (provider, probe) = probe;
        let actual = probe.to_c_definition(&provider);
        assert_contents(&data_file(&format!("{}-defn.txt", &provider)), &actual);
    }

    #[test]
    fn test_provider_struct() {
        let provider_name = "foo";
        let defn = std::fs::read_to_string(&data_file(&format!("{}.d", provider_name))).unwrap();
        let provider = Provider::try_from(
            &DTraceParser::parse(Rule::FILE, &defn)
                .unwrap()
                .next()
                .unwrap()
                .into_inner(),
        );
        let provider = provider.unwrap();
        assert_eq!(provider.name(), provider_name);
        assert_eq!(provider.probes().len(), 1);
        assert_eq!(provider.probes()[0].name(), "baz");

        let actual = provider.to_rust_impl(&provider_name);
        let filename = data_file(&format!("{}-impl.txt", provider_name));
        assert_contents(&filename, &actual);
    }

    #[test]
    fn test_file_struct() {
        let defn = r#"
            /* a comment */
            #pragma do stuff
            provider foo {
                probe quux();
                probe quack(char*, uint16_t, uint8_t);
            };
            provider bar {
                probe bar();
                probe baz(char*, uint16_t, uint8_t);
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
