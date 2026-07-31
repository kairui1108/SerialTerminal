// 最小复现：serialport 读取 PTY 串口对
use std::time::Duration;

fn main() {
    let port_name = std::env::args().nth(1).expect("usage: readtest <port>");
    let mut p = serialport::new(&port_name, 9600)
        .timeout(Duration::from_millis(100))
        .open()
        .expect("open failed");
    eprintln!("opened {}", port_name);
    let mut buf = [0u8; 64];
    let mut n_total = 0usize;
    loop {
        match p.read(&mut buf) {
            Ok(n) if n > 0 => {
                n_total += n;
                eprintln!("READ {} bytes: {:?}", n, &buf[..n]);
                if n_total > 1000 {
                    break;
                }
            }
            Ok(_) => {}
            Err(_) => {} // 超时，继续
        }
    }
}
