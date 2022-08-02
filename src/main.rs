use std::error::Error;
use std::net::{Ipv4Addr, UdpSocket};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use argh::FromArgs;
use once_cell::sync::Lazy;
use pnet::datalink::{self, DataLinkSender, NetworkInterface};

use pnet::datalink::{Channel, DataLinkReceiver};
use pnet::packet::arp::{ArpHardwareTypes, ArpOperations, ArpPacket, MutableArpPacket};
use pnet::packet::ethernet::{EtherTypes, EthernetPacket, MutableEthernetPacket};
use pnet::packet::ip::IpNextHeaderProtocols;
use pnet::packet::ipv4::Ipv4Packet;
use pnet::packet::udp::UdpPacket;
use pnet::packet::{Packet, PacketSize};
use pnet::util::MacAddr;
use regex::Regex;
use std::fmt::Formatter;
use std::{fmt, thread};

const ROUTING_TABLE_NUMBER: u32 = 123;

static OUR_TURN_TO_HOLD_IP: Lazy<Mutex<bool>> = Lazy::new(|| Mutex::new(false));

#[derive(Debug)]
struct SimpleError(String);

impl Error for SimpleError {}

impl fmt::Display for SimpleError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for SimpleError {
    fn from(s: &str) -> Self {
        SimpleError(String::from(s))
    }
}
impl From<std::io::Error> for SimpleError {
    fn from(e: std::io::Error) -> Self {
        SimpleError(e.to_string())
    }
}

fn err(s: String) -> Box<SimpleError> {
    Box::new(SimpleError(s))
}

/// Settings
#[derive(Debug, FromArgs)]
struct Settings {
    /// the IP whose owner is to change
    #[argh(option)]
    ip_to_juggle: Ipv4Addr,
    /// gateway of IP to juggle
    #[argh(option)]
    gateway: Ipv4Addr,
    /// netmask of IP to juggle
    #[argh(option)]
    netmask: u8,
    /// local interface to operate on
    #[argh(option)]
    local_interface_regex: String,
    /// total number of participating machines
    #[argh(option)]
    total_participants: usize,
    /// index of this instance in the set of participants
    #[argh(option)]
    local_index: usize,
    /// duration in seconds of holding an address. Default: 15
    #[argh(option, default = "15.0")]
    turn_duration: f64,
    /// the UDP port for pinging. Default: 1234
    #[argh(option, default = "1234")]
    udp_ping_port: u16,
    /// interval in seconds for UDP pings. Default: 1
    #[argh(option, default = "1.0")]
    udp_ping_interval: f64,
}

fn main() -> Result<(), Box<dyn Error>> {
    let settings: Settings = argh::from_env();
    println!("Instance ID: {}", settings.local_index);
    println!("Settings: {:?}", settings);

    let iface: NetworkInterface = {
        let local_interface_regex = Regex::new(&settings.local_interface_regex)?;
        let mut all_ifaces = datalink::interfaces();
        all_ifaces.sort_by_cached_key(|iface| iface.name.clone());
        all_ifaces
            .into_iter()
            .find(|iface| local_interface_regex.is_match(&iface.name))
            .expect(&format!(
                "Could not find interface matching regex: {}",
                settings.local_interface_regex
            ))
    };

    let (tx, rx) = {
        match datalink::channel(&iface, Default::default()) {
            Ok(Channel::Ethernet(tx, rx)) => (tx, rx),
            Ok(_) => return Err(err(format!("Unhandled channel type"))),
            Err(e) => {
                return Err(err(format!(
                    "An error occurred when creating the datalink channel: {}",
                    e
                )))
            }
        }
    };

    let juggler_thread = {
        let iface_name = iface.name.clone();
        let source_mac = iface.mac.ok_or(SimpleError(format!(
            "Failed to get MAC address of interface: {}",
            iface.name
        )))?;
        let ip = settings.ip_to_juggle;
        let netmask = settings.netmask;
        let gateway = settings.gateway;
        let turn_duration = settings.turn_duration;
        let total_participants = settings.total_participants;
        let local_index = settings.local_index;
        thread::spawn(move || {
            juggler(
                tx,
                iface_name,
                source_mac,
                ip,
                netmask,
                gateway,
                turn_duration,
                total_participants,
                local_index,
            )
        })
    };
    let ping_sender_thread = {
        let ping_interval = settings.udp_ping_interval;
        let ip_to_juggle = settings.ip_to_juggle;
        let port = settings.udp_ping_port;
        thread::spawn(move || ping_sender(ping_interval, ip_to_juggle, port))
    };
    let ping_listener_thread = {
        let port = settings.udp_ping_port;
        thread::spawn(move || ping_listener(rx, port))
    };

    juggler_thread.join().expect("juggler thread panicked");
    ping_sender_thread
        .join()
        .expect("ping sender thread panicked");
    ping_listener_thread
        .join()
        .expect("ping listener thread panicked");

    Ok(())
}

