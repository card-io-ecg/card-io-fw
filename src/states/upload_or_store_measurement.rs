use signal_processing::{buffer::Buffer, i24::i24};

use crate::{board::initialized::Board, states::BIG_OBJECTS, AppState};

/// Whether to store the measurement or not. Used instead of a bool to reduce confusion.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum StoreMeasurement {
    Store,
    DontStore,
}

pub async fn upload_or_store_measurement(board: &mut Board, next_state: AppState) -> AppState {
    let ecg = unsafe { BIG_OBJECTS.as_ecg() };

    if try_to_upload(board, &mut ecg.buffer).await == StoreMeasurement::Store {
        try_store_measurement(board, &mut ecg.buffer).await;
    }

    next_state
}

async fn try_to_upload<const N: usize>(
    board: &mut Board,
    buffer: &mut Buffer<i24, N>,
) -> StoreMeasurement {
    if buffer.len() < buffer.capacity() / 2 {
        log::debug!("Buffer is too short to upload.");
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

    log::debug!("Trying to upload measurement");
    // TODO

    StoreMeasurement::DontStore
}

async fn try_store_measurement<const N: usize>(_board: &mut Board, _buffer: &mut Buffer<i24, N>) {
    log::debug!("Trying to store measurement");
    unimplemented!()
}
