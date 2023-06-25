use signal_processing::compressing_buffer::CompressingBuffer;

use crate::{board::initialized::Board, AppState};

/// Whether to store the measurement or not. Used instead of a bool to reduce confusion.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum StoreMeasurement {
    Store,
    DontStore,
}

pub async fn upload_or_store_measurement<const SIZE: usize>(
    board: &mut Board,
    buffer: &mut CompressingBuffer<SIZE>,
    next_state: AppState,
) -> AppState {
    if try_to_upload(board, buffer).await == StoreMeasurement::Store {
        try_store_measurement(board, buffer).await;
    }

    next_state
}

async fn try_to_upload<const SIZE: usize>(
    board: &mut Board,
    buffer: &mut CompressingBuffer<SIZE>,
) -> StoreMeasurement {
    const SAMPLE_RATE: usize = 1000; // samples/sec
    if buffer.len() < 20 * SAMPLE_RATE {
        debug!("Buffer is too short to upload.");
        // We don't want to store too-short measurements.
        return StoreMeasurement::DontStore;
    }

    // TODO: scan/connection shouldn't be here.
    let is_connected = false;
    // If we're not connected, look around for a network to connect to.
    if !is_connected {
        if board.config.known_networks.is_empty() {
            // We don't have networks configured. Best we can do is store the measurement.
            return StoreMeasurement::Store;
        }

        // TODO: scan
        // If we found a network, attempt to upload.
        // TODO: only try to upload if we are registered.
        // If we could not upload, save to file.
    }

    debug!("Trying to upload measurement");
    // TODO

    StoreMeasurement::DontStore
}

async fn try_store_measurement<const SIZE: usize>(
    _board: &mut Board,
    _buffer: &mut CompressingBuffer<SIZE>,
) {
    debug!("Trying to store measurement");
    unimplemented!()
}
