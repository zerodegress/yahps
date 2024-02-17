use std::io::{Read, Write};

use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use bytes::{buf::Reader, Buf, Bytes};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

#[derive(Clone)]
pub struct Packet {
    ty: PacketType,
    bytes: Bytes,
}

impl Packet {
    pub const HEADER_SIZE: i32 = 8;
    pub const MAX_SIZE: i32 = 50000000;

    pub fn new<B>(ty: PacketType, bytes: B) -> Self
    where
        B: Into<Bytes>,
    {
        Self {
            ty,
            bytes: bytes.into(),
        }
    }

    pub fn ty(&self) -> &PacketType {
        &self.ty
    }

    pub fn bytes(&self) -> &Bytes {
        &self.bytes
    }
}

impl Packet {
    pub async fn write_into_game<W>(&self, out: &mut W) -> Result<(), std::io::Error>
    where
        W: AsyncWrite + Unpin,
    {
        out.write_i32(self.ty.into()).await?;
        out.write_i32(self.bytes.len() as i32).await?;
        out.write_all(&self.bytes).await?;
        Ok(())
    }

    pub async fn read_from_game<R>(input: &mut R) -> Result<Self, std::io::Error>
    where
        R: AsyncRead + Unpin,
    {
        let ty = input.read_i32().await?.into();
        let bytes = {
            let mut buf = vec![0; input.read_i32().await? as usize];
            input.read_exact(&mut buf).await?;
            Bytes::from(buf)
        };
        Ok(Self { bytes, ty })
    }

    pub async fn write_into_net<W>(&self, out: &mut W) -> Result<(), std::io::Error>
    where
        W: AsyncWrite + Unpin,
    {
        out.write_i32(self.bytes.len() as i32).await?;
        out.write_i32(self.ty.into()).await?;
        out.write_all(&self.bytes).await?;
        Ok(())
    }

    pub async fn read_from_net<R>(input: &mut R) -> Result<Self, std::io::Error>
    where
        R: AsyncRead + Unpin,
    {
        let len = input.read_i32().await?;

        if len < 0 {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "packet length below zero",
            ))?
        } else if len > Packet::MAX_SIZE {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "packet length too long",
            ))?
        }

        let ty: PacketType = input.read_i32().await?.into();

        let bytes = {
            let mut buf = vec![0; len as usize];
            input.read_exact(&mut buf).await?;
            Bytes::from(buf)
        };

        Ok(Self { ty, bytes })
    }
}

pub struct PacketBuilder {
    ty: PacketType,
    bytes: Vec<u8>,
}

impl Default for PacketBuilder {
    fn default() -> Self {
        Self {
            ty: PacketType::NOT_RESOLVED,
            bytes: vec![],
        }
    }
}

impl PacketBuilder {
    pub fn build(self) -> Packet {
        Packet::new(self.ty, self.bytes.clone())
    }

    pub fn with_type(mut self, ty: PacketType) -> Self {
        self.ty = ty;
        self
    }

    pub fn write_str(mut self, s: &str) -> Self {
        let str_bytes = s.as_bytes();
        WriteBytesExt::write_u16::<BigEndian>(&mut self.bytes, str_bytes.len() as u16)
            .expect("write str failed");
        Write::write_all(&mut self.bytes, str_bytes).expect("write str failed");
        self
    }

    pub fn write_i32(mut self, n: i32) -> Self {
        WriteBytesExt::write_i32::<BigEndian>(&mut self.bytes, n).expect("write i32 failed");
        self
    }
}

pub struct PacketReader {
    ty: PacketType,
    reader: Reader<Bytes>,
}

impl From<Packet> for PacketReader {
    fn from(value: Packet) -> Self {
        Self {
            ty: value.ty,
            reader: value.bytes.reader(),
        }
    }
}

impl PacketReader {
    pub fn ty(&self) -> &PacketType {
        &self.ty
    }

    pub fn read_bool(&mut self) -> Result<bool, std::io::Error> {
        ReadBytesExt::read_u8(&mut self.reader).map(|x| !matches!(x, 0))
    }

    pub fn skip(&mut self, len: u64) -> Result<(), std::io::Error> {
        self.reader.read_exact(&mut vec![0; len as usize])
    }

    pub fn read_is_str(&mut self) -> Result<String, std::io::Error> {
        if self.read_bool()? {
            self.read_str()
        } else {
            Ok("".to_string())
        }
    }

    pub fn read_str(&mut self) -> Result<String, std::io::Error> {
        let len = ReadBytesExt::read_u16::<BigEndian>(&mut self.reader)?;
        let mut buf = vec![0; len as usize];
        Read::read_exact(&mut self.reader, &mut buf)?;
        String::from_utf8(buf).map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err))
    }

    pub fn read_i32(&mut self) -> Result<i32, std::io::Error> {
        ReadBytesExt::read_i32::<BigEndian>(&mut self.reader)
    }
}

pub type PacketWithControl = (Packet, PacketControl);

pub enum PacketControl {
    Continue,
    Stop,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, num_enum::FromPrimitive, num_enum::IntoPrimitive,
)]
#[repr(i32)]
#[allow(non_camel_case_types, clippy::upper_case_acronyms)]
pub enum PacketType {
    /* CUSTOM */
    SERVER_DEBUG_RECEIVE = 2000,
    SERVER_DEBUG = 2001,
    /* EX */
    GET_SERVER_INFO_RECEIVE = 3000,
    GET_SERVER_INFO = 3001,
    UPDATA_CLASS_RECEIVE = 3010,
    STATUS_RESULT = 3999,
    /* GAME CORE */
    PREREGISTER_INFO_RECEIVE = 160,
    PREREGISTER_INFO = 161,
    PASSWD_ERROR = 113,
    REGISTER_PLAYER = 110,
    /* SERVER INFO */
    SERVER_INFO = 106,
    TEAM_LIST = 115,
    /* HEART */
    HEART_BEAT = 108,
    HEART_BEAT_RESPONSE = 109,
    /* CHAT */
    CHAT_RECEIVE = 140,
    CHAT = 141,
    /* NET STATUS */
    PACKET_DOWNLOAD_PENDING = 4,
    KICK = 150,
    DISCONNECT = 111,
    /** START GAME */
    START_GAME = 120,
    ACCEPT_START_GAME = 112,
    RETURN_TO_BATTLE_ROOM = 122,
    /** GAME START COMMANDS */
    TICK = 10,
    GAMECOMMAND_RECEIVE = 20,
    SYNCCHECKSUM_STATUS = 31,
    SYNC_CHECK = 30,
    SYNC = 35,
    /* RELAY */
    RELAY_117 = 117,
    RELAY_118_117_RETURN = 118,
    RELAY_POW = 151,
    RELAY_POW_RECEIVE = 152,
    RELAY_VERSION_INFO = 163,
    RELAY_BECOME_SERVER = 170,
    FORWARD_CLIENT_ADD = 172,
    FORWARD_CLIENT_REMOVE = 173,
    PACKET_FORWARD_CLIENT_FROM = 174,
    PACKET_FORWARD_CLIENT_TO = 175,
    PACKET_FORWARD_CLIENT_TO_REPEATED = 176,
    PACKET_RECONNECT_TO = 178,
    /* MISC */
    EMPTYP_ACKAGE = 0,
    #[num_enum(default)]
    NOT_RESOLVED = -1,
}
