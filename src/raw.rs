use crate::protocol::{GridTarget, ImmediateWrite, PacketIdentity};
use crate::transport::{send_immediate, SerialTransport};
use crate::Result;

const RAW_PACKET_IDENTITY: PacketIdentity = PacketIdentity::new(0, 1);

pub fn send_screen_raw(
    transport: &mut impl SerialTransport,
    target: GridTarget,
    lua: &str,
) -> Result<()> {
    send_immediate(
        transport,
        &ImmediateWrite {
            target,
            lua,
            identity: RAW_PACKET_IDENTITY,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::frame_lua;
    use crate::transport::{FakeTransport, TransportError};

    #[test]
    fn send_screen_raw_frames_and_writes_one_immediate_packet() {
        let mut transport = FakeTransport::default();

        send_screen_raw(&mut transport, GridTarget::new(1, 2), " return 1 ").unwrap();

        assert_eq!(transport.immediate_writes().len(), 1);
        assert_eq!(transport.config_writes().len(), 0);

        let packet = &transport.immediate_writes()[0];
        let payload = &packet[32..packet.len() - 5];

        assert_eq!(payload, frame_lua(" return 1 ").as_bytes());
        assert_eq!(&packet[14..18], b"8081");
    }

    #[test]
    fn send_screen_raw_strips_non_ascii_before_sending() {
        let mut transport = FakeTransport::default();

        send_screen_raw(&mut transport, GridTarget::BROADCAST, "snowman = '☃'").unwrap();

        let packet = &transport.immediate_writes()[0];
        let payload = &packet[32..packet.len() - 5];

        assert_eq!(payload, b"<?lua --[[@cb]] snowman = '' ?>");
    }

    #[test]
    fn send_screen_raw_surfaces_transport_errors() {
        let mut transport = FakeTransport::default();
        transport.fail_next_immediate(TransportError::immediate("write failed"));

        let error = send_screen_raw(&mut transport, GridTarget::BROADCAST, "return 1").unwrap_err();

        assert_eq!(
            error.to_string(),
            "immediate transport write failed: write failed"
        );
    }
}
