extern crate sodiumoxide;
extern crate libc;
extern crate libsodium_sys as ffi;

#[macro_use]
mod macros;
pub mod parse_args;
pub mod generichash;

use generichash::*;
use sodiumoxide::crypto::sign::{gen_keypair, SECRETKEYBYTES, PUBLICKEYBYTES, SIGNATUREBYTES};
use sodiumoxide::crypto::pwhash::{OpsLimit, MemLimit, OPSLIMIT_SENSITIVE, MEMLIMIT_SENSITIVE,
                                  SALTBYTES};
use sodiumoxide::randombytes::*;

use std::fmt::{Debug, Error, Formatter};
use std::mem;
use std::io::Cursor;
use std::io::Read;

pub const KEYNUMBYTES: usize = 8;
pub const TWOBYTES: usize = 2;
pub const TR_COMMENT_PREFIX_LEN: usize = 17;
pub const PASSWORDMAXBYTES: usize = 1024;
pub const COMMENTBYTES: usize = 1024;
pub const TRUSTEDCOMMENTMAXBYTES: usize = 8192;
pub const SIGALG: &'static str = "Ed";
pub const SIGALG_HASHED: &'static str = "ED";
pub const KDFALG: &'static str = "Sc";
pub const CHKALG: &'static str = "B2";
pub const COMMENT_PREFIX: &'static str = "untrusted comment: ";
pub const DEFAULT_COMMENT: &'static str = "signature from rsign secret key";
pub const SECRETKEY_DEFAULT_COMMENT: &'static str = "rsign encrypted secret key";
pub const TRUSTED_COMMENT_PREFIX: &'static str = "trusted comment: ";
pub const SIG_DEFAULT_CONFIG_DIR: &'static str = ".rsign";
pub const SIG_DEFAULT_CONFIG_DIR_ENV_VAR: &'static str = "MINISIGN_CONFIG_DIR";
pub const SIG_DEFAULT_PKFILE: &'static str = "rsign.pub";
pub const SIG_DEFAULT_SKFILE: &'static str = "rsign.key";
pub const SIG_SUFFIX: &'static str = ".rsign";
pub const VERSION_STRING: &'static str = "rsign 0.1";


#[derive(Debug, Default, Clone)]
pub struct KeynumSK {
    pub keynum: Vec<u8>,
    pub sk: Vec<u8>,
    pub chk: Vec<u8>,
}
impl KeynumSK {
    pub fn len(&self) -> usize {
        self.keynum.len() + self.sk.len() + self.chk.len()
    }
}


