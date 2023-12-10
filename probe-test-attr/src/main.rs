//! Example using the `usdt` crate, defining probes inline in Rust code which accept any
//! serializable data type.

// Copyright 2022 Oxide Computer Company
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

#![cfg_attr(usdt_need_feat_asm, feature(asm))]
#![cfg_attr(usdt_need_feat_asm_sym, feature(asm_sym))]

use serde::Serialize;

/// By deriving the `serde::Serialize` trait, the `Arg` struct can be used as an argument to a
/// DTrace probe. DTrace provides the `json` function, which accepts a JSON-encoded string and a
/// (possibly-nested) key, and prints the corresponding value of the JSON object. For example, in
/// this case one could use the DTrace snippet:
///
/// ```bash
/// $ dtrace -n 'stop { printf("arg.x = %s", json(copyinstr(arg1), "ok.x")); }'
/// ```
///
/// to print the value of the `x` field.
#[derive(Debug, Clone, Serialize)]
pub struct Arg {
    x: u8,
    buffer: Vec<i32>,
}

/// Note that not all types are JSON serializable. The most common case is internally-tagged
/// enums with a newtype variant, such as this type. Note that this will not break your program,
/// but an error message will be transmitted to DTrace rather than a successfully-converted value.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum Whoops {
    NoBueno(u8),
}

/// Providers may be defined directly in Rust code using the `usdt::provider` attribute.
///
/// The attribute should be attached to a module, whose name becomes the provider name. The module
/// can contain `use` statements to import any required items. Probe are defined as `fn`s, whose
/// bodies must be empty. The types of the function are the types of the DTrace probe, assuming
/// they are supported.
///
/// Note that in most cases, writing the provider in Rust or a D script is equivalent. The main
/// difference is in the support of serializable types. This can't be conveniently expressed in D,
/// as data there is simply a string. So if you want to provide a probe with a more complex Rust
/// type as an argument, it must be defined using this macro.
#[usdt::provider]
mod test {
    /// The `Arg` type needs to be imported here, just like in any other module. Note that you
    /// _must_ use an absolute import, such as `crate::Arg` or `::std::net::IpAddr`. Relative
    /// imports will generate a compiler error. The generated probe macros may be called from
    /// anywhere, meaning that those relative imports generally can't be resolved in the same way
    /// at the macro invocation site.
    use crate::Arg;

    /// Parameters may be given names, but these are only for documentation purposes.
    fn start_work(x: u8) {}

    /// Parameters need not have names, and may be taken by reference...
    fn stop_work(_: String, arg: &Arg) {}

    /// ... or by value
    fn stop_work_by_value(_: String, _: Arg) {}

    /// Probes usually contain standard path types, such as `u8` or `std::net::IpAddr`. However,
    /// they may also contain slices, arrays, tuples, and references. In these cases, as in the
    /// case of any non-native D type, the value will be JSON serialized when sending to DTrace.
    fn arg_as_tuple(_: (u8, &[i32])) {}

    /// Some types aren't JSON serializable. These will not break the program, but an error message
    /// will be seen in DTrace.
    fn not_json_serializable(_: crate::Whoops) {}

    /// Constant pointers to integer types are also supported
    fn work_with_pointer(_buffer: *const u8, _: u64) {}
}

fn main() {
    usdt::register_probes().unwrap();
    let mut arg = Arg {
        x: 0,
        buffer: vec![1; 12],
    };
    let buffer = [2; 4];
    loop {
        test::start_work!(|| arg.x);
        std::thread::sleep(std::time::Duration::from_secs(1));
        arg.x = arg.x.wrapping_add(1);
        test::stop_work!(|| { (format!("the probe has fired {}", arg.x), &arg) });
        test::stop_work_by_value!(|| {
            let new_arg = Arg {
                x: arg.x,
                buffer: vec![arg.x.into()],
            };
            (format!("the probe has fired {}", arg.x), new_arg)
        });
        test::arg_as_tuple!(|| (arg.x, &arg.buffer[..]));
        test::not_json_serializable!(|| Whoops::NoBueno(0));
        test::work_with_pointer!(|| (buffer.as_ptr(), buffer.len() as u64));
    }
}
