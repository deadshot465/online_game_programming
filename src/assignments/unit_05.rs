use crate::bindings::Windows::Win32::NetworkManagement::IpHelper::AF_INET;
use crate::bindings::Windows::Win32::Networking::WinSock::{
    accept, bind, closesocket, htons, listen, recv, send, socket, WSACleanup, WSAData,
    WSAGetLastError, WSAStartup, IN_ADDR, IN_ADDR_0, SEND_FLAGS, SOCKADDR, SOCKADDR_IN, SOCKET,
    SOCKET_ERROR, SOCK_STREAM, SOMAXCONN,
};
use crate::bindings::Windows::Win32::System::SystemServices::{CHAR, PSTR};
use std::sync::{Arc, Mutex};
use winapi::shared::minwindef::MAKEWORD;
use winapi::shared::ws2def::INADDR_ANY;
use winapi::um::winsock2::INVALID_SOCKET;

const PORT: u16 = 7000;
const CLIENT_ADDR_SIZE: usize = std::mem::size_of::<SOCKADDR_IN>();
const BUFFER_SIZE: usize = 2048;
const RECV_PREFIX: &str = "受信データ：";
const DEFAULT_MAX_CLIENTS: usize = 10;

#[derive(Clone)]
struct Client {
    pub id: u32,
    pub addr: SOCKADDR_IN,
    pub socket: SOCKET,
}

impl Default for Client {
    fn default() -> Self {
        Client {
            id: 0,
            addr: SOCKADDR_IN {
                sin_family: 0,
                sin_port: 0,
                sin_addr: IN_ADDR {
                    S_un: IN_ADDR_0 { S_addr: 0 },
                },
                sin_zero: [CHAR(0); 8],
            },
            socket: SOCKET(INVALID_SOCKET),
        }
    }
}

unsafe fn check_socket_error(result: i32, msg: &str) -> bool {
    if result == SOCKET_ERROR {
        eprintln!("{}", msg);
        eprintln!("Error: {}", WSAGetLastError().0);
        WSACleanup();
        false
    } else {
        true
    }
}

struct ClientPool {
    pub socket_clients: Vec<Arc<Mutex<Client>>>,
    pub socket_client_threads: Vec<std::thread::JoinHandle<()>>,
}

impl ClientPool {
    pub fn new(pool_size: usize) -> Self {
        let mut client = Client::default();
        let mut client_vec = vec![];
        client_vec.resize_with(pool_size, || {
            let inner_client = client.clone();
            client.id += 1;
            Arc::new(Mutex::new(inner_client))
        });
        ClientPool {
            socket_clients: client_vec,
            socket_client_threads: Vec::with_capacity(pool_size),
        }
    }

    pub fn find_empty_client(&mut self) -> Arc<Mutex<Client>> {
        self.socket_clients
            .iter()
            .find(|c| c.lock().expect("Failed to lock socket client.").socket.0 == INVALID_SOCKET)
            .cloned()
            .unwrap_or_else(|| {
                self.socket_clients
                    .push(Arc::new(Mutex::new(Client::default())));
                self.socket_clients
                    .last()
                    .cloned()
                    .expect("There are no available socket clients.")
            })
    }

    pub unsafe fn start_messaging(
        &mut self,
        socket_client: Arc<Mutex<Client>>,
        mut server_msg: String,
        other_clients: Vec<Arc<Mutex<Client>>>,
    ) {
        self.socket_client_threads.push(std::thread::spawn(move || {
            let mut client_lock = socket_client.lock().expect("Failed to lock socket client.");
            send(
                &client_lock.socket,
                PSTR(server_msg.as_mut_ptr()),
                (server_msg.chars().count() as i32) + 1,
                SEND_FLAGS(0),
            );

            let mut recv_buffer = [0_u8; BUFFER_SIZE];
            loop {
                let recv_size = recv(
                    &client_lock.socket,
                    PSTR(recv_buffer.as_mut_ptr()),
                    recv_buffer.len() as i32,
                    0,
                );
                let mut incoming_message =
                    String::from_utf8_lossy(&recv_buffer[..(recv_size as usize)]).to_string();
                println!("{}{}", RECV_PREFIX, &incoming_message);
                if incoming_message.starts_with(":end") {
                    println!("{}", "終了コマンドを受信しました\n");
                    let mut bye_message = "Bye!\0".to_string();
                    send(
                        &client_lock.socket,
                        PSTR(bye_message.as_mut_ptr()),
                        bye_message.len() as i32,
                        SEND_FLAGS(0),
                    );
                    break;
                }

                println!(
                    "{} -> {}：{}\n",
                    client_lock.id, client_lock.id, &incoming_message
                );
                send(
                    &client_lock.socket,
                    PSTR(incoming_message.as_mut_ptr()),
                    incoming_message.len() as i32,
                    SEND_FLAGS(0),
                );

                for client in other_clients.iter() {
                    let other_client_lock = client.lock().expect("Failed to lock client socket.");
                    if other_client_lock.socket.0 == INVALID_SOCKET {
                        continue;
                    }
                    println!(
                        "{} -> {}：{}\n",
                        client_lock.id, other_client_lock.id, &incoming_message
                    );
                    send(
                        &other_client_lock.socket,
                        PSTR(incoming_message.as_mut_ptr()),
                        incoming_message.len() as i32,
                        SEND_FLAGS(0),
                    );
                }
            }

            let result = closesocket(&client_lock.socket);
            check_socket_error(result, "切断に失敗しました。");
            client_lock.socket.0 = INVALID_SOCKET;
        }));
    }
}

