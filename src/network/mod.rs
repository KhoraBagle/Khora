use std::{net::TcpStream, time::{Instant, Duration}, io::{Read, Write}};


const LOOP_WAIT: u64 = 100;

/// reads the entire buffer if it can within the time period
pub fn read_timeout(stream: &mut TcpStream, mut buf: &mut [u8], timeout: Duration) -> bool {
    let time = Instant::now();
    println!("Reading from: {:?}",stream);
    // stream.set_read_timeout(Some(timeout));
    // return stream.read(buf).is_ok();
    stream.set_read_timeout(Some(std::time::Duration::from_millis(LOOP_WAIT)));
    std::thread::sleep(std::time::Duration::from_millis(LOOP_WAIT));
    while time.elapsed() <= timeout {
        if buf.is_empty() {
            return true;
        }
        match stream.read(buf) {
            Ok(0) => {
                // return true
            },
            Ok(n) => {
                println!("Read {} bytes",n);
                let tmp = buf;
                buf = &mut tmp[n..];
            }
            Err(e) => {
                println!("Error: {}",e);
                break
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(LOOP_WAIT));
    }
    println!("read_timeout timed out");
    false
}

/// reads the entire stream if it can within the time period
pub fn read_to_end_timeout(stream: &mut TcpStream, timeout: Duration) -> Option<Vec<u8>> {
    let mut vec = Vec::<u8>::new();
    let mut buf = [0u8;1000];
    let time = Instant::now();

    stream.set_read_timeout(Some(timeout));
    std::thread::sleep(std::time::Duration::from_millis(LOOP_WAIT));
    while time.elapsed() <= timeout {
        match stream.read(&mut buf) {
            Ok(0) => {
                return Some(vec)
            },
            Ok(n) => {
                println!("Read {} bytes",n);
                vec.extend(&buf[..n]);
            }
            Err(e) => {
                println!("Error: {}",e);
                break
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(LOOP_WAIT));
    }
    println!("read_to_end_timeout timed out");
    return None
}

/// writes the entire buffer if it can within the time period
pub fn write_timeout(stream: &mut TcpStream, mut buf: &[u8], timeout: Duration) -> bool {
    let time = Instant::now();
    println!("Writing to: {:?}",stream);

    while time.elapsed() < timeout {
        if buf.is_empty() {
            return true;
        }
        match stream.write(buf) {
            Ok(0) => {
                return true
            },
            Ok(n) => {
                let tmp = buf;
                buf = &tmp[n..];
            }
            Err(e) => {
                println!("Error: {}",e);
                break
            }
        }
    }
    println!("write_timeout timed out");
    false
}