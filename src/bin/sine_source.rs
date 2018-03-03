extern crate syscall;

use std::io::Read;

use syscall::*;

fn main() {
    let connect = std::env::args().nth(1).expect("argument 1 should be a sink name to connect to");
    let fd = open(format!("audio:sine_source?buf_sz=256&connect={}", connect), O_CREAT | O_WRONLY).unwrap();
    let addr = unsafe { fmap(fd, 0, 1024) }.unwrap();
    let buffer: &'static mut [i32] = unsafe { std::slice::from_raw_parts_mut(addr as *mut i32, 256) };
    println!("buffer view: {:?}", &buffer[0..16]);
    fevent(fd, EVENT_WRITE);
    let mut event_file = std::fs::File::open("event:").unwrap();
    let mut phase = 0.;
    loop {
        let mut event = Event::default();
        if event_file.read(&mut event).unwrap() > 0 {
            if event.id == fd {
                if event.data == 0 {
                    break;
                }
                println!("starting phase: {}", phase);
                for val in buffer.iter_mut() {
                    *val = ((phase * std::f32::consts::PI * 2.0).sin() * (i32::max_value() / 16) as f32) as i32;
                    phase += 0.05;
                }
            }
        }
    }
}