fn juggler(
    tx: Box<dyn DataLinkSender>,
    iface_name: String,
    source_mac: MacAddr,
    ip_to_juggle: Ipv4Addr,
    netmask: u8,
    gateway: Ipv4Addr,
    turn_duration: f64,
    total_participants: usize,
    local_index: usize,
) {
    let tx: Arc<Mutex<Box<dyn DataLinkSender>>> = Arc::new(Mutex::new(tx));

    let _ = give_up_ip(&iface_name, ip_to_juggle, netmask, gateway); // Ignore any errors
    loop {
        let t = unix_time();
        let turn_number = (t / turn_duration).floor() as usize;
        let turn_remaining = turn_duration - t % turn_duration;

        if turn_number % total_participants == local_index {
            println!("Taking IP (turn {})", turn_number % total_participants);
            *OUR_TURN_TO_HOLD_IP.lock().unwrap() = true;
            match take_ip(&iface_name, ip_to_juggle, netmask, gateway) {
                Ok(_) => {
                    let tx = tx.clone();
                    thread::spawn(move || {
                        arp_spammer(
                            tx.lock().unwrap().as_mut(),
                            source_mac,
                            ip_to_juggle,
                            gateway,
                        );
                    });
                }
                Err(e) => println!("Failed to take IP: {}", e),
            }
        } else {
            println!("Giving up IP (turn {})", turn_number % total_participants);
            *OUR_TURN_TO_HOLD_IP.lock().unwrap() = false;
            if let Err(e) = give_up_ip(&iface_name, ip_to_juggle, netmask, gateway) {
                println!("Failed to give up IP: {}", e);
            }
        }

        thread::sleep(Duration::from_secs_f64(turn_remaining + 0.0001));
    }
}

fn arp_spammer(
    tx: &mut dyn DataLinkSender,
    source_mac: MacAddr,
    source_ip: Ipv4Addr,
    _gateway: Ipv4Addr,
) {
    for _ in 0..5 {
        if *OUR_TURN_TO_HOLD_IP.lock().unwrap() {
            let mut arp_packet =
                MutableArpPacket::owned(vec![0; MutableArpPacket::minimum_packet_size()]).unwrap();
            arp_packet.set_hardware_type(ArpHardwareTypes::Ethernet);
            arp_packet.set_protocol_type(EtherTypes::Ipv4);
            arp_packet.set_hw_addr_len(6);
            arp_packet.set_proto_addr_len(4);
            arp_packet.set_operation(ArpOperations::Reply);
            arp_packet.set_sender_hw_addr(source_mac);
            arp_packet.set_sender_proto_addr(source_ip);
            arp_packet.set_target_hw_addr(MacAddr::broadcast()); // https://gist.github.com/seungwon0/7110259
            arp_packet.set_target_proto_addr(source_ip); // TODO: gateway?

            let mut eth_packet = MutableEthernetPacket::owned(vec![
                0;
                MutableEthernetPacket::minimum_packet_size()
                    + arp_packet.packet_size()
            ])
            .unwrap();
            eth_packet.set_destination(MacAddr::broadcast());
            eth_packet.set_source(source_mac);
            eth_packet.set_ethertype(EtherTypes::Arp);
            eth_packet.set_payload(arp_packet.packet());

            match tx.send_to(eth_packet.packet(), None) {
                Some(res) => match res {
                    Ok(_) => {}
                    Err(e) => eprintln!("Failed to send gratuitous ARP: {}", e),
                },
                None => eprintln!("Failed to send gratuitous ARP: no result"),
            }
            thread::sleep(Duration::from_secs_f64(0.1));
        } else {
            break;
        }
    }
}

fn unix_time() -> f64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
}

fn take_ip(
    iface_name: &str,
    ip_to_juggle: Ipv4Addr,
    netmask: u8,
    gateway: Ipv4Addr,
) -> Result<(), SimpleError> {
    // TODO: do we need to set boradcast address with 'brd'?
    run_cmd(&[
        "ip",
        "addr",
        "add",
        &format!("{}/{}", ip_to_juggle, netmask),
        "dev",
        iface_name,
    ])?;
    run_cmd(&[
        "ip",
        "rule",
        "add",
        "from",
        &ip_to_juggle.to_string(),
        "lookup",
        &ROUTING_TABLE_NUMBER.to_string(),
    ])?;
    run_cmd(&[
        "ip",
        "rule",
        "add",
        "to",
        &ip_to_juggle.to_string(),
        "lookup",
        &ROUTING_TABLE_NUMBER.to_string(),
    ])?;
    run_cmd(&[
        "ip",
        "rule",
        "add",
        "default",
        "via",
        &gateway.to_string(),
        "dev",
        iface_name,
        "table",
        &ROUTING_TABLE_NUMBER.to_string(),
    ])?;

    // TODO: arp?

    Ok(())
}

