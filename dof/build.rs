use std::env;
use std::path::PathBuf;

fn main() {
    let whitelist = ".*dof_.*|.*DOF_.*|dt_node";
    let bindings = bindgen::Builder::default()
        .header("src/dtrace.h")
        .ignore_functions()
        .layout_tests(false)
        .rustfmt_bindings(true)
        .disable_untagged_union()
        .derive_default(true)
        .whitelist_type(whitelist)
        .whitelist_var(whitelist)
        .generate()
        .unwrap();
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    std::fs::write(
        out_dir.join("bindings.rs"),
        format!(
            "use zerocopy::{{AsBytes, FromBytes}};\n{}",
            bindings
                .to_string()
                .split("\n")
                .map(|line| line.replace("#[derive(", "#[derive(AsBytes, FromBytes, "))
                .collect::<Vec<_>>()
                .join("\n")
        ),
    )
    .unwrap();
}
