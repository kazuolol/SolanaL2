//! Stub for solana-secp256r1-program
//!
//! This stub provides the same API as the real crate but without OpenSSL dependency.
//! Used only for Windows development - secp256r1 verification is not needed for our L2.

use solana_pubkey::Pubkey;
use solana_instruction::Instruction;
use solana_feature_set::FeatureSet;
use solana_precompile_error::PrecompileError;
use bytemuck::{Pod, Zeroable};
use thiserror::Error;

/// The secp256r1 program ID (same as the real one)
pub static ID: Pubkey = Pubkey::from_str_const("Secp256r1SigVerify1111111111111111111111111");

/// Returns the program ID
pub fn id() -> Pubkey {
    ID
}

/// Returns true if the given pubkey is the program ID
pub fn check_id(id: &Pubkey) -> bool {
    id == &ID
}

/// Compressed public key size
pub const COMPRESSED_PUBKEY_SERIALIZED_SIZE: usize = 33;

/// Signature size
pub const SIGNATURE_SERIALIZED_SIZE: usize = 64;

/// Field size for secp256r1
pub const FIELD_SIZE: usize = 32;

/// Start of data section
pub const DATA_START: usize = 2;

/// Signature offsets start
pub const SIGNATURE_OFFSETS_START: usize = 2;

/// Signature offsets serialized size
pub const SIGNATURE_OFFSETS_SERIALIZED_SIZE: usize = 14;

/// secp256r1 order
pub const SECP256R1_ORDER: [u8; 32] = [
    0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00,
    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
    0xbc, 0xe6, 0xfa, 0xad, 0xa7, 0x17, 0x9e, 0x84,
    0xf3, 0xb9, 0xca, 0xc2, 0xfc, 0x63, 0x25, 0x51,
];

/// secp256r1 order minus one
pub const SECP256R1_ORDER_MINUS_ONE: [u8; 32] = [
    0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00,
    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
    0xbc, 0xe6, 0xfa, 0xad, 0xa7, 0x17, 0x9e, 0x84,
    0xf3, 0xb9, 0xca, 0xc2, 0xfc, 0x63, 0x25, 0x50,
];

/// secp256r1 half order
pub const SECP256R1_HALF_ORDER: [u8; 32] = [
    0x7f, 0xff, 0xff, 0xff, 0x80, 0x00, 0x00, 0x00,
    0x7f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
    0xde, 0x73, 0x7d, 0x56, 0xd3, 0x8b, 0xcf, 0x42,
    0x79, 0xdc, 0xe5, 0x61, 0x7e, 0x31, 0x92, 0xa8,
];

/// Signature offsets structure
/// Note: Order matters - matching the real crate's layout
#[derive(Debug, Clone, Copy, Default)]
#[repr(C, packed)]
pub struct Secp256r1SignatureOffsets {
    /// Offset to the signature
    pub signature_offset: u16,
    /// Instruction index for signature
    pub signature_instruction_index: u8,
    /// Offset to the public key
    pub public_key_offset: u16,
    /// Instruction index for public key
    pub public_key_instruction_index: u8,
    /// Offset to the message
    pub message_data_offset: u16,
    /// Length of the message
    pub message_data_size: u16,
    /// Instruction index for message
    pub message_instruction_index: u8,
}

// Safety: The struct is repr(C, packed) with no padding
unsafe impl Pod for Secp256r1SignatureOffsets {}
unsafe impl Zeroable for Secp256r1SignatureOffsets {}

/// Errors for secp256r1 operations
#[derive(Error, Debug)]
pub enum Secp256r1Error {
    #[error("Secp256r1 operations not supported in stub")]
    NotSupported,
}

/// Create a new secp256r1 instruction (deprecated stub)
#[deprecated(note = "Use new_secp256r1_instruction_with_signature instead")]
pub fn new_secp256r1_instruction(
    _priv_key: &[u8],
    _message: &[u8],
) -> Result<Instruction, Secp256r1Error> {
    Err(Secp256r1Error::NotSupported)
}

/// Create a new secp256r1 instruction with signature (stub)
pub fn new_secp256r1_instruction_with_signature(
    _public_key: &[u8],
    _signature: &[u8],
    _message: &[u8],
) -> Result<Instruction, Secp256r1Error> {
    Err(Secp256r1Error::NotSupported)
}

/// Sign a message (stub)
pub fn sign_message(
    _priv_key: &[u8],
    _message: &[u8],
) -> Result<[u8; 64], Secp256r1Error> {
    Err(Secp256r1Error::NotSupported)
}

/// Verify a secp256r1 signature (precompile interface)
///
/// This is the function called by solana-precompiles.
/// Stub always returns error since we don't support secp256r1 on Windows.
pub fn verify(
    _data: &[u8],
    _instruction_datas: &[&[u8]],
    _feature_set: &FeatureSet,
) -> Result<(), PrecompileError> {
    // Return an error - secp256r1 verification not supported in stub
    Err(PrecompileError::InvalidInstructionDataSize)
}
