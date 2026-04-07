use crate::types::Fr;
use crate::Error;

/// Split a byte buffer into field elements for on-chain storage.
///
/// The first field stores the buffer length. Subsequent fields each hold
/// up to 31 bytes, left-aligned at byte 1 in a 32-byte field (byte 0 is zero).
/// The output is zero-padded to `target_length`.
pub fn buffer_as_fields(input: &[u8], target_length: usize) -> Result<Vec<Fr>, Error> {
    let mut encoded = vec![Fr::from(input.len() as u64)];
    for chunk in input.chunks(31) {
        let mut padded = [0u8; 32];
        padded[1..1 + chunk.len()].copy_from_slice(chunk);
        encoded.push(Fr::from(padded));
    }
    if encoded.len() > target_length {
        return Err(Error::Abi(format!(
            "buffer exceeds maximum field count: got {} but max is {}",
            encoded.len(),
            target_length
        )));
    }
    encoded.resize(target_length, Fr::zero());
    Ok(encoded)
}

/// Reconstruct a byte buffer from field elements produced by `buffer_as_fields`.
///
/// The first field is the original buffer length. Subsequent fields each
/// contribute 31 bytes (from bytes [1..32] of the big-endian representation).
pub fn buffer_from_fields(fields: &[Fr]) -> Result<Vec<u8>, Error> {
    if fields.is_empty() {
        return Err(Error::Abi("empty field array".to_owned()));
    }
    let length = fields[0].to_usize();
    let mut result = Vec::with_capacity(length);
    for field in &fields[1..] {
        let bytes = field.to_be_bytes();
        result.extend_from_slice(&bytes[1..]);
    }
    result.truncate(length);
    Ok(result)
}
