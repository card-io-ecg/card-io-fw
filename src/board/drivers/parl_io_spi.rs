use core::ptr;

use embedded_hal::spi::{Error, ErrorKind, ErrorType};
use embedded_hal_async::spi::SpiBus;
use esp_hal::{
    dma::{BurstConfig, DmaDescriptor, DmaError, DmaTxBuffer, Preparation},
    gpio::interconnect::{InputSignal, PeripheralInput, PeripheralOutput},
    parl_io::{
        self, BitPackOrder, ClkOutPin, ConfigError, ParlIo, ParlIoTx, ParlIoTxTransfer, RxCreator,
        SampleEdge, TxConfig, TxTwoBits,
    },
    time::Rate,
    Async,
};

// The C6 TX FIFO drops a 32-bit word only after all of its bytes went out. A transfer that is
// not a multiple of 4 bytes leaves its last word in the FIFO, and the next transfer sends that
// word again. So every transfer is a multiple of 4 bytes.
const WORD: usize = 4;

// Data bytes in one transfer. Each data byte takes 2 bytes on the 2-line bus.
const CHUNK_LEN: usize = 256;

const STAGING_LEN: usize = 2 * CHUNK_LEN;

// One clock of the 2-line bus is a bit pair: MOSI on line 0 (the low bit), CS on line 1 (the high
// bit). `TX_BIT_UNPACK_ORDER = 1` sends the high pair of a byte first.
const CS_HIGH: u8 = 0b10;

// Four clocks with CS high and MOSI low. The SSD1306 ignores SCLK while CS is high.
const PADDING: u8 = CS_HIGH * 0b0101_0101;

// The two bus bytes of each data byte, with CS low: bit 7 goes out first.
static EXPAND: [[u8; 2]; 256] = {
    let mut table = [[0; 2]; 256];
    let mut byte = 0;
    while byte < 256 {
        let mut bits = 0u16;
        let mut bit = 0;
        while bit < 8 {
            if byte & (1 << bit) != 0 {
                bits |= 1 << (2 * bit);
            }
            bit += 1;
        }
        table[byte] = [(bits >> 8) as u8, bits as u8];
        byte += 1;
    }
    table
};

#[derive(Clone, Copy, PartialEq, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum ParlIoSpiError {
    Start(parl_io::Error),
    Dma(DmaError),
}

impl Error for ParlIoSpiError {
    fn kind(&self) -> ErrorKind {
        ErrorKind::Other
    }
}

#[repr(C, align(4))]
struct Staging([u8; STAGING_LEN]);

pub struct ParlIoSpi<'d> {
    tx: TxUnit<'d>,
    staging: Staging,
    _rx: RxCreator<'d, Async>,
    _miso: InputSignal<'d>,
}

impl<'d> ParlIoSpi<'d> {
    pub fn new(
        parl_io: ParlIo<'d, Async>,
        sclk: impl PeripheralOutput<'d>,
        mosi: impl PeripheralOutput<'d>,
        miso: impl PeripheralInput<'d>,
        cs: impl PeripheralOutput<'d>,
        frequency: Rate,
    ) -> Result<Self, ConfigError> {
        let tx = parl_io.tx.with_config(
            TxTwoBits::new(mosi, cs),
            ClkOutPin::new(sclk),
            // esp-hal maps both settings to the opposite hardware behaviour. On TX, the "sample
            // edge" is the edge that shifts out the next bit, and mode 0 shifts on the falling
            // edge. `BitPackOrder::Lsb` writes `TX_BIT_UNPACK_ORDER = 1`, which sends the high
            // bit pair first.
            TxConfig::default()
                .with_frequency(frequency)
                .with_idle_value(CS_HIGH as u16)
                .with_sample_edge(SampleEdge::Invert)
                .with_bit_order(BitPackOrder::Lsb),
        )?;

        Ok(Self {
            tx: TxUnit {
                tx: Some(tx),
                descriptor: DmaDescriptor::EMPTY,
            },
            staging: Staging([0; STAGING_LEN]),
            _rx: parl_io.rx,
            _miso: miso.into(),
        })
    }
}

impl ErrorType for ParlIoSpi<'_> {
    type Error = ParlIoSpiError;
}

