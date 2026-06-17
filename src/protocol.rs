use std::error::Error as StdError;
use std::fmt;

pub const GRID_BAUD_RATE: u32 = 2_000_000;
pub const GRID_BROADCAST_COORDINATE: i16 = -127;
pub const GRID_MAX_LUA_BYTES: usize = 909;
pub const GRID_LUA_CALLBACK_PREFIX: &str = "--[[@cb]]";

const GRID_CONST_SOH: u8 = 0x01;
const GRID_CONST_STX: u8 = 0x02;
const GRID_CONST_ETX: u8 = 0x03;
const GRID_CONST_EOT: u8 = 0x04;
const GRID_CONST_LF: u8 = 0x0a;
const GRID_CONST_BRC: u8 = 0x0f;
const GRID_CONST_EOB: u8 = 0x17;

const GRID_CLASS_CONFIG: u16 = 0x060;
const GRID_CLASS_PAGESTORE: u16 = 0x061;
const GRID_CLASS_IMMEDIATE: u16 = 0x085;
const GRID_CLASS_HEARTBEAT: u16 = 0x010;
const GRID_CLASS_PAGEACTIVE: u16 = 0x030;
const GRID_HEARTBEAT_EDITOR_TYPE: u8 = 0xff;
const GRID_HEARTBEAT_EDITOR_HWCFG: u8 = 0xff;
const GRID_INSTR_EXECUTE: u8 = 0x0e;
const GRID_INSTR_FETCH: u8 = 0x0f;

const GRID_PROTOCOL_VERSION_MAJOR: u8 = 0x01;
const GRID_PROTOCOL_VERSION_MINOR: u8 = 0x05;
const GRID_PROTOCOL_VERSION_PATCH: u8 = 0x01;
const GRID_EDITOR_VERSION_MAJOR: u8 = 0x01;
const GRID_EDITOR_VERSION_MINOR: u8 = 0x06;
const GRID_EDITOR_VERSION_PATCH: u8 = 0x05;

