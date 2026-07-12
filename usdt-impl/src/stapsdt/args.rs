// Copyright 2024 Aapo Alasuutari
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

//! Helpers for generating GNU Assembler format for use in STAPSDT probes.

use crate::DataType;
use dtrace_parser::{BitWidth, DataType as NativeDataType, Integer, Sign};

/// Convert an Integer type and an argument index into a GNU Assembler
/// operand that reads the integer's value from the argument by name. The
/// exact register choice for argument passing is left up to the compiler,
/// meaning that this function generates a string like "{arg_N}" with possible
/// register type/size suffix after the `arg_N`, separated by a colon.
fn integer_to_asm_op(integer: &Integer, reg_index: u8) -> String {
    // See common.rs for note on argument passing and maximum supported
    // argument count.
    assert!(
        reg_index <= 5,
        "Up to 6 probe arguments are currently supported"
    );
    if cfg!(target_arch = "x86_64") {
        match integer.width {
            BitWidth::Bit8 => format!("{{arg_{reg_index}}}"),
            BitWidth::Bit16 => format!("{{arg_{reg_index}:x}}"),
            BitWidth::Bit32 => format!("{{arg_{reg_index}:e}}"),
            BitWidth::Bit64 => format!("{{arg_{reg_index}:r}}"),
            #[cfg(target_pointer_width = "32")]
            BitWidth::Pointer => format!("{{arg_{reg_index}:e}}"),
            #[cfg(target_pointer_width = "64")]
            BitWidth::Pointer => format!("{{arg_{reg_index}:r}}"),
            #[cfg(not(any(target_pointer_width = "32", target_pointer_width = "64")))]
            BitWidth::Pointer => compile_error!("Unsupported pointer width"),
        }
    } else if cfg!(target_arch = "aarch64") {
        // GNU Assembly syntax for SystemTap only uses the extended register
        // for some reason.
        format!("{{arg_{reg_index}:x}}")
    } else {
        unreachable!("Unsupported Linux target architecture")
    }
}

/// Convert an Integer type into its STAPSDT probe arguments definition
/// signedness and size value as a String.
fn integer_to_arg_size(integer: &Integer) -> &'static str {
    match integer.width {
        BitWidth::Bit8 => match integer.sign {
            Sign::Unsigned => "1",
            _ => "-1",
        },
        BitWidth::Bit16 => match integer.sign {
            Sign::Unsigned => "2",
            _ => "-2",
        },
        BitWidth::Bit32 => match integer.sign {
            Sign::Unsigned => "4",
            _ => "-4",
        },
        BitWidth::Bit64 => match integer.sign {
            Sign::Unsigned => "8",
            _ => "-8",
        },
        #[cfg(target_pointer_width = "32")]
        BitWidth::Pointer => "4",
        #[cfg(target_pointer_width = "64")]
        BitWidth::Pointer => "8",
        #[cfg(not(any(target_pointer_width = "32", target_pointer_width = "64")))]
        BitWidth::Pointer => compile_error!("Unsupported pointer width"),
    }
}

const POINTER: Integer = Integer {
    sign: Sign::Unsigned,
    width: BitWidth::Pointer,
};

const UNIQUE_ID: Integer = Integer {
    sign: Sign::Unsigned,
    width: BitWidth::Bit64,
};

/// Convert a NativeDataType and register index to its GNU Assembler operand
/// as a String.
fn native_data_type_to_asm_op(typ: &NativeDataType, reg_index: u8) -> String {
    match typ {
        NativeDataType::Integer(int) => integer_to_asm_op(int, reg_index).into(),
        // Integer pointers are dereferenced by wrapping the pointer assembly
        // into parentheses.
        NativeDataType::Pointer(_) => format!("({})", integer_to_asm_op(&POINTER, reg_index)),
        NativeDataType::String => integer_to_asm_op(&POINTER, reg_index).into(),
    }
}

/// Convert a type to its GNU Assembler size representation as a string.
fn native_data_type_to_arg_size(typ: &NativeDataType) -> &'static str {
    match typ {
        NativeDataType::Integer(int) => integer_to_arg_size(int),
        NativeDataType::Pointer(_) | NativeDataType::String => integer_to_arg_size(&POINTER),
        // Note: If NativeDataType::Float becomes supported, it will need an
        // "f" suffix in the type, eg. `4f` or `8f`.
    }
}

/// Convert a DataType and register index to a GNU Assembler operand as a
/// String.
fn data_type_to_asm_op(typ: &DataType, reg_index: u8) -> String {
    match typ {
        DataType::Native(ty) => native_data_type_to_asm_op(ty, reg_index),
        DataType::UniqueId => integer_to_asm_op(&UNIQUE_ID, reg_index).into(),
        DataType::Serializable(_) => integer_to_asm_op(&POINTER, reg_index).into(),
    }
}

/// Convert a DataType to its STAPSDT probe argument size representation as a
/// String.
fn data_type_to_arg_size(typ: &DataType) -> &'static str {
    match typ {
        DataType::Native(ty) => native_data_type_to_arg_size(ty),
        DataType::UniqueId => integer_to_arg_size(&UNIQUE_ID),
        DataType::Serializable(_) => integer_to_arg_size(&POINTER),
    }
}

