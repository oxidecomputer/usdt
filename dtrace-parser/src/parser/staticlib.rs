// Impl of types specific to the static library version of the crate.
use crate::parser::{DataType, File, Probe, Provider};
use textwrap::indent;

impl Probe {
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

impl Provider {
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

impl File {
    /// Return the C declarations of the providers and probes in this file
    pub fn to_c_declaration(&self) -> String {
        self.map_providers(Provider::to_c_declaration)
    }

    /// Return the C definitions of the providers and probes in this file
    pub fn to_c_definition(&self) -> String {
        self.map_providers(Provider::to_c_definition)
    }
}
