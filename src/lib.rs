extern crate libc;

use std::io;
use std::mem;
use std::os::unix::ffi::OsStringExt;
use std::slice;

#[derive(Copy, Clone)]
pub enum ReadState {
    ReadOnly,
    WriteOnly,
    ReadWrite,
}

impl ReadState {
    fn flag(&self) -> u32 {
        match self {
            ReadState::ReadOnly => mdbm_sys::MDBM_O_RDONLY,
            ReadState::WriteOnly => mdbm_sys::MDBM_O_WRONLY,
            ReadState::ReadWrite => mdbm_sys::MDBM_O_RDWR,
        }
    }
}

#[derive(Copy, Clone)]
pub enum HashFunction {
    CRC32,
    EJB,
    // FNV32, the default
    FNV,
    // Hsieh SuperFast
    HSIEH,
    JENKINS,
    MAX,
    MD5,
    OZ,
    PHONG,
    SHA1,
    STL,
    TOREK,
}

impl HashFunction {
    fn hash_constant(&self) -> u32 {
        match self {
            HashFunction::CRC32 => mdbm_sys::MDBM_HASH_CRC32,
            HashFunction::EJB => mdbm_sys::MDBM_HASH_EJB,
            HashFunction::FNV => mdbm_sys::MDBM_HASH_FNV,
            HashFunction::HSIEH => mdbm_sys::MDBM_HASH_HSIEH,
            HashFunction::JENKINS => mdbm_sys::MDBM_HASH_JENKINS,
            HashFunction::MAX => mdbm_sys::MDBM_HASH_MAX,
            HashFunction::MD5 => mdbm_sys::MDBM_HASH_MD5,
            HashFunction::OZ => mdbm_sys::MDBM_HASH_OZ,
            HashFunction::PHONG => mdbm_sys::MDBM_HASH_PHONG,
            HashFunction::SHA1 => mdbm_sys::MDBM_HASH_SHA_1,
            HashFunction::STL => mdbm_sys::MDBM_HASH_STL,
            HashFunction::TOREK => mdbm_sys::MDBM_HASH_TOREK,
        }
    }
}

#[derive(Copy, Clone)]
pub struct Options {
    pub reads: ReadState,
    pub create: bool,
    pub hash: Option<HashFunction>,
}

impl<'a> Into<u32> for Options {
    fn into(self) -> u32 {
        let f = self.reads.flag();
        if !self.create {
            return f;
        }

        return f | mdbm_sys::MDBM_O_CREAT;
    }
}

impl Default for Options {
    fn default() -> Options {
        Options {
            reads: ReadState::ReadWrite,
            create: true,
            hash: None,
        }
    }
}

pub struct MDBM {
    db: *mut mdbm_sys::MDBM,
}

impl MDBM {
    /// Open a database.
    ///
    pub fn new<P: Into<std::path::PathBuf>>(
        path: P,
        options: Options,
        mode: usize,
        psize: usize,
        presize: usize,
    ) -> Result<MDBM, io::Error> {
        // Rust Path objects are not null-terminated.
        // To null-terminate it, we need to:

        // 1. Take ownership of it, so we can modify the underlying buf.
        //   - This may or may not copy, depending on what was passed in.
        let path_buf = path.into();
        // 2. Treat the string as a Unix string (i.e. assume Unix utf8 encoding)
        //   - This should be a no-op
        let path_bytes = path_buf.into_os_string();
        // 3. Treat it as a vector of bytes
        //   - This should be a no-op
        let path_vec: Vec<u8> = path_bytes.into_vec();
        // 4. Append a null byte
        let path_cstring = std::ffi::CString::new(path_vec)?;

        let flag_u32: u32 = options.into();

        unsafe {
            let db = mdbm_sys::mdbm_open(
                path_cstring.into_raw(),
                flag_u32 as libc::c_int,
                mode as libc::c_int,
                psize as libc::c_int,
                presize as libc::c_int,
            );

            if db.is_null() {
                return Err(io::Error::last_os_error());
            }
            match options.hash {
                None => {}
                Some(h) => {
                    mdbm_sys::mdbm_set_hash(db, h.hash_constant() as libc::c_int);
                }
            };
            Ok(MDBM { db: db })
        }
    }

    /// Set a key.
    pub fn set<'k, 'v, K, V>(&self, key: &'k K, value: &'v V, flags: isize) -> Result<(), io::Error>
    where
        K: AsDatum<'k> + ?Sized,
        V: AsDatum<'v> + ?Sized,
    {
        unsafe {
            let rc = mdbm_sys::mdbm_store(
                self.db,
                to_raw_datum(&key.as_datum()),
                to_raw_datum(&value.as_datum()),
                flags as libc::c_int,
            );

            if rc == -1 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        }
    }

    /// Lock a key.
    pub fn lock<'a, K>(&'a self, key: &'a K, flags: isize) -> Result<Lock<'a>, io::Error>
    where
        K: AsDatum<'a> + ?Sized,
    {
        let rc = unsafe {
            mdbm_sys::mdbm_lock_smart(
                self.db,
                &to_raw_datum(&key.as_datum()),
                flags as libc::c_int,
            )
        };

        if rc == 1 {
            Ok(Lock {
                db: self,
                key: key.as_datum(),
            })
        } else {
            Err(io::Error::last_os_error())
        }
    }
}

impl Drop for MDBM {
    fn drop(&mut self) {
        unsafe {
            mdbm_sys::mdbm_sync(self.db);
            mdbm_sys::mdbm_close(self.db);
        }
    }
}

pub struct Datum<'a> {
    bytes: &'a [u8],
}