impl SpiBus for ParlIoSpi<'_> {
    async fn read(&mut self, _words: &mut [u8]) -> Result<(), Self::Error> {
        todo!()
    }

    async fn write(&mut self, words: &[u8]) -> Result<(), Self::Error> {
        for chunk in words.chunks(CHUNK_LEN) {
            let staging = &mut self.staging.0;
            for (byte, out) in chunk.iter().zip(staging.as_chunks_mut::<2>().0) {
                *out = EXPAND[*byte as usize];
            }
            let len = 2 * chunk.len();
            let padded = len.next_multiple_of(WORD);
            staging[len..padded].fill(PADDING);
            self.tx.send(&staging[..padded]).await?;
        }
        Ok(())
    }

    async fn transfer(&mut self, _read: &mut [u8], _write: &[u8]) -> Result<(), Self::Error> {
        todo!()
    }

    async fn transfer_in_place(&mut self, _words: &mut [u8]) -> Result<(), Self::Error> {
        todo!()
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

struct TxUnit<'d> {
    /// `None` only while a transfer runs.
    tx: Option<ParlIoTx<'d, Async>>,
    descriptor: DmaDescriptor,
}

impl TxUnit<'_> {
    async fn send(&mut self, data: &[u8]) -> Result<(), ParlIoSpiError> {
        let tx = unwrap!(self.tx.take());
        let buffer = TxSlice::new(&mut self.descriptor, data);
        let transfer = match tx.write(data.len(), buffer) {
            Ok(transfer) => transfer,
            Err((error, tx, _)) => {
                self.tx = Some(tx);
                return Err(ParlIoSpiError::Start(error));
            }
        };

        TransferGuard {
            slot: &mut self.tx,
            transfer: Some(transfer),
        }
        .finish()
        .await
        .map_err(ParlIoSpiError::Dma)
    }
}

/// Puts the TX unit back when the transfer ends, also when the write future is dropped.
struct TransferGuard<'s, 'd, 'a> {
    slot: &'s mut Option<ParlIoTx<'d, Async>>,
    transfer: Option<ParlIoTxTransfer<'d, TxSlice<'a>, Async>>,
}

impl TransferGuard<'_, '_, '_> {
    async fn finish(mut self) -> Result<(), DmaError> {
        if let Some(transfer) = &mut self.transfer {
            transfer.wait_for_done().await;
        }
        self.restore()
    }

    fn restore(&mut self) -> Result<(), DmaError> {
        let Some(transfer) = self.transfer.take() else {
            return Ok(());
        };
        // Dropping a `ParlIoTxTransfer` destroys the TX unit, and PARL_IO cannot stop a
        // transfer early. `wait` blocks until the last bit is out.
        let (result, tx, ()) = transfer.wait();
        *self.slot = Some(tx);
        result
    }
}

impl Drop for TransferGuard<'_, '_, '_> {
    fn drop(&mut self) {
        if let Err(error) = self.restore() {
            warn!("PARL_IO SPI transfer failed after cancellation: {}", error);
        }
    }
}

struct TxSlice<'a> {
    descriptor: &'a mut DmaDescriptor,
}

impl<'a> TxSlice<'a> {
    fn new(descriptor: &'a mut DmaDescriptor, data: &'a [u8]) -> Self {
        descriptor.buffer = data.as_ptr().cast_mut();
        descriptor.set_size(data.len());
        descriptor.set_length(data.len());
        descriptor.next = ptr::null_mut();
        descriptor.reset_for_tx(true);
        Self { descriptor }
    }
}

// SAFETY: `TxSlice` borrows the descriptor and the data for `'a`. `TransferGuard` waits for the
// transfer to end before the borrow ends, also when the write future is dropped. A leaked write
// future breaks this.
unsafe impl DmaTxBuffer for TxSlice<'_> {
    type View = Self;
    type Final = ();

    fn prepare(&mut self) -> Preparation {
        Preparation {
            start: ptr::from_mut(self.descriptor),
            burst_transfer: BurstConfig::default(),
            check_owner: None,
            auto_write_back: false,
        }
    }

    fn into_view(self) -> Self {
        self
    }

    fn from_view(_view: Self) {}
}
