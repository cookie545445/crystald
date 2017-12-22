use std::collections::{HashMap, BTreeMap, BTreeSet};
use std::rc::Rc;
use std::cell::RefCell;
use std::fs::File;
use std::io::Write;

use syscall;
use syscall::scheme::SchemeMut;
use syscall::data::Packet;
use syscall::flag::*;
use syscall::error::*;
use rand::random;

use std::str;

struct Endpoint {
    file_id: usize,
    buffer: Vec<i32>,
    connections: BTreeSet<usize>,
    endpoint_type: EndpointType,
}

enum EndpointType {
    Source,
    Sink { input_buffers: BTreeMap<usize, Vec<i32>> },
}

pub struct AudioScheme {
    scheme_file: Rc<RefCell<File>>,
    endpoint_ids_to_name: BTreeMap<usize, String>,
    endpoints: HashMap<String, Endpoint>,
    connections: BTreeMap<usize, String>,
    used_file_ids: BTreeSet<usize>,
}

impl AudioScheme {
    pub fn new(scheme_file: Rc<RefCell<File>>) -> Self {
        AudioScheme {
            scheme_file,
            endpoint_ids_to_name: BTreeMap::new(),
            endpoints: HashMap::new(),
            connections: BTreeMap::new(),
            used_file_ids: BTreeSet::new(),
        }
    }
}

impl SchemeMut for AudioScheme {
    fn open(&mut self, path: &[u8], flags: usize, uid: u32, gid: u32) -> Result<usize> {
        let path = str::from_utf8(path).or(Err(Error::new(EINVAL)))?;
        let (name, args) = {
            let mut iter = path.split('?');
            (iter.next().unwrap(), iter.nth(1).ok_or(Error::new(EINVAL))?)
        };
        let args_iter = args.split('&')
            .map(|key_equals_value| {
                     let mut key_equals_value = key_equals_value.split('=');
                     let key = key_equals_value.next();
                     let value = key_equals_value.next();
                     (key, value)
                 });
        let mut args = HashMap::new();
        for (key, value) in args_iter {
            let (key, value) = (key.ok_or(Error::new(EINVAL))?, value.ok_or(Error::new(EINVAL))?);
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
            self.endpoints
                .insert(name.to_owned(), match flags & O_RDWR {
                    O_RDONLY => {
                        Endpoint {
                            file_id,
                            buffer: vec![0; buffer_size],
                            connections: BTreeSet::new(),
                            endpoint_type: EndpointType::Sink { input_buffers: BTreeMap::new() },
                        }
                    },
                    O_WRONLY => {
                        Endpoint {
                            file_id,
                            buffer: vec![0; buffer_size],
                            connections: BTreeSet::new(),
                            endpoint_type: EndpointType::Source,
                        }
                    },
                    _ => return Err(Error::new(EINVAL)),
                });
            self.endpoint_ids_to_name.insert(file_id, name.to_owned());
        } else {
            self.endpoints.get_mut(name).ok_or(Error::new(ENOENT))?.connections.insert(file_id);
            self.connections.insert(file_id, name.to_owned());
        }
        Ok(file_id)
    }

    fn fevent(&mut self, id: usize, flags: usize) -> Result<usize> {
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

        if let Some(name) = self.endpoint_ids_to_name.get(&id) {
            let endpoint = self.endpoints.get(name).unwrap();
            if size != endpoint.buffer.len() {
                return Err(Error::new(EINVAL));
            }
            return Ok(endpoint.buffer.as_slice().as_ptr() as usize);
        } else if let Some(connected_name) = self.connections.get(&id) {
            let endpoint = self.endpoints.get(connected_name).unwrap();
            if size != endpoint.buffer.len() {
                return Err(Error::new(EINVAL));
            }

            match endpoint.endpoint_type {
                EndpointType::Sink { ref input_buffers } => {
                    return Ok(input_buffers.get(&id).unwrap().as_slice().as_ptr() as usize);
                },
                EndpointType::Source => {
                    return Ok(endpoint.buffer.as_slice().as_ptr() as usize);
                },
            }
        } else {
            return Err(Error::new(EBADF));
        }
    }

    fn fsync(&mut self, id: usize) -> Result<usize> {
        let endpoint = {
            let name = self.endpoint_ids_to_name.get(&id).ok_or(Error::new(EBADF))?;
            self.endpoints.get_mut(name).unwrap()
        };
        match &endpoint.endpoint_type {
            &EndpointType::Source => {
                for file_id in endpoint.connections.iter() {
                    self.scheme_file.borrow_mut().write(&Packet {
                        id: 0,
                        pid: 0,
                        uid: 0,
                        gid: 0,
                        a: syscall::number::SYS_FEVENT,
                        b: *file_id,
                        c: syscall::EVENT_READ,
                        d: endpoint.buffer.len(),
                    }).expect("failed to write to scheme file");
                }
            },
            &EndpointType::Sink { ref input_buffers } => {
                // mixing time!
                for buffer in input_buffers.values() {
                    for (idx, val) in endpoint.buffer.iter_mut().enumerate() {
                        *val += buffer[idx] / 2;
                    }
                }

                for file_id in endpoint.connections.iter() {
                    self.scheme_file.borrow_mut().write(&Packet {
                        id: 0,
                        pid: 0,
                        uid: 0,
                        gid: 0,
                        a: syscall::number::SYS_FEVENT,
                        b: *file_id,
                        c: syscall::EVENT_WRITE,
                        d: endpoint.buffer.len(),
                    }).expect("failed to write to scheme file");
                }
            },
        }
        Ok(0)
    }

    fn close(&mut self, id: usize) -> Result<usize> {
        if !self.used_file_ids.remove(&id) {
            return Err(Error::new(EBADF));
        }
        if let Some(name) = self.endpoint_ids_to_name.remove(&id) {
            let endpoint = self.endpoints.remove(&name).unwrap();
            for conn_id in endpoint.connections {
                self.scheme_file.borrow_mut().write(&Packet {
                    id: 0,
                    pid: 0,
                    uid: 0,
                    gid: 0,
                    a: syscall::number::SYS_FEVENT,
                    b: conn_id,
                    c: syscall::EVENT_WRITE,
                    d: 0,   // zero-size buffer, endpoint is closed
                }).expect("failed to write to scheme file");
                self.connections.remove(&conn_id);
            }
            return Ok(0);
        }

        if let Some(name) = self.connections.remove(&id) {
            let endpoint = self.endpoints.get_mut(&name).unwrap();
            endpoint.connections.remove(&id);
            if let EndpointType::Sink { ref mut input_buffers } = endpoint.endpoint_type {
                input_buffers.remove(&id);
            }
            return Ok(0);
        }

        unreachable!(); // theoretically
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
