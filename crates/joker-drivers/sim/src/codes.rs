//! The registry codes the simulator reports (ref/03 §2.4, document 16 §9).

use joker_hal::code;
use joker_hal::types::Code;

/// Unknown signal (in an `[implements]` target).
pub const J0101: Code = code!("J0101");
/// Incompatible unit (dimension).
pub const J0107: Code = code!("J0107");
/// Incompatible quantity kind.
pub const J0108: Code = code!("J0108");
/// Unit outside the catalogue.
pub const J0109: Code = code!("J0109");
/// Unknown key.
pub const J0140: Code = code!("J0140");
/// Reserved key (`_…`).
pub const J0141: Code = code!("J0141");
/// Selector without matches.
pub const J0155: Code = code!("J0155");
/// Expression with incompatible dimensions.
pub const J0301: Code = code!("J0301");
/// Unknown signal in an expression.
pub const J0302: Code = code!("J0302");
/// Enum without that variant.
pub const J0303: Code = code!("J0303");
/// Window, `delay` or `lag` above 15 min.
pub const J0305: Code = code!("J0305");
/// `own()` outside a class method.
pub const J0306: Code = code!("J0306");
/// `schema` missing or not first.
pub const J0800: Code = code!("J0800");
/// Unsupported `schema` version.
pub const J0801: Code = code!("J0801");
/// The file requires a newer runtime.
pub const J0802: Code = code!("J0802");
/// Wrong file type.
pub const J0803: Code = code!("J0803");
/// Invalid name.
pub const J0805: Code = code!("J0805");
/// Invalid literal.
pub const J0806: Code = code!("J0806");
/// Key reserved for a later version.
pub const J0807: Code = code!("J0807");
/// Invalid raw type in a profile.
pub const J0809: Code = code!("J0809");
/// Path collision, or more signals than a model holds.
pub const J0811: Code = code!("J0811");
/// Enum or bit nonexistent, duplicate or out of range.
pub const J0814: Code = code!("J0814");
/// Signal type or unit inconsistent.
pub const J0815: Code = code!("J0815");
/// Invalid `limits`.
pub const J0816: Code = code!("J0816");
/// Invalid action step.
pub const J0821: Code = code!("J0821");
/// Action arguments missing, extra or of the wrong type.
pub const J0822: Code = code!("J0822");
/// Unknown `implements` class reference.
pub const J0823: Code = code!("J0823");
/// Invalid `implements` mapping.
pub const J0824: Code = code!("J0824");
/// Unknown generator or invalid parameters.
pub const J0831: Code = code!("J0831");
/// More than one source for a signal.
pub const J0832: Code = code!("J0832");
/// Invalid dynamics.
pub const J0833: Code = code!("J0833");
/// Invalid fault.
pub const J0834: Code = code!("J0834");
/// Invalid panel.
pub const J0835: Code = code!("J0835");
/// Unknown, missing or ill-typed parameter.
pub const J0836: Code = code!("J0836");
/// Wasm plant incompatible with the model.
pub const J0837: Code = code!("J0837");

/// The code of a `joker-units` error.
pub fn of_units(e: &joker_units::Error) -> Code {
    Code::new(e.code()).unwrap_or(J0806)
}
