use crate::{
    board::initialized::Context,
    states::{
        firmware_update::firmware_update, menu::AppMenu, upload_or_store_measurement::upload_stored,
    },
    AppState,
};

pub async fn sync(context: &mut Context) -> AppState {
    if sync_all(context).await {
        AppState::Shutdown
    } else {
        AppState::Menu(AppMenu::Main)
    }
}

/// Uploads the stored measurements and the log records, then looks for a firmware update.
/// Returns whether a new firmware waits for a restart.
pub async fn sync_all(context: &mut Context) -> bool {
    // Starting the radio takes seconds and draws nothing.
    context.display_message("Syncing...").await;

    upload_stored(context).await && firmware_update(context).await
}
