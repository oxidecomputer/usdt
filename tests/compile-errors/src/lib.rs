#![feature(asm)]
#![deny(warnings)]

#[cfg(test)]
mod tests {
    #[test]
    fn test_compile_errors() {
        let t = trybuild::TestCases::new();
        t.compile_fail("src/type-mismatch.rs");
        t.compile_fail("src/unsupported-type.rs");
        t.compile_fail("src/no-closure.rs");
        t.compile_fail("src/no-provider-file.rs");
        t.compile_fail("src/zero-arg-probe-type-check.rs");
        t.compile_fail("src/different-serializable-type.rs");
    }
}