impl<'a> Datum<'a> {
    pub fn new(bytes: &'a [u8]) -> Datum<'a> {
        Datum { bytes: bytes }
    }
}

pub trait AsDatum<'a> {
    fn as_datum(&'a self) -> Datum<'a>;
}

impl<'a, T: AsDatum<'a> + ?Sized> AsDatum<'a> for &'a T {
    fn as_datum(&'a self) -> Datum<'a> {
        (**self).as_datum()
    }
}

impl<'a> AsDatum<'a> for [u8] {
    fn as_datum(&'a self) -> Datum<'a> {
        Datum::new(self)
    }
}

impl<'a> AsDatum<'a> for str {
    fn as_datum(&'a self) -> Datum<'a> {
        self.as_bytes().as_datum()
    }
}

fn to_raw_datum(datum: &Datum) -> mdbm_sys::datum {
    mdbm_sys::datum {
        dptr: datum.bytes.as_ptr() as *mut _,
        dsize: datum.bytes.len() as libc::c_int,
    }
}

pub struct Lock<'a> {
    db: &'a MDBM,
    key: Datum<'a>,
}

impl<'a> Lock<'a> {
    /// Fetch a key.
    pub fn get(&'a self) -> Option<&'a [u8]> {
        unsafe {
            let value = mdbm_sys::mdbm_fetch(self.db.db, to_raw_datum(&self.key));

            if value.dptr.is_null() {
                None
            } else {
                // Cast pointer from signed char (c) to unsigned char (rust)
                let u8_ptr: *const u8 = mem::transmute::<*mut i8, *const u8>(value.dptr);
                Some(slice::from_raw_parts(u8_ptr, value.dsize as usize))
            }
        }
    }
}

impl<'a> Drop for Lock<'a> {
    fn drop(&mut self) {
        unsafe {
            let rc = mdbm_sys::mdbm_unlock_smart(self.db.db, &to_raw_datum(&self.key), 0);

            assert_eq!(rc, 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::MDBM;
    use std::fs::remove_file;
    use std::path::Path;
    use std::str;

    #[test]
    fn test_set_get() {
        let path = Path::new("test.db");
        let db = MDBM::new(&path, Default::default(), 0o644, 0, 0).unwrap();

        db.set(&"hello", &"world", 0).unwrap();

        // key needs to be an lvalue so the lock can hold a reference to
        // it.
        let key = "hello";

        // Lock the key. RIAA will unlock it when we exit this scope.
        let value = db.lock(&key, 0).unwrap();

        // Convert the value into a string. The lock is still live at this
        // point.
        let value = str::from_utf8(value.get().unwrap()).unwrap();
        assert_eq!(value, "world");
        println!("hello: {}", value);

        let _ = remove_file(path);
    }

    #[test]
    fn test_read_only() {
        let path = Path::new("test_rw.db");
        let mut opts: super::Options = Default::default();
        opts.reads = super::ReadState::WriteOnly;
        opts.hash = Some(super::HashFunction::JENKINS);

        let db = MDBM::new(&path, opts, 0o644, 0, 0).unwrap();

        db.set(&"hello", &"world", 0).unwrap();

        //// Strangely enough, this doesn't fail
        // let err = db.lock(&"hello", 0);

        // match err {
        //     Ok(value) => assert!(
        //         false,
        //         "WriteOnly should error on read, instead got {}",
        //         value
        //             .get()
        //             .and_then(|v| Some(str::from_utf8(v).unwrap_or("utf8 failure")))
        //             .unwrap_or("none"),
        //     ),
        //     Err(_) => assert!(true),
        // }

        opts.reads = super::ReadState::ReadOnly;
        let db = MDBM::new(&path, opts, 0o644, 0, 0).unwrap();
        let err = db.set(&"another", &"world", 0);
        match err {
            Ok(_) => assert!(false, "ReadOnly should error on Write"),
            Err(_) => assert!(true),
        }

        // key needs to be an lvalue so the lock can hold a reference to
        // it.
        let key = "hello";

        // Lock the key. RIAA will unlock it when we exit this scope.
        let value = db.lock(&key, 0).unwrap();

        // Convert the value into a string. The lock is still live at this
        // point.
        let value = str::from_utf8(value.get().unwrap()).unwrap();
        assert_eq!(value, "world");
        println!("hello: {}", value);

        let _ = remove_file(path);
    }

    // Tests that should fail to compile

    /*
    #[test]
    fn test_keys_cannot_escape() {
        let db = MDBM::new(
            &Path::new("test.db"),
            super::MDBM_O_RDWR | super::MDBM_O_CREAT,
            0o644,
            0,
            0
        ).unwrap();

        db.set(&"hello", &"world", 0).unwrap();

        let _ = {
            let key = vec![1];
            db.lock(&key.as_slice(), 0).unwrap()
        };
    }
    */

    /*
    #[test]
    fn test_values_cannot_escape() {
        let db = MDBM::new(
            &Path::new("test.db"),
            super::MDBM_O_RDWR | super::MDBM_O_CREAT,
            0o644,
            0,
            0,
        )
        .unwrap();

        let _ = {
            db.set(&"hello", &"world", 0).unwrap();

            let key = "hello";
            let value = db.lock(&key, 0).unwrap();
            str::from_utf8(value.get().unwrap()).unwrap()
        };
    }
    */

    /*
    #[test]
    fn test_values_cannot_escape_database() {
        let _ = {
            let db = MDBM::new(
                &Path::new("test.db"),
                super::MDBM_O_RDWR | super::MDBM_O_CREAT,
                0o644,
                0,
                0,
            )
            .unwrap();

            db.set(&"hello", &"world", 0).unwrap();

            let key = "hello";
            let value = db.lock(&key, 0).unwrap();
            str::from_utf8(value.get().unwrap()).unwrap()
        };
    }
    */
}
