use std::io::{self, Read, Write};

pub fn read_message<R: Read>(reader: &mut R) -> io::Result<Vec<u8>> {
    let mut length_bytes = [0_u8; 4];
    reader.read_exact(&mut length_bytes)?;
    let length = u32::from_le_bytes(length_bytes) as usize;

    let mut payload = vec![0_u8; length];
    reader.read_exact(&mut payload)?;

    Ok(payload)
}

pub fn write_message<W: Write>(writer: &mut W, payload: &[u8]) -> io::Result<()> {
    let length = u32::try_from(payload.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "message too large"))?;

    writer.write_all(&length.to_le_bytes())?;
    writer.write_all(payload)?;
    writer.flush()
}
