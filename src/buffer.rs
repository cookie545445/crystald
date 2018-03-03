use std::{mem, slice};
use std::ops::{Drop, Deref, DerefMut};

use syscall::call::{physalloc, physfree, physmap};
use syscall::flag::MAP_WRITE;

pub struct AudioBuffer<T: 'static> {
    pub phys_addr: usize,
    ptr: *mut T,
    size: usize,
}

impl<'a, T> AudioBuffer<T>
where
    T: From<u8> + 'static,
{
    pub fn new(size: usize) -> Self {
        let size_bytes = size * mem::size_of::<T>();
        let phys_addr;
        let ptr;
        let buf;
        unsafe {
            phys_addr = physalloc(size_bytes).unwrap();
            ptr = physmap(phys_addr, size_bytes, MAP_WRITE).unwrap() as *mut _;
            println!("mapped allocated buffer: {:#010X}", phys_addr);
            buf = slice::from_raw_parts_mut(ptr, size);
        }
        AudioBuffer { phys_addr, ptr, size }
    }
}

impl<'a, T> Drop for AudioBuffer<T> {
    fn drop(&mut self) {
        unsafe { physfree(self.phys_addr, self.size * mem::size_of::<T>()).unwrap() };
    }
}

impl<'a, T> Deref for AudioBuffer<T> {
    type Target = [T];
    fn deref(&self) -> &Self::Target {
        unsafe { slice::from_raw_parts(self.ptr, self.size) }
    }
}

impl<'a, T> DerefMut for AudioBuffer<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { slice::from_raw_parts_mut(self.ptr, self.size) }
    }
}
