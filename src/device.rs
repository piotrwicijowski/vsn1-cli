use std::error::Error as StdError;
use std::fmt;
use std::path::Path;

use serialport::{SerialPortInfo, SerialPortType};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceError {
    DiscoveryFailed {
        message: String,
    },
    NoSupportedDevice,
    AmbiguousDeviceSelection {
        port_names: Vec<String>,
    },
    RequestedDeviceNotFound {
        requested: String,
        port_names: Vec<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct KnownUsbDevice {
    vendor_id: u16,
    product_id: u16,
    label: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredDevice {
    pub port_name: String,
    pub vendor_id: u16,
    pub product_id: u16,
    pub serial_number: Option<String>,
    pub manufacturer: Option<String>,
    pub product: Option<String>,
    pub known_label: &'static str,
}

pub trait DeviceDiscovery {
    fn discover(&self) -> std::result::Result<Vec<DiscoveredDevice>, DeviceError>;
}

pub struct SystemDeviceDiscovery;

const KNOWN_USB_DEVICES: &[KnownUsbDevice] = &[
    KnownUsbDevice::new(0x03eb, 0xecac, "Grid / VSN1"),
    KnownUsbDevice::new(0x03eb, 0xecad, "Grid / VSN1"),
    KnownUsbDevice::new(0x303a, 0x8123, "Grid / VSN1"),
    KnownUsbDevice::new(0x03eb, 0x2402, "Grid D51 bootloader"),
    KnownUsbDevice::new(0x303a, 0x8122, "Grid ESP32 bootloader"),
    KnownUsbDevice::new(0x303a, 0x8124, "Knot bootloader"),
];

impl DeviceError {
    pub fn discovery_failed(message: impl Into<String>) -> Self {
        Self::DiscoveryFailed {
            message: message.into(),
        }
    }
}

impl KnownUsbDevice {
    const fn new(vendor_id: u16, product_id: u16, label: &'static str) -> Self {
        Self {
            vendor_id,
            product_id,
            label,
        }
    }
}

impl fmt::Display for DeviceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DiscoveryFailed { message } => {
                write!(f, "USB serial discovery failed: {message}")
            }
            Self::NoSupportedDevice => {
                write!(
                    f,
                    "no supported VSN1/Grid USB serial device found; reconnect the device and run `vsn1-cli device list` to inspect discovery"
                )
            }
            Self::AmbiguousDeviceSelection { port_names } => {
                write!(
                    f,
                    "multiple supported VSN1/Grid USB serial devices found ({}); rerun with `--device <path>` to select one explicitly",
                    port_names.join(", ")
                )
            }
            Self::RequestedDeviceNotFound {
                requested,
                port_names,
            } => {
                write!(
                    f,
                    "requested USB serial device `{requested}` was not found among supported VSN1/Grid devices ({})",
                    port_names.join(", ")
                )
            }
        }
    }
}

impl StdError for DeviceError {}

impl fmt::Display for DiscoveredDevice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} [{}] VID:PID {:04x}:{:04x}",
            self.port_name, self.known_label, self.vendor_id, self.product_id
        )?;

        if let Some(product) = self.product.as_deref() {
            write!(f, " product={product}")?;
        }

        if let Some(manufacturer) = self.manufacturer.as_deref() {
            write!(f, " manufacturer={manufacturer}")?;
        }

        if let Some(serial_number) = self.serial_number.as_deref() {
            write!(f, " serial={serial_number}")?;
        }

        Ok(())
    }
}

impl DeviceDiscovery for SystemDeviceDiscovery {
    fn discover(&self) -> std::result::Result<Vec<DiscoveredDevice>, DeviceError> {
        let ports = serialport::available_ports()
            .map_err(|error| DeviceError::discovery_failed(error.to_string()))?;

        let mut devices = ports
            .into_iter()
            .filter_map(map_matching_device)
            .filter(|device| device_path_exists(&device.port_name))
            .collect::<Vec<_>>();
        devices.sort_by(|left, right| left.port_name.cmp(&right.port_name));

        Ok(devices)
    }
}

pub fn discover_supported_devices(
    discovery: &impl DeviceDiscovery,
) -> std::result::Result<Vec<DiscoveredDevice>, DeviceError> {
    Ok(normalize_discovered_devices(discovery.discover()?))
}

pub fn select_device(
    devices: &[DiscoveredDevice],
    requested_port_name: Option<&str>,
) -> std::result::Result<DiscoveredDevice, DeviceError> {
    if let Some(requested_port_name) = requested_port_name {
        return devices
            .iter()
            .find(|device| device.port_name == requested_port_name)
            .cloned()
            .ok_or_else(|| DeviceError::RequestedDeviceNotFound {
                requested: requested_port_name.to_string(),
                port_names: devices
                    .iter()
                    .map(|device| device.port_name.clone())
                    .collect(),
            });
    }

    select_single_device(devices)
}

pub fn select_single_device(
    devices: &[DiscoveredDevice],
) -> std::result::Result<DiscoveredDevice, DeviceError> {
    match devices {
        [] => Err(DeviceError::NoSupportedDevice),
        [device] => Ok(device.clone()),
        _ => Err(DeviceError::AmbiguousDeviceSelection {
            port_names: devices
                .iter()
                .map(|device| device.port_name.clone())
                .collect(),
        }),
    }
}

