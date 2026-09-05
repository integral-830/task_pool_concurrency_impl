pub fn u16_to_le_bytes(value: u16) -> [u8; 2] {
    [(value & 0xff) as u8, ((value >> 8) & 0xff) as u8]
}

pub fn u16_to_be_bytes(value: u16) -> [u8; 2] {
    [((value >> 8) & 0xff) as u8, (value & 0xff) as u8]
}

pub fn u32_to_le_bytes(value: u32) -> [u8; 4] {
    [
        (value & 0xff) as u8,
        ((value >> 8) & 0xff) as u8,
        ((value >> 16) & 0xff) as u8,
        ((value >> 24) & 0xff) as u8,
    ]
}

pub fn u32_to_be_bytes(value: u32) -> [u8; 4] {
    [
        ((value >> 24) & 0xff) as u8,
        ((value >> 16) & 0xff) as u8,
        ((value >> 8) & 0xff) as u8,
        (value & 0xff) as u8,
    ]
}

pub fn u16_from_le_bytes(bytes: &[u8]) -> Option<u16> {
    if bytes.len() < 2 {
        return None;
    }
    Some((bytes[0] as u16) | (bytes[1] as u16) << 8)
}

pub fn u16_from_be_bytes(bytes: &[u8]) -> Option<u16> {
    if bytes.len() < 2 {
        return None;
    }
    Some((bytes[0] as u16) << 8 | (bytes[1] as u16))
}

pub fn u32_from_le_bytes(bytes: &[u8]) -> Option<u32> {
    if bytes.len() < 4 {
        return None;
    }

    Some(
        (bytes[0] as u32)
            | ((bytes[1] as u32) << 8)
            | ((bytes[2] as u32) << 16)
            | ((bytes[3] as u32) << 24),
    )
}

pub fn u32_from_be_bytes(bytes: &[u8]) -> Option<u32> {
    if bytes.len() < 4 {
        return None;
    }

    Some(
        ((bytes[0] as u32) << 24)
            | ((bytes[1] as u32) << 16)
            | ((bytes[2] as u32) << 8)
            | (bytes[3] as u32),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Record {
    pub id: u32,
    pub flags: u16,
    pub kind: u8,
}

pub fn serialize_record(record: &Record) -> Vec<u8> {
    let mut output = Vec::with_capacity(7);

    output.extend_from_slice(&u32_to_le_bytes(record.id));
    output.extend_from_slice(&u16_to_le_bytes(record.flags));
    output.push(record.kind);

    output
}

pub fn deserialize_record(bytes: &[u8]) -> Option<Record> {
    if bytes.len() < 7 {
        return None;
    }

    let id = u32_from_le_bytes(&bytes[0..4])?;
    let flags = u16_from_le_bytes(&bytes[4..6])?;
    let kind = bytes[6];

    Some(Record { id, flags, kind })
}
