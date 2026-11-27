//! Wire protocol between the engine and any display client.
//!
//! Binary, little-endian, no dependencies. The engine only ever says what
//! *exists* in the world; how it looks is entirely up to the client.
//!
//! Server -> client
//!   Hello    : [0x01][u16 version][u32 player_id]
//!   Snapshot : [0x02][u32 tick][u16 count] then count x ([u32 id][u8 kind][f32 x][f32 y])
//! Client -> server
//!   Input    : [0x01][u8 buttons]
//!
//! World space: units are tiles, +x is right, +y is UP (math convention).

pub const PROTOCOL_VERSION: u16 = 1;

pub type EntityId = u32;

/// What an entity *is*. Clients map this to a sprite/mesh/colour.
pub mod kind {
    pub const CUBE: u8 = 1;
}

/// Bit flags for the `buttons` byte in `ClientMsg::Input`.
pub mod button {
    pub const UP: u8 = 1 << 0;
    pub const DOWN: u8 = 1 << 1;
    pub const LEFT: u8 = 1 << 2;
    pub const RIGHT: u8 = 1 << 3;
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EntityState {
    pub id: EntityId,
    pub kind: u8,
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ServerMsg {
    Hello { version: u16, player_id: EntityId },
    Snapshot { tick: u32, entities: Vec<EntityState> },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ClientMsg {
    Input { buttons: u8 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    Empty,
    UnknownTag(u8),
    Truncated,
}

const TAG_HELLO: u8 = 0x01;
const TAG_SNAPSHOT: u8 = 0x02;
const TAG_INPUT: u8 = 0x01;

struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take<const N: usize>(&mut self) -> Result<[u8; N], DecodeError> {
        let end = self.pos + N;
        let bytes = self.buf.get(self.pos..end).ok_or(DecodeError::Truncated)?;
        self.pos = end;
        Ok(bytes.try_into().unwrap())
    }
    fn u8(&mut self) -> Result<u8, DecodeError> {
        Ok(self.take::<1>()?[0])
    }
    fn u16(&mut self) -> Result<u16, DecodeError> {
        Ok(u16::from_le_bytes(self.take()?))
    }
    fn u32(&mut self) -> Result<u32, DecodeError> {
        Ok(u32::from_le_bytes(self.take()?))
    }
    fn f32(&mut self) -> Result<f32, DecodeError> {
        Ok(f32::from_le_bytes(self.take()?))
    }
}

impl ServerMsg {
    pub fn encode(&self) -> Vec<u8> {
        match self {
            ServerMsg::Hello { version, player_id } => {
                let mut out = Vec::with_capacity(7);
                out.push(TAG_HELLO);
                out.extend_from_slice(&version.to_le_bytes());
                out.extend_from_slice(&player_id.to_le_bytes());
                out
            }
            ServerMsg::Snapshot { tick, entities } => {
                let count = entities.len().min(u16::MAX as usize);
                let mut out = Vec::with_capacity(7 + count * 13);
                out.push(TAG_SNAPSHOT);
                out.extend_from_slice(&tick.to_le_bytes());
                out.extend_from_slice(&(count as u16).to_le_bytes());
                for e in &entities[..count] {
                    out.extend_from_slice(&e.id.to_le_bytes());
                    out.push(e.kind);
                    out.extend_from_slice(&e.x.to_le_bytes());
                    out.extend_from_slice(&e.y.to_le_bytes());
                }
                out
            }
        }
    }

    pub fn decode(buf: &[u8]) -> Result<Self, DecodeError> {
        let mut r = Reader { buf, pos: 0 };
        match r.u8().map_err(|_| DecodeError::Empty)? {
            TAG_HELLO => Ok(ServerMsg::Hello { version: r.u16()?, player_id: r.u32()? }),
            TAG_SNAPSHOT => {
                let tick = r.u32()?;
                let count = r.u16()? as usize;
                let mut entities = Vec::with_capacity(count);
                for _ in 0..count {
                    entities.push(EntityState {
                        id: r.u32()?,
                        kind: r.u8()?,
                        x: r.f32()?,
                        y: r.f32()?,
                    });
                }
                Ok(ServerMsg::Snapshot { tick, entities })
            }
            tag => Err(DecodeError::UnknownTag(tag)),
        }
    }
}

impl ClientMsg {
    pub fn encode(&self) -> Vec<u8> {
        match self {
            ClientMsg::Input { buttons } => vec![TAG_INPUT, *buttons],
        }
    }

    pub fn decode(buf: &[u8]) -> Result<Self, DecodeError> {
        let mut r = Reader { buf, pos: 0 };
        match r.u8().map_err(|_| DecodeError::Empty)? {
            TAG_INPUT => Ok(ClientMsg::Input { buttons: r.u8()? }),
            tag => Err(DecodeError::UnknownTag(tag)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hello_roundtrip() {
        let m = ServerMsg::Hello { version: PROTOCOL_VERSION, player_id: 7 };
        assert_eq!(ServerMsg::decode(&m.encode()), Ok(m));
    }

    #[test]
    fn snapshot_roundtrip() {
        let m = ServerMsg::Snapshot {
            tick: 42,
            entities: vec![
                EntityState { id: 1, kind: kind::CUBE, x: 1.5, y: -2.25 },
                EntityState { id: 2, kind: kind::CUBE, x: 0.0, y: 3.0 },
            ],
        };
        assert_eq!(ServerMsg::decode(&m.encode()), Ok(m));
    }

    #[test]
    fn input_roundtrip() {
        let m = ClientMsg::Input { buttons: button::UP | button::LEFT };
        assert_eq!(ClientMsg::decode(&m.encode()), Ok(m));
    }

    #[test]
    fn bad_input_is_rejected_not_panicked() {
        assert_eq!(ClientMsg::decode(&[]), Err(DecodeError::Empty));
        assert_eq!(ClientMsg::decode(&[0x01]), Err(DecodeError::Truncated));
        assert_eq!(ClientMsg::decode(&[0xFF, 0]), Err(DecodeError::UnknownTag(0xFF)));
        assert_eq!(ServerMsg::decode(&[0x02, 1, 0, 0, 0, 5, 0]), Err(DecodeError::Truncated));
    }
}
