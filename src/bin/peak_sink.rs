extern crate syscall;
extern crate termion;

use std::io::{stdin, stdout, Read, Write};

use syscall::*;
use termion::cursor;
use termion::raw::IntoRawMode;

fn main() {
    let endpoint_name = std::env::args().nth(1).expect("argument 1 should be the desired endpoint name");
    let fd = open(format!("audio:{}?buf_sz=256", endpoint_name), O_CREAT | O_RDONLY).unwrap();
    let addr = unsafe { fmap(fd, 0, 1024).unwrap() };
    let buffer: &'static mut [i32] = unsafe { std::slice::from_raw_parts_mut(addr as *mut i32, 256) };
    println!("Press any key to send a clock tick");
    let mut stdout = stdout().into_raw_mode().unwrap();
    write!(stdout, "Peak: {}", cursor::Save);
    stdout.flush();
    let mut stdin_buf = [0; 4];
    loop {
        stdin().read(&mut stdin_buf);
        fsync(fd).unwrap();
        let high_peak = buffer.iter().max().unwrap();
        let low_peak = buffer.iter().min().unwrap();
        let peak;
        if low_peak.abs() > *high_peak {
            peak = low_peak;
        } else {
            peak = high_peak;
        }
        write!(stdout, "{}{}", cursor::Restore, peak);
        stdout.flush();
    }
}
