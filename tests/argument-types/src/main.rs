//! An example and compile-test showing the various ways in which probe arguments may be specified,
//! both in the parameter list and when passing values in the probe argument closure.

// Copyright 2021 Oxide Computer Company

#![feature(asm)]
#![cfg_attr(target_os = "macos", feature(asm_sym))]
use serde::Serialize;

/// Most struct or tuple types implementing serde::Serialize may be used in probes.
#[derive(Default, Clone, Serialize)]
struct Arg {
    x: Vec<i32>,
}

/// Types with references are not supported.
#[derive(Serialize)]
struct NotSupported<'a> {
    x: &'a [i32],
}

#[usdt::provider]
mod refs {
    /// Simple types such as integers may be taken by value ...
    fn u8_as_value(_: u8) {}

    /// ... or by reference
    fn u8_as_reference(_: &u8) {}

    /// Same with strings
    fn string_as_value(_: String) {}
    fn string_as_reference(_: &String) {}

    /// Slices are supported
    fn slice(_: &[u8]) {}

    /// As are arrays.
    fn array(_: [u8; 4]) {}

    /// And tuples.
    fn tuple(_: (u8, &[u8])) {}

    /// Tuples cannot be passed by reference, so this won't work. This would require naming the
    /// lifetime of the inner shared slice, which isn't currently supported.
    // fn tuple_by_reference(_: &(u8, &[u8])) {}

    /// Serializable types may also be taken by value or reference.
    fn serializable_as_value(_: crate::Arg) {}
    fn serializable_as_reference(_: &crate::Arg) {}
}

fn main() {
    usdt::register_probes().unwrap();

    // Probe macros internally take a _reference_ to the data whenever possible. This means probes
    // that accept a type by value...
    refs::u8_as_value!(|| 0);

    // ... may also take that type by reference.
    refs::u8_as_value!(|| &0);

    // And vice-versa: a probe accepting a parameter by reference may take it by value as well.
    refs::u8_as_reference!(|| 0);
    refs::u8_as_reference!(|| &0);

    // This is true for string types as well. Probes accepting a string type may be called with
    // anything that implements `AsRef<str>`, which includes `&str`, owned `String`s, and
    // `&String` as well.
    refs::string_as_value!(|| "&'static str");
    refs::string_as_value!(|| String::from("owned"));
    refs::string_as_reference!(|| "&'static str");
    refs::string_as_reference!(|| String::from("owned"));

    // Vectors are supported as well. In this case, the probe argument behaves the way it might in
    // a "normal" function -- with a signature like `fn foo(_: Vec<T>)`, one can pass a `Vec<T>`.
    // (In this case a reference would also work, i.e., `&Vec<T>`.) However, with a _slice_ as the
    // argument, `fn foo(_: &[T])`, one may pass anything that implements `AsRef<[T]>`, which
    // includes slices and `Vec<T>`.
    let x = vec![0, 1, 2];

    // Call with an actual slice ...
    refs::slice!(|| &x[..]);

    // .. Or the vector itself, just like any function `fn(&[T])`.
    refs::slice!(|| &x);

    // Arrays may also be passed to something expecting a slice.
    let arr: [u8; 4] = [0, 1, 2, 3];
    refs::slice!(|| &arr[..2]);
    refs::array!(|| arr);
    refs::array!(|| &arr);

    // Tuples may be passed in by value.
    refs::tuple!(|| ((0, &x[..])));

    // Serializable types may be passed by value or reference, to a probe expecting either a value
    // or a reference. Note, however, that the normal lifetime rules apply: you can't return a
    // reference from an argument closure to data constructed _inside_ the closure. I.e., this will
    // _not_ work:
    //
    // ```
    // refs::serializable_as_reference!(|| &crate::Arg::default());
    // ```
    let arg = crate::Arg::default();
    refs::serializable_as_value!(|| crate::Arg::default());
    refs::serializable_as_value!(|| &arg);
    refs::serializable_as_reference!(|| crate::Arg::default());
    refs::serializable_as_reference!(|| &arg);

    // It's also possible to capture and return local variables by value in the probe argument
    // closure. This behaves just like any other captured variable, and so `arg` cannot be used
    // again, unless it implements Copy.
    refs::serializable_as_reference!(|| arg);

    // This line will fail to compile, indicating that `arg` is borrowed after it's been moved.
    // println!("{:#?}", arg.x);
}