fn map_matching_device(port: SerialPortInfo) -> Option<DiscoveredDevice> {
    let SerialPortType::UsbPort(usb_info) = port.port_type else {
        return None;
    };

    let known = KNOWN_USB_DEVICES
        .iter()
        .find(|device| device.vendor_id == usb_info.vid && device.product_id == usb_info.pid)?;

    Some(DiscoveredDevice {
        port_name: port.port_name,
        vendor_id: usb_info.vid,
        product_id: usb_info.pid,
        serial_number: usb_info.serial_number,
        manufacturer: usb_info.manufacturer,
        product: usb_info.product,
        known_label: known.label,
    })
}

fn device_path_exists(port_name: &str) -> bool {
    Path::new(port_name).exists()
}

fn normalize_discovered_devices(devices: Vec<DiscoveredDevice>) -> Vec<DiscoveredDevice> {
    let mut normalized = Vec::with_capacity(devices.len());

    for device in devices {
        match normalized
            .iter_mut()
            .find(|existing| are_macos_tty_cu_aliases(existing, &device))
        {
            Some(existing) if prefers_device(&device, existing) => *existing = device,
            Some(_) => {}
            None => normalized.push(device),
        }
    }

    normalized.sort_by(|left, right| left.port_name.cmp(&right.port_name));
    normalized
}

fn are_macos_tty_cu_aliases(left: &DiscoveredDevice, right: &DiscoveredDevice) -> bool {
    let Some(left_suffix) = macos_serial_suffix(&left.port_name) else {
        return false;
    };
    let Some(right_suffix) = macos_serial_suffix(&right.port_name) else {
        return false;
    };

    left_suffix == right_suffix
        && left.vendor_id == right.vendor_id
        && left.product_id == right.product_id
        && left.serial_number == right.serial_number
        && left.manufacturer == right.manufacturer
        && left.product == right.product
        && left.known_label == right.known_label
}

fn macos_serial_suffix(port_name: &str) -> Option<&str> {
    port_name
        .strip_prefix("/dev/cu.")
        .or_else(|| port_name.strip_prefix("/dev/tty."))
}

fn prefers_device(candidate: &DiscoveredDevice, existing: &DiscoveredDevice) -> bool {
    device_preference_rank(&candidate.port_name) < device_preference_rank(&existing.port_name)
}

fn device_preference_rank(port_name: &str) -> u8 {
    if port_name.starts_with("/dev/cu.") {
        0
    } else if port_name.starts_with("/dev/tty.") {
        1
    } else {
        2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct StaticDiscovery {
        devices: Vec<DiscoveredDevice>,
    }

    impl DeviceDiscovery for StaticDiscovery {
        fn discover(&self) -> std::result::Result<Vec<DiscoveredDevice>, DeviceError> {
            Ok(self.devices.clone())
        }
    }

    #[test]
    fn selects_the_only_discovered_device() {
        let discovery = StaticDiscovery {
            devices: vec![test_device("/dev/ttyACM0")],
        };

        let devices = discover_supported_devices(&discovery).unwrap();
        let selected = select_single_device(&devices).unwrap();

        assert_eq!(selected.port_name, "/dev/ttyACM0");
    }

    #[test]
    fn collapses_macos_tty_cu_pairs_to_the_callout_device() {
        let discovery = StaticDiscovery {
            devices: vec![
                test_device("/dev/tty.usbmodem101"),
                test_device("/dev/cu.usbmodem101"),
            ],
        };

        let devices = discover_supported_devices(&discovery).unwrap();

        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].port_name, "/dev/cu.usbmodem101");
    }

    #[test]
    fn fails_when_multiple_supported_devices_are_visible() {
        let devices = vec![test_device("/dev/ttyACM0"), test_device("/dev/ttyACM1")];

        let error = select_single_device(&devices).unwrap_err();

        assert_eq!(
            error,
            DeviceError::AmbiguousDeviceSelection {
                port_names: vec!["/dev/ttyACM0".to_string(), "/dev/ttyACM1".to_string()],
            }
        );
    }

    #[test]
    fn selects_the_requested_device_when_multiple_supported_devices_are_visible() {
        let devices = vec![test_device("/dev/ttyACM0"), test_device("/dev/ttyACM1")];

        let selected = select_device(&devices, Some("/dev/ttyACM1")).unwrap();

        assert_eq!(selected.port_name, "/dev/ttyACM1");
    }

    #[test]
    fn fails_when_the_requested_device_is_not_visible() {
        let devices = vec![test_device("/dev/ttyACM0")];

        let error = select_device(&devices, Some("/dev/ttyACM1")).unwrap_err();

        assert_eq!(
            error,
            DeviceError::RequestedDeviceNotFound {
                requested: "/dev/ttyACM1".to_string(),
                port_names: vec!["/dev/ttyACM0".to_string()],
            }
        );
    }

    #[test]
    fn system_discovery_only_keeps_device_paths_that_exist() {
        assert!(device_path_exists("/dev/null"));
        assert!(!device_path_exists("/definitely/not/a/vsn1/device"));
    }

    fn test_device(port_name: &str) -> DiscoveredDevice {
        DiscoveredDevice {
            port_name: port_name.to_string(),
            vendor_id: 0x03eb,
            product_id: 0xecac,
            serial_number: Some("ABC123".to_string()),
            manufacturer: Some("Intech".to_string()),
            product: Some("VSN1".to_string()),
            known_label: "Grid / VSN1",
        }
    }
}
