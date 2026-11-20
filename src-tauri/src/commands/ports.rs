use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct PortEndpoint {
    pub protocol: String,
    pub local_address: String,
    pub local_port: u16,
    pub remote_address: Option<String>,
    pub remote_port: Option<u16>,
    pub state: String,
    pub pid: u32,
    pub process_name: Option<String>,
    pub executable_path: Option<String>,
    pub process_status: String,
    pub port_classification: String,
    pub common_service: Option<String>,
}

#[tauri::command]
pub async fn get_port_overview() -> Result<Vec<PortEndpoint>, String> {
    tauri::async_runtime::spawn_blocking(collect_port_overview)
        .await
        .map_err(|error| format!("Port discovery task could not be completed: {error}"))?
}

fn port_classification(port: u16) -> &'static str {
    match port {
        0..=1023 => "well_known",
        1024..=49151 => "registered",
        _ => "dynamic_private",
    }
}

fn common_service(port: u16, protocol: &str) -> Option<&'static str> {
    match (port, protocol) {
        (20, "TCP") => Some("FTP data (common association)"),
        (21, "TCP") => Some("FTP control (common association)"),
        (22, "TCP") => Some("SSH (common association)"),
        (25, "TCP") => Some("SMTP (common association)"),
        (53, "TCP" | "UDP") => Some("DNS (common association)"),
        (67, "UDP") => Some("DHCP server (common association)"),
        (68, "UDP") => Some("DHCP client (common association)"),
        (80, "TCP") => Some("HTTP (common association)"),
        (110, "TCP") => Some("POP3 (common association)"),
        (123, "UDP") => Some("NTP (common association)"),
        (143, "TCP") => Some("IMAP (common association)"),
        (443, "TCP" | "UDP") => Some("HTTPS / QUIC (common association)"),
        (445, "TCP") => Some("SMB (common association)"),
        (3306, "TCP") => Some("MySQL (common association)"),
        (5432, "TCP") => Some("PostgreSQL (common association)"),
        (8080, "TCP") => Some("HTTP alternate (common association)"),
        _ => None,
    }
}

fn make_endpoint(
    protocol: &str,
    local_address: String,
    local_port: u16,
    remote_address: Option<String>,
    remote_port: Option<u16>,
    state: String,
    pid: u32,
    process: ProcessInfo,
) -> PortEndpoint {
    PortEndpoint {
        protocol: protocol.to_string(),
        local_address,
        local_port,
        remote_address,
        remote_port,
        state,
        pid,
        process_name: process.name,
        executable_path: process.executable_path,
        process_status: process.status,
        port_classification: port_classification(local_port).to_string(),
        common_service: common_service(local_port, protocol).map(str::to_string),
    }
}

#[derive(Clone)]
struct ProcessInfo {
    name: Option<String>,
    executable_path: Option<String>,
    status: String,
}

#[cfg(windows)]
fn collect_port_overview() -> Result<Vec<PortEndpoint>, String> {
    windows::collect_port_overview()
}

#[cfg(not(windows))]
fn collect_port_overview() -> Result<Vec<PortEndpoint>, String> {
    Err("Port Intelligence is only supported on Windows.".to_string())
}

