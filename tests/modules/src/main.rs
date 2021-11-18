#![feature(asm)]
#![cfg_attr(target_os = "macos", feature(asm_sym))]

mod inner;

fn main() {
    usdt::register_probes().expect("Could not register probes");
    // Verify that we can call the probe from its full path.
    inner::probes::am_i_visible!(|| ());

    // This is an overly-cautious test for macOS. We define extern symbols inside each expanded
    // probe macro, with a link-name for a symbol that the macOS linker will generate for us. This
    // checks that there is no issue defining these locally-scoped extern symbols multiple times.
    inner::probes::am_i_visible!(|| ());
}
