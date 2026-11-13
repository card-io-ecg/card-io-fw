use esp_println::Printer;

/// The USB Serial/JTAG port, which `espflash --log-format defmt` reads.
pub struct Transport {
    // `Printer` locks and flushes for each call, and the encoder writes single bytes.
    buffer: [u8; 64],
    len: usize,
}

impl Transport {
    pub const fn new() -> Self {
        Self {
            buffer: [0; 64],
            len: 0,
        }
    }

    pub fn init(&mut self) {}

    pub fn start_frame(&mut self) {
        // espflash finds a defmt frame by this prefix.
        self.write(&[0xFF, 0x00]);
    }

    pub fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            if self.len == self.buffer.len() {
                self.flush();
            }
            self.buffer[self.len] = *byte;
            self.len += 1;
        }
    }

    pub fn end_frame(&mut self) {
        self.flush();
    }

    fn flush(&mut self) {
        Printer::write_bytes(&self.buffer[..self.len]);
        self.len = 0;
    }
}