#[cfg(windows)]
mod windows {
    use super::{make_endpoint, PortEndpoint, ProcessInfo};
    use std::{
        collections::HashMap,
        ffi::c_void,
        mem::size_of,
        net::{Ipv4Addr, Ipv6Addr},
        os::windows::ffi::OsStringExt,
        ptr,
    };
    use windows_sys::Win32::{
        Foundation::{
            CloseHandle, GetLastError, ERROR_ACCESS_DENIED, ERROR_INSUFFICIENT_BUFFER,
            ERROR_INVALID_PARAMETER, ERROR_NO_DATA, ERROR_SUCCESS,
        },
        NetworkManagement::IpHelper::{
            GetExtendedTcpTable, GetExtendedUdpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCPROW_OWNER_PID,
            MIB_TCP_STATE_CLOSED, MIB_TCP_STATE_CLOSE_WAIT, MIB_TCP_STATE_CLOSING,
            MIB_TCP_STATE_DELETE_TCB, MIB_TCP_STATE_ESTAB, MIB_TCP_STATE_FIN_WAIT1,
            MIB_TCP_STATE_FIN_WAIT2, MIB_TCP_STATE_LAST_ACK, MIB_TCP_STATE_LISTEN,
            MIB_TCP_STATE_SYN_RCVD, MIB_TCP_STATE_SYN_SENT, MIB_TCP_STATE_TIME_WAIT,
            MIB_UDP6ROW_OWNER_PID, MIB_UDPROW_OWNER_PID, TCP_TABLE_OWNER_PID_ALL,
            UDP_TABLE_OWNER_PID,
        },
        Networking::WinSock::{AF_INET, AF_INET6},
        System::Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
            PROCESS_QUERY_LIMITED_INFORMATION,
        },
    };

    pub fn collect_port_overview() -> Result<Vec<PortEndpoint>, String> {
        let mut endpoints = Vec::new();
        let mut processes = HashMap::new();

        for (family, is_ipv6) in [(AF_INET as u32, false), (AF_INET6 as u32, true)] {
            let tcp_table = query_table(|table, size| unsafe {
                GetExtendedTcpTable(table, size, 0, family, TCP_TABLE_OWNER_PID_ALL, 0)
            })?;
            if is_ipv6 {
                for row in table_rows::<MIB_TCP6ROW_OWNER_PID>(&tcp_table)? {
                    let pid = row.dwOwningPid;
                    let process = process_info(&mut processes, pid);
                    endpoints.push(make_endpoint(
                        "TCP",
                        format_ipv6(row.ucLocalAddr, row.dwLocalScopeId),
                        network_port(row.dwLocalPort),
                        Some(format_ipv6(row.ucRemoteAddr, row.dwRemoteScopeId)),
                        Some(network_port(row.dwRemotePort)),
                        tcp_state(row.dwState).to_string(),
                        pid,
                        process,
                    ));
                }
            } else {
                for row in table_rows::<MIB_TCPROW_OWNER_PID>(&tcp_table)? {
                    let pid = row.dwOwningPid;
                    let process = process_info(&mut processes, pid);
                    endpoints.push(make_endpoint(
                        "TCP",
                        Ipv4Addr::from(row.dwLocalAddr.to_ne_bytes()).to_string(),
                        network_port(row.dwLocalPort),
                        Some(Ipv4Addr::from(row.dwRemoteAddr.to_ne_bytes()).to_string()),
                        Some(network_port(row.dwRemotePort)),
                        tcp_state(row.dwState).to_string(),
                        pid,
                        process,
                    ));
                }
            }

            let udp_table = query_table(|table, size| unsafe {
                GetExtendedUdpTable(table, size, 0, family, UDP_TABLE_OWNER_PID, 0)
            })?;
            if is_ipv6 {
                for row in table_rows::<MIB_UDP6ROW_OWNER_PID>(&udp_table)? {
                    let pid = row.dwOwningPid;
                    let process = process_info(&mut processes, pid);
                    endpoints.push(make_endpoint(
                        "UDP",
                        format_ipv6(row.ucLocalAddr, row.dwLocalScopeId),
                        network_port(row.dwLocalPort),
                        None,
                        None,
                        "N/A".to_string(),
                        pid,
                        process,
                    ));
                }
            } else {
                for row in table_rows::<MIB_UDPROW_OWNER_PID>(&udp_table)? {
                    let pid = row.dwOwningPid;
                    let process = process_info(&mut processes, pid);
                    endpoints.push(make_endpoint(
                        "UDP",
                        Ipv4Addr::from(row.dwLocalAddr.to_ne_bytes()).to_string(),
                        network_port(row.dwLocalPort),
                        None,
                        None,
                        "N/A".to_string(),
                        pid,
                        process,
                    ));
                }
            }
        }

        endpoints.sort_by(|left, right| {
            left.protocol
                .cmp(&right.protocol)
                .then(left.local_port.cmp(&right.local_port))
                .then(left.local_address.cmp(&right.local_address))
                .then(left.pid.cmp(&right.pid))
        });
        Ok(endpoints)
    }

    struct NativeTable {
        words: Vec<u64>,
        byte_len: usize,
    }

    fn query_table(
        mut collect: impl FnMut(*mut c_void, *mut u32) -> u32,
    ) -> Result<NativeTable, String> {
        let mut required_size = 0u32;
        let first_result = collect(ptr::null_mut(), &mut required_size);
        if first_result == ERROR_NO_DATA {
            return Ok(NativeTable {
                words: Vec::new(),
                byte_len: 0,
            });
        }
        if first_result != ERROR_INSUFFICIENT_BUFFER && first_result != ERROR_SUCCESS {
            return Err(format!(
                "Windows could not query local port information (error {first_result})."
            ));
        }
        if first_result == ERROR_SUCCESS && required_size == 0 {
            return Ok(NativeTable {
                words: Vec::new(),
                byte_len: 0,
            });
        }

        for _ in 0..4 {
            let byte_len = required_size as usize;
            let word_count = byte_len.div_ceil(size_of::<u64>());
            let mut words = vec![0u64; word_count];
            let mut available_size = u32::try_from(word_count * size_of::<u64>())
                .map_err(|_| "Windows port table exceeded supported size.".to_string())?;
            let result = collect(words.as_mut_ptr().cast(), &mut available_size);
            if result == ERROR_SUCCESS {
                return Ok(NativeTable {
                    words,
                    byte_len: (available_size as usize).min(word_count * size_of::<u64>()),
                });
            }
            if result == ERROR_NO_DATA {
                return Ok(NativeTable {
                    words: Vec::new(),
                    byte_len: 0,
                });
            }
            if result != ERROR_INSUFFICIENT_BUFFER {
                return Err(format!(
                    "Windows could not collect local port information (error {result})."
                ));
            }
            required_size = available_size.max(required_size.saturating_mul(2));
        }
        Err("Windows port tables changed repeatedly during collection; retry refresh.".to_string())
    }

    fn table_rows<T: Copy>(table: &NativeTable) -> Result<Vec<T>, String> {
        if table.byte_len < size_of::<u32>() {
            return Ok(Vec::new());
        }
        let base = table.words.as_ptr().cast::<u8>();
        let count = unsafe { ptr::read_unaligned(base.cast::<u32>()) } as usize;
        let rows_offset = size_of::<u32>();
        let row_bytes = table.byte_len.saturating_sub(rows_offset);
        if count > row_bytes / size_of::<T>() {
            return Err("Windows returned an invalid local port table size.".to_string());
        }
        let rows = unsafe { base.add(rows_offset).cast::<T>() };
        Ok((0..count)
            .map(|index| unsafe { ptr::read_unaligned(rows.add(index)) })
            .collect())
    }

    fn network_port(value: u32) -> u16 {
        u16::from_be(value as u16)
    }

    fn format_ipv6(address: [u8; 16], scope_id: u32) -> String {
        let address = Ipv6Addr::from(address);
        if scope_id == 0 {
            address.to_string()
        } else {
            format!("{address}%{scope_id}")
        }
    }

    fn tcp_state(state: u32) -> &'static str {
        match state as i32 {
            MIB_TCP_STATE_CLOSED => "CLOSED",
            MIB_TCP_STATE_LISTEN => "LISTENING",
            MIB_TCP_STATE_SYN_SENT => "SYN_SENT",
            MIB_TCP_STATE_SYN_RCVD => "SYN_RECEIVED",
            MIB_TCP_STATE_ESTAB => "ESTABLISHED",
            MIB_TCP_STATE_FIN_WAIT1 => "FIN_WAIT_1",
            MIB_TCP_STATE_FIN_WAIT2 => "FIN_WAIT_2",
            MIB_TCP_STATE_CLOSE_WAIT => "CLOSE_WAIT",
            MIB_TCP_STATE_CLOSING => "CLOSING",
            MIB_TCP_STATE_LAST_ACK => "LAST_ACK",
            MIB_TCP_STATE_TIME_WAIT => "TIME_WAIT",
            MIB_TCP_STATE_DELETE_TCB => "DELETE_TCB",
            _ => "UNKNOWN",
        }
    }

    fn process_info(cache: &mut HashMap<u32, ProcessInfo>, pid: u32) -> ProcessInfo {
        cache
            .entry(pid)
            .or_insert_with(|| identify_process(pid))
            .clone()
    }

    fn identify_process(pid: u32) -> ProcessInfo {
        if pid == 0 {
            return ProcessInfo {
                name: None,
                executable_path: None,
                status: "system_process".to_string(),
            };
        }
        unsafe {
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if process.is_null() {
                let status = if GetLastError() == ERROR_ACCESS_DENIED {
                    "access_denied"
                } else {
                    "unavailable"
                };
                return ProcessInfo {
                    name: None,
                    executable_path: None,
                    status: status.to_string(),
                };
            }
            let mut path = vec![0u16; 32768];
            let mut length = path.len() as u32;
            let succeeded = QueryFullProcessImageNameW(
                process,
                PROCESS_NAME_WIN32,
                path.as_mut_ptr(),
                &mut length,
            ) != 0;
            let query_error = if succeeded {
                ERROR_SUCCESS
            } else {
                GetLastError()
            };
            let close_result = CloseHandle(process);
            if close_result == 0 {
                eprintln!(
                    "Could not close process information handle for PID {pid}: {}",
                    std::io::Error::last_os_error()
                );
            }
            if !succeeded {
                let status = if query_error == ERROR_ACCESS_DENIED {
                    "access_denied"
                } else if query_error == ERROR_INVALID_PARAMETER {
                    "terminated"
                } else {
                    "unavailable"
                };
                return ProcessInfo {
                    name: None,
                    executable_path: None,
                    status: status.to_string(),
                };
            }
            path.truncate(length as usize);
            let executable = std::ffi::OsString::from_wide(&path);
            let executable_path = executable.to_string_lossy().into_owned();
            let name = std::path::Path::new(&executable)
                .file_name()
                .map(|value| value.to_string_lossy().into_owned());
            ProcessInfo {
                name,
                executable_path: Some(executable_path),
                status: "identified".to_string(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{common_service, make_endpoint, port_classification, ProcessInfo};

    #[test]
    fn classifies_ports_at_range_boundaries() {
        assert_eq!(port_classification(0), "well_known");
        assert_eq!(port_classification(1023), "well_known");
        assert_eq!(port_classification(1024), "registered");
        assert_eq!(port_classification(49151), "registered");
        assert_eq!(port_classification(49152), "dynamic_private");
        assert_eq!(port_classification(u16::MAX), "dynamic_private");
    }

    #[test]
    fn common_service_labels_do_not_claim_protocol_identity() {
        assert_eq!(
            common_service(443, "TCP"),
            Some("HTTPS / QUIC (common association)")
        );
        assert_eq!(
            common_service(443, "UDP"),
            Some("HTTPS / QUIC (common association)")
        );
        assert_eq!(common_service(45678, "TCP"), None);
        assert_eq!(common_service(53, "TCP"), Some("DNS (common association)"));
    }

    #[test]
    fn endpoint_keeps_missing_process_information_explicit() {
        let endpoint = make_endpoint(
            "UDP",
            "127.0.0.1".to_string(),
            5353,
            None,
            None,
            "N/A".to_string(),
            99_999,
            ProcessInfo {
                name: None,
                executable_path: None,
                status: "terminated".to_string(),
            },
        );
        assert_eq!(endpoint.process_status, "terminated");
        assert_eq!(endpoint.process_name, None);
        assert_eq!(endpoint.remote_address, None);
    }

    #[cfg(windows)]
    #[test]
    fn windows_snapshot_contains_live_loopback_tcp_and_udp_owned_by_this_process() {
        use super::windows::collect_port_overview;
        use std::{
            net::{TcpListener, TcpStream, UdpSocket},
            thread,
            time::Duration,
        };

        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind TCP test listener");
        let port = listener.local_addr().unwrap().port();
        let client = TcpStream::connect(("127.0.0.1", port)).expect("connect TCP test client");
        let (server, _) = listener.accept().expect("accept TCP test connection");
        let udp = UdpSocket::bind(("127.0.0.1", 0)).expect("bind UDP test endpoint");
        let ipv6_listener = TcpListener::bind("[::1]:0").ok();
        let ipv6_connection = ipv6_listener.as_ref().and_then(|listener| {
            let client = TcpStream::connect(listener.local_addr().ok()?).ok()?;
            let (server, _) = listener.accept().ok()?;
            Some((client, server, listener.local_addr().ok()?.port()))
        });
        thread::sleep(Duration::from_millis(50));

        let endpoints = collect_port_overview().expect("collect live Windows port tables");
        let pid = std::process::id();
        assert!(endpoints.iter().any(|entry| {
            entry.protocol == "TCP"
                && entry.local_port == port
                && entry.state == "LISTENING"
                && entry.pid == pid
                && entry.process_status == "identified"
        }));
        assert!(endpoints.iter().any(|entry| {
            entry.protocol == "TCP"
                && entry.local_port == port
                && entry.state == "ESTABLISHED"
                && entry.pid == pid
        }));
        assert!(endpoints.iter().any(|entry| {
            entry.protocol == "UDP"
                && entry.local_port == udp.local_addr().unwrap().port()
                && entry.pid == pid
        }));
        assert!(endpoints.iter().any(|entry| {
            entry.protocol == "UDP"
                && entry.local_port == udp.local_addr().unwrap().port()
                && entry.pid == pid
                && entry.process_name.is_some()
                && entry.executable_path.is_some()
        }));
        if let Some((_, _, ipv6_port)) = &ipv6_connection {
            assert!(endpoints.iter().any(|entry| {
                entry.protocol == "TCP"
                    && entry.local_address == "::1"
                    && entry.local_port == *ipv6_port
                    && entry.state == "LISTENING"
                    && entry.pid == pid
            }));
        }
        drop((
            server,
            client,
            listener,
            udp,
            ipv6_connection,
            ipv6_listener,
        ));
    }
}