#[derive(Clone)]
pub struct SeckeyStruct {
    pub sig_alg: Vec<u8>,
    pub kdf_alg: Vec<u8>,
    pub chk_alg: Vec<u8>,
    pub kdf_salt: Vec<u8>,
    pub kdf_opslimit_le: OpsLimit,
    pub kdf_memlimit_le: MemLimit,
    pub keynum_sk: KeynumSK,
}
impl AsRef<[u8]> for SeckeyStruct {
    fn as_ref(&self) -> &[u8] {
        self.sig_alg.as_ref()
    }
}
impl SeckeyStruct {
    pub fn len(self) -> usize {
        mem::size_of_val(&self)
    }
    pub fn from(bytes_buf: &[u8]) -> Result<SeckeyStruct, ()> {
        let sk = SeckeyStruct {
            sig_alg: bytes_buf[..2].to_vec(),
            kdf_alg: bytes_buf[2..4].to_vec(),
            chk_alg: bytes_buf[4..6].to_vec(),
            kdf_salt: bytes_buf[6..38].to_vec(),
            kdf_opslimit_le: OpsLimit(load_usize_le(&bytes_buf[38..46])),
            kdf_memlimit_le: MemLimit(load_usize_le(&bytes_buf[46..54])),
            keynum_sk: KeynumSK {
                keynum: bytes_buf[54..62].to_vec(),
                sk: bytes_buf[62..126].to_vec(),
                chk: bytes_buf[126..].to_vec(),
            },
        };
        Ok(sk)
    }
    pub fn bytes(&self) -> Vec<u8> {
        let OpsLimit(op_lim) = self.kdf_opslimit_le;
        let opslim_arr = store_usize_le(op_lim);
        let MemLimit(mem_lim) = self.kdf_memlimit_le;
        let memlim_arr = store_usize_le(mem_lim);
        let mut opslim_vec = Vec::new();
        let mut memlim_vec = Vec::new();
        opslim_vec.extend_from_slice(&opslim_arr[..]);
        memlim_vec.extend_from_slice(&memlim_arr[..]);

        let mut iters = Vec::new();
        iters.push(self.sig_alg.iter());
        iters.push(self.kdf_alg.iter());
        iters.push(self.chk_alg.iter());
        iters.push(self.kdf_salt.iter());
        iters.push(opslim_vec.iter());
        iters.push(memlim_vec.iter());
        iters.push(self.keynum_sk.keynum.iter());
        iters.push(self.keynum_sk.sk.iter());
        iters.push(self.keynum_sk.chk.iter());
        let v: Vec<u8> = iters
            .iter()
            .flat_map(|b| {
                          let b = b.clone();
                          b.into_iter().cloned()
                      })
            .collect();
        v
    }
    pub fn checksum(&mut self) {
        let state_sz = unsafe { ffi::crypto_generichash_statebytes() };
        let mut state: Vec<u8> = vec![0;state_sz];
        let ptr_state = state.as_mut_ptr() as *mut ffi::crypto_generichash_state;
        generichash::init(ptr_state).unwrap();
        generichash::update(ptr_state, self.sig_alg.as_ref()).unwrap();
        generichash::update(ptr_state, self.keynum_sk.keynum.as_ref()).unwrap();
        generichash::update(ptr_state, self.keynum_sk.sk.as_ref()).unwrap();
        let h = generichash::finalize(ptr_state).unwrap();
        self.keynum_sk.chk = h.as_ref().to_vec();
    }
    pub fn xor_keynum(&mut self, mut stream: Vec<u8>) {

        let b8 = self.keynum_sk
            .keynum
            .iter_mut()
            .zip(stream.iter())
            .map(|(byte, stream)| *byte = *byte ^ *stream)
            .count();

        let b64 = self.keynum_sk
            .sk
            .iter_mut()
            .zip(stream[b8..].iter())
            .map(|(byte, stream)| *byte = *byte ^ *stream)
            .count();

        let _b32 = self.keynum_sk
            .chk
            .iter_mut()
            .zip(stream[b8 + b64..].iter())
            .map(|(byte, stream)| *byte = *byte ^ *stream)
            .count();

        sodiumoxide::utils::memzero(&mut stream);
    }
}



impl Debug for SeckeyStruct {
    fn fmt(&self, f: &mut Formatter) -> Result<(), Error> {
        let OpsLimit(opl) = self.kdf_opslimit_le;
        let MemLimit(meml) = self.kdf_memlimit_le;
        write!(f,
               "{:?} {:?} {:?} {:?} {:?} {:?} {:?}",
               self.sig_alg,
               self.kdf_alg,
               self.chk_alg,
               self.kdf_salt,
               opl,
               meml,
               self.keynum_sk)
    }
}

#[derive(Debug)]
pub struct PubkeyStruct {
    pub sig_alg: [u8;2],
    pub keynum_pk: KeynumPK,
}
#[derive(Debug, Clone)]
pub struct KeynumPK {
    pub keynum: [u8;KEYNUMBYTES],
    pub pk: [u8;PUBLICKEYBYTES],
}
impl PubkeyStruct {
    
    pub fn from(buf: &[u8]) -> Result<PubkeyStruct, std::io::Error> {
        let mut buf = Cursor::new(buf);
        let mut sig_alg = [0u8;2];
        let mut keynum = [0u8;KEYNUMBYTES];
        let mut pk = [0u8;PUBLICKEYBYTES];
        buf.read(&mut sig_alg)?;
        buf.read(&mut keynum)?;
        buf.read(&mut pk)?;
        Ok(PubkeyStruct {
            sig_alg: sig_alg,
            keynum_pk: KeynumPK {
                keynum: keynum,
                pk: pk,
            },
        })
    }

