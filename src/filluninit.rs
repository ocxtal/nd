// @file filluninit.rs
// @author Hajime Suzuki

use std::io::{Error, ErrorKind, Result};

pub trait FillUninit {
    fn fill_uninit_with_ret<T, F>(&mut self, len: usize, f: F) -> Result<(T, usize)>
    where
        T: Sized,
        F: FnMut(&mut [u8]) -> Result<(T, usize)>;

    fn fill_uninit_on_option_with_ret<T, F>(&mut self, len: usize, f: F) -> Option<(T, usize)>
    where
        T: Sized,
        F: FnMut(&mut [u8]) -> Option<(T, usize)>,
    {
        let mut f = f;
        self.fill_uninit_with_ret(len, |buf| f(buf).ok_or_else(|| Error::from(ErrorKind::Other)))
            .ok()
    }

    fn fill_uninit<F>(&mut self, len: usize, f: F) -> Result<usize>
    where
        F: FnMut(&mut [u8]) -> Result<usize>,
    {
        let mut f = f;
        self.fill_uninit_with_ret(len, |buf| f(buf).map(|len| ((), len)))
            .map(|(_, len)| len)
    }
}

impl FillUninit for Vec<u8> {
    fn fill_uninit_with_ret<T, F>(&mut self, len: usize, f: F) -> Result<(T, usize)>
    where
        T: Sized,
        F: FnMut(&mut [u8]) -> Result<(T, usize)>,
    {
        let mut f = f;

        if self.capacity() < self.len() + len {
            let shift = (self.len() + len).leading_zeros() as usize;
            debug_assert!(shift > 0);

            let new_len = 0x8000000000000000 >> (shift.min(56) - 1);
            self.reserve(new_len - self.len());
        }

        // reserve buffer and call the function
        let arr = self.spare_capacity_mut();
        let arr = unsafe { std::mem::transmute::<&mut [std::mem::MaybeUninit<u8>], &mut [u8]>(arr) };
        let ret = f(&mut arr[..len]);

        // truncate the buffer
        let clip = match ret {
            Ok((_, clip)) => clip,
            _ => 0,
        };
        unsafe { self.set_len(self.len() + clip) };

        ret
    }
}

// end of filluninit.rs
