extern crate syscall;
extern crate event;
extern crate rand;

use std::fs::File;
use std::rc::Rc;
use std::cell::RefCell;
use std::os::unix::io::{FromRawFd, RawFd};
use std::io::{Result, Read, Write};

use syscall::data::Packet;
use syscall::SchemeMut;

mod buffer;
mod scheme;

fn main() {
    if unsafe { syscall::clone(0).unwrap() } == 0 {
        let socket_fd = syscall::open(":audio", syscall::O_RDWR | syscall::O_CREAT | syscall::O_NONBLOCK).expect("crystald: failed to open scheme") as RawFd;
        let socket = Rc::new(RefCell::new(unsafe { File::from_raw_fd(socket_fd) }));
        let socket_closure = socket.clone();
        let mut event_queue = event::EventQueue::<usize>::new().unwrap();
        let scheme = RefCell::new(scheme::AudioScheme::new(socket));
        event_queue.add(socket_fd, move |_count| -> Result<Option<usize>> {
            loop {
                let mut packet = Packet::default();
                if socket_closure.borrow_mut().read(&mut packet)? == 0 {
                    break;
                }

                scheme.borrow_mut().handle(&mut packet);
                socket_closure.borrow_mut().write(&mut packet).unwrap();
            }
            Ok(None)
        }).expect("failed to catch events on scheme");
        loop {
            event_queue.run().unwrap();
        }
    }
}
