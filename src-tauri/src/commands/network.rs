use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct NetworkInterface {
    pub name: String,
    pub description: String,
    pub adapter_id: String,
    pub status: String,
    pub status_detail: String,
    pub is_up: bool,
    pub ipv4_addresses: Vec<String>,
    pub ipv6_addresses: Vec<String>,
    pub mac_address: Option<String>,
    pub dns_servers: Vec<String>,
    pub gateways: Vec<String>,
    pub mtu: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct NetworkOverview {
    pub interfaces: Vec<NetworkInterface>,
    pub default_gateways: Vec<String>,
    pub connected_interface_count: usize,
    pub collector_warning: Option<String>,
}

#[tauri::command]
pub fn get_network_overview() -> Result<NetworkOverview, String> {
    #[cfg(windows)]
    {
        windows::collect_network_overview()
    }

    #[cfg(not(windows))]
    {
        Err("Network overview is only supported on Windows.".to_string())
    }
}

#[cfg(windows)]
mod windows {
    use super::{NetworkInterface, NetworkOverview};
    use std::mem::size_of;
    use windows_sys::Win32::Foundation::{ERROR_BUFFER_OVERFLOW, ERROR_SUCCESS};
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        GetAdaptersAddresses, GAA_FLAG_INCLUDE_GATEWAYS, GAA_FLAG_INCLUDE_PREFIX,
        GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_MULTICAST, IP_ADAPTER_ADDRESSES_LH,
        IP_ADAPTER_DNS_SERVER_ADDRESS_XP, IP_ADAPTER_GATEWAY_ADDRESS_LH,
        IP_ADAPTER_UNICAST_ADDRESS_LH,
    };
    use windows_sys::Win32::NetworkManagement::Ndis::{
        IfOperStatusDormant as IF_OPER_STATUS_DORMANT, IfOperStatusDown as IF_OPER_STATUS_DOWN,
        IfOperStatusLowerLayerDown as IF_OPER_STATUS_LOWER_LAYER_DOWN,
        IfOperStatusNotPresent as IF_OPER_STATUS_NOT_PRESENT,
        IfOperStatusTesting as IF_OPER_STATUS_TESTING,
        IfOperStatusUnknown as IF_OPER_STATUS_UNKNOWN, IfOperStatusUp as IF_OPER_STATUS_UP,
    };
    use windows_sys::Win32::Networking::WinSock::{
        AF_INET, AF_INET6, AF_UNSPEC, SOCKADDR, SOCKADDR_IN, SOCKADDR_IN6, SOCKET_ADDRESS,
    };

    pub fn collect_network_overview() -> Result<NetworkOverview, String> {
        unsafe {
            let mut required_size: u32 = 15 * 1024;
            let mut adapters: Vec<IP_ADAPTER_ADDRESSES_LH> = Vec::new();
            let flags = GAA_FLAG_INCLUDE_GATEWAYS
                | GAA_FLAG_INCLUDE_PREFIX
                | GAA_FLAG_SKIP_ANYCAST
                | GAA_FLAG_SKIP_MULTICAST;
            let mut result = u32::MAX;

            for _ in 0..3 {
                let element_count =
                    (required_size as usize).div_ceil(size_of::<IP_ADAPTER_ADDRESSES_LH>());
                adapters = vec![IP_ADAPTER_ADDRESSES_LH::default(); element_count];
                result = GetAdaptersAddresses(
                    AF_UNSPEC as u32,
                    flags,
                    std::ptr::null(),
                    adapters.as_mut_ptr(),
                    &mut required_size,
                );

                if result == ERROR_SUCCESS {
                    break;
                }
                if result != ERROR_BUFFER_OVERFLOW {
                    return Err(format!(
                        "Windows could not enumerate network adapters (error {result})."
                    ));
                }
            }

            if result != ERROR_SUCCESS {
                return Err(format!(
                    "Windows could not allocate enough information for network adapters (error {result})."
                ));
            }

            let mut interfaces = Vec::new();
            let mut default_gateways = Vec::new();
            let mut connected_interface_count = 0usize;

            let mut current = adapters.as_mut_ptr();
            while !current.is_null() {
                let adapter = &*current;

                let adapter_id = pstr_to_string(adapter.AdapterName as *const u8)
                    .unwrap_or_else(|| "Unknown adapter".to_string());
                let description = pwstr_to_string(adapter.Description)
                    .filter(|value| !value.is_empty())
                    .unwrap_or_else(|| adapter_id.clone());
                let name = pwstr_to_string(adapter.FriendlyName)
                    .filter(|value| !value.is_empty())
                    .unwrap_or_else(|| description.clone());

                let status_value = adapter.OperStatus;
                let (status, status_detail) = status_label(status_value);
                let is_up = status_value == IF_OPER_STATUS_UP;
                if is_up {
                    connected_interface_count += 1;
                }

                let mut ipv4_addresses = Vec::new();
                let mut ipv6_addresses = Vec::new();
                collect_unicast_addresses(
                    adapter.FirstUnicastAddress,
                    &mut ipv4_addresses,
                    &mut ipv6_addresses,
                );

                let mac_address =
                    mac_address(&adapter.PhysicalAddress, adapter.PhysicalAddressLength);

                let mut dns_servers = Vec::new();
                collect_dns_server_addresses(adapter.FirstDnsServerAddress, &mut dns_servers);

                let mut gateways = Vec::new();
                collect_gateway_addresses(adapter.FirstGatewayAddress, &mut gateways);

                for gateway in &gateways {
                    if !default_gateways.contains(gateway) {
                        default_gateways.push(gateway.clone());
                    }
                }

                interfaces.push(NetworkInterface {
                    name,
                    description,
                    adapter_id,
                    status: status.to_string(),
                    status_detail: status_detail.to_string(),
                    is_up,
                    ipv4_addresses,
                    ipv6_addresses,
                    mac_address,
                    dns_servers,
                    gateways,
                    mtu: Some(adapter.Mtu).filter(|mtu| *mtu > 0),
                });

                current = adapter.Next;
            }

            Ok(NetworkOverview {
                interfaces,
                default_gateways,
                connected_interface_count,
                collector_warning: None,
            })
        }
    }

    unsafe fn collect_unicast_addresses(
        mut address: *mut IP_ADAPTER_UNICAST_ADDRESS_LH,
        ipv4: &mut Vec<String>,
        ipv6: &mut Vec<String>,
    ) {
        while !address.is_null() {
            if let Some(value) = socket_address_to_string((*address).Address) {
                match value.address_family {
                    AF_INET => push_unique(ipv4, value.address),
                    AF_INET6 => push_unique(ipv6, value.address),
                    _ => {}
                }
            }
            address = (*address).Next;
        }
    }

    unsafe fn collect_dns_server_addresses(
        mut address: *mut IP_ADAPTER_DNS_SERVER_ADDRESS_XP,
        output: &mut Vec<String>,
    ) {
        while !address.is_null() {
            // Each Windows DNS list node contains one socket address.
            if let Some(value) = socket_address_to_string((*address).Address) {
                push_unique(output, value.address);
            }
            address = (*address).Next;
        }
    }

    unsafe fn collect_gateway_addresses(
        mut address: *mut IP_ADAPTER_GATEWAY_ADDRESS_LH,
        output: &mut Vec<String>,
    ) {
        while !address.is_null() {
            if let Some(value) = socket_address_to_string((*address).Address) {
                push_unique(output, value.address);
            }
            address = (*address).Next;
        }
    }

    struct SocketAddressValue {
        address_family: u16,
        address: String,
    }

    unsafe fn socket_address_to_string(address: SOCKET_ADDRESS) -> Option<SocketAddressValue> {
        if address.lpSockaddr.is_null() || address.iSockaddrLength < size_of::<u16>() as i32 {
            return None;
        }

        let sockaddr = &*address.lpSockaddr;
        match sockaddr.sa_family {
            AF_INET => {
                if address.iSockaddrLength < size_of::<SOCKADDR_IN>() as i32 {
                    return None;
                }
                let ipv4 = &*(sockaddr as *const SOCKADDR as *const SOCKADDR_IN);
                let bytes = unsafe { ipv4.sin_addr.S_un.S_un_b };
                Some(SocketAddressValue {
                    address_family: AF_INET,
                    address: std::net::Ipv4Addr::new(
                        bytes.s_b1, bytes.s_b2, bytes.s_b3, bytes.s_b4,
                    )
                    .to_string(),
                })
            }
            AF_INET6 => {
                if address.iSockaddrLength < size_of::<SOCKADDR_IN6>() as i32 {
                    return None;
                }
                let ipv6 = &*(sockaddr as *const SOCKADDR as *const SOCKADDR_IN6);
                let bytes = unsafe { ipv6.sin6_addr.u.Byte };
                let scope_id = unsafe { ipv6.Anonymous.sin6_scope_id };
                let mut address = std::net::Ipv6Addr::from(bytes).to_string();
                if scope_id != 0 && std::net::Ipv6Addr::from(bytes).is_unicast_link_local() {
                    address.push('%');
                    address.push_str(&scope_id.to_string());
                }
                Some(SocketAddressValue {
                    address_family: AF_INET6,
                    address,
                })
            }
            _ => None,
        }
    }

    fn push_unique(output: &mut Vec<String>, value: String) {
        if !output.contains(&value) {
            output.push(value);
        }
    }

    fn mac_address(bytes: &[u8; 8], length: u32) -> Option<String> {
        let length = length as usize;
        if length == 0 {
            return None;
        }
        let length = length.min(bytes.len());
        if length == 0 {
            return None;
        }
        Some(
            bytes[..length]
                .iter()
                .map(|byte| format!("{byte:02X}"))
                .collect::<Vec<_>>()
                .join(":"),
        )
    }

    fn status_label(status: i32) -> (&'static str, &'static str) {
        match status {
            IF_OPER_STATUS_UP => ("Up", "Operational"),
            IF_OPER_STATUS_DOWN => ("Down", "No network connection"),
            IF_OPER_STATUS_DORMANT => ("Dormant", "Waiting for network activity"),
            IF_OPER_STATUS_LOWER_LAYER_DOWN => {
                ("Down", "Physical or lower-layer connection unavailable")
            }
            IF_OPER_STATUS_NOT_PRESENT => {
                ("Disconnected", "Adapter hardware or binding is not present")
            }
            IF_OPER_STATUS_TESTING => ("Testing", "Adapter is running a self-test"),
            IF_OPER_STATUS_UNKNOWN => ("Unknown", "Windows could not determine adapter status"),
            _ => ("Unknown", "Windows returned an unrecognized status"),
        }
    }

    unsafe fn pwstr_to_string(value: *mut u16) -> Option<String> {
        if value.is_null() {
            return None;
        }
        let mut length = 0usize;
        while *value.add(length) != 0 {
            length += 1;
            if length >= 4096 {
                break;
            }
        }
        Some(String::from_utf16_lossy(std::slice::from_raw_parts(
            value, length,
        )))
    }

    unsafe fn pstr_to_string(value: *const u8) -> Option<String> {
        if value.is_null() {
            return None;
        }
        let mut length = 0usize;
        while *value.add(length) != 0 {
            length += 1;
            if length >= 4096 {
                break;
            }
        }
        let bytes: Vec<u8> = std::slice::from_raw_parts(value, length).to_vec();
        String::from_utf8(bytes).ok()
    }
}
