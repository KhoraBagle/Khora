use std::net::{TcpListener, TcpStream};
use std::thread;
use std::io::Read;
use std::io::Write;

fn handle_client(mut stream: TcpStream) {
    // read 20 bytes at a time from stream echoing back to stream
    loop {
        let mut read = [0; 1028];
        match stream.read(&mut read) {
            Ok(n) => {
                println!("# RECIEVED MESSAGE: {}",String::from_utf8_lossy(&read));
                if n == 0 { 
                    // connection was closed
                    break;
                }
                stream.write(&read[0..n]).unwrap();
            }
            Err(err) => {
                panic!("# ERROR: {}",err);
            }
        }
    }
}
const DEFAULT_PORT: u16 = 8334;

fn main() {
    // let listener = TcpListener::bind(format!("127.0.0.1:{}",DEFAULT_PORT)).unwrap();
    let listener = TcpListener::bind(format!("0.0.0.0:{}",DEFAULT_PORT)).unwrap();
    // let listener = TcpListener::bind(format!("172.16.0.11:{}",DEFAULT_PORT)).unwrap();

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                thread::spawn(move || {
                    handle_client(stream);
                });
            }
            Err(_) => {
                println!("Error");
            }
        }
    }
}