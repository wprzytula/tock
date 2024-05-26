use core::cell::Cell;
use kernel::hil::symmetric_encryption;
use kernel::utilities::cells::{OptionalCell, TakeCell};
use kernel::ErrorCode;

#[derive(Copy, Clone, Debug)]
enum AESMode {
    ECB,
    CTR,
    CBC,
    CCM,
}

pub struct AesECB<'a> {
    mode: Cell<AESMode>,
    client: OptionalCell<&'a dyn kernel::hil::symmetric_encryption::Client<'a>>,
    /// Input either plaintext or ciphertext to be encrypted or decrypted.
    input: TakeCell<'static, [u8]>,
    output: TakeCell<'static, [u8]>,
    current_idx: Cell<usize>,
    start_idx: Cell<usize>,
    end_idx: Cell<usize>,
}

impl<'a> AesECB<'a> {
    pub fn new() -> AesECB<'a> {
        AesECB {
            mode: Cell::new(AESMode::CTR),
            client: OptionalCell::empty(),
            input: TakeCell::empty(),
            output: TakeCell::empty(),
            current_idx: Cell::new(0),
            start_idx: Cell::new(0),
            end_idx: Cell::new(0),
        }
    }
}

impl<'a> kernel::hil::symmetric_encryption::AES128<'a> for AesECB<'a> {
    fn enable(&self) {
        // self.set_dma();
    }

    fn disable(&self) {
        // self.registers.task_stopecb.write(Task::ENABLE::CLEAR);
        // self.disable_interrupts();
    }

    fn set_client(&'a self, client: &'a dyn symmetric_encryption::Client<'a>) {
        self.client.set(client);
    }

    fn set_key(&self, key: &[u8]) -> Result<(), ErrorCode> {
        if key.len() != symmetric_encryption::AES128_KEY_SIZE {
            Err(ErrorCode::INVAL)
        } else {
            for (i, c) in key.iter().enumerate() {
                unsafe {
                    // ECB_DATA[i] = *c;
                }
            }
            Ok(())
        }
    }

    fn set_iv(&self, iv: &[u8]) -> Result<(), ErrorCode> {
        if iv.len() != symmetric_encryption::AES128_BLOCK_SIZE {
            Err(ErrorCode::INVAL)
        } else {
            for (i, c) in iv.iter().enumerate() {
                unsafe {
                    // ECB_DATA[i + PLAINTEXT_START] = *c;
                }
            }
            Ok(())
        }
    }

    // not needed by NRF5x
    fn start_message(&self) {}

    fn crypt(
        &self,
        source: Option<&'static mut [u8]>,
        dest: &'static mut [u8],
        start_index: usize,
        stop_index: usize,
    ) -> Option<(
        Result<(), ErrorCode>,
        Option<&'static mut [u8]>,
        &'static mut [u8],
    )> {
        self.input.put(source);
        self.output.replace(dest);
        // if self.try_set_indices(start_index, stop_index) {
        //     self.crypt();
        //     None
        // } else {
        //     Some((
        //         Err(ErrorCode::INVAL),
        //         self.input.take(),
        //         self.output.take().unwrap(),
        //     ))
        // }
        None
    }
}

impl kernel::hil::symmetric_encryption::AES128ECB for AesECB<'_> {
    // not needed by NRF5x (the configuration is the same for encryption and decryption)
    fn set_mode_aes128ecb(&self, encrypting: bool) -> Result<(), ErrorCode> {
        if encrypting {
            self.mode.set(AESMode::ECB);
            Ok(())
        } else {
            Err(ErrorCode::INVAL)
        }
    }
}

impl kernel::hil::symmetric_encryption::AES128Ctr for AesECB<'_> {
    // not needed by NRF5x (the configuration is the same for encryption and decryption)
    fn set_mode_aes128ctr(&self, _encrypting: bool) -> Result<(), ErrorCode> {
        self.mode.set(AESMode::CTR);
        Ok(())
    }
}

impl kernel::hil::symmetric_encryption::AES128CBC for AesECB<'_> {
    fn set_mode_aes128cbc(&self, encrypting: bool) -> Result<(), ErrorCode> {
        if encrypting {
            self.mode.set(AESMode::CBC);
            Ok(())
        } else {
            Err(ErrorCode::INVAL)
        }
    }
}

//TODO: replace this placeholder with a proper implementation of the AES system
impl<'a> kernel::hil::symmetric_encryption::AES128CCM<'a> for AesECB<'a> {
    /// Set the client instance which will receive `crypt_done()` callbacks
    fn set_client(&'a self, _client: &'a dyn kernel::hil::symmetric_encryption::CCMClient) {}

    /// Set the key to be used for CCM encryption
    fn set_key(&self, _key: &[u8]) -> Result<(), ErrorCode> {
        Ok(())
    }

    /// Set the nonce (length NONCE_LENGTH) to be used for CCM encryption
    fn set_nonce(&self, _nonce: &[u8]) -> Result<(), ErrorCode> {
        Ok(())
    }

    /// Try to begin the encryption/decryption process
    fn crypt(
        &self,
        _buf: &'static mut [u8],
        _a_off: usize,
        _m_off: usize,
        _m_len: usize,
        _mic_len: usize,
        _confidential: bool,
        _encrypting: bool,
    ) -> Result<(), (ErrorCode, &'static mut [u8])> {
        Ok(())
    }
}
