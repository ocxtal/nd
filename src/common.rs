// @file common.rs
// @author Hajime Suzuki
// @brief formatter implementations

pub const BLOCK_SIZE: usize = 2 * 1024 * 1024;

#[derive(Copy, Clone, Debug)]
pub struct InoutFormat {
    pub offset: Option<u8>, // in {'b', 'd', 'x'}
    pub length: Option<u8>, // in {'b', 'd', 'x'}
    pub body: Option<u8>,   // in {'b', 'd', 'x', 'a'}
}

pub trait ReadBlock {
    fn read_block(&mut self, buf: &mut Vec<u8>) -> Option<usize>;
}

pub trait DumpBlock {
    fn dump_block(&mut self) -> Option<usize>;
}

pub trait ExtendUninit {
    fn extend_uninit<T, F>(&mut self, len: usize, f: F) -> Option<T>
    where
        T: Sized,
        F: FnMut(&mut [u8]) -> Option<(T, usize)>;
}

impl ExtendUninit for Vec<u8> {
    fn extend_uninit<T, F>(&mut self, len: usize, f: F) -> Option<T>
    where
        T: Sized,
        F: FnMut(&mut [u8]) -> Option<(T, usize)>,
    {
        let mut f = f;

        if self.capacity() < self.len() + len {
            let shift = (self.len() + len).leading_zeros() as usize;
            debug_assert!(shift > 0);

            let new_len = 0x8000000000000000 >> (shift.min(56) - 1);
            self.reserve(new_len - self.len());
        }

        let arr = self.spare_capacity_mut();
        let arr = unsafe { std::mem::transmute::<&mut [std::mem::MaybeUninit<u8>], &mut [u8]>(arr) };
        let ret = f(&mut arr[..len]);
        let clip = match ret {
            Some((_, clip)) => clip,
            None => 0,
        };
        unsafe { self.set_len(self.len() + clip) };

        match ret {
            Some((ret, _)) => Some(ret),
            None => None,
        }
    }
}

// end of common.rs