fn give_up_ip(
    iface_name: &str,
    ip_to_juggle: Ipv4Addr,
    netmask: u8,
    gateway: Ipv4Addr,
) -> Result<(), SimpleError> {
    let mut results = Vec::new();
    results.push(run_cmd(&[
        "ip",
        "rule",
        "del",
        "default",
        "via",
        &gateway.to_string(),
        "dev",
        iface_name,
        "table",
        &ROUTING_TABLE_NUMBER.to_string(),
    ]));
    results.push(run_cmd(&[
        "ip",
        "rule",
        "del",
        "to",
        &ip_to_juggle.to_string(),
        "lookup",
        &ROUTING_TABLE_NUMBER.to_string(),
    ]));
    results.push(run_cmd(&[
        "ip",
        "rule",
        "del",
        "from",
        &ip_to_juggle.to_string(),
        "lookup",
        &ROUTING_TABLE_NUMBER.to_string(),
    ]));
    results.push(run_cmd(&[
        "ip",
        "addr",
        "del",
        &format!("{}/{}", ip_to_juggle, netmask),
        "dev",
        iface_name,
    ]));

    // TODO: arp?

    for result in results {
        if let Err(e) = result {
            return Err(e);
        }
    }

    Ok(())
}

fn run_cmd(cmd: &[&str]) -> Result<Vec<u8>, SimpleError> {
    Command::new(cmd[0])
        .args(&cmd[1..])
        .output()
        .map(|o| o.stdout)
        .map_err(|e| SimpleError(format!("Failed to run command '{}': {}", cmd.join(" "), e)))
}

fn ping_sender(interval: f64, dest_ip: Ipv4Addr, port: u16) {
    loop {
        thread::sleep(Duration::from_secs_f64(interval));
        if !*OUR_TURN_TO_HOLD_IP.lock().unwrap() {
            let socket =
                UdpSocket::bind("0.0.0.0:0").expect("failed to open UDP socket for sending");
            let local_addr = socket.local_addr().unwrap();
            println!("Sending {} -> {}", local_addr, dest_ip);
            match socket.send_to("hello".as_bytes(), (dest_ip, port)) {
                Ok(_) => {}
                Err(e) => eprintln!("failed to send UDP ping: {}", e),
            }
        }
    }
}

fn ping_listener(mut rx: Box<dyn DataLinkReceiver>, ping_port: u16) {
    loop {
        match rx.next() {
            Ok(packet) => match handle_incoming_packet(packet, ping_port) {
                Ok(()) => {}
                Err(e) => eprintln!("Failed to handle incoming Ethernet packet: {}", e),
            },
            Err(e) => {
                panic!("Failed to read from interface: {}", e)
            }
        }
    }
}

fn handle_incoming_packet(eth_packet_buf: &[u8], ping_port: u16) -> Result<(), SimpleError> {
    let eth_packet =
        EthernetPacket::new(eth_packet_buf).ok_or("failed to parse ethernet packet")?;
    if eth_packet.get_ethertype() == EtherTypes::Ipv4 {
        let ipv4_packet =
            Ipv4Packet::new(eth_packet.payload()).ok_or("failed to parse IPv4 packet")?;
        if ipv4_packet.get_next_level_protocol() == IpNextHeaderProtocols::Udp {
            let udp_packet =
                UdpPacket::new(ipv4_packet.payload()).ok_or("failed to parse UDP packet")?;
            if udp_packet.get_destination() == ping_port {
                println!(
                    "Received {} -> {} ({})",
                    ipv4_packet.get_source(),
                    ipv4_packet.get_destination(),
                    if *OUR_TURN_TO_HOLD_IP.lock().unwrap() {
                        "ok"
                    } else {
                        "but IP not held!"
                    }
                );
            }
        }
    } else if eth_packet.get_ethertype() == EtherTypes::Arp {
        let arp_packet =
            ArpPacket::new(eth_packet.payload()).ok_or("failed to parse ARP packet")?;
        println!(
            "Got ARP {} : {} ({}) -> {} ({})",
            arp_packet.get_operation().0,
            arp_packet.get_sender_proto_addr(),
            arp_packet.get_sender_hw_addr(),
            arp_packet.get_target_proto_addr(),
            arp_packet.get_target_hw_addr(),
        );
    }

    Ok(())
}
