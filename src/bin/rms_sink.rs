extern crate syscall;
extern crate termion;

use std::time::Duration;
use std::thread::sleep;

use syscall::*;
use termion::cursor;

fn main() {
    let fd = open("audio:rms_sink?buf_sz=256", O_CREAT | O_RDONLY).unwrap();
    let addr = unsafe { fmap(fd, 0, 1024).unwrap() };
    let buf = unsafe { *(addr as *const [i32; 256]) };
    print!("RMS: {}", cursor::Save);
    loop {
        fsync(fd).unwrap();
        sleep(Duration::new(1, 0));
        let sum_of_squares = buf.iter().fold(0, |acc, &x| acc + x^2);
        let mean = sum_of_squares / 256;
        let rms = (mean as f64).sqrt();
        print!("{}{}", cursor::Restore, rms);
    }
}
