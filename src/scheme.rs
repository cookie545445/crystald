use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;
use std::cell::RefCell;
use std::fs::File;
use std::io::Write;
use std::str;

use syscall;
use syscall::scheme::SchemeMut;
use syscall::data::Packet;
use syscall::flag::*;
use syscall::error::*;
use rand::random;

use buffer::AudioBuffer;

struct Endpoint {
    name: String,
    buffer: AudioBuffer<i32>,
    connections: BTreeSet<usize>,
    endpoint_type: EndpointType,
    clock_source: Option<usize>,
}

enum EndpointType {
    Source,
    Sink,
}

pub struct AudioScheme {
    scheme_file: Rc<RefCell<File>>,
    endpoint_name_to_id: HashMap<String, usize>,
    endpoints: HashMap<usize, Endpoint>,
    used_file_ids: BTreeSet<usize>,
}

impl AudioScheme {
    pub fn new(scheme_file: Rc<RefCell<File>>) -> Self {
        AudioScheme {
            scheme_file,
            endpoint_name_to_id: HashMap::new(),
            endpoints: HashMap::new(),
            used_file_ids: BTreeSet::new(),
        }
    }
}

impl SchemeMut for AudioScheme {
    fn open(&mut self, path: &[u8], flags: usize, _uid: u32, _gid: u32) -> Result<usize> {
        let path = str::from_utf8(path).or(Err(Error::new(EINVAL)))?;
        let (name, args) = {
            let mut iter = path.split('?');
            (iter.next().unwrap(), iter.next().ok_or(Error::new(EINVAL))?)
        };
        let args_iter = args.split('&').map(|key_equals_value| {
            let mut key_equals_value = key_equals_value.split('=');
            let key = key_equals_value.next();
            let value = key_equals_value.next();
            (key, value)
        });
        let mut args = HashMap::new();
        for (key, value) in args_iter {
            let (key, value) = (
                key.ok_or(Error::new(EINVAL))?,
                value.ok_or(Error::new(EINVAL))?,
            );
            if let Some(_) = args.insert(key, value) {
                // key was set twice
                return Err(Error::new(EINVAL));
            }
        }

        let file_id = gen_file_id(&mut self.used_file_ids);
        if flags & O_CREAT == O_CREAT {
            let buffer_size = args.get("buf_sz")
                .ok_or(Error::new(EINVAL))?
                .parse()
                .map_err(|_| Error::new(EINVAL))?;
            let mut endpoint = match flags & O_RDWR {
                O_RDONLY => Endpoint {
                    name: name.to_owned(),
                    buffer: AudioBuffer::new(buffer_size),
                    connections: BTreeSet::new(),
                    endpoint_type: EndpointType::Sink,
                    clock_source: None,
                },
                O_WRONLY => Endpoint {
                    name: name.to_owned(),
                    buffer: AudioBuffer::new(buffer_size),
                    connections: BTreeSet::new(),
                    endpoint_type: EndpointType::Source,
                    clock_source: None,
                },
                _ => return Err(Error::new(EINVAL)),
            };
            if let Some(conn_names) = args.get("connect") {
                let conn_names = conn_names.split(',');
                for conn_name in conn_names {
                    let conn_id = self.endpoint_name_to_id.get(conn_name).ok_or(Error::new(EINVAL))?;
                    endpoint.connections.insert(*conn_id);
                    self.endpoints.get_mut(conn_id).unwrap().connections.insert(file_id);
                }
            }
            self.endpoints.insert(file_id, endpoint);
            self.endpoint_name_to_id.insert(name.to_owned(), file_id);
        } else {
            return Err(Error::new(EINVAL));
        }
        Ok(file_id)
    }

    fn fevent(&mut self, id: usize, _flags: usize) -> Result<usize> {
        if self.used_file_ids.contains(&id) {
            Ok(id)
        } else {
            Err(Error::new(EBADF))
        }
    }

    fn fmap(&mut self, id: usize, offset: usize, size: usize) -> Result<usize> {
        if !self.used_file_ids.contains(&id) {
            return Err(Error::new(EBADF));
        }
        if offset != 0 {
            return Err(Error::new(EINVAL));
        }

        if let Some(endpoint) = self.endpoints.get(&id) {
            if size != endpoint.buffer.len() * 4 {
                return Err(Error::new(EINVAL));
            }
            return Ok(endpoint.buffer.as_ptr() as usize);
        } else {
            return Err(Error::new(EBADF));
        }
    }

    fn fsync(&mut self, id: usize) -> Result<usize> {
        let mut endpoint = self.endpoints.remove(&id).ok_or(Error::new(EBADF))?;
        match endpoint.endpoint_type {
            EndpointType::Source => for file_id in endpoint.connections.iter() {
                if self.endpoints.get(file_id).unwrap().clock_source == Some(id) {
                    self.scheme_file
                        .borrow_mut()
                        .write(&Packet {
                            id: 0,
                            pid: 0,
                            uid: 0,
                            gid: 0,
                            a: syscall::number::SYS_FEVENT,
                            b: *file_id,
                            c: syscall::EVENT_READ,
                            d: endpoint.buffer.len(),
                        })
                        .expect("failed to write to scheme file");
                }
            },
            EndpointType::Sink => {
                for val in endpoint.buffer.iter_mut() {
                    *val = 0;
                }
                // mixing time!
                for file_id in endpoint.connections.iter() {
                    let connected = self.endpoints.get_mut(&file_id).unwrap();
                    println!("mixing to {} from {} (whose clock source is {:?})", endpoint.name, connected.name, connected.clock_source);
                    for (idx, val) in endpoint.buffer.iter_mut().enumerate() {
                        *val += connected.buffer[idx] / 2;
                    }

                    if connected.clock_source == None {
                        connected.clock_source = Some(id);
                    }
                    if connected.clock_source == Some(id) {
                        self.scheme_file
                            .borrow_mut()
                            .write(&Packet {
                                id: 0,
                                pid: 0,
                                uid: 0,
                                gid: 0,
                                a: syscall::number::SYS_FEVENT,
                                b: *file_id,
                                c: syscall::EVENT_WRITE,
                                d: endpoint.buffer.len(),
                            })
                            .expect("failed to write to scheme file");
                    }
                }
            }
        }
        self.endpoints.insert(id, endpoint);
        Ok(0)
    }

    fn close(&mut self, id: usize) -> Result<usize> {
        if !self.used_file_ids.remove(&id) {
            return Err(Error::new(EBADF));
        }
        if let Some(endpoint) = self.endpoints.remove(&id) {
            self.endpoint_name_to_id.remove(&endpoint.name);
            for conn_id in endpoint.connections {
                self.scheme_file
                    .borrow_mut()
                    .write(&Packet {
                        id: 0,
                        pid: 0,
                        uid: 0,
                        gid: 0,
                        a: syscall::number::SYS_FEVENT,
                        b: conn_id,
                        c: syscall::EVENT_WRITE,
                        d: 0, // zero-size buffer, endpoint is closed
                    })
                    .expect("failed to write to scheme file");
            }
            return Ok(0);
        }

        panic!("file id marked as used but wasn't");
    }
}

fn gen_file_id(used_ids: &mut BTreeSet<usize>) -> usize {
    loop {
        let id = random();
        if !used_ids.contains(&id) {
            used_ids.insert(id);
            return id;
        }
    }
}