    pub fn bytes(&self) -> Vec<u8> {
        let mut iters = Vec::new();
        iters.push(self.sig_alg.iter());
        iters.push(self.keynum_pk.keynum.iter());
        iters.push(self.keynum_pk.pk.iter());
        let v: Vec<u8> = iters
            .iter()
            .flat_map(|b| {
                          let b = b.clone();
                          b.into_iter().cloned()
                      })
            .collect();
        v
    }
}

#[derive(Debug)]
pub struct SigStruct {
    pub sig_alg: Vec<u8>,
    pub keynum: Vec<u8>,
    pub sig: Vec<u8>,
}
impl SigStruct {
    pub fn bytes(&self) -> Vec<u8> {
        let mut iters = Vec::new();
        iters.push(self.sig_alg.iter());
        iters.push(self.keynum.iter());
        iters.push(self.sig.iter());
        let v: Vec<u8> = iters
            .iter()
            .flat_map(|b| {
                          let b = b.clone();
                          b.into_iter().cloned()
                      })
            .collect();
        v
    }
    pub fn from(bytes_buf: &[u8]) -> Result<SigStruct, ()> {
        Ok(SigStruct {
            sig_alg: bytes_buf[..2].to_vec(),
            keynum: bytes_buf[2..10].to_vec(),
            sig: bytes_buf[10..74].to_vec(),   
        })
    }
}

impl Default for SigStruct {
    fn default() -> Self {
        SigStruct {
            sig_alg: vec![0u8;2],
            keynum: vec![0u8;8],
            sig: vec![0u8;64],
        }
    }
}

pub fn gen_keystruct() -> (PubkeyStruct, SeckeyStruct) {
    let (pk, sk) = gen_keypair();
    let mut pk_vec = [0u8;PUBLICKEYBYTES];
    let mut sk_vec = Vec::with_capacity(SECRETKEYBYTES);
    let keynum_vec = randombytes(KEYNUMBYTES);
    let mut keynum = [0u8;KEYNUMBYTES];
    keynum.copy_from_slice(keynum_vec.as_slice());
    pk_vec.copy_from_slice(&pk[..]);
    sk_vec.extend_from_slice(&sk[..]);
    let mut sig_alg = [0u8;2];
    sig_alg.copy_from_slice(&SIGALG.as_bytes()[..]);
    let p_struct = PubkeyStruct {
        sig_alg: sig_alg,
        keynum_pk: KeynumPK {
            keynum: keynum,
            pk: pk_vec,
        },
    };
    let s_struct = SeckeyStruct {
        sig_alg: SIGALG.bytes().collect(),
        kdf_alg: KDFALG.bytes().collect(),
        chk_alg: CHKALG.bytes().collect(),
        kdf_salt: randombytes(SALTBYTES),
        kdf_opslimit_le: OPSLIMIT_SENSITIVE,
        kdf_memlimit_le: MEMLIMIT_SENSITIVE,
        keynum_sk: KeynumSK {
            keynum: keynum_vec,
            sk: sk_vec,
            chk: Vec::with_capacity(BYTES),
        },
    };
    (p_struct, s_struct)
}

pub fn store_usize_le(x: usize) -> [u8; 8] {
    let b1: u8 = (x & 0xff) as u8;
    let b2: u8 = ((x >> 8) & 0xff) as u8;
    let b3: u8 = ((x >> 16) & 0xff) as u8;
    let b4: u8 = ((x >> 24) & 0xff) as u8;
    let b5: u8 = ((x >> 32) & 0xff) as u8;
    let b6: u8 = ((x >> 40) & 0xff) as u8;
    let b7: u8 = ((x >> 48) & 0xff) as u8;
    let b8: u8 = ((x >> 56) & 0xff) as u8;
    return [b1, b2, b3, b4, b5, b6, b7, b8];
}

pub fn load_usize_le(x: &[u8]) -> usize {
    (x[0] as usize) | (x[1] as usize) << 8 | (x[2] as usize) << 16 | (x[3] as usize) << 24 |
    (x[4] as usize) << 32 | (x[5] as usize) << 40 |
    (x[6] as usize) << 48 | (x[7] as usize) << 56
}
