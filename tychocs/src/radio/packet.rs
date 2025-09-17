use num_enum::{TryFromPrimitive, TryFromPrimitiveError};

pub(in super::super::radio) const BUFFER_SIZE: usize = 32;
pub(in super::super::radio) const META_SIZE: usize = 3;

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, TryFromPrimitive, Debug)]
pub(in super::super::radio) enum PacketType {
    Data,
    Ack,
    Advertise,
    EstablishConnection,
}

#[derive(Clone, Copy, Debug)]
pub struct Packet {
    pub addr: u8,
    pub(in super::super::radio) buffer: [u8; BUFFER_SIZE + META_SIZE],
}

impl Packet {
    const LEN_INDEX: usize = 0;
    const ID_INDEX: usize = 1;
    const TYPE_INDEX: usize = 2;

    pub const fn default() -> Self {
        Self {
            addr: 0,
            buffer: [(META_SIZE - 1) as u8; BUFFER_SIZE + META_SIZE],
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn len(&self) -> usize {
        // Subtract META_SIZE by 1 for len as len field in the radio perp doesn't count the len byte
        self.buffer[Self::LEN_INDEX] as usize - (META_SIZE - 1)
    }

    pub fn set_len(&mut self, len: usize) {
        self.buffer[Self::LEN_INDEX] = (META_SIZE - 1) as u8 + len as u8;
    }

    pub fn id(&self) -> u8 {
        self.buffer[Self::ID_INDEX]
    }

    pub fn set_id(&mut self, id: u8) {
        self.buffer[Self::ID_INDEX] = id;
    }

    pub(in super::super::radio) fn packet_type(
        &self,
    ) -> Result<PacketType, TryFromPrimitiveError<PacketType>> {
        self.buffer[Self::TYPE_INDEX].try_into()
    }

    pub(in super::super::radio) fn set_type(&mut self, packet_type: PacketType) {
        self.buffer[Self::TYPE_INDEX] = packet_type as u8;
    }

    pub fn copy_from_slice(&mut self, src: &[u8]) {
        assert!(src.len() <= BUFFER_SIZE);
        self.buffer[META_SIZE..][..src.len()].copy_from_slice(src);
        self.set_len(src.len());
    }
}

impl core::ops::Deref for Packet {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &self.buffer[META_SIZE..][..self.len()]
    }
}

impl core::ops::DerefMut for Packet {
    fn deref_mut(&mut self) -> &mut [u8] {
        let len = self.len();
        &mut self.buffer[META_SIZE..][..len]
    }
}
