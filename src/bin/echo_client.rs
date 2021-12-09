use std::fs::File;
use std::net::{TcpStream};
use std::io::{Read, Write};
use std::str::from_utf8;

fn main() {
    match TcpStream::connect("73.118.24.25:8334") {
        Ok(mut stream) => {
            println!("Successfully connected to server in port 3333");

            // let msg = b"Hello!";
            let msg = &[0u8;1028];

            stream.write(msg).unwrap();
            println!("Sent Hello, awaiting reply...");

            let mut data = vec![]; // using 6 byte buffer
            match stream.read_to_end(&mut data) {
                Ok(n) => {
                    println!("recieved {} bytes!",n);
                    let mut f = File::create("khora_usr").unwrap();
                    f.write(&data);
                },
                Err(e) => {
                    println!("Failed to receive data: {}", e);
                }
            }
        },
        Err(e) => {
            println!("Failed to connect: {}", e);
        }
    }
    println!("Terminated.");
}