/// ## Format a STAPSDT probe argument into the SystemTap argument format.
/// Source: https://sourceware.org/systemtap/wiki/UserSpaceProbeImplementation
///
/// ### Summary
///
/// Argument format is `Nf@OP`, N is an optional `-` to signal signedness
/// followed by one of `{1,2,4,8}` for bit width, `f` is an optional marker
/// for floating point values, @ is a separator, and OP is the
/// "actual assembly operand". The assembly operand is given in the GNU
/// Assembler format. See
/// https://en.wikibooks.org/wiki/X86_Assembly/GNU_assembly_syntax
/// for details.
///
/// ### Examples
///
/// 1. Read a u64 from RDI: `8@%rdi`.
/// 2. Read an i32 through a pointer in RSI: `-4@(%rsi)`.
/// 3. Read an f64 through a pointer in RDI: `8f@(%rdi)`.
///    (Not sure if `-` should be added.)
/// 4. Read a u64 through a pointer with an offset: `8%-4(%rdi)`.
pub(crate) fn format_argument((reg_index, typ): (usize, &DataType)) -> String {
    format!(
        "{}@{}",
        data_type_to_arg_size(typ),
        data_type_to_asm_op(typ, u8::try_from(reg_index).unwrap())
    )
}

#[cfg(test)]
mod test {
    use dtrace_parser::{BitWidth, Integer};

    use crate::{
        internal::args::{format_argument, integer_to_asm_op},
        DataType,
    };

    fn int(width: BitWidth) -> Integer {
        Integer {
            sign: dtrace_parser::Sign::Signed,
            width,
        }
    }

    fn uint(width: BitWidth) -> Integer {
        Integer {
            sign: dtrace_parser::Sign::Unsigned,
            width,
        }
    }

    #[test]
    fn integer_to_asm_op_tests() {
        if cfg!(target_arch = "x86_64") {
            assert_eq!(integer_to_asm_op(&uint(BitWidth::Bit8), 0), "{arg_0}");
            assert_eq!(integer_to_asm_op(&uint(BitWidth::Bit16), 0), "{arg_0:x}");
            assert_eq!(integer_to_asm_op(&uint(BitWidth::Bit32), 0), "{arg_0:e}");
            assert_eq!(integer_to_asm_op(&uint(BitWidth::Bit64), 0), "{arg_0:r}");
            assert_eq!(integer_to_asm_op(&uint(BitWidth::Pointer), 0), "{arg_0:r}");
            assert_eq!(integer_to_asm_op(&uint(BitWidth::Bit8), 1), "{arg_1}");
            assert_eq!(integer_to_asm_op(&uint(BitWidth::Bit16), 1), "{arg_1:x}");
            assert_eq!(integer_to_asm_op(&uint(BitWidth::Bit32), 1), "{arg_1:e}");
            assert_eq!(integer_to_asm_op(&uint(BitWidth::Bit64), 1), "{arg_1:r}");
            assert_eq!(integer_to_asm_op(&uint(BitWidth::Pointer), 1), "{arg_1:r}");
        } else if cfg!(target_arch = "aarch64") {
            // GNU Assembly syntax for SystemTap only uses the extended register
            // for some reason.
            assert_eq!(integer_to_asm_op(&uint(BitWidth::Bit8), 0), "{arg_0:x}");
            assert_eq!(integer_to_asm_op(&uint(BitWidth::Bit16), 0), "{arg_0:x}");
            assert_eq!(integer_to_asm_op(&uint(BitWidth::Bit32), 0), "{arg_0:x}");
            assert_eq!(integer_to_asm_op(&uint(BitWidth::Bit64), 0), "{arg_0:x}");
            assert_eq!(integer_to_asm_op(&uint(BitWidth::Pointer), 0), "{arg_0:x}");
            assert_eq!(integer_to_asm_op(&uint(BitWidth::Bit8), 1), "{arg_1:x}");
            assert_eq!(integer_to_asm_op(&uint(BitWidth::Bit16), 1), "{arg_1:x}");
            assert_eq!(integer_to_asm_op(&uint(BitWidth::Bit32), 1), "{arg_1:x}");
            assert_eq!(integer_to_asm_op(&uint(BitWidth::Bit64), 1), "{arg_1:x}");
            assert_eq!(integer_to_asm_op(&uint(BitWidth::Pointer), 1), "{arg_1:x}");
        } else {
            unreachable!("Unsupported Linux target architecture")
        }
    }

    #[test]
    fn format_argument_tests() {
        if cfg!(target_arch = "x86_64") {
            assert_eq!(format_argument((0, &DataType::UniqueId)), "8@{arg_0:r}");
            assert_eq!(
                format_argument((4, &DataType::Native(dtrace_parser::DataType::String))),
                "8@{arg_4:r}"
            );
            assert_eq!(
                format_argument((
                    4,
                    &DataType::Native(dtrace_parser::DataType::Integer(uint(BitWidth::Bit32)))
                )),
                "4@{arg_4:e}"
            );
            assert_eq!(
                format_argument((
                    3,
                    &DataType::Native(dtrace_parser::DataType::Integer(int(BitWidth::Bit16)))
                )),
                "-2@{arg_3:x}"
            );
        } else if cfg!(target_arch = "aarch64") {
            assert_eq!(format_argument((0, &DataType::UniqueId)), "8@{arg_0:x}");
            assert_eq!(
                format_argument((4, &DataType::Native(dtrace_parser::DataType::String))),
                "8@{arg_4:x}"
            );
            assert_eq!(
                format_argument((
                    4,
                    &DataType::Native(dtrace_parser::DataType::Integer(uint(BitWidth::Bit32)))
                )),
                "4@{arg_4:x}"
            );
            assert_eq!(
                format_argument((
                    3,
                    &DataType::Native(dtrace_parser::DataType::Integer(int(BitWidth::Bit16)))
                )),
                "-2@{arg_3:x}"
            );
        } else {
            unreachable!("Unsupported Linux target architecture")
        }
    }
}