pub type Result<T> = std::result::Result<T, ProtocolError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolError {
    NonAsciiLua,
    LuaTooLong { length: usize, max_length: usize },
    CoordinateOutOfRange { axis: &'static str, value: i16 },
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct GridTarget {
    pub dx: i16,
    pub dy: i16,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct PacketIdentity {
    pub session_id: u8,
    pub message_id: u8,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct ConfigLocation {
    pub page: u8,
    pub element: u8,
    pub event: u8,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct ImmediateWrite<'a> {
    pub target: GridTarget,
    pub lua: &'a str,
    pub identity: PacketIdentity,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct ConfigWrite<'a> {
    pub target: GridTarget,
    pub location: ConfigLocation,
    pub lua: &'a str,
    pub identity: PacketIdentity,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct ConfigFetch {
    pub target: GridTarget,
    pub location: ConfigLocation,
    pub identity: PacketIdentity,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Heartbeat {
    pub identity: PacketIdentity,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct PageStore {
    pub target: GridTarget,
    pub identity: PacketIdentity,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct PageActive {
    pub target: GridTarget,
    pub page: u8,
    pub identity: PacketIdentity,
}

impl GridTarget {
    pub const BROADCAST: Self = Self {
        dx: GRID_BROADCAST_COORDINATE,
        dy: GRID_BROADCAST_COORDINATE,
    };

    pub const fn new(dx: i16, dy: i16) -> Self {
        Self { dx, dy }
    }
}

impl PacketIdentity {
    pub const fn new(session_id: u8, message_id: u8) -> Self {
        Self {
            session_id,
            message_id,
        }
    }
}

impl ConfigLocation {
    pub const fn new(page: u8, element: u8, event: u8) -> Self {
        Self {
            page,
            element,
            event,
        }
    }
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonAsciiLua => {
                f.write_str("the current Grid packet encoder supports ASCII Lua only")
            }
            Self::LuaTooLong { length, max_length } => {
                write!(
                    f,
                    "script is too long: {length} bytes (maximum is {max_length})"
                )
            }
            Self::CoordinateOutOfRange { axis, value } => {
                write!(f, "grid coordinate {axis}={value} is out of range")
            }
        }
    }
}

impl StdError for ProtocolError {}

pub fn frame_lua(lua: &str) -> String {
    let trimmed = lua.trim();

    let normalize_body = |body: &str| {
        let body = body.trim();

        if body.starts_with(GRID_LUA_CALLBACK_PREFIX) {
            body.to_string()
        } else if body.is_empty() {
            GRID_LUA_CALLBACK_PREFIX.to_string()
        } else {
            format!("{GRID_LUA_CALLBACK_PREFIX} {body}")
        }
    };

    if let Some(inner) = trimmed
        .strip_prefix("<?lua")
        .and_then(|inner| inner.strip_suffix("?>"))
    {
        return format!("<?lua {} ?>", normalize_body(inner));
    }

    format!("<?lua {} ?>", normalize_body(trimmed))
}

pub fn encode_immediate_packet(write: &ImmediateWrite<'_>) -> Result<Vec<u8>> {
    let framed_lua = frame_lua(write.lua);
    let script_bytes = encode_script_bytes(&framed_lua)?;

    let mut class = vec![GRID_CONST_STX];
    write_ascii_hex(&mut class, 3, GRID_CLASS_IMMEDIATE as usize);
    write_ascii_hex(&mut class, 1, GRID_INSTR_EXECUTE as usize);
    write_ascii_hex(&mut class, 4, script_bytes.len());
    class.extend_from_slice(&script_bytes);
    class.push(GRID_CONST_ETX);
    class.push(GRID_CONST_EOT);

    encode_packet(write.target, write.identity, &class)
}

pub fn encode_config_packet(write: &ConfigWrite<'_>) -> Result<Vec<u8>> {
    let framed_lua = frame_lua(write.lua);
    let script_bytes = encode_script_bytes(&framed_lua)?;

    let mut class = vec![GRID_CONST_STX];
    write_ascii_hex(&mut class, 3, GRID_CLASS_CONFIG as usize);
    write_ascii_hex(&mut class, 1, GRID_INSTR_EXECUTE as usize);
    write_ascii_hex(&mut class, 2, GRID_PROTOCOL_VERSION_MAJOR as usize);
    write_ascii_hex(&mut class, 2, GRID_PROTOCOL_VERSION_MINOR as usize);
    write_ascii_hex(&mut class, 2, GRID_PROTOCOL_VERSION_PATCH as usize);
    write_ascii_hex(&mut class, 2, write.location.page as usize);
    write_ascii_hex(&mut class, 2, write.location.element as usize);
    write_ascii_hex(&mut class, 2, write.location.event as usize);
    write_ascii_hex(&mut class, 4, script_bytes.len());
    class.extend_from_slice(&script_bytes);
    class.push(GRID_CONST_ETX);
    class.push(GRID_CONST_EOT);

    encode_packet(write.target, write.identity, &class)
}

pub fn encode_config_fetch_packet(fetch: &ConfigFetch) -> Result<Vec<u8>> {
    let mut class = vec![GRID_CONST_STX];
    write_ascii_hex(&mut class, 3, GRID_CLASS_CONFIG as usize);
    write_ascii_hex(&mut class, 1, GRID_INSTR_FETCH as usize);
    write_ascii_hex(&mut class, 2, GRID_PROTOCOL_VERSION_MAJOR as usize);
    write_ascii_hex(&mut class, 2, GRID_PROTOCOL_VERSION_MINOR as usize);
    write_ascii_hex(&mut class, 2, GRID_PROTOCOL_VERSION_PATCH as usize);
    write_ascii_hex(&mut class, 2, fetch.location.page as usize);
    write_ascii_hex(&mut class, 2, fetch.location.element as usize);
    write_ascii_hex(&mut class, 2, fetch.location.event as usize);
    write_ascii_hex(&mut class, 4, 0);
    class.push(GRID_CONST_ETX);
    class.push(GRID_CONST_EOT);

    encode_packet(fetch.target, fetch.identity, &class)
}

pub fn encode_heartbeat_packet(heartbeat: &Heartbeat) -> Result<Vec<u8>> {
    let mut class = vec![GRID_CONST_STX];
    write_ascii_hex(&mut class, 3, GRID_CLASS_HEARTBEAT as usize);
    write_ascii_hex(&mut class, 1, GRID_INSTR_EXECUTE as usize);
    write_ascii_hex(&mut class, 2, GRID_HEARTBEAT_EDITOR_TYPE as usize);
    write_ascii_hex(&mut class, 2, GRID_HEARTBEAT_EDITOR_HWCFG as usize);
    write_ascii_hex(&mut class, 2, GRID_EDITOR_VERSION_MAJOR as usize);
    write_ascii_hex(&mut class, 2, GRID_EDITOR_VERSION_MINOR as usize);
    write_ascii_hex(&mut class, 2, GRID_EDITOR_VERSION_PATCH as usize);
    write_ascii_hex(&mut class, 2, 0);
    write_ascii_hex(&mut class, 2, 0);
    class.push(GRID_CONST_ETX);
    class.push(GRID_CONST_EOT);

    encode_packet(GridTarget::BROADCAST, heartbeat.identity, &class)
}

pub fn encode_page_store_packet(store: &PageStore) -> Result<Vec<u8>> {
    let mut class = vec![GRID_CONST_STX];
    write_ascii_hex(&mut class, 3, GRID_CLASS_PAGESTORE as usize);
    write_ascii_hex(&mut class, 1, GRID_INSTR_EXECUTE as usize);
    class.push(GRID_CONST_ETX);
    class.push(GRID_CONST_EOT);

    encode_packet(store.target, store.identity, &class)
}

pub fn encode_page_active_packet(change: &PageActive) -> Result<Vec<u8>> {
    let mut class = vec![GRID_CONST_STX];
    write_ascii_hex(&mut class, 3, GRID_CLASS_PAGEACTIVE as usize);
    write_ascii_hex(&mut class, 1, GRID_INSTR_EXECUTE as usize);
    write_ascii_hex(&mut class, 2, change.page as usize);
    class.push(GRID_CONST_ETX);
    class.push(GRID_CONST_EOT);

    encode_packet(change.target, change.identity, &class)
}

fn encode_packet(
    target: GridTarget,
    identity: PacketIdentity,
    class_block: &[u8],
) -> Result<Vec<u8>> {
    let mut brc = vec![GRID_CONST_SOH, GRID_CONST_BRC];
    write_ascii_hex(&mut brc, 4, 0);
    write_ascii_hex(&mut brc, 2, identity.message_id as usize);
    write_ascii_hex(&mut brc, 2, identity.session_id as usize);
    write_ascii_hex(&mut brc, 2, 0);
    write_ascii_hex(&mut brc, 2, 0);
    write_ascii_hex(&mut brc, 2, grid_coordinate_to_wire("dx", target.dx)?);
    write_ascii_hex(&mut brc, 2, grid_coordinate_to_wire("dy", target.dy)?);
    write_ascii_hex(&mut brc, 1, 0);
    write_ascii_hex(&mut brc, 1, 0);
    write_ascii_hex(&mut brc, 2, 0);
    brc.push(GRID_CONST_EOB);

    let mut packet = brc;
    packet.extend_from_slice(class_block);

    let frame_length = packet.len();
    overwrite_ascii_hex(&mut packet, 2, 4, frame_length);

    let checksum = packet.iter().fold(0u8, |acc, byte| acc ^ byte);
    write_ascii_hex(&mut packet, 2, checksum as usize);
    packet.push(GRID_CONST_LF);

    Ok(packet)
}

fn encode_script_bytes(script: &str) -> Result<Vec<u8>> {
    if !script.is_ascii() {
        return Err(ProtocolError::NonAsciiLua);
    }

    if script.len() >= GRID_MAX_LUA_BYTES {
        return Err(ProtocolError::LuaTooLong {
            length: script.len(),
            max_length: GRID_MAX_LUA_BYTES - 1,
        });
    }

    Ok(script.as_bytes().to_vec())
}

fn grid_coordinate_to_wire(axis: &'static str, value: i16) -> Result<usize> {
    let shifted = value + 127;
    if !(0..=255).contains(&shifted) {
        return Err(ProtocolError::CoordinateOutOfRange { axis, value });
    }

    Ok(shifted as usize)
}

fn write_ascii_hex(buffer: &mut Vec<u8>, width: usize, value: usize) {
    let text = format!("{value:0width$x}");
    buffer.extend_from_slice(text.as_bytes());
}

fn overwrite_ascii_hex(buffer: &mut [u8], offset: usize, width: usize, value: usize) {
    let text = format!("{value:0width$x}");
    buffer[offset..offset + width].copy_from_slice(text.as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_unframed_lua_deterministically() {
        assert_eq!(frame_lua("  return 1\n"), "<?lua --[[@cb]] return 1 ?>");
    }

    #[test]
    fn normalizes_existing_lua_frame() {
        assert_eq!(
            frame_lua(" <?lua   return 1   ?> "),
            "<?lua --[[@cb]] return 1 ?>"
        );
    }

    #[test]
    fn does_not_duplicate_existing_callback_prefix() {
        assert_eq!(
            frame_lua(" <?lua --[[@cb]] return 1 ?> "),
            "<?lua --[[@cb]] return 1 ?>"
        );
    }

    #[test]
    fn encodes_immediate_packet_with_framed_payload() {
        let packet = encode_immediate_packet(&ImmediateWrite {
            target: GridTarget::BROADCAST,
            lua: "test",
            identity: PacketIdentity::new(0xab, 0x01),
        })
        .unwrap();

        assert_eq!(&packet[0..2], &[GRID_CONST_SOH, GRID_CONST_BRC]);
        assert_eq!(&packet[2..6], b"0039");
        assert_eq!(&packet[23..28], b"\x02085e");
        assert_eq!(&packet[28..32], b"0017");
        assert_eq!(immediate_payload(&packet), b"<?lua --[[@cb]] test ?>");
        assert!(packet_has_valid_checksum(&packet));
        assert_eq!(packet.last(), Some(&GRID_CONST_LF));
    }

    #[test]
    fn encodes_config_packet_with_versioned_header() {
        let packet = encode_config_packet(&ConfigWrite {
            target: GridTarget::new(0, 1),
            location: ConfigLocation::new(0xff, 13, 8),
            lua: "return 1",
            identity: PacketIdentity::new(0x55, 0x02),
        })
        .unwrap();

        assert_eq!(&packet[2..6], b"0049");
        assert_eq!(&packet[14..18], b"7f80");
        assert_eq!(&packet[23..28], b"\x02060e");
        assert_eq!(&packet[28..34], b"010501");
        assert_eq!(&packet[34..40], b"ff0d08");
        assert_eq!(&packet[40..44], b"001b");
        assert_eq!(config_payload(&packet), b"<?lua --[[@cb]] return 1 ?>");
        assert!(packet_has_valid_checksum(&packet));
    }

    #[test]
    fn rejects_non_ascii_lua() {
        let error = encode_immediate_packet(&ImmediateWrite {
            target: GridTarget::BROADCAST,
            lua: "snowman = '☃'",
            identity: PacketIdentity::new(0, 1),
        })
        .unwrap_err();

        assert_eq!(error, ProtocolError::NonAsciiLua,);
    }

    #[test]
    fn rejects_out_of_range_coordinates() {
        let error = encode_immediate_packet(&ImmediateWrite {
            target: GridTarget::new(129, 0),
            lua: "return 1",
            identity: PacketIdentity::new(0, 1),
        })
        .unwrap_err();

        assert_eq!(
            error,
            ProtocolError::CoordinateOutOfRange {
                axis: "dx",
                value: 129,
            }
        );
    }

    #[test]
    fn encodes_config_fetch_packet_with_versioned_header() {
        let packet = encode_config_fetch_packet(&ConfigFetch {
            target: GridTarget::new(0, 1),
            location: ConfigLocation::new(0xff, 13, 8),
            identity: PacketIdentity::new(0x55, 0x02),
        })
        .unwrap();

        assert_eq!(&packet[23..28], b"\x02060f");
        assert_eq!(&packet[28..34], b"010501");
        assert_eq!(&packet[34..40], b"ff0d08");
        assert_eq!(&packet[40..44], b"0000");
        assert!(packet_has_valid_checksum(&packet));
    }

    #[test]
    fn encodes_heartbeat_packet_for_editor_session_bootstrap() {
        let packet = encode_heartbeat_packet(&Heartbeat {
            identity: PacketIdentity::new(0xaa, 0x03),
        })
        .unwrap();

        assert_eq!(&packet[14..18], b"0000");
        assert_eq!(&packet[23..28], b"\x02010e");
        assert_eq!(&packet[28..42], b"ffff0106050000");
        assert!(packet_has_valid_checksum(&packet));
    }

    #[test]
    fn encodes_page_store_packet() {
        let packet = encode_page_store_packet(&PageStore {
            target: GridTarget::new(0, 1),
            identity: PacketIdentity::new(0x55, 0x02),
        })
        .unwrap();

        assert_eq!(&packet[14..18], b"7f80");
        assert_eq!(&packet[23..28], b"\x02061e");
        assert!(packet_has_valid_checksum(&packet));
    }

    #[test]
    fn encodes_page_active_packet() {
        let packet = encode_page_active_packet(&PageActive {
            target: GridTarget::new(0, 1),
            page: 3,
            identity: PacketIdentity::new(0x55, 0x02),
        })
        .unwrap();

        assert_eq!(&packet[14..18], b"7f80");
        assert_eq!(&packet[23..30], b"\x02030e03");
        assert!(packet_has_valid_checksum(&packet));
    }

    fn immediate_payload(packet: &[u8]) -> &[u8] {
        &packet[32..packet.len() - 5]
    }

    fn config_payload(packet: &[u8]) -> &[u8] {
        &packet[44..packet.len() - 5]
    }

    fn packet_has_valid_checksum(packet: &[u8]) -> bool {
        let checksum_start = packet.len() - 3;
        let checksum = std::str::from_utf8(&packet[checksum_start..checksum_start + 2]).unwrap();
        let received = usize::from_str_radix(checksum, 16).unwrap() as u8;
        let calculated = packet[..checksum_start]
            .iter()
            .fold(0u8, |acc, byte| acc ^ byte);

        received == calculated
    }
}