unsafe fn startup_wsa() -> bool {
    let version = MAKEWORD(2, 2);
    let mut wsa_data = WSAData::default();
    let result = WSAStartup(version, &mut wsa_data as *mut _);
    if result != 0 {
        eprintln!(
            "WSAStartup failed to initialize with error: {}\n",
            WSAGetLastError().0
        );
        false
    } else {
        true
    }
}

unsafe fn create_and_bind_socket() -> Option<SOCKET> {
    let addr = SOCKADDR_IN {
        sin_family: AF_INET.0 as u16,
        sin_port: htons(PORT),
        sin_addr: IN_ADDR {
            S_un: IN_ADDR_0 { S_addr: INADDR_ANY },
        },
        sin_zero: [CHAR(0); 8],
    };
    let socket = socket(AF_INET.0 as i32, SOCK_STREAM as i32, 0);
    if socket.0 == INVALID_SOCKET {
        eprintln!("ソケットの生成に失敗しました：{}\n", WSAGetLastError().0);
        WSACleanup();
        None
    } else {
        let result = bind(
            &socket,
            &addr as *const _ as *const SOCKADDR,
            std::mem::size_of::<SOCKADDR_IN>() as i32,
        );
        if !check_socket_error(result, "Socket binding failed.") {
            None
        } else {
            Some(socket)
        }
    }
}

pub unsafe fn unit_05() -> bool {
    if !startup_wsa() {
        return false;
    }

    let server_socket = create_and_bind_socket().expect("Failed to create server socket.");
    let result = listen(&server_socket, SOMAXCONN as i32);
    if !check_socket_error(result, "Socket failed to start listening.") {
        return false;
    }

    println!("サーバーが起動しました。\n");
    let server_msg = "Hello".to_string();

    let mut client_pool = ClientPool::new(DEFAULT_MAX_CLIENTS);

    loop {
        let client = client_pool.find_empty_client();
        let mut client_addr_size = CLIENT_ADDR_SIZE;
        let mut client_lock = client.lock().expect("Failed to lock client socket.");
        let accepted_socket = accept(
            &server_socket,
            &mut client_lock.addr as *mut _ as *mut SOCKADDR,
            &mut client_addr_size as *mut _ as *mut i32,
        );
        client_lock.socket = accepted_socket;

        if client_lock.socket.0 == INVALID_SOCKET {
            eprintln!("クライアントと接続失敗。エラー：{}\n", WSAGetLastError().0);
            continue;
        }

        let ip_address = format!(
            "クライアントが接続してきました！：IPAddress({}.{}.{}.{})\n",
            client_lock.addr.sin_addr.S_un.S_un_b.s_b1,
            client_lock.addr.sin_addr.S_un.S_un_b.s_b2,
            client_lock.addr.sin_addr.S_un.S_un_b.s_b3,
            client_lock.addr.sin_addr.S_un.S_un_b.s_b4,
        );
        println!("{}", &ip_address);
        let client_id = client_lock.id;
        drop(client_lock);
        let other_clients = client_pool
            .socket_clients
            .clone()
            .into_iter()
            .filter(|c| c.lock().expect("Failed to lock client socket.").id != client_id)
            .collect::<Vec<_>>();
        client_pool.start_messaging(client, server_msg.clone(), other_clients);
    }
}